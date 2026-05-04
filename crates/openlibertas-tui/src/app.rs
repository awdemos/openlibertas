use crate::markdown::MarkdownRenderer;
use crate::theme::Theme;
use openlibertas_core::backend::{Message, Model, ToolCall, ToolDefinition};
use openlibertas_core::commands::{find_model, get_model_suggestions};
use openlibertas_core::config::Config;
use openlibertas_core::domain::ProviderId;
use openlibertas_core::domain::Role;
use openlibertas_core::export::{self, ExportFormat};
use openlibertas_core::mcp::{McpClient, McpTool};
use openlibertas_core::prompt::PromptManager;
use openlibertas_core::search;
use openlibertas_core::store::ConversationStore;

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

pub const SPINNER_FRAMES: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
];

pub struct ChatState {
    pub messages: Vec<Message>,
    pub scroll: usize,
    pub streaming: bool,
    pub auto_scroll: bool,
    pub cancel_token: tokio_util::sync::CancellationToken,
    pub spinner_frame: usize,
    pub compactor: openlibertas_core::conversation::ContextCompactor,
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
}

pub struct ModelState {
    pub models: Vec<Model>,
    pub selected: usize,
    pub current: Option<String>,
    pub provider: ProviderId,
}

pub struct PanelState {
    pub show_tools: bool,
    pub show_mcp: bool,
    pub show_sessions: bool,
    pub show_palette: bool,
    pub show_themes: bool,
    pub show_help: bool,
    pub show_agents: bool,
}

pub struct McpState {
    pub client: Option<std::sync::Arc<McpClient>>,
    pub available_tools: Vec<McpTool>,
    pub pending_tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<String>,
    pub server_statuses: std::collections::HashMap<String, openlibertas_core::domain::McpServerStatus>,
}

pub struct SearchState {
    pub matches: Vec<search::SearchMatch>,
    pub index: usize,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Disabled,
    Idle,
    Active,
}

pub struct AgentState {
    pub status: AgentStatus,
    pub max_iterations: usize,
    pub current_iteration: usize,
    pub persona: String,
    pub yolo_mode: bool,
}

pub struct App {
    pub screen: Screen,
    pub loading: bool,
    pub error: Option<String>,
    pub config: Config,
    pub connection_status: ConnectionStatus,
    pub chat: ChatState,
    pub input: InputState,
    pub models: ModelState,
    pub panels: PanelState,
    pub mcp: McpState,
    pub search: SearchState,
    pub agents: AgentState,
    pub store: Option<ConversationStore>,
    prompt_manager: PromptManager,
    pub system_prompt: Option<String>,
    pub palette_commands: Vec<(String, String)>,
    pub palette_selected: usize,
    pub markdown_renderer: MarkdownRenderer,
    pub theme: Theme,
    pub theme_selected: usize,
    pub agent_selected: usize,
}

impl App {
    pub fn new(config: Config) -> Self {
        let store = Config::data_dir()
            .and_then(|d| ConversationStore::new(d).ok());

        Self {
            screen: Screen::Chat,
            loading: true,
            error: None,
            config,
            connection_status: ConnectionStatus::Checking,
            chat: ChatState {
                messages: Vec::new(),
                scroll: 0,
                streaming: false,
                auto_scroll: true,
                cancel_token: tokio_util::sync::CancellationToken::new(),
                spinner_frame: 0,
                compactor: openlibertas_core::conversation::ContextCompactor::default(),
            },
            input: InputState {
                buffer: String::new(),
                cursor_pos: 0,
                history: Vec::new(),
                history_index: None,
                history_stash: String::new(),
                autocomplete_index: 0,
                show_autocomplete: false,
            },
            models: ModelState {
                models: Vec::new(),
                selected: 0,
                current: None,
                provider: ProviderId::new(""),
            },
            panels: PanelState {
                show_tools: false,
                show_mcp: false,
                show_sessions: false,
                show_palette: false,
                show_themes: false,
                show_help: false,
                show_agents: false,
            },
            mcp: McpState {
                client: None,
                available_tools: Vec::new(),
                pending_tool_calls: Vec::new(),
                tool_results: Vec::new(),
                server_statuses: std::collections::HashMap::new(),
            },
            search: SearchState {
                matches: Vec::new(),
                index: 0,
                active: false,
            },
            agents: AgentState {
                status: AgentStatus::Disabled,
                max_iterations: 10,
                current_iteration: 0,
                persona: "General".to_string(),
                yolo_mode: false,
            },
            store,
            prompt_manager: PromptManager::new(),
            system_prompt: None,
            palette_commands: Vec::new(),
            palette_selected: 0,
            markdown_renderer: MarkdownRenderer::new(),
            theme: Theme::Default,
            theme_selected: 0,
            agent_selected: 0,
        }
    }

    pub fn set_provider(&mut self, provider: impl Into<ProviderId>) {
        let provider = provider.into();
        let prompt = self.prompt_manager.get_prompt(provider.as_str());
        self.system_prompt = Some(prompt.to_string());
        self.models.provider = provider;
    }

    #[cfg(test)]
    fn get_autocomplete_suggestions(&self) -> Vec<&'static str> {
        SlashCommand::autocomplete(&self.input.buffer)
    }

    pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
        SlashCommand::parse(input)
    }

    pub fn execute_slash_command(&mut self, cmd: SlashCommand) -> Option<String> {
        match cmd {
            SlashCommand::Help => {
                self.panels.show_help = true;
                None
            }
            SlashCommand::Version => {
                Some(format!("OpenLibertas v{}", env!("CARGO_PKG_VERSION")))
            }
            SlashCommand::Tools => {
                self.panels.show_tools = !self.panels.show_tools;
                self.panels.show_mcp = false;
                if self.panels.show_tools {
                    let tool_list = if self.mcp.available_tools.is_empty() {
                        "No tools available. Check MCP server configuration.".to_string()
                    } else {
                        self.mcp.available_tools
                            .iter()
                            .map(|t| format!("  {}: {}", t.name, t.description))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    Some(format!("Available Tools ({})\n{}", self.mcp.available_tools.len(), tool_list))
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
                            let suggestions = get_model_suggestions(&self.models.models, &model_name);
                            if suggestions.is_empty() {
                                Some(format!("Model '{}' not found. Use /model to see available models.", model_name))
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
                self.chat.messages.clear();
                self.chat.scroll = 0;
                Some("Conversation cleared".to_string())
            }
            SlashCommand::Quit => None,
            SlashCommand::Agents => {
                if self.agents.status == AgentStatus::Disabled {
                    self.agents.status = AgentStatus::Idle;
                    return Some(format!(
                        "Agents enabled (Persona: {})",
                        self.agents.persona
                    ));
                }
                self.panels.show_agents = true;
                self.agent_selected = 0;
                None
            }
            SlashCommand::Yolo => {
                self.agents.yolo_mode = !self.agents.yolo_mode;
                if self.agents.yolo_mode {
                    Some("YOLO mode enabled. Destructive tools will execute without confirmation.".to_string())
                } else {
                    Some("YOLO mode disabled. Destructive tools will require confirmation.".to_string())
                }
            }
            SlashCommand::Compact => {
                let before = self.chat.messages.len();
                let compacted = self.chat.compactor.compact(&self.chat.messages);
                let after = compacted.len();
                if after < before {
                    self.chat.messages = compacted;
                    Some(format!("Context compacted: {} → {} messages", before, after))
                } else {
                    Some(format!("No compaction needed ({} messages)", before))
                }
            }
            SlashCommand::Edit(n) => {
                let user_msgs: Vec<(usize, &Message)> = self.chat.messages.iter().enumerate()
                    .filter(|(_, m)| m.role == Role::User)
                    .collect();
                if n == 0 || n > user_msgs.len() {
                    Some(format!("Invalid message number. There are {} user messages. Use /edit 1..{}", user_msgs.len(), user_msgs.len()))
                } else {
                    let (idx, msg) = user_msgs[n - 1];
                    self.input.buffer = msg.content.clone();
                    self.input.cursor_pos = msg.content.len();
                    self.chat.messages.truncate(idx);
                    Some(format!("Editing message {}. Press Enter to resend.", n))
                }
            }
            SlashCommand::Remove(n) => {
                if n == 0 || n > self.chat.messages.len() {
                    Some(format!("Invalid message number. There are {} messages. Use /remove 1..{}", self.chat.messages.len(), self.chat.messages.len()))
                } else {
                    let removed = self.chat.messages.remove(n - 1);
                    let preview = if removed.content.len() > 40 {
                        format!("{}...", &removed.content[..40])
                    } else {
                        removed.content.clone()
                    };
                    Some(format!("Removed message {}: [{}] {}", n, removed.role, preview))
                }
            }
            SlashCommand::Mcp => {
                self.panels.show_mcp = !self.panels.show_mcp;
                self.panels.show_tools = false;
                if self.panels.show_mcp {
                    if let Some(client) = &self.mcp.client {
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
                    match store.save_markdown(&id, self.models.current.as_deref(), &self.chat.messages) {
                        Ok(_) => Some(format!("Session '{}' saved", id)),
                        Err(e) => Some(format!("Failed to save: {}", e)),
                    }
                } else {
                    Some("Store not available".to_string())
                }
            }
            SlashCommand::Load(name) => {
                if name.is_empty() {
                    self.panels.show_sessions = true;
                    Some("Session manager opened".to_string())
                } else if let Some(ref store) = self.store {
                    match openlibertas_core::commands::load_session(store, &name) {
                        Ok(session) => {
                            self.chat.messages = session.messages;
                            self.chat.scroll = 0;
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
                self.panels.show_sessions = !self.panels.show_sessions;
                self.panels.show_tools = false;
                self.panels.show_mcp = false;
                if self.panels.show_sessions {
                    Some("Session manager opened".to_string())
                } else {
                    Some("Session manager closed".to_string())
                }
            }
            SlashCommand::Delete(name) => {
                if name.is_empty() {
                    self.panels.show_sessions = true;
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
                    self.search.matches = search::search_messages(&self.chat.messages, &query);
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
                            Some(format!("Unknown theme '{}'. Available: {}", name, available))
                        }
                    }
                }
            }
            SlashCommand::New => {
                self.chat.messages.clear();
                self.chat.scroll = 0;
                self.input.buffer.clear();
                self.mcp.pending_tool_calls.clear();
                self.mcp.tool_results.clear();
                let loaded = self.load_context_files();
                if loaded.is_empty() {
                    Some("New session started".to_string())
                } else {
                    Some(format!("New session started. Loaded: {}", loaded.join(", ")))
                }
            }
            SlashCommand::Undo => {
                if self.chat.messages.len() >= 2 {
                    self.chat.messages.pop();
                    self.chat.messages.pop();
                    Some("Undid last turn".to_string())
                } else {
                    Some("Nothing to undo".to_string())
                }
            }
            SlashCommand::Title(name) => {
                if name.is_empty() {
                    Some("Usage: /title <name>".to_string())
                } else {
                    Some(format!("Session title set to: '{}' (Note: titles are not yet persisted)", name))
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
        let mut messages = self.chat.messages.clone();

        let content = openlibertas_core::conversation::parse_file_context(&self.input.buffer);

        if self.agents.status == AgentStatus::Active {
            messages.insert(0, Message {
                role: Role::System,
                content: self.agent_system_prompt(),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        if let Some(prompt) = self.system_prompt.as_deref() {
            if !messages.iter().any(|m| m.role == Role::System && m.content == prompt) {
                messages.insert(0, Message {
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

        self.input.buffer.clear();
        self.chat.messages.push(Message {
            role: Role::User,
            content,
            tool_calls: None,
            tool_call_id: None,
        });
        self.chat.messages.push(Message {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: None,
            tool_call_id: None,
        });
        self.chat.streaming = true;
        self.chat.auto_scroll = true;
        self.mcp.pending_tool_calls.clear();
        messages
    }

    pub fn load_context_files(&mut self) -> Vec<String> {
        let loaded = openlibertas_core::conversation::read_context_files();
        for (filename, content) in &loaded {
            self.chat.messages.push(Message {
                role: Role::System,
                content: format!("--- {} ---\n{}", filename, content),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        loaded.into_iter().map(|(name, _)| name).collect()
    }

    pub fn append_stream_chunk(&mut self, chunk: &str) {
        if let Some(last) = self.chat.messages.last_mut() {
            last.content.push_str(chunk);
        }
        self.chat.auto_scroll = true;
    }

    pub fn add_tool_call(&mut self, tool_call: ToolCall) {
        self.mcp.pending_tool_calls.push(tool_call);
    }

    pub fn finish_stream(&mut self) {
        self.chat.streaming = false;
        // Attach pending tool calls to the last assistant message so the
        // conversation history is valid for the API.
        if !self.mcp.pending_tool_calls.is_empty() {
            if let Some(last) = self.chat.messages.last_mut() {
                if last.role == Role::Assistant {
                    last.tool_calls = Some(self.mcp.pending_tool_calls.clone());
                }
            }
        }
        self.autosave();
    }

    pub fn autosave(&self) {
        if let Some(ref store) = self.store {
            let model = self.models.current.as_deref().unwrap_or("unknown");
            let id = format!("autosave-{}.md", model.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-").replace("--", "-"));
            let _ = store.save_markdown(&id, self.models.current.as_deref(), &self.chat.messages);
        }
    }

    pub fn advance_spinner(&mut self) {
        if self.chat.streaming {
            self.chat.spinner_frame = (self.chat.spinner_frame + 1) % SPINNER_FRAMES.len();
        }
    }

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

    pub fn agent_personas(&self) -> Vec<(String, String)> {
        let mut personas = Vec::new();
        
        // Try to load from personas directory
        if let Ok(entries) = std::fs::read_dir("personas") {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "md") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut lines = content.lines();
                        let name = lines.next()
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
        
        // Fall back to built-in personas if no files found
        if personas.is_empty() {
            personas.push(("General".to_string(), "You are an autonomous agent. You have access to tools that can help you complete tasks. \
When you need to use a tool, call it. After receiving tool results, analyze them and decide \
whether you need to call more tools or provide a final answer. Keep iterating with tools until \
the task is fully complete. Be thorough and precise in your reasoning.".to_string()));
            personas.push(("Coding".to_string(), "You are a senior software engineer with access to development tools. \
You write clean, well-tested code. When using tools, prefer file operations and code analysis. \
Think step by step and verify your changes compile and pass tests.".to_string()));
            personas.push(("Research".to_string(), "You are a research assistant with access to search and analysis tools. \
You gather information thoroughly, verify facts, and synthesize comprehensive answers. \
Cite sources when possible and note uncertainty.".to_string()));
            personas.push(("Creative".to_string(), "You are a creative assistant with access to media and design tools. \
You think outside the box and provide novel, inspiring solutions. Iterate on ideas using \
available tools to refine and polish your work.".to_string()));
        }
        
        personas
    }

    pub fn agent_system_prompt(&self) -> String {
        let personas = self.agent_personas();
        personas
            .iter()
            .find(|(name, _)| name == &self.agents.persona)
            .map(|(_, prompt)| prompt.clone())
            .unwrap_or_else(|| personas[0].1.clone())
    }

    pub fn cycle_agent_persona(&mut self) -> String {
        let personas = self.agent_personas();
        let current = personas.iter().position(|(n, _)| *n == self.agents.persona).unwrap_or(0);
        let next = (current + 1) % personas.len();
        self.agents.persona = personas[next].0.to_string();
        self.agents.persona.clone()
    }

    pub fn add_error_message(&mut self, error: String) {
        self.chat.messages.push(Message {
            role: Role::System,
            content: error,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    pub fn export_conversation(&self, format: ExportFormat) -> String {
        export::export_messages(&self.chat.messages, self.models.current.as_deref(), format)
    }

    pub fn add_system_message(&mut self, content: String) {
        self.chat.messages.push(Message {
            role: Role::System,
            content,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    pub fn tool_needs_approval(tool_name: &str) -> bool {
        let destructive = [
            "write_file", "writefile", "writeFile",
            "edit_file", "editfile", "editFile",
            "shell", "execute", "exec",
            "bash", "sh", "run_command",
            "delete_file", "deletefile", "deleteFile", "remove_file",
            "create_file", "createfile", "createFile",
            "patch", "apply_patch", "applyPatch",
        ];
        destructive.iter().any(|&d| tool_name.eq_ignore_ascii_case(d))
    }

    pub fn extract_key_argument(tool_name: &str, arguments: &str) -> String {
        let parsed: serde_json::Value = serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
        let key_fields = match tool_name.to_lowercase().as_str() {
            n if n.contains("read") => vec!["path", "file", "uri", "url"],
            n if n.contains("write") || n.contains("edit") => vec!["path", "file", "uri"],
            n if n.contains("shell") || n.contains("exec") || n.contains("run") || n.contains("bash") => vec!["command", "cmd", "script"],
            n if n.contains("search") => vec!["query", "q", "pattern", "term"],
            n if n.contains("fetch") || n.contains("get") || n.contains("download") => vec!["url", "uri", "path"],
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

    pub fn scroll_page_up(&mut self) {
        self.chat.scroll = self.chat.scroll.saturating_sub(10);
        self.chat.auto_scroll = false;
    }

    pub fn scroll_page_down(&mut self) {
        self.chat.scroll += 10;
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
            for (i, msg) in self.chat.messages.iter().enumerate() {
                if i == target {
                    self.chat.scroll = line_count;
                    self.chat.auto_scroll = false;
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

    pub fn history_prev(&mut self) {
        if self.input.history.is_empty() {
            return;
        }
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

    pub fn push_to_history(&mut self, input: String) {
        if !input.trim().is_empty() {
            self.input.history.push(input);
        }
        self.input.history_index = None;
        self.input.history_stash.clear();
    }

    pub fn move_cursor_left(&mut self) {
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        self.input.cursor_pos = if self.input.buffer.is_char_boundary(pos) {
            self.input.buffer[..pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        } else {
            self.input.buffer.char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < pos)
                .last()
                .unwrap_or(0)
        };
    }

    pub fn move_cursor_right(&mut self) {
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        self.input.cursor_pos = if self.input.buffer.is_char_boundary(pos) {
            self.input.buffer[pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| pos + i)
                .unwrap_or(self.input.buffer.len())
        } else {
            self.input.buffer.char_indices()
                .map(|(i, _)| i)
                .find(|&i| i > pos)
                .unwrap_or(self.input.buffer.len())
        };
    }

    pub fn move_cursor_to_start(&mut self) {
        self.input.cursor_pos = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.input.cursor_pos = self.input.buffer.len();
    }

    pub fn delete_word_backward(&mut self) {
        if self.input.cursor_pos == 0 {
            return;
        }
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = if self.input.buffer.is_char_boundary(pos) {
            pos
        } else {
            self.input.buffer.char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < pos)
                .last()
                .unwrap_or(0)
        };
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
        self.panels.show_themes = false;
    }

    pub fn open_themes_panel(&mut self) {
        let themes = Theme::all();
        self.theme_selected = themes.iter().position(|(_, t)| *t == self.theme).unwrap_or(0);
        self.panels.show_themes = true;
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
                self.agents.status = if self.agents.status == AgentStatus::Disabled {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Disabled
                };
            }
            1 => {
                self.cycle_agent_persona();
            }
            2 => {
                self.agents.max_iterations = if self.agents.max_iterations >= 50 {
                    5
                } else {
                    (self.agents.max_iterations + 5).min(50)
                };
            }
            _ => {}
        }
    }

    pub fn update_command_palette(&mut self) {
        let prefix = self.input.buffer.to_lowercase();
        self.palette_commands = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(&prefix))
            .map(|cmd| {
                (cmd.to_string(), openlibertas_core::commands::command_description(cmd).to_string())
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
            self.palette_selected = (self.palette_selected + 1).min(self.palette_commands.len() - 1);
        }
    }

    pub fn select_palette_command(&mut self) -> Option<String> {
        self.palette_commands.get(self.palette_selected).map(|(cmd, _)| cmd.clone())
    }

    pub fn go_to_models(&mut self) {
        self.screen = Screen::Models;
        self.chat.streaming = false;
        self.panels.show_tools = false;
        self.panels.show_mcp = false;
        self.panels.show_sessions = false;
        self.panels.show_help = false;
    }

    pub fn get_tools_for_request(&self) -> Option<Vec<ToolDefinition>> {
        openlibertas_core::conversation::get_tools_for_request(&self.mcp.available_tools)
    }

    pub fn build_tool_result_messages(&self) -> Vec<Message> {
        let mut messages = self.chat.messages.clone();

        if self.agents.status == AgentStatus::Active {
            messages.insert(0, Message {
                role: Role::System,
                content: self.agent_system_prompt(),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        openlibertas_core::conversation::build_tool_result_messages(
            &messages,
            &self.mcp.pending_tool_calls,
            &self.mcp.tool_results,
        )
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
            Model { id: "qwen3-8b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
            Model { id: "llama3-8b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
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
        app.models.models = vec![
            Model { id: "qwen3-8b-instruct".to_string(), provider: ProviderId::new("local"), supports_tools: true },
        ];

        let result = app.execute_slash_command(SlashCommand::Model("instruct".to_string()));
        assert_eq!(app.models.current, Some("qwen3-8b-instruct".to_string()));
        assert!(result.unwrap().contains("Switched to model: qwen3-8b-instruct"));
    }

    #[test]
    fn model_switch_not_found_shows_suggestions() {
        let config = Config::default();
        let mut app = App::new(config);
        app.models.models = vec![
            Model { id: "qwen3-8b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
            Model { id: "qwen3-4b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
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
            Model { id: "qwen3-8b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
            Model { id: "qwen3-4b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
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
        app.models.models = vec![
            Model { id: "qwen3-8b".to_string(), provider: ProviderId::new("local"), supports_tools: true },
        ];

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
