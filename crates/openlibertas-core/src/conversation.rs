pub mod tool_parser;

use crate::domain::{Message, ToolCall};
use crate::domain::Role;

const MAX_TOOL_RESULT_CHARS: usize = 4000;

#[derive(Debug, Clone)]
pub struct ContextCompactor {
    pub message_threshold: usize,
    pub preserve_recent: usize,
    pub compaction_count: usize,
}

impl Default for ContextCompactor {
    fn default() -> Self {
        Self {
            message_threshold: 20,
            preserve_recent: 6,
            compaction_count: 0,
        }
    }
}

impl ContextCompactor {
    pub fn new(message_threshold: usize, preserve_recent: usize) -> Self {
        Self {
            message_threshold,
            preserve_recent,
            compaction_count: 0,
        }
    }

    pub fn should_compact(&self, messages: &[Message]) -> bool {
        messages.len() > self.message_threshold
    }

    pub fn compact(&mut self, messages: &[Message]) -> Vec<Message> {
        if !self.should_compact(messages) {
            return messages.to_vec();
        }

        let mut system_msgs: Vec<Message> = Vec::new();
        let mut non_system: Vec<&Message> = Vec::new();

        for msg in messages {
            if msg.role == Role::System {
                system_msgs.push(msg.clone());
            } else {
                non_system.push(msg);
            }
        }

        let total_non_system = non_system.len();
        if total_non_system <= self.preserve_recent {
            let mut result = system_msgs;
            result.extend(non_system.into_iter().cloned());
            return result;
        }

        let drop_count = total_non_system - self.preserve_recent;

        let mut dropped_users = 0usize;
        let mut dropped_assistants = 0usize;
        let mut dropped_tools = 0usize;
        for msg in non_system.iter().take(drop_count) {
            match msg.role {
                Role::User => dropped_users += 1,
                Role::Assistant => dropped_assistants += 1,
                Role::Tool => dropped_tools += 1,
                _ => {}
            }
        }

        let mut result = system_msgs;

        let mut summary_parts = Vec::new();
        if dropped_users > 0 {
            summary_parts.push(format!("{} user", dropped_users));
        }
        if dropped_assistants > 0 {
            summary_parts.push(format!("{} assistant", dropped_assistants));
        }
        if dropped_tools > 0 {
            summary_parts.push(format!("{} tool", dropped_tools));
        }

        let summary = if summary_parts.is_empty() {
            format!("[{} earlier messages compacted]", drop_count)
        } else {
            format!(
                "[Context compacted: {} messages summarized ({}). Recent context preserved.]",
                drop_count,
                summary_parts.join(", ")
            )
        };

        result.push(Message { role: Role::System, content: summary, tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None });

        for msg in non_system.iter().skip(drop_count) {
            result.push((*msg).clone());
        }

        self.compaction_count += 1;
        result
    }
}

pub fn parse_file_context(input: &str) -> String {
    let mut result = input.to_string();
    let mut failed_files = Vec::new();

    let re = match regex::Regex::new(r"@(\S+)") {
        Ok(re) => re,
        Err(_) => return input.to_string(),
    };
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
            messages.push(Message { role: Role::System, content: prompt.to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None });
        }
    }

    messages.push(Message { role: Role::User, content: content.clone(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None });
    (messages, content)
}

/// Read context files (SKILLS.md, SOUL.md) from disk.
/// These are injected as system messages at startup for developer context.
pub fn read_context_files() -> Vec<(String, String)> {
    let mut loaded = Vec::new();
    for filename in &["SKILLS.md", "SOUL.md"] {
        let path = std::path::Path::new(filename);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                loaded.push((filename.to_string(), content));
            }
        }
    }
    loaded
}

/// Build tool result messages from pending tool calls and results.
/// The assistant message with tool_calls must already be present in
/// `messages`; this function only appends the `role: "tool"` results.
/// Results exceeding MAX_TOOL_RESULT_CHARS are truncated to protect context window.
pub fn build_tool_result_messages(
    messages: &[Message],
    pending_tool_calls: &[ToolCall],
    tool_results: &[String],
) -> Vec<Message> {
    let mut result = messages.to_vec();
    for (i, result_text) in tool_results.iter().enumerate() {
        if let Some(tool_call) = pending_tool_calls.get(i) {
            let content = if result_text.len() > MAX_TOOL_RESULT_CHARS {
                format!(
                    "{}\n\n[Result truncated: {} chars total, showing first {}]",
                    &result_text[..MAX_TOOL_RESULT_CHARS],
                    result_text.len(),
                    MAX_TOOL_RESULT_CHARS
                )
            } else {
                result_text.clone()
            };
            result.push(Message {
                role: Role::Tool,
                content,
                tool_calls: None,
                tool_call_id: Some(tool_call.id.clone()),
                timestamp: None,
reasoning_content: None,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FunctionCall;

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
            timestamp: None,
reasoning_content: None,
        }];
        let (result, _) = build_chat_request(&messages, "hello", Some("new prompt"));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "existing");
    }

    #[test]
    fn build_tool_result_messages_appends_tool_messages_only() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "".to_string(),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "tool".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
        ];
        let tool_calls = vec![ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "tool".to_string(),
                arguments: "{}".to_string(),
            },
        }];
        let results = vec!["result".to_string()];
        let result = build_tool_result_messages(&messages, &tool_calls, &results);
        assert_eq!(result.len(), 3);
        assert_eq!(result[2].role, Role::Tool);
        assert_eq!(result[2].tool_call_id, Some("call_1".to_string()));
    }

    #[test]
    fn build_tool_result_messages_truncates_long_results() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "".to_string(),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "tool".to_string(),
                        arguments: "{}".to_string(),
                    },
                }]),
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
        ];
        let tool_calls = vec![ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "tool".to_string(),
                arguments: "{}".to_string(),
            },
        }];
        let long_result = "x".repeat(MAX_TOOL_RESULT_CHARS + 100);
        let results = vec![long_result.clone()];
        let result = build_tool_result_messages(&messages, &tool_calls, &results);
        assert_eq!(result.len(), 3);
        assert_eq!(result[2].role, Role::Tool);
        assert!(result[2].content.len() < long_result.len());
        assert!(result[2].content.contains("truncated"));
    }

    #[test]
    fn parse_file_context_no_references() {
        let input = "hello world";
        let result = parse_file_context(input);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn parse_file_context_existing_file() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_file_context.txt");
        std::fs::write(&file_path, "file content").unwrap();

        let input = format!("@{} some text", file_path.display());
        let result = parse_file_context(&input);

        assert!(result.contains("--- "));
        assert!(result.contains("file content"));
        assert!(result.contains("some text"));

        std::fs::remove_file(&file_path).unwrap();
    }

    #[test]
    fn parse_file_context_missing_file() {
        let input = "@/nonexistent/path/file.txt";
        let result = parse_file_context(input);
        assert!(result.contains("[Error reading"));
        assert!(result.contains("/nonexistent/path/file.txt"));
    }

    #[test]
    fn parse_file_context_multiple_files() {
        let temp_dir = std::env::temp_dir();
        let file1 = temp_dir.join("test_multi_1.txt");
        let file2 = temp_dir.join("test_multi_2.txt");
        std::fs::write(&file1, "content1").unwrap();
        std::fs::write(&file2, "content2").unwrap();

        let input = format!("@{} and @{} done", file1.display(), file2.display());
        let result = parse_file_context(&input);

        assert!(result.contains("content1"));
        assert!(result.contains("content2"));
        assert!(result.contains("done"));

        std::fs::remove_file(&file1).unwrap();
        std::fs::remove_file(&file2).unwrap();
    }

    #[test]
    fn parse_file_context_mixed_existing_and_missing() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_mixed.txt");
        std::fs::write(&file_path, "exists").unwrap();

        let input = format!("@{} @/missing/file.txt", file_path.display());
        let result = parse_file_context(&input);

        assert!(result.contains("exists"));
        assert!(result.contains("[Error reading"));
        assert!(result.contains("File errors:"));

        std::fs::remove_file(&file_path).unwrap();
    }

    #[test]
    fn compactor_does_nothing_below_threshold() {
        let mut compactor = ContextCompactor::new(10, 4);
        let messages: Vec<Message> = (0..5)
            .map(|i| Message { role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            }, content: format!("msg {}", i), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None })
            .collect();

        let result = compactor.compact(&messages);
        assert_eq!(result.len(), 5);
        assert_eq!(compactor.compaction_count, 0);
    }

    #[test]
    fn compactor_preserves_system_messages() {
        let mut compactor = ContextCompactor::new(5, 2);
        let messages = vec![
            Message {
                role: Role::System,
                content: "sys1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
        ];

        let result = compactor.compact(&messages);
        assert!(result.iter().any(|m| m.content == "sys1"));
        assert_eq!(result.len(), 4);
        assert_eq!(compactor.compaction_count, 1);
    }

    #[test]
    fn compactor_adds_summary_message() {
        let mut compactor = ContextCompactor::new(5, 2);
        let messages = vec![
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
        ];

        let result = compactor.compact(&messages);
        let summary = result
            .iter()
            .find(|m| m.role == Role::System && m.content.contains("compacted"));
        assert!(summary.is_some());
        let summary = summary.unwrap();
        assert!(summary.content.contains("2 user"));
        assert!(summary.content.contains("2 assistant"));
    }

    #[test]
    fn compactor_preserves_recent_messages() {
        let mut compactor = ContextCompactor::new(5, 2);
        let messages = vec![
            Message {
                role: Role::User,
                content: "old1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "old2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "old3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "old4".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "keep1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "keep2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
        ];

        let result = compactor.compact(&messages);
        assert!(result.iter().any(|m| m.content == "keep1"));
        assert!(result.iter().any(|m| m.content == "keep2"));
        assert!(!result.iter().any(|m| m.content == "old1"));
        assert!(!result.iter().any(|m| m.content == "old3"));
    }

    #[test]
    fn compactor_counts_tool_messages() {
        let mut compactor = ContextCompactor::new(5, 2);
        let messages = vec![
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Tool,
                content: "t1".to_string(),
                tool_calls: None,
                tool_call_id: Some("1".to_string()),
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: "u3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
            },
        ];

        let result = compactor.compact(&messages);
        let summary = result
            .iter()
            .find(|m| m.role == Role::System && m.content.contains("compacted"))
            .unwrap();
        assert!(summary.content.contains("1 tool"));
    }
}
