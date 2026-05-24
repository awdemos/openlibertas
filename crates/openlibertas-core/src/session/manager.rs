use crate::domain::Message;
use crate::store::{SessionMeta, SessionStore};
use anyhow::Result;

pub type SessionError = anyhow::Error;

use super::context::SessionContext;

pub struct SessionManager {
    store: SessionStore,
    context: SessionContext,
}

impl SessionManager {
    pub fn new(store: SessionStore) -> Self {
        Self {
            store,
            context: SessionContext::default(),
        }
    }

    pub fn context(&self) -> &SessionContext {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut SessionContext {
        &mut self.context
    }

    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    pub fn branch(
        &mut self,
        messages: &[Message],
        msg_idx: Option<usize>,
        model: Option<&str>,
    ) -> Result<String, SessionError> {
        let mut parent_id = self.context.current_id.clone().unwrap_or_default();

        if parent_id.is_empty() {
            let auto_model = model.unwrap_or("unknown");
            let auto_id = SessionStore::generate_name(auto_model);
            self.store.save(&auto_id, model, messages).map_err(|e| {
                SessionError::msg(format!("Failed to auto-save before branch: {}", e))
            })?;
            self.context.current_id = Some(auto_id.clone());
            parent_id = auto_id;
        }

        let branch_point = msg_idx.unwrap_or_else(|| messages.len().saturating_sub(1));

        if branch_point >= messages.len() {
            return Err(SessionError::msg(format!(
                "Invalid branch point. There are {} messages (0..{})",
                messages.len(),
                messages.len().saturating_sub(1)
            )));
        }

        let branch_messages: Vec<_> = messages[..=branch_point].to_vec();
        let branch_model = model.unwrap_or("unknown");
        let branch_id = format!("{}-branch", SessionStore::generate_name(branch_model));

        self.store.save_branch(
            &branch_id,
            model,
            &branch_messages,
            Some(&parent_id),
            Some(branch_point),
        )?;

        self.store.add_branch(&parent_id, &branch_id)?;

        self.context.current_id = Some(branch_id.clone());
        self.context.parent_id = Some(parent_id.clone());
        self.context.branch_point = Some(branch_point);
        self.refresh_has_branches();

        Ok(branch_id)
    }

    pub fn save(&mut self, messages: &[Message], model: Option<&str>) -> Result<(), SessionError> {
        let id = self
            .context
            .current_id
            .clone()
            .unwrap_or_else(|| SessionStore::generate_name(model.unwrap_or("unknown")));
        self.store.save(&id, model, messages)?;
        self.context.current_id = Some(id);
        self.refresh_has_branches();
        Ok(())
    }

    pub fn save_with_id(
        &mut self,
        id: &str,
        messages: &[Message],
        model: Option<&str>,
    ) -> Result<(), SessionError> {
        self.store.save(id, model, messages)?;
        self.context.current_id = Some(id.to_string());
        self.refresh_has_branches();
        Ok(())
    }

    pub fn load(&mut self, id: &str) -> Result<crate::store::Session, SessionError> {
        let path = self.store.session_path(id);
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read session: {id}"))?;
        let session: crate::store::Session = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse session: {id}"))?;

        self.context.current_id = Some(id.to_string());
        self.context.parent_id = session.parent_id.clone();
        self.context.branch_point = session.branch_point;
        self.refresh_has_branches();

        Ok(session)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), SessionError> {
        self.store.delete(id)?;
        if self.context.current_id.as_deref() == Some(id) {
            self.context.current_id = None;
            self.context.parent_id = None;
            self.context.branch_point = None;
            self.context.has_branches = false;
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<String>, SessionError> {
        self.store.list()
    }

    pub fn list_with_meta(&self) -> Result<Vec<SessionMeta>, SessionError> {
        self.store.list_with_meta()
    }

    pub fn session_path(&self, id: &str) -> std::path::PathBuf {
        self.store.session_path(id)
    }

    pub fn clear(&mut self) {
        self.context.current_id = None;
        self.context.parent_id = None;
        self.context.branch_point = None;
        self.context.has_branches = false;
    }

    pub fn set_current_id(&mut self, id: Option<String>) {
        self.context.current_id = id;
        self.refresh_has_branches();
    }

    fn refresh_has_branches(&mut self) {
        if let Some(ref id) = self.context.current_id {
            if let Ok(sessions) = self.store.list_with_meta() {
                self.context.has_branches = sessions
                    .iter()
                    .any(|s| s.id == *id && !s.branches.is_empty());
            }
        } else {
            self.context.has_branches = false;
        }
    }
}

use anyhow::Context as _;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Message, Role};

    #[test]
    fn branch_creates_new_session_with_parent() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let mut manager = SessionManager::new(store);

        let messages = vec![
            Message {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Assistant,
                content: "hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];

        // Save parent first
        manager
            .save_with_id("parent-session", &messages, Some("gpt-4"))
            .unwrap();

        let branch_id = manager.branch(&messages, Some(0), Some("gpt-4")).unwrap();
        assert!(branch_id.contains("branch"));

        assert_eq!(manager.context.current_id, Some(branch_id));
        assert_eq!(
            manager.context.parent_id,
            Some("parent-session".to_string())
        );
        assert_eq!(manager.context.branch_point, Some(0));
    }

    #[test]
    fn branch_auto_saves_when_no_current_id() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let mut manager = SessionManager::new(store);

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];

        let branch_id = manager.branch(&messages, None, Some("gpt-4")).unwrap();
        assert!(branch_id.contains("branch"));
        assert!(manager.context.current_id.is_some());
        assert!(manager.context.parent_id.is_some());
    }

    #[test]
    fn branch_rejects_invalid_index() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let mut manager = SessionManager::new(store);

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];

        let result = manager.branch(&messages, Some(5), Some("gpt-4"));
        assert!(result.is_err());
    }

    #[test]
    fn save_sets_current_id() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let mut manager = SessionManager::new(store);

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];

        manager.save(&messages, Some("gpt-4")).unwrap();
        assert!(manager.context.current_id.is_some());
    }

    #[test]
    fn load_updates_context() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let mut manager = SessionManager::new(store);

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];

        manager
            .save_with_id("test-session", &messages, Some("gpt-4"))
            .unwrap();
        manager.clear();
        assert!(manager.context.current_id.is_none());

        let session = manager.load("test-session").unwrap();
        assert_eq!(session.id, "test-session");
        assert_eq!(manager.context.current_id, Some("test-session".to_string()));
    }

    #[test]
    fn delete_clears_context_when_current() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let store = SessionStore::new(tmp_dir.path().to_path_buf()).unwrap();
        let mut manager = SessionManager::new(store);

        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];

        manager.save_with_id("to-delete", &messages, None).unwrap();
        manager.delete("to-delete").unwrap();
        assert!(manager.context.current_id.is_none());
    }
}
