use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::backend::client::{BackendClient, BackendCommand, BackendEvent};
use crate::backend::protocol::{
    ActionDescriptor, ActionsInvokeResult, ActionsListResult, DecisionResult, ExportResult,
    HealthStatus, HistoryResult, LedgerInfo, LedgerSummary, LedgerTasks, PreviewResult,
    RowHistoryEvent, RowResult, RowView, RowsListResult, RpcResponseEnvelope, RpcResponseError,
    ShutdownResult, ValidationResult,
};
use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Startup,
    Dashboard,
    Queue,
    Groups,
    Detail,
    Bulk,
    Apply,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupField {
    BackendCommand,
    LedgerPath,
    TaskName,
}

impl StartupField {
    fn next(self) -> Self {
        match self {
            Self::BackendCommand => Self::LedgerPath,
            Self::LedgerPath => Self::TaskName,
            Self::TaskName => Self::BackendCommand,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::BackendCommand => Self::TaskName,
            Self::LedgerPath => Self::BackendCommand,
            Self::TaskName => Self::LedgerPath,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StartupForm {
    pub backend_command: String,
    pub ledger_path: String,
    pub task_name: String,
    pub active_field: StartupField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuePreset {
    All,
    Unresolved,
    NeedsReview,
    Approved,
    Rejected,
    ProviderError,
    ApplyReady,
    Group,
}

impl QueuePreset {
    pub const ALL: [QueuePreset; 7] = [
        QueuePreset::All,
        QueuePreset::Unresolved,
        QueuePreset::NeedsReview,
        QueuePreset::Approved,
        QueuePreset::Rejected,
        QueuePreset::ProviderError,
        QueuePreset::ApplyReady,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Unresolved => "unresolved",
            Self::NeedsReview => "needs_review",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::ProviderError => "provider_error",
            Self::ApplyReady => "apply_ready",
            Self::Group => "group drill",
        }
    }

    fn cycle(self, direction: isize) -> Self {
        if self == Self::Group {
            return if direction >= 0 {
                Self::All
            } else {
                Self::ApplyReady
            };
        }
        let presets = Self::ALL;
        let index = presets.iter().position(|item| *item == self).unwrap_or(0) as isize;
        let next = (index + direction).rem_euclid(presets.len() as isize) as usize;
        presets[next]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupDimension {
    HumanStatus,
    MachineStatus,
    RiskFlag,
    SourcePrefix,
    Category,
}

impl GroupDimension {
    pub const ALL: [GroupDimension; 5] = [
        GroupDimension::HumanStatus,
        GroupDimension::MachineStatus,
        GroupDimension::RiskFlag,
        GroupDimension::SourcePrefix,
        GroupDimension::Category,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HumanStatus => "human",
            Self::MachineStatus => "machine",
            Self::RiskFlag => "risk",
            Self::SourcePrefix => "source",
            Self::Category => "category",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Overview,
    Preview,
    History,
}

impl DetailTab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Preview => "preview",
            Self::History => "history",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupEntry {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub struct GroupFilter {
    pub dimension: GroupDimension,
    pub value: String,
}

impl GroupFilter {
    pub(crate) fn matches(&self, row: &RowView) -> bool {
        match self.dimension {
            GroupDimension::HumanStatus => row.human_status == self.value,
            GroupDimension::MachineStatus => row.machine_status == self.value,
            GroupDimension::RiskFlag => {
                if self.value == "none" {
                    row.risk_flags.is_empty()
                } else {
                    row.risk_flags.iter().any(|value| value == &self.value)
                }
            }
            GroupDimension::SourcePrefix => source_prefix(row) == self.value,
            GroupDimension::Category => {
                if self.value == "uncategorized" {
                    row.categories.is_empty()
                } else {
                    row.categories.iter().any(|value| value == &self.value)
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DetailState {
    pub task_name: String,
    pub item_id: String,
    pub row: Option<RowView>,
    pub preview: Option<Value>,
    pub history: Vec<RowHistoryEvent>,
    pub tab: DetailTab,
}

impl Default for DetailState {
    fn default() -> Self {
        Self {
            task_name: String::new(),
            item_id: String::new(),
            row: None,
            preview: None,
            history: Vec::new(),
            tab: DetailTab::Overview,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TextInputTarget {
    QueueFilter,
    ApproveEdit { task_name: String, item_id: String },
    ExportPath,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Decision {
        task_name: String,
        item_id: String,
        action: String,
        edits: Option<Value>,
    },
    BulkInvoke {
        task_name: String,
        action: ActionDescriptor,
        item_ids: Vec<String>,
    },
    Export {
        task_name: Option<String>,
        item_ids: Vec<String>,
        output: String,
    },
}

#[derive(Debug, Clone)]
pub struct TextInputModal {
    pub title: String,
    pub value: String,
    pub target: TextInputTarget,
    pub hint: String,
}

#[derive(Debug, Clone)]
pub struct ConfirmModal {
    pub title: String,
    pub body: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub struct InfoModal {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub enum Modal {
    TextInput(TextInputModal),
    Confirm(ConfirmModal),
    Info(InfoModal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct ValidationCheck {
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone)]
enum PendingRequest {
    Health,
    Info,
    Tasks,
    Summary,
    Rows,
    Actions,
    DetailRow {
        task_name: String,
        item_id: String,
    },
    DetailHistory {
        task_name: String,
        item_id: String,
    },
    DetailPreview {
        task_name: String,
        item_id: String,
    },
    ValidateForDecision {
        task_name: String,
        item_id: String,
        action: String,
        edits: Option<Value>,
    },
    ApplyDecision {
        task_name: String,
        item_id: String,
    },
    InvokeAction {
        action: String,
    },
    Export,
    Shutdown,
}

struct BackendConnection {
    client: BackendClient,
    events: Receiver<BackendEvent>,
}

pub struct App {
    pub running: bool,
    pub screen: Screen,
    previous_screen: Screen,
    pub config_path: PathBuf,
    pub config: AppConfig,
    pub startup: StartupForm,
    connection: Option<BackendConnection>,
    pending: HashMap<String, PendingRequest>,
    pub notifications: VecDeque<String>,
    pub validation_checks: Vec<ValidationCheck>,
    pub ledger_info: Option<LedgerInfo>,
    pub health: Option<HealthStatus>,
    pub tasks: LedgerTasks,
    pub summary: LedgerSummary,
    pub all_rows: Vec<RowView>,
    pub current_task: Option<String>,
    pub queue_preset: QueuePreset,
    pub queue_filter: String,
    pub queue_selected: usize,
    pub group_dimension: GroupDimension,
    pub group_selected: usize,
    pub group_drill: Option<GroupFilter>,
    pub detail: DetailState,
    pub bulk_actions: Vec<ActionDescriptor>,
    pub bulk_selected: usize,
    pub modal: Option<Modal>,
}

impl App {
    pub fn new(config_path: PathBuf, config: AppConfig) -> Self {
        let startup = StartupForm {
            backend_command: config.backend_command.clone().unwrap_or_default(),
            ledger_path: config.default_ledger_path.clone().unwrap_or_default(),
            task_name: config.default_task_name.clone().unwrap_or_default(),
            active_field: StartupField::BackendCommand,
        };

        Self {
            running: true,
            screen: Screen::Startup,
            previous_screen: Screen::Dashboard,
            config_path,
            config,
            startup,
            connection: None,
            pending: HashMap::new(),
            notifications: VecDeque::new(),
            validation_checks: Vec::new(),
            ledger_info: None,
            health: None,
            tasks: LedgerTasks::default(),
            summary: LedgerSummary::default(),
            all_rows: Vec::new(),
            current_task: None,
            queue_preset: QueuePreset::Unresolved,
            queue_filter: String::new(),
            queue_selected: 0,
            group_dimension: GroupDimension::HumanStatus,
            group_selected: 0,
            group_drill: None,
            detail: DetailState::default(),
            bulk_actions: Vec::new(),
            bulk_selected: 0,
            modal: None,
        }
    }

    pub fn on_tick(&mut self) {
        let mut drained = Vec::new();
        if let Some(connection) = &self.connection {
            loop {
                match connection.events.try_recv() {
                    Ok(event) => drained.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        drained.push(BackendEvent::Exited(None));
                        break;
                    }
                }
            }
        }
        for event in drained {
            self.handle_backend_event(event);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }

        if self.screen == Screen::Startup {
            return self.handle_startup_key(key);
        }

        if self.screen == Screen::Help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => {
                    self.screen = self.previous_screen;
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Char('?') => {
                self.previous_screen = self.screen;
                self.screen = Screen::Help;
                return Ok(());
            }
            KeyCode::Char('d') => self.screen = Screen::Dashboard,
            KeyCode::Char('l') => self.screen = Screen::Queue,
            KeyCode::Char('g') => self.screen = Screen::Groups,
            KeyCode::Char('b') => {
                self.screen = Screen::Bulk;
                self.refresh_actions_for_scope()?;
            }
            KeyCode::Char('p') => self.screen = Screen::Apply,
            KeyCode::Char('r') => self.refresh_data()?,
            KeyCode::Char('t') => self.cycle_task()?,
            KeyCode::Char('x') => self.open_export_prompt(),
            KeyCode::Esc if self.screen == Screen::Detail => self.screen = Screen::Queue,
            KeyCode::Esc if self.screen == Screen::Groups => {
                self.group_drill = None;
                self.queue_preset = QueuePreset::Unresolved;
            }
            _ => {}
        }

        match self.screen {
            Screen::Dashboard => self.handle_dashboard_key(key),
            Screen::Queue => self.handle_queue_key(key),
            Screen::Groups => self.handle_group_key(key),
            Screen::Detail => self.handle_detail_key(key),
            Screen::Bulk => self.handle_bulk_key(key),
            Screen::Apply => self.handle_apply_key(key),
            Screen::Startup | Screen::Help => Ok(()),
        }
    }

    fn handle_startup_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Tab => self.startup.active_field = self.startup.active_field.next(),
            KeyCode::BackTab => self.startup.active_field = self.startup.active_field.previous(),
            KeyCode::Enter => self.connect()?,
            KeyCode::Backspace => {
                self.current_startup_value_mut().pop();
            }
            KeyCode::Char('q') if key.modifiers.is_empty() => self.running = false,
            KeyCode::Char(value)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.current_startup_value_mut().push(value);
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter => self.screen = Screen::Queue,
            KeyCode::Char('1') => self.queue_preset = QueuePreset::Unresolved,
            KeyCode::Char('2') => self.queue_preset = QueuePreset::Approved,
            KeyCode::Char('3') => self.queue_preset = QueuePreset::Rejected,
            KeyCode::Char('4') => self.queue_preset = QueuePreset::ProviderError,
            _ => {}
        }
        Ok(())
    }

    fn handle_queue_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_queue_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_queue_selection(-1),
            KeyCode::PageDown => self.move_queue_selection(10),
            KeyCode::PageUp => self.move_queue_selection(-10),
            KeyCode::Char('[') => {
                self.queue_preset = self.queue_preset.cycle(-1);
                self.group_drill = None;
                self.clamp_queue_selection();
            }
            KeyCode::Char(']') => {
                self.queue_preset = self.queue_preset.cycle(1);
                self.group_drill = None;
                self.clamp_queue_selection();
            }
            KeyCode::Char('/') => {
                self.modal = Some(Modal::TextInput(TextInputModal {
                    title: "Queue Filter".to_string(),
                    value: self.queue_filter.clone(),
                    target: TextInputTarget::QueueFilter,
                    hint: "Filter by item id, locator, or summary".to_string(),
                }));
            }
            KeyCode::Backspace => {
                self.queue_filter.clear();
                self.clamp_queue_selection();
            }
            KeyCode::Enter => self.open_selected_row()?,
            _ => {}
        }
        Ok(())
    }

    fn handle_group_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('1') => self.group_dimension = GroupDimension::HumanStatus,
            KeyCode::Char('2') => self.group_dimension = GroupDimension::MachineStatus,
            KeyCode::Char('3') => self.group_dimension = GroupDimension::RiskFlag,
            KeyCode::Char('4') => self.group_dimension = GroupDimension::SourcePrefix,
            KeyCode::Char('5') => self.group_dimension = GroupDimension::Category,
            KeyCode::Down | KeyCode::Char('j') => self.move_group_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_group_selection(-1),
            KeyCode::Enter => self.drill_into_group(),
            KeyCode::Char('c') => {
                self.group_drill = None;
                self.queue_preset = QueuePreset::Unresolved;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('o') => self.detail.tab = DetailTab::Overview,
            KeyCode::Char('p') => self.detail.tab = DetailTab::Preview,
            KeyCode::Char('h') => self.detail.tab = DetailTab::History,
            KeyCode::Char('a') => self.confirm_builtin_decision("approve", None),
            KeyCode::Char('x') => self.confirm_builtin_decision("reject", None),
            KeyCode::Char('f') => self.confirm_builtin_decision("needs_review", None),
            KeyCode::Char('e') => {
                self.modal = Some(Modal::TextInput(TextInputModal {
                    title: "Approve With Edit JSON".to_string(),
                    value: "{}".to_string(),
                    target: TextInputTarget::ApproveEdit {
                        task_name: self.detail.task_name.clone(),
                        item_id: self.detail.item_id.clone(),
                    },
                    hint: "Enter JSON edits; backend will validate and derive approved output"
                        .to_string(),
                }));
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_bulk_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_bulk_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_bulk_selection(-1),
            KeyCode::Enter => self.confirm_bulk_action()?,
            _ => {}
        }
        Ok(())
    }

    fn handle_apply_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('l') => {
                self.queue_preset = QueuePreset::ApplyReady;
                self.screen = Screen::Queue;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Result<()> {
        let Some(modal) = self.modal.clone() else {
            return Ok(());
        };

        match modal {
            Modal::TextInput(mut prompt) => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Backspace => {
                    prompt.value.pop();
                    self.modal = Some(Modal::TextInput(prompt));
                }
                KeyCode::Enter => {
                    self.modal = None;
                    self.submit_text_input(prompt)?;
                }
                KeyCode::Char(value)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    prompt.value.push(value);
                    self.modal = Some(Modal::TextInput(prompt));
                }
                _ => self.modal = Some(Modal::TextInput(prompt)),
            },
            Modal::Confirm(confirm) => match key.code {
                KeyCode::Esc | KeyCode::Char('n') => self.modal = None,
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.modal = None;
                    self.submit_confirm(confirm.action)?;
                }
                _ => self.modal = Some(Modal::Confirm(confirm)),
            },
            Modal::Info(_) => match key.code {
                KeyCode::Esc | KeyCode::Enter => self.modal = None,
                _ => {}
            },
        }
        Ok(())
    }

    fn current_startup_value_mut(&mut self) -> &mut String {
        match self.startup.active_field {
            StartupField::BackendCommand => &mut self.startup.backend_command,
            StartupField::LedgerPath => &mut self.startup.ledger_path,
            StartupField::TaskName => &mut self.startup.task_name,
        }
    }

    fn connect(&mut self) -> Result<()> {
        let ledger_path = PathBuf::from(self.startup.ledger_path.trim());
        let task_name = trimmed_option(&self.startup.task_name);
        let command = BackendCommand::new(self.startup.backend_command.trim().to_string());
        let (client, events) = BackendClient::launch(command, &ledger_path, task_name.as_deref())?;

        self.config.backend_command = Some(self.startup.backend_command.clone());
        self.config.default_task_name = task_name.clone();
        self.config
            .record_recent_ledger(self.startup.ledger_path.trim());
        self.config.save(&self.config_path)?;

        self.connection = Some(BackendConnection { client, events });
        self.pending.clear();
        self.current_task = task_name;
        self.screen = Screen::Dashboard;
        self.group_drill = None;
        self.queue_preset = QueuePreset::Unresolved;
        self.push_notification("backend connected");
        self.send_request("ledger.health", json!({}), PendingRequest::Health)?;
        self.send_request("ledger.info", json!({}), PendingRequest::Info)?;
        self.send_request("ledger.tasks", json!({}), PendingRequest::Tasks)?;
        self.refresh_data()?;
        self.update_validation_checks();
        Ok(())
    }

    fn quit(&mut self) -> Result<()> {
        let shutdown_error = if let Some(connection) = &self.connection {
            match connection.client.shutdown() {
                Ok(id) => {
                    self.pending.insert(id, PendingRequest::Shutdown);
                    None
                }
                Err(err) => {
                    let _ = connection.client.kill();
                    Some(err.to_string())
                }
            }
        } else {
            None
        };
        if let Some(err) = shutdown_error {
            self.push_notification(format!("backend shutdown request failed: {err}"));
        }
        self.running = false;
        Ok(())
    }

    pub fn refresh_data(&mut self) -> Result<()> {
        if self.connection.is_none() {
            return Ok(());
        }
        let params = self.current_query_params();
        self.send_request("ledger.summary", params.clone(), PendingRequest::Summary)?;
        let row_params = json!({
            "task_name": self.current_task,
            "include_superseded": true,
            "include_approved": true,
            "with_view": true,
        });
        self.send_request("rows.list", row_params, PendingRequest::Rows)?;
        self.refresh_actions_for_scope()?;
        Ok(())
    }

    fn refresh_actions_for_scope(&mut self) -> Result<()> {
        if self.connection.is_none() {
            return Ok(());
        }
        if let Some(task_name) = &self.current_task {
            self.send_request(
                "actions.list",
                json!({ "task_name": task_name }),
                PendingRequest::Actions,
            )?;
        } else {
            self.bulk_actions.clear();
        }
        Ok(())
    }

    fn cycle_task(&mut self) -> Result<()> {
        if self.tasks.task_names.is_empty() {
            return Ok(());
        }
        let mut tasks = vec![String::new()];
        tasks.extend(self.tasks.task_names.clone());
        let current = self.current_task.clone().unwrap_or_default();
        let index = tasks.iter().position(|task| task == &current).unwrap_or(0);
        let next = (index + 1) % tasks.len();
        self.current_task = trimmed_option(&tasks[next]);
        self.startup.task_name = self.current_task.clone().unwrap_or_default();
        self.refresh_data()?;
        Ok(())
    }

    fn open_selected_row(&mut self) -> Result<()> {
        let Some(row) = self.selected_row().cloned() else {
            return Ok(());
        };
        self.detail = DetailState {
            task_name: row.task_name.clone(),
            item_id: row.item_id.clone(),
            row: Some(row.clone()),
            preview: None,
            history: Vec::new(),
            tab: DetailTab::Overview,
        };
        self.screen = Screen::Detail;
        self.send_request(
            "rows.get",
            json!({ "task_name": row.task_name, "item_id": row.item_id, "with_view": true }),
            PendingRequest::DetailRow {
                task_name: row.task_name.clone(),
                item_id: row.item_id.clone(),
            },
        )?;
        self.send_request(
            "rows.history",
            json!({ "task_name": row.task_name, "item_id": row.item_id }),
            PendingRequest::DetailHistory {
                task_name: row.task_name.clone(),
                item_id: row.item_id.clone(),
            },
        )?;
        self.send_request(
            "rows.preview",
            json!({ "task_name": row.task_name, "item_id": row.item_id }),
            PendingRequest::DetailPreview {
                task_name: row.task_name,
                item_id: row.item_id,
            },
        )?;
        Ok(())
    }

    fn confirm_builtin_decision(&mut self, action: &str, edits: Option<Value>) {
        if self.detail.task_name.is_empty() || self.detail.item_id.is_empty() {
            return;
        }
        let title = format!("Confirm {action}");
        let body = if let Some(row) = &self.detail.row {
            format!(
                "Apply '{action}' to {}:{}?\n\n{}",
                row.task_name, row.item_id, row.rendered_summary
            )
        } else {
            format!(
                "Apply '{action}' to {}:{}?",
                self.detail.task_name, self.detail.item_id
            )
        };
        self.modal = Some(Modal::Confirm(ConfirmModal {
            title,
            body,
            action: ConfirmAction::Decision {
                task_name: self.detail.task_name.clone(),
                item_id: self.detail.item_id.clone(),
                action: action.to_string(),
                edits,
            },
        }));
    }

    fn confirm_bulk_action(&mut self) -> Result<()> {
        let Some(task_name) = self.current_task.clone() else {
            self.modal = Some(Modal::Info(InfoModal {
                title: "Task Required".to_string(),
                body: "Bulk actions require a concrete task selection. Press 't' to cycle tasks first.".to_string(),
            }));
            return Ok(());
        };
        let Some(action) = self.bulk_actions.get(self.bulk_selected).cloned() else {
            return Ok(());
        };
        if action.requires_edits {
            self.modal = Some(Modal::Info(InfoModal {
                title: "Bulk Edits Unsupported".to_string(),
                body: "This action requires edits. v1 only supports bulk invocation for actions without edit payloads.".to_string(),
            }));
            return Ok(());
        }
        let item_ids = self
            .visible_rows()
            .into_iter()
            .map(|row| row.item_id.clone())
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            return Ok(());
        }
        self.modal = Some(Modal::Confirm(ConfirmModal {
            title: format!("Run {}", action.name),
            body: format!(
                "Invoke '{}' on {} rows in the current queue scope?",
                action.name,
                item_ids.len()
            ),
            action: ConfirmAction::BulkInvoke {
                task_name,
                action,
                item_ids,
            },
        }));
        Ok(())
    }

    fn open_export_prompt(&mut self) {
        self.modal = Some(Modal::TextInput(TextInputModal {
            title: "Export Snapshot".to_string(),
            value: "respkit-snapshot.md".to_string(),
            target: TextInputTarget::ExportPath,
            hint: "Write .md, .csv, or .jsonl for the current queue scope".to_string(),
        }));
    }

    fn submit_text_input(&mut self, prompt: TextInputModal) -> Result<()> {
        match prompt.target {
            TextInputTarget::QueueFilter => {
                self.queue_filter = prompt.value.trim().to_string();
                self.clamp_queue_selection();
            }
            TextInputTarget::ApproveEdit { task_name, item_id } => {
                let edits: Value = serde_json::from_str(prompt.value.trim())
                    .context("approve-with-edit input must be valid JSON")?;
                self.send_request(
                    "rows.validate",
                    json!({
                        "task_name": task_name,
                        "item_id": item_id,
                        "edits": edits,
                        "derive_output": true,
                    }),
                    PendingRequest::ValidateForDecision {
                        task_name,
                        item_id,
                        action: "approve_with_edit".to_string(),
                        edits: Some(edits),
                    },
                )?;
            }
            TextInputTarget::ExportPath => {
                let output = prompt.value.trim().to_string();
                if output.is_empty() {
                    return Ok(());
                }
                let item_ids = self
                    .visible_rows()
                    .into_iter()
                    .map(|row| row.item_id.clone())
                    .collect::<Vec<_>>();
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "Confirm Export".to_string(),
                    body: format!("Export {} rows to {}?", item_ids.len(), output),
                    action: ConfirmAction::Export {
                        task_name: self.current_task.clone(),
                        item_ids,
                        output,
                    },
                }));
            }
        }
        Ok(())
    }

    fn submit_confirm(&mut self, action: ConfirmAction) -> Result<()> {
        match action {
            ConfirmAction::Decision {
                task_name,
                item_id,
                action,
                edits,
            } => {
                self.send_request(
                    "rows.decide",
                    json!({
                        "task_name": task_name,
                        "item_id": item_id,
                        "action": action,
                        "edits": edits,
                        "apply": true,
                    }),
                    PendingRequest::ApplyDecision { task_name, item_id },
                )?;
            }
            ConfirmAction::BulkInvoke {
                task_name,
                action,
                item_ids,
            } => {
                self.send_request(
                    "actions.invoke",
                    json!({
                        "task_name": task_name,
                        "action": action.name,
                        "item_ids": item_ids,
                        "apply": action.builtin,
                    }),
                    PendingRequest::InvokeAction {
                        action: action.name,
                    },
                )?;
            }
            ConfirmAction::Export {
                task_name,
                item_ids,
                output,
            } => {
                let format = export_format_from_path(&output);
                self.send_request(
                    "export",
                    json!({
                        "task_name": task_name,
                        "item_ids": item_ids,
                        "format": format,
                        "output": output,
                        "include_superseded": true,
                        "include_approved": true,
                    }),
                    PendingRequest::Export,
                )?;
            }
        }
        Ok(())
    }

    fn move_queue_selection(&mut self, delta: isize) {
        let len = self.visible_rows().len() as isize;
        if len == 0 {
            self.queue_selected = 0;
            return;
        }
        let next = (self.queue_selected as isize + delta).clamp(0, len - 1);
        self.queue_selected = next as usize;
    }

    fn move_group_selection(&mut self, delta: isize) {
        let len = self.group_entries().len() as isize;
        if len == 0 {
            self.group_selected = 0;
            return;
        }
        let next = (self.group_selected as isize + delta).clamp(0, len - 1);
        self.group_selected = next as usize;
    }

    fn move_bulk_selection(&mut self, delta: isize) {
        let len = self.bulk_actions.len() as isize;
        if len == 0 {
            self.bulk_selected = 0;
            return;
        }
        let next = (self.bulk_selected as isize + delta).clamp(0, len - 1);
        self.bulk_selected = next as usize;
    }

    fn drill_into_group(&mut self) {
        let Some(entry) = self.group_entries().get(self.group_selected).cloned() else {
            return;
        };
        self.group_drill = Some(GroupFilter {
            dimension: self.group_dimension,
            value: entry.label,
        });
        self.queue_preset = QueuePreset::Group;
        self.queue_selected = 0;
        self.screen = Screen::Queue;
    }

    fn clamp_queue_selection(&mut self) {
        let len = self.visible_rows().len();
        if len == 0 {
            self.queue_selected = 0;
        } else if self.queue_selected >= len {
            self.queue_selected = len.saturating_sub(1);
        }
    }

    pub fn selected_row(&self) -> Option<&RowView> {
        self.visible_rows().get(self.queue_selected).copied()
    }

    pub fn visible_rows(&self) -> Vec<&RowView> {
        let needle = self.queue_filter.to_lowercase();
        self.all_rows
            .iter()
            .filter(|row| self.matches_queue_preset(row))
            .filter(|row| {
                self.group_drill
                    .as_ref()
                    .map(|filter| filter.matches(row))
                    .unwrap_or(true)
            })
            .filter(|row| {
                if needle.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {}",
                    row.item_id,
                    row.item_locator.clone().unwrap_or_default(),
                    row.rendered_summary
                )
                .to_lowercase();
                haystack.contains(&needle)
            })
            .collect()
    }

    fn matches_queue_preset(&self, row: &RowView) -> bool {
        match self.queue_preset {
            QueuePreset::All | QueuePreset::Group => true,
            QueuePreset::Unresolved => {
                !matches!(row.machine_status.as_str(), "applied" | "superseded")
            }
            QueuePreset::NeedsReview => row.human_status == "needs_review",
            QueuePreset::Approved => row.human_status == "approved",
            QueuePreset::Rejected => row.human_status == "rejected",
            QueuePreset::ProviderError => row.machine_status == "provider_error",
            QueuePreset::ApplyReady => row.machine_status == "apply_ready",
        }
    }

    pub fn group_entries(&self) -> Vec<GroupEntry> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for row in &self.all_rows {
            match self.group_dimension {
                GroupDimension::HumanStatus => {
                    *counts.entry(row.human_status.clone()).or_default() += 1;
                }
                GroupDimension::MachineStatus => {
                    *counts.entry(row.machine_status.clone()).or_default() += 1;
                }
                GroupDimension::RiskFlag => {
                    if row.risk_flags.is_empty() {
                        *counts.entry("none".to_string()).or_default() += 1;
                    } else {
                        for flag in &row.risk_flags {
                            *counts.entry(flag.clone()).or_default() += 1;
                        }
                    }
                }
                GroupDimension::SourcePrefix => {
                    *counts.entry(source_prefix(row)).or_default() += 1;
                }
                GroupDimension::Category => {
                    if row.categories.is_empty() {
                        *counts.entry("uncategorized".to_string()).or_default() += 1;
                    } else {
                        for category in &row.categories {
                            *counts.entry(category.clone()).or_default() += 1;
                        }
                    }
                }
            }
        }
        let mut entries = counts
            .into_iter()
            .map(|(label, count)| GroupEntry { label, count })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
        });
        entries
    }

    pub fn dashboard_groups(&self) -> Vec<(String, Vec<GroupEntry>)> {
        [
            GroupDimension::HumanStatus,
            GroupDimension::MachineStatus,
            GroupDimension::RiskFlag,
            GroupDimension::SourcePrefix,
            GroupDimension::Category,
        ]
        .into_iter()
        .map(|dimension| {
            let mut clone = self.clone_for_groups();
            clone.group_dimension = dimension;
            let mut entries = clone.group_entries();
            entries.truncate(5);
            (dimension.label().to_string(), entries)
        })
        .collect()
    }

    fn clone_for_groups(&self) -> Self {
        Self {
            running: self.running,
            screen: self.screen,
            previous_screen: self.previous_screen,
            config_path: self.config_path.clone(),
            config: self.config.clone(),
            startup: self.startup.clone(),
            connection: None,
            pending: HashMap::new(),
            notifications: self.notifications.clone(),
            validation_checks: self.validation_checks.clone(),
            ledger_info: self.ledger_info.clone(),
            health: self.health.clone(),
            tasks: self.tasks.clone(),
            summary: self.summary.clone(),
            all_rows: self.all_rows.clone(),
            current_task: self.current_task.clone(),
            queue_preset: self.queue_preset,
            queue_filter: self.queue_filter.clone(),
            queue_selected: self.queue_selected,
            group_dimension: self.group_dimension,
            group_selected: self.group_selected,
            group_drill: self.group_drill.clone(),
            detail: self.detail.clone(),
            bulk_actions: self.bulk_actions.clone(),
            bulk_selected: self.bulk_selected,
            modal: self.modal.clone(),
        }
    }

    pub fn counts_snapshot(&self) -> Vec<(String, usize)> {
        let preferred = [
            "total",
            "unresolved",
            "approved",
            "needs_review",
            "rejected",
            "provider_error",
            "not_run",
            "reviewed",
            "apply_ready",
            "applied",
            "superseded",
        ];
        let mut ordered = preferred
            .iter()
            .map(|key| {
                (
                    key.to_string(),
                    *self.summary.counts.get(*key).unwrap_or(&0),
                )
            })
            .collect::<Vec<_>>();
        for (key, value) in &self.summary.counts {
            if !preferred.iter().any(|known| known == key) {
                ordered.push((key.clone(), *value));
            }
        }
        ordered
    }

    pub fn status_line(&self) -> String {
        let task = self.current_task.as_deref().unwrap_or("all tasks");
        let connection = if let Some(connection) = &self.connection {
            format!("connected via {}", connection.client.command_line())
        } else {
            "disconnected".to_string()
        };
        let pending = if self.pending.is_empty() {
            String::from("idle")
        } else {
            format!("{} pending", self.pending.len())
        };
        format!("task: {task} | {connection} | {pending}")
    }

    fn handle_backend_event(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::Response(response) => self.handle_response(response),
            BackendEvent::Log(line) => self.push_notification(format!("backend: {line}")),
            BackendEvent::Exited(code) => {
                self.connection = None;
                self.pending.clear();
                self.push_notification(format!(
                    "backend exited ({})",
                    code.map(|value| value.to_string())
                        .unwrap_or_else(|| "signal".to_string())
                ));
                if self.screen != Screen::Startup {
                    self.screen = Screen::Startup;
                }
            }
        }
    }

    fn handle_response(&mut self, response: RpcResponseEnvelope) {
        let request_id = response.id_string();
        let Some(pending) = self.pending.remove(&request_id) else {
            return;
        };
        let outcome = match pending {
            PendingRequest::Health => self.decode_response::<HealthStatus>(response).map(|payload| {
                self.health = Some(payload);
                self.update_validation_checks();
            }),
            PendingRequest::Info => self.decode_response::<LedgerInfo>(response).map(|payload| {
                self.ledger_info = Some(payload);
                self.update_validation_checks();
            }),
            PendingRequest::Tasks => self.decode_response::<LedgerTasks>(response).map(|payload| {
                self.tasks = payload;
                if self.current_task.is_none() && self.tasks.task_names.len() == 1 {
                    self.current_task = self.tasks.task_names.first().cloned();
                    self.startup.task_name = self.current_task.clone().unwrap_or_default();
                    let _ = self.refresh_data();
                }
                self.update_validation_checks();
            }),
            PendingRequest::Summary => self.decode_response::<LedgerSummary>(response).map(|payload| {
                self.summary = payload;
                self.update_validation_checks();
            }),
            PendingRequest::Rows => self.decode_response::<RowsListResult>(response).map(|payload| {
                self.all_rows = payload.rows;
                self.clamp_queue_selection();
            }),
            PendingRequest::Actions => self.decode_response::<ActionsListResult>(response).map(|payload| {
                self.bulk_actions = payload.actions;
                self.bulk_selected = 0;
            }),
            PendingRequest::DetailRow { task_name, item_id } => self.decode_response::<RowResult>(response).map(|payload| {
                if self.detail.task_name == task_name && self.detail.item_id == item_id {
                    self.detail.row = Some(payload.row);
                }
            }),
            PendingRequest::DetailHistory { task_name, item_id } => self.decode_response::<HistoryResult>(response).map(|payload| {
                if self.detail.task_name == task_name && self.detail.item_id == item_id {
                    self.detail.history = payload.events;
                }
            }),
            PendingRequest::DetailPreview { task_name, item_id } => self.decode_response::<PreviewResult>(response).map(|payload| {
                if self.detail.task_name == task_name && self.detail.item_id == item_id {
                    self.detail.preview = Some(payload.preview);
                }
            }),
            PendingRequest::ValidateForDecision {
                task_name,
                item_id,
                action,
                edits,
            } => self.decode_response::<ValidationResult>(response).map(|payload| {
                if !payload.valid {
                    self.modal = Some(Modal::Info(InfoModal {
                        title: "Validation Failed".to_string(),
                        body: payload.errors.join("\n"),
                    }));
                } else {
                    let derived = payload
                        .approved_output
                        .as_ref()
                        .map(format_json)
                        .unwrap_or_else(|| "no approved output preview returned".to_string());
                    self.modal = Some(Modal::Confirm(ConfirmModal {
                        title: "Confirm Approve With Edit".to_string(),
                        body: format!(
                            "Validation passed for {task_name}:{item_id}.\n\nDerived approved output:\n{derived}"
                        ),
                        action: ConfirmAction::Decision {
                            task_name,
                            item_id,
                            action,
                            edits,
                        },
                    }));
                }
            }),
            PendingRequest::ApplyDecision { task_name, item_id } => self.decode_response::<DecisionResult>(response).map(|payload| {
                self.push_notification(format!(
                    "{}:{} -> {}",
                    task_name,
                    item_id,
                    payload.status
                ));
                let _ = self.refresh_data();
                if self.detail.task_name == task_name && self.detail.item_id == item_id {
                    let _ = self.open_selected_row();
                }
            }),
            PendingRequest::InvokeAction { action } => self.decode_response::<ActionsInvokeResult>(response).map(|payload| {
                let successes = payload.results.iter().filter(|result| result.status == "ok" || result.status == "applied").count();
                self.push_notification(format!("{} completed on {successes} rows", action));
                let _ = self.refresh_data();
            }),
            PendingRequest::Export => self.decode_response::<ExportResult>(response).map(|payload| {
                let location = payload.output.unwrap_or_else(|| "inline data".to_string());
                self.push_notification(format!("export wrote {location}"));
            }),
            PendingRequest::Shutdown => self.decode_response::<ShutdownResult>(response).map(|payload| {
                self.push_notification(format!("backend {}", payload.status));
            }),
        };
        if let Err(err) = outcome {
            self.push_notification(format!("request failed: {err}"));
        }
    }

    fn decode_response<T>(&self, response: RpcResponseEnvelope) -> Result<T>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        response
            .into_result::<T>()
            .map_err(|err| anyhow::anyhow!(format_rpc_error(err)))
    }

    fn send_request(&mut self, method: &str, params: Value, pending: PendingRequest) -> Result<()> {
        let Some(connection) = &self.connection else {
            return Ok(());
        };
        let id = connection.client.request(method, params)?;
        self.pending.insert(id, pending);
        Ok(())
    }

    fn current_query_params(&self) -> Value {
        json!({
            "task_name": self.current_task,
            "include_superseded": true,
            "include_approved": true,
        })
    }

    fn push_notification(&mut self, message: impl Into<String>) {
        self.notifications.push_front(message.into());
        while self.notifications.len() > 8 {
            self.notifications.pop_back();
        }
    }

    fn update_validation_checks(&mut self) {
        let ledger_path = self.startup.ledger_path.trim();
        let mut checks = Vec::new();
        checks.push(local_file_check(ledger_path));

        checks.push(match &self.health {
            Some(health) if health.status == "ok" => ValidationCheck {
                label: "backend reachable".to_string(),
                status: CheckStatus::Pass,
                detail: health.ledger_path.clone(),
            },
            Some(health) => ValidationCheck {
                label: "backend reachable".to_string(),
                status: CheckStatus::Warn,
                detail: health.status.clone(),
            },
            None => ValidationCheck {
                label: "backend reachable".to_string(),
                status: CheckStatus::Warn,
                detail: "waiting for response".to_string(),
            },
        });

        checks.push(match &self.ledger_info {
            Some(info) if info.schema_version == "1.0" => ValidationCheck {
                label: "schema/version".to_string(),
                status: CheckStatus::Pass,
                detail: format!(
                    "schema {} service {}",
                    info.schema_version, info.service_version
                ),
            },
            Some(info) => ValidationCheck {
                label: "schema/version".to_string(),
                status: CheckStatus::Warn,
                detail: format!("reported schema {}", info.schema_version),
            },
            None => ValidationCheck {
                label: "schema/version".to_string(),
                status: CheckStatus::Warn,
                detail: "waiting for ledger.info".to_string(),
            },
        });

        checks.push(
            if let Some(task_name) = trimmed_option(&self.startup.task_name) {
                let status = if self.tasks.task_names.is_empty()
                    || self.tasks.task_names.contains(&task_name)
                {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warn
                };
                ValidationCheck {
                    label: "task selection".to_string(),
                    status,
                    detail: task_name,
                }
            } else {
                ValidationCheck {
                    label: "task selection".to_string(),
                    status: CheckStatus::Warn,
                    detail: "all tasks / not specified".to_string(),
                }
            },
        );

        checks.push(ValidationCheck {
            label: "registered adapters".to_string(),
            status: if self.tasks.registered_adapters.is_empty() {
                CheckStatus::Warn
            } else {
                CheckStatus::Pass
            },
            detail: if self.tasks.registered_adapters.is_empty() {
                "no adapter metadata reported".to_string()
            } else {
                self.tasks.registered_adapters.join(", ")
            },
        });

        checks.push(ValidationCheck {
            label: "unresolved count".to_string(),
            status: CheckStatus::Pass,
            detail: self
                .summary
                .counts
                .get("unresolved")
                .copied()
                .unwrap_or_default()
                .to_string(),
        });

        for label in [
            "missing files",
            "path drift",
            "duplicate collisions",
            "validation issues",
        ] {
            checks.push(ValidationCheck {
                label: label.to_string(),
                status: CheckStatus::Warn,
                detail: "not exposed by current public backend".to_string(),
            });
        }

        self.validation_checks = checks;
    }
}

fn trimmed_option(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn local_file_check(path: &str) -> ValidationCheck {
    match fs::metadata(Path::new(path)) {
        Ok(metadata) if metadata.is_file() => ValidationCheck {
            label: "ledger exists/readable".to_string(),
            status: CheckStatus::Pass,
            detail: path.to_string(),
        },
        Ok(_) => ValidationCheck {
            label: "ledger exists/readable".to_string(),
            status: CheckStatus::Warn,
            detail: format!("{} exists but is not a regular file", path),
        },
        Err(err) => ValidationCheck {
            label: "ledger exists/readable".to_string(),
            status: CheckStatus::Warn,
            detail: err.to_string(),
        },
    }
}

fn source_prefix(row: &RowView) -> String {
    let candidate = row
        .item_locator
        .as_deref()
        .or(Some(row.item_id.as_str()))
        .unwrap_or_default();
    candidate
        .split(['/', ':'])
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or("unknown")
        .to_string()
}

pub fn format_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn format_rpc_error(error: RpcResponseError) -> String {
    match error {
        RpcResponseError::Service(payload) => format!("rpc {}: {}", payload.code, payload.message),
        other => other.to_string(),
    }
}

fn export_format_from_path(path: &str) -> &'static str {
    if path.ends_with(".jsonl") {
        "jsonl"
    } else if path.ends_with(".csv") {
        "csv"
    } else {
        "markdown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn sample_row(item_id: &str, machine_status: &str, human_status: &str) -> RowView {
        RowView {
            task_name: "task-a".to_string(),
            item_id: item_id.to_string(),
            item_locator: Some(format!("alpha/{item_id}.txt")),
            machine_status: machine_status.to_string(),
            human_status: human_status.to_string(),
            rendered_summary: format!("summary for {item_id}"),
            ..RowView::default()
        }
    }

    #[test]
    fn queue_presets_filter_rows() {
        let mut app = App::new(PathBuf::from("config.toml"), AppConfig::default());
        app.all_rows = vec![
            sample_row("a", "not_run", "needs_review"),
            sample_row("b", "apply_ready", "approved"),
            sample_row("c", "provider_error", "needs_review"),
        ];
        app.queue_preset = QueuePreset::ApplyReady;
        assert_eq!(app.visible_rows().len(), 1);
        app.queue_preset = QueuePreset::ProviderError;
        assert_eq!(app.visible_rows()[0].item_id, "c");
    }

    #[test]
    fn group_drill_matches_selected_dimension() {
        let mut app = App::new(PathBuf::from("config.toml"), AppConfig::default());
        let mut row = sample_row("a", "not_run", "needs_review");
        row.categories = vec!["adapter:fast-lane".to_string()];
        app.all_rows = vec![row];
        app.group_drill = Some(GroupFilter {
            dimension: GroupDimension::Category,
            value: "adapter:fast-lane".to_string(),
        });
        app.queue_preset = QueuePreset::Group;
        assert_eq!(app.visible_rows().len(), 1);
    }

    #[test]
    fn export_format_is_inferred_from_extension() {
        assert_eq!(export_format_from_path("report.md"), "markdown");
        assert_eq!(export_format_from_path("report.csv"), "csv");
        assert_eq!(export_format_from_path("report.jsonl"), "jsonl");
    }
}
