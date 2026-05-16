//! Chat engine: state management for chat, input, agents, and tool orchestration.
//!
//! Core types:
//! - `ChatEngine` — owns chat, input, agent, and tool state
//! - `ChatState` — messages, scroll, streaming, spinner
//! - `InputState` — buffer, cursor, history, autocomplete, selection
//! - `AgentState` — status, iteration count, persona, yolo mode

use crate::completion::{CompletionEngine, CompletionItem};
use crate::session::ContextCompactor;
use crate::domain::{Message, Role, ToolDefinition};
use crate::env_context::EnvContext;
use crate::history::HistoryStore;
use crate::mcp::McpClient;
use std::sync::Arc;
use crate::tool_format::ToolFormat;
use crate::tool_registry::ToolRegistry;
use crate::agent_tools::{MessageAssembler, ToolExecutor};

pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AgentModeStatus {
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
    pub status: AgentModeStatus,
    pub max_iterations: usize,
    pub current_iteration: usize,
    pub persona: String,
    pub yolo_mode: bool,
    pub mode: AgentMode,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            status: AgentModeStatus::Disabled,
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

impl ChatState {
    pub fn with_context_window(context_window: usize) -> Self {
        Self {
            messages: Vec::new(),
            scroll: 0,
            streaming: false,
            auto_scroll: true,
            cancel_token: tokio_util::sync::CancellationToken::new(),
            spinner_frame: 0,
            compactor: ContextCompactor::with_context_window(context_window),
        }
    }
}

impl Default for ChatState {
    fn default() -> Self {
        Self::with_context_window(8192)
    }
}

pub struct ChatEngine {
    chat: ChatState,
    input: InputState,
    agents: AgentState,
    tool_registry: ToolRegistry,
    tool_executor: ToolExecutor,
    message_assembler: MessageAssembler,
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
            tool_registry: ToolRegistry::default(),
            tool_executor: ToolExecutor::default(),
            message_assembler: MessageAssembler::default(),
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

    pub fn with_context_window(mut self, context_window: usize) -> Self {
        self.chat = ChatState::with_context_window(context_window);
        self
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

    pub fn with_mcp_client(mut self, client: Arc<McpClient>) -> Self {
        self.tool_executor.set_client(Some(client.clone()));
        self.tool_registry.set_client(Some(client));
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
        &self.tool_registry
    }

    pub fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }

    pub fn tool_executor(&self) -> &ToolExecutor {
        &self.tool_executor
    }

    pub fn tool_executor_mut(&mut self) -> &mut ToolExecutor {
        &mut self.tool_executor
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

    pub fn set_env_context(&mut self, ctx: EnvContext) {
        self.env_context = Some(ctx);
    }

    pub fn tools_for_request(&self) -> Option<Vec<ToolDefinition>> {
        let plan_mode = self.agents.mode == AgentMode::Plan;
        self.tool_registry.definitions_for_api(plan_mode)
    }

    pub fn assemble_tool_result_messages(&self) -> Vec<Message> {
        let agent_prompt = if self.agents.status == AgentModeStatus::Active {
            self.agent_prompt.as_deref()
        } else {
            None
        };
        self.message_assembler.assemble(
            &self.chat.messages,
            Some("active").filter(|_| self.agents.status == AgentModeStatus::Active),
            agent_prompt,
            self.tool_executor.pending_tool_calls(),
            self.tool_executor.tool_results(),
        )
    }

    pub async fn execute_pending_tools(&mut self) -> Vec<crate::domain::ToolExecutionResult> {
        let results = self
            .tool_executor
            .execute_pending_tools(self.agents.yolo_mode)
            .await;

        for msg in self.tool_executor.create_tool_result_messages() {
            self.chat.messages.push(msg);
        }

        results
    }

    pub fn advance_spinner(&mut self) {
        if self.chat.streaming {
            self.chat.spinner_frame = (self.chat.spinner_frame + 1) % SPINNER_FRAMES.len();
        }
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
}

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

mod agent;
mod completion;
mod session;
mod input;

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
    fn tool_needs_approval_detects_destructive() {
        assert!(!crate::tool_registry::tool_needs_approval("write_file"));
        assert!(crate::tool_registry::tool_needs_approval("shell"));
        assert!(!crate::tool_registry::tool_needs_approval("read_file"));
        assert!(crate::tool_registry::tool_needs_approval(
            "str_replace_file"
        ));
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
    fn agent_state_defaults() {
        let state = AgentState::default();
        assert_eq!(state.status, AgentModeStatus::Disabled);
        assert_eq!(state.max_iterations, 10);
        assert_eq!(state.current_iteration, 0);
        assert_eq!(state.persona, "Orchestrator");
        assert!(!state.yolo_mode);
    }

    #[test]
    fn input_state_defaults() {
        let state = InputState::default();
        assert!(state.buffer.is_empty());
        assert_eq!(state.cursor_pos, 0);
        assert!(state.history.is_empty());
        assert_eq!(state.history_index, None);
        assert!(!state.show_autocomplete);
    }

    #[test]
    fn chat_state_default_context_window() {
        let state = ChatState::default();
        assert!(state.messages.is_empty());
        assert!(!state.streaming);
        assert!(state.auto_scroll);
        assert_eq!(state.scroll, 0);
    }

    #[test]
    fn chat_state_with_custom_context_window() {
        let state = ChatState::with_context_window(16384);
        assert!(state.messages.is_empty());
        assert!(!state.streaming);
    }

    #[test]
    fn spinner_wraps_around() {
        let mut engine = ChatEngine::new();
        engine.chat.streaming = true;
        for _ in 0..SPINNER_FRAMES.len() + 3 {
            engine.advance_spinner();
        }
        assert_eq!(engine.chat.spinner_frame, 3);
    }

    #[test]
    fn count_wrapped_lines_empty_string() {
        assert_eq!(count_wrapped_lines("", 10), 1);
    }

    #[test]
    fn count_wrapped_lines_zero_width() {
        assert_eq!(count_wrapped_lines("hello", 0), 1);
    }

    #[test]
    fn count_wrapped_lines_multiple_paragraphs() {
        let text = "first paragraph\n\nsecond paragraph";
        assert_eq!(count_wrapped_lines(text, 20), 3);
    }

    #[test]
    fn message_at_y_returns_none_for_empty_chat() {
        let engine = ChatEngine::new();
        assert!(engine.message_at_y(0, 80).is_none());
    }

    #[test]
    fn message_at_y_finds_first_message() {
        let mut engine = ChatEngine::new();
        engine.chat.messages.push(Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        });
        assert_eq!(engine.message_at_y(0, 80), Some(0));
    }
}
