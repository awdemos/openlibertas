use std::sync::Arc;

use crate::backend::registry::ProviderRegistry;
use crate::commands::executor::{CommandExecutor, CommandResult};
use crate::commands::SlashCommand;
use crate::config::Config;
use crate::domain::{BackendError, Message, Model, ProviderId};
use crate::engine::{AgentMode, ChatEngine};
use crate::prompt::PromptManager;
use crate::session::SessionManager;
use crate::soul::{UserInput, WireSender};
use crate::voice::VoiceManager;

pub use crate::commands::executor::CommandResult;

/// Model selection state.
#[derive(Debug)]
pub struct ModelState {
    pub models: Vec<Model>,
    pub selected: usize,
    pub current: Option<String>,
    pub provider: ProviderId,
    pub search: String,
}

/// Curated facade exposing the core public API to the TUI.
///
/// Holds all business-logic state and provides a single entrypoint
/// for slash-command execution and agent-turn spawning.
pub struct AppFacade {
    pub engine: ChatEngine,
    pub registry: Arc<ProviderRegistry>,
    pub session_manager: Option<SessionManager>,
    pub config: Config,
    pub models: ModelState,
    pub voice: VoiceManager,
    pub temperature: Option<f32>,
    pub rlm_mode: bool,
    pub mouse_enabled: bool,
    pub avatar_enabled: bool,
    pub prompt_manager: PromptManager,
}

impl AppFacade {
    pub fn new(engine: ChatEngine, registry: Arc<ProviderRegistry>, config: Config) -> Self {
        let voice_api_key = config.elevenlabs_api_key.clone();
        let voice_id = config.elevenlabs_voice_id.clone();
        let voice_input_device = config.input_device.clone();
        let mut voice = VoiceManager::new(voice_api_key, voice_id);
        voice.set_input_device(voice_input_device);

        let local_models = crate::model_scanner::scan_local_models(&config.models_dir);
        let filtered_local: Vec<Model> = if config.filter_require_voice_and_tools {
            local_models
                .into_iter()
                .filter(|m| m.supports_tools && m.supports_voice)
                .collect()
        } else {
            local_models
        };

        Self {
            engine,
            registry,
            session_manager: None,
            config,
            models: ModelState {
                models: filtered_local,
                selected: 0,
                current: None,
                provider: ProviderId::new(""),
                search: String::new(),
            },
            voice,
            temperature: None,
            rlm_mode: false,
            mouse_enabled: false,
            avatar_enabled: false,
            prompt_manager: PromptManager::new(),
        }
    }

    pub fn with_session_manager(mut self, sm: SessionManager) -> Self {
        self.session_manager = Some(sm);
        self
    }

    pub fn execute_slash_command(&mut self, cmd: SlashCommand) -> CommandResult {
        let models_slice: Vec<Model> = self.models.models.clone();
        let mut executor = CommandExecutor {
            engine: &mut self.engine,
            session_manager: &mut self.session_manager,
            models: &models_slice,
            current_model: &mut self.models.current,
            provider: &mut self.models.provider,
            config: &mut self.config,
            voice: &mut self.voice,
            temperature: &mut self.temperature,
            rlm_mode: &mut self.rlm_mode,
            mouse_enabled: &mut self.mouse_enabled,
            avatar_enabled: &mut self.avatar_enabled,
            prompt_manager: &self.prompt_manager,
        };
        executor.execute(cmd)
    }

    pub fn spawn_agent_turn(&self, input: UserInput, wire: WireSender) {
        let mode = if self.engine.agents().status
            == crate::engine::AgentModeStatus::Active
        {
            self.engine.plan_mode()
        } else {
            AgentMode::Auto
        };
        crate::agent_turn::spawn_agent_turn(
            &self.engine,
            &self.registry,
            self.models.provider.clone(),
            self.models.current.clone(),
            self.config.max_tokens,
            input.text,
            mode,
            wire,
        );
    }

    pub async fn fetch_models(&self) -> Result<Vec<Model>, BackendError> {
        match self.registry.default_provider() {
            Some(provider) => provider
                .fetch_models()
                .await
                .map_err(|e| BackendError::Unknown(e.to_string())),
            None => Ok(vec![]),
        }
    }

    pub async fn health_check(&self) -> Result<(), BackendError> {
        match self.registry.default_provider() {
            Some(provider) => provider
                .health_check()
                .await
                .map_err(|e| BackendError::Unknown(e.to_string())),
            None => Ok(()),
        }
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

    pub fn autosave(&mut self) -> Option<String> {
        if !self.config.auto_save {
            return None;
        }
        if let Some(ref mut sm) = self.session_manager {
            let model = self.models.current.as_deref().unwrap_or("unknown");
            let id = format!(
                "autosave-{}",
                model
                    .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
                    .replace("--", "-")
            );
            match sm.save_with_id(
                &id,
                &self.engine.chat().messages,
                self.models.current.as_deref(),
            ) {
                Ok(_) => None,
                Err(e) => Some(format!("Auto-save failed: {}", e)),
            }
        } else {
            Some("Auto-save failed: no session store".to_string())
        }
    }

    pub fn export_session(&self, format: crate::export::ExportFormat) -> String {
        crate::export::export_messages(
            &self.engine.chat().messages,
            self.models.current.as_deref(),
            format,
        )
    }

    pub fn load_context_files(&mut self) -> Vec<String> {
        let loaded = crate::session::read_context_files();
        for (filename, content) in &loaded {
            self.engine.chat_mut().messages.push(Message {
                role: crate::domain::Role::System,
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
}
