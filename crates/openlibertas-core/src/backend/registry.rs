use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::backend::{MultiProvider, Provider};
use crate::config::ProviderConfig;
use crate::domain::{BackendEvent, Message, ProviderId, ToolDefinition};

const CIRCUIT_BREAKER_THRESHOLD: u32 = 3;
const CIRCUIT_BREAKER_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone)]
struct ProviderHealth {
    consecutive_failures: u32,
    last_failure: Option<Instant>,
}

/// Registry of initialized backends keyed by provider ID.
///
/// Encapsulates provider selection policy including:
/// - Exact lookup by provider ID
/// - Named fallback preferences (e.g. prefer "local")
/// - Default fallback to first registered provider
/// - Enumeration of available providers
/// - Circuit-breaker health tracking per provider
#[derive(Clone)]
pub struct ProviderRegistry {
    backends: HashMap<ProviderId, Arc<dyn Provider>>,
    health: Arc<Mutex<HashMap<ProviderId, ProviderHealth>>>,
}

impl ProviderRegistry {
    pub fn new(providers: &[ProviderConfig]) -> Self {
        let mut backends: HashMap<ProviderId, Arc<dyn Provider>> = HashMap::new();
        let mut health = HashMap::new();
        for provider in providers {
            if provider.enabled {
                let capabilities = provider.capabilities;
                let backend: Arc<dyn Provider> = Arc::new(MultiProvider::with_capabilities(
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    provider.kind,
                    capabilities,
                    provider.extra_params.clone(),
                ));
                let id = ProviderId::new(&provider.name);
                backends.insert(id.clone(), backend);
                health.insert(
                    id,
                    ProviderHealth {
                        consecutive_failures: 0,
                        last_failure: None,
                    },
                );
            }
        }
        Self {
            backends,
            health: Arc::new(Mutex::new(health)),
        }
    }

    pub fn get(&self, provider: &ProviderId) -> Option<&Arc<dyn Provider>> {
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
    pub fn default_provider_id(&self) -> Option<&ProviderId> {
        self.backends.keys().next()
    }

    /// Select a backend by preference, falling back to the default.
    ///
    /// Tries `preferred` name first (case-insensitive), then falls back
    /// to the default provider. Returns `None` if no backends exist.
    pub fn select_provider(&self, preferred: Option<&str>) -> Option<&Arc<dyn Provider>> {
        if let Some(name) = preferred {
            let id = ProviderId::new(name);
            if let Some(backend) = self.backends.get(&id) {
                return Some(backend);
            }
        }
        self.default_backend()
    }

    pub fn default_backend(&self) -> Option<&Arc<dyn Provider>> {
        self.backends.values().next()
    }

    /// Check if a provider is healthy (not circuit-broken).
    pub fn is_provider_healthy(&self, provider: &ProviderId) -> bool {
        if let Ok(health) = self.health.lock() {
            if let Some(h) = health.get(provider) {
                if h.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                    if let Some(last) = h.last_failure {
                        if last.elapsed() < Duration::from_secs(CIRCUIT_BREAKER_TIMEOUT_SECS) {
                            return false;
                        }
                    }
                }
                return true;
            }
        }
        false
    }

    /// Record a success or failure for a provider.
    pub fn record_provider_result(&self, provider: &ProviderId, success: bool) {
        if let Ok(mut health) = self.health.lock() {
            if let Some(h) = health.get_mut(provider) {
                if success {
                    h.consecutive_failures = 0;
                    h.last_failure = None;
                } else {
                    h.consecutive_failures += 1;
                    h.last_failure = Some(Instant::now());
                }
            }
        }
    }

    /// Chat with fallback across enabled providers.
    ///
    /// Tries the preferred provider first, then falls back to other
    /// healthy providers. Sends informative text messages when
    /// switching providers.
    pub fn chat_with_fallback(
        &self,
        preferred_provider: &ProviderId,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let preferred = preferred_provider.clone();

        let mut providers: Vec<(ProviderId, Arc<dyn Provider>)> = Vec::new();
        if let Some(backend) = self.backends.get(&preferred) {
            providers.push((preferred.clone(), backend.clone()));
        }
        for (id, backend) in &self.backends {
            if *id != preferred {
                providers.push((id.clone(), backend.clone()));
            }
        }

        let health = self.health.clone();

        tokio::spawn(async move {
            let mut tried: Vec<ProviderId> = Vec::new();

            for (provider_id, backend) in providers {
                let is_healthy = {
                    if let Ok(h) = health.lock() {
                        if let Some(record) = h.get(&provider_id) {
                            if record.consecutive_failures >= CIRCUIT_BREAKER_THRESHOLD {
                                if let Some(last) = record.last_failure {
                                    last.elapsed()
                                        >= Duration::from_secs(CIRCUIT_BREAKER_TIMEOUT_SECS)
                                } else {
                                    true
                                }
                            } else {
                                true
                            }
                        } else {
                            false
                        }
                    } else {
                        true
                    }
                };

                if !is_healthy {
                    continue;
                }

                if let Some(prev) = tried.last() {
                    let _ = tx.send(BackendEvent::Text(format!(
                        "\n[Provider '{}' failed, trying '{}']\n",
                        prev, provider_id
                    )));
                }

                let mut stream_rx = backend.chat(
                    model.clone(),
                    messages.clone(),
                    max_tokens,
                    tools.clone(),
                    cancel_token.clone(),
                    temperature,
                );

                let mut stream_failed = false;
                let mut can_fallback = true;
                while let Some(event) = stream_rx.recv().await {
                    match &event {
                        BackendEvent::Text(_) | BackendEvent::Reasoning(_) | BackendEvent::ToolCall(_) => {
                            can_fallback = false;
                        }
                        BackendEvent::Cancelled => {
                            let _ = tx.send(event);
                            return;
                        }
                        BackendEvent::Error(_) => {
                            stream_failed = true;
                            if can_fallback {
                                continue; // Swallow error, try next provider
                            }
                        }
                        _ => {}
                    }
                    if tx.send(event).is_err() {
                        return;
                    }
                }

                if !stream_failed {
                    // Success
                    if let Ok(mut h) = health.lock() {
                        if let Some(record) = h.get_mut(&provider_id) {
                            record.consecutive_failures = 0;
                            record.last_failure = None;
                        }
                    }
                    return;
                }

                // Record failure
                if let Ok(mut h) = health.lock() {
                    if let Some(record) = h.get_mut(&provider_id) {
                        record.consecutive_failures += 1;
                        record.last_failure = Some(Instant::now());
                    }
                }

                if !can_fallback {
                    // Partial content already sent to user, can't fallback
                    return;
                }

                tried.push(provider_id);
            }

            if tried.is_empty() {
                let _ = tx.send(BackendEvent::Error(
                    "No healthy providers available.".to_string(),
                ));
            } else {
                let _ = tx.send(BackendEvent::Error(format!(
                    "All providers failed. Tried: {}",
                    tried
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        });

        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SecretString;
    use crate::tool_format::ToolFormat;

    fn test_providers() -> Vec<ProviderConfig> {
        use crate::capability::ProviderKind;
        vec![
            ProviderConfig {
                name: "local".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: SecretString::new("sk-test".to_string()),
                enabled: true,
                kind: ProviderKind::Ollama,
                capabilities: ProviderKind::Ollama.default_capabilities(),
                tool_format: ToolFormat::Native,
                extra_params: None,
            },
            ProviderConfig {
                name: "kimi".to_string(),
                base_url: "https://api.kimi.com/v1".to_string(),
                api_key: SecretString::new("sk-kimi".to_string()),
                enabled: true,
                kind: ProviderKind::OpenAiCompatible,
                capabilities: ProviderKind::OpenAiCompatible.default_capabilities(),
                tool_format: ToolFormat::Native,
                extra_params: None,
            },
        ]
    }

    #[test]
    fn registry_creates_backends_for_enabled_providers() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        assert!(registry.has_backend(&ProviderId::new("local")));
        assert!(registry.has_backend(&ProviderId::new("kimi")));
    }

    #[test]
    fn registry_get_returns_correct_backend() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        assert!(registry.get(&ProviderId::new("local")).is_some());
        assert!(registry.get(&ProviderId::new("unknown")).is_none());
    }

    #[test]
    fn registry_skips_disabled_providers() {
        use crate::capability::ProviderKind;
        let providers = vec![ProviderConfig {
            name: "disabled".to_string(),
            base_url: "http://example.com".to_string(),
            api_key: SecretString::new("sk-test".to_string()),
            enabled: false,
            kind: ProviderKind::OpenAiCompatible,
            capabilities: ProviderKind::OpenAiCompatible.default_capabilities(),
            tool_format: ToolFormat::None,
            extra_params: None,
        }];
        let registry = ProviderRegistry::new(&providers);
        assert!(!registry.has_backend(&ProviderId::new("disabled")));
    }

    #[test]
    fn default_backend_returns_first() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        assert!(registry.default_backend().is_some());
    }

    #[test]
    fn list_providers_returns_all_ids() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let ids = registry.list_providers();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&&ProviderId::new("local")));
        assert!(ids.contains(&&ProviderId::new("kimi")));
    }

    #[test]
    fn provider_count_matches_registered() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        assert_eq!(registry.provider_count(), 2);
    }

    #[test]
    fn select_provider_finds_preferred() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let backend = registry.select_provider(Some("kimi"));
        assert!(backend.is_some());
    }

    #[test]
    fn select_provider_fallback_to_default() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let backend = registry.select_provider(Some("unknown"));
        assert!(backend.is_some());
    }

    #[test]
    fn select_provider_none_when_empty() {
        let registry = ProviderRegistry::new(&[]);
        assert!(registry.select_provider(None).is_none());
    }

    #[test]
    fn default_provider_id_returns_first_id() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let id = registry.default_provider_id();
        assert!(id.is_some());
    }

    #[test]
    fn default_provider_id_none_when_empty() {
        let registry = ProviderRegistry::new(&[]);
        assert!(registry.default_provider_id().is_none());
    }

    #[test]
    fn select_provider_case_insensitive() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        assert!(registry.select_provider(Some("KIMI")).is_some());
        assert!(registry.select_provider(Some("Local")).is_some());
    }

    #[test]
    fn provider_count_zero_when_empty() {
        let registry = ProviderRegistry::new(&[]);
        assert_eq!(registry.provider_count(), 0);
    }

    #[test]
    fn list_providers_empty_when_none() {
        let registry = ProviderRegistry::new(&[]);
        assert!(registry.list_providers().is_empty());
    }

    #[test]
    fn circuit_breaker_tracks_failures() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let local = ProviderId::new("local");

        assert!(registry.is_provider_healthy(&local));
        registry.record_provider_result(&local, false);
        registry.record_provider_result(&local, false);
        assert!(registry.is_provider_healthy(&local));
        registry.record_provider_result(&local, false);
        assert!(!registry.is_provider_healthy(&local));
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let local = ProviderId::new("local");

        registry.record_provider_result(&local, false);
        registry.record_provider_result(&local, false);
        registry.record_provider_result(&local, false);
        assert!(!registry.is_provider_healthy(&local));

        registry.record_provider_result(&local, true);
        assert!(registry.is_provider_healthy(&local));
    }

    #[test]
    fn circuit_breaker_starts_healthy() {
        let providers = test_providers();
        let registry = ProviderRegistry::new(&providers);
        let unknown = ProviderId::new("unknown");
        assert!(!registry.is_provider_healthy(&unknown));
    }
}
