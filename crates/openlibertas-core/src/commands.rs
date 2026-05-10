use crate::domain::{Message, Model, ProviderId};
use crate::store::ConversationStore;

/// All available slash commands organized by category
pub const SLASH_COMMANDS: &[&str] = &[
    // Info
    "/help",
    "/version",
    // Config
    "/model",
    "/theme",
    // Session
    "/new",
    "/clear",
    "/save",
    "/load",
    "/sessions",
    "/delete",
    "/export",
    "/undo",
    "/title",
    // Chat
    "/search",
    "/edit",
    "/remove",
    // Agent
    "/agents",
    "/yolo",
    "/compact",
    // Tools
    "/mcp",
    "/tools",
    // Voice
    "/voice",
    "/voice_device",
    // System
    "/mouse",
    "/quit",
];

/// Category for each slash command
pub fn command_category(cmd: &str) -> &'static str {
    match cmd {
        "/help" | "/version" => "Info",
        "/model" | "/theme" => "Config",
        "/new" | "/clear" | "/save" | "/load" | "/sessions" | "/delete" | "/export" | "/undo"
        | "/title" => "Session",
        "/search" | "/edit" | "/remove" => "Chat",
        "/agents" | "/yolo" | "/compact" => "Agent",
        "/mcp" | "/tools" => "Tools",
        "/voice" => "Voice",
        "/voice_device" => "Voice",
        "/mouse" => "System",
        "/quit" => "System",
        _ => "Other",
    }
}

/// Description for each slash command
pub fn command_description(cmd: &str) -> &'static str {
    match cmd {
        "/help" => "Show help panel",
        "/version" => "Show version info",
        "/model" => "Switch model (or open picker)",
        "/theme" => "Change color theme",
        "/new" => "Start new session",
        "/clear" => "Clear conversation history",
        "/save" => "Save session to disk",
        "/load" => "Load session from disk",
        "/sessions" => "List saved sessions",
        "/delete" => "Delete a saved session",
        "/export" => "Export to markdown/json/txt",
        "/undo" => "Undo last turn",
        "/title" => "Rename current session",
        "/search" => "Search in conversation",
        "/edit" => "Edit a message by index",
        "/remove" => "Remove a message by index",
        "/agents" => "Open agent configuration",
        "/yolo" => "Toggle auto-approval for tools",
        "/compact" => "Compact conversation context",
        "/mcp" => "Show MCP server status",
        "/tools" => "Toggle tools panel",
        "/voice" => "Toggle voice chat mode",
        "/voice_device" => "List or select voice input device",
        "/mouse" => "Toggle mouse capture (for tmux copy mode)",
        "/quit" => "Quit application",
        _ => "",
    }
}

/// Parsed slash command
#[derive(Debug, Clone, PartialEq)]
pub enum SlashCommand {
    Help,
    Version,
    Model(String),
    Theme(String),
    New,
    Clear,
    Save(String),
    Load(String),
    Sessions,
    Delete(String),
    Export(String),
    Undo,
    Title(String),
    Search(String),
    Edit(usize),
    Remove(usize),
    Agents,
    Yolo,
    Compact,
    Mcp,
    Tools,
    Voice,
    VoiceDevice(String),
    Mouse,
    Quit,
    Unknown(String),
}

impl SlashCommand {
    /// Parse a slash command from user input
    pub fn parse(input: &str) -> Option<Self> {
        if !input.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();

        match parts[0] {
            "/help" => Some(SlashCommand::Help),
            "/version" => Some(SlashCommand::Version),
            "/model" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Model(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Model(String::new()))
                }
            }
            "/theme" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Theme(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Theme(String::new()))
                }
            }
            "/new" => Some(SlashCommand::New),
            "/clear" => Some(SlashCommand::Clear),
            "/quit" => Some(SlashCommand::Quit),
            "/mcp" => Some(SlashCommand::Mcp),
            "/agents" => Some(SlashCommand::Agents),
            "/yolo" => Some(SlashCommand::Yolo),
            "/compact" => Some(SlashCommand::Compact),
            "/tools" => Some(SlashCommand::Tools),
            "/voice" => Some(SlashCommand::Voice),
            "/voice_device" => {
                if parts.len() > 1 {
                    Some(SlashCommand::VoiceDevice(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::VoiceDevice(String::new()))
                }
            }
            "/mouse" => Some(SlashCommand::Mouse),
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
            "/remove" => {
                if parts.len() > 1 {
                    parts[1].parse::<usize>().ok().map(SlashCommand::Remove)
                } else {
                    Some(SlashCommand::Remove(0))
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
            "/undo" => Some(SlashCommand::Undo),
            "/title" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Title(parts[1..].join(" ")))
                } else {
                    Some(SlashCommand::Title(String::new()))
                }
            }
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

/// Build the help message with categorized commands
pub fn build_help_message() -> String {
    use std::fmt::Write;
    let mut output = String::from("Slash Commands\n\n");

    let categories = [
        ("Info", &["/help", "/version"][..]),
        ("Config", &["/model", "/theme"][..]),
        (
            "Session",
            &[
                "/new",
                "/clear",
                "/save",
                "/load",
                "/sessions",
                "/delete",
                "/export",
                "/undo",
                "/title",
            ][..],
        ),
        ("Chat", &["/search", "/edit", "/remove"][..]),
        ("Agent", &["/agents", "/yolo"][..]),
        ("Tools", &["/mcp", "/tools"][..]),
        ("Voice", &["/voice", "/voice_device"][..]),
        ("System", &["/quit"][..]),
    ];

    for (category, commands) in &categories {
        let _ = writeln!(output, "  [{}]", category);
        for cmd in *commands {
            let desc = command_description(cmd);
            let _ = writeln!(output, "    {:14} - {}", cmd, desc);
        }
        let _ = writeln!(output);
    }

    output
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
    fn parse_version_command() {
        let cmd = SlashCommand::parse("/version");
        assert_eq!(cmd, Some(SlashCommand::Version));
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
    fn parse_remove_command() {
        let cmd = SlashCommand::parse("/remove 3");
        assert_eq!(cmd, Some(SlashCommand::Remove(3)));
    }

    #[test]
    fn parse_voice_command() {
        let cmd = SlashCommand::parse("/voice");
        assert_eq!(cmd, Some(SlashCommand::Voice));
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
            Model {
                id: "gpt-4".to_string(),
                provider: ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "gpt-3.5".to_string(),
                provider: ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];
        let result = find_model(&models, "gpt-4");
        assert_eq!(result, Some((0, "gpt-4".to_string())));
    }

    #[test]
    fn find_model_case_insensitive() {
        let models = vec![Model {
            id: "GPT-4".to_string(),
            provider: ProviderId::new("openai"),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }];
        let result = find_model(&models, "gpt-4");
        assert_eq!(result, Some((0, "GPT-4".to_string())));
    }

    #[test]
    fn find_model_substring_match() {
        let models = vec![Model {
            id: "gpt-4-turbo".to_string(),
            provider: ProviderId::new("openai"),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }];
        let result = find_model(&models, "turbo");
        assert_eq!(result, Some((0, "gpt-4-turbo".to_string())));
    }

    #[test]
    fn find_model_no_match() {
        let models = vec![Model {
            id: "gpt-4".to_string(),
            provider: ProviderId::new("openai"),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }];
        let result = find_model(&models, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn model_suggestions_filter_by_query() {
        let models = vec![
            Model {
                id: "gpt-4".to_string(),
                provider: ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "gpt-4-turbo".to_string(),
                provider: ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "claude-3".to_string(),
                provider: ProviderId::new("anthropic"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];
        let suggestions = get_model_suggestions(&models, "gpt");
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions.contains(&"gpt-4".to_string()));
        assert!(suggestions.contains(&"gpt-4-turbo".to_string()));
    }

    #[test]
    fn model_suggestions_empty_query_returns_all() {
        let models = vec![Model {
            id: "model-a".to_string(),
            provider: ProviderId::new("local"),
            supports_tools: true,
            supports_voice: false,
            local: false,
        }];
        let suggestions = get_model_suggestions(&models, "");
        assert_eq!(suggestions.len(), 1);
    }
}
