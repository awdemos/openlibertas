use crate::commands::{find_model, get_model_suggestions, SlashCommand};
use crate::config::Config;
use crate::domain::{Message, Model, ProviderId, Role};
use crate::engine::{AgentMode, AgentModeStatus, ChatEngine};
use crate::export::{self, ExportFormat};
use crate::prompt::PromptManager;
use crate::search;
use crate::session::SessionManager;
use crate::store::SessionStore;
use crate::voice::VoiceManager;

/// UI-agnostic result of executing a slash command.
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Command succeeded silently.
    Ok,
    /// Show a message to the user.
    Message(String),
    /// Show an error message.
    Error(String),
    /// Toggle a named overlay panel.
    ToggleOverlay(String),
    /// Exit the application.
    Exit,
}

/// Business-logic executor for slash commands.
///
/// Holds mutable references to all state needed to execute commands
/// without touching any TUI-specific types.
pub struct CommandExecutor<'a> {
    pub engine: &'a mut ChatEngine,
    pub session_manager: &'a mut Option<SessionManager>,
    pub models: &'a [Model],
    pub current_model: &'a mut Option<String>,
    pub provider: &'a mut ProviderId,
    pub config: &'a mut Config,
    pub voice: &'a mut VoiceManager,
    pub temperature: &'a mut Option<f32>,
    pub rlm_mode: &'a mut bool,
    pub mouse_enabled: &'a mut bool,
    pub avatar_enabled: &'a mut bool,
    pub prompt_manager: &'a PromptManager,
}

impl<'a> CommandExecutor<'a> {
    fn set_provider(&mut self, provider: ProviderId) {
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
        *self.provider = provider;
    }

    pub fn execute(&mut self, cmd: SlashCommand) -> CommandResult {
        match cmd {
            SlashCommand::Help(arg) => self.cmd_help(arg),
            SlashCommand::Tools => self.cmd_tools(),
            SlashCommand::Model(name) => self.cmd_model(name),
            SlashCommand::Clear => self.cmd_clear(),
            SlashCommand::Quit => CommandResult::Exit,
            SlashCommand::Agents => self.cmd_agents(),
            SlashCommand::Avatar(name) => self.cmd_avatar(name),
            SlashCommand::AvatarMenu => CommandResult::ToggleOverlay("avatar_menu".to_string()),
            SlashCommand::Yolo => self.cmd_yolo(),
            SlashCommand::Plan => self.cmd_plan(),
            SlashCommand::Compact => self.cmd_compact(),
            SlashCommand::Rlm => self.cmd_rlm(),
            SlashCommand::Edit(n) => self.cmd_edit(n),
            SlashCommand::Remove(n) => self.cmd_remove(n),
            SlashCommand::Mcp => CommandResult::ToggleOverlay("mcp".to_string()),
            SlashCommand::Save(name) => self.cmd_save(name),
            SlashCommand::Load(name) => self.cmd_load(name),
            SlashCommand::Sessions => CommandResult::ToggleOverlay("sessions".to_string()),
            SlashCommand::Delete(name) => self.cmd_delete(name),
            SlashCommand::Export(arg) => self.cmd_export(arg),
            SlashCommand::Search(query) => self.cmd_search(query),
            SlashCommand::Theme(name) => self.cmd_theme(name),
            SlashCommand::New => self.cmd_new(),
            SlashCommand::Undo => self.cmd_undo(),
            SlashCommand::Title(name) => self.cmd_title(name),
            SlashCommand::Branch(msg_idx) => self.cmd_branch(msg_idx),
            SlashCommand::Voice => self.cmd_voice(),
            SlashCommand::VoiceDevice(device) => self.cmd_voice_device(device),
            SlashCommand::Temperature(temp) => {
                *self.temperature = Some(temp);
                CommandResult::Message(format!("Temperature set to: {}", temp))
            }
            SlashCommand::SetKey(provider, key) => self.cmd_set_key(provider, key),
            SlashCommand::Mouse => {
                *self.mouse_enabled = !*self.mouse_enabled;
                if *self.mouse_enabled {
                    CommandResult::Message(
                        "Mouse capture enabled — wheel scroll, click to focus, right-click to copy. Native text selection disabled. Disable with /mouse.".to_string(),
                    )
                } else {
                    CommandResult::Message(
                        "Mouse capture disabled — native terminal text selection and copy work. Enable with /mouse for wheel scrolling and clicking.".to_string(),
                    )
                }
            }
            SlashCommand::Unknown(cmd) => CommandResult::Error(format!("Unknown command: {}", cmd)),
        }
    }

    fn cmd_help(&self, arg: Option<String>) -> CommandResult {
        if let Some(cmd_name) = arg {
            let cmd_name = cmd_name.trim();
            if cmd_name.is_empty() {
                CommandResult::ToggleOverlay("help".to_string())
            } else {
                match super::command_detailed_help(cmd_name) {
                    Some(help) => CommandResult::Message(help),
                    None => CommandResult::Message(format!(
                        "No detailed help for '/{}'. Use /help to see all commands.",
                        cmd_name
                    )),
                }
            }
        } else {
            CommandResult::ToggleOverlay("help".to_string())
        }
    }

    fn cmd_tools(&self) -> CommandResult {
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
        CommandResult::Message(format!(
            "Available Tools ({}\n{}",
            self.engine.tools().available_tools().len(),
            tool_list
        ))
    }

    fn cmd_model(&mut self, model_name: String) -> CommandResult {
        if model_name.is_empty() {
            CommandResult::Message(if self.models.is_empty() {
                "Loading models... Select a model from the menu.".to_string()
            } else {
                "Select a model from the menu".to_string()
            })
        } else {
            match find_model(self.models, &model_name) {
                Some((idx, matched_name)) => {
                    if let Some(model) = self.models.get(idx) {
                        self.set_provider(model.provider.clone());
                    }
                    *self.current_model = Some(matched_name.clone());
                    CommandResult::Message(format!("Switched to model: {}", matched_name))
                }
                None => {
                    let suggestions = get_model_suggestions(self.models, &model_name);
                    if suggestions.is_empty() {
                        CommandResult::Message(format!(
                            "Model '{}' not found. Use /model to see available models.",
                            model_name
                        ))
                    } else {
                        CommandResult::Message(format!(
                            "Model '{}' not found. Did you mean: {}?",
                            model_name,
                            suggestions.join(", ")
                        ))
                    }
                }
            }
        }
    }

    fn cmd_clear(&mut self) -> CommandResult {
        self.engine.clear_messages();
        if let Some(ref mut sm) = self.session_manager {
            sm.clear();
        }
        CommandResult::Message("Session cleared".to_string())
    }

    fn cmd_agents(&mut self) -> CommandResult {
        if self.engine.agents_mut().status == AgentModeStatus::Disabled {
            self.engine.agents_mut().status = AgentModeStatus::Idle;
            CommandResult::Message(format!(
                "Agents enabled (Persona: {})",
                self.engine.agents_mut().persona
            ))
        } else {
            CommandResult::ToggleOverlay("agents".to_string())
        }
    }

    fn cmd_avatar(&mut self, name: Option<String>) -> CommandResult {
        let arg = name.as_deref().unwrap_or("");
        if arg == "off" || (arg.is_empty() && *self.avatar_enabled) {
            *self.avatar_enabled = false;
            CommandResult::Message("Avatars disabled".to_string())
        } else if arg == "on" || (arg.is_empty() && !*self.avatar_enabled) {
            *self.avatar_enabled = true;
            CommandResult::Message("Avatars enabled".to_string())
        } else {
            CommandResult::Ok
        }
    }

    fn cmd_yolo(&mut self) -> CommandResult {
        self.engine.agents_mut().yolo_mode = !self.engine.agents_mut().yolo_mode;
        if self.engine.agents_mut().yolo_mode {
            CommandResult::Message(
                "YOLO mode enabled. Destructive tools will execute without confirmation."
                    .to_string(),
            )
        } else {
            CommandResult::Message(
                "YOLO mode disabled. Destructive tools will require confirmation.".to_string(),
            )
        }
    }

    fn cmd_plan(&mut self) -> CommandResult {
        let new_mode = match self.engine.plan_mode() {
            AgentMode::Auto => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Auto,
        };
        self.engine.set_plan_mode(new_mode);
        match new_mode {
            AgentMode::Plan => CommandResult::Message(
                "Plan mode enabled. Only read-only tools available.".to_string(),
            ),
            AgentMode::Auto => {
                CommandResult::Message("Plan mode disabled. All tools available.".to_string())
            }
        }
    }

    fn cmd_compact(&mut self) -> CommandResult {
        let (before, after) = self.engine.compact_context();
        if after < before {
            CommandResult::Message(format!(
                "Context compacted: {} → {} messages",
                before, after
            ))
        } else {
            CommandResult::Message(format!("No compaction needed ({} messages)", before))
        }
    }

    fn cmd_rlm(&mut self) -> CommandResult {
        *self.rlm_mode = !*self.rlm_mode;
        if *self.rlm_mode {
            self.engine.set_system_prompt(
                "You are in RLM mode. Use the `rlm_repl` tool to execute Python code. \
When you have your final answer, output 'FINAL(answer)' on its own line."
                    .to_string(),
            );
            self.engine.set_agent_prompt(String::new());
            if self.engine.agents().status == AgentModeStatus::Disabled {
                self.engine.agents_mut().status = AgentModeStatus::Idle;
            }
            CommandResult::Message(
                "RLM mode enabled. The model will use Python code execution.".to_string(),
            )
        } else {
            self.set_provider(self.provider.clone());
            CommandResult::Message("RLM mode disabled.".to_string())
        }
    }

    fn cmd_edit(&mut self, n: usize) -> CommandResult {
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
            CommandResult::Error(format!(
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
            CommandResult::Message(format!("Editing message {}. Press Enter to resend.", n))
        }
    }

    fn cmd_remove(&mut self, n: usize) -> CommandResult {
        if n == 0 || n > self.engine.chat_mut().messages.len() {
            CommandResult::Error(format!(
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
            CommandResult::Message(format!(
                "Removed message {}: [{}] {}",
                n, removed.role, preview
            ))
        }
    }

    fn cmd_save(&mut self, name: String) -> CommandResult {
        if let Some(ref mut sm) = self.session_manager {
            let id = if name.is_empty() {
                let model = self.current_model.as_deref().unwrap_or("unknown");
                SessionStore::generate_name(model)
            } else {
                name
            };
            match sm.save_with_id(
                &id,
                &self.engine.chat().messages,
                self.current_model.as_deref(),
            ) {
                Ok(_) => CommandResult::Message(format!("Session '{}' saved", id)),
                Err(e) => CommandResult::Error(format!("Failed to save: {}", e)),
            }
        } else {
            CommandResult::Error("Store not available".to_string())
        }
    }

    fn cmd_load(&mut self, name: String) -> CommandResult {
        if name.is_empty() {
            CommandResult::ToggleOverlay("sessions".to_string())
        } else if let Some(ref mut sm) = self.session_manager {
            match sm.load(&name) {
                Ok(session) => {
                    self.engine.chat_mut().messages = session.messages;
                    self.engine.chat_mut().scroll = 0;
                    if let Some(ref model) = session.model {
                        *self.current_model = Some(model.clone());
                        if let Some((idx, _)) = find_model(self.models, model) {
                            if let Some(m) = self.models.get(idx) {
                                self.set_provider(m.provider.clone());
                            }
                        }
                    }
                    CommandResult::Message(format!("Session '{}' loaded", name))
                }
                Err(e) => CommandResult::Error(format!("Failed to load: {}", e)),
            }
        } else {
            CommandResult::Error("Store not available".to_string())
        }
    }

    fn cmd_delete(&mut self, name: String) -> CommandResult {
        if name.is_empty() {
            CommandResult::ToggleOverlay("sessions".to_string())
        } else if let Some(ref mut sm) = self.session_manager {
            match sm.delete(&name) {
                Ok(_) => CommandResult::Message(format!("Session '{}' deleted", name)),
                Err(e) => CommandResult::Error(format!("Failed to delete: {}", e)),
            }
        } else {
            CommandResult::Error("Store not available".to_string())
        }
    }

    fn cmd_export(&mut self, arg: String) -> CommandResult {
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
                .session_manager
                .as_ref()
                .and_then(|sm| sm.context().current_id.as_deref())
                .unwrap_or("untitled");
            let exports_dir = Config::data_dir()
                .map(|d| d.join("exports"))
                .unwrap_or_else(|| std::path::PathBuf::from("exports"));
            let _ = std::fs::create_dir_all(&exports_dir);
            let path = exports_dir.join(format!("{}.{}", session_name, ext));
            (format, path.to_string_lossy().to_string())
        };
        let content = export::export_messages(
            &self.engine.chat().messages,
            self.current_model.as_deref(),
            format,
        );
        match std::fs::write(&path, content) {
            Ok(_) => CommandResult::Message(format!("Exported to '{}'", path)),
            Err(e) => CommandResult::Error(format!("Failed to export: {}", e)),
        }
    }

    fn cmd_search(&mut self, query: String) -> CommandResult {
        if query.is_empty() {
            CommandResult::Message("Search cleared".to_string())
        } else {
            let matches = search::search_messages(&self.engine.chat().messages, &query);
            let count = matches.len();
            if count > 0 {
                CommandResult::Message(format!("Found {} match(es) for '{}'", count, query))
            } else {
                CommandResult::Message(format!("No matches for '{}'", query))
            }
        }
    }

    fn cmd_theme(&self, name: String) -> CommandResult {
        if name.is_empty() {
            CommandResult::ToggleOverlay("themes".to_string())
        } else {
            CommandResult::Message(format!(
                "Theme '{}' set. (Theme switching is handled by the UI)",
                name
            ))
        }
    }

    fn cmd_new(&mut self) -> CommandResult {
        self.engine.clear_messages();
        self.engine.input_mut().buffer.clear();
        self.engine.tool_executor_mut().clear_pending_tool_calls();
        self.engine.tool_executor_mut().clear_tool_results();
        if let Some(ref mut sm) = self.session_manager {
            sm.clear();
        }
        let loaded = crate::session::read_context_files();
        if loaded.is_empty() {
            CommandResult::Message("New session started".to_string())
        } else {
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
            CommandResult::Message(format!(
                "New session started. Loaded: {}",
                loaded
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }

    fn cmd_undo(&mut self) -> CommandResult {
        if self.engine.chat_mut().messages.len() >= 2 {
            self.engine.chat_mut().messages.pop();
            self.engine.chat_mut().messages.pop();
            CommandResult::Message("Undid last turn".to_string())
        } else {
            CommandResult::Message("Nothing to undo".to_string())
        }
    }

    fn cmd_title(&self, name: String) -> CommandResult {
        if name.is_empty() {
            CommandResult::Message("Usage: /title <name>".to_string())
        } else {
            CommandResult::Message(format!(
                "Session title set to: '{}' (Note: titles are not yet persisted)",
                name
            ))
        }
    }

    fn cmd_branch(&mut self, msg_idx: Option<usize>) -> CommandResult {
        if let Some(ref mut sm) = self.session_manager {
            let messages = self.engine.chat().messages.clone();
            match sm.branch(&messages, msg_idx, self.current_model.as_deref()) {
                Ok(branch_id) => {
                    let branch_point = sm.context().branch_point.unwrap_or(0);
                    let parent_id = sm.context().parent_id.clone().unwrap_or_default();
                    self.engine.chat_mut().messages = messages[..=branch_point].to_vec();
                    self.engine.chat_mut().scroll = 0;
                    CommandResult::Message(format!(
                        "Created branch '{}' from message {}. Parent: '{}'",
                        branch_id, branch_point, parent_id
                    ))
                }
                Err(e) => CommandResult::Error(format!("Failed to create branch: {}", e)),
            }
        } else {
            CommandResult::Error("Store not available".to_string())
        }
    }

    fn cmd_voice(&mut self) -> CommandResult {
        let was_enabled = self.voice.is_enabled();
        if !was_enabled && self.voice.api_key().is_none() {
            CommandResult::Error(
                "Voice mode requires an ElevenLabs API key. Set ELEVENLABS_API_KEY or add elevenlabs_api_key to config.toml".to_string(),
            )
        } else {
            let enabled = self.voice.toggle();
            if enabled {
                CommandResult::Message(
                    "Voice mode enabled. Hold Ctrl+Space to record, release to send.".to_string(),
                )
            } else {
                CommandResult::Message("Voice mode disabled.".to_string())
            }
        }
    }

    fn cmd_voice_device(&mut self, device: String) -> CommandResult {
        if device.is_empty() {
            match crate::voice::list_input_devices() {
                Ok(devices) => {
                    if devices.is_empty() {
                        CommandResult::Message("No input devices found.".to_string())
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
                        CommandResult::Message(msg)
                    }
                }
                Err(e) => CommandResult::Error(format!("Failed to list devices: {}", e)),
            }
        } else {
            self.voice.set_input_device(Some(device.clone()));
            self.config.input_device = Some(device.clone());
            let msg = format!("Voice input device set to: {}", device);
            if let Err(e) = self.config.save() {
                return CommandResult::Message(format!("{} (config save failed: {})", msg, e));
            }
            CommandResult::Message(msg)
        }
    }

    fn cmd_set_key(&mut self, provider: String, key: String) -> CommandResult {
        if provider.is_empty() || key.is_empty() {
            CommandResult::Message("Usage: /set-key <provider> <key>".to_string())
        } else {
            match crate::credentials::CredentialManager::set_api_key(&provider, &key) {
                Ok(()) => {
                    if let Some(p) = self
                        .config
                        .providers
                        .iter_mut()
                        .find(|p| p.name == provider)
                    {
                        p.api_key = crate::config::SecretString::new(key);
                    }
                    CommandResult::Message(format!(
                        "API key stored in keyring for provider '{}'",
                        provider
                    ))
                }
                Err(e) => CommandResult::Error(format!("Failed to store API key: {}", e)),
            }
        }
    }
}
