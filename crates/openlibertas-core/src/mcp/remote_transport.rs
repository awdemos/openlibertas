use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use super::{McpTool, ToolCallResult, ToolContent, ToolResult};
use crate::config::SecretString;
use crate::mcp::transport::{McpServerType, McpTransport, McpTransportError};

pub struct RemoteTransport {
    url: String,
    api_key: Option<SecretString>,
    headers: HashMap<String, String>,
}

impl RemoteTransport {
    pub fn new(
        url: String,
        api_key: Option<SecretString>,
        headers: HashMap<String, String>,
    ) -> Self {
        Self {
            url,
            api_key,
            headers,
        }
    }
}

#[async_trait]
impl McpTransport for RemoteTransport {
    async fn discover_tools(&self) -> Result<Vec<McpTool>, McpTransportError> {
        let client = reqwest::Client::new();
        let mut req = client.get(format!("{}/tools", self.url.trim_end_matches("/mcp")));
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key.expose_secret()));
        }
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.map_err(|e| {
            McpTransportError::RequestFailed(format!("Failed to discover remote tools: {e}"))
        })?;

        if resp.status().is_success() {
            let tools: Vec<McpTool> = resp
                .json()
                .await
                .map_err(|e| McpTransportError::InvalidResponse(e.to_string()))?;
            Ok(tools)
        } else {
            Err(McpTransportError::RequestFailed(format!(
                "HTTP {}",
                resp.status()
            )))
        }
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult, McpTransportError> {
        let client = reqwest::Client::new();
        let mut req = client
            .post(format!("{}/call", self.url.trim_end_matches("/mcp")))
            .json(&serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            }));
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key.expose_secret()));
        }
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;

        if resp.status().is_success() {
            let result: ToolCallResult = resp
                .json()
                .await
                .map_err(|e| McpTransportError::InvalidResponse(e.to_string()))?;
            Ok(ToolResult {
                content: result
                    .content
                    .into_iter()
                    .map(|c| ToolContent {
                        content_type: "text".to_string(),
                        text: c.text,
                    })
                    .collect(),
                is_error: result.is_error,
            })
        } else {
            Err(McpTransportError::RequestFailed(format!(
                "HTTP {}",
                resp.status()
            )))
        }
    }

    async fn health_check(&self) -> Result<bool, McpTransportError> {
        let client = reqwest::Client::new();
        let mut req = client.get(format!("{}/tools", self.url.trim_end_matches("/mcp")));
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key.expose_secret()));
        }
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        match req.send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    fn server_type(&self) -> McpServerType {
        McpServerType::Remote {
            url: self.url.clone(),
        }
    }
}
