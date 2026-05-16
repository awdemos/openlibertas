//! Test utilities shared across the `openlibertas-core` test suite.
//!
//! Everything in this module is gated behind `#[cfg(test)]` and re-exported
//! from `lib.rs` only when running tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::backend::Provider;
use crate::capability::ProviderCapabilities;
use crate::config::SecretString;
use crate::domain::{BackendEvent, Message, Model, ProviderId, Role, ToolDefinition};

/// Create a temporary directory suitable for `SessionStore` tests.
///
/// The directory is automatically cleaned up when the returned
/// `tempfile::TempDir` goes out of scope.
pub fn temp_store_dir() -> std::path::PathBuf {
    tempfile::tempdir()
        .expect("failed to create temp dir")
        .path()
        .to_path_buf()
}

/// A mock `Provider` that replays a fixed sequence of `BackendEvent`s.
///
/// Useful for testing agent loops, stream processing, and error handling
/// without making real HTTP requests.
pub struct MockProvider {
    events: Mutex<Vec<BackendEvent>>,
}

impl MockProvider {
    pub fn new(events: Vec<BackendEvent>) -> Self {
        Self {
            events: Mutex::new(events),
        }
    }

    pub fn empty() -> Self {
        Self::new(vec![])
    }

    pub fn text_then_done(text: &str) -> Self {
        Self::new(vec![
            BackendEvent::Text(text.to_string()),
            BackendEvent::Done,
        ])
    }

    pub fn error(msg: &str) -> Self {
        Self::new(vec![BackendEvent::Error(msg.to_string())])
    }

    pub fn cancelled() -> Self {
        Self::new(vec![BackendEvent::Cancelled])
    }
}

impl Provider for MockProvider {
    fn chat(
        &self,
        _model: String,
        _messages: Vec<Message>,
        _max_tokens: u32,
        _tools: Option<Vec<ToolDefinition>>,
        _cancel_token: tokio_util::sync::CancellationToken,
        _temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let events: Vec<BackendEvent> = self
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for event in events {
            let _ = tx.send(event);
        }
        rx
    }

    fn fetch_models(
        &self,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<Model>>> + Send + '_>> {
        Box::pin(async { Ok(vec![]) })
    }

    fn health_check(
        &self,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn cancel_token(&self) -> tokio_util::sync::CancellationToken {
        tokio_util::sync::CancellationToken::new()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }
}

/// Build a sample `Message` with the given role and content.
pub fn sample_message(role: Role, content: &str) -> Message {
    Message {
        role,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    }
}

/// Build a sample `Message` from the user.
pub fn user_message(content: &str) -> Message {
    sample_message(Role::User, content)
}

/// Build a sample `Message` from the assistant.
pub fn assistant_message(content: &str) -> Message {
    sample_message(Role::Assistant, content)
}

/// Build a sample `Message` with a tool call.
pub fn assistant_with_tool(name: &str, args: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: String::new(),
        tool_calls: Some(vec![crate::domain::ToolCall {
            id: format!("call_{}", name),
            call_type: "function".to_string(),
            function: crate::domain::FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        }]),
        tool_call_id: None,
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    }
}

/// Build a tool-result message.
pub fn tool_result(tool_call_id: &str, content: &str) -> Message {
    Message {
        role: Role::Tool,
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
        timestamp: None,
        reasoning_content: None,
        is_prompt: false,
    }
}

/// Create a mock `ProviderConfig` for testing.
pub fn mock_provider_config(name: &str, base_url: &str) -> crate::config::ProviderConfig {
    crate::config::ProviderConfig {
        name: name.to_string(),
        base_url: base_url.to_string(),
        api_key: SecretString::new("sk-test".to_string()),
        enabled: true,
        kind: crate::capability::ProviderKind::OpenAiCompatible,
        capabilities: crate::capability::ProviderKind::OpenAiCompatible.default_capabilities(),
        tool_format: crate::tool_format::ToolFormat::Native,
        extra_params: None,
    }
}

/// Create a `SessionStore` backed by a temporary directory.
pub fn temp_session_store() -> crate::store::SessionStore {
    let dir = temp_store_dir();
    crate::store::SessionStore::new(dir).expect("failed to create SessionStore")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_provider_replays_events() {
        let provider = MockProvider::text_then_done("hi");
        let mut rx = provider.chat(
            "m".to_string(),
            vec![],
            1024,
            None,
            tokio_util::sync::CancellationToken::new(),
            None,
        );
        let event = rx.recv().await;
        assert!(matches!(event, Some(crate::domain::BackendEvent::Text(ref t)) if t == "hi"));
        let event = rx.recv().await;
        assert!(matches!(event, Some(crate::domain::BackendEvent::Done)));
    }

    #[test]
    fn sample_messages_build_correctly() {
        let u = user_message("hello");
        assert_eq!(u.role, Role::User);
        assert_eq!(u.content, "hello");

        let a = assistant_message("world");
        assert_eq!(a.role, Role::Assistant);

        let t = assistant_with_tool("read_file", r#"{"path":"/tmp"}"#);
        assert!(t.tool_calls.is_some());

        let r = tool_result("call_1", "ok");
        assert_eq!(r.role, Role::Tool);
        assert_eq!(r.tool_call_id, Some("call_1".to_string()));
    }

    #[test]
    fn temp_session_store_creates_directory() {
        let store = temp_session_store();
        // Saving a session should succeed.
        store
            .save("test", Some("model"), &[user_message("hi")])
            .unwrap();
        let loaded = store.load("test").unwrap();
        assert_eq!(loaded.len(), 1);
    }
}
