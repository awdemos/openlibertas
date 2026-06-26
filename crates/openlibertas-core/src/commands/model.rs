use crate::domain::{Model, ProviderId};

/// Model switch result
#[derive(Debug, Clone)]
pub enum ModelSwitchResult {
    /// Successfully switched to this model and provider
    Switched { model: String, provider: ProviderId },
    /// Model not found, with suggestions
    NotFound {
        query: String,
        suggestions: Vec<String>,
    },
    /// Show current model
    Current(String),
    /// Open model picker
    ShowPicker,
}

/// Find a model by name (exact or fuzzy match)
pub fn find_model(models: &[Model], query: &str) -> Option<(usize, String)> {
    let query_lower = query.to_lowercase();

    // First try exact match (case-insensitive)
    if let Some((idx, model)) = models
        .iter()
        .enumerate()
        .find(|(_, m)| m.id.to_lowercase() == query_lower)
    {
        return Some((idx, model.id.clone()));
    }

    // Then try substring match
    let matches: Vec<(usize, String)> = models
        .iter()
        .enumerate()
        .filter(|(_, m)| m.id.to_lowercase().contains(&query_lower))
        .map(|(idx, m)| (idx, m.id.clone()))
        .collect();

    if matches.len() == 1 {
        return Some(matches[0].clone());
    }

    None
}

/// Get fuzzy match suggestions for a model query
pub fn get_model_suggestions(models: &[Model], query: &str) -> Vec<String> {
    let query_lower = query.to_lowercase();
    models
        .iter()
        .filter(|m| m.id.to_lowercase().contains(&query_lower))
        .map(|m| m.id.clone())
        .take(5)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_model(id: &str, provider: &str) -> Model {
        Model {
            id: id.to_string(),
            provider: ProviderId::new(provider),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }
    }

    #[test]
    fn find_model_exact_match() {
        let models = vec![
            test_model("gpt-4", "openai"),
            test_model("gpt-3.5", "openai"),
        ];
        let result = find_model(&models, "gpt-4");
        assert_eq!(result, Some((0, "gpt-4".to_string())));
    }

    #[test]
    fn find_model_case_insensitive() {
        let models = vec![test_model("GPT-4", "openai")];
        let result = find_model(&models, "gpt-4");
        assert_eq!(result, Some((0, "GPT-4".to_string())));
    }

    #[test]
    fn find_model_substring_match() {
        let models = vec![test_model("gpt-4-turbo", "openai")];
        let result = find_model(&models, "turbo");
        assert_eq!(result, Some((0, "gpt-4-turbo".to_string())));
    }

    #[test]
    fn find_model_no_match() {
        let models = vec![test_model("gpt-4", "openai")];
        let result = find_model(&models, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn model_suggestions_filter_by_query() {
        let models = vec![
            test_model("gpt-4", "openai"),
            test_model("gpt-4-turbo", "openai"),
            test_model("claude-3", "anthropic"),
        ];
        let suggestions = get_model_suggestions(&models, "gpt");
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions.contains(&"gpt-4".to_string()));
        assert!(suggestions.contains(&"gpt-4-turbo".to_string()));
    }

    #[test]
    fn model_suggestions_empty_query_returns_all() {
        let models = vec![test_model("model-a", "local")];
        let suggestions = get_model_suggestions(&models, "");
        assert_eq!(suggestions.len(), 1);
    }

    #[test]
    fn find_model_ambiguous_substring_returns_none() {
        let models = vec![
            test_model("gpt-4-turbo", "openai"),
            test_model("gpt-4-mini", "openai"),
        ];
        // "gpt-4" matches both, so result should be None.
        let result = find_model(&models, "gpt-4");
        assert!(result.is_none());
    }
}
