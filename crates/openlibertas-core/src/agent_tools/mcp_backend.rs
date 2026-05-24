use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::agent_tools::backend::{ToolBackend, ToolBackendError};
use crate::domain::{FunctionDefinition, ToolDefinition};
use crate::mcp::{McpClient, McpTool};

#[derive(Debug)]
pub struct McpBackend {
    client: Arc<McpClient>,
    available_tools: Mutex<Vec<McpTool>>,
}

impl McpBackend {
    pub fn new(client: Arc<McpClient>) -> Self {
        Self {
            client,
            available_tools: Mutex::new(Vec::new()),
        }
    }

    pub fn client(&self) -> Arc<McpClient> {
        self.client.clone()
    }

    pub fn set_available_tools(&self, tools: Vec<McpTool>) {
        if let Ok(mut guard) = self.available_tools.lock() {
            *guard = tools;
        }
    }

    pub fn available_tools(&self) -> Vec<McpTool> {
        self.available_tools
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl ToolBackend for McpBackend {
    fn can_execute(&self, tool_name: &str) -> bool {
        self.available_tools
            .lock()
            .map(|g| g.iter().any(|t| t.name == tool_name))
            .unwrap_or(false)
    }

    async fn execute(&self, tool_name: &str, args: Value) -> Result<String, ToolBackendError> {
        match self.client.call_tool(tool_name, args).await {
            Ok(output) => {
                let text = output
                    .content
                    .into_iter()
                    .map(|c| c.text)
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(text)
            }
            Err(e) => Err(ToolBackendError::ExecutionFailed(e.to_string())),
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.available_tools
            .lock()
            .map(|g| {
                g.iter()
                    .map(|t| ToolDefinition {
                        tool_type: "function".to_string(),
                        function: FunctionDefinition {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.input_schema.clone(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn mcp_client(&self) -> Option<Arc<McpClient>> {
        Some(self.client.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
