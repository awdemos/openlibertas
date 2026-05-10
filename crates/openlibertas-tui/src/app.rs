//! Application state and command execution for the TUI.
//!
//! The `App` struct holds all mutable UI state and handles:
//! - Slash command parsing and execution
//! - Model selection and provider switching
//! - Session management (save/load/delete)
//! - File context loading
//! - Voice mode state coordination

use crate::markdown::MarkdownRenderer;
use crate::theme::Theme;
use openlibertas_core::domain::{Message, Model, ToolCall, ToolDefinition};
use openlibertas_core::env_context::EnvContext;
use openlibertas_core::commands::{find_model, get_model_suggestions};
use openlibertas_core::config::Config;
use openlibertas_core::domain::ProviderId;
use openlibertas_core::domain::Role;
use openlibertas_core::engine::{AgentStatus, ChatEngine};
use openlibertas_core::export::{self, ExportFormat};
use openlibertas_core::prompt::PromptManager;
use openlibertas_core::search;
use openlibertas_core::store::ConversationStore;
use openlibertas_core::voice::{VoiceManager, VoiceState};
use std::ops::{Deref, DerefMut};
use std::time::Instant;

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
    pub store: Option<ConversationStore>,
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
    pub mouse_enabled: bool,
    pub last_click_time: Option<Instant>,
    pub last_click_pos: Option<(u16, u16)>,
}

impl Deref for App {
    type Target = ChatEngine;
    fn deref(&self) -> &Self::Target {
        &self.engine
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.engine
    }
}

impl App {
    pub fn new(config: Config) -> Self {
        let store = Config::data_dir().and_then(|d| ConversationStore::new(d).ok());
        let voice_api_key = config.elevenlabs_api_key.clone();
        let voice_id = config.elevenlabs_voice_id.clone();
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
            },
            overlay: Overlay::None,
            search: SearchState {
                matches: Vec::new(),
                index: 0,
                active: false,
            },
            store,
            engine: ChatEngine::new().with_env_context(EnvContext::detect()),
            prompt_manager: PromptManager::new(),
            palette_commands: Vec::new(),
            palette_selected: 0,
            markdown_renderer: MarkdownRenderer::new(),
            theme: Theme::Default,
            theme_selected: 0,
            agent_selected: 0,
            voice: VoiceManager::new(voice_api_key, voice_id),
            voice_status: None,
            mouse_enabled: true,
            last_click_time: None,
            last_click_pos: None,
        }
    }

    pub fn set_provider(&mut self, provider: impl Into<ProviderId>) {
        let provider = provider.into();
        let prompt = self.prompt_manager.get_prompt(provider.as_str());
        self.engine.system_prompt = Some(prompt.to_string());
        self.models.provider = provider;
    }

    #[cfg(test)]
    fn get_autocomplete_suggestions(&self) -> Vec<&'static str> {
        SlashCommand::autocomplete(&self.engine.input.buffer)
    }

    pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
        SlashCommand::parse(input)
    }

    pub fn execute_slash_command(&mut self, cmd: SlashCommand) -> Option<String> {
        match cmd {
            SlashCommand::Help => {
                self.overlay = Overlay::Help;
                None
            }
            SlashCommand::Version => Some(format!("OpenLibertas v{}", env!("CARGO_PKG_VERSION"))),
            SlashCommand::Tools => {
                self.overlay = if self.overlay == Overlay::Tools {
                    Overlay::None
                } else {
                    Overlay::Tools
                };
                if self.overlay == Overlay::Tools {
                    let tool_list = if self.engine.tools.available_tools.is_empty() {
                        "No tools available. Check MCP server configuration.".to_string()
                    } else {
                        self.engine
                            .tools
                            .available_tools
                            .iter()
                            .map(|t| format!("  {}: {}", t.name, t.description))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    Some(format!(
                        "Available Tools ({}\n{}",
                        self.engine.tools.available_tools.len(),
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
                        Some((idx, matched_name)) => {
                            self.models.selected = idx;
                            self.models.current = Some(matched_name.clone());
                            if let Some(model) = self.models.models.get(idx) {
                                self.set_provider(model.provider.clone());
                            }
                            Some(format!("Switched to model: {}", matched_name))
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
                Some("Conversation cleared".to_string())
            }
            SlashCommand::Quit => None,
            SlashCommand::Agents => {
                if self.engine.agents.status == AgentStatus::Disabled {
                    self.engine.agents.status = AgentStatus::Idle;
                    return Some(format!(
                        "Agents enabled (Persona: {})",
                        self.engine.agents.persona
                    ));
                }
                self.overlay = Overlay::Agents;
                self.agent_selected = 0;
                None
            }
            SlashCommand::Yolo => {
                self.engine.agents.yolo_mode = !self.engine.agents.yolo_mode;
                if self.engine.agents.yolo_mode {
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
            SlashCommand::Edit(n) => {
                let user_msgs: Vec<(usize, &Message)> = self
                    .engine
                    .chat
                    .messages
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.role == Role::User)
                    .collect();
                if n == 0 || n > user_msgs.len() {
                    Some(format!(
                        "Invalid message number. There are {} user messages. Use /edit 1..{}",
                        user_msgs.len(),
                        user_msgs.len()
                    ))
                } else {
                    let (idx, msg) = user_msgs[n - 1];
                    self.engine.input.buffer = msg.content.clone();
                    self.engine.input.cursor_pos = msg.content.len();
                    self.engine.chat.messages.truncate(idx);
                    Some(format!("Editing message {}. Press Enter to resend.", n))
                }
            }
            SlashCommand::Remove(n) => {
                if n == 0 || n > self.engine.chat.messages.len() {
                    Some(format!(
                        "Invalid message number. There are {} messages. Use /remove 1..{}",
                        self.engine.chat.messages.len(),
                        self.engine.chat.messages.len()
                    ))
                } else {
                    let removed = self.engine.chat.messages.remove(n - 1);
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
                    if let Some(client) = &self.engine.tools.client {
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
                        ConversationStore::generate_name(model)
                    } else {
                        name
                    };
                    let id = if id.ends_with(".md") {
                        id
                    } else {
                        format!("{}.md", id)
                    };
                    match store.save_markdown(
                        &id,
                        self.models.current.as_deref(),
                        &self.engine.chat.messages,
                    ) {
                        Ok(_) => Some(format!("Session '{}' saved", id)),
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
                            self.engine.chat.messages = session.messages;
                            self.engine.chat.scroll = 0;
                            if let Some(ref model) = session.model {
                                self.models.current = Some(model.clone());
                                if let Some((idx, _)) = find_model(&self.models.models, model) {
                                    self.models.selected = idx;
                                    if let Some(m) = self.models.models.get(idx) {
                                        self.set_provider(m.provider.clone());
                                    }
                                }
                            }
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
            SlashCommand::Export(filename) => {
                let fname = if filename.is_empty() {
                    "chat.md".to_string()
                } else {
                    filename
                };
                let format = ExportFormat::from_extension(&fname);
                let content = self.export_conversation(format);
                match std::fs::write(&fname, content) {
                    Ok(_) => Some(format!("Exported to '{}'", fname)),
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
                        search::search_messages(&self.engine.chat.messages, &query);
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
                self.engine.input.buffer.clear();
                self.engine.tools.pending_tool_calls.clear();
                self.engine.tools.tool_results.clear();
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
                if self.engine.chat.messages.len() >= 2 {
                    self.engine.chat.messages.pop();
                    self.engine.chat.messages.pop();
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
            SlashCommand::Voice => {
                let enabled = self.voice.toggle();
                if enabled {
                    if self.voice.config.api_key.is_none() {
                        self.voice.enabled = false;
                        self.voice.state = VoiceState::Idle;
                        Some("Voice mode requires an ElevenLabs API key. Set ELEVENLABS_API_KEY or add elevenlabs_api_key to config.toml".to_string())
                    } else {
                        Some(
                            "Voice mode enabled. Hold Ctrl+Space to record, release to send."
                                .to_string(),
                        )
                    }
                } else {
                    self.voice.cancel();
                    Some("Voice mode disabled.".to_string())
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
                    Some(format!("Voice input device set to: {}", device))
                }
            }
            SlashCommand::Mouse => {
                self.mouse_enabled = !self.mouse_enabled;
                if self.mouse_enabled {
                    Some("Mouse capture enabled. Shift+drag to select text natively. Double-click or right-click a message to copy. Scroll to scroll chat. Disable with /mouse.".to_string())
                } else {
                    Some("Mouse capture disabled. tmux can now handle mouse selection and copy mode.".to_string())
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

    pub fn push_user_message(&mut self) -> Vec<Message> {
        let content =
            openlibertas_core::conversation::parse_file_context(&self.engine.input.buffer);
        if self.engine.agents.status == AgentStatus::Active {
            self.engine.agent_prompt = Some(self.agent_system_prompt());
        }
        self.engine.push_user_message(content)
    }

    pub fn load_context_files(&mut self) -> Vec<String> {
        let loaded = openlibertas_core::conversation::read_context_files();
        for (filename, content) in &loaded {
            self.engine.chat.messages.push(Message { role: Role::System, content: format!("--- {} ---\n{}", filename, content), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None });
        }
        loaded.into_iter().map(|(name, _)| name).collect()
    }

    pub fn finish_stream(&mut self) {
        self.engine.finish_stream();
        let _ = self.autosave();
    }

    pub fn autosave(&self) -> Option<String> {
        if !self.config.auto_save {
            return None;
        }
        if let Some(ref store) = self.store {
            let model = self.models.current.as_deref().unwrap_or("unknown");
            let id = format!(
                "autosave-{}.md",
                model
                    .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
                    .replace("--", "-")
            );
            match store.save_markdown(
                &id,
                self.models.current.as_deref(),
                &self.engine.chat.messages,
            ) {
                Ok(_) => None,
                Err(e) => Some(format!("Auto-save failed: {}", e)),
            }
        } else {
            Some("Auto-save failed: no conversation store".to_string())
        }
    }

    pub fn switch_agent_persona(&mut self, persona: &str, prompt: &str) {
        self.engine.switch_persona(persona, prompt);
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

    pub fn agent_system_prompt(&self) -> String {
        let personas = self.agent_personas();
        personas
            .iter()
            .find(|(name, _)| name == &self.engine.agents.persona)
            .map(|(_, prompt)| prompt.clone())
            .unwrap_or_else(|| personas[0].1.clone())
    }

    pub fn get_persona_prompt(&self, persona_name: &str) -> Option<String> {
        let personas = self.agent_personas();
        personas
            .iter()
            .find(|(name, _)| name == persona_name)
            .map(|(_, prompt)| prompt.clone())
    }

    pub fn cycle_agent_persona(&mut self) -> String {
        let personas = self.agent_personas();
        let current = personas
            .iter()
            .position(|(n, _)| *n == self.engine.agents.persona)
            .unwrap_or(0);
        let next = (current + 1) % personas.len();
        self.engine.agents.persona = personas[next].0.to_string();
        self.engine.agents.persona.clone()
    }

    pub fn export_conversation(&self, format: ExportFormat) -> String {
        export::export_messages(
            &self.engine.chat.messages,
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
            for (i, msg) in self.engine.chat.messages.iter().enumerate() {
                if i == target {
                    self.engine.chat.scroll = line_count;
                    self.engine.chat.auto_scroll = false;
                    break;
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
        self.agent_selected = (self.agent_selected + 1).min(2);
    }

    pub fn select_agent_option(&mut self) {
        match self.agent_selected {
            0 => {
                self.engine.agents.status = if self.engine.agents.status == AgentStatus::Disabled {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Disabled
                };
            }
            1 => {
                self.cycle_agent_persona();
            }
            2 => {
                self.engine.agents.max_iterations = if self.engine.agents.max_iterations >= 50 {
                    5
                } else {
                    (self.engine.agents.max_iterations + 5).min(50)
                };
            }
            _ => {}
        }
    }

    pub fn update_command_palette(&mut self) {
        let prefix = self.engine.input.buffer.to_lowercase();
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
        self.engine.chat.streaming = false;
        self.overlay = Overlay::None;
    }

    pub fn voice(&self) -> &VoiceManager {
        &self.voice
    }
    pub fn voice_mut(&mut self) -> &mut VoiceManager {
        &mut self.voice
    }
    pub fn search(&self) -> &SearchState {
        &self.search
    }
    pub fn search_mut(&mut self) -> &mut SearchState {
        &mut self.search
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    pub fn theme_mut(&mut self) -> &mut Theme {
        &mut self.theme
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_command() {
        let cmd = App::parse_slash_command("/help");
        assert_eq!(cmd, Some(SlashCommand::Help));
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
        app.input.buffer = "/s".to_string();
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
        app.input.history = vec!["first".to_string(), "second".to_string()];

        app.history_prev();
        assert_eq!(app.input.buffer, "second");

        app.history_prev();
        assert_eq!(app.input.buffer, "first");

        app.history_next();
        assert_eq!(app.input.buffer, "second");
    }

    #[test]
    fn history_wraps_around() {
        let config = Config::default();
        let mut app = App::new(config);
        app.input.history = vec!["only".to_string()];

        app.history_prev();
        assert_eq!(app.input.buffer, "only");

        app.history_next();
        assert_eq!(app.input.buffer, "");
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
        assert_eq!(app.agents.status, AgentStatus::Disabled);
        assert_eq!(app.agents.max_iterations, 10);
        assert_eq!(app.agents.current_iteration, 0);
    }

    #[test]
    fn agent_status_transitions() {
        let config = Config::default();
        let mut app = App::new(config);
        app.agents.status = AgentStatus::Idle;
        app.start_agent_loop();
        assert_eq!(app.agents.status, AgentStatus::Active);
        assert_eq!(app.agents.current_iteration, 0);

        app.finish_agent_loop();
        assert_eq!(app.agents.status, AgentStatus::Idle);
        assert_eq!(app.agents.current_iteration, 0);
    }

    #[test]
    fn agent_iteration_tracking() {
        let config = Config::default();
        let mut app = App::new(config);
        app.agents.status = AgentStatus::Idle;
        app.start_agent_loop();
        assert_eq!(app.agents.current_iteration, 0);

        app.increment_agent_iteration();
        assert_eq!(app.agents.current_iteration, 1);
        assert!(!app.agent_iteration_exceeded());

        app.agents.current_iteration = 10;
        assert!(app.agent_iteration_exceeded());
    }

    #[test]
    fn agent_persona_cycles() {
        let config = Config::default();
        let mut app = App::new(config);
        let initial = app.agents.persona.clone();

        app.cycle_agent_persona();
        assert_ne!(app.agents.persona, initial);
    }

    #[test]
    fn agent_system_prompt_returns_content() {
        let config = Config::default();
        let app = App::new(config);
        let prompt = app.agent_system_prompt();
        assert!(!prompt.is_empty());
    }
}
