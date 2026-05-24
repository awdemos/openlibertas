use async_trait::async_trait;
use serde_json::Value;

use crate::agent_tools;
use crate::agent_tools::backend::{ToolBackend, ToolBackendError};
use crate::domain::ToolDefinition;

#[derive(Debug)]
pub struct BuiltinBackend;

impl BuiltinBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BuiltinBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolBackend for BuiltinBackend {
    fn can_execute(&self, tool_name: &str) -> bool {
        agent_tools::is_builtin(tool_name)
    }

    async fn execute(&self, tool_name: &str, args: Value) -> Result<String, ToolBackendError> {
        let tool_name = tool_name.to_string();
        let result =
            tokio::task::spawn_blocking(move || agent_tools::execute_builtin(&tool_name, args))
                .await;

        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(ToolBackendError::ExecutionFailed(e.to_string())),
            Err(e) => Err(ToolBackendError::ExecutionFailed(format!(
                "Tool execution panicked: {e}"
            ))),
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        agent_tools::builtin_tools()
            .into_iter()
            .map(|t| t.to_tool_definition())
            .collect()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
