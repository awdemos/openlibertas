//! Runtime DI container for OpenLibertas.
//!
//! The [`Runtime`] struct holds all application dependencies in a central,
//! cloneable structure. It is inspired by kimi-cli's Runtime layer.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::backend::registry::BackendRegistry;
use crate::config::Config;
use crate::domain::{ChatEvent, Message, ProviderId, ToolDefinition};
use crate::engine::ChatEngine;
use crate::env_context::EnvContext;
use crate::history::HistoryStore;
use crate::mcp::McpClient;
use crate::store::ConversationStore;

/// Errors that can occur during runtime construction or agent creation.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("config error: {0}")]
    ConfigError(String),
    #[error("backend error: {0}")]
    BackendError(String),
    #[error("store error: {0}")]
    StoreError(String),
    #[error("other error: {0}")]
    Other(String),
}

impl From<anyhow::Error> for RuntimeError {
    fn from(e: anyhow::Error) -> Self {
        RuntimeError::Other(e.to_string())
    }
}

impl From<std::io::Error> for RuntimeError {
    fn from(e: std::io::Error) -> Self {
        RuntimeError::StoreError(e.to_string())
    }
}

/// Communication wire for an active chat session.
pub struct Wire {
    pub rx: mpsc::UnboundedReceiver<ChatEvent>,
}

/// Minimal agent trait for chat agents.
pub trait Agent: Send {
    /// Access the underlying chat engine.
    fn engine(&self) -> &ChatEngine;
    /// Mutable access to the underlying chat engine.
    fn engine_mut(&mut self) -> &mut ChatEngine;
    /// Start a chat with the given messages and return a wire for receiving events.
    fn chat(
        &mut self,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        temperature: Option<f32>,
    ) -> Wire;
}

/// A chat agent with all runtime dependencies injected.
pub struct ChatAgent {
    pub engine: ChatEngine,
    pub model: String,
    pub backend_registry: Arc<BackendRegistry>,
}

impl Agent for ChatAgent {
    fn engine(&self) -> &ChatEngine {
        &self.engine
    }

    fn engine_mut(&mut self) -> &mut ChatEngine {
        &mut self.engine
    }

    fn chat(
        &mut self,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        temperature: Option<f32>,
    ) -> Wire {
        self.engine.chat_mut().cancel_token = CancellationToken::new();
        self.engine.tool_executor_mut().clear_pending_tool_calls();

        let provider = self
            .backend_registry
            .default_provider_id()
            .cloned()
            .unwrap_or_else(|| ProviderId::new("default"));

        let rx = self.backend_registry.chat_with_fallback(
            &provider,
            self.model.clone(),
            messages,
            max_tokens,
            tools,
            self.engine.chat().cancel_token.clone(),
            temperature,
        );

        Wire { rx }
    }
}

/// Central dependency injection container for OpenLibertas.
///
/// All heavy state is held behind `Arc`, making [`Runtime`] cheap to clone.
#[derive(Clone)]
pub struct Runtime {
    pub config: Arc<Config>,
    pub backend_registry: Arc<BackendRegistry>,
    pub conversation_store: Arc<ConversationStore>,
    pub history_store: Option<Arc<HistoryStore>>,
    pub mcp_client: Option<Arc<McpClient>>,
    pub env_context: String,
}

impl Runtime {
    /// Construct a new [`Runtime`] with default settings.
    ///
    /// - Loads config from the standard config path (falling back to defaults).
    /// - Builds the backend registry from configured providers.
    /// - Initializes the conversation store at `~/.config/openlibertas/sessions/`.
    /// - Initializes the history store if the config directory is available.
    /// - Initializes the MCP client from the opencode config if available.
    /// - Captures the current environment context.
    pub fn new() -> Result<Self, RuntimeError> {
        let config = Config::load().unwrap_or_default();
        let config = Arc::new(config);

        let backend_registry = Arc::new(BackendRegistry::new(&config.providers));

        let sessions_dir = Config::config_path()
            .ok_or_else(|| {
                RuntimeError::ConfigError("Could not determine config path".to_string())
            })?
            .parent()
            .ok_or_else(|| {
                RuntimeError::ConfigError("Could not determine config directory".to_string())
            })?
            .join("sessions");

        let conversation_store =
            ConversationStore::new(sessions_dir).map_err(|e| RuntimeError::StoreError(e.to_string()))?;
        let conversation_store = Arc::new(conversation_store);

        let history_store = Config::config_path()
            .and_then(|p| p.parent().map(PathBuf::from))
            .map(|config_dir| {
                let history_path = config_dir.join("history.jsonl");
                Arc::new(HistoryStore::new(history_path))
            });

        let mcp_client = McpClient::from_opencode_config()
            .ok()
            .map(Arc::new);

        let env_context = EnvContext::detect().to_prompt_section();

        Ok(Self {
            config,
            backend_registry,
            conversation_store,
            history_store,
            mcp_client,
            env_context,
        })
    }

    /// Create a new chat agent with all runtime dependencies injected.
    ///
    /// The agent is configured with the given model and optional persona.
    /// If a persona is provided, the agent loop is started automatically.
    pub fn create_agent(
        &self,
        model: String,
        persona: Option<String>,
    ) -> Result<Box<dyn Agent>, RuntimeError> {
        let mut engine = ChatEngine::new().with_env_context(EnvContext::detect());

        if let Some(ref mcp) = self.mcp_client {
            engine.tool_executor_mut().set_client(Some(mcp.clone()));
            engine.tools_mut().set_client(Some(mcp.clone()));
        }

        if let Some(persona) = persona {
            let prompt = format!(
                "You are the {} agent. Execute tasks autonomously using available tools.",
                persona
            );
            engine.switch_persona(&persona, &prompt);
            engine.agents_mut().status = crate::engine::AgentStatus::Idle;
            engine.start_agent_loop();
        }

        if let Some(ref history) = self.history_store {
            engine = engine.with_history_store((**history).clone());
        }

        engine.load_history();

        Ok(Box::new(ChatAgent {
            engine,
            model,
            backend_registry: self.backend_registry.clone(),
        }))
    }
}

impl Default for Runtime {
    fn default() -> Self {
        // Infallible default: use an in-memory conversation store and default config.
        let config = Arc::new(Config::default());
        let backend_registry = Arc::new(BackendRegistry::new(&config.providers));
        let conversation_store = Arc::new(
            ConversationStore::new(std::env::temp_dir().join("openlibertas-sessions"))
                .expect("temp dir should always be writable"),
        );
        Self {
            config,
            backend_registry,
            conversation_store,
            history_store: None,
            mcp_client: None,
            env_context: EnvContext::detect().to_prompt_section(),
        }
    }
}

/// Fluent builder for constructing a [`Runtime`] with overrides.
///
/// Useful for testing or when specific components need to be injected.
pub struct RuntimeBuilder {
    config: Option<Arc<Config>>,
    backend_registry: Option<Arc<BackendRegistry>>,
    conversation_store: Option<Arc<ConversationStore>>,
    history_store: Option<Option<Arc<HistoryStore>>>,
    mcp_client: Option<Option<Arc<McpClient>>>,
    env_context: Option<String>,
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            backend_registry: None,
            conversation_store: None,
            history_store: None,
            mcp_client: None,
            env_context: None,
        }
    }

    pub fn config(mut self, config: Arc<Config>) -> Self {
        self.config = Some(config);
        self
    }

    pub fn backend_registry(mut self, registry: Arc<BackendRegistry>) -> Self {
        self.backend_registry = Some(registry);
        self
    }

    pub fn conversation_store(mut self, store: Arc<ConversationStore>) -> Self {
        self.conversation_store = Some(store);
        self
    }

    pub fn history_store(mut self, store: Option<Arc<HistoryStore>>) -> Self {
        self.history_store = Some(store);
        self
    }

    pub fn mcp_client(mut self, client: Option<Arc<McpClient>>) -> Self {
        self.mcp_client = Some(client);
        self
    }

    pub fn env_context(mut self, ctx: String) -> Self {
        self.env_context = Some(ctx);
        self
    }

    pub fn build(self) -> Result<Runtime, RuntimeError> {
        let config = self
            .config
            .unwrap_or_else(|| Arc::new(Config::load().unwrap_or_default()));
        let backend_registry = self
            .backend_registry
            .unwrap_or_else(|| Arc::new(BackendRegistry::new(&config.providers)));

        let conversation_store = match self.conversation_store {
            Some(store) => store,
            None => {
                let sessions_dir = Config::config_path()
                    .ok_or_else(|| {
                        RuntimeError::ConfigError("Could not determine config path".to_string())
                    })?
                    .parent()
                    .ok_or_else(|| {
                        RuntimeError::ConfigError(
                            "Could not determine config directory".to_string(),
                        )
                    })?
                    .join("sessions");
                Arc::new(
                    ConversationStore::new(sessions_dir)
                        .map_err(|e| RuntimeError::StoreError(e.to_string()))?,
                )
            }
        };

        let history_store = match self.history_store {
            Some(store) => store,
            None => Config::config_path()
                .and_then(|p| p.parent().map(PathBuf::from))
                .map(|config_dir| {
                    let history_path = config_dir.join("history.jsonl");
                    Arc::new(HistoryStore::new(history_path))
                }),
        };

        let mcp_client = match self.mcp_client {
            Some(client) => client,
            None => McpClient::from_opencode_config().ok().map(Arc::new),
        };

        let env_context = self
            .env_context
            .unwrap_or_else(|| EnvContext::detect().to_prompt_section());

        Ok(Runtime {
            config,
            backend_registry,
            conversation_store,
            history_store,
            mcp_client,
            env_context,
        })
    }
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AgentStatus;
    use tempfile::tempdir;

    #[test]
    fn runtime_default_is_infallible() {
        let runtime = Runtime::default();
        assert!(!runtime.config.providers.is_empty());
        assert!(runtime.history_store.is_none());
        assert!(runtime.mcp_client.is_none());
    }

    #[test]
    fn runtime_builder_can_override_config() {
        let config = Arc::new(Config::default());
        let runtime = RuntimeBuilder::new()
            .config(config.clone())
            .build()
            .expect("build should succeed");
        assert_eq!(runtime.config.providers.len(), 1);
    }

    #[test]
    fn runtime_builder_with_custom_store() {
        let tmp = tempdir().unwrap();
        let store = Arc::new(ConversationStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .conversation_store(store)
            .build()
            .expect("build should succeed");
        assert!(runtime.conversation_store.list().is_ok());
    }

    #[test]
    fn runtime_clone_is_cheap() {
        let tmp = tempdir().unwrap();
        let store = Arc::new(ConversationStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .conversation_store(store)
            .build()
            .expect("build should succeed");

        let cloned = runtime.clone();
        assert_eq!(cloned.config.providers.len(), runtime.config.providers.len());
        assert!(Arc::ptr_eq(&cloned.config, &runtime.config));
    }

    #[test]
    fn create_agent_without_persona() {
        let tmp = tempdir().unwrap();
        let store = Arc::new(ConversationStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .conversation_store(store)
            .build()
            .expect("build should succeed");

        let agent = runtime
            .create_agent("test-model".to_string(), None)
            .expect("create_agent should succeed");
        assert_eq!(agent.engine().agents().status, AgentStatus::Disabled);
    }

    #[test]
    fn create_agent_with_persona_starts_loop() {
        let tmp = tempdir().unwrap();
        let store = Arc::new(ConversationStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .conversation_store(store)
            .build()
            .expect("build should succeed");

        let agent = runtime
            .create_agent("test-model".to_string(), Some("Coding".to_string()))
            .expect("create_agent should succeed");
        assert_eq!(agent.engine().agents().status, AgentStatus::Active);
        assert_eq!(agent.engine().agents().persona, "Coding");

        assert!(agent.engine().env_context().is_some());
    }

    #[test]
    fn create_agent_injects_mcp_client() {
        let tmp = tempdir().unwrap();
        let store = Arc::new(ConversationStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .conversation_store(store)
            .mcp_client(None)
            .build()
            .expect("build should succeed");

        let agent = runtime
            .create_agent("test-model".to_string(), None)
            .expect("create_agent should succeed");
        assert!(agent.engine().tool_executor().client().is_none());
    }

    #[test]
    fn runtime_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "test");
        let runtime_err: RuntimeError = io_err.into();
        assert!(matches!(runtime_err, RuntimeError::StoreError(_)));
    }

    #[test]
    fn runtime_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("test error");
        let runtime_err: RuntimeError = anyhow_err.into();
        assert!(matches!(runtime_err, RuntimeError::Other(_)));
    }
}
