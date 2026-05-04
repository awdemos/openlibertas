use std::collections::HashMap;

const DEFAULT_PROMPT: &str = include_str!("prompts/default.txt");
const ANTHROPIC_PROMPT: &str = include_str!("prompts/anthropic.txt");
const KIMI_PROMPT: &str = include_str!("prompts/kimi.txt");
const GPT_PROMPT: &str = include_str!("prompts/gpt.txt");
const LOCAL_PROMPT: &str = include_str!("prompts/local.txt");

pub struct PromptManager {
    prompts: HashMap<String, &'static str>,
}

impl PromptManager {
    pub fn new() -> Self {
        let mut prompts = HashMap::new();
        prompts.insert("default".to_string(), DEFAULT_PROMPT);
        prompts.insert("anthropic".to_string(), ANTHROPIC_PROMPT);
        prompts.insert("kimi".to_string(), KIMI_PROMPT);
        prompts.insert("gpt".to_string(), GPT_PROMPT);
        prompts.insert("openai".to_string(), GPT_PROMPT);
        prompts.insert("local".to_string(), LOCAL_PROMPT);
        Self { prompts }
    }

    pub fn get_prompt(&self, provider_name: &str) -> &'static str {
        let key = Self::normalize_provider(provider_name);
        self.prompts.get(&key).copied().unwrap_or(DEFAULT_PROMPT)
    }

    fn normalize_provider(name: &str) -> String {
        let lower = name.to_lowercase();
        if lower.contains("anthropic") || lower.contains("claude") {
            "anthropic".to_string()
        } else if lower.contains("kimi") {
            "kimi".to_string()
        } else if lower.contains("openai") || lower.contains("gpt") {
            "gpt".to_string()
        } else if lower.contains("local") || lower.contains("ollama") || lower.contains("lmstudio") || lower.contains("llamacpp") {
            "local".to_string()
        } else {
            "default".to_string()
        }
    }
}

impl Default for PromptManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_manager_has_all_default_prompts() {
        let manager = PromptManager::new();
        assert!(!manager.get_prompt("default").is_empty());
        assert!(!manager.get_prompt("anthropic").is_empty());
        assert!(!manager.get_prompt("kimi").is_empty());
        assert!(!manager.get_prompt("gpt").is_empty());
        assert!(!manager.get_prompt("openai").is_empty());
        assert!(!manager.get_prompt("local").is_empty());
    }

    #[test]
    fn get_prompt_returns_default_for_unknown_provider() {
        let manager = PromptManager::new();
        let default = manager.get_prompt("default");
        let unknown = manager.get_prompt("some_unknown_provider");
        assert_eq!(default, unknown);
    }

    #[test]
    fn normalize_provider_case_insensitive() {
        assert_eq!(PromptManager::normalize_provider("ANTHROPIC"), "anthropic");
        assert_eq!(PromptManager::normalize_provider("Kimi"), "kimi");
        assert_eq!(PromptManager::normalize_provider("OpenAI"), "gpt");
    }

    #[test]
    fn normalize_provider_partial_matches() {
        assert_eq!(PromptManager::normalize_provider("claude-3-opus"), "anthropic");
        assert_eq!(PromptManager::normalize_provider("gpt-4-turbo"), "gpt");
        assert_eq!(PromptManager::normalize_provider("kimi-moonshot"), "kimi");
    }

    #[test]
    fn normalize_provider_local_variants() {
        assert_eq!(PromptManager::normalize_provider("ollama"), "local");
        assert_eq!(PromptManager::normalize_provider("lmstudio"), "local");
        assert_eq!(PromptManager::normalize_provider("llamacpp"), "local");
        assert_eq!(PromptManager::normalize_provider("local"), "local");
    }

    #[test]
    fn openai_maps_to_gpt_prompt() {
        let manager = PromptManager::new();
        let gpt = manager.get_prompt("gpt");
        let openai = manager.get_prompt("openai");
        assert_eq!(gpt, openai);
    }

    #[test]
    fn default_returns_same_as_new() {
        let manager1 = PromptManager::new();
        let manager2 = PromptManager::default();
        assert_eq!(manager1.get_prompt("default"), manager2.get_prompt("default"));
    }
}
