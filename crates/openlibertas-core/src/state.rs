use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub struct State {
    pub last_model: Option<String>,
    pub mcp_enabled: HashMap<String, bool>,
}

impl State {
    pub fn load() -> Self {
        if let Some(path) = Self::state_path() {
            if path.exists() {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(state) = serde_json::from_str(&contents) {
                        return state;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::state_path().context("Could not determine state path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create state directory: {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(self).context("Failed to serialize state")?;
        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, json)
            .with_context(|| format!("Failed to write state temp file: {:?}", temp_path))?;
        std::fs::rename(&temp_path, &path)
            .with_context(|| format!("Failed to rename state file to: {:?}", path))?;
        Ok(())
    }

    pub fn state_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
            .map(|dirs| dirs.data_dir().join("state.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_default_empty() {
        let state = State::default();
        assert!(state.last_model.is_none());
        assert!(state.mcp_enabled.is_empty());
    }

    #[test]
    fn state_roundtrip() {
        let mut state = State {
            last_model: Some("test-model".to_string()),
            ..Default::default()
        };
        state.mcp_enabled.insert("websearch".to_string(), true);

        let json = serde_json::to_string_pretty(&state).unwrap();
        let restored: State = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.last_model, Some("test-model".to_string()));
        assert_eq!(restored.mcp_enabled.get("websearch"), Some(&true));
    }

    #[test]
    fn state_path_returns_some() {
        assert!(State::state_path().is_some());
    }

    #[test]
    fn state_serialization_format() {
        let state = State::default();
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("last_model"));
        assert!(json.contains("mcp_enabled"));
    }

    #[test]
    fn state_with_multiple_mcp_entries() {
        let mut state = State::default();
        state.mcp_enabled.insert("web".to_string(), true);
        state.mcp_enabled.insert("git".to_string(), false);
        assert_eq!(state.mcp_enabled.len(), 2);
        assert_eq!(state.mcp_enabled.get("web"), Some(&true));
        assert_eq!(state.mcp_enabled.get("git"), Some(&false));
    }
}
