use crate::agent_tools::ToolExecutor;
use crate::domain::{now_timestamp, FunctionCall, Message, Role, ToolCall};
use crate::session::parse_file_context;
use crate::tool_format::ToolFormat;
use crate::tool_registry::ToolRegistry;
use tracing::info;

use super::ChatEngine;

impl ChatEngine {
    pub fn push_user_message(&mut self, content: impl Into<String>) -> Vec<Message> {
        let content = content.into();
        let parsed = parse_file_context(&content);

        let mut system_messages = Vec::new();

        if self.agents.status == super::AgentModeStatus::Active {
            if let Some(prompt) = &self.agent_prompt {
                system_messages.push(Message {
                    role: Role::System,
                    content: prompt.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                    timestamp: None,
                    reasoning_content: None,
                    is_prompt: true,
                });
            }
        }

        let tool_instructions = if self.tool_format.expects_native_format() {
            None
        } else {
            self.tool_registry.tool_instructions(self.tool_format)
        };
        let env_section = self.env_context.as_ref().map(|ctx| ctx.to_prompt_section());

        if let Some(prompt) = &self.system_prompt {
            let mut full_prompt = if let Some(tool_text) = &tool_instructions {
                format!("{}\n\n{}", prompt, tool_text)
            } else {
                prompt.clone()
            };
            if let Some(env) = &env_section {
                full_prompt.push_str(env);
            }
            system_messages.push(Message {
                role: Role::System,
                content: full_prompt,
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: true,
            });
        } else if let Some(tool_text) = &tool_instructions {
            let mut full_prompt = tool_text.clone();
            if let Some(env) = &env_section {
                full_prompt.push_str(env);
            }
            system_messages.push(Message {
                role: Role::System,
                content: full_prompt,
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: true,
            });
        } else if let Some(env) = &env_section {
            system_messages.push(Message {
                role: Role::System,
                content: env.clone(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: true,
            });
        }

        if !system_messages.is_empty() {
            let already_present = system_messages.iter().enumerate().all(|(i, sys_msg)| {
                self.chat
                    .messages
                    .get(i)
                    .is_some_and(|m| m.role == Role::System && m.content == sys_msg.content)
            });

            if !already_present {
                let mut to_remove = Vec::new();
                for (i, msg) in self.chat.messages.iter().enumerate() {
                    if msg.role != Role::System {
                        break;
                    }
                    if system_messages.iter().any(|s| s.content == msg.content) {
                        to_remove.push(i);
                    } else {
                        break;
                    }
                }
                for &idx in to_remove.iter().rev() {
                    self.chat.messages.remove(idx);
                }

                for msg in &system_messages {
                    self.chat.messages.insert(0, msg.clone());
                }
            }
        }

        let mut messages = self.chat.messages.clone();

        let ts = Some(now_timestamp());
        messages.push(Message {
            role: Role::User,
            content: parsed.clone(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: ts.clone(),
            reasoning_content: None,
            is_prompt: false,
        });

        self.chat.messages.push(Message {
            role: Role::User,
            content: parsed,
            tool_calls: None,
            tool_call_id: None,
            timestamp: ts.clone(),
            reasoning_content: None,
            is_prompt: false,
        });
        self.chat.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: ts,
            reasoning_content: None,
            is_prompt: false,
        });
        self.chat.streaming = true;
        self.chat.auto_scroll = true;
        self.tool_executor.clear_pending();
        messages
    }

    pub fn append_stream_chunk(&mut self, chunk: &str) {
        if let Some(last) = self.chat.messages.last_mut() {
            last.content.push_str(chunk);
            Self::strip_think_tags(&mut last.content);
        }
        self.chat.auto_scroll = true;
    }

    pub fn sanitize_assistant_content(&mut self) {
        if let Some(last) = self.chat.messages.last_mut() {
            if last.role != Role::Assistant {
                return;
            }
            Self::strip_think_tags(&mut last.content);

            match self.tool_format {
                ToolFormat::Native | ToolFormat::None => {
                    // No content parsing needed for native or no-tool formats.
                }
                ToolFormat::ContentJson => {
                    let content = &last.content;
                    let trimmed = content.trim();

                    let json_text = if trimmed.starts_with("```") {
                        trimmed
                            .trim_start_matches("```json")
                            .trim_start_matches("```")
                            .trim_end_matches("```")
                            .trim()
                    } else {
                        trimmed
                    };

                    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(json_text) {
                        if let Some(thought) = json_val.get("thought").and_then(|v| v.as_str()) {
                            info!("Sanitizer extracted thought: {} chars", thought.len());
                            last.content = thought.to_string();
                            return;
                        }

                        let mut found_tools = false;

                        if let Some(arr) = json_val.as_array() {
                            info!(
                                "Sanitizer detected JSON array with {} items, checking for tool calls",
                                arr.len()
                            );
                            for item in arr {
                                if Self::extract_tool_call_from_json(
                                    item,
                                    &self.tool_registry,
                                    &mut self.tool_executor,
                                ) {
                                    found_tools = true;
                                }
                            }
                        }

                        if json_val.is_object() {
                            info!("Sanitizer detected JSON object, checking for tool call");
                            if Self::extract_tool_call_from_json(
                                &json_val,
                                &self.tool_registry,
                                &mut self.tool_executor,
                            ) {
                                found_tools = true;
                            }
                        }

                        if found_tools {
                            let tool_count = self.tool_executor.pending_tool_calls().len();
                            info!(
                                "Sanitizer extracted {} tool calls from content JSON",
                                tool_count
                            );
                            last.content = String::new();
                            last.tool_calls =
                                Some(self.tool_executor.pending_tool_calls().to_vec());
                        }
                    }
                }
                ToolFormat::Xml => {
                    let content = last.content.clone();
                    if Self::extract_tool_calls_from_xml(
                        &content,
                        &self.tool_registry,
                        &mut self.tool_executor,
                    ) {
                        let tool_count = self.tool_executor.pending_tool_calls().len();
                        info!(
                            "Sanitizer extracted {} tool calls from XML content",
                            tool_count
                        );
                        last.content = String::new();
                        last.tool_calls = Some(self.tool_executor.pending_tool_calls().to_vec());
                    }
                }
            }
        }
    }

    fn extract_tool_call_from_json(
        item: &serde_json::Value,
        registry: &ToolRegistry,
        executor: &mut ToolExecutor,
    ) -> bool {
        if let (Some(name), Some(args)) = (
            item.get("name").and_then(|v| v.as_str()),
            item.get("arguments").or_else(|| item.get("args")),
        ) {
            if !registry.has_tool(name) {
                return false;
            }
            let args_str = serde_json::to_string(args).unwrap_or_default();
            executor.add_tool_call(ToolCall {
                id: format!(
                    "extracted_{}_{}",
                    executor.pending_tool_calls().len(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: name.to_string(),
                    arguments: args_str,
                },
            });
            return true;
        }

        if let (Some(name), Some(args_str)) = (
            item.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str()),
            item.get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str()),
        ) {
            if !registry.has_tool(name) {
                return false;
            }
            executor.add_tool_call(ToolCall {
                id: format!(
                    "extracted_{}_{}",
                    executor.pending_tool_calls().len(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: name.to_string(),
                    arguments: args_str.to_string(),
                },
            });
            return true;
        }

        false
    }

    fn extract_tool_calls_from_xml(
        content: &str,
        registry: &ToolRegistry,
        executor: &mut ToolExecutor,
    ) -> bool {
        let mut found = false;
        let mut rest = content;

        while let Some(start) = rest.find("<tool_call>") {
            let after_start = &rest[start + "<tool_call>".len()..];
            if let Some(end) = after_start.find("</tool_call>") {
                let inner = &after_start[..end];
                rest = &after_start[end + "</tool_call>".len()..];

                // Try to parse inner content as JSON first.
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(inner.trim()) {
                    if Self::extract_tool_call_from_json(&json_val, registry, executor) {
                        found = true;
                        continue;
                    }
                }

                // Fall back to XML tag extraction.
                if let Some(name) = Self::extract_xml_tag_content(inner, "name") {
                    if let Some(args) = Self::extract_xml_tag_content(inner, "arguments") {
                        if registry.has_tool(&name) {
                            executor.add_tool_call(ToolCall {
                                id: format!(
                                    "xml_{}_{}",
                                    executor.pending_tool_calls().len(),
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                ),
                                call_type: "function".to_string(),
                                function: FunctionCall {
                                    name,
                                    arguments: args,
                                },
                            });
                            found = true;
                        }
                    }
                }
            } else {
                break;
            }
        }

        found
    }

    fn extract_xml_tag_content(xml: &str, tag: &str) -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = xml.find(&open)? + open.len();
        let after_open = &xml[start..];
        let end = after_open.find(&close)?;
        Some(after_open[..end].trim().to_string())
    }

    pub fn append_reasoning_chunk(&mut self, chunk: &str) {
        if let Some(last) = self.chat.messages.last_mut() {
            if last.reasoning_content.is_none() {
                last.reasoning_content = Some(String::new());
            }
            if let Some(ref mut reasoning) = last.reasoning_content {
                reasoning.push_str(chunk);
            }
        }
        self.chat.auto_scroll = true;
    }

    pub fn add_tool_call(&mut self, tool_call: ToolCall) {
        self.tool_executor.add_tool_call(tool_call);
    }

    pub fn finish_stream(&mut self) {
        self.chat.streaming = false;
        if !self.tool_executor.pending_tool_calls().is_empty() {
            if let Some(last) = self.chat.messages.last_mut() {
                if last.role == Role::Assistant {
                    last.tool_calls = Some(self.tool_executor.pending_tool_calls().to_vec());
                }
            }
        }
    }

    pub fn cancel_stream(&mut self) {
        self.chat.cancel_token.cancel();
        self.finish_stream();
    }

    pub fn add_system_message(&mut self, content: impl Into<String>) {
        self.chat.messages.push(Message {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some(now_timestamp()),
            reasoning_content: None,
            is_prompt: false,
        });
    }

    pub fn add_error_message(&mut self, error: impl Into<String>) {
        self.chat.messages.push(Message {
            role: Role::System,
            content: error.into(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some(now_timestamp()),
            reasoning_content: None,
            is_prompt: false,
        });
    }

    pub fn clear_messages(&mut self) {
        self.chat.messages.clear();
        self.chat.scroll = 0;
    }

    pub fn compact_context(&mut self) -> (usize, usize) {
        let before = self.chat.messages.len();
        let compacted = self.chat.compactor.compact(&self.chat.messages);
        let after = compacted.len();
        if after < before {
            self.chat.messages = compacted;
        }
        (before, after)
    }

    pub fn strip_think_tags(text: &mut String) {
        loop {
            let start = text.find("<think>");
            let end = text.rfind("</think>");
            match (start, end) {
                (Some(s), Some(e)) if s < e => {
                    text.replace_range(s..=e + 7, "");
                }
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{Message, Role};
    use crate::engine::{AgentModeStatus, ChatEngine};
    use crate::tool_format::ToolFormat;

    #[test]
    fn push_user_message_adds_messages() {
        let mut engine = ChatEngine::new();
        let messages = engine.push_user_message("Hello");
        assert_eq!(engine.chat.messages.len(), 2);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(engine.chat.messages[0].role, Role::User);
        assert_eq!(engine.chat.messages[1].role, Role::Assistant);
    }

    #[test]
    fn system_prompt_inserted_once() {
        let mut engine = ChatEngine::new().with_system_prompt("You are helpful");
        let messages1 = engine.push_user_message("Hello");
        assert!(messages1.iter().any(|m| m.role == Role::System));
        let messages2 = engine.push_user_message("Again");
        assert_eq!(
            messages2.iter().filter(|m| m.role == Role::System).count(),
            1
        );
    }

    #[test]
    fn system_prompt_persisted_in_chat_messages() {
        let mut engine = ChatEngine::new().with_system_prompt("You are helpful");
        let _messages = engine.push_user_message("Hello");

        assert_eq!(engine.chat.messages[0].role, Role::System);
        assert_eq!(engine.chat.messages[0].content, "You are helpful");
    }

    #[test]
    fn agent_prompt_persisted_in_chat_messages() {
        let mut engine = ChatEngine::new()
            .with_system_prompt("You are helpful")
            .with_agent_prompt("You are an agent");
        engine.agents.status = AgentModeStatus::Active;

        let _messages = engine.push_user_message("Hello");

        assert_eq!(engine.chat.messages[0].role, Role::System);
        assert_eq!(engine.chat.messages[0].content, "You are helpful");
        assert_eq!(engine.chat.messages[1].role, Role::System);
        assert_eq!(engine.chat.messages[1].content, "You are an agent");
    }

    #[test]
    fn no_duplicate_system_prompts_on_subsequent_messages() {
        let mut engine = ChatEngine::new().with_system_prompt("You are helpful");
        let _ = engine.push_user_message("Hello");
        let _ = engine.push_user_message("Again");

        let system_count = engine
            .chat
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .count();
        assert_eq!(system_count, 1);
        assert_eq!(engine.chat.messages[0].content, "You are helpful");
    }

    #[test]
    fn streaming_message_appended() {
        let mut engine = ChatEngine::new();
        engine.push_user_message("Hello");
        engine.append_stream_chunk("world");
        let last = engine.chat.messages.last().unwrap();
        assert_eq!(last.content, "world");
        assert!(engine.chat.streaming);
    }

    #[test]
    fn finish_stream_sets_tool_calls() {
        let mut engine = ChatEngine::new();
        engine.push_user_message("test");
        engine.tool_executor.add_tool_call(crate::domain::ToolCall {
            id: "t1".to_string(),
            call_type: "function".to_string(),
            function: crate::domain::FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        });
        engine.finish_stream();
        assert!(!engine.chat.streaming);
        let last = engine.chat.messages.last().unwrap();
        assert!(last.tool_calls.is_some());
    }

    #[test]
    fn sanitize_extracts_tool_call_from_json_content() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::ContentJson);
        engine.push_user_message("Read the file");
        engine.append_stream_chunk(
            r#"{
  "name": "read_file",
  "arguments": {
    "path": "/tmp/test.txt"
  }
}"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(
            last.content.is_empty(),
            "content should be cleared, got: {:?}",
            last.content
        );
        assert!(last.tool_calls.is_some(), "tool_calls should be present");
        let tcs = last.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "read_file");
        assert!(tcs[0].function.arguments.contains("/tmp/test.txt"));
    }

    #[test]
    fn sanitize_ignores_unknown_tool_names() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::ContentJson);
        engine.push_user_message("Explain this JSON");
        engine.append_stream_chunk(
            r#"{
  "name": "calculate",
  "arguments": {
    "expression": "15 + 27"
  }
}"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(
            !last.content.is_empty(),
            "content should NOT be cleared for unknown tool"
        );
        assert!(
            last.tool_calls.is_none(),
            "tool_calls should NOT be present for unknown tool"
        );
    }

    #[test]
    fn sanitize_extracts_tool_call_from_xml_content() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::Xml);
        engine.push_user_message("Read the file");
        engine.append_stream_chunk(
            r#"<tool_call>
<name>read_file</name>
<arguments>{"path": "/tmp/test.txt"}</arguments>
</tool_call>"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(
            last.content.is_empty(),
            "content should be cleared, got: {:?}",
            last.content
        );
        assert!(last.tool_calls.is_some(), "tool_calls should be present");
        let tcs = last.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "read_file");
        assert!(tcs[0].function.arguments.contains("/tmp/test.txt"));
    }

    #[test]
    fn sanitize_xml_ignores_unknown_tool_names() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::Xml);
        engine.push_user_message("Explain this XML");
        engine.append_stream_chunk(
            r#"<tool_call>
<name>calculate</name>
<arguments>{"expression": "15 + 27"}</arguments>
</tool_call>"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(
            !last.content.is_empty(),
            "content should NOT be cleared for unknown tool"
        );
        assert!(
            last.tool_calls.is_none(),
            "tool_calls should NOT be present for unknown tool"
        );
    }

    #[test]
    fn sanitize_xml_fallback_to_json_in_inner_content() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::Xml);
        engine.push_user_message("Run a tool");
        engine.append_stream_chunk(
            r#"<tool_call>
{"name": "read_file", "arguments": {"path": "/tmp/test.txt"}}
</tool_call>"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(last.content.is_empty());
        assert!(last.tool_calls.is_some());
        let tcs = last.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].function.name, "read_file");
    }

    #[test]
    fn sanitize_native_does_not_parse_content() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::Native);
        engine.push_user_message("Read the file");
        engine.append_stream_chunk(
            r#"{"name": "read_file", "arguments": {"path": "/tmp/test.txt"}}"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(
            !last.content.is_empty(),
            "content should NOT be parsed in Native mode"
        );
        assert!(last.tool_calls.is_none());
    }

    #[test]
    fn sanitize_none_does_not_parse_content() {
        let mut engine = ChatEngine::new();
        engine.set_tool_format(ToolFormat::None);
        engine.push_user_message("Read the file");
        engine.append_stream_chunk(
            r#"{"name": "read_file", "arguments": {"path": "/tmp/test.txt"}}"#,
        );
        engine.sanitize_assistant_content();

        let last = engine.chat.messages.last().unwrap();
        assert!(
            !last.content.is_empty(),
            "content should NOT be parsed in None mode"
        );
        assert!(last.tool_calls.is_none());
    }

    #[test]
    fn compact_context_reduces_messages() {
        let mut engine = ChatEngine::new();
        for i in 0..10 {
            engine.chat.messages.push(Message {
                role: Role::User,
                content: format!("msg {}", i),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            });
        }
        let (before, after) = engine.compact_context();
        assert_eq!(before, 10);
        assert!(after <= before);
    }

    #[test]
    fn cancel_stream_stops_streaming() {
        let mut engine = ChatEngine::new();
        engine.push_user_message("test");
        assert!(engine.chat.streaming);
        engine.cancel_stream();
        assert!(!engine.chat.streaming);
    }

    #[test]
    fn add_system_and_error_messages() {
        let mut engine = ChatEngine::new();
        engine.add_system_message("sys");
        engine.add_error_message("err");
        assert_eq!(engine.chat.messages.len(), 2);
        assert_eq!(engine.chat.messages[0].content, "sys");
        assert_eq!(engine.chat.messages[1].content, "err");
    }

    #[test]
    fn strip_think_tags_removes_blocks() {
        let mut text = "before <think>thinking</think> after".to_string();
        ChatEngine::strip_think_tags(&mut text);
        assert_eq!(text, "before  after");
    }

    #[test]
    fn strip_think_tags_handles_nested() {
        let mut text = "a <think>b <think>c</think> d</think> e".to_string();
        ChatEngine::strip_think_tags(&mut text);
        assert_eq!(text, "a  e");
    }

    #[test]
    fn strip_think_tags_noop_when_missing() {
        let mut text = "no think tags here".to_string();
        ChatEngine::strip_think_tags(&mut text);
        assert_eq!(text, "no think tags here");
    }
}
