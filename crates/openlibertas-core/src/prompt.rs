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
        } else if lower.contains("local") || lower.contains("ollama") || lower.contains("lmstudio") {
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
