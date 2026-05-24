use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::domain::Message;
use crate::domain::Role;
use crate::export::{self, ExportFormat};

pub use crate::session::SessionManager;

#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Session {
    pub id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_point: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub message_count: usize,
    pub preview: String,
    pub parent_id: Option<String>,
    pub branch_point: Option<usize>,
    pub branches: Vec<String>,
}

#[derive(Clone)]
pub struct SessionStore {
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory: {data_dir:?}"))?;
        Ok(Self { data_dir })
    }

    pub fn generate_name(model: &str) -> String {
        let now = time::OffsetDateTime::now_utc();
        let model_clean = model
            .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
            .replace("--", "-");
        format!(
            "{}-{:04}-{:02}-{:02}-{:02}{:02}",
            model_clean,
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute()
        )
    }

    fn derive_title(messages: &[Message]) -> Option<String> {
        let first_user = messages.iter().find(|m| m.role == Role::User)?;
        let content = first_user.content.trim();
        if content.is_empty() || content.eq_ignore_ascii_case("untitled") {
            return None;
        }
        let title = if content.len() > 40 {
            format!("{}...", &content[..40])
        } else {
            content.to_string()
        };
        Some(title)
    }

    pub fn save_markdown(&self, id: &str, model: Option<&str>, messages: &[Message]) -> Result<()> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let mut md = String::new();
        md.push_str("# Chat Session\n\n");
        if let Some(m) = model {
            md.push_str(&format!("**Model:** {m}\n\n"));
        }
        md.push_str(&format!("**Date:** {now}\n\n"));
        md.push_str("---\n\n");
        md.push_str(&export::export_messages(
            messages,
            model,
            ExportFormat::Markdown,
        ));

        let path = self.data_dir.join(id);
        fs::write(&path, md).with_context(|| format!("Failed to write markdown file: {path:?}"))?;
        Ok(())
    }

    pub fn save(&self, id: &str, model: Option<&str>, messages: &[Message]) -> Result<()> {
        let path = self.session_path(id);
        let existing = if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<Session>(&s).ok())
        } else {
            None
        };

        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let title = Self::derive_title(messages);

        let session = Session {
            id: id.to_string(),
            title,
            model: model.map(std::string::ToString::to_string),
            created_at: existing
                .as_ref()
                .map(|e| e.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: Some(now),
            messages: messages.to_vec(),
            parent_id: existing.as_ref().and_then(|e| e.parent_id.clone()),
            branch_point: existing.as_ref().and_then(|e| e.branch_point),
            branches: existing
                .as_ref()
                .map(|e| e.branches.clone())
                .unwrap_or_default(),
        };

        let temp_path = self.data_dir.join(format!("{id}.tmp"));
        let final_path = self.session_path(id);

        let json = serde_json::to_string_pretty(&session).context("Failed to serialize session")?;
        fs::write(&temp_path, json)
            .with_context(|| format!("Failed to write temp file: {temp_path:?}"))?;
        fs::rename(&temp_path, &final_path)
            .with_context(|| format!("Failed to rename temp file to: {final_path:?}"))?;

        Ok(())
    }

    pub fn save_branch(
        &self,
        id: &str,
        model: Option<&str>,
        messages: &[Message],
        parent_id: Option<&str>,
        branch_point: Option<usize>,
    ) -> Result<()> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let title = Self::derive_title(messages);

        let session = Session {
            id: id.to_string(),
            title,
            model: model.map(std::string::ToString::to_string),
            created_at: now.clone(),
            updated_at: Some(now),
            messages: messages.to_vec(),
            parent_id: parent_id.map(std::string::ToString::to_string),
            branch_point,
            branches: Vec::new(),
        };

        let temp_path = self.data_dir.join(format!("{id}.tmp"));
        let final_path = self.session_path(id);

        let json =
            serde_json::to_string_pretty(&session).context("Failed to serialize branch session")?;
        fs::write(&temp_path, json)
            .with_context(|| format!("Failed to write temp file: {temp_path:?}"))?;
        fs::rename(&temp_path, &final_path)
            .with_context(|| format!("Failed to rename temp file to: {final_path:?}"))?;

        Ok(())
    }

    pub fn add_branch(&self, parent_id: &str, branch_id: &str) -> Result<()> {
        let path = self.session_path(parent_id);
        if !path.exists() {
            return Ok(());
        }
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read parent session: {path:?}"))?;
        let mut session: Session = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse parent session: {path:?}"))?;

        if !session.branches.contains(&branch_id.to_string()) {
            session.branches.push(branch_id.to_string());
            let json = serde_json::to_string_pretty(&session)
                .context("Failed to serialize parent session")?;
            fs::write(&path, json)
                .with_context(|| format!("Failed to write parent session: {path:?}"))?;
        }
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Vec<Message>> {
        let path = self.session_path(id);
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read session: {path:?}"))?;
        let session: Session = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse session: {path:?}"))?;
        Ok(session.messages)
    }

    pub fn list_with_meta(&self) -> Result<Vec<SessionMeta>> {
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&self.data_dir)
            .with_context(|| format!("Failed to read data directory: {:?}", self.data_dir))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some(stem) = path.file_stem() {
                    let id = stem.to_string_lossy().to_string();
                    if let Ok(contents) = fs::read_to_string(&path) {
                        if let Ok(conv) = serde_json::from_str::<Session>(&contents) {
                            let message_count = conv.messages.len();
                            let preview = conv
                                .messages
                                .iter()
                                .rev()
                                .find(|m| m.role != Role::System)
                                .map(|m| {
                                    let content = m.content.trim();
                                    if content.len() > 100 {
                                        format!("{}...", &content[..100])
                                    } else {
                                        content.to_string()
                                    }
                                })
                                .unwrap_or_default();
                            sessions.push(SessionMeta {
                                id,
                                title: conv.title,
                                model: conv.model,
                                created_at: conv.created_at,
                                updated_at: conv.updated_at,
                                message_count,
                                preview,
                                parent_id: conv.parent_id,
                                branch_point: conv.branch_point,
                                branches: conv.branches,
                            });
                        }
                    }
                }
            }
        }
        sessions.sort_by(|a, b| {
            let a_time = a.updated_at.as_ref().unwrap_or(&a.created_at);
            let b_time = b.updated_at.as_ref().unwrap_or(&b.created_at);
            b_time.cmp(a_time)
        });
        Ok(sessions)
    }

    pub fn list(&self) -> Result<Vec<String>> {
        self.list_with_meta()
            .map(|v| v.into_iter().map(|meta| meta.id).collect())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.session_path(id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete session: {path:?}"))?;
        }
        Ok(())
    }

    pub fn session_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{id}.json"))
    }
}

pub fn format_relative_time(iso_str: &str) -> String {
    use time::OffsetDateTime;
    let now = OffsetDateTime::now_utc();
    let parsed =
        OffsetDateTime::parse(iso_str, &time::format_description::well_known::Rfc3339).ok();

    if let Some(dt) = parsed {
        let duration = now - dt;
        let seconds = duration.whole_seconds();
        if seconds < 60 {
            "just now".to_string()
        } else if seconds < 3600 {
            format!("{}m ago", seconds / 60)
        } else if seconds < 86400 {
            format!("{}h ago", seconds / 3600)
        } else if seconds < 604800 {
            format!("{}d ago", seconds / 86400)
        } else {
            format!("{:02}-{:02}", dt.month() as u8, dt.day())
        }
    } else {
        iso_str.chars().take(10).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Message;
    use crate::domain::Role;

    #[test]
    fn generate_name_includes_model_and_timestamp() {
        let name = SessionStore::generate_name("qwen2.5-coder");
        assert!(name.starts_with("qwen2-5-coder-"));
        assert!(name.contains("2025") || name.contains("2026"));
    }

    #[test]
    fn generate_name_sanitizes_special_chars() {
        let name = SessionStore::generate_name("model/name@v1");
        assert!(!name.contains('/'));
        assert!(!name.contains('@'));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,

            is_prompt: false,
        }];

        store
            .save("test-session", Some("gpt-4"), &messages)
            .unwrap();
        let loaded = store.load("test-session").unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].content, "hello");
    }

    #[test]
    fn list_returns_sorted_sessions() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "test".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,

            is_prompt: false,
        }];

        store.save("session-a", Some("model-a"), &messages).unwrap();
        store.save("session-b", Some("model-b"), &messages).unwrap();

        let sessions = store.list_with_meta().unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn delete_removes_session() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "test".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,

            is_prompt: false,
        }];

        store.save("to-delete", None, &messages).unwrap();
        assert!(store.session_path("to-delete").exists());

        store.delete("to-delete").unwrap();
        assert!(!store.session_path("to-delete").exists());
    }

    #[test]
    fn list_empty_store_returns_empty() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let sessions = store.list_with_meta().unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn load_missing_session_fails() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        assert!(store.load("never-saved").is_err());
    }

    #[test]
    fn list_skips_malformed_json() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "valid".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];
        store.save("valid", None, &messages).unwrap();
        std::fs::write(tmp_dir.path().join("invalid.json"), "not json").unwrap();

        let sessions = store.list_with_meta().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "valid");
    }

    #[test]
    fn list_skips_non_json_files() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "ok".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];
        store.save("session", None, &messages).unwrap();
        std::fs::write(tmp_dir.path().join("notes.txt"), "hello").unwrap();

        let sessions = store.list_with_meta().unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn concurrent_saves_are_safe() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();

        std::thread::scope(|s| {
            for i in 0..5 {
                let store_ref = &store;
                s.spawn(move || {
                    let messages = vec![Message {
                        role: Role::User,
                        content: format!("msg {}", i),
                        tool_calls: None,
                        tool_call_id: None,
                        timestamp: None,
                        reasoning_content: None,
                        is_prompt: false,
                    }];
                    store_ref
                        .save(&format!("thread-{}", i), None, &messages)
                        .unwrap();
                });
            }
        });

        let sessions = store.list().unwrap();
        assert_eq!(sessions.len(), 5);
    }

    #[test]
    fn derive_title_from_first_user_message() {
        let messages = vec![
            Message {
                role: Role::System,
                content: "sys".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::User,
                content: "My question here".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        store.save("titled", None, &messages).unwrap();

        let meta = store.list_with_meta().unwrap();
        assert_eq!(meta[0].title, Some("My question here".to_string()));
    }

    #[test]
    fn derive_title_truncates_long_content() {
        let long_content = "a".repeat(100);
        let messages = vec![Message {
            role: Role::User,
            content: long_content.clone(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        store.save("long-title", None, &messages).unwrap();

        let meta = store.list_with_meta().unwrap();
        let title = meta[0].title.as_ref().unwrap();
        assert!(title.len() < long_content.len());
        assert!(title.ends_with("..."));
    }

    #[test]
    fn derive_title_returns_none_for_empty_or_untitled() {
        for content in ["", "untitled", "UNTITLED"] {
            let messages = vec![Message {
                role: Role::User,
                content: content.to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            }];
            let tmp_dir = tempfile::tempdir().unwrap();
            let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
            store
                .save(&format!("empty-{}", content), None, &messages)
                .unwrap();

            let meta = store.list_with_meta().unwrap();
            assert!(
                meta[0].title.is_none(),
                "title should be None for '{}'",
                content
            );
        }
    }

    #[test]
    fn format_relative_time_parses_recent() {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        assert_eq!(format_relative_time(&now), "just now");
    }

    #[test]
    fn format_relative_time_fallback_for_invalid() {
        let result = format_relative_time("not-a-date");
        assert_eq!(result, "not-a-date");
    }
}
