use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_SDK_PATH: &str = "/home/emmy/git/respkit";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub backend_command: Option<String>,
    pub default_ledger_path: Option<String>,
    pub default_task_name: Option<String>,
    #[serde(default)]
    pub recent_ledgers: Vec<String>,
}

impl AppConfig {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default_for_environment());
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config {}", path.display()))?;
        let mut config: Self = toml::from_str(&text).context("failed to parse config toml")?;
        config.normalize();
        if config.backend_command.is_none() {
            config.backend_command = Self::default_for_environment().backend_command;
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("failed to serialize config")?;
        fs::write(path, text).with_context(|| format!("failed to write config {}", path.display()))
    }

    pub fn normalize(&mut self) {
        self.recent_ledgers.retain(|value| !value.trim().is_empty());
        let mut unique = Vec::new();
        for ledger in self.recent_ledgers.drain(..) {
            if !unique.contains(&ledger) {
                unique.push(ledger);
            }
        }
        self.recent_ledgers = unique;
    }

    pub fn record_recent_ledger(&mut self, ledger: &str) {
        let ledger = ledger.trim();
        if ledger.is_empty() {
            return;
        }
        self.recent_ledgers.retain(|existing| existing != ledger);
        self.recent_ledgers.insert(0, ledger.to_string());
        self.recent_ledgers.truncate(8);
        self.default_ledger_path = Some(ledger.to_string());
    }

    pub fn default_for_environment() -> Self {
        let backend_command = if Path::new(DEFAULT_SDK_PATH).exists() {
            Some(format!(
                "PYTHONPATH={sdk} python -m respkit.service.backend --ledger {{ledger}} --stdio",
                sdk = DEFAULT_SDK_PATH
            ))
        } else {
            Some("respkit-ledger-service --ledger {ledger} --stdio".to_string())
        };

        Self {
            backend_command,
            default_ledger_path: None,
            default_task_name: None,
            recent_ledgers: Vec::new(),
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".config"));
    base.join("respkit-tui").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recent_ledgers_are_deduplicated_and_bounded() {
        let mut config = AppConfig::default();
        for ledger in [
            "a.sqlite", "b.sqlite", "a.sqlite", "c.sqlite", "d.sqlite", "e.sqlite", "f.sqlite",
            "g.sqlite", "h.sqlite", "i.sqlite",
        ] {
            config.record_recent_ledger(ledger);
        }
        assert_eq!(config.recent_ledgers[0], "i.sqlite");
        assert_eq!(config.recent_ledgers.len(), 8);
        assert_eq!(
            config
                .recent_ledgers
                .iter()
                .filter(|item| item.as_str() == "a.sqlite")
                .count(),
            1
        );
    }

    #[test]
    fn config_round_trip_preserves_fields() {
        let dir = tempdir().expect("tempdir should work");
        let path = dir.path().join("config.toml");
        let mut config = AppConfig::default_for_environment();
        config.default_ledger_path = Some("ledger.sqlite".to_string());
        config.default_task_name = Some("task-a".to_string());
        config.record_recent_ledger("ledger.sqlite");
        config.save(&path).expect("config save should work");
        let loaded = AppConfig::load_or_default(&path).expect("config should load");
        assert_eq!(loaded.default_task_name.as_deref(), Some("task-a"));
        assert_eq!(loaded.recent_ledgers[0], "ledger.sqlite");
    }
}
