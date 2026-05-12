pub mod tool_parser;

use crate::domain::Role;
use crate::domain::{estimate_messages_tokens, Message, ToolCall};

const MAX_TOOL_RESULT_CHARS: usize = 4000;

#[derive(Debug, Clone)]
enum MessageBlock {
    Single(Message),
    ToolCallPair {
        assistant: Message,
        results: Vec<Message>,
    },
}

impl MessageBlock {
    fn estimate_tokens(&self) -> usize {
        match self {
            MessageBlock::Single(msg) => msg.estimate_tokens(),
            MessageBlock::ToolCallPair { assistant, results } => {
                let results_tokens: usize = results.iter().map(|r| r.estimate_tokens()).sum();
                assistant.estimate_tokens().saturating_add(results_tokens)
            }
        }
    }

    fn into_messages(self) -> Vec<Message> {
        match self {
            MessageBlock::Single(msg) => vec![msg],
            MessageBlock::ToolCallPair { assistant, results } => {
                let mut msgs = vec![assistant];
                msgs.extend(results);
                msgs
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextCompactor {
    pub context_window: usize,
    pub threshold_ratio: f32,
    pub preserve_ratio: f32,
    pub compaction_count: usize,
}

impl Default for ContextCompactor {
    fn default() -> Self {
        Self {
            context_window: 8192,
            threshold_ratio: 0.75,
            preserve_ratio: 0.50,
            compaction_count: 0,
        }
    }
}

impl ContextCompactor {
    pub fn with_context_window(context_window: usize) -> Self {
        Self {
            context_window,
            ..Default::default()
        }
    }

    pub fn threshold_tokens(&self) -> usize {
        (self.context_window as f32 * self.threshold_ratio) as usize
    }

    pub fn preserve_tokens(&self) -> usize {
        (self.context_window as f32 * self.preserve_ratio) as usize
    }

    pub fn should_compact(&self, messages: &[Message]) -> bool {
        estimate_messages_tokens(messages) > self.threshold_tokens()
    }

    pub fn compact(&mut self, messages: &[Message]) -> Vec<Message> {
        if !self.should_compact(messages) {
            return messages.to_vec();
        }

        let mut system_msgs: Vec<Message> = Vec::new();
        let mut non_system: Vec<Message> = Vec::new();

        for msg in messages {
            if msg.role == Role::System {
                system_msgs.push(msg.clone());
            } else {
                non_system.push(msg.clone());
            }
        }

        let system_tokens: usize = system_msgs.iter().map(|m| m.estimate_tokens()).sum();
        let preserve_budget = self.preserve_tokens().saturating_sub(system_tokens);

        let blocks = Self::build_blocks(&non_system);
        if blocks.is_empty() {
            return system_msgs;
        }

        // Keep the most recent blocks that fit within the preserve budget.
        let mut preserved_blocks: Vec<MessageBlock> = Vec::new();
        let mut preserved_tokens = 0usize;
        for block in blocks.iter().rev() {
            let block_tokens = block.estimate_tokens();
            if preserved_tokens + block_tokens > preserve_budget && !preserved_blocks.is_empty() {
                break;
            }
            preserved_tokens += block_tokens;
            preserved_blocks.push(block.clone());
        }
        preserved_blocks.reverse();

        let dropped_count = blocks.len().saturating_sub(preserved_blocks.len());

        let mut result = system_msgs;

        if dropped_count > 0 {
            let summary = self.summarize_dropped(&blocks[..dropped_count]);
            result.push(Message { role: Role::System, content: summary, tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false });
        }

        for block in preserved_blocks {
            result.extend(block.into_messages());
        }

        self.compaction_count += 1;
        result
    }

    fn build_blocks(messages: &[Message]) -> Vec<MessageBlock> {
        let mut blocks = Vec::new();
        let mut i = 0;
        while i < messages.len() {
            let msg = &messages[i];
            if msg.role == Role::Assistant
                && msg.tool_calls.is_some()
                && !msg.tool_calls.as_ref().unwrap().is_empty()
            {
                let call_ids: std::collections::HashSet<&str> = msg
                    .tool_calls
                    .as_ref()
                    .unwrap()
                    .iter()
                    .map(|tc| tc.id.as_str())
                    .collect();
                let mut results = Vec::new();
                let mut j = i + 1;
                while j < messages.len() && messages[j].role == Role::Tool {
                    if let Some(ref id) = messages[j].tool_call_id {
                        if call_ids.contains(id.as_str()) {
                            results.push(messages[j].clone());
                            j += 1;
                            continue;
                        }
                    }
                    break;
                }
                blocks.push(MessageBlock::ToolCallPair {
                    assistant: msg.clone(),
                    results,
                });
                i = j;
            } else {
                blocks.push(MessageBlock::Single(msg.clone()));
                i += 1;
            }
        }
        blocks
    }

    fn summarize_dropped(&self, blocks: &[MessageBlock]) -> String {
        let mut dropped_users = 0usize;
        let mut dropped_assistants = 0usize;
        let mut dropped_tools = 0usize;
        for block in blocks {
            match block {
                MessageBlock::Single(msg) => match msg.role {
                    Role::User => dropped_users += 1,
                    Role::Assistant => dropped_assistants += 1,
                    Role::Tool => dropped_tools += 1,
                    _ => {}
                },
                MessageBlock::ToolCallPair { results, .. } => {
                    dropped_assistants += 1;
                    dropped_tools += results.len();
                }
            }
        }

        let mut summary_parts = Vec::new();
        if dropped_users > 0 {
            summary_parts.push(format!("{} user message(s)", dropped_users));
        }
        if dropped_assistants > 0 {
            summary_parts.push(format!("{} assistant message(s)", dropped_assistants));
        }
        if dropped_tools > 0 {
            summary_parts.push(format!("{} tool message(s)", dropped_tools));
        }

        if summary_parts.is_empty() {
            format!("[{} earlier message blocks compacted]", blocks.len())
        } else {
            format!(
                "[Context compacted: {} older message blocks summarized ({}). Recent context preserved.]",
                blocks.len(),
                summary_parts.join(", ")
            )
        }
    }
}

pub fn parse_file_context(input: &str) -> String {
    let mut result = input.to_string();
    let mut failed_files = Vec::new();

    let re = match regex::Regex::new(r#"(?:^|[\s\("'])@(?:"([^"]+)"|(\S+))"#) {
        Ok(re) => re,
        Err(_) => return input.to_string(),
    };
    let mut replacements = Vec::new();

    for cap in re.captures_iter(input) {
        let path_str = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        let full_match = cap
            .get(0)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();

        if path_str.is_empty() {
            continue;
        }

        let path_str = strip_trailing_punctuation(&path_str);
        let path = std::path::Path::new(&path_str);

        match std::fs::read_to_string(path) {
            Ok(content) => {
                let expanded = format!("--- {} ---\n```\n{}\n```", path_str, content);
                replacements.push((full_match, expanded));
            }
            Err(e) => {
                failed_files.push(format!("{}: {}", path_str, e));
                replacements.push((full_match, format!("[Error reading {}: {}]", path_str, e)));
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

fn strip_trailing_punctuation(s: &str) -> String {
    s.trim_end_matches(|c: char| {
        c == ','
            || c == '.'
            || c == ';'
            || c == ':'
            || c == '!'
            || c == '?'
            || c == ')'
            || c == ']'
            || c == '}'
            || c == '"'
            || c == '\''
    })
    .to_string()
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
            messages.push(Message { role: Role::System, content: prompt.to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false });
        }
    }

    messages.push(Message { role: Role::User, content: content.clone(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false });
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
            
                is_prompt: false,});
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
        
            is_prompt: false,}];
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
            
                is_prompt: false,},
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
            
                is_prompt: false,},
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
            
                is_prompt: false,},
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
            
                is_prompt: false,},
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
        let mut compactor = ContextCompactor::with_context_window(1000);
        let messages: Vec<Message> = (0..5)
            .map(|i| Message { role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            }, content: format!("msg {}", i), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false })
            .collect();

        let result = compactor.compact(&messages);
        assert_eq!(result.len(), 5);
        assert_eq!(compactor.compaction_count, 0);
    }

    #[test]
    fn compactor_preserves_system_messages() {
        let mut compactor = ContextCompactor::with_context_window(30);
        let messages = vec![
            Message {
                role: Role::System,
                content: "sys1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
        ];

        let result = compactor.compact(&messages);
        assert!(result.iter().any(|m| m.content == "sys1"));
        assert!(result.iter().any(|m| m.content.contains("compacted")));
        assert_eq!(compactor.compaction_count, 1);
    }

    #[test]
    fn compactor_adds_summary_message() {
        let mut compactor = ContextCompactor::with_context_window(50);
        let messages = vec![
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
        ];

        let result = compactor.compact(&messages);
        let summary = result
            .iter()
            .find(|m| m.role == Role::System && m.content.contains("compacted"));
        assert!(summary.is_some());
        let summary = summary.unwrap();
        assert!(summary.content.contains("user message(s)"));
        assert!(summary.content.contains("assistant message(s)"));
    }

    #[test]
    fn compactor_preserves_recent_messages() {
        let mut compactor = ContextCompactor::with_context_window(50);
        let messages = vec![
            Message {
                role: Role::User,
                content: "old1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "old2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "old3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "old4".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "keep1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "keep2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
        ];

        let result = compactor.compact(&messages);
        assert!(result.iter().any(|m| m.content == "keep1"));
        assert!(result.iter().any(|m| m.content == "keep2"));
        assert!(!result.iter().any(|m| m.content == "old1"));
        assert!(!result.iter().any(|m| m.content == "old3"));
    }

    #[test]
    fn compactor_counts_tool_messages() {
        let mut compactor = ContextCompactor::with_context_window(50);
        let messages = vec![
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Tool,
                content: "t1".to_string(),
                tool_calls: None,
                tool_call_id: Some("1".to_string()),
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u3".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
        ];

        let result = compactor.compact(&messages);
        let summary = result
            .iter()
            .find(|m| m.role == Role::System && m.content.contains("compacted"))
            .unwrap();
        assert!(summary.content.contains("tool message(s)"));
    }

    #[test]
    fn compactor_preserves_tool_call_pairs_atomically() {
        let mut compactor = ContextCompactor::with_context_window(80);
        let messages = vec![
            Message {
                role: Role::User,
                content: "u1".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a1".to_string(),
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
            
                is_prompt: false,},
            Message {
                role: Role::Tool,
                content: "result1".to_string(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::User,
                content: "u2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
            Message {
                role: Role::Assistant,
                content: "a2".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            
                is_prompt: false,},
        ];

        let result = compactor.compact(&messages);
        let has_assistant_with_call = result.iter().any(|m| {
            m.role == Role::Assistant
                && m.tool_calls.is_some()
                && m.tool_calls.as_ref().unwrap().iter().any(|tc| tc.id == "call_1")
        });
        let has_tool_result = result
            .iter()
            .any(|m| m.role == Role::Tool && m.tool_call_id == Some("call_1".to_string()));
        assert_eq!(
            has_assistant_with_call, has_tool_result,
            "tool call pair must be preserved atomically"
        );
    }

    #[test]
    fn compactor_uses_token_budget_not_message_count() {
        // Many very short messages (low token count) should NOT trigger compaction
        // when the context window is large enough.
        let mut compactor = ContextCompactor::with_context_window(1000);
        let short_messages: Vec<Message> = (0..30)
            .map(|i| Message { role: if i % 2 == 0 { Role::User } else { Role::Assistant }, content: "x".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false })
            .collect();

        let result = compactor.compact(&short_messages);
        assert_eq!(compactor.compaction_count, 0);
        assert_eq!(result.len(), 30);

        // A few very long messages (high token count) SHOULD trigger compaction
        // even with the same message count that was safe above.
        let mut compactor = ContextCompactor::with_context_window(100);
        let long_content = "word ".repeat(200);
        let long_messages: Vec<Message> = (0..6)
            .map(|i| Message { role: if i % 2 == 0 { Role::User } else { Role::Assistant }, content: long_content.clone(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false })
            .collect();

        let result = compactor.compact(&long_messages);
        assert_eq!(compactor.compaction_count, 1);
        assert!(result.len() < long_messages.len() + 2);
    }

    #[test]
    fn parse_file_context_ignores_email_addresses() {
        let input = "Contact me at user@example.com for details";
        let result = parse_file_context(input);
        assert_eq!(result, input);
    }

    #[test]
    fn parse_file_context_strips_trailing_punctuation() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_punct.txt");
        std::fs::write(&file_path, "punct content").unwrap();

        let input = format!("Check @{} for details.", file_path.display());
        let result = parse_file_context(&input);

        assert!(result.contains("punct content"));
        assert!(!result.contains("[Error reading"));

        std::fs::remove_file(&file_path).unwrap();
    }

    #[test]
    fn parse_file_context_quoted_path_with_spaces() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test spaced file.txt");
        std::fs::write(&file_path, "spaced content").unwrap();

        let input = format!("Read @\"{}\" now", file_path.display());
        let result = parse_file_context(&input);

        assert!(result.contains("spaced content"));

        std::fs::remove_file(&file_path).unwrap();
    }
}
