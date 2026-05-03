use std::collections::HashMap;
use std::sync::Arc;

use crate::backend::{Backend, OpenAiBackend};
use crate::config::Provider;
use crate::domain::ProviderId;

pub struct BackendRegistry {
    backends: HashMap<ProviderId, Arc<dyn Backend>>,
}

impl BackendRegistry {
    pub fn new(providers: &[Provider]) -> Self {
        let mut backends = HashMap::new();
        for provider in providers {
            if provider.enabled {
                let backend: Arc<dyn Backend> = Arc::new(OpenAiBackend::with_tools(
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    provider.supports_tools,
                ));
                backends.insert(ProviderId::new(&provider.name), backend);
            }
        }
        Self { backends }
    }

    pub fn get(&self, provider: &ProviderId) -> Option<&Arc<dyn Backend>> {
        self.backends.get(provider)
    }

    pub fn default_backend(&self) -> Option<&Arc<dyn Backend>> {
        self.backends.values().next()
    }

    #[allow(dead_code)]
    pub fn has_backend(&self, provider: &ProviderId) -> bool {
        self.backends.contains_key(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_providers() -> Vec<Provider> {
        vec![
            Provider {
                name: "local".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: "sk-test".to_string(),
                enabled: true,
                supports_tools: true,
            },
            Provider {
                name: "kimi".to_string(),
                base_url: "https://api.kimi.com/v1".to_string(),
                api_key: "sk-kimi".to_string(),
                enabled: true,
                supports_tools: true,
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
            api_key: "sk-test".to_string(),
            enabled: false,
            supports_tools: false,
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
}
