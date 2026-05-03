use crate::backend::{Message, Model};
use crate::domain::ProviderId;
use crate::store::ConversationStore;

/// All available slash commands
pub const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/tools",
    "/model",
    "/models",
    "/clear",
    "/quit",
    "/mcp",
    "/agents",
    "/poke",
    "/save",
    "/load",
    "/sessions",
    "/edit",
    "/delmsg",
    "/delete",
    "/export",
    "/search",
    "/themes",
    "/new",
];

/// Description for each slash command
pub fn command_description(cmd: &str) -> &'static str {
    match cmd {
        "/help" => "Show help",
        "/tools" => "Toggle tools panel",
        "/model" => "Switch model",
        "/models" => "Open model selection",
        "/clear" => "Clear conversation",
        "/save" => "Save session",
        "/load" => "Load session",
        "/sessions" => "Show saved sessions",
        "/export" => "Export to markdown/json/txt",
        "/search" => "Search in conversation",
        "/themes" => "Change color theme",
        "/delete" => "Delete session",
        "/new" => "Start new session",
        "/agents" => "Open agent configuration",
        "/poke" => "Toggle poke mode",
        "/edit" => "Edit a message",
        "/delmsg" => "Delete a message",
        "/mcp" => "Toggle MCP panel",
        "/quit" => "Quit",
        _ => "",
    }
}

/// Parsed slash command
#[derive(Debug, Clone, PartialEq)]
pub enum SlashCommand {
    Help,
    Tools,
    Model(String),
    Models,
    Clear,
    Quit,
    Mcp,
    Agents,
    Poke,
    Save(String),
    Load(String),
    Sessions,
    Edit(usize),
    DeleteMessage(usize),
    Delete(String),
    Export(String),
    Search(String),
    Themes(String),
    New,
    Unknown(String),
}

impl SlashCommand {
    /// Parse a slash command from user input
    pub fn parse(input: &str) -> Option<Self> {
        if !input.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "/help" => Some(SlashCommand::Help),
            "/tools" => Some(SlashCommand::Tools),
            "/model" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Model(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Model(String::new()))
                }
            }
            "/models" => Some(SlashCommand::Models),
            "/clear" => Some(SlashCommand::Clear),
            "/quit" => Some(SlashCommand::Quit),
            "/mcp" => Some(SlashCommand::Mcp),
            "/agents" => Some(SlashCommand::Agents),
            "/poke" => Some(SlashCommand::Poke),
            "/save" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Save(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Save(String::new()))
                }
            }
            "/load" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Load(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Load(String::new()))
                }
            }
            "/sessions" => Some(SlashCommand::Sessions),
            "/edit" => {
                if parts.len() > 1 {
                    parts[1].parse::<usize>().ok().map(SlashCommand::Edit)
                } else {
                    Some(SlashCommand::Edit(0))
                }
            }
            "/delmsg" => {
                if parts.len() > 1 {
                    parts[1].parse::<usize>().ok().map(SlashCommand::DeleteMessage)
                } else {
                    Some(SlashCommand::DeleteMessage(0))
                }
            }
            "/delete" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Delete(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Delete(String::new()))
                }
            }
            "/export" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Export(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Export(String::new()))
                }
            }
            "/search" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Search(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Search(String::new()))
                }
            }
            "/themes" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Themes(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Themes(String::new()))
                }
            }
            "/new" => Some(SlashCommand::New),
            cmd => Some(SlashCommand::Unknown(cmd.to_string())),
        }
    }

    /// Get autocomplete suggestions for a given prefix
    pub fn autocomplete(prefix: &str) -> Vec<&'static str> {
        if !prefix.starts_with('/') {
            return Vec::new();
        }
        SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix) && *cmd != &prefix)
            .copied()
            .collect()
    }
}

/// Result of executing a slash command
#[derive(Debug, Clone)]
pub enum CommandResult {
    /// Show a message to the user
    Message(String),
    /// Quit the application
    Quit,
    /// No response needed
    Silent,
}

/// Model switch result
#[derive(Debug, Clone)]
pub enum ModelSwitchResult {
    /// Successfully switched to this model and provider
    Switched { model: String, provider: ProviderId },
    /// Model not found, with suggestions
    NotFound { query: String, suggestions: Vec<String> },
    /// Show current model
    Current(String),
    /// Open model picker
    ShowPicker,
}

/// Find a model by name (exact or fuzzy match)
pub fn find_model(models: &[Model], query: &str) -> Option<(usize, String)> {
    let query_lower = query.to_lowercase();
    
    // First try exact match (case-insensitive)
    if let Some((idx, model)) = models.iter().enumerate().find(|(_, m)| {
        m.id.to_lowercase() == query_lower
    }) {
        return Some((idx, model.id.clone()));
    }
    
    // Then try substring match
    let matches: Vec<(usize, String)> = models.iter().enumerate()
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
    models.iter()
        .filter(|m| m.id.to_lowercase().contains(&query_lower))
        .map(|m| m.id.clone())
        .take(5)
        .collect()
}

/// Build the help message
pub fn build_help_message() -> String {
    "Available commands:\n\
     /help           - Show this help\n\
     /tools          - Toggle tools panel\n\
     /model          - Switch model (e.g., /model gpt-4)\n\
     /models         - Open model selection menu\n\
     /clear          - Clear conversation\n\
     /new            - Start new session\n\
     /save           - Save session (/save [name])\n\
     /load           - Load session (/load [name])\n\
     /sessions       - List saved sessions\n\
     /export         - Export to markdown/json/txt (/export [file])\n\
     /search         - Search in conversation (/search [query])\n\
     /themes         - Change color theme (/theme [name], or /themes for picker)\n\
     /delete         - Delete session (/delete [name])\n\
     /quit           - Quit openlibertas\n\
     /mcp            - Toggle MCP servers panel\n\
     /agents         - Open agent configuration panel\n\
     /poke           - Toggle poke mode (click to send [POKE] to LLM)\n\
     /edit <n>       - Edit the nth user message\n\
     /delmsg <n>     - Delete the nth message".to_string()
}

/// Session data returned from load
#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub messages: Vec<Message>,
    pub model: Option<String>,
}

/// Load a session including model information
pub fn load_session(store: &ConversationStore, name: &str) -> Result<LoadedSession, String> {
    let path = store.conversation_path(name);
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read conversation: {}", e))?;
    let conversation: crate::store::Conversation = serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse conversation: {}", e))?;
    
    Ok(LoadedSession {
        messages: conversation.messages,
        model: conversation.model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_command() {
        let cmd = SlashCommand::parse("/help");
        assert_eq!(cmd, Some(SlashCommand::Help));
    }

    #[test]
    fn parse_model_command_with_arg() {
        let cmd = SlashCommand::parse("/model gpt-4");
        assert_eq!(cmd, Some(SlashCommand::Model("gpt-4".to_string())));
    }

    #[test]
    fn parse_model_command_empty() {
        let cmd = SlashCommand::parse("/model");
        assert_eq!(cmd, Some(SlashCommand::Model(String::new())));
    }

    #[test]
    fn parse_save_command_default_name() {
        let cmd = SlashCommand::parse("/save");
        assert_eq!(cmd, Some(SlashCommand::Save(String::new())));
    }

    #[test]
    fn parse_save_command_with_name() {
        let cmd = SlashCommand::parse("/save my-session");
        assert_eq!(cmd, Some(SlashCommand::Save("my-session".to_string())));
    }

    #[test]
    fn parse_unknown_command() {
        let cmd = SlashCommand::parse("/foobar");
        assert_eq!(cmd, Some(SlashCommand::Unknown("/foobar".to_string())));
    }

    #[test]
    fn parse_no_slash_returns_none() {
        let cmd = SlashCommand::parse("hello world");
        assert!(cmd.is_none());
    }

    #[test]
    fn autocomplete_suggestions_for_prefix() {
        let suggestions = SlashCommand::autocomplete("/s");
        assert!(suggestions.contains(&"/save"));
        assert!(suggestions.contains(&"/search"));
        assert!(suggestions.contains(&"/sessions"));
    }

    #[test]
    fn autocomplete_no_match_for_non_slash() {
        let suggestions = SlashCommand::autocomplete("hello");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn find_model_exact_match() {
        let models = vec![
            Model { id: "gpt-4".to_string(), provider: ProviderId::new("openai") },
            Model { id: "gpt-3.5".to_string(), provider: ProviderId::new("openai") },
        ];
        let result = find_model(&models, "gpt-4");
        assert_eq!(result, Some((0, "gpt-4".to_string())));
    }

    #[test]
    fn find_model_case_insensitive() {
        let models = vec![
            Model { id: "GPT-4".to_string(), provider: ProviderId::new("openai") },
        ];
        let result = find_model(&models, "gpt-4");
        assert_eq!(result, Some((0, "GPT-4".to_string())));
    }

    #[test]
    fn find_model_substring_match() {
        let models = vec![
            Model { id: "gpt-4-turbo".to_string(), provider: ProviderId::new("openai") },
        ];
        let result = find_model(&models, "turbo");
        assert_eq!(result, Some((0, "gpt-4-turbo".to_string())));
    }

    #[test]
    fn find_model_no_match() {
        let models = vec![
            Model { id: "gpt-4".to_string(), provider: ProviderId::new("openai") },
        ];
        let result = find_model(&models, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn model_suggestions_filter_by_query() {
        let models = vec![
            Model { id: "gpt-4".to_string(), provider: ProviderId::new("openai") },
            Model { id: "gpt-4-turbo".to_string(), provider: ProviderId::new("openai") },
            Model { id: "claude-3".to_string(), provider: ProviderId::new("anthropic") },
        ];
        let suggestions = get_model_suggestions(&models, "gpt");
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions.contains(&"gpt-4".to_string()));
        assert!(suggestions.contains(&"gpt-4-turbo".to_string()));
    }
}