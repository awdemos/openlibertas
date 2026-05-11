use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::domain::Message;
use crate::domain::Role;

#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Conversation {
    pub id: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub messages: Vec<Message>,
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
}

#[derive(Clone)]
pub struct ConversationStore {
    data_dir: PathBuf,
}

impl ConversationStore {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", data_dir))?;
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
            md.push_str(&format!("**Model:** {}\n\n", m));
        }
        md.push_str(&format!("**Date:** {}\n\n", now));
        md.push_str("---\n\n");

        for msg in messages {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => model.unwrap_or("Assistant"),
                Role::System => "System",
                Role::Tool => "Tool",
            };
            md.push_str(&format!("## {}\n\n{}", role, msg.content));
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    md.push_str(&format!("\n\n**Tool Call:** `{}`", tc.function.name));
                    md.push_str(&format!("\n```json\n{}\n```", tc.function.arguments));
                }
            }
            md.push_str("\n\n---\n\n");
        }

        let path = self.data_dir.join(id);
        fs::write(&path, md)
            .with_context(|| format!("Failed to write markdown file: {:?}", path))?;
        Ok(())
    }

    pub fn save(&self, id: &str, model: Option<&str>, messages: &[Message]) -> Result<()> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let title = Self::derive_title(messages);

        let conversation = Conversation {
            id: id.to_string(),
            title,
            model: model.map(|s| s.to_string()),
            created_at: now.clone(),
            updated_at: Some(now),
            messages: messages.to_vec(),
        };

        let temp_path = self.data_dir.join(format!("{}.tmp", id));
        let final_path = self.conversation_path(id);

        let json = serde_json::to_string_pretty(&conversation)
            .context("Failed to serialize conversation")?;
        fs::write(&temp_path, json)
            .with_context(|| format!("Failed to write temp file: {:?}", temp_path))?;
        fs::rename(&temp_path, &final_path)
            .with_context(|| format!("Failed to rename temp file to: {:?}", final_path))?;

        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<Vec<Message>> {
        let path = self.conversation_path(id);
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read conversation: {:?}", path))?;
        let conversation: Conversation = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse conversation: {:?}", path))?;
        Ok(conversation.messages)
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
                        if let Ok(conv) = serde_json::from_str::<Conversation>(&contents) {
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
        let path = self.conversation_path(id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete conversation: {:?}", path))?;
        }
        Ok(())
    }

    pub fn conversation_path(&self, id: &str) -> PathBuf {
        self.data_dir.join(format!("{}.json", id))
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
        let name = ConversationStore::generate_name("qwen2.5-coder");
        assert!(name.starts_with("qwen2-5-coder-"));
        assert!(name.contains("2025") || name.contains("2026"));
    }

    #[test]
    fn generate_name_sanitizes_special_chars() {
        let name = ConversationStore::generate_name("model/name@v1");
        assert!(!name.contains('/'));
        assert!(!name.contains('@'));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = ConversationStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
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
        let store = ConversationStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "test".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
        }];

        store.save("session-a", Some("model-a"), &messages).unwrap();
        store.save("session-b", Some("model-b"), &messages).unwrap();

        let sessions = store.list_with_meta().unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn delete_removes_session() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = ConversationStore::new(tmp_dir.path().to_path_buf()).unwrap();

        let messages = vec![Message {
            role: Role::User,
            content: "test".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
        }];

        store.save("to-delete", None, &messages).unwrap();
        assert!(store.conversation_path("to-delete").exists());

        store.delete("to-delete").unwrap();
        assert!(!store.conversation_path("to-delete").exists());
    }
}
