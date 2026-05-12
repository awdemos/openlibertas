//! Chat engine: state management for chat, input, agents, and tool orchestration.
//!
//! Core types:
//! - `ChatEngine` — owns chat, input, agent, and tool state
//! - `ChatState` — messages, scroll, streaming, spinner
//! - `InputState` — buffer, cursor, history, autocomplete, selection
//! - `AgentState` — status, iteration count, persona, yolo mode

use crate::conversation::{parse_file_context, ContextCompactor};
use crate::domain::{now_timestamp, Role};
use crate::domain::{FunctionCall, Message, ToolCall, ToolDefinition};
use crate::env_context::EnvContext;
use crate::history::HistoryStore;
use crate::mcp::McpClient;
use crate::tool_format::ToolFormat;
use crate::tool_registry::ToolRegistry;
use tracing::{info, warn};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::completion::{CompletionEngine, CompletionItem, CompletionType};
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AgentStatus {
    #[default]
    Disabled,
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentMode {
    Auto,
    Plan,
}

impl Default for AgentMode {
    fn default() -> Self {
        AgentMode::Auto
    }
}

#[derive(Debug, PartialEq)]
pub struct AgentState {
    pub status: AgentStatus,
    pub max_iterations: usize,
    pub current_iteration: usize,
    pub persona: String,
    pub yolo_mode: bool,
    pub mode: AgentMode,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            status: AgentStatus::Disabled,
            max_iterations: 10,
            current_iteration: 0,
            persona: "Orchestrator".to_string(),
            yolo_mode: false,
            mode: AgentMode::Auto,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct InputState {
    pub buffer: String,
    pub cursor_pos: usize,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub history_stash: String,
    pub autocomplete_index: usize,
    pub show_autocomplete: bool,
    /// Selection anchor (start of selection). When `selecting` is true,
    /// this is the fixed end; `cursor_pos` is the moving end.
    pub selection_anchor: Option<usize>,
    /// Horizontal scroll offset for the input viewport
    pub scroll_offset: usize,
    /// Current completion candidates
    pub completion_items: Vec<CompletionItem>,
    /// Whether the completion popup is currently visible
    pub completion_active: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            buffer: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_index: None,
            history_stash: String::new(),
            autocomplete_index: 0,
            show_autocomplete: false,
            selection_anchor: None,
            scroll_offset: 0,
            completion_items: Vec::new(),
            completion_active: false,
        }
    }
}
pub struct ChatState {
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub streaming: bool,
    pub auto_scroll: bool,
    pub cancel_token: tokio_util::sync::CancellationToken,
    pub spinner_frame: usize,
    pub compactor: ContextCompactor,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            scroll: 0,
            streaming: false,
            auto_scroll: true,
            cancel_token: tokio_util::sync::CancellationToken::new(),
            spinner_frame: 0,
            compactor: ContextCompactor::default(),
        }
    }
}

pub struct ChatEngine {
    chat: ChatState,
    input: InputState,
    agents: AgentState,
    tools: ToolRegistry,
    system_prompt: Option<String>,
    agent_prompt: Option<String>,
    env_context: Option<EnvContext>,
    tool_format: ToolFormat,
    completion_engine: CompletionEngine,
    history_store: Option<HistoryStore>,
}

impl Default for ChatEngine {
    fn default() -> Self {
        Self {
            chat: ChatState::default(),
            input: InputState::default(),
            agents: AgentState::default(),
            tools: ToolRegistry::default(),
            system_prompt: None,
            agent_prompt: None,
            env_context: None,
            tool_format: ToolFormat::Native,
            completion_engine: CompletionEngine::new(),
            history_store: None,
        }
    }
}

impl ChatEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_agent_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.agent_prompt = Some(prompt.into());
        self
    }

    pub fn with_env_context(mut self, ctx: EnvContext) -> Self {
        self.env_context = Some(ctx);
        self
    }
    pub fn with_history_store(mut self, store: HistoryStore) -> Self {
        self.history_store = Some(store);
        self
    }

    pub fn load_history(&mut self) {
        if let Some(ref store) = self.history_store {
            let entries = store.load();
            self.input.history.extend(entries);
        }
    }

    pub fn chat(&self) -> &ChatState {
        &self.chat
    }

    pub fn chat_mut(&mut self) -> &mut ChatState {
        &mut self.chat
    }

    pub fn input(&self) -> &InputState {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut InputState {
        &mut self.input
    }

    pub fn agents(&self) -> &AgentState {
        &self.agents
    }

    pub fn agents_mut(&mut self) -> &mut AgentState {
        &mut self.agents
    }

    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    pub fn agent_prompt(&self) -> Option<&str> {
        self.agent_prompt.as_deref()
    }

    pub fn set_agent_prompt(&mut self, prompt: impl Into<String>) {
        self.agent_prompt = Some(prompt.into());
    }

    pub fn set_tool_format(&mut self, tool_format: ToolFormat) {
        self.tool_format = tool_format;
    }

    pub fn env_context(&self) -> Option<&EnvContext> {
        self.env_context.as_ref()
    }

    pub fn set_plan_mode(&mut self, mode: AgentMode) {
        self.agents.mode = mode;
    }

    pub fn plan_mode(&self) -> AgentMode {
        self.agents.mode
    }

    pub fn set_env_context(&mut self, ctx: EnvContext) {
        self.env_context = Some(ctx);
    }

    pub fn switch_persona(&mut self, persona: impl Into<String>, prompt: impl Into<String>) {
        let persona = persona.into();
        let prompt = prompt.into();
        self.agents.persona = persona.clone();
        self.agent_prompt = Some(prompt);
        self.agents.current_iteration = 0;
    }

    pub fn with_mcp_client(mut self, client: McpClient) -> Self {
        self.tools = self.tools.with_client(client);
        self
    }

    // -- Message lifecycle --

    pub fn push_user_message(&mut self, content: impl Into<String>) -> Vec<Message> {
        let content = content.into();
        let parsed = parse_file_context(&content);

        let mut system_messages = Vec::new();

        if self.agents.status == AgentStatus::Active {
            if let Some(prompt) = &self.agent_prompt {
                system_messages.push(Message {
                    role: Role::System,
                    content: prompt.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                    timestamp: None,
                    reasoning_content: None,
                });
            }
        }

        let tool_instructions = if self.tool_format.expects_native_format() {
            None
        } else {
            self.tools.tool_instructions(self.tool_format)
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
            });
        } else if let Some(env) = &env_section {
            system_messages.push(Message {
                role: Role::System,
                content: env.clone(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
            });
        }

        if !system_messages.is_empty() {
            let already_present = system_messages.iter().enumerate().all(|(i, sys_msg)| {
                self.chat.messages.get(i).map_or(false, |m| {
                    m.role == Role::System && m.content == sys_msg.content
                })
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
        });

        self.chat.messages.push(Message {
            role: Role::User,
            content: parsed,
            tool_calls: None,
            tool_call_id: None,
            timestamp: ts.clone(),
            reasoning_content: None,
        });
        self.chat.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: ts,
            reasoning_content: None,
        });
        self.chat.streaming = true;
        self.chat.auto_scroll = true;
        self.tools.clear_pending();
        messages
    }

    pub fn append_stream_chunk(&mut self, chunk: &str) {
        if let Some(last) = self.chat.messages.last_mut() {
            last.content.push_str(chunk);
            Self::strip_think_tags(&mut last.content);
        }
        self.chat.auto_scroll = true;
    }

    fn strip_think_tags(text: &mut String) {
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

    pub fn sanitize_assistant_content(&mut self) {
        if let Some(last) = self.chat.messages.last_mut() {
            if last.role != Role::Assistant {
                return;
            }
            Self::strip_think_tags(&mut last.content);

            match self.tool_format {
                ToolFormat::Native | ToolFormat::None => {
                    // No content parsing needed for native or no-tool formats.
                    return;
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
                                if Self::extract_tool_call_from_json(item, &mut self.tools) {
                                    found_tools = true;
                                }
                            }
                        }

                        if json_val.is_object() {
                            info!("Sanitizer detected JSON object, checking for tool call");
                            if Self::extract_tool_call_from_json(&json_val, &mut self.tools) {
                                found_tools = true;
                            }
                        }

                        if found_tools {
                            let tool_count = self.tools.pending_tool_calls().len();
                            info!(
                                "Sanitizer extracted {} tool calls from content JSON",
                                tool_count
                            );
                            last.content = String::new();
                            last.tool_calls = Some(self.tools.pending_tool_calls().to_vec());
                        }
                    }
                }
                ToolFormat::Xml => {
                    let content = last.content.clone();
                    if Self::extract_tool_calls_from_xml(&content, &mut self.tools) {
                        let tool_count = self.tools.pending_tool_calls().len();
                        info!(
                            "Sanitizer extracted {} tool calls from XML content",
                            tool_count
                        );
                        last.content = String::new();
                        last.tool_calls = Some(self.tools.pending_tool_calls().to_vec());
                    }
                }
            }
        }
    }

    fn extract_tool_call_from_json(item: &serde_json::Value, tools: &mut ToolRegistry) -> bool {
        if let (Some(name), Some(args)) = (
            item.get("name").and_then(|v| v.as_str()),
            item.get("arguments").or_else(|| item.get("args")),
        ) {
            if !tools.has_tool(name) {
                return false;
            }
            let args_str = serde_json::to_string(args).unwrap_or_default();
            tools.add_tool_call(ToolCall {
                id: format!(
                    "extracted_{}_{}",
                    tools.pending_tool_calls().len(),
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
            if !tools.has_tool(name) {
                return false;
            }
            tools.add_tool_call(ToolCall {
                id: format!(
                    "extracted_{}_{}",
                    tools.pending_tool_calls().len(),
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

    fn extract_tool_calls_from_xml(content: &str, tools: &mut ToolRegistry) -> bool {
        let mut found = false;
        let mut rest = content;

        while let Some(start) = rest.find("<tool_call>") {
            let after_start = &rest[start + "<tool_call>".len()..];
            if let Some(end) = after_start.find("</tool_call>") {
                let inner = &after_start[..end];
                rest = &after_start[end + "</tool_call>".len()..];

                // Try to parse inner content as JSON first.
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(inner.trim()) {
                    if Self::extract_tool_call_from_json(&json_val, tools) {
                        found = true;
                        continue;
                    }
                }

                // Fall back to XML tag extraction.
                if let Some(name) = Self::extract_xml_tag_content(inner, "name") {
                    if let Some(args) = Self::extract_xml_tag_content(inner, "arguments") {
                        if tools.has_tool(&name) {
                            tools.add_tool_call(ToolCall {
                                id: format!(
                                    "xml_{}_{}",
                                    tools.pending_tool_calls().len(),
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
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
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
        self.tools.add_tool_call(tool_call);
    }

    pub fn finish_stream(&mut self) {
        self.chat.streaming = false;
        if !self.tools.pending_tool_calls().is_empty() {
            if let Some(last) = self.chat.messages.last_mut() {
                if last.role == Role::Assistant {
                    last.tool_calls = Some(self.tools.pending_tool_calls().to_vec());
                }
            }
        }
    }

    pub fn cancel_stream(&mut self) {
        self.chat.cancel_token.cancel();
        self.finish_stream();
    }

    // -- Agent loop --

    pub fn start_agent_loop(&mut self) {
        if self.agents.status == AgentStatus::Idle {
            self.agents.status = AgentStatus::Active;
            self.agents.current_iteration = 0;
        }
    }

    pub fn finish_agent_loop(&mut self) {
        if self.agents.status == AgentStatus::Active {
            self.agents.status = AgentStatus::Idle;
            self.agents.current_iteration = 0;
        }
    }

    pub fn agent_iteration_exceeded(&self) -> bool {
        self.agents.status == AgentStatus::Active
            && self.agents.current_iteration >= self.agents.max_iterations
    }

    pub fn increment_agent_iteration(&mut self) {
        self.agents.current_iteration += 1;
    }

    pub fn isolate_session(&mut self) {
        self.chat.messages.retain(|m| m.role == Role::System);
        self.chat.scroll = 0;
        self.chat.streaming = false;
    }

    // -- Tool execution --

    pub fn tools_for_request(&self) -> Option<Vec<ToolDefinition>> {
        let plan_mode = self.agents.mode == AgentMode::Plan;
        self.tools.tools_for_request(plan_mode)
    }

    /// Assemble tool result messages with agent context.
    /// Delegates to ToolRegistry::assemble_messages_with_agent_context.
    pub fn assemble_tool_result_messages(&self) -> Vec<Message> {
        let agent_prompt = if self.agents.status == AgentStatus::Active {
            self.agent_prompt.as_deref()
        } else {
            None
        };
        self.tools.assemble_messages_with_agent_context(
            &self.chat.messages,
            Some("active").filter(|_| self.agents.status == AgentStatus::Active),
            agent_prompt,
        )
    }

    pub async fn execute_pending_tools(&mut self) -> Vec<crate::domain::ToolExecutionResult> {
        let results = self
            .tools
            .execute_pending_tools(self.agents.yolo_mode)
            .await;

        for msg in self.tools.create_tool_result_messages() {
            self.chat.messages.push(msg);
        }

        results
    }

    // -- Input helpers --

    pub fn push_to_history(&mut self, input: String) {
        if input.trim().is_empty() {
            self.input.history_index = None;
            self.input.history_stash.clear();
            return;
        }
        // Deduplication: skip consecutive duplicates
        if self.input.history.last() == Some(&input) {
            self.input.history_index = None;
            self.input.history_stash.clear();
            return;
        }
        self.input.history.push(input.clone());
        self.input.history_index = None;
        self.input.history_stash.clear();

        // Persist asynchronously to avoid blocking the UI thread
        if let Some(ref store) = self.history_store {
            let store = store.clone();
            std::thread::spawn(move || {
                if let Err(e) = store.append(&input) {
                    warn!("Failed to append history entry: {}", e);
                }
            });
        }
    }

    pub fn history_prev(&mut self) {
        if self.input.history.is_empty() {
            return;
        }
        self.input.selection_anchor = None;
        if self.input.history_index.is_none() {
            self.input.history_stash = self.input.buffer.clone();
            self.input.history_index = Some(self.input.history.len() - 1);
        } else if let Some(idx) = self.input.history_index {
            if idx > 0 {
                self.input.history_index = Some(idx - 1);
            }
        }
        if let Some(idx) = self.input.history_index {
            self.input.buffer = self.input.history[idx].clone();
            self.input.cursor_pos = self.input.buffer.len();
        }
    }

    pub fn history_next(&mut self) {
        if self.input.history.is_empty() {
            return;
        }
        self.input.selection_anchor = None;
        if let Some(idx) = self.input.history_index {
            if idx + 1 < self.input.history.len() {
                self.input.history_index = Some(idx + 1);
                self.input.buffer = self.input.history[idx + 1].clone();
            } else {
                self.input.history_index = None;
                self.input.buffer = self.input.history_stash.clone();
            }
            self.input.cursor_pos = self.input.buffer.len();
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.input.selection_anchor = None;
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        self.input.cursor_pos = self.input.buffer[..pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    pub fn move_cursor_right(&mut self) {
        self.input.selection_anchor = None;
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        self.input.cursor_pos = self.input.buffer[pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| pos + i)
            .unwrap_or(self.input.buffer.len());
    }

    pub fn move_cursor_to_start(&mut self) {
        self.input.selection_anchor = None;
        self.input.cursor_pos = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.input.selection_anchor = None;
        self.input.cursor_pos = self.input.buffer.len();
    }

    pub fn delete_word_backward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.input.cursor_pos == 0 {
            return;
        }
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        let before = &self.input.buffer[..safe_pos];
        let mut chars = before.char_indices().rev().peekable();
        while let Some((_, ch)) = chars.peek() {
            if !ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        while let Some((_, ch)) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        let pos = chars.next().map(|(i, ch)| i + ch.len_utf8()).unwrap_or(0);
        self.input.buffer.replace_range(pos..safe_pos, "");
        self.input.cursor_pos = pos;
    }

    pub fn insert_char(&mut self, c: char) {
        if self.has_selection() {
            self.delete_selection();
        }
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        self.input.buffer.insert(safe_pos, c);
        self.input.cursor_pos = safe_pos + c.len_utf8();
        self.input.show_autocomplete = false;
        self.input.autocomplete_index = 0;
        self.input.history_index = None;
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.input.cursor_pos > 0 {
            let pos = self.input.cursor_pos.min(self.input.buffer.len());
            let safe_pos = pos;
            let prev = self.input.buffer[..safe_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.buffer.remove(prev);
            self.input.cursor_pos = prev;
        }
        self.input.show_autocomplete = false;
        self.input.autocomplete_index = 0;
    }

    pub fn clear_input(&mut self) {
        self.input.buffer.clear();
        self.input.cursor_pos = 0;
        self.input.selection_anchor = None;
        self.input.scroll_offset = 0;
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.input.selection_anchor?;
        let cursor = self.input.cursor_pos.min(self.input.buffer.len());
        let start = anchor.min(cursor);
        let end = anchor.max(cursor);
        if start == end {
            None
        } else {
            Some((start, end))
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    pub fn select_all(&mut self) {
        self.input.selection_anchor = Some(0);
        self.input.cursor_pos = self.input.buffer.len();
    }

    pub fn clear_selection(&mut self) {
        self.input.selection_anchor = None;
    }

    pub fn start_selection(&mut self) {
        self.input.selection_anchor = Some(self.input.cursor_pos);
    }

    pub fn extend_selection_left(&mut self) {
        if self.input.selection_anchor.is_none() {
            self.input.selection_anchor = Some(self.input.cursor_pos);
        }
        self.move_cursor_left();
    }

    pub fn extend_selection_right(&mut self) {
        if self.input.selection_anchor.is_none() {
            self.input.selection_anchor = Some(self.input.cursor_pos);
        }
        self.move_cursor_right();
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection()
            .map(|(start, end)| self.input.buffer[start..end].to_string())
    }

    pub fn delete_selection(&mut self) -> Option<String> {
        let (start, end) = self.selection()?;
        let text = self.input.buffer[start..end].to_string();
        self.input.buffer.replace_range(start..end, "");
        self.input.cursor_pos = start;
        self.input.selection_anchor = None;
        Some(text)
    }

    pub fn move_cursor_word_left(&mut self) {
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        let before = &self.input.buffer[..safe_pos];
        let mut chars = before.char_indices().rev().peekable();
        while let Some((_, ch)) = chars.peek() {
            if !ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        while let Some((_, ch)) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        self.input.cursor_pos = chars.next().map(|(i, ch)| i + ch.len_utf8()).unwrap_or(0);
    }

    pub fn move_cursor_word_right(&mut self) {
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        let after = &self.input.buffer[safe_pos..];
        let mut chars = after.char_indices().peekable();
        while let Some((_, ch)) = chars.peek() {
            if !ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        while let Some((_, ch)) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        self.input.cursor_pos = chars
            .next()
            .map(|(i, _)| safe_pos + i)
            .unwrap_or(self.input.buffer.len());
    }

    // -- Clipboard --

    pub fn copy_selection(&mut self) -> Option<String> {
        self.selected_text()
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        self.delete_selection()
    }

    pub fn ensure_cursor_visible(&mut self, viewport_width: usize, prompt_width: usize) {
        let text_before_cursor =
            &self.input.buffer[..self.input.cursor_pos.min(self.input.buffer.len())];
        let cursor_display_pos = text_before_cursor.width() + prompt_width;
        if cursor_display_pos < self.input.scroll_offset {
            self.input.scroll_offset = cursor_display_pos.saturating_sub(1);
        } else if cursor_display_pos >= self.input.scroll_offset + viewport_width {
            self.input.scroll_offset = cursor_display_pos
                .saturating_sub(viewport_width)
                .saturating_add(1);
        }
    }

    pub fn set_cursor_from_click(&mut self, click_x: usize, prompt_width: usize) {
        let text_x = click_x.saturating_sub(prompt_width);
        let mut accumulated_width = 0;
        let mut byte_pos = 0;
        for (i, ch) in self.input.buffer.char_indices() {
            let ch_width = ch.width().unwrap_or(0);
            if accumulated_width + ch_width / 2 > text_x {
                break;
            }
            accumulated_width += ch_width;
            byte_pos = i + ch.len_utf8();
        }
        self.input.cursor_pos = byte_pos.min(self.input.buffer.len());
    }

    pub fn advance_spinner(&mut self) {
        if self.chat.streaming {
            self.chat.spinner_frame = (self.chat.spinner_frame + 1) % SPINNER_FRAMES.len();
        }
    }

    pub fn add_system_message(&mut self, content: impl Into<String>) {
        self.chat.messages.push(Message {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some(now_timestamp()),
            reasoning_content: None,
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
        });
    }

    pub fn scroll_page_up(&mut self) {
        self.chat.scroll = self.chat.scroll.saturating_sub(10);
        self.chat.auto_scroll = false;
    }

    pub fn scroll_page_down(&mut self) {
        self.chat.scroll += 10;
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

    pub fn message_at_y(&self, y: usize, viewport_width: usize) -> Option<usize> {
        let scroll = self.chat.scroll;
        let mut line = 0usize;
        for (idx, msg) in self.chat.messages.iter().enumerate() {
            if let Some(ref tool_calls) = msg.tool_calls {
                let mut tool_lines = 0;
                for tc in tool_calls {
                    tool_lines += 1;
                    let key_arg = crate::tool_registry::extract_key_argument(
                        &tc.function.name,
                        &tc.function.arguments,
                    );
                    if key_arg.is_empty() {
                        tool_lines += 1;
                    }
                }
                tool_lines += 1;
                if line + tool_lines > y + scroll {
                    return Some(idx);
                }
                line += tool_lines;
            } else {
                let mut msg_lines = 1;

                if msg.role == Role::Assistant {
                    if let Some(ref reasoning) = msg.reasoning_content {
                        if !reasoning.is_empty() && viewport_width > 10 {
                            msg_lines += 2;
                            msg_lines +=
                                count_wrapped_lines(reasoning, viewport_width.saturating_sub(2));
                            msg_lines += 1;
                        }
                    }
                }

                if viewport_width > 10 {
                    msg_lines += count_wrapped_lines(&msg.content, viewport_width);
                } else {
                    msg_lines += msg.content.lines().count().max(1);
                }

                let is_last = idx == self.chat.messages.len().saturating_sub(1);
                if is_last && self.chat.streaming && msg.role == Role::Assistant {
                    msg_lines += 1;
                }

                msg_lines += 1;

                if line + msg_lines > y + scroll {
                    return Some(idx);
                }
                line += msg_lines;
            }
        }
        None
    }
    // -- Completion --

    pub fn refresh_completions(&mut self, models: &[crate::domain::Model]) {
        let buffer = self.input.buffer.clone();
        let cursor = self.input.cursor_pos;
        self.input.completion_items = self.completion_engine.complete(&buffer, cursor, models);
        self.input.completion_active = !self.input.completion_items.is_empty();
        self.input.autocomplete_index = 0;
    }

    pub fn clear_completions(&mut self) {
        self.input.completion_active = false;
        self.input.completion_items.clear();
        self.input.autocomplete_index = 0;
    }

    pub fn cycle_completion_next(&mut self) {
        if !self.input.completion_items.is_empty() {
            self.input.autocomplete_index =
                (self.input.autocomplete_index + 1) % self.input.completion_items.len();
        }
    }

    pub fn cycle_completion_prev(&mut self) {
        if !self.input.completion_items.is_empty() {
            if self.input.autocomplete_index == 0 {
                self.input.autocomplete_index = self.input.completion_items.len() - 1;
            } else {
                self.input.autocomplete_index -= 1;
            }
        }
    }

    pub fn apply_completion(&mut self) -> bool {
        if !self.input.completion_active || self.input.completion_items.is_empty() {
            return false;
        }
        let idx = self.input.autocomplete_index;
        if let Some(item) = self.input.completion_items.get(idx).cloned() {
            crate::completion::accept_completion(
                &mut self.input.buffer,
                &mut self.input.cursor_pos,
                &item,
            );
            // For file paths, keep input mode active but clear completions
            // For slash commands and models, also clear
            self.input.completion_active = false;
            self.input.completion_items.clear();
            self.input.autocomplete_index = 0;
            true
        } else {
            false
        }
    }

    pub fn completion_type(&self) -> Option<CompletionType> {
        CompletionEngine::detect_completion_type(&self.input.buffer, self.input.cursor_pos)
    }
}

/// Approximate line count after markdown-style wrapping.
/// Mirrors `wrap_markdown` in openlibertas-tui/src/markdown.rs.
fn count_wrapped_lines(content: &str, width: usize) -> usize {
    if width == 0 {
        return content.lines().count().max(1);
    }

    let mut in_code = false;
    let mut paragraph_chars = 0usize;
    let mut lines = 0usize;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") {
            if paragraph_chars > 0 {
                lines += paragraph_chars.div_ceil(width);
                paragraph_chars = 0;
            }
            in_code = !in_code;
            lines += 1;
        } else if in_code {
            lines += 1;
        } else if raw_line.trim().is_empty() {
            if paragraph_chars > 0 {
                lines += paragraph_chars.div_ceil(width);
                paragraph_chars = 0;
            }
            lines += 1;
        } else {
            paragraph_chars += raw_line.trim().chars().count() + 1;
        }
    }

    if paragraph_chars > 0 {
        lines += paragraph_chars.div_ceil(width);
    }

    lines.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_starts_empty() {
        let engine = ChatEngine::new();
        assert!(engine.chat.messages.is_empty());
        assert!(!engine.chat.streaming);
    }

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
        engine.agents.status = AgentStatus::Active;

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
    fn agent_loop_transitions() {
        let mut engine = ChatEngine::new();
        engine.agents.status = AgentStatus::Idle;
        engine.start_agent_loop();
        assert_eq!(engine.agents.status, AgentStatus::Active);
        engine.finish_agent_loop();
        assert_eq!(engine.agents.status, AgentStatus::Idle);
    }

    #[test]
    fn input_history_navigation() {
        let mut engine = ChatEngine::new();
        engine.push_to_history("first".to_string());
        engine.push_to_history("second".to_string());
        engine.history_prev();
        assert_eq!(engine.input.buffer, "second");
        engine.history_prev();
        assert_eq!(engine.input.buffer, "first");
        engine.history_next();
        assert_eq!(engine.input.buffer, "second");
    }

    #[test]
    fn tool_needs_approval_detects_destructive() {
        assert!(!crate::tool_registry::tool_needs_approval("write_file"));
        assert!(crate::tool_registry::tool_needs_approval("shell"));
        assert!(!crate::tool_registry::tool_needs_approval("read_file"));
        assert!(crate::tool_registry::tool_needs_approval(
            "str_replace_file"
        ));
    }

    #[test]
    fn cursor_moves_left_and_right() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "abc".to_string();
        engine.input.cursor_pos = 3;
        engine.move_cursor_left();
        assert_eq!(engine.input.cursor_pos, 2);
        engine.move_cursor_right();
        assert_eq!(engine.input.cursor_pos, 3);
    }

    #[test]
    fn cursor_moves_to_start_and_end() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "hello".to_string();
        engine.input.cursor_pos = 3;
        engine.move_cursor_to_start();
        assert_eq!(engine.input.cursor_pos, 0);
        engine.move_cursor_to_end();
        assert_eq!(engine.input.cursor_pos, 5);
    }

    #[test]
    fn insert_char_adds_text() {
        let mut engine = ChatEngine::new();
        engine.insert_char('h');
        engine.insert_char('i');
        assert_eq!(engine.input.buffer, "hi");
        assert_eq!(engine.input.cursor_pos, 2);
    }

    #[test]
    fn backspace_removes_char() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "ab".to_string();
        engine.input.cursor_pos = 2;
        engine.backspace();
        assert_eq!(engine.input.buffer, "a");
        assert_eq!(engine.input.cursor_pos, 1);
    }

    #[test]
    fn backspace_at_start_does_nothing() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "a".to_string();
        engine.input.cursor_pos = 0;
        engine.backspace();
        assert_eq!(engine.input.buffer, "a");
    }

    #[test]
    fn selection_works() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "hello world".to_string();
        engine.input.cursor_pos = 0;
        engine.start_selection();
        engine.input.cursor_pos = 5;
        assert_eq!(engine.selection(), Some((0, 5)));
        assert!(engine.has_selection());
    }

    #[test]
    fn select_all_selects_everything() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "test".to_string();
        engine.select_all();
        assert_eq!(engine.selection(), Some((0, 4)));
    }

    #[test]
    fn delete_selection_removes_text() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "hello world".to_string();
        engine.input.cursor_pos = 0;
        engine.start_selection();
        engine.input.cursor_pos = 5;
        let deleted = engine.delete_selection();
        assert_eq!(deleted, Some("hello".to_string()));
        assert_eq!(engine.input.buffer, " world");
    }

    #[test]
    fn clear_input_resets_state() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "text".to_string();
        engine.input.cursor_pos = 2;
        engine.input.selection_anchor = Some(0);
        engine.input.scroll_offset = 5;
        engine.clear_input();
        assert_eq!(engine.input.buffer, "");
        assert_eq!(engine.input.cursor_pos, 0);
        assert!(engine.input.selection_anchor.is_none());
        assert_eq!(engine.input.scroll_offset, 0);
    }

    #[test]
    fn empty_history_prev_does_nothing() {
        let mut engine = ChatEngine::new();
        engine.history_prev();
        assert_eq!(engine.input.buffer, "");
    }

    #[test]
    fn history_next_at_end_restores_stash() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "draft".to_string();
        engine.push_to_history("first".to_string());
        engine.history_prev();
        assert_eq!(engine.input.buffer, "first");
        engine.history_next();
        assert_eq!(engine.input.buffer, "draft");
    }

    #[test]
    fn agent_loop_exceeded_check() {
        let mut engine = ChatEngine::new();
        engine.agents.status = AgentStatus::Active;
        engine.agents.max_iterations = 3;
        engine.agents.current_iteration = 3;
        assert!(engine.agent_iteration_exceeded());
        engine.agents.current_iteration = 2;
        assert!(!engine.agent_iteration_exceeded());
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
        engine.tools_mut().add_tool_call(crate::domain::ToolCall {
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
            });
        }
        let (before, after) = engine.compact_context();
        assert_eq!(before, 10);
        assert!(after <= before);
    }

    #[test]
    fn count_wrapped_lines_basic() {
        assert_eq!(count_wrapped_lines("hello", 10), 1);
        assert_eq!(count_wrapped_lines("hello world", 5), 3);
    }

    #[test]
    fn count_wrapped_lines_code_blocks() {
        let text = "```\ncode\n```";
        assert_eq!(count_wrapped_lines(text, 10), 3);
    }

    #[test]
    fn scroll_page_up_down() {
        let mut engine = ChatEngine::new();
        engine.chat.scroll = 20;
        engine.scroll_page_up();
        assert_eq!(engine.chat.scroll, 10);
        engine.scroll_page_down();
        assert_eq!(engine.chat.scroll, 20);
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
    fn spinner_advances_only_when_streaming() {
        let mut engine = ChatEngine::new();
        engine.chat.streaming = true;
        engine.advance_spinner();
        assert_eq!(engine.chat.spinner_frame, 1);
        engine.chat.streaming = false;
        let frame = engine.chat.spinner_frame;
        engine.advance_spinner();
        assert_eq!(engine.chat.spinner_frame, frame);
    }

    #[test]
    fn with_methods_chain() {
        let engine = ChatEngine::new()
            .with_system_prompt("sys")
            .with_agent_prompt("agent")
            .with_env_context(EnvContext::detect());
        assert!(engine.system_prompt.is_some());
        assert!(engine.agent_prompt.is_some());
        assert!(engine.env_context.is_some());
    }

    #[test]
    fn switch_persona_updates_state() {
        let mut engine = ChatEngine::new();
        engine.switch_persona("Research", "You are a researcher");
        assert_eq!(engine.agents.persona, "Research");
        assert_eq!(
            engine.agent_prompt,
            Some("You are a researcher".to_string())
        );
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
