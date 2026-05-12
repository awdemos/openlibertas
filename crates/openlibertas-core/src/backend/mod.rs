use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::capability::ProviderCapabilities;
use crate::domain::{ChatEvent, Message, Model, ToolDefinition};

pub mod multi_provider;
pub mod registry;

pub use multi_provider::MultiProviderBackend;

pub trait Backend: Send + Sync {
    fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<ChatEvent>;

    fn fetch_models(&self) -> Pin<Box<dyn Future<Output = Result<Vec<Model>>> + Send + '_>>;

    fn health_check(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    fn cancel_token(&self) -> CancellationToken;

    fn capabilities(&self) -> ProviderCapabilities;
}
