use crate::agent_tools;
use crate::agent_tools::backend::{ToolBackend, ToolBackendError};
use crate::domain::{now_timestamp, Message, Role, ToolCall, ToolExecutionResult};
use crate::mcp::McpClient;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub struct ToolExecutor {
    backends: Vec<Box<dyn ToolBackend>>,
    pending_tool_calls: Vec<ToolCall>,
    tool_results: Vec<String>,
}

impl Default for ToolExecutor {
    fn default() -> Self {
        Self {
            backends: vec![Box::new(
                crate::agent_tools::builtin_backend::BuiltinBackend::new(),
            )],
            pending_tool_calls: Vec::new(),
            tool_results: Vec::new(),
        }
    }
}

impl std::fmt::Debug for ToolExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolExecutor")
            .field("backends", &self.backends.len())
            .field("pending_tool_calls", &self.pending_tool_calls)
            .field("tool_results", &self.tool_results)
            .finish()
    }
}

impl ToolExecutor {
    pub fn add_backend(&mut self, backend: Box<dyn ToolBackend>) {
        self.backends.push(backend);
    }

    pub fn with_client(mut self, client: McpClient) -> Self {
        self.add_backend(Box::new(crate::agent_tools::mcp_backend::McpBackend::new(
            Arc::new(client),
        )));
        self
    }

    pub fn client(&self) -> Option<Arc<McpClient>> {
        for backend in &self.backends {
            if let Some(client) = backend.mcp_client() {
                return Some(client);
            }
        }
        None
    }

    pub fn set_client(&mut self, client: Option<Arc<McpClient>>) {
        self.backends.retain(|b| b.mcp_client().is_none());
        if let Some(client) = client {
            self.add_backend(Box::new(crate::agent_tools::mcp_backend::McpBackend::new(
                client,
            )));
        }
    }

    pub fn pending_tool_calls(&self) -> &[ToolCall] {
        &self.pending_tool_calls
    }

    pub fn clear_pending_tool_calls(&mut self) {
        self.pending_tool_calls.clear();
    }

    pub fn add_tool_call(&mut self, tool_call: ToolCall) {
        self.pending_tool_calls.push(tool_call);
    }

    pub fn tool_results(&self) -> &[String] {
        &self.tool_results
    }

    pub fn push_tool_result(&mut self, result: String) {
        self.tool_results.push(result);
    }

    pub fn clear_tool_results(&mut self) {
        self.tool_results.clear();
    }

    pub fn has_pending_tool_calls(&self) -> bool {
        !self.pending_tool_calls.is_empty()
    }

    pub fn clear_pending(&mut self) {
        self.pending_tool_calls.clear();
    }

    pub async fn execute_pending_tools(
        &mut self,
        permission_service: &crate::permission::PermissionService,
        session_id: &str,
    ) -> Vec<ToolExecutionResult> {
        let mut results = Vec::new();
        info!(
            "Executing {} pending tool calls",
            self.pending_tool_calls.len(),
        );

        for tool_call in &self.pending_tool_calls {
            info!(
                "Tool call: {}({})",
                tool_call.function.name, tool_call.function.arguments
            );
            let key_arg =
                extract_key_argument(&tool_call.function.name, &tool_call.function.arguments);
            let is_builtin = agent_tools::is_builtin(&tool_call.function.name);
            let is_destructive = tool_needs_approval(&tool_call.function.name);
            let requires_approval = !is_builtin || is_destructive;

            if requires_approval {
                let args =
                    match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
                        Ok(args) => args,
                        Err(e) => {
                            results.push(ToolExecutionResult::Error {
                                tool_name: tool_call.function.name.clone(),
                                key_arg,
                                error: format!("Parse error: {e}"),
                            });
                            continue;
                        }
                    };

                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if crate::permission::tool_requires_permission(&tool_call.function.name)
                    && !crate::permission::is_safe_readonly_command(&command)
                {
                    let request = crate::permission::PermissionRequest::new(
                        session_id,
                        &tool_call.function.name,
                        format!("Execute: {}", command),
                        "execute",
                        args.clone(),
                        std::env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                    );

                    let approved = permission_service.request(request).await;

                    if !approved {
                        results.push(ToolExecutionResult::Skipped {
                            tool_name: tool_call.function.name.clone(),
                            reason: format!(
                                "Permission denied for '{}'",
                                tool_call.function.name
                            ),
                        });
                        continue;
                    }
                }
            }

            let args =
                match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
                    Ok(args) => args,
                    Err(e) => {
                        results.push(ToolExecutionResult::Error {
                            tool_name: tool_call.function.name.clone(),
                            key_arg,
                            error: format!("Parse error: {e}"),
                        });
                        continue;
                    }
                };

            let mut executed = false;
            for backend in &self.backends {
                if backend.can_execute(&tool_call.function.name) {
                    match backend
                        .execute(&tool_call.function.name, args.clone())
                        .await
                    {
                        Ok(output) => {
                            results.push(ToolExecutionResult::Success {
                                tool_name: tool_call.function.name.clone(),
                                key_arg: key_arg.clone(),
                                output,
                            });
                        }
                        Err(ToolBackendError::NotFound(_)) => continue,
                        Err(e) => {
                            results.push(ToolExecutionResult::Error {
                                tool_name: tool_call.function.name.clone(),
                                key_arg: key_arg.clone(),
                                error: e.to_string(),
                            });
                        }
                    }
                    executed = true;
                    break;
                }
            }

            if !executed {
                results.push(ToolExecutionResult::Error {
                    tool_name: tool_call.function.name.clone(),
                    key_arg,
                    error: "No backend found for tool".to_string(),
                });
            }
        }

        for result in &results {
            match result {
                ToolExecutionResult::Success {
                    tool_name, output, ..
                } => {
                    info!(
                        "Tool {} succeeded: {} chars output",
                        tool_name,
                        output.len()
                    );
                    debug!("Tool {} output: {}", tool_name, output);
                }
                ToolExecutionResult::Error {
                    tool_name, error, ..
                } => {
                    warn!("Tool {} failed: {}", tool_name, error);
                }
                ToolExecutionResult::Skipped {
                    tool_name, reason, ..
                } => {
                    info!("Tool {} skipped: {}", tool_name, reason);
                }
            }
        }

        self.tool_results = results
            .iter()
            .map(super::super::domain::ToolExecutionResult::content_for_message)
            .collect();
        results
    }

    pub fn create_tool_result_messages(&self) -> Vec<Message> {
        let mut messages = Vec::new();
        for (i, result) in self.tool_results.iter().enumerate() {
            if let Some(tool_call) = self.pending_tool_calls.get(i) {
                messages.push(Message {
                    role: Role::Tool,
                    content: result.clone(),
                    tool_calls: None,
                    tool_call_id: Some(tool_call.id.clone()),
                    timestamp: Some(now_timestamp()),
                    reasoning_content: None,
                    is_prompt: false,
                });
            }
        }
        messages
    }
}

pub fn tool_needs_approval(tool_name: &str) -> bool {
    let destructive = [
        "edit_file",
        "editfile",
        "editFile",
        "delete_file",
        "deletefile",
        "deleteFile",
        "remove_file",
        "patch",
        "apply_patch",
        "applyPatch",
        "str_replace_file",
        "strreplacefile",
        "strReplaceFile",
        "shell",
        "exec",
        "execute",
    ];
    destructive
        .iter()
        .any(|&d| tool_name.eq_ignore_ascii_case(d))
}

pub fn extract_key_argument(tool_name: &str, arguments: &str) -> String {
    let parsed: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
    let key_fields = match tool_name.to_lowercase().as_str() {
        n if n.contains("read") => vec!["path", "file", "uri", "url"],
        n if n.contains("write") || n.contains("edit") => vec!["path", "file", "uri"],
        n if n.contains("shell")
            || n.contains("exec")
            || n.contains("run")
            || n.contains("bash") =>
        {
            vec!["command", "cmd", "script"]
        }
        n if n.contains("search") => vec!["query", "q", "pattern", "term"],
        n if n.contains("fetch") || n.contains("get") || n.contains("download") => {
            vec!["url", "uri", "path"]
        }
        _ => vec!["path", "file", "name", "query", "command", "input"],
    };
    for field in key_fields {
        if let Some(val) = parsed.get(field) {
            if let Some(s) = val.as_str() {
                return s.to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tools::backend::ToolBackend;
    use crate::domain::{FunctionDefinition, ToolDefinition};
    use async_trait::async_trait;
    use serde_json::Value;

    #[derive(Debug)]
    struct MockBackend {
        tools: Vec<String>,
    }

    #[async_trait]
    impl ToolBackend for MockBackend {
        fn can_execute(&self, tool_name: &str) -> bool {
            self.tools.contains(&tool_name.to_string())
        }

        async fn execute(
            &self,
            _tool_name: &str,
            _args: Value,
        ) -> Result<String, ToolBackendError> {
            Ok("mock result".to_string())
        }

        fn tool_definitions(&self) -> Vec<ToolDefinition> {
            self.tools
                .iter()
                .map(|name| ToolDefinition {
                    tool_type: "function".to_string(),
                    function: FunctionDefinition {
                        name: name.clone(),
                        description: "mock".to_string(),
                        parameters: serde_json::json!({}),
                    },
                })
                .collect()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn tool_needs_approval_detects_destructive() {
        assert!(!tool_needs_approval("write_file"));
        assert!(tool_needs_approval("shell"));
        assert!(tool_needs_approval("str_replace_file"));
        assert!(tool_needs_approval("strReplaceFile"));
        assert!(!tool_needs_approval("read_file"));
    }

    #[test]
    fn extract_key_argument_reads_path() {
        let args = r#"{"path": "/path/to/test.txt"}"#;
        assert_eq!(extract_key_argument("read_file", args), "/path/to/test.txt");
    }

    #[test]
    fn extract_key_argument_falls_back() {
        let args = r#"{"file": "main.rs"}"#;
        assert_eq!(extract_key_argument("read_file", args), "main.rs");
    }

    #[test]
    fn extract_key_argument_shell_command() {
        let args = r#"{"command": "ls -la"}"#;
        assert_eq!(extract_key_argument("shell", args), "ls -la");
    }

    #[test]
    fn extract_key_argument_search_query() {
        let args = r#"{"query": "test pattern"}"#;
        assert_eq!(extract_key_argument("search", args), "test pattern");
    }

    #[test]
    fn extract_key_argument_no_match() {
        let args = r#"{}"#;
        assert_eq!(extract_key_argument("unknown", args), "");
    }

    #[test]
    fn executor_starts_empty() {
        let executor = ToolExecutor::default();
        assert!(executor.pending_tool_calls.is_empty());
        assert!(executor.tool_results.is_empty());
    }

    #[test]
    fn clear_pending_removes_calls() {
        let mut executor = ToolExecutor::default();
        executor.add_tool_call(ToolCall {
            id: "t1".to_string(),
            call_type: "function".to_string(),
            function: crate::domain::FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        });
        assert_eq!(executor.pending_tool_calls.len(), 1);
        executor.clear_pending();
        assert!(executor.pending_tool_calls.is_empty());
    }

    #[test]
    fn tool_needs_approval_variants() {
        assert!(tool_needs_approval("editFile"));
        assert!(tool_needs_approval("EXECUTE"));
        assert!(tool_needs_approval("Delete_File"));
        assert!(!tool_needs_approval("read_file"));
        assert!(!tool_needs_approval("search"));
    }

    #[tokio::test]
    async fn mock_backend_executes_tool() {
        let mut executor = ToolExecutor::default();
        executor.add_backend(Box::new(MockBackend {
            tools: vec!["mock_tool".to_string()],
        }));
        executor.add_tool_call(ToolCall {
            id: "t1".to_string(),
            call_type: "function".to_string(),
            function: crate::domain::FunctionCall {
                name: "mock_tool".to_string(),
                arguments: "{}".to_string(),
            },
        });
        let perm_service = crate::permission::PermissionService::new();
        let results = executor.execute_pending_tools(&perm_service, "test-session").await;
        assert_eq!(results.len(), 1);
        assert!(
            matches!(results[0], ToolExecutionResult::Success { ref output, .. } if output == "mock result")
        );
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        let mut executor = ToolExecutor::default();
        executor.add_tool_call(ToolCall {
            id: "t1".to_string(),
            call_type: "function".to_string(),
            function: crate::domain::FunctionCall {
                name: "unknown_tool".to_string(),
                arguments: "{}".to_string(),
            },
        });
        let perm_service = crate::permission::PermissionService::new();
        let results = executor.execute_pending_tools(&perm_service, "test-session").await;
        assert_eq!(results.len(), 1);
        assert!(
            matches!(results[0], ToolExecutionResult::Error { ref error, .. } if error == "No backend found for tool")
        );
    }
}
