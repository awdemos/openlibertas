use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11435/v1";
const LLAMACPP_BASE_URL: &str = "http://localhost:8080/v1";
const DEFAULT_API_KEY: &str = "sk-local";
const DEFAULT_MAX_TOKENS: u32 = 2048;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_supports_tools")]
    pub supports_tools: bool,
}

fn default_supports_tools() -> bool {
    true
}

impl Provider {
    pub fn local_default() -> Self {
        Self {
            name: "local".to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: DEFAULT_API_KEY.to_string(),
            enabled: true,
            supports_tools: true,
        }
    }

    pub fn llamacpp_default() -> Self {
        Self {
            name: "llamacpp".to_string(),
            base_url: LLAMACPP_BASE_URL.to_string(),
            api_key: DEFAULT_API_KEY.to_string(),
            enabled: true,
            supports_tools: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: Vec<Provider>,
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            providers: vec![Provider::local_default()],
            model: None,
            max_tokens: default_max_tokens(),
        }
    }
}

fn default_max_tokens() -> u32 {
    DEFAULT_MAX_TOKENS
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut config = Self::default();

        if let Some(config_path) = Self::config_path() {
            if config_path.exists() {
                let contents = std::fs::read_to_string(&config_path)
                    .with_context(|| format!("Failed to read config from {:?}", config_path))?;
                let file_config: Config = toml::from_str(&contents)
                    .with_context(|| format!("Failed to parse config from {:?}", config_path))?;
                config = file_config;
            }
        }

        if config.providers.is_empty() {
            config.providers.push(Provider::local_default());
        }

        if let Ok(url) = std::env::var("OPENLIBERTAS_URL") {
            if let Some(first) = config.providers.first_mut() {
                first.base_url = url;
            }
        }
        if let Ok(key) = std::env::var("OPENLIBERTAS_API_KEY") {
            if let Some(first) = config.providers.first_mut() {
                first.api_key = key;
            }
        }
        if let Ok(model) = std::env::var("OPENLIBERTAS_MODEL") {
            config.model = Some(model);
        }
        if let Ok(tokens) = std::env::var("OPENLIBERTAS_MAX_TOKENS") {
            if let Ok(tokens) = tokens.parse() {
                config.max_tokens = tokens;
            }
        }

        Ok(config)
    }

    pub fn config_path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    #[allow(dead_code)]
    pub fn data_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "openlibertas", "openlibertas")
            .map(|dirs| dirs.data_dir().to_path_buf())
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
        assert_eq!(p.api_key, DEFAULT_API_KEY);
        assert!(p.enabled);
        assert!(p.supports_tools);
    }

    #[test]
    fn provider_llamacpp_default() {
        let p = Provider::llamacpp_default();
        assert_eq!(p.name, "llamacpp");
        assert_eq!(p.base_url, LLAMACPP_BASE_URL);
        assert_eq!(p.api_key, DEFAULT_API_KEY);
        assert!(p.enabled);
        assert!(p.supports_tools);
    }
}
