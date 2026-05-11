use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11436/v1";
const DEFAULT_API_KEY: &str = "sk-local";
const DEFAULT_MAX_TOKENS: u32 = 2048;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: SecretString,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_supports_tools")]
    pub supports_tools: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_params: Option<serde_json::Map<String, serde_json::Value>>,
}

fn default_supports_tools() -> bool {
    true
}

impl Provider {
    pub fn local_default() -> Self {
        Self {
            name: "local".to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: SecretString::new(DEFAULT_API_KEY.to_string()),
            enabled: true,
            supports_tools: true,
            extra_params: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Config {
    #[serde(default)]
    pub providers: Vec<Provider>,
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
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
            providers: vec![Provider::local_default()],
            model: None,
            max_tokens: default_max_tokens(),
            elevenlabs_api_key: None,
            elevenlabs_voice_id: None,
            models_dir: default_models_dir(),
            auto_save: default_auto_save(),
            filter_require_voice_and_tools: default_filter_voice_tools(),
            input_device: None,
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

impl Config {
    pub fn load() -> Result<Self> {
        let mut config = Self::default();

        if let Some(config_path) = Self::config_path() {
            if config_path.exists() {
                info!("Loading config from {:?}", config_path);
                let contents = std::fs::read_to_string(&config_path)
                    .with_context(|| format!("Failed to read config from {:?}", config_path))?;
                let file_config: Config = toml::from_str(&contents)
                    .with_context(|| format!("Failed to parse config from {:?}", config_path))?;
                config = file_config;
            } else {
                info!("Config file not found at {:?}, using defaults", config_path);
            }
        } else {
            warn!("Could not determine config path");
        }

        if config.providers.is_empty() {
            info!("No providers configured, adding local default");
            config.providers.push(Provider::local_default());
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
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }
        let contents =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize config to TOML")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write config to {:?}", path))?;
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
        let p = Provider::local_default();
        assert_eq!(p.name, "local");
        assert_eq!(p.base_url, DEFAULT_BASE_URL);
        assert_eq!(p.api_key.expose_secret(), DEFAULT_API_KEY);
        assert!(p.enabled);
        assert!(p.supports_tools);
        assert!(p.extra_params.is_none());
    }

    #[test]
    fn provider_serialization_redacts_api_key() {
        let p = Provider {
            name: "test".to_string(),
            base_url: "http://test:8080/v1".to_string(),
            api_key: SecretString::new("sk-test".to_string()),
            enabled: false,
            supports_tools: false,
            extra_params: None,
        };
        let toml_str = toml::to_string(&p).unwrap();
        assert!(
            toml_str.contains("[REDACTED]"),
            "api_key should be redacted in serialization: {}",
            toml_str
        );
        let deserialized: Provider = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.name, p.name);
        assert_eq!(deserialized.base_url, p.base_url);
        assert!(!deserialized.enabled);
        assert!(!deserialized.supports_tools);
    }

    #[test]
    fn provider_supports_tools_defaults_to_true() {
        let toml_str = r#"
            name = "test"
            base_url = "http://test:8080/v1"
        "#;
        let p: Provider = toml::from_str(toml_str).unwrap();
        assert!(p.supports_tools);
    }

    #[test]
    fn provider_with_extra_params_serializes() {
        let mut extra = serde_json::Map::new();
        extra.insert("temperature".to_string(), serde_json::json!(0.7));
        let p = Provider {
            name: "test".to_string(),
            base_url: "http://test:8080/v1".to_string(),
            api_key: SecretString::new(DEFAULT_API_KEY.to_string()),
            enabled: true,
            supports_tools: true,
            extra_params: Some(extra),
        };
        let json = serde_json::to_value(&p).unwrap();
        assert!(json.get("extra_params").is_some());
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = Config {
            providers: vec![Provider::local_default()],
            model: Some("test-model".to_string()),
            max_tokens: 4096,
            elevenlabs_api_key: None,
            elevenlabs_voice_id: None,
            models_dir: default_models_dir(),
            auto_save: default_auto_save(),
            filter_require_voice_and_tools: default_filter_voice_tools(),
            input_device: None,
        };
        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.model, Some("test-model".to_string()));
        assert_eq!(deserialized.max_tokens, 4096);
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
    fn config_path_returns_some() {
        assert!(Config::config_path().is_some());
    }

    #[test]
    fn config_data_dir_returns_some() {
        assert!(Config::data_dir().is_some());
    }
}
