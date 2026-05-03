use crate::backend::{Message, ToolCall, ToolDefinition, FunctionDefinition};
use crate::domain::Role;
use crate::mcp::McpTool;

pub fn parse_file_context(input: &str) -> String {
    let mut result = input.to_string();
    let mut failed_files = Vec::new();

    let re = regex::Regex::new(r"@(\S+)").unwrap();
    let mut replacements = Vec::new();

    for cap in re.captures_iter(input) {
        let path_str = cap[1].to_string();
        let path = std::path::Path::new(&path_str);

        match std::fs::read_to_string(path) {
            Ok(content) => {
                let expanded = format!("--- {} ---\n```\n{}\n```", path_str, content);
                replacements.push((cap[0].to_string(), expanded));
            }
            Err(e) => {
                failed_files.push(format!("{}: {}", path_str, e));
                replacements.push((
                    cap[0].to_string(),
                    format!("[Error reading {}: {}]", path_str, e),
                ));
            }
        }
    }

    for (pattern, replacement) in replacements {
        result = result.replace(&pattern, &replacement);
    }

    if !failed_files.is_empty() {
        result.push_str("\n\n[File errors: ");
        result.push_str(&failed_files.join(", "));
        result.push(']');
    }

    result
}

/// Build chat request messages including system prompt and user input
pub fn build_chat_request(
    existing_messages: &[Message],
    input: &str,
    system_prompt: Option<&str>,
) -> (Vec<Message>, String) {
    let content = parse_file_context(input);
    let mut messages = existing_messages.to_vec();

    if let Some(prompt) = system_prompt {
        if !messages.iter().any(|m| m.role == Role::System) {
            messages.push(Message {
                role: Role::System,
                content: prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    messages.push(Message {
        role: Role::User,
        content: content.clone(),
        tool_calls: None,
        tool_call_id: None,
    });
    (messages, content)
}

/// Read context files (SKILLS.md, SOUL.md, AGENTS.md) from disk
pub fn read_context_files() -> Vec<(String, String)> {
    let mut loaded = Vec::new();
    for filename in &["SKILLS.md", "SOUL.md", "AGENTS.md"] {
        let path = std::path::Path::new(filename);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                loaded.push((filename.to_string(), content));
            }
        }
    }
    loaded
}

/// Build tool result messages from pending tool calls and results
pub fn build_tool_result_messages(
    messages: &[Message],
    pending_tool_calls: &[ToolCall],
    tool_results: &[String],
) -> Vec<Message> {
    let mut result = messages.to_vec();
    for tool_call in pending_tool_calls {
        result.push(Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: Some(vec![tool_call.clone()]),
            tool_call_id: None,
        });
    }
    for (i, result_text) in tool_results.iter().enumerate() {
        if let Some(tool_call) = pending_tool_calls.get(i) {
            result.push(Message {
                role: Role::Tool,
                content: result_text.clone(),
                tool_calls: None,
                tool_call_id: Some(tool_call.id.clone()),
            });
        }
    }
    result
}

/// Convert available MCP tools to ToolDefinitions for API requests
pub fn get_tools_for_request(available_tools: &[McpTool]) -> Option<Vec<ToolDefinition>> {
    if available_tools.is_empty() {
        return None;
    }

    let tools: Vec<ToolDefinition> = available_tools
        .iter()
        .map(|t| ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            },
        })
        .collect();

    Some(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_chat_request_adds_user_message() {
        let messages = vec![];
        let (result, content) = build_chat_request(&messages, "hello", None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].role, Role::User);
        assert_eq!(result[0].content, "hello");
        assert_eq!(content, "hello");
    }

    #[test]
    fn build_chat_request_adds_system_prompt() {
        let messages = vec![];
        let (result, _) = build_chat_request(&messages, "hello", Some("system prompt"));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].role, Role::System);
        assert_eq!(result[1].role, Role::User);
    }

    #[test]
    fn build_chat_request_skips_duplicate_system() {
        let messages = vec![Message {
            role: Role::System,
            content: "existing".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];
        let (result, _) = build_chat_request(&messages, "hello", Some("new prompt"));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "existing");
    }

    #[test]
    fn get_tools_for_request_returns_none_for_empty() {
        assert!(get_tools_for_request(&[]).is_none());
    }

    #[test]
    fn get_tools_for_request_converts_tools() {
        let tools = vec![McpTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({}),
        }];
        let result = get_tools_for_request(&tools).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].function.name, "test_tool");
    }

    #[test]
    fn build_tool_result_messages_assembles_correctly() {
        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }];
        let tool_calls = vec![ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: crate::backend::FunctionCall {
                name: "tool".to_string(),
                arguments: "{}".to_string(),
            },
        }];
        let results = vec!["result".to_string()];
        let result = build_tool_result_messages(&messages, &tool_calls, &results);
        assert_eq!(result.len(), 3);
        assert_eq!(result[1].role, Role::Assistant);
        assert_eq!(result[2].role, Role::Tool);
        assert_eq!(result[2].tool_call_id, Some("call_1".to_string()));
    }
}