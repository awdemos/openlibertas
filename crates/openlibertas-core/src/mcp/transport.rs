use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn discover_tools(&self) -> Result<Vec<super::McpTool>, McpTransportError>;
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<super::ToolResult, McpTransportError>;
    async fn health_check(&self) -> Result<bool, McpTransportError>;
    fn server_type(&self) -> McpServerType;
}

#[derive(Debug, Clone)]
pub enum McpServerType {
    Local { command: String, args: Vec<String> },
    Remote { url: String },
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum McpTransportError {
    #[error("transport not connected: {0}")]
    NotConnected(String),
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("timeout")]
    Timeout,
}
