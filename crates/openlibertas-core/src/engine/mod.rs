//! Chat engine: state management for chat, input, agents, and tool orchestration.
//!
//! Core types:
//! - `ChatEngine` — owns chat, input, agent, and tool state
//! - `ChatState` — messages, scroll, streaming, spinner
//! - `InputState` — buffer, cursor, history, autocomplete, selection
//! - `AgentState` — status, iteration count, persona, yolo mode

use crate::agent_tools::ToolRegistry;
use crate::agent_tools::{MessageAssembler, ToolExecutor};
use crate::completion::{CompletionEngine, CompletionItem, CompletionType};
use crate::domain::{Message, Model, ToolDefinition};
use crate::env_context::EnvContext;
use crate::history::HistoryStore;
use crate::mcp::McpClient;
use crate::session::ContextCompactor;
use crate::tool_format::ToolFormat;
use std::sync::Arc;

pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AgentModeStatus {
    #[default]
    Disabled,
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AgentMode {
    #[default]
    Auto,
    Plan,
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

#[derive(Debug, PartialEq, Default)]
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
    mcp_client: Option<Arc<McpClient>>,
}

impl Default for ChatEngine {
    fn default() -> Self {
        Self {
            chat: ChatState::default(),
            input: InputState::default(),
            agents: AgentState::default(),
            tool_registry: ToolRegistry::default(),
            tool_executor: ToolExecutor::default(),
            message_assembler: MessageAssembler,
            system_prompt: None,
            agent_prompt: None,
            env_context: None,
            tool_format: ToolFormat::Native,
            completion_engine: CompletionEngine::new(),
            history_store: None,
            mcp_client: None,
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
        self.mcp_client = Some(client.clone());
        self.tool_executor
            .add_backend(Box::new(crate::agent_tools::mcp_backend::McpBackend::new(
                client.clone(),
            )));
        self.tool_registry
            .add_backend(Box::new(crate::agent_tools::mcp_backend::McpBackend::new(
                client,
            )));
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

    pub fn mcp_client(&self) -> Option<Arc<McpClient>> {
        self.mcp_client.clone()
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

    pub fn start_agent_loop(&mut self) {
        if self.agents.status == AgentModeStatus::Idle {
            self.agents.status = AgentModeStatus::Active;
            self.agents.current_iteration = 0;
        }
    }

    pub fn finish_agent_loop(&mut self) {
        if self.agents.status == AgentModeStatus::Active {
            self.agents.status = AgentModeStatus::Idle;
            self.agents.current_iteration = 0;
        }
    }

    pub fn agent_iteration_exceeded(&self) -> bool {
        self.agents.status == AgentModeStatus::Active
            && self.agents.current_iteration >= self.agents.max_iterations
    }

    pub fn increment_agent_iteration(&mut self) {
        self.agents.current_iteration += 1;
    }

    pub fn isolate_session(&mut self) {
        self.chat
            .messages
            .retain(|m| m.role == crate::domain::Role::System);
        self.chat.scroll = 0;
        self.chat.streaming = false;
    }

    pub fn switch_persona(&mut self, persona: impl Into<String>, prompt: impl Into<String>) {
        let persona = persona.into();
        let prompt = prompt.into();
        self.agents.persona = persona.clone();
        self.agent_prompt = Some(prompt);
        self.agents.current_iteration = 0;
    }

    pub fn set_plan_mode(&mut self, mode: AgentMode) {
        self.agents.mode = mode;
    }

    pub fn plan_mode(&self) -> AgentMode {
        self.agents.mode
    }

    pub fn refresh_completions(&mut self, models: &[Model]) {
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

mod input;
mod session;

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
        assert!(!crate::agent_tools::tool_needs_approval("write_file"));
        assert!(crate::agent_tools::tool_needs_approval("shell"));
        assert!(!crate::agent_tools::tool_needs_approval("read_file"));
        assert!(crate::agent_tools::tool_needs_approval("str_replace_file"));
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
    fn agent_loop_transitions() {
        let mut engine = ChatEngine::new();
        engine.agents.status = AgentModeStatus::Idle;
        engine.start_agent_loop();
        assert_eq!(engine.agents.status, AgentModeStatus::Active);
        engine.finish_agent_loop();
        assert_eq!(engine.agents.status, AgentModeStatus::Idle);
    }

    #[test]
    fn agent_loop_exceeded_check() {
        let mut engine = ChatEngine::new();
        engine.agents.status = AgentModeStatus::Active;
        engine.agents.max_iterations = 3;
        engine.agents.current_iteration = 3;
        assert!(engine.agent_iteration_exceeded());
        engine.agents.current_iteration = 2;
        assert!(!engine.agent_iteration_exceeded());
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
}
