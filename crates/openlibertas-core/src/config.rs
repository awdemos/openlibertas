use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::capability::{ProviderCapabilities, ProviderKind};
use crate::credentials::CredentialManager;
use crate::tool_format::ToolFormat;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11436/v1";
const DEFAULT_API_KEY: &str = "sk-local";
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// Resolve an environment variable reference in the form `{env:VAR_NAME}`.
/// If the value matches the `{env:...}` syntax, looks up the env var and returns
/// its value if set. If the env var is not set, logs a warning and returns an
/// empty string. If the value does not match the syntax, returns it unchanged.
pub fn resolve_env_ref(value: &str) -> String {
    if let Some(inner) = value
        .strip_prefix("{env:")
        .and_then(|s| s.strip_suffix('}'))
    {
        let var_name = inner.trim();
        if var_name.is_empty() {
            warn!("Empty environment variable name in '{}'", value);
            return String::new();
        }
        match std::env::var(var_name) {
            Ok(v) => v,
            Err(_) => {
                warn!(
                    "Environment variable '{}' not set for api_key reference '{}'",
                    var_name, value
                );
                String::new()
            }
        }
    } else {
        value.to_string()
    }
}

/// A string that redacts its contents in Debug, Display, and serde::Serialize.
/// Use `expose_secret()` to access the plaintext value for authentication.
#[derive(Clone, Deserialize, PartialEq, Default)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(s)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl Serialize for SecretString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: SecretString,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub kind: ProviderKind,
    #[serde(default)]
    pub capabilities: ProviderCapabilities,
    pub tool_format: ToolFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_params: Option<serde_json::Map<String, serde_json::Value>>,
}

impl<'de> Deserialize<'de> for ProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ProviderHelper {
            name: String,
            base_url: String,
            #[serde(default)]
            api_key: SecretString,
            #[serde(default)]
            enabled: bool,
            #[serde(default)]
            kind: Option<ProviderKind>,
            #[serde(default)]
            capabilities: Option<ProviderCapabilities>,
            #[serde(default)]
            tool_format: Option<ToolFormat>,
            #[serde(default)]
            supports_tools: Option<bool>,
            #[serde(default)]
            extra_params: Option<serde_json::Map<String, serde_json::Value>>,
        }

        let helper = ProviderHelper::deserialize(deserializer)?;

        let kind = helper.kind.unwrap_or_default();
        let mut capabilities = helper
            .capabilities
            .unwrap_or_else(|| kind.default_capabilities());

        let tool_format = if let Some(tf) = helper.tool_format {
            tf
        } else if let Some(st) = helper.supports_tools {
            if st {
                ToolFormat::Native
            } else {
                ToolFormat::None
            }
        } else {
            capabilities.tool_format
        };

        // When tool_format is explicitly set, it drives capabilities.tools.
        // Otherwise fall back to supports_tools for backward compat.
        if helper.tool_format.is_some() {
            capabilities.tools = tool_format != ToolFormat::None;
        } else if let Some(st) = helper.supports_tools {
            capabilities.tools = st;
        }
        capabilities.tool_format = tool_format;

        Ok(ProviderConfig {
            name: helper.name,
            base_url: helper.base_url,
            api_key: helper.api_key,
            enabled: helper.enabled,
            kind,
            capabilities,
            tool_format,
            extra_params: helper.extra_params,
        })
    }
}

impl ProviderConfig {
    pub fn local_default() -> Self {
        Self {
            name: "local".to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: SecretString::new(DEFAULT_API_KEY.to_string()),
            enabled: true,
            kind: ProviderKind::Ollama,
            capabilities: ProviderKind::Ollama.default_capabilities(),
            tool_format: ToolFormat::Native,
            extra_params: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Config {
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub elevenlabs_api_key: Option<SecretString>,
    #[serde(default)]
    pub elevenlabs_voice_id: Option<String>,
    #[serde(default = "default_models_dir")]
    pub models_dir: PathBuf,
    #[serde(default = "default_auto_save")]
    pub auto_save: bool,
    #[serde(default = "default_filter_voice_tools")]
    pub filter_require_voice_and_tools: bool,
    #[serde(default)]
    pub input_device: Option<String>,
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,
    #[serde(default = "default_permission_policy")]
    pub permission_policy: PermissionPolicy,
}

fn default_models_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("models")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: vec![ProviderConfig::local_default()],
            model: None,
            max_tokens: default_max_tokens(),
            context_window: None,
            elevenlabs_api_key: None,
            elevenlabs_voice_id: None,
            models_dir: default_models_dir(),
            auto_save: default_auto_save(),
            filter_require_voice_and_tools: default_filter_voice_tools(),
            input_device: None,
            auto_approve_tools: Vec::new(),
            permission_policy: default_permission_policy(),
        }
    }
}

fn default_max_tokens() -> u32 {
    DEFAULT_MAX_TOKENS
}

fn default_auto_save() -> bool {
    true
}

fn default_filter_voice_tools() -> bool {
    false
}

fn default_permission_policy() -> PermissionPolicy {
    PermissionPolicy::Ask
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicy {
    #[default]
    Ask,
    AutoApprove,
    Deny,
}

/// Persistent permission state, stored separately from config.
/// This includes session-level grants that survive app restarts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionState {
    /// Tools that have been granted permanent auto-approval by the user.
    #[serde(default)]
    pub auto_approve_tools: Vec<String>,
    /// Map of session_id to list of granted tool names.
    #[serde(default)]
    pub granted_sessions: std::collections::HashMap<String, Vec<String>>,
}

impl PermissionState {
    pub fn load() -> Self {
        if let Some(path) = Self::state_path() {
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(contents) => match toml::from_str(&contents) {
                        Ok(state) => return state,
                        Err(e) => warn!("Failed to parse permission state: {}", e),
                    },
                    Err(e) => warn!("Failed to read permission state: {}", e),
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::state_path().context("Could not determine permission state path")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {parent:?}"))?;
        }
        let contents =
            toml::to_string_pretty(self).context("Failed to serialize permission state")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write permission state to {path:?}"))?;
        Ok(())
    }

    pub fn state_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
            .map(|p| p.data_dir().join("permissions.toml"))
    }

    pub fn grant_session(&mut self, session_id: &str, tool_name: &str) {
        self.granted_sessions
            .entry(session_id.to_string())
            .or_default()
            .push(tool_name.to_string());
    }

    pub fn is_session_granted(&self, session_id: &str, tool_name: &str) -> bool {
        self.granted_sessions
            .get(session_id)
            .map(|tools| tools.iter().any(|t| t.eq_ignore_ascii_case(tool_name)))
            .unwrap_or(false)
    }

    pub fn add_auto_approve_tool(&mut self, tool_name: &str) {
        if !self
            .auto_approve_tools
            .iter()
            .any(|t| t.eq_ignore_ascii_case(tool_name))
        {
            self.auto_approve_tools.push(tool_name.to_string());
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut config = Self::default();

        if let Some(config_path) = Self::config_path() {
            if config_path.exists() {
                info!("Loading config from {:?}", config_path);
                let contents = std::fs::read_to_string(&config_path)
                    .with_context(|| format!("Failed to read config from {config_path:?}"))?;
                let file_config: Config = toml::from_str(&contents)
                    .with_context(|| format!("Failed to parse config from {config_path:?}"))?;
                config = file_config;
            } else {
                info!("Config file not found at {:?}, using defaults", config_path);
            }
        } else {
            warn!("Could not determine config path");
        }

        if config.providers.is_empty() {
            info!("No providers configured, adding local default");
            config.providers.push(ProviderConfig::local_default());
        }

        for provider in &mut config.providers {
            let resolved =
                CredentialManager::get_api_key(provider.api_key.expose_secret(), &provider.name);
            provider.api_key = SecretString::new(resolved);
        }

        if let Some(ref key) = config.elevenlabs_api_key {
            let resolved = resolve_env_ref(key.expose_secret());
            config.elevenlabs_api_key = Some(SecretString::new(resolved));
        }

        if let Ok(url) = std::env::var("OPENLIBERTAS_URL") {
            info!("Overriding base URL from environment");
            if let Some(first) = config.providers.first_mut() {
                first.base_url = url;
            }
        }
        if let Ok(key) = std::env::var("OPENLIBERTAS_API_KEY") {
            info!("Overriding API key from environment");
            if let Some(first) = config.providers.first_mut() {
                first.api_key = SecretString::new(key);
            }
        }
        if let Ok(model) = std::env::var("OPENLIBERTAS_MODEL") {
            info!("Overriding model from environment: {}", model);
            config.model = Some(model);
        }
        if let Ok(tokens) = std::env::var("OPENLIBERTAS_MAX_TOKENS") {
            if let Ok(tokens) = tokens.parse() {
                info!("Overriding max tokens from environment: {}", tokens);
                config.max_tokens = tokens;
            } else {
                warn!("Invalid OPENLIBERTAS_MAX_TOKENS value, using default");
            }
        }
        if let Ok(window) = std::env::var("OPENLIBERTAS_CONTEXT_WINDOW") {
            if let Ok(window) = window.parse() {
                info!("Overriding context window from environment: {}", window);
                config.context_window = Some(window);
            } else {
                warn!("Invalid OPENLIBERTAS_CONTEXT_WINDOW value, ignoring");
            }
        }
        if let Ok(key) = std::env::var("ELEVENLABS_API_KEY") {
            info!("Loading ElevenLabs API key from environment");
            config.elevenlabs_api_key = Some(SecretString::new(key));
        }
        if let Ok(voice_id) = std::env::var("ELEVENLABS_VOICE_ID") {
            info!("Loading ElevenLabs voice ID from environment: {}", voice_id);
            config.elevenlabs_voice_id = Some(voice_id);
        }

        info!("Config loaded: {} providers", config.providers.len());
        Ok(config)
    }

    /// Return the effective context window size in tokens.
    /// If `context_window` is explicitly configured, use it;
    /// otherwise fall back to `max_tokens * 4` as a rough heuristic.
    pub fn effective_context_window(&self) -> u32 {
        self.context_window
            .unwrap_or(self.max_tokens.saturating_mul(4))
    }

    pub fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    pub fn data_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
            .map(|dirs| dirs.data_dir().to_path_buf())
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config path"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {parent:?}"))?;
        }
        let contents =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize config to TOML")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write config to {path:?}"))?;
        info!("Config saved to {:?}", path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default_has_local_provider() {
        let config = Config::default();
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].name, "local");
        assert_eq!(config.providers[0].base_url, DEFAULT_BASE_URL);
        assert!(config.providers[0].enabled);
    }

    #[test]
    fn config_default_max_tokens() {
        let config = Config::default();
        assert_eq!(config.max_tokens, DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn provider_local_default() {
        let p = ProviderConfig::local_default();
        assert_eq!(p.name, "local");
        assert_eq!(p.base_url, DEFAULT_BASE_URL);
        assert_eq!(p.api_key.expose_secret(), DEFAULT_API_KEY);
        assert!(p.enabled);
        assert_eq!(p.kind, ProviderKind::Ollama);
        assert!(p.capabilities.tools);
        assert!(p.capabilities.streaming);
        assert!(!p.capabilities.json_mode);
        assert_eq!(p.tool_format, ToolFormat::Native);
        assert!(p.extra_params.is_none());
    }

    #[test]
    fn provider_serialization_redacts_api_key() {
        let p = ProviderConfig {
            name: "test".to_string(),
            base_url: "http://test:8080/v1".to_string(),
            api_key: SecretString::new("sk-test".to_string()),
            enabled: false,
            kind: ProviderKind::OpenAiCompatible,
            capabilities: ProviderKind::OpenAiCompatible.default_capabilities(),
            tool_format: ToolFormat::None,
            extra_params: None,
        };
        let toml_str = toml::to_string(&p).unwrap();
        assert!(
            toml_str.contains("[REDACTED]"),
            "api_key should be redacted in serialization: {}",
            toml_str
        );
        let deserialized: ProviderConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.name, p.name);
        assert_eq!(deserialized.base_url, p.base_url);
        assert!(!deserialized.enabled);
        assert_eq!(deserialized.tool_format, ToolFormat::None);
    }

    #[test]
    fn provider_tool_format_defaults_to_native() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
        "#;
        let p: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(p.tool_format, ToolFormat::Native);
    }

    #[test]
    fn provider_supports_tools_backward_compat_false() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
            supports_tools = false
        "#;
        let p: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(p.tool_format, ToolFormat::None);
        assert!(!p.capabilities.tools);
    }

    #[test]
    fn provider_supports_tools_backward_compat_true() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
            supports_tools = true
        "#;
        let p: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(p.tool_format, ToolFormat::Native);
        assert!(p.capabilities.tools);
    }

    #[test]
    fn provider_tool_format_explicit_overrides_supports_tools() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
            tool_format = "ContentJson"
            supports_tools = false
        "#;
        let p: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(p.tool_format, ToolFormat::ContentJson);
        // Explicit tool_format drives capabilities.tools; ContentJson implies tools enabled.
        assert!(p.capabilities.tools);
    }

    #[test]
    fn provider_with_extra_params_serializes() {
        let mut extra = serde_json::Map::new();
        extra.insert("temperature".to_string(), serde_json::json!(0.7));
        let p = ProviderConfig {
            name: "test".to_string(),
            base_url: "http://test:8080/v1".to_string(),
            api_key: SecretString::new(DEFAULT_API_KEY.to_string()),
            enabled: true,
            kind: ProviderKind::OpenAiCompatible,
            capabilities: ProviderKind::OpenAiCompatible.default_capabilities(),
            tool_format: ToolFormat::Native,
            extra_params: Some(extra),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("extra_params").is_some());
    }

    #[test]
    fn provider_kind_deserializes_from_snake_case() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
            kind = "anthropic"
        "#;
        let p: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(p.kind, ProviderKind::Anthropic);
        assert!(p.capabilities.tools);
        assert!(p.capabilities.streaming);
        assert!(p.capabilities.reasoning);
        assert!(!p.capabilities.json_mode);
    }

    #[test]
    fn provider_kind_defaults_to_openai_compatible() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
        "#;
        let p: ProviderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(p.kind, ProviderKind::OpenAiCompatible);
        assert!(p.capabilities.tools);
        assert!(p.capabilities.json_mode);
        assert!(p.capabilities.streaming);
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = Config {
            providers: vec![ProviderConfig::local_default()],
            model: Some("test-model".to_string()),
            max_tokens: 4096,
            context_window: Some(8192),
            elevenlabs_api_key: None,
            elevenlabs_voice_id: None,
            models_dir: default_models_dir(),
            auto_save: default_auto_save(),
            filter_require_voice_and_tools: default_filter_voice_tools(),
            input_device: None,
            auto_approve_tools: Vec::new(),
            permission_policy: default_permission_policy(),
        };
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.model, Some("test-model".to_string()));
        assert_eq!(deserialized.max_tokens, 4096);
        assert_eq!(deserialized.context_window, Some(8192));
        assert_eq!(deserialized.providers.len(), 1);
    }

    #[test]
    fn config_empty_providers_gets_default() {
        let toml_str = r#"
            model = "test-model"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.providers.is_empty());
    }

    #[test]
    fn config_effective_context_window_explicit() {
        let config = Config {
            context_window: Some(16000),
            max_tokens: 2048,
            ..Config::default()
        };
        assert_eq!(config.effective_context_window(), 16000);
    }

    #[test]
    fn config_effective_context_window_fallback() {
        let config = Config {
            context_window: None,
            max_tokens: 2048,
            ..Config::default()
        };
        assert_eq!(config.effective_context_window(), 8192);
    }

    #[test]
    fn config_path_returns_some() {
        assert!(Config::config_path().is_some());
    }

    #[test]
    fn config_data_dir_returns_some() {
        assert!(Config::data_dir().is_some());
    }

    #[test]
    fn resolve_env_ref_substitutes_env_var() {
        std::env::set_var("OPENLIBERTAS_TEST_KEY", "sk-test-value");
        let result = resolve_env_ref("{env:OPENLIBERTAS_TEST_KEY}");
        assert_eq!(result, "sk-test-value");
        std::env::remove_var("OPENLIBERTAS_TEST_KEY");
    }

    #[test]
    fn resolve_env_ref_returns_literal_when_no_match() {
        let result = resolve_env_ref("sk-plain-key");
        assert_eq!(result, "sk-plain-key");
    }

    #[test]
    fn resolve_env_ref_returns_empty_when_var_missing() {
        std::env::remove_var("OPENLIBERTAS_TEST_MISSING");
        let result = resolve_env_ref("{env:OPENLIBERTAS_TEST_MISSING}");
        assert_eq!(result, "");
    }

    #[test]
    fn resolve_env_ref_returns_empty_on_empty_var_name() {
        let result = resolve_env_ref("{env:}");
        assert_eq!(result, "");
    }

    #[test]
    fn resolve_env_ref_handles_value_with_braces() {
        std::env::set_var("OPENLIBERTAS_BRACES", "val}ue");
        let result = resolve_env_ref("{env:OPENLIBERTAS_BRACES}");
        assert_eq!(result, "val}ue");
        std::env::remove_var("OPENLIBERTAS_BRACES");
    }
}
