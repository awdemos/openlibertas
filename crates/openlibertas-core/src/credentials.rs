use anyhow::Result;
use tracing::{info, warn};

const KEYRING_SERVICE: &str = "com.openlibertas";

/// Manages API key resolution from multiple sources with a defined priority:
/// 1. Environment variable (`{env:VAR}` syntax)
/// 2. OS keyring (stored per provider name)
/// 3. Plaintext in config.toml (backward compatible fallback)
pub struct CredentialManager;

impl CredentialManager {
    pub fn get_api_key(config_value: &str, provider_name: &str) -> String {
        if let Some(env_value) = resolve_env_value(config_value) {
            if !env_value.is_empty() {
                return env_value;
            }
        }

        if let Some(password) = Self::get_keyring_password(provider_name) {
            return password;
        }

        if config_value.starts_with("{env:") {
            String::new()
        } else {
            config_value.to_string()
        }
    }

    pub fn set_api_key(provider_name: &str, api_key: &str) -> Result<()> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, provider_name)
            .map_err(|e| anyhow::anyhow!("Failed to create keyring entry: {e}"))?;
        entry
            .set_password(api_key)
            .map_err(|e| anyhow::anyhow!("Failed to store API key in keyring: {e}"))?;
        info!("API key stored in keyring for provider '{}'", provider_name);
        Ok(())
    }

    fn get_keyring_password(provider_name: &str) -> Option<String> {
        match keyring::Entry::new(KEYRING_SERVICE, provider_name) {
            Ok(entry) => match entry.get_password() {
                Ok(password) => {
                    if password.is_empty() {
                        None
                    } else {
                        Some(password)
                    }
                }
                Err(keyring::Error::NoEntry) => None,
                Err(e) => {
                    warn!(
                        "Keyring lookup failed for provider '{}': {}",
                        provider_name, e
                    );
                    None
                }
            },
            Err(e) => {
                warn!(
                    "Failed to create keyring entry for provider '{}': {}",
                    provider_name, e
                );
                None
            }
        }
    }
}

fn resolve_env_value(value: &str) -> Option<String> {
    if let Some(inner) = value
        .strip_prefix("{env:")
        .and_then(|s| s.strip_suffix('}'))
    {
        let var_name = inner.trim();
        if var_name.is_empty() {
            warn!("Empty environment variable name in '{}'", value);
            return Some(String::new());
        }
        match std::env::var(var_name) {
            Ok(v) => Some(v),
            Err(_) => {
                warn!(
                    "Environment variable '{}' not set for api_key reference '{}'",
                    var_name, value
                );
                Some(String::new())
            }
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_api_key_resolves_env_var() {
        std::env::set_var("OPENLIBERTAS_CRED_TEST", "sk-from-env");
        let result =
            CredentialManager::get_api_key("{env:OPENLIBERTAS_CRED_TEST}", "test-provider");
        assert_eq!(result, "sk-from-env");
        std::env::remove_var("OPENLIBERTAS_CRED_TEST");
    }

    #[test]
    fn get_api_key_returns_plaintext_when_no_env_match() {
        let result = CredentialManager::get_api_key("sk-plaintext", "test-provider");
        assert_eq!(result, "sk-plaintext");
    }

    #[test]
    fn get_api_key_returns_empty_on_unresolved_env_ref() {
        std::env::remove_var("OPENLIBERTAS_CRED_MISSING");
        let result =
            CredentialManager::get_api_key("{env:OPENLIBERTAS_CRED_MISSING}", "test-provider");
        assert_eq!(result, "");
    }

    #[test]
    fn get_api_key_returns_empty_on_empty_env_var_name() {
        let result = CredentialManager::get_api_key("{env:}", "test-provider");
        assert_eq!(result, "");
    }

    fn keyring_available() -> bool {
        let test_provider = "openlibertas-test-keyring-availability";
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, test_provider) {
            if entry.set_password("test").is_ok() {
                let _ = entry.delete_credential();
                return true;
            }
        }
        false
    }

    #[test]
    fn get_api_key_env_takes_priority_over_keyring_and_plaintext() {
        std::env::set_var("OPENLIBERTAS_CRED_PRIORITY", "sk-env-wins");
        let result =
            CredentialManager::get_api_key("{env:OPENLIBERTAS_CRED_PRIORITY}", "test-provider");
        assert_eq!(result, "sk-env-wins");
        std::env::remove_var("OPENLIBERTAS_CRED_PRIORITY");
    }

    #[test]
    fn set_and_get_api_key_roundtrip() {
        if !keyring_available() {
            return;
        }
        let provider = "openlibertas-test-provider";
        let key = "sk-test-key-12345";

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }

        CredentialManager::set_api_key(provider, key).expect("Failed to set API key");

        let result = CredentialManager::get_api_key("", provider);
        assert_eq!(result, key);

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }
    }

    #[test]
    fn get_api_key_keyring_takes_priority_over_plaintext() {
        if !keyring_available() {
            return;
        }
        let provider = "openlibertas-test-priority";
        let keyring_key = "sk-keyring-wins";

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }

        CredentialManager::set_api_key(provider, keyring_key).expect("Failed to set API key");

        let result = CredentialManager::get_api_key("sk-plaintext", provider);
        assert_eq!(result, keyring_key);

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }
    }

    #[test]
    fn get_api_key_falls_back_to_plaintext_when_keyring_empty() {
        let provider = "openlibertas-test-fallback";

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }

        let result = CredentialManager::get_api_key("sk-fallback", provider);
        assert_eq!(result, "sk-fallback");
    }

    #[test]
    fn get_api_key_env_takes_priority_over_keyring() {
        if !keyring_available() {
            return;
        }
        let provider = "openlibertas-test-env-priority";
        let keyring_key = "sk-keyring";

        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }

        CredentialManager::set_api_key(provider, keyring_key).expect("Failed to set API key");
        std::env::set_var("OPENLIBERTAS_CRED_ENV_PRIORITY", "sk-env-wins");

        let result =
            CredentialManager::get_api_key("{env:OPENLIBERTAS_CRED_ENV_PRIORITY}", provider);
        assert_eq!(result, "sk-env-wins");

        std::env::remove_var("OPENLIBERTAS_CRED_ENV_PRIORITY");
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, provider) {
            let _ = entry.delete_credential();
        }
    }
}
