//! Integration tests for configuration loading, env-var substitution, and serialization.

use openlibertas_core::config::{resolve_env_ref, Config, ProviderConfig, SecretString};
use openlibertas_core::capability::ProviderKind;
use openlibertas_core::tool_format::ToolFormat;
use std::io::Write;

#[test]
fn config_loads_from_toml_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let mut file = std::fs::File::create(&config_path).unwrap();
    write!(
        file,
        r#"
model = "gpt-test"
max_tokens = 4096

[[providers]]
name = "test-provider"
base_url = "http://localhost:9999/v1"
api_key = "sk-test"
enabled = true
"#
    )
    .unwrap();

    // Temporarily override the config path by setting the project dirs env.
    // Since Config::load uses directories::ProjectDirs, we can't easily override it.
    // Instead, test the deserialization directly.
    let toml_str = std::fs::read_to_string(&config_path).unwrap();
    let config: Config = toml::from_str(&toml_str).unwrap();

    assert_eq!(config.providers.len(), 1);
    assert_eq!(config.providers[0].name, "test-provider");
    assert_eq!(config.providers[0].base_url, "http://localhost:9999/v1");
    assert_eq!(config.model, Some("gpt-test".to_string()));
    assert_eq!(config.max_tokens, 4096);
}

#[test]
fn config_env_var_substitution_in_api_key() {
    std::env::set_var("OPENLIBERTAS_INTEGRATION_KEY", "sk-from-env");
    let resolved = resolve_env_ref("{env:OPENLIBERTAS_INTEGRATION_KEY}");
    assert_eq!(resolved, "sk-from-env");
    std::env::remove_var("OPENLIBERTAS_INTEGRATION_KEY");
}

#[test]
fn config_env_var_substitution_missing_returns_empty() {
    std::env::remove_var("OPENLIBERTAS_DEFINITELY_MISSING");
    let resolved = resolve_env_ref("{env:OPENLIBERTAS_DEFINITELY_MISSING}");
    assert_eq!(resolved, "");
}

#[test]
fn config_serialization_roundtrip_preserve_fields() {
    let toml_str = r#"
model = "test-model"
max_tokens = 8192
context_window = 32768
elevenlabs_api_key = "sk-eleven"
elevenlabs_voice_id = "voice-123"
models_dir = "/tmp/models"
auto_save = false
filter_require_voice_and_tools = true
input_device = "Mic"

[[providers]]
name = "local"
base_url = "http://127.0.0.1:11436/v1"
api_key = "sk-local"
"#;

    let original: Config = toml::from_str(toml_str).unwrap();
    let serialized = toml::to_string(&original).unwrap();
    let deserialized: Config = toml::from_str(&serialized).unwrap();

    assert_eq!(deserialized.model, Some("test-model".to_string()));
    assert_eq!(deserialized.max_tokens, 8192);
    assert_eq!(deserialized.context_window, Some(32768));
    assert_eq!(deserialized.auto_save, false);
    assert_eq!(deserialized.filter_require_voice_and_tools, true);
    assert_eq!(deserialized.input_device, Some("Mic".to_string()));
    assert_eq!(deserialized.models_dir, std::path::PathBuf::from("/tmp/models"));
}

#[test]
fn provider_config_deserializes_all_fields() {
    let toml_str = r#"
name = "custom"
base_url = "http://custom:8080/v1"
api_key = "sk-custom"
enabled = false
kind = "anthropic"
supports_tools = true
extra_params = { temperature = 0.5 }
"#;
    let provider: ProviderConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(provider.name, "custom");
    assert_eq!(provider.base_url, "http://custom:8080/v1");
    assert_eq!(provider.api_key.expose_secret(), "sk-custom");
    assert!(!provider.enabled);
    assert_eq!(provider.kind, ProviderKind::Anthropic);
    assert!(provider.capabilities.tools);
    assert_eq!(provider.tool_format, ToolFormat::Native);
    assert!(provider.extra_params.is_some());
}

#[test]
fn config_default_values_are_sensible() {
    let config = Config::default();
    assert!(!config.providers.is_empty());
    assert_eq!(config.max_tokens, 2048);
    assert!(config.models_dir.as_os_str().len() > 0);
    assert!(!config.filter_require_voice_and_tools);
}

#[test]
fn config_effective_context_window_with_explicit_value() {
    let toml_str = r#"
max_tokens = 2048
context_window = 16000
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.effective_context_window(), 16000);
}

#[test]
fn config_effective_context_window_fallback_calculation() {
    let toml_str = r#"
max_tokens = 1024
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.effective_context_window(), 4096);
}
