//! Application state and command execution for the TUI.
//!
//! The `App` struct holds all mutable UI state and handles:
//! - Slash command parsing and execution
//! - Model selection and provider switching
//! - Session management (save/load/delete)
//! - File context loading
//! - Voice mode state coordination

use crate::avatar::{AnimatedAvatar, IDLE_FRAMES};
use crate::markdown::MarkdownRenderer;
use crate::theme::Theme;
use openlibertas_core::commands::{find_model, get_model_suggestions};
use openlibertas_core::config::Config;
use openlibertas_core::domain::ProviderId;
use openlibertas_core::domain::Role;
use openlibertas_core::domain::{Message, Model};
use openlibertas_core::engine::{AgentModeStatus, ChatEngine};
use openlibertas_core::env_context::EnvContext;
use openlibertas_core::export::{self, ExportFormat};
use openlibertas_core::prompt::PromptManager;
use openlibertas_core::search;
use openlibertas_core::store::SessionStore;
use openlibertas_core::voice::VoiceManager;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Models,
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Checking,
}

pub use openlibertas_core::commands::{SlashCommand, SLASH_COMMANDS};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Overlay {
    None,
    Tools,
    Mcp,
    Sessions,
    Palette,
    Themes,
    Help,
    Agents,
    AvatarMenu,
}

pub struct SearchState {
    pub matches: Vec<search::SearchMatch>,
    pub index: usize,
    pub active: bool,
}

pub struct ModelState {
    pub models: Vec<Model>,
    pub selected: usize,
    pub current: Option<String>,
    pub provider: ProviderId,
    pub search: String,
}

pub struct App {
    pub screen: Screen,
    pub loading: bool,
    pub error: Option<String>,
    pub config: Config,
    pub connection_status: ConnectionStatus,
    pub models: ModelState,
    pub overlay: Overlay,
    pub(crate) search: SearchState,
    pub store: Option<SessionStore>,
    pub(crate) engine: ChatEngine,
    prompt_manager: PromptManager,
    pub palette_commands: Vec<(String, String)>,
    pub palette_selected: usize,
    pub markdown_renderer: MarkdownRenderer,
    pub(crate) theme: Theme,
    pub theme_selected: usize,
    pub agent_selected: usize,
    pub(crate) voice: VoiceManager,
    pub voice_status: Option<String>,
    pub pending_voice_generation: Option<u64>,
    pub mouse_enabled: bool,
    pub avatar_enabled: bool,
    pub avatars: Vec<AnimatedAvatar>,
    pub avatar_menu_selected: usize,
    pub last_click_time: Option<Instant>,
    pub last_click_pos: Option<(u16, u16)>,
    pub last_voice_key_at: Option<Instant>,
    pub voice_activity_at: Option<Instant>,
    pub temperature: Option<f32>,
    pub session_selected: usize,
    pub session_search: String,
    pub current_session_id: Option<String>,
    pub current_session_parent_id: Option<String>,
    pub current_session_branch_point: Option<usize>,
    pub current_session_has_branches: bool,
    pub rlm_mode: bool,
    pub error_banner: Option<(String, Instant)>,
    pub mcp_selected_server: usize,
    pub mcp_selected_tool: usize,
    pub mcp_health: HashMap<String, bool>,
    pub mcp_test_result: Option<String>,
    pub mcp_show_detail: bool,
    pub mcp_scroll: usize,
    pub cancel_token: Option<tokio_util::sync::CancellationToken>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let store = Config::data_dir().and_then(|d| SessionStore::new(d).ok());
        let voice_api_key = config.elevenlabs_api_key.clone();
        let voice_id = config.elevenlabs_voice_id.clone();
        let voice_input_device = config.input_device.clone();
        let context_window = config.effective_context_window() as usize;
        let local_models = openlibertas_core::model_scanner::scan_local_models(&config.models_dir);
        let filtered_local: Vec<Model> = if config.filter_require_voice_and_tools {
            local_models
                .into_iter()
                .filter(|m| m.supports_tools && m.supports_voice)
                .collect()
        } else {
            local_models
        };

        Self {
            screen: Screen::Chat,
            loading: true,
            error: None,
            config,
            connection_status: ConnectionStatus::Checking,
            models: ModelState {
                models: filtered_local,
                selected: 0,
                current: None,
                provider: ProviderId::new(""),
                search: String::new(),
            },
            overlay: Overlay::None,
            search: SearchState {
                matches: Vec::new(),
                index: 0,
                active: false,
            },
            store,
            engine: {
                let mut engine = ChatEngine::new()
                    .with_context_window(context_window)
                    .with_env_context(EnvContext::detect());
                if let Some(config_dir) =
                    Config::config_path().and_then(|p| p.parent().map(|p| p.to_path_buf()))
                {
                    let history_path = config_dir.join("history.jsonl");
                    let history_store = openlibertas_core::history::HistoryStore::new(history_path);
                    engine = engine.with_history_store(history_store);
                    engine.load_history();
                }
                engine
            },
            prompt_manager: PromptManager::new(),
            palette_commands: Vec::new(),
            palette_selected: 0,
            markdown_renderer: MarkdownRenderer::new(),
            theme: Theme::Default,
            theme_selected: 0,
            agent_selected: 0,
            voice: {
                let mut vm = VoiceManager::new(voice_api_key, voice_id);
                vm.config.input_device = voice_input_device;
                vm
            },
            voice_status: None,
            pending_voice_generation: None,
            mouse_enabled: false,
            avatar_enabled: false,
            avatars: vec![AnimatedAvatar::new("default", IDLE_FRAMES)],
            avatar_menu_selected: 0,
            last_click_time: None,
            last_click_pos: None,
            last_voice_key_at: None,
            voice_activity_at: None,
            temperature: None,
            session_selected: 0,
            session_search: String::new(),
            current_session_id: None,
            current_session_parent_id: None,
            current_session_branch_point: None,
            current_session_has_branches: false,
            rlm_mode: false,
            error_banner: None,
            mcp_selected_server: 0,
            mcp_selected_tool: 0,
            mcp_health: HashMap::new(),
            mcp_test_result: None,
            mcp_show_detail: false,
            mcp_scroll: 0,
            cancel_token: None,
        }
    }

    pub fn set_error_banner(&mut self, msg: String) {
        self.error_banner = Some((msg, Instant::now()));
    }

    pub fn clear_expired_error_banner(&mut self) {
        if let Some((_, instant)) = self.error_banner {
            if instant.elapsed() > Duration::from_secs(5) {
                self.error_banner = None;
            }
        }
    }

    pub fn session_has_branches(&self) -> bool {
        if let Some(ref id) = self.current_session_id {
            if let Some(ref store) = self.store {
                if let Ok(sessions) = store.list_with_meta() {
                    return sessions.iter().any(|s| s.id == *id && !s.branches.is_empty());
                }
            }
        }
        false
    }

    pub fn voice_key_debounce(&self) -> bool {
        const DEBOUNCE_MS: u128 = 200;
        self.last_voice_key_at
            .is_some_and(|t| t.elapsed().as_millis() < DEBOUNCE_MS)
    }

    pub fn set_provider(&mut self, provider: impl Into<ProviderId>) {
        let provider = provider.into();
        let prompt = self.prompt_manager.get_prompt(provider.as_str());
        self.engine.set_system_prompt(prompt.to_string());
        let tool_format = self
            .config
            .providers
            .iter()
            .find(|p| ProviderId::new(&p.name) == provider)
            .map(|p| p.tool_format)
            .unwrap_or_default();
        self.engine.set_tool_format(tool_format);
        self.models.provider = provider;
    }

    #[cfg(test)]
    fn get_autocomplete_suggestions(&self) -> Vec<&'static str> {
        SlashCommand::autocomplete(&self.engine.input().buffer)
    }

    pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
        SlashCommand::parse(input)
    }

    pub fn execute_slash_command(&mut self, cmd: SlashCommand) -> Option<String> {
        match cmd {
            SlashCommand::Help(cmd) => {
                if let Some(cmd_name) = cmd {
                    let cmd_name = cmd_name.trim();
                    if cmd_name.is_empty() {
                        self.overlay = Overlay::Help;
                        None
                    } else {
                        match openlibertas_core::commands::command_detailed_help(cmd_name) {
                            Some(help) => Some(help),
                            None => Some(format!("No detailed help for '/{}'. Use /help to see all commands.", cmd_name)),
                        }
                    }
                } else {
                    self.overlay = Overlay::Help;
                    None
                }
            }
            SlashCommand::Tools => {
                self.overlay = if self.overlay == Overlay::Tools {
                    Overlay::None
                } else {
                    Overlay::Tools
                };
                if self.overlay == Overlay::Tools {
                    let tool_list = if self.engine.tools().available_tools().is_empty() {
                        "No tools available. Check MCP server configuration.".to_string()
                    } else {
                        self.engine
                            .tools()
                            .available_tools()
                            .iter()
                            .map(|t| format!("  {}: {}", t.name, t.description))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    Some(format!(
                        "Available Tools ({}\n{}",
                        self.engine.tools().available_tools().len(),
                        tool_list
                    ))
                } else {
                    Some("Tools panel hidden".to_string())
                }
            }
            SlashCommand::Model(model_name) => {
                if model_name.is_empty() {
                    if self.models.models.is_empty() {
                        self.screen = Screen::Models;
                        Some("Loading models... Select a model from the menu.".to_string())
                    } else {
                        self.screen = Screen::Models;
                        Some("Select a model from the menu".to_string())
                    }
                } else {
                    match find_model(&self.models.models, &model_name) {
                        Some((idx, _matched_name)) => {
                            self.models.selected = idx;
                            self.models.current = Some(_matched_name.clone());
                            if let Some(model) = self.models.models.get(idx) {
                                self.set_provider(model.provider.clone());
                            }
                            Some(format!("Switched to model: {}", _matched_name))
                        }
                        None => {
                            let suggestions =
                                get_model_suggestions(&self.models.models, &model_name);
                            if suggestions.is_empty() {
                                Some(format!(
                                    "Model '{}' not found. Use /model to see available models.",
                                    model_name
                                ))
                            } else {
                                Some(format!(
                                    "Model '{}' not found. Did you mean: {}?",
                                    model_name,
                                    suggestions.join(", ")
                                ))
                            }
                        }
                    }
                }
            }
            SlashCommand::Clear => {
                self.engine.clear_messages();
                self.current_session_id = None;
                self.current_session_parent_id = None;
                self.current_session_branch_point = None;
                Some("Session cleared".to_string())
            }
            SlashCommand::Quit => None,
            SlashCommand::Agents => {
                if self.engine.agents_mut().status == AgentModeStatus::Disabled {
                    self.engine.agents_mut().status = AgentModeStatus::Idle;
                    return Some(format!(
                        "Agents enabled (Persona: {})",
                        self.engine.agents_mut().persona
                    ));
                }
                self.overlay = Overlay::Agents;
                self.agent_selected = 0;
                None
            }
            SlashCommand::Avatar(name) => {
                let arg = name.as_deref().unwrap_or("");
                if arg == "off" || (arg.is_empty() && self.avatar_enabled) {
                    self.avatar_enabled = false;
                    Some("Avatars disabled".to_string())
                } else if arg == "on" || (arg.is_empty() && !self.avatar_enabled) {
                    self.avatar_enabled = true;
                    Some("Avatars enabled".to_string())
                } else {
                    None
                }
            }
            SlashCommand::AvatarMenu => {
                self.overlay = Overlay::AvatarMenu;
                None
            }
            SlashCommand::Yolo => {
                self.engine.agents_mut().yolo_mode = !self.engine.agents_mut().yolo_mode;
                if self.engine.agents_mut().yolo_mode {
                    Some(
                        "YOLO mode enabled. Destructive tools will execute without confirmation."
                            .to_string(),
                    )
                } else {
                    Some(
                        "YOLO mode disabled. Destructive tools will require confirmation."
                            .to_string(),
                    )
                }
            }
            SlashCommand::Plan => {
                let new_mode = match self.engine.plan_mode() {
                    openlibertas_core::engine::AgentMode::Auto => {
                        openlibertas_core::engine::AgentMode::Plan
                    }
                    openlibertas_core::engine::AgentMode::Plan => {
                        openlibertas_core::engine::AgentMode::Auto
                    }
                };
                self.engine.set_plan_mode(new_mode);
                match new_mode {
                    openlibertas_core::engine::AgentMode::Plan => {
                        Some("Plan mode enabled. Only read-only tools available.".to_string())
                    }
                    openlibertas_core::engine::AgentMode::Auto => {
                        Some("Plan mode disabled. All tools available.".to_string())
                    }
                }
            }
            SlashCommand::Compact => {
                let (before, after) = self.engine.compact_context();
                if after < before {
                    Some(format!(
                        "Context compacted: {} → {} messages",
                        before, after
                    ))
                } else {
                    Some(format!("No compaction needed ({} messages)", before))
                }
            }
            SlashCommand::Rlm => {
                self.rlm_mode = !self.rlm_mode;
                if self.rlm_mode {
                    self.engine.set_system_prompt(
                        "You are in RLM mode. Use the `rlm_repl` tool to execute Python code. \
When you have your final answer, output 'FINAL(answer)' on its own line."
                            .to_string(),
                    );
                    self.engine.set_agent_prompt(String::new());
                    if self.engine.agents().status
                        == openlibertas_core::engine::AgentModeStatus::Disabled
                    {
                        self.engine.agents_mut().status =
                            openlibertas_core::engine::AgentModeStatus::Idle;
                    }
                    Some("RLM mode enabled. The model will use Python code execution.".to_string())
                } else {
                    self.set_provider(self.models.provider.clone());
                    Some("RLM mode disabled.".to_string())
                }
            }
            SlashCommand::Edit(n) => {
                let user_msgs: Vec<(usize, String)> = self
                    .engine
                    .chat()
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.role == Role::User)
                    .map(|(i, m)| (i, m.content.clone()))
                    .collect();
                if n == 0 || n > user_msgs.len() {
                    Some(format!(
                        "Invalid message number. There are {} user messages. Use /edit 1..{}",
                        user_msgs.len(),
                        user_msgs.len()
                    ))
                } else {
                    let (idx, content) = &user_msgs[n - 1];
                    let content_len = content.len();
                    self.engine.input_mut().buffer = content.clone();
                    self.engine.input_mut().cursor_pos = content_len;
                    self.engine.chat_mut().messages.truncate(*idx);
                    Some(format!("Editing message {}. Press Enter to resend.", n))
                }
            }
            SlashCommand::Remove(n) => {
                if n == 0 || n > self.engine.chat_mut().messages.len() {
                    Some(format!(
                        "Invalid message number. There are {} messages. Use /remove 1..{}",
                        self.engine.chat_mut().messages.len(),
                        self.engine.chat_mut().messages.len()
                    ))
                } else {
                    let removed = self.engine.chat_mut().messages.remove(n - 1);
                    let preview = if removed.content.len() > 40 {
                        format!("{}...", &removed.content[..40])
                    } else {
                        removed.content.clone()
                    };
                    Some(format!(
                        "Removed message {}: [{}] {}",
                        n, removed.role, preview
                    ))
                }
            }
            SlashCommand::Mcp => {
                self.overlay = if self.overlay == Overlay::Mcp {
                    Overlay::None
                } else {
                    Overlay::Mcp
                };
                if self.overlay == Overlay::Mcp {
                    if let Some(client) = self.engine.tools().client() {
                        let servers = client.server_names();
                        if servers.is_empty() {
                            Some("No MCP servers configured".to_string())
                        } else {
                            let list = servers.join("\n  ");
                            Some(format!("MCP Servers:\n  {}", list))
                        }
                    } else {
                        Some("MCP client not initialized".to_string())
                    }
                } else {
                    Some("MCP panel hidden".to_string())
                }
            }
            SlashCommand::Save(name) => {
                if let Some(ref store) = self.store {
                    let id = if name.is_empty() {
                        let model = self.models.current.as_deref().unwrap_or("unknown");
                        SessionStore::generate_name(model)
                    } else {
                        name
                    };
                    match store.save(
                        &id,
                        self.models.current.as_deref(),
                        &self.engine.chat().messages,
                    ) {
                        Ok(_) => {
                            self.current_session_id = Some(id.clone());
                            self.current_session_has_branches = self.session_has_branches();
                            Some(format!("Session '{}' saved", id))
                        }
                        Err(e) => Some(format!("Failed to save: {}", e)),
                    }
                } else {
                    Some("Store not available".to_string())
                }
            }
            SlashCommand::Load(name) => {
                if name.is_empty() {
                    self.overlay = Overlay::Sessions;
                    Some("Session manager opened".to_string())
                } else if let Some(ref store) = self.store {
                    match openlibertas_core::commands::load_session(store, &name) {
                        Ok(session) => {
                            self.engine.chat_mut().messages = session.messages;
                            self.engine.chat_mut().scroll = 0;
                            if let Some(ref model) = session.model {
                                self.models.current = Some(model.clone());
                                if let Some((idx, _)) = find_model(&self.models.models, model) {
                                    self.models.selected = idx;
                                    if let Some(m) = self.models.models.get(idx) {
                                        self.set_provider(m.provider.clone());
                                    }
                                }
                            }
                            self.current_session_id = Some(name.clone());
                            self.current_session_parent_id = session.parent_id;
                            self.current_session_branch_point = session.branch_point;
                            self.current_session_has_branches = self.session_has_branches();
                            Some(format!("Session '{}' loaded", name))
                        }
                        Err(e) => Some(format!("Failed to load: {}", e)),
                    }
                } else {
                    Some("Store not available".to_string())
                }
            }
            SlashCommand::Sessions => {
                self.overlay = if self.overlay == Overlay::Sessions {
                    Overlay::None
                } else {
                    Overlay::Sessions
                };
                if self.overlay == Overlay::Sessions {
                    Some("Session manager opened".to_string())
                } else {
                    Some("Session manager closed".to_string())
                }
            }
            SlashCommand::Delete(name) => {
                if name.is_empty() {
                    self.overlay = Overlay::Sessions;
                    Some("Session manager opened. Select a session to delete.".to_string())
                } else if let Some(ref store) = self.store {
                    match store.delete(&name) {
                        Ok(_) => Some(format!("Session '{}' deleted", name)),
                        Err(e) => Some(format!("Failed to delete: {}", e)),
                    }
                } else {
                    Some("Store not available".to_string())
                }
            }
            SlashCommand::Export(arg) => {
                let (format, path) = if arg.is_empty() || arg.contains('/') || arg.contains('.') {
                    let path = if arg.is_empty() {
                        "chat.md".to_string()
                    } else {
                        arg
                    };
                    (ExportFormat::from_extension(&path), path)
                } else {
                    let format = match arg.to_lowercase().as_str() {
                        "json" => ExportFormat::Json,
                        "txt" | "text" => ExportFormat::PlainText,
                        _ => ExportFormat::Markdown,
                    };
                    let ext = match format {
                        ExportFormat::Json => "json",
                        ExportFormat::PlainText => "txt",
                        ExportFormat::Markdown => "md",
                    };
                    let session_name = self
                        .current_session_id
                        .as_deref()
                        .unwrap_or("untitled");
                    let exports_dir = Config::data_dir()
                        .map(|d| d.join("exports"))
                        .unwrap_or_else(|| std::path::PathBuf::from("exports"));
                    let _ = std::fs::create_dir_all(&exports_dir);
                    let path = exports_dir.join(format!("{}.{}", session_name, ext));
                    (format, path.to_string_lossy().to_string())
                };
                let content = self.export_session(format);
                match std::fs::write(&path, content) {
                    Ok(_) => Some(format!("Exported to '{}'", path)),
                    Err(e) => Some(format!("Failed to export: {}", e)),
                }
            }
            SlashCommand::Search(query) => {
                if query.is_empty() {
                    self.search.active = false;
                    self.search.matches.clear();
                    Some("Search cleared".to_string())
                } else {
                    self.search.matches =
                        search::search_messages(&self.engine.chat_mut().messages, &query);
                    self.search.index = 0;
                    self.search.active = !self.search.matches.is_empty();
                    let count = self.search.matches.len();
                    if count > 0 {
                        Some(format!("Found {} match(es) for '{}'", count, query))
                    } else {
                        Some(format!("No matches for '{}'", query))
                    }
                }
            }
            SlashCommand::Theme(name) => {
                if name.is_empty() {
                    self.open_themes_panel();
                    None
                } else {
                    match Theme::from_name(&name) {
                        Some(theme) => {
                            self.theme = theme;
                            Some(format!("Theme changed to: {}", self.theme.name()))
                        }
                        None => {
                            let available = Theme::available_themes().join(", ");
                            Some(format!(
                                "Unknown theme '{}'. Available: {}",
                                name, available
                            ))
                        }
                    }
                }
            }
            SlashCommand::New => {
                self.engine.clear_messages();
                self.engine.input_mut().buffer.clear();
                self.engine.tool_executor_mut().clear_pending_tool_calls();
                self.engine.tool_executor_mut().clear_tool_results();
                self.current_session_id = None;
                self.current_session_parent_id = None;
                self.current_session_branch_point = None;
                let loaded = self.load_context_files();
                if loaded.is_empty() {
                    Some("New session started".to_string())
                } else {
                    Some(format!(
                        "New session started. Loaded: {}",
                        loaded.join(", ")
                    ))
                }
            }
            SlashCommand::Undo => {
                if self.engine.chat_mut().messages.len() >= 2 {
                    self.engine.chat_mut().messages.pop();
                    self.engine.chat_mut().messages.pop();
                    Some("Undid last turn".to_string())
                } else {
                    Some("Nothing to undo".to_string())
                }
            }
            SlashCommand::Title(name) => {
                if name.is_empty() {
                    Some("Usage: /title <name>".to_string())
                } else {
                    Some(format!(
                        "Session title set to: '{}' (Note: titles are not yet persisted)",
                        name
                    ))
                }
            }
            SlashCommand::Branch(msg_idx) => {
                if let Some(ref store) = self.store {
                    let mut parent_id = self.current_session_id.clone().unwrap_or_default();
                    if parent_id.is_empty() {
                        let model = self.models.current.as_deref().unwrap_or("unknown");
                        let auto_id = SessionStore::generate_name(model);
                        if let Err(e) = store.save(
                            &auto_id,
                            self.models.current.as_deref(),
                            &self.engine.chat().messages,
                        ) {
                            return Some(format!("Failed to auto-save before branch: {}", e));
                        }
                        self.current_session_id = Some(auto_id.clone());
                        parent_id = auto_id;
                    }

                    let messages = self.engine.chat().messages.clone();
                    let branch_point = msg_idx.unwrap_or_else(|| messages.len().saturating_sub(1));

                    if branch_point >= messages.len() {
                        return Some(format!(
                            "Invalid branch point. There are {} messages (0..{})",
                            messages.len(),
                            messages.len().saturating_sub(1)
                        ));
                    }

                    let branch_messages: Vec<_> = messages[..=branch_point].to_vec();
                    let model = self.models.current.as_deref().unwrap_or("unknown");
                    let branch_id = format!("{}-branch", SessionStore::generate_name(model));

                    match store.save_branch(
                        &branch_id,
                        self.models.current.as_deref(),
                        &branch_messages,
                        Some(&parent_id),
                        Some(branch_point),
                    ) {
                        Ok(_) => {
                            if let Err(e) = store.add_branch(&parent_id, &branch_id) {
                                return Some(format!(
                                    "Branch created but failed to update parent: {}",
                                    e
                                ));
                            }
                            self.engine.chat_mut().messages = branch_messages;
                            self.engine.chat_mut().scroll = 0;
                            self.current_session_id = Some(branch_id.clone());
                            self.current_session_parent_id = Some(parent_id.clone());
                            self.current_session_branch_point = Some(branch_point);
                            Some(format!(
                                "Created branch '{}' from message {}. Parent: '{}'",
                                branch_id, branch_point, parent_id
                            ))
                        }
                        Err(e) => Some(format!("Failed to create branch: {}", e)),
                    }
                } else {
                    Some("Store not available".to_string())
                }
            }
            SlashCommand::Voice => {
                let was_enabled = self.voice.is_enabled();
                if !was_enabled && self.voice.config.api_key.is_none() {
                    Some("Voice mode requires an ElevenLabs API key. Set ELEVENLABS_API_KEY or add elevenlabs_api_key to config.toml".to_string())
                } else {
                    let enabled = self.voice.toggle();
                    if enabled {
                        Some(
                            "Voice mode enabled. Hold Ctrl+Space to record, release to send."
                                .to_string(),
                        )
                    } else {
                        Some("Voice mode disabled.".to_string())
                    }
                }
            }
            SlashCommand::VoiceDevice(device) => {
                if device.is_empty() {
                    match openlibertas_core::voice::list_input_devices() {
                        Ok(devices) => {
                            if devices.is_empty() {
                                Some("No input devices found.".to_string())
                            } else {
                                let mut msg = String::from("Available input devices:\n\n");
                                for (i, d) in devices.iter().enumerate() {
                                    let marker = if d.is_default { " [default]" } else { "" };
                                    msg.push_str(&format!(
                                        "  {}. {}{} — {}Hz, {}ch, {}",
                                        i + 1,
                                        d.name,
                                        marker,
                                        d.sample_rate,
                                        d.channels,
                                        d.sample_format
                                    ));
                                    if i < devices.len() - 1 {
                                        msg.push_str("\n\n");
                                    } else {
                                        msg.push('\n');
                                    }
                                }
                                msg.push_str("\nUse /voice_device <name> to select a device.");
                                Some(msg)
                            }
                        }
                        Err(e) => Some(format!("Failed to list devices: {}", e)),
                    }
                } else {
                    self.voice.config.input_device = Some(device.clone());
                    self.config.input_device = Some(device.clone());
                    let msg = format!("Voice input device set to: {}", device);
                    if let Err(e) = self.config.save() {
                        return Some(format!("{} (config save failed: {})", msg, e));
                    }
                    Some(msg)
                }
            }
            SlashCommand::Temperature(temp) => {
                self.temperature = Some(temp);
                Some(format!("Temperature set to: {}", temp))
            }
            SlashCommand::SetKey(provider, key) => {
                if provider.is_empty() || key.is_empty() {
                    Some("Usage: /set-key <provider> <key>".to_string())
                } else {
                    match openlibertas_core::credentials::CredentialManager::set_api_key(&provider, &key) {
                        Ok(()) => {
                            if let Some(p) = self.config.providers.iter_mut().find(|p| p.name == provider) {
                                p.api_key = openlibertas_core::config::SecretString::new(key);
                            }
                            Some(format!("API key stored in keyring for provider '{}'", provider))
                        }
                        Err(e) => Some(format!("Failed to store API key: {}", e)),
                    }
                }
            }
            SlashCommand::Mouse => {
                self.mouse_enabled = !self.mouse_enabled;
                if self.mouse_enabled {
                    Some("Mouse capture enabled — wheel scroll, click to focus, right-click to copy. Native text selection disabled. Disable with /mouse.".to_string())
                } else {
                    Some("Mouse capture disabled — native terminal text selection and copy work. Enable with /mouse for wheel scrolling and clicking.".to_string())
                }
            }
            SlashCommand::Unknown(cmd) => Some(format!("Unknown command: {}", cmd)),
        }
    }

    pub fn select_next_model(&mut self) {
        if !self.models.models.is_empty() {
            self.models.selected = (self.models.selected + 1).min(self.models.models.len() - 1);
        }
    }
    pub fn select_prev_model(&mut self) {
        if !self.models.models.is_empty() {
            self.models.selected = self.models.selected.saturating_sub(1);
        }
    }

    pub fn select_current_model(&mut self) {
        if let Some(model) = self.models.models.get(self.models.selected) {
            let provider = model.provider.clone();
            self.models.current = Some(model.id.clone());
            self.set_provider(provider);
            self.screen = Screen::Chat;
        }
    }

    pub fn load_context_files(&mut self) -> Vec<String> {
        let loaded = openlibertas_core::session::read_context_files();
        for (filename, content) in &loaded {
            self.engine.chat_mut().messages.push(Message {
                role: Role::System,
                content: format!("--- {} ---\n{}", filename, content),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            });
        }
        loaded.into_iter().map(|(name, _)| name).collect()
    }

    pub fn finish_stream(&mut self) {
        self.engine.finish_stream();
        self.engine.sanitize_assistant_content();
        let _ = self.autosave();
    }

    pub fn autosave(&mut self) -> Option<String> {
        if !self.config.auto_save {
            return None;
        }
        if let Some(ref store) = self.store {
            let model = self.models.current.as_deref().unwrap_or("unknown");
            let id = format!(
                "autosave-{}",
                model
                    .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
                    .replace("--", "-")
            );
            match store.save(
                &id,
                self.models.current.as_deref(),
                &self.engine.chat().messages,
            ) {
                Ok(_) => {
                    self.current_session_id = Some(id);
                    None
                }
                Err(e) => Some(format!("Auto-save failed: {}", e)),
            }
        } else {
            Some("Auto-save failed: no session store".to_string())
        }
    }

    pub fn agent_personas(&self) -> Vec<(String, String)> {
        let mut personas = Vec::new();

        if let Ok(entries) = std::fs::read_dir("personas") {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "md") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut lines = content.lines();
                        let name = lines
                            .next()
                            .and_then(|line| line.strip_prefix('#'))
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|| {
                                path.file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Unknown")
                                    .to_string()
                            });
                        let prompt = lines.collect::<Vec<_>>().join("\n").trim().to_string();
                        if !prompt.is_empty() {
                            personas.push((name, prompt));
                        }
                    }
                }
            }
        }

        if personas.is_empty() {
            personas.push(("Orchestrator".to_string(), "You are the Orchestrator — the central intelligence of a multi-agent system. You coordinate specialized agents to solve complex problems through strategic planning, parallel delegation, and rigorous verification.".to_string()));
            personas.push((
                "Coding".to_string(),
                "You are a senior software engineer with access to development tools. \
You write clean, well-tested code. When using tools, prefer file operations and code analysis. \
Think step by step and verify your changes compile and pass tests."
                    .to_string(),
            ));
            personas.push((
                "Research".to_string(),
                "You are a research assistant with access to search and analysis tools. \
You gather information thoroughly, verify facts, and synthesize comprehensive answers. \
Cite sources when possible and note uncertainty."
                    .to_string(),
            ));
            personas.push((
                "Creative".to_string(),
                "You are a creative assistant with access to media and design tools. \
You think outside the box and provide novel, inspiring solutions. Iterate on ideas using \
available tools to refine and polish your work."
                    .to_string(),
            ));
        }

        personas.sort_by(|a, b| {
            if a.0 == "Orchestrator" {
                std::cmp::Ordering::Less
            } else if b.0 == "Orchestrator" {
                std::cmp::Ordering::Greater
            } else {
                a.0.cmp(&b.0)
            }
        });

        personas
    }

    pub fn cycle_agent_persona(&mut self) -> String {
        let personas = self.agent_personas();
        let current = personas
            .iter()
            .position(|(n, _)| *n == self.engine.agents_mut().persona)
            .unwrap_or(0);
        let next = (current + 1) % personas.len();
        self.engine.agents_mut().persona = personas[next].0.to_string();
        self.engine.agents_mut().persona.clone()
    }

    pub fn export_session(&self, format: ExportFormat) -> String {
        export::export_messages(
            &self.engine.chat().messages,
            self.models.current.as_deref(),
            format,
        )
    }

    pub fn search_next(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        self.search.index = (self.search.index + 1) % self.search.matches.len();
        self.scroll_to_match();
    }

    pub fn search_prev(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        if self.search.index == 0 {
            self.search.index = self.search.matches.len() - 1;
        } else {
            self.search.index -= 1;
        }
        self.scroll_to_match();
    }

    fn scroll_to_match(&mut self) {
        if let Some(m) = self.search.matches.get(self.search.index) {
            let target = m.message_index;
            let mut line_count = 0;
            for (i, msg) in self.engine.chat_mut().messages.iter().enumerate() {
                if i == target {
                    self.engine.chat_mut().scroll = line_count;
                    self.engine.chat_mut().auto_scroll = false;
                    break;
                }
                if msg.role == Role::System && msg.is_prompt {
                    continue;
                }
                line_count += 3;
                if msg.tool_calls.is_some() {
                    line_count += 2;
                }
                let content_lines = msg.content.lines().count();
                line_count += content_lines.max(1);
            }
        }
    }

    pub fn theme_next(&mut self) {
        let themes = Theme::all();
        if !themes.is_empty() {
            self.theme_selected = (self.theme_selected + 1).min(themes.len() - 1);
        }
    }

    pub fn theme_prev(&mut self) {
        self.theme_selected = self.theme_selected.saturating_sub(1);
    }

    pub fn select_theme(&mut self) {
        let themes = Theme::all();
        if let Some((_, theme)) = themes.get(self.theme_selected) {
            self.theme = *theme;
        }
        self.overlay = Overlay::None;
    }

    pub fn open_themes_panel(&mut self) {
        let themes = Theme::all();
        self.theme_selected = themes
            .iter()
            .position(|(_, t)| *t == self.theme)
            .unwrap_or(0);
        self.overlay = Overlay::Themes;
    }

    pub fn agent_prev(&mut self) {
        self.agent_selected = self.agent_selected.saturating_sub(1);
    }

    pub fn agent_next(&mut self) {
        let count = self.agent_personas().len().saturating_add(3);
        self.agent_selected = (self.agent_selected + 1) % count.max(1);
    }

    pub fn avatar_menu_prev(&mut self) {
        self.avatar_menu_selected = self.avatar_menu_selected.saturating_sub(1);
    }

    pub fn avatar_menu_next(&mut self) {
        self.avatar_menu_selected = (self.avatar_menu_selected + 1) % 4;
    }

    pub fn avatar_menu_select(&mut self) {
        match self.avatar_menu_selected {
            0 => {
                self.avatar_enabled = !self.avatar_enabled;
            }
            1 => {
                if let Some(avatar) = self.avatars.first_mut() {
                    avatar.anim_speed = match avatar.anim_speed as u32 {
                        120 => 240.0,
                        240 => 480.0,
                        480 => 960.0,
                        _ => 120.0,
                    };
                }
            }
            2 => {
                if let Some(avatar) = self.avatars.first_mut() {
                    avatar.velocity.0 = match (avatar.velocity.0 * 100.0) as i32 {
                        0 => 0.3,
                        30 => 0.6,
                        60 => 1.2,
                        _ => 0.0,
                    };
                }
            }
            3 => {
                if let Some(avatar) = self.avatars.first_mut() {
                    avatar.velocity.1 = match (avatar.velocity.1 * 100.0) as i32 {
                        0 => 0.15,
                        15 => 0.3,
                        30 => 0.6,
                        _ => 0.0,
                    };
                }
            }
            _ => {}
        }
    }

    pub fn select_agent_option(&mut self) {
        match self.agent_selected {
            0 => {
                self.engine.agents_mut().status =
                    if self.engine.agents_mut().status == AgentModeStatus::Disabled {
                        AgentModeStatus::Idle
                    } else {
                        AgentModeStatus::Disabled
                    };
            }
            1 => {
                self.cycle_agent_persona();
            }
            2 => {
                self.engine.agents_mut().max_iterations =
                    if self.engine.agents_mut().max_iterations >= 50 {
                        5
                    } else {
                        (self.engine.agents_mut().max_iterations + 5).min(50)
                    };
            }
            _ => {}
        }
    }

    pub fn update_command_palette(&mut self) {
        let prefix = self.engine.input_mut().buffer.to_lowercase();
        self.palette_commands = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(&prefix))
            .map(|cmd| {
                (
                    cmd.to_string(),
                    openlibertas_core::commands::command_description(cmd).to_string(),
                )
            })
            .collect();
        self.palette_selected = 0;
    }

    pub fn palette_prev(&mut self) {
        if !self.palette_commands.is_empty() {
            self.palette_selected = self.palette_selected.saturating_sub(1);
        }
    }

    pub fn palette_next(&mut self) {
        if !self.palette_commands.is_empty() {
            self.palette_selected =
                (self.palette_selected + 1).min(self.palette_commands.len() - 1);
        }
    }

    pub fn select_palette_command(&mut self) -> Option<String> {
        self.palette_commands
            .get(self.palette_selected)
            .map(|(cmd, _)| cmd.clone())
    }

    pub fn go_to_models(&mut self) {
        self.screen = Screen::Models;
        self.engine.chat_mut().streaming = false;
        self.overlay = Overlay::None;
    }

    pub fn filtered_sessions(&self) -> Vec<openlibertas_core::store::SessionMeta> {
        let all = self
            .store
            .as_ref()
            .and_then(|s| s.list_with_meta().ok())
            .unwrap_or_default();

        if self.session_search.is_empty() {
            all
        } else {
            let search = self.session_search.to_lowercase();
            all.into_iter()
                .filter(|meta| {
                    meta.id.to_lowercase().contains(&search)
                        || meta
                            .title
                            .as_ref()
                            .map(|t| t.to_lowercase().contains(&search))
                            .unwrap_or(false)
                        || meta.preview.to_lowercase().contains(&search)
                })
                .collect()
        }
    }

    pub fn session_prev(&mut self) {
        self.session_selected = self.session_selected.saturating_sub(1);
    }

    pub fn session_next(&mut self, count: usize) {
        if count > 0 {
            self.session_selected = (self.session_selected + 1).min(count - 1);
        }
    }

    pub fn load_selected_session(&mut self) -> Option<String> {
        let sessions = self.filtered_sessions();
        let selected = sessions.get(self.session_selected)?;
        let id = selected.id.clone();

        if let Some(ref store) = self.store {
            match openlibertas_core::commands::load_session(store, &id) {
                Ok(session) => {
                    self.engine.chat_mut().messages = session.messages;
                    self.engine.chat_mut().scroll = 0;
                    if let Some(ref model) = session.model {
                        self.models.current = Some(model.clone());
                        if let Some((idx, _matched_name)) = find_model(&self.models.models, model) {
                            self.models.selected = idx;
                            if let Some(m) = self.models.models.get(idx) {
                                self.set_provider(m.provider.clone());
                            }
                        }
                    }
                    self.current_session_id = Some(id.clone());
                    self.current_session_parent_id = session.parent_id;
                    self.current_session_branch_point = session.branch_point;
                    self.current_session_has_branches = self.session_has_branches();
                    self.overlay = Overlay::None;
                    self.session_search.clear();
                    self.session_selected = 0;
                    Some(format!("Session '{}' loaded", id))
                }
                Err(e) => Some(format!("Failed to load: {}", e)),
            }
        } else {
            Some("Store not available".to_string())
        }
    }

    pub fn delete_selected_session(&mut self) -> Option<String> {
        let sessions = self.filtered_sessions();
        let selected = sessions.get(self.session_selected)?;
        let id = selected.id.clone();

        if let Some(ref store) = self.store {
            match store.delete(&id) {
                Ok(_) => {
                    if self.session_selected > 0
                        && self.session_selected >= sessions.len().saturating_sub(1)
                    {
                        self.session_selected -= 1;
                    }
                    Some(format!("Session '{}' deleted", id))
                }
                Err(e) => Some(format!("Failed to delete: {}", e)),
            }
        } else {
            Some("Store not available".to_string())
        }
    }

    // -- Completion helpers --

    pub fn refresh_completions(&mut self) {
        self.engine.refresh_completions(&self.models.models);
    }

    pub fn clear_completions(&mut self) {
        self.engine.clear_completions();
    }

    pub fn cycle_completion_next(&mut self) {
        self.engine.cycle_completion_next();
    }

    pub fn cycle_completion_prev(&mut self) {
        self.engine.cycle_completion_prev();
    }

    pub fn apply_completion(&mut self) -> bool {
        self.engine.apply_completion()
    }

    pub fn completion_active(&self) -> bool {
        self.engine.input().completion_active
    }

    pub fn completion_items(&self) -> &[openlibertas_core::completion::CompletionItem] {
        &self.engine.input().completion_items
    }

    pub fn completion_selected(&self) -> usize {
        self.engine.input().autocomplete_index
    }

    pub fn mcp_server_names(&self) -> Vec<String> {
        self.engine
            .tools()
            .client()
            .as_ref()
            .map_or(Vec::new(), |c| c.server_names())
    }

    pub fn mcp_server_count(&self) -> usize {
        self.mcp_server_names().len()
    }

    pub fn mcp_selected_server_name(&self) -> Option<String> {
        let names = self.mcp_server_names();
        names.get(self.mcp_selected_server).cloned()
    }

    pub fn mcp_tools_for_server(&self, server_name: &str) -> Vec<openlibertas_core::mcp::McpTool> {
        let tool_map = self.engine.tools().tool_server_map();
        self.engine
            .tools()
            .available_tools()
            .iter()
            .filter(|t| {
                tool_map
                    .get(&t.name)
                    .map(|s| s == server_name)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    pub fn mcp_server_prev(&mut self) {
        self.mcp_selected_server = self.mcp_selected_server.saturating_sub(1);
        self.mcp_selected_tool = 0;
        self.mcp_scroll = 0;
    }

    pub fn mcp_server_next(&mut self) {
        let count = self.mcp_server_count();
        if count > 0 {
            self.mcp_selected_server = (self.mcp_selected_server + 1).min(count - 1);
            self.mcp_selected_tool = 0;
            self.mcp_scroll = 0;
        }
    }

    pub fn toggle_mcp_detail(&mut self) {
        self.mcp_show_detail = !self.mcp_show_detail;
    }

    pub fn selected_tool_name(&self) -> Option<String> {
        if let Some(server_name) = self.mcp_selected_server_name() {
            let tools = self.mcp_tools_for_server(&server_name);
            tools.get(self.mcp_selected_tool).map(|t| t.name.clone())
        } else {
            None
        }
    }

    pub fn handle_wire_message(&mut self, msg: &openlibertas_core::soul::WireMessage) {
        use openlibertas_core::domain::{now_timestamp, Message, Role};
        use openlibertas_core::soul::WireMessage;

        match msg {
            WireMessage::TurnStarted { .. } => {
                self.engine.chat_mut().streaming = true;
            }
            WireMessage::TextDelta(text) => {
                self.ensure_last_message_is_assistant();
                self.engine.append_stream_chunk(text);
            }
            WireMessage::ReasoningDelta(text) => {
                self.ensure_last_message_is_assistant();
                self.engine.append_reasoning_chunk(text);
            }
            WireMessage::ToolCallStarted { id, name } => {
                self.engine
                    .add_tool_call(openlibertas_core::domain::ToolCall {
                        id: id.clone(),
                        call_type: "function".to_string(),
                        function: openlibertas_core::domain::FunctionCall {
                            name: name.clone(),
                            arguments: "{}".to_string(),
                        },
                    });
            }
            WireMessage::ToolExecuting { .. } => {}
            WireMessage::ToolResult { id, output } => {
                self.engine.chat_mut().messages.push(Message {
                    role: Role::Tool,
                    content: output.clone(),
                    tool_calls: None,
                    tool_call_id: Some(id.clone()),
                    timestamp: Some(now_timestamp()),
                    reasoning_content: None,
                    is_prompt: false,
                });
            }
            WireMessage::TurnFinished { .. } => {
                self.finish_stream();
                self.engine.sanitize_assistant_content();
                self.engine.finish_agent_loop();
            }
            WireMessage::Error(err) => {
                self.finish_stream();
                self.engine.finish_agent_loop();
                self.engine.add_error_message(err.clone());
            }
            WireMessage::Cancelled => {
                self.finish_stream();
                self.engine.finish_agent_loop();
                self.engine.tool_executor_mut().clear_pending_tool_calls();
            }
            _ => {}
        }
    }

    fn ensure_last_message_is_assistant(&mut self) {
        use openlibertas_core::domain::{now_timestamp, Message, Role};

        if self.engine.chat().messages.last().map(|m| m.role) != Some(Role::Assistant) {
            if let Some(idx) = self
                .engine
                .chat()
                .messages
                .iter()
                .rposition(|m| m.role == Role::Assistant)
            {
                let pending = self.engine.tool_executor().pending_tool_calls().to_vec();
                if !pending.is_empty() && self.engine.chat().messages[idx].tool_calls.is_none() {
                    self.engine.chat_mut().messages[idx].tool_calls = Some(pending);
                }
            }
            self.engine.tool_executor_mut().clear_pending_tool_calls();
            self.engine.chat_mut().messages.push(Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: Some(now_timestamp()),
                reasoning_content: None,
                is_prompt: false,
            });
            self.engine.chat_mut().streaming = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_command() {
        let cmd = App::parse_slash_command("/help");
        assert_eq!(cmd, Some(SlashCommand::Help(None)));
    }

    #[test]
    fn parse_model_command_with_arg() {
        let cmd = App::parse_slash_command("/model gpt-4");
        assert_eq!(cmd, Some(SlashCommand::Model("gpt-4".to_string())));
    }

    #[test]
    fn parse_save_command_with_name() {
        let cmd = App::parse_slash_command("/save my-session");
        assert_eq!(cmd, Some(SlashCommand::Save("my-session".to_string())));
    }

    #[test]
    fn parse_unknown_command() {
        let cmd = App::parse_slash_command("/foobar");
        assert_eq!(cmd, Some(SlashCommand::Unknown("/foobar".to_string())));
    }

    #[test]
    fn parse_no_slash_returns_none() {
        let cmd = App::parse_slash_command("hello world");
        assert!(cmd.is_none());
    }

    #[test]
    fn autocomplete_suggestions_for_prefix() {
        let config = Config::default();
        let mut app = App::new(config);
        app.engine.input_mut().buffer = "/s".to_string();
        let suggestions = app.get_autocomplete_suggestions();
        assert!(suggestions.contains(&"/save"));
        assert!(suggestions.contains(&"/sessions"));
    }

    #[test]
    fn autocomplete_no_match_for_non_slash() {
        let config = Config::default();
        let app = App::new(config);
        let suggestions = app.get_autocomplete_suggestions();
        assert!(suggestions.is_empty());
    }

    #[test]
    fn history_navigation() {
        let config = Config::default();
        let mut app = App::new(config);
        app.engine.input_mut().history = vec!["first".to_string(), "second".to_string()];

        app.engine.history_prev();
        assert_eq!(app.engine.input_mut().buffer, "second");

        app.engine.history_prev();
        assert_eq!(app.engine.input_mut().buffer, "first");

        app.engine.history_next();
        assert_eq!(app.engine.input_mut().buffer, "second");
    }

    #[test]
    fn history_wraps_around() {
        let config = Config::default();
        let mut app = App::new(config);
        app.engine.input_mut().history = vec!["only".to_string()];

        app.engine.history_prev();
        assert_eq!(app.engine.input_mut().buffer, "only");

        app.engine.history_next();
        assert_eq!(app.engine.input_mut().buffer, "");
    }

    #[test]
    fn model_switch_validates_and_updates_provider() {
        let config = Config::default();
        let mut app = App::new(config);
        app.models.models = vec![
            Model {
                id: "qwen3-8b".to_string(),
                provider: ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "llama3-8b".to_string(),
                provider: ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];

        let result = app.execute_slash_command(SlashCommand::Model("qwen3-8b".to_string()));
        assert_eq!(app.models.current, Some("qwen3-8b".to_string()));
        assert_eq!(app.models.provider, ProviderId::new("local"));
        assert!(result.unwrap().contains("Switched to model: qwen3-8b"));
    }

    #[test]
    fn model_switch_fuzzy_match() {
        let config = Config::default();
        let mut app = App::new(config);
        app.models.models = vec![Model {
            id: "qwen3-8b-instruct".to_string(),
            provider: ProviderId::new("local"),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }];

        let result = app.execute_slash_command(SlashCommand::Model("instruct".to_string()));
        assert_eq!(app.models.current, Some("qwen3-8b-instruct".to_string()));
        assert!(result
            .unwrap()
            .contains("Switched to model: qwen3-8b-instruct"));
    }

    #[test]
    fn model_switch_not_found_shows_suggestions() {
        let config = Config::default();
        let mut app = App::new(config);
        app.models.models = vec![
            Model {
                id: "qwen3-8b".to_string(),
                provider: ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "qwen3-4b".to_string(),
                provider: ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];

        let result = app.execute_slash_command(SlashCommand::Model("nonexistent".to_string()));
        let msg = result.unwrap();
        assert!(msg.contains("not found"));
        assert!(msg.contains("/model"));
    }

    #[test]
    fn model_switch_partial_match_shows_suggestions() {
        let config = Config::default();
        let mut app = App::new(config);
        app.models.models = vec![
            Model {
                id: "qwen3-8b".to_string(),
                provider: ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "qwen3-4b".to_string(),
                provider: ProviderId::new("local"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];

        let result = app.execute_slash_command(SlashCommand::Model("qwen".to_string()));
        let msg = result.unwrap();
        assert!(msg.contains("not found"));
        assert!(msg.contains("qwen3-8b") || msg.contains("qwen3-4b"));
    }

    #[test]
    fn model_switch_empty_opens_picker() {
        let config = Config::default();
        let mut app = App::new(config);
        app.models.models = vec![Model {
            id: "qwen3-8b".to_string(),
            provider: ProviderId::new("local"),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }];

        let result = app.execute_slash_command(SlashCommand::Model(String::new()));
        assert_eq!(app.screen, Screen::Models);
        assert!(result.unwrap().contains("Select a model"));
    }

    #[test]
    fn agent_starts_disabled() {
        let config = Config::default();
        let app = App::new(config);
        assert_eq!(app.engine.agents().status, AgentModeStatus::Disabled);
        assert_eq!(app.engine.agents().max_iterations, 10);
        assert_eq!(app.engine.agents().current_iteration, 0);
    }

    #[test]
    fn agent_status_transitions() {
        let config = Config::default();
        let mut app = App::new(config);
        app.engine.agents_mut().status = AgentModeStatus::Idle;
        app.engine.start_agent_loop();
        assert_eq!(app.engine.agents_mut().status, AgentModeStatus::Active);
        assert_eq!(app.engine.agents().current_iteration, 0);

        app.engine.finish_agent_loop();
        assert_eq!(app.engine.agents_mut().status, AgentModeStatus::Idle);
        assert_eq!(app.engine.agents().current_iteration, 0);
    }

    #[test]
    fn agent_iteration_tracking() {
        let config = Config::default();
        let mut app = App::new(config);
        app.engine.agents_mut().status = AgentModeStatus::Idle;
        app.engine.start_agent_loop();
        assert_eq!(app.engine.agents().current_iteration, 0);

        app.engine.increment_agent_iteration();
        assert_eq!(app.engine.agents().current_iteration, 1);
        assert!(!app.engine.agent_iteration_exceeded());

        app.engine.agents_mut().current_iteration = 10;
        assert!(app.engine.agent_iteration_exceeded());
    }

    #[test]
    fn agent_persona_cycles() {
        let config = Config::default();
        let mut app = App::new(config);
        let initial = app.engine.agents().persona.clone();

        app.cycle_agent_persona();
        assert_ne!(app.engine.agents().persona, initial);
    }
}
