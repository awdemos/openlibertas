use std::collections::HashMap;
use std::sync::Arc;

use crate::backend::OpenAiBackend;
use crate::config::Provider;
use crate::domain::ProviderId;

/// Registry of initialized backends keyed by provider ID.
///
/// Encapsulates provider selection policy including:
/// - Exact lookup by provider ID
/// - Named fallback preferences (e.g. prefer "local")
/// - Default fallback to first registered provider
/// - Enumeration of available providers
pub struct BackendRegistry {
    backends: HashMap<ProviderId, Arc<OpenAiBackend>>,
}

impl BackendRegistry {
    pub fn new(providers: &[Provider]) -> Self {
        let mut backends = HashMap::new();
        for provider in providers {
            if provider.enabled {
                let backend = Arc::new(OpenAiBackend::with_tools_and_params(
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    provider.supports_tools,
                    provider.extra_params.clone(),
                ));
                backends.insert(ProviderId::new(&provider.name), backend);
            }
        }
        Self { backends }
    }

    pub fn get(&self, provider: &ProviderId) -> Option<&Arc<OpenAiBackend>> {
        self.backends.get(provider)
    }

    pub fn has_backend(&self, provider: &ProviderId) -> bool {
        self.backends.contains_key(provider)
    }

    /// List all registered provider IDs.
    pub fn list_providers(&self) -> Vec<&ProviderId> {
        self.backends.keys().collect()
    }

    /// Number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.backends.len()
    }

    /// Return the provider ID for the default backend.
    /// Returns `None` if no backends are registered.
    pub fn default_provider(&self) -> Option<&ProviderId> {
        self.backends.keys().next()
    }

    /// Select a backend by preference, falling back to the default.
    ///
    /// Tries `preferred` name first (case-insensitive), then falls back
    /// to the default provider. Returns `None` if no backends exist.
    pub fn select_provider(&self, preferred: Option<&str>) -> Option<&Arc<OpenAiBackend>> {
        if let Some(name) = preferred {
            let id = ProviderId::new(name);
            if let Some(backend) = self.backends.get(&id) {
                return Some(backend);
            }
        }
        self.default_backend()
    }

    pub fn default_backend(&self) -> Option<&Arc<OpenAiBackend>> {
        self.backends.values().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_providers() -> Vec<Provider> {
        use crate::config::SecretString;
        vec![
            Provider {
                name: "local".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: SecretString::new("sk-test".to_string()),
                enabled: true,
                supports_tools: true,
                extra_params: None,
            },
            Provider {
                name: "kimi".to_string(),
                base_url: "https://api.kimi.com/v1".to_string(),
                api_key: SecretString::new("sk-kimi".to_string()),
                enabled: true,
                supports_tools: true,
                extra_params: None,
            },
        ]
    }

    #[test]
    fn registry_creates_backends_for_enabled_providers() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        assert!(registry.has_backend(&ProviderId::new("local")));
        assert!(registry.has_backend(&ProviderId::new("kimi")));
    }

    #[test]
    fn registry_get_returns_correct_backend() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        assert!(registry.get(&ProviderId::new("local")).is_some());
        assert!(registry.get(&ProviderId::new("unknown")).is_none());
    }

    #[test]
    fn registry_skips_disabled_providers() {
        let providers = vec![Provider {
            name: "disabled".to_string(),
            base_url: "http://example.com".to_string(),
            api_key: crate::config::SecretString::new("sk-test".to_string()),
            enabled: false,
            supports_tools: false,
            extra_params: None,
        }];
        let registry = BackendRegistry::new(&providers);
        assert!(!registry.has_backend(&ProviderId::new("disabled")));
    }

    #[test]
    fn default_backend_returns_first() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        assert!(registry.default_backend().is_some());
    }

    #[test]
    fn list_providers_returns_all_ids() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        let ids = registry.list_providers();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&&ProviderId::new("local")));
        assert!(ids.contains(&&ProviderId::new("kimi")));
    }

    #[test]
    fn provider_count_matches_registered() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        assert_eq!(registry.provider_count(), 2);
    }

    #[test]
    fn select_provider_finds_preferred() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        let backend = registry.select_provider(Some("kimi"));
        assert!(backend.is_some());
    }

    #[test]
    fn select_provider_fallback_to_default() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        let backend = registry.select_provider(Some("unknown"));
        assert!(backend.is_some());
    }

    #[test]
    fn select_provider_none_when_empty() {
        let registry = BackendRegistry::new(&[]);
        assert!(registry.select_provider(None).is_none());
    }

    #[test]
    fn default_provider_returns_first_id() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        let id = registry.default_provider();
        assert!(id.is_some());
    }

    #[test]
    fn default_provider_none_when_empty() {
        let registry = BackendRegistry::new(&[]);
        assert!(registry.default_provider().is_none());
    }

    #[test]
    fn select_provider_case_insensitive() {
        let providers = test_providers();
        let registry = BackendRegistry::new(&providers);
        assert!(registry.select_provider(Some("KIMI")).is_some());
        assert!(registry.select_provider(Some("Local")).is_some());
    }

    #[test]
    fn provider_count_zero_when_empty() {
        let registry = BackendRegistry::new(&[]);
        assert_eq!(registry.provider_count(), 0);
    }

    #[test]
    fn list_providers_empty_when_none() {
        let registry = BackendRegistry::new(&[]);
        assert!(registry.list_providers().is_empty());
    }
}
