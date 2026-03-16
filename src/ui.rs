use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
};

use crate::app::{
    App, CheckStatus, DetailTab, Modal, QueuePreset, Screen, StartupField, format_json,
};

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(frame, app, outer[0]);
    match app.screen {
        Screen::Startup => render_startup(frame, app, outer[1]),
        Screen::Dashboard => render_dashboard(frame, app, outer[1]),
        Screen::Queue => render_queue(frame, app, outer[1]),
        Screen::Groups => render_groups(frame, app, outer[1]),
        Screen::Detail => render_detail(frame, app, outer[1]),
        Screen::Bulk => render_bulk(frame, app, outer[1]),
        Screen::Apply => render_apply(frame, app, outer[1]),
        Screen::Help => render_help(frame, app, outer[1]),
    }
    render_footer(frame, app, outer[2]);

    if let Some(modal) = &app.modal {
        render_modal(frame, modal, area);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(
        " respkit-tui | {} | {} ",
        screen_label(app.screen),
        app.status_line()
    );
    frame.render_widget(
        Paragraph::new(title).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let note = app
        .notifications
        .front()
        .cloned()
        .unwrap_or_else(|| current_key_hint(app.screen));
    frame.render_widget(
        Paragraph::new(note)
            .style(Style::default().fg(Color::Black).bg(Color::Gray))
            .alignment(Alignment::Left),
        area,
    );
}

fn render_startup(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let form_lines = vec![
        field_line(
            "Backend",
            &app.startup.backend_command,
            app.startup.active_field == StartupField::BackendCommand,
        ),
        field_line(
            "Ledger",
            &app.startup.ledger_path,
            app.startup.active_field == StartupField::LedgerPath,
        ),
        field_line(
            "Task",
            &app.startup.task_name,
            app.startup.active_field == StartupField::TaskName,
        ),
        Line::from(""),
        Line::from(
            "Tab/Shift-Tab move fields. Enter launches the backend and opens the dashboard.",
        ),
        Line::from("The backend command may include {ledger} and {task} placeholders."),
    ];
    let block = Block::default().title("Connection").borders(Borders::ALL);
    frame.render_widget(
        Paragraph::new(Text::from(form_lines))
            .block(block)
            .wrap(Wrap { trim: false }),
        chunks[0],
    );

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(chunks[1]);

    let validation = app
        .validation_checks
        .iter()
        .map(|check| {
            ListItem::new(Line::from(vec![
                Span::styled(status_icon(check.status), status_style(check.status)),
                Span::raw(format!(" {}: {}", check.label, check.detail)),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(validation).block(
            Block::default()
                .title("Startup Validation")
                .borders(Borders::ALL),
        ),
        right[0],
    );

    let recent = if app.config.recent_ledgers.is_empty() {
        vec![ListItem::new("No recent ledgers yet")]
    } else {
        app.config
            .recent_ledgers
            .iter()
            .map(|ledger| ListItem::new(ledger.as_str()))
            .collect::<Vec<_>>()
    };
    frame.render_widget(
        List::new(recent).block(
            Block::default()
                .title("Recent Ledgers")
                .borders(Borders::ALL),
        ),
        right[1],
    );
}

fn render_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Min(10),
            Constraint::Length(8),
        ])
        .split(area);

    let counts = app.counts_snapshot();
    let count_text = counts
        .chunks(3)
        .map(|chunk| {
            Line::from(
                chunk
                    .iter()
                    .map(|(label, value)| format!("{:>14}: {:<5}", label, value))
                    .collect::<Vec<_>>()
                    .join("    "),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(count_text))
            .block(Block::default().title("Health Cards").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        rows[0],
    );

    let middle = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(rows[1]);

    let mut summary_lines = Vec::new();
    if let Some(info) = &app.ledger_info {
        summary_lines.push(Line::from(format!("Ledger: {}", info.ledger_path)));
        summary_lines.push(Line::from(format!(
            "Schema: {} | Service: {}",
            info.schema_version, info.service_version
        )));
        summary_lines.push(Line::from(format!(
            "Rows: {} | Tasks: {}",
            info.row_count, info.task_count
        )));
    }
    if !app.tasks.task_names.is_empty() {
        summary_lines.push(Line::from(""));
        summary_lines.push(Line::from(format!(
            "Tasks: {}",
            app.tasks.task_names.join(", ")
        )));
    }
    if !app.tasks.registered_adapters.is_empty() {
        summary_lines.push(Line::from(format!(
            "Adapters: {}",
            app.tasks.registered_adapters.join(", ")
        )));
    }
    summary_lines.push(Line::from(""));
    summary_lines.push(Line::from("Known backend gaps in v1 summary:"));
    summary_lines.push(Line::from("missing files / path drift / collisions / final apply plan are not exposed by the public SDK backend yet."));
    frame.render_widget(
        Paragraph::new(Text::from(summary_lines))
            .block(
                Block::default()
                    .title("Ledger Summary")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        middle[0],
    );

    let groups = app.dashboard_groups();
    let group_lines = groups
        .into_iter()
        .flat_map(|(label, entries)| {
            let mut lines = vec![Line::from(Span::styled(
                label,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))];
            if entries.is_empty() {
                lines.push(Line::from("  no data"));
            } else {
                lines.extend(
                    entries
                        .into_iter()
                        .map(|entry| Line::from(format!("  {:<20} {}", entry.label, entry.count))),
                );
            }
            lines.push(Line::from(""));
            lines
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(group_lines))
            .block(
                Block::default()
                    .title("Grouped Summaries")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        middle[1],
    );

    let validation = app
        .validation_checks
        .iter()
        .map(|check| {
            Line::from(format!(
                "{} {:<20} {}",
                status_icon(check.status),
                check.label,
                check.detail
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(validation))
            .block(
                Block::default()
                    .title("Startup Validation Status")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        rows[2],
    );
}

fn render_queue(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    let visible = app.visible_rows();
    let start = app.queue_selected.saturating_sub(8);
    let end = visible.len().min(start + 16);
    let rows = visible[start..end]
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let selected = start + offset == app.queue_selected;
            let style = if selected {
                Style::default().bg(Color::Blue).fg(Color::Black)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(row.item_id.clone()),
                Cell::from(row.item_locator.clone().unwrap_or_default()),
                Cell::from(row.machine_status.clone()),
                Cell::from(row.human_status.clone()),
                Cell::from(if row.risk_flags.is_empty() {
                    "-".to_string()
                } else {
                    row.risk_flags.join(",")
                }),
            ])
            .style(style)
        })
        .collect::<Vec<_>>();
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Percentage(42),
            Constraint::Length(15),
            Constraint::Length(15),
            Constraint::Length(18),
        ],
    )
    .header(
        Row::new(vec!["item", "locator", "machine", "human", "risk"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .title(format!(
                "Queue | preset={} | filter='{}' | rows={} ",
                app.queue_preset.label(),
                app.queue_filter,
                visible.len()
            ))
            .borders(Borders::ALL),
    );
    frame.render_widget(table, cols[0]);

    let inspector = if let Some(row) = app.selected_row() {
        vec![
            Line::from(format!("task: {}", row.task_name)),
            Line::from(format!("item: {}", row.item_id)),
            Line::from(format!(
                "locator: {}",
                row.item_locator.clone().unwrap_or_default()
            )),
            Line::from(format!("machine: {}", row.machine_status)),
            Line::from(format!("human: {}", row.human_status)),
            Line::from(format!("rerun eligible: {}", row.rerun_eligible)),
            Line::from(format!(
                "risk: {}",
                if row.risk_flags.is_empty() {
                    "-".to_string()
                } else {
                    row.risk_flags.join(", ")
                }
            )),
            Line::from(format!(
                "categories: {}",
                if row.categories.is_empty() {
                    "-".to_string()
                } else {
                    row.categories.join(", ")
                }
            )),
            Line::from(""),
            Line::from(row.rendered_summary.clone()),
        ]
    } else {
        vec![Line::from("No rows match the current queue scope.")]
    };
    frame.render_widget(
        Paragraph::new(Text::from(inspector))
            .block(Block::default().title("Inspector").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        cols[1],
    );
}

fn render_groups(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let tabs = Tabs::new(
        crate::app::GroupDimension::ALL
            .iter()
            .map(|dimension| Line::from(dimension.label()))
            .collect::<Vec<_>>(),
    )
    .select(
        crate::app::GroupDimension::ALL
            .iter()
            .position(|dimension| *dimension == app.group_dimension)
            .unwrap_or(0),
    )
    .block(Block::default().title("Dimensions").borders(Borders::ALL))
    .style(Style::default().fg(Color::White))
    .highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(cols[0]);
    frame.render_widget(tabs, left_chunks[0]);

    let entries = app.group_entries();
    let items = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let style = if index == app.group_selected {
                Style::default().bg(Color::Blue).fg(Color::Black)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(format!("{:>4}  {}", entry.count, entry.label))).style(style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(Block::default().title("Groups").borders(Borders::ALL)),
        left_chunks[1],
    );

    let preview = entries
        .get(app.group_selected)
        .map(|entry| {
            let samples = app
                .all_rows
                .iter()
                .filter(|row| {
                    crate::app::GroupFilter {
                        dimension: app.group_dimension,
                        value: entry.label.clone(),
                    }
                    .matches(row)
                })
                .take(8)
                .map(|row| Line::from(format!("{}  {}", row.item_id, row.rendered_summary)))
                .collect::<Vec<_>>();
            let mut lines = vec![
                Line::from(format!("group: {}", entry.label)),
                Line::from(format!("rows: {}", entry.count)),
                Line::from(""),
            ];
            lines.extend(samples);
            lines
        })
        .unwrap_or_else(|| vec![Line::from("No groups available")]);
    frame.render_widget(
        Paragraph::new(Text::from(preview))
            .block(
                Block::default()
                    .title("Preview / Drill Down")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        cols[1],
    );
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Min(10),
        ])
        .split(area);

    let info = if let Some(row) = &app.detail.row {
        vec![
            Line::from(format!("task: {}", row.task_name)),
            Line::from(format!("item: {}", row.item_id)),
            Line::from(format!(
                "locator: {}",
                row.item_locator.clone().unwrap_or_default()
            )),
            Line::from(format!(
                "machine: {} | human: {} | rerun eligible: {}",
                row.machine_status, row.human_status, row.rerun_eligible
            )),
            Line::from(format!(
                "decision actor: {} | source: {}",
                row.decision_actor
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
                row.decision_source
                    .clone()
                    .unwrap_or_else(|| "-".to_string())
            )),
            Line::from(format!(
                "notes: {}",
                row.human_notes.clone().unwrap_or_else(|| "-".to_string())
            )),
            Line::from(format!("summary: {}", row.rendered_summary)),
        ]
    } else {
        vec![Line::from("Loading row detail...")]
    };
    frame.render_widget(
        Paragraph::new(Text::from(info))
            .block(Block::default().title("Row Detail").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        rows[0],
    );

    let tabs = Tabs::new(
        [DetailTab::Overview, DetailTab::Preview, DetailTab::History]
            .iter()
            .map(|tab| Line::from(tab.label()))
            .collect::<Vec<_>>(),
    )
    .select(match app.detail.tab {
        DetailTab::Overview => 0,
        DetailTab::Preview => 1,
        DetailTab::History => 2,
    })
    .block(Block::default().title("Panels").borders(Borders::ALL))
    .highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(tabs, rows[1]);

    let body = match app.detail.tab {
        DetailTab::Overview => render_overview_panel(app),
        DetailTab::Preview => render_preview_panel(app),
        DetailTab::History => render_history_panel(app),
    };
    frame.render_widget(
        Paragraph::new(body)
            .block(Block::default().title("Payload").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        rows[2],
    );
}

fn render_bulk(frame: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);

    let actions = if app.bulk_actions.is_empty() {
        vec![ListItem::new(
            "No actions available for the current task/scope",
        )]
    } else {
        app.bulk_actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let style = if index == app.bulk_selected {
                    Style::default().bg(Color::Blue).fg(Color::Black)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(format!(
                    "{}{} - {}",
                    action.name,
                    if action.requires_edits {
                        " (edits)"
                    } else {
                        ""
                    },
                    action.description
                )))
                .style(style)
            })
            .collect::<Vec<_>>()
    };
    frame.render_widget(
        List::new(actions).block(
            Block::default()
                .title("Available Actions")
                .borders(Borders::ALL),
        ),
        cols[0],
    );

    let scope_rows = app.visible_rows();
    let scope = vec![
        Line::from(format!(
            "Task: {}",
            app.current_task
                .as_deref()
                .unwrap_or("select a task with 't'")
        )),
        Line::from(format!("Queue preset: {}", app.queue_preset.label())),
        Line::from(format!("Filtered scope size: {}", scope_rows.len())),
        Line::from(""),
        Line::from("Bulk invocations are backend-driven through actions.list/actions.invoke."),
        Line::from(
            "Builtin approve/reject/follow-up actions are surfaced here when the backend advertises them.",
        ),
        Line::from("Actions requiring edits are listed but not executable in v1 bulk mode."),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(scope))
            .block(Block::default().title("Scope").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        cols[1],
    );
}

fn render_apply(frame: &mut Frame, app: &App, area: Rect) {
    let counts = app.counts_snapshot();
    let apply_ready = counts
        .iter()
        .find(|(label, _)| label == "apply_ready")
        .map(|(_, value)| *value)
        .unwrap_or(0);
    let applied = counts
        .iter()
        .find(|(label, _)| label == "applied")
        .map(|(_, value)| *value)
        .unwrap_or(0);
    let body = vec![
        Line::from(format!("apply_ready rows: {}", apply_ready)),
        Line::from(format!("applied rows: {}", applied)),
        Line::from(""),
        Line::from(
            "The public SDK backend does not currently expose a final apply/dry-run plan RPC.",
        ),
        Line::from(
            "This screen is the placeholder for that flow and currently offers quick access to the apply_ready queue.",
        ),
        Line::from("Press Enter to jump to the apply_ready queue."),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(body))
            .block(
                Block::default()
                    .title("Dry Run / Apply")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_help(frame: &mut Frame, _app: &App, area: Rect) {
    let help = vec![
        Line::from("Global"),
        Line::from(
            "  d dashboard | l queue | g groups | b bulk | p apply | t cycle task | r refresh | x export | q quit | ? help",
        ),
        Line::from("Queue"),
        Line::from(
            "  j/k or arrows move | [/ ] preset cycle | / filter | Backspace clear filter | Enter row detail",
        ),
        Line::from("Groups"),
        Line::from("  1-5 dimension | j/k move | Enter drill into matching queue"),
        Line::from("Detail"),
        Line::from(
            "  a approve | e approve with edit JSON | x reject | f follow-up | o/p/h switch panels | Esc back",
        ),
        Line::from("Bulk"),
        Line::from("  j/k move | Enter invoke selected action on current queue scope"),
        Line::from("Startup"),
        Line::from("  Tab move fields | Enter connect"),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(help))
            .block(
                Block::default()
                    .title("Help / Keybindings")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_modal(frame: &mut Frame, modal: &Modal, area: Rect) {
    let popup = centered_rect(70, 45, area);
    frame.render_widget(Clear, popup);
    match modal {
        Modal::TextInput(prompt) => {
            let body = vec![
                Line::from(prompt.hint.clone()),
                Line::from(""),
                Line::from(prompt.value.clone()),
                Line::from(""),
                Line::from("Enter submits. Esc cancels."),
            ];
            frame.render_widget(
                Paragraph::new(Text::from(body))
                    .block(
                        Block::default()
                            .title(prompt.title.as_str())
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Modal::Confirm(confirm) => {
            let body = vec![
                Line::from(confirm.body.clone()),
                Line::from(""),
                Line::from("Enter/y confirms. Esc/n cancels."),
            ];
            frame.render_widget(
                Paragraph::new(Text::from(body))
                    .block(
                        Block::default()
                            .title(confirm.title.as_str())
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
        Modal::Info(info) => {
            frame.render_widget(
                Paragraph::new(info.body.as_str())
                    .block(
                        Block::default()
                            .title(info.title.as_str())
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false }),
                popup,
            );
        }
    }
}

fn render_overview_panel(app: &App) -> Text<'static> {
    if let Some(row) = &app.detail.row {
        let sections = vec![
            format!(
                "proposed output:\n{}",
                row.proposal_payload
                    .as_ref()
                    .map(format_json)
                    .unwrap_or_else(|| "null".to_string())
            ),
            format!(
                "review payload:\n{}",
                row.review_payload
                    .as_ref()
                    .map(format_json)
                    .unwrap_or_else(|| "null".to_string())
            ),
            format!(
                "approved output:\n{}",
                row.approved_output
                    .as_ref()
                    .map(format_json)
                    .unwrap_or_else(|| "null".to_string())
            ),
            format!("extras:\n{}", format_json(&row.extras)),
        ];
        Text::from(sections.join("\n\n"))
    } else {
        Text::from("Waiting for row payload")
    }
}

fn render_preview_panel(app: &App) -> Text<'static> {
    if let Some(preview) = &app.detail.preview {
        Text::from(format_json(preview))
    } else {
        Text::from("Waiting for preview payload")
    }
}

fn render_history_panel(app: &App) -> Text<'static> {
    if app.detail.history.is_empty() {
        return Text::from("Waiting for history payload");
    }
    let lines = app
        .detail
        .history
        .iter()
        .map(|event| {
            format!(
                "v{} {} | {} -> {} @ {}\n{}",
                event.version,
                event.event_type,
                event.machine_status,
                event.human_status,
                event.event_at,
                format_json(&event.payload)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    Text::from(lines)
}

fn field_line(label: &str, value: &str, selected: bool) -> Line<'static> {
    let value = if value.is_empty() { "<empty>" } else { value };
    let style = if selected {
        Style::default().fg(Color::Black).bg(Color::Yellow)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(
            format!("{label:<8}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {value}"), style),
    ])
}

fn status_style(status: CheckStatus) -> Style {
    match status {
        CheckStatus::Pass => Style::default().fg(Color::Green),
        CheckStatus::Warn => Style::default().fg(Color::Yellow),
        CheckStatus::Fail => Style::default().fg(Color::Red),
    }
}

fn status_icon(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "OK",
        CheckStatus::Warn => "??",
        CheckStatus::Fail => "!!",
    }
}

fn screen_label(screen: Screen) -> &'static str {
    match screen {
        Screen::Startup => "startup",
        Screen::Dashboard => "dashboard",
        Screen::Queue => "queue",
        Screen::Groups => "groups",
        Screen::Detail => "detail",
        Screen::Bulk => "bulk",
        Screen::Apply => "apply",
        Screen::Help => "help",
    }
}

fn current_key_hint(screen: Screen) -> String {
    match screen {
        Screen::Startup => "Tab fields | Enter connect | q quit".to_string(),
        Screen::Dashboard => {
            "l queue | g groups | b bulk | p apply | t cycle task | r refresh | ? help".to_string()
        }
        Screen::Queue => format!(
            "preset {} | j/k move | [ ] preset | / filter | Enter detail | x export",
            QueuePreset::Unresolved.label()
        ),
        Screen::Groups => "1-5 dimension | j/k move | Enter drill | c clear drill".to_string(),
        Screen::Detail => {
            "a approve | e approve+edit | x reject | f follow-up | o/p/h panels | Esc back"
                .to_string()
        }
        Screen::Bulk => "j/k move | Enter invoke | t cycle task".to_string(),
        Screen::Apply => "Enter jumps to apply_ready queue".to_string(),
        Screen::Help => "Esc closes help".to_string(),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
