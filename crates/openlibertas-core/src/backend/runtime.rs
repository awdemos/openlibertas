use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::backend::registry::{ChatRequest, ProviderRegistry};
use crate::backend::Provider;
use crate::capability::ProviderCapabilities;
use crate::domain::{BackendEvent, Message, Model, ProviderId, ToolDefinition};

/// Provider wrapper that delegates chat to [`ProviderRegistry::chat_with_fallback`].
#[derive(Clone)]
pub struct RegistryBackend {
    registry: ProviderRegistry,
    preferred_provider: ProviderId,
}

impl RegistryBackend {
    pub fn new(registry: ProviderRegistry, preferred_provider: ProviderId) -> Self {
        Self {
            registry,
            preferred_provider,
        }
    }
}

impl Provider for RegistryBackend {
    fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        let request = ChatRequest::new(model, messages)
            .with_max_tokens(max_tokens)
            .with_tools(tools.unwrap_or_default());
        let request = if let Some(temp) = temperature {
            request.with_temperature(temp)
        } else {
            request
        };
        self.registry
            .chat_with_fallback(&self.preferred_provider, request, cancel_token)
    }

    fn fetch_models(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<Model>>> + Send + '_>> {
        match self.registry.default_provider() {
            Some(provider) => provider.fetch_models(),
            None => Box::pin(async { Ok(vec![]) }),
        }
    }

    fn health_check(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        match self.registry.default_provider() {
            Some(provider) => provider.health_check(),
            None => Box::pin(async { Ok(()) }),
        }
    }

    fn cancel_token(&self) -> CancellationToken {
        CancellationToken::new()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        match self.registry.default_provider() {
            Some(provider) => provider.capabilities(),
            None => ProviderCapabilities::default(),
        }
    }
}

/// Wrapper that injects a shared cancellation token into every chat call.
#[derive(Clone)]
pub struct CancellableBackend {
    inner: Arc<dyn Provider>,
    cancel_token: CancellationToken,
}

impl CancellableBackend {
    pub fn new(inner: Arc<dyn Provider>, cancel_token: CancellationToken) -> Self {
        Self {
            inner,
            cancel_token,
        }
    }
}

impl Provider for CancellableBackend {
    fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        _cancel_token: CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        self.inner.chat(
            model,
            messages,
            max_tokens,
            tools,
            self.cancel_token.clone(),
            temperature,
        )
    }

    fn fetch_models(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<Vec<Model>>> + Send + '_>> {
        self.inner.fetch_models()
    }

    fn health_check(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        self.inner.health_check()
    }

    fn cancel_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner.capabilities()
    }
}
