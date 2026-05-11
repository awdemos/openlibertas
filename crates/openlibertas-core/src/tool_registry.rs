use crate::domain::{Message, ToolCall, ToolDefinition};
use crate::conversation::build_tool_result_messages;
use crate::domain::{now_timestamp, Role, ToolExecutionResult};
use crate::mcp::{McpClient, McpTool};
use crate::tools;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug)]
pub struct ToolRegistry {
    client: Option<Arc<McpClient>>,
    available_tools: Vec<McpTool>,
    builtin_tools: Vec<tools::BuiltinTool>,
    pending_tool_calls: Vec<ToolCall>,
    tool_results: Vec<String>,
    server_statuses: HashMap<String, crate::domain::McpServerStatus>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            client: None,
            available_tools: Vec::new(),
            builtin_tools: tools::builtin_tools(),
            pending_tool_calls: Vec::new(),
            tool_results: Vec::new(),
            server_statuses: HashMap::new(),
        }
    }
}

impl ToolRegistry {
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

    pub fn available_tools(&self) -> &[McpTool] {
        &self.available_tools
    }

    pub fn set_available_tools(&mut self, tools: Vec<McpTool>) {
        self.available_tools = tools;
    }

    pub fn builtin_tools(&self) -> &[tools::BuiltinTool] {
        &self.builtin_tools
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

    pub fn server_statuses(&self) -> &HashMap<String, crate::domain::McpServerStatus> {
        &self.server_statuses
    }

    pub fn set_server_statuses(&mut self, statuses: HashMap<String, crate::domain::McpServerStatus>) {
        self.server_statuses = statuses;
    }

    pub fn has_pending_tool_calls(&self) -> bool {
        !self.pending_tool_calls.is_empty()
    }

    pub fn clear_pending(&mut self) {
        self.pending_tool_calls.clear();
    }

    pub fn tools_for_request(&self, plan_mode: bool) -> Option<Vec<ToolDefinition>> {
        let mut all_tools = Vec::new();
        if !plan_mode {
            for tool in &self.available_tools {
                all_tools.push(ToolDefinition {
                    tool_type: "function".to_string(),
                    function: crate::domain::FunctionDefinition {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.input_schema.clone(),
                    },
                });
            }
        }
        let read_only_builtins: &[&str] = &[
            "read_file", "glob", "grep", "web_search", "fetch_url", "think", "git",
        ];
        for tool in &self.builtin_tools {
            if plan_mode && !read_only_builtins.contains(&tool.name.as_str()) {
                continue;
            }
            all_tools.push(tool.to_tool_definition());
        }
        if all_tools.is_empty() {
            None
        } else {
            Some(all_tools)
        }
    }

    pub fn tool_instructions(&self) -> Option<String> {
        let mut tool_lines = Vec::new();
        for tool in &self.builtin_tools {
            let params = serde_json::to_string(&tool.parameters).unwrap_or_default();
            tool_lines.push(format!(
                "  - {}: {}\n    Parameters: {}",
                tool.name, tool.description, params
            ));
        }
        for tool in &self.available_tools {
            let schema = serde_json::to_string(&tool.input_schema).unwrap_or_default();
            tool_lines.push(format!(
                "  - {}: {}\n    Parameters: {}",
                tool.name, tool.description, schema
            ));
        }
        if tool_lines.is_empty() {
            None
        } else {
            Some(format!(
                "You have access to the following tools. When a tool can help answer the user's question, you MUST invoke it by producing a tool_calls array containing the function name and arguments.\n\n\
                Available tools:\n{}\n\n\
                RULES:\n\
                1. Output ONLY the tool_calls JSON array. Do not add explanatory text before or after.\n\
                2. Each tool call must have this exact structure:\n\
                   {{\"id\": \"call_1\", \"type\": \"function\", \"function\": {{\"name\": \"tool_name\", \"arguments\": \"{{\\\"param\\\": \\\"value\\\"}}\"}}}}\n\
                3. The 'arguments' field must be a JSON-encoded string, not a raw object.\n\
                4. Do not describe what the tool does. Do not write example code. Do not say 'I will use'.\n\
                5. Call the tool directly — results will be provided to you automatically.",
                tool_lines.join("\n")
            ))
        }
    }

    /// Assemble messages with agent context injected.
    /// Prepends the agent prompt as a system message when active, then
    /// appends tool result messages for pending tool calls.
    pub fn assemble_messages_with_agent_context(
        &self,
        messages: &[Message],
        agent_status: Option<&str>,
        agent_prompt: Option<&str>,
    ) -> Vec<Message> {
        let mut result = messages.to_vec();

        if let Some(prompt) = agent_prompt {
            if agent_status == Some("active") {
                result.insert(
                    0,
                    Message { role: Role::System, content: prompt.to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None },
                );
            }
        }

        build_tool_result_messages(&result, &self.pending_tool_calls, &self.tool_results)
    }

    pub async fn execute_pending_tools(&mut self, yolo_mode: bool) -> Vec<ToolExecutionResult> {
        let mut results = Vec::new();
        info!("Executing {} pending tool calls (yolo={})", self.pending_tool_calls.len(), yolo_mode);

        for tool_call in &self.pending_tool_calls {
            info!("Tool call: {}({})", tool_call.function.name, tool_call.function.arguments);
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
                            match tools::execute_builtin(&tool_call.function.name, args) {
                                Ok(output) => ToolExecutionResult::Success {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg: key_arg.clone(),
                                    output,
                                },
                                Err(e) => ToolExecutionResult::Error {
                                    tool_name: tool_call.function.name.clone(),
                                    key_arg: key_arg.clone(),
                                    error: e.to_string(),
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
                ToolExecutionResult::Success { tool_name, output, .. } => {
                    info!("Tool {} succeeded: {} chars output", tool_name, output.len());
                    debug!("Tool {} output: {}", tool_name, output);
                }
                ToolExecutionResult::Error { tool_name, error, .. } => {
                    warn!("Tool {} failed: {}", tool_name, error);
                }
                ToolExecutionResult::Skipped { tool_name, reason, .. } => {
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
                messages.push(Message { role: Role::Tool, content: result.clone(), tool_calls: None, tool_call_id: Some(tool_call.id.clone()), timestamp: Some(now_timestamp()), reasoning_content: None });
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
        assert!(!tool_needs_approval("shell"));
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
    fn registry_starts_empty() {
        let registry = ToolRegistry::default();
        assert!(registry.available_tools.is_empty());
        assert!(registry.pending_tool_calls.is_empty());
    }

    #[test]
    fn tools_for_request_includes_builtins() {
        let registry = ToolRegistry::default();
        let result = registry.tools_for_request(false).unwrap();
        assert!(!result.is_empty(), "builtin tools should be present");
    }

    #[test]
    fn tools_for_request_converts_available_tools() {
        let mut registry = ToolRegistry::default();
        let builtin_count = registry.tools_for_request(false).unwrap().len();
        registry.available_tools.push(crate::mcp::McpTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({}),
        });
        let result = registry.tools_for_request(false).unwrap();
        assert_eq!(result.len(), builtin_count + 1);
        assert!(result.iter().any(|t| t.function.name == "test_tool"));
    }

    #[test]
    fn clear_pending_removes_calls() {
        let mut registry = ToolRegistry::default();
        registry.add_tool_call(ToolCall {
            id: "t1".to_string(),
            call_type: "function".to_string(),
            function: crate::domain::FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        });
        assert_eq!(registry.pending_tool_calls.len(), 1);
        registry.clear_pending();
        assert!(registry.pending_tool_calls.is_empty());
    }

    #[test]
    fn tool_needs_approval_variants() {
        assert!(tool_needs_approval("editFile"));
        assert!(!tool_needs_approval("EXECUTE"));
        assert!(tool_needs_approval("Delete_File"));
        assert!(!tool_needs_approval("read_file"));
        assert!(!tool_needs_approval("search"));
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
}
