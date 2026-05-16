//! Runtime DI container for OpenLibertas.
//!
//! The [`Runtime`] struct holds all application dependencies in a central,
//! cloneable structure. It is inspired by kimi-cli's Runtime layer.

use std::path::PathBuf;
use std::sync::Arc;

use crate::backend::registry::ProviderRegistry;
use crate::config::Config;
use crate::env_context::EnvContext;
use crate::history::HistoryStore;
use crate::mcp::McpClient;
use crate::store::SessionStore;

/// Errors that can occur during runtime construction or agent creation.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("config error: {0}")]
    ConfigError(String),
    #[error("provider error: {0}")]
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

/// Central dependency injection container for OpenLibertas.
///
/// All heavy state is held behind `Arc`, making [`Runtime`] cheap to clone.
#[derive(Clone)]
pub struct Runtime {
    pub config: Arc<Config>,
    pub backend_registry: Arc<ProviderRegistry>,
    pub session_store: Arc<SessionStore>,
    pub history_store: Option<Arc<HistoryStore>>,
    pub mcp_client: Option<Arc<McpClient>>,
    pub env_context: String,
}

impl Runtime {
    /// Construct a new [`Runtime`] with default settings.
    ///
    /// - Loads config from the standard config path (falling back to defaults).
    /// - Builds the provider registry from configured providers.
    /// - Initializes the session store at `~/.config/openlibertas/sessions/`.
    /// - Initializes the history store if the config directory is available.
    /// - Initializes the MCP client from the opencode config if available.
    /// - Captures the current environment context.
    pub fn new() -> Result<Self, RuntimeError> {
        let config = Config::load().unwrap_or_default();
        let config = Arc::new(config);

        let backend_registry = Arc::new(ProviderRegistry::new(&config.providers));

        let sessions_dir = Config::config_path()
            .ok_or_else(|| {
                RuntimeError::ConfigError("Could not determine config path".to_string())
            })?
            .parent()
            .ok_or_else(|| {
                RuntimeError::ConfigError("Could not determine config directory".to_string())
            })?
            .join("sessions");

        let session_store =
            SessionStore::new(sessions_dir).map_err(|e| RuntimeError::StoreError(e.to_string()))?;
        let session_store = Arc::new(session_store);

        let history_store = Config::config_path()
            .and_then(|p| p.parent().map(PathBuf::from))
            .map(|config_dir| {
                let history_path = config_dir.join("history.jsonl");
                Arc::new(HistoryStore::new(history_path))
            });

        let mcp_client = McpClient::from_opencode_config().ok().map(Arc::new);

        let env_context = EnvContext::detect().to_prompt_section();

        Ok(Self {
            config,
            backend_registry,
            session_store,
            history_store,
            mcp_client,
            env_context,
        })
    }
}

impl Default for Runtime {
    fn default() -> Self {
        // Infallible default: use an in-memory session store and default config.
        let config = Arc::new(Config::default());
        let backend_registry = Arc::new(ProviderRegistry::new(&config.providers));
        let session_store = Arc::new(
            SessionStore::new(std::env::temp_dir().join("openlibertas-sessions"))
                .expect("temp dir should always be writable"),
        );
        Self {
            config,
            backend_registry,
            session_store,
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
    backend_registry: Option<Arc<ProviderRegistry>>,
    session_store: Option<Arc<SessionStore>>,
    history_store: Option<Option<Arc<HistoryStore>>>,
    mcp_client: Option<Option<Arc<McpClient>>>,
    env_context: Option<String>,
}

impl RuntimeBuilder {
    pub fn new() -> Self {
        Self {
            config: None,
            backend_registry: None,
            session_store: None,
            history_store: None,
            mcp_client: None,
            env_context: None,
        }
    }

    pub fn config(mut self, config: Arc<Config>) -> Self {
        self.config = Some(config);
        self
    }

    pub fn backend_registry(mut self, registry: Arc<ProviderRegistry>) -> Self {
        self.backend_registry = Some(registry);
        self
    }

    pub fn session_store(mut self, store: Arc<SessionStore>) -> Self {
        self.session_store = Some(store);
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
            .unwrap_or_else(|| Arc::new(ProviderRegistry::new(&config.providers)));

        let session_store = match self.session_store {
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
                    SessionStore::new(sessions_dir)
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
            session_store,
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
        let store = Arc::new(SessionStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .session_store(store)
            .build()
            .expect("build should succeed");
        assert!(runtime.session_store.list().is_ok());
    }

    #[test]
    fn runtime_clone_is_cheap() {
        let tmp = tempdir().unwrap();
        let store = Arc::new(SessionStore::new(tmp.path().to_path_buf()).unwrap());
        let runtime = RuntimeBuilder::new()
            .session_store(store)
            .build()
            .expect("build should succeed");

        let cloned = runtime.clone();
        assert_eq!(
            cloned.config.providers.len(),
            runtime.config.providers.len()
        );
        assert!(Arc::ptr_eq(&cloned.config, &runtime.config));
    }

    #[test]
    fn runtime_error_from_io_error() {
        let io_err = std::io::Error::other("test");
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
