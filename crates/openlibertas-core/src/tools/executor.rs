use crate::domain::{now_timestamp, Message, Role, ToolCall, ToolExecutionResult};
use crate::mcp::McpClient;
use crate::tools;
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct ToolExecutor {
    client: Option<Arc<McpClient>>,
    pending_tool_calls: Vec<ToolCall>,
    tool_results: Vec<String>,
}

impl Default for ToolExecutor {
    fn default() -> Self {
        Self {
            client: None,
            pending_tool_calls: Vec::new(),
            tool_results: Vec::new(),
        }
    }
}

impl ToolExecutor {
    pub fn with_client(mut self, client: McpClient) -> Self {
        self.client = Some(Arc::new(client));
        self
    }

    pub fn client(&self) -> Option<Arc<McpClient>> {
        self.client.clone()
    }

    pub fn set_client(&mut self, client: Option<Arc<McpClient>>) {
        self.client = client;
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

    pub async fn execute_pending_tools(&mut self, yolo_mode: bool) -> Vec<ToolExecutionResult> {
        let mut results = Vec::new();
        info!(
            "Executing {} pending tool calls (yolo={})",
            self.pending_tool_calls.len(),
            yolo_mode
        );

        for tool_call in &self.pending_tool_calls {
            info!(
                "Tool call: {}({})",
                tool_call.function.name, tool_call.function.arguments
            );
            let key_arg =
                extract_key_argument(&tool_call.function.name, &tool_call.function.arguments);

            if !yolo_mode
                && (!tools::is_builtin(&tool_call.function.name)
                    || tool_needs_approval(&tool_call.function.name))
            {
                results.push(ToolExecutionResult::Skipped {
                    tool_name: tool_call.function.name.clone(),
                    reason: format!(
                        "Approval required for '{}'. Enable YOLO mode to skip confirmations.",
                        tool_call.function.name
                    ),
                });
                continue;
            }

            let result =
                match serde_json::from_str::<serde_json::Value>(&tool_call.function.arguments) {
                    Ok(args) => {
                        if tools::is_builtin(&tool_call.function.name) {
                            let tool_name = tool_call.function.name.clone();
                            let key_arg = key_arg.clone();
                            let builtin_result = tokio::task::spawn_blocking(move || {
                                tools::execute_builtin(&tool_name, args)
                            })
                            .await;
                            match builtin_result {
                                Ok(Ok(output)) => ToolExecutionResult::Success {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg,
                                    output,
                                },
                                Ok(Err(e)) => ToolExecutionResult::Error {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg,
                                    error: e.to_string(),
                                },
                                Err(e) => ToolExecutionResult::Error {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg,
                                    error: format!("Tool execution panicked: {}", e),
                                },
                            }
                        } else if let Some(client) = &self.client {
                            match client.call_tool(&tool_call.function.name, args).await {
                                Ok(output) => {
                                    let text = output
                                        .content
                                        .into_iter()
                                        .map(|c| c.text)
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    ToolExecutionResult::Success {
                                        tool_name: tool_call.function.name.clone(),
                                        key_arg: key_arg.clone(),
                                        output: text,
                                    }
                                }
                                Err(e) => ToolExecutionResult::Error {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg: key_arg.clone(),
                                    error: e.to_string(),
                                },
                            }
                        } else {
                            ToolExecutionResult::Error {
                                tool_name: tool_call.function.name.clone(),
                                key_arg: key_arg.clone(),
                                error: "MCP client not available".to_string(),
                            }
                        }
                    }
                    Err(e) => ToolExecutionResult::Error {
                        tool_name: tool_call.function.name.clone(),
                        key_arg: key_arg.clone(),
                        error: format!("Parse error: {}", e),
                    },
                };
            match &result {
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
            results.push(result);
        }

        self.tool_results = results.iter().map(|r| r.content_for_message()).collect();
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
        let args = r#"{"path": "/tmp/test.txt"}"#;
        assert_eq!(extract_key_argument("read_file", args), "/tmp/test.txt");
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
}
