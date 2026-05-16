use crate::domain::{Message, Model, ProviderId};
use crate::store::SessionStore;
use anyhow::Context;

/// All available slash commands organized by category
pub const SLASH_COMMANDS: &[&str] = &[
    // Info
    "/help",
    // Config
    "/model",
    "/models",
    "/theme",
    "/temp",
    "/set-key",
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
    "/branch",
    // Chat
    "/search",
    "/edit",
    "/remove",
    // Agent
    "/agents",
    "/avatar",
    "/avatar-menu",
    "/yolo",
    "/plan",
    "/compact",
    // RLM
    "/rlm",
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
        "/help" => "Info",
        "/model" | "/theme" | "/temp" | "/avatar" | "/avatar-menu" | "/set-key" => "Config",
        "/new" | "/clear" | "/save" | "/load" | "/sessions" | "/delete" | "/export" | "/undo"
        | "/title" | "/branch" => "Session",
        "/search" | "/edit" | "/remove" => "Chat",
        "/agents" | "/yolo" | "/plan" | "/compact" => "Agent",
        "/rlm" => "RLM",
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
        "/model" | "/models" => "Switch model (or open picker)",
        "/avatar" => "Toggle avatar display",
        "/avatar-menu" => "Open avatar configuration menu",
        "/theme" => "Change color theme",
        "/temp" => "Set LLM temperature (0.0-2.0)",
        "/set-key" => "Store provider API key in OS keyring",
        "/new" => "Start new session",
        "/clear" => "Clear session history",
        "/save" => "Save session to disk",
        "/load" => "Load session from disk",
        "/sessions" => "List saved sessions",
        "/delete" => "Delete a saved session",
        "/export" => "Export to markdown/json/txt",
        "/undo" => "Undo last turn",
        "/title" => "Rename current session",
        "/branch" => "Branch session from message index",
        "/search" => "Search in session",
        "/edit" => "Edit a message by index",
        "/remove" => "Remove a message by index",
        "/agents" => "Open agent configuration",
        "/yolo" => "Toggle auto-approval for tools",
        "/plan" => "Toggle plan mode (read-only research)",
        "/compact" => "Compact session context",
        "/rlm" => "Toggle RLM (recursive language model) mode",
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
    Help(Option<String>),
    Model(String),
    Theme(String),
    Temperature(f32),
    SetKey(String, String),
    New,
    Clear,
    Save(String),
    Load(String),
    Sessions,
    Delete(String),
    Export(String),
    Undo,
    Title(String),
    Branch(Option<usize>),
    Search(String),
    Edit(usize),
    Remove(usize),
    Agents,
    Avatar(Option<String>),
    AvatarMenu,
    Yolo,
    Plan,
    Compact,
    Rlm,
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
            "/help" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Help(Some(parts[1..].join(" "))))
                } else {
                    Some(SlashCommand::Help(None))
                }
            }
            "/model" | "/models" => {
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
            "/avatar" => {
                if parts.len() > 1 {
                    Some(SlashCommand::Avatar(Some(parts[1..].join(" "))))
                } else {
                    Some(SlashCommand::Avatar(None))
                }
            }
            "/avatar-menu" => Some(SlashCommand::AvatarMenu),
            "/yolo" => Some(SlashCommand::Yolo),
            "/plan" => Some(SlashCommand::Plan),
            "/compact" => Some(SlashCommand::Compact),
            "/rlm" => Some(SlashCommand::Rlm),
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
            "/temp" => {
                if parts.len() > 1 {
                    parts[1].parse::<f32>().ok().map(SlashCommand::Temperature)
                } else {
                    Some(SlashCommand::Temperature(0.7))
                }
            }
            "/set-key" => {
                if parts.len() >= 3 {
                    let provider = parts[1].to_string();
                    let key = parts[2..].join(" ");
                    Some(SlashCommand::SetKey(provider, key))
                } else {
                    Some(SlashCommand::SetKey(String::new(), String::new()))
                }
            }
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
            "/branch" => {
                if parts.len() > 1 {
                    parts[1]
                        .parse::<usize>()
                        .ok()
                        .map(|n| SlashCommand::Branch(Some(n)))
                } else {
                    Some(SlashCommand::Branch(None))
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

/// Detailed help for a specific command
pub fn command_detailed_help(cmd: &str) -> Option<String> {
    let cmd = if cmd.starts_with('/') { cmd } else { &format!("/{}", cmd) };
    let desc = command_description(cmd);
    if desc.is_empty() {
        return None;
    }
    let usage = match cmd {
        "/help" => "Usage: /help [command]\nShow help panel or detailed help for a specific command.",
        "/model" | "/models" => "Usage: /model [name]\nSwitch to a specific model or open the model picker.\nExamples: /model gpt-4, /model qwen2.5-coder",
        "/theme" => "Usage: /theme [name]\nChange the color theme.\nAvailable themes: default, solarized-dark, solarized-light, tokyo-night, tokyo-night-storm, catppuccin, catppuccin-mocha, gruvbox-dark, gruvbox-light, one-dark, dracula, nord, monokai",
        "/temp" => "Usage: /temp [0.0-2.0]\nSet the LLM temperature. Lower = more deterministic, higher = more creative.\nDefault: 0.7",
        "/set-key" => "Usage: /set-key <provider> <key>\nStore a provider API key in the OS keyring.\nThe key will be retrieved automatically when loading config.\nExample: /set-key openai sk-abc123",
        "/new" => "Usage: /new\nStart a new empty session.",
        "/clear" => "Usage: /clear\nClear the current session history.",
        "/save" => "Usage: /save [name]\nSave the current session. Auto-generates a name if none provided.",
        "/load" => "Usage: /load [name]\nLoad a saved session or open the session manager.",
        "/sessions" => "Usage: /sessions\nOpen the session manager popup.",
        "/delete" => "Usage: /delete [name]\nDelete a saved session.",
        "/export" => "Usage: /export [format|path]\nExport the current session.\nFormats: markdown (default), json, txt\nExamples: /export markdown, /export /path/to/chat.md",
        "/undo" => "Usage: /undo\nRemove the last user-assistant message pair.",
        "/title" => "Usage: /title <name>\nRename the current session.",
        "/branch" => "Usage: /branch [message_index]\nCreate a new branch from a message index.\nDefault: branch from the last message.",
        "/search" => "Usage: /search [query]\nSearch for text in the current session messages.",
        "/edit" => "Usage: /edit <n>\nEdit the nth user message and re-send from that point.",
        "/remove" => "Usage: /remove <n>\nRemove the nth message from the session.",
        "/agents" => "Usage: /agents\nOpen the agent configuration panel.",
        "/yolo" => "Usage: /yolo\nToggle auto-approval for destructive tools.",
        "/plan" => "Usage: /plan\nToggle plan mode (read-only research).",
        "/compact" => "Usage: /compact\nCompact the session context to reduce token usage.",
        "/rlm" => "Usage: /rlm\nToggle RLM (recursive language model) mode for Python code execution.",
        "/mcp" => "Usage: /mcp\nShow MCP server status panel.",
        "/tools" => "Usage: /tools\nToggle the MCP tools panel.",
        "/voice" => "Usage: /voice\nToggle voice chat mode (STT/TTS).",
        "/voice_device" => "Usage: /voice_device [name]\nList or select the audio input device.",
        "/mouse" => "Usage: /mouse\nToggle mouse capture for scrolling and clicking.",
        "/quit" => "Usage: /quit\nExit the application.",
        "/avatar" => "Usage: /avatar [on/off]\nToggle avatar display.",
        "/avatar-menu" => "Usage: /avatar-menu\nOpen avatar configuration menu.",
        _ => "",
    };
    Some(format!("{}\n\n{}\n\n{}\n\nCategory: {}", cmd, desc, usage, command_category(cmd)))
}

/// Build the help message with categorized commands
pub fn build_help_message() -> String {
    use std::fmt::Write;
    let mut output = String::from("Slash Commands\n\n");

    let categories = [
        ("Info", &["/help"][..]),
        ("Config", &["/model", "/avatar", "/theme", "/temp", "/set-key"][..]),
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
                "/branch",
            ][..],
        ),
        ("Chat", &["/search", "/edit", "/remove"][..]),
        ("Agent", &["/agents", "/yolo"][..]),
        ("RLM", &["/rlm"][..]),
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
    pub parent_id: Option<String>,
    pub branch_point: Option<usize>,
}

/// Load a session including model information
pub fn load_session(store: &SessionStore, name: &str) -> anyhow::Result<LoadedSession> {
    let path = store.session_path(name);
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read session: {}", name))?;
    let session: crate::store::Session = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse session: {}", name))?;

    Ok(LoadedSession {
        messages: session.messages,
        model: session.model,
        parent_id: session.parent_id,
        branch_point: session.branch_point,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help_command() {
        let cmd = SlashCommand::parse("/help");
        assert_eq!(cmd, Some(SlashCommand::Help(None)));
    }

    #[test]
    fn parse_help_command_with_arg() {
        let cmd = SlashCommand::parse("/help save");
        assert_eq!(cmd, Some(SlashCommand::Help(Some("save".to_string()))));
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

    #[test]
    fn parse_temp_command_with_value() {
        let cmd = SlashCommand::parse("/temp 0.5");
        assert_eq!(cmd, Some(SlashCommand::Temperature(0.5)));
    }

    #[test]
    fn parse_temp_command_defaults_to_07() {
        let cmd = SlashCommand::parse("/temp");
        assert_eq!(cmd, Some(SlashCommand::Temperature(0.7)));
    }

    #[test]
    fn parse_temp_command_invalid_returns_none() {
        let cmd = SlashCommand::parse("/temp abc");
        assert!(cmd.is_none());
    }

    #[test]
    fn parse_edit_command_with_index() {
        let cmd = SlashCommand::parse("/edit 5");
        assert_eq!(cmd, Some(SlashCommand::Edit(5)));
    }

    #[test]
    fn parse_edit_command_defaults_to_zero() {
        let cmd = SlashCommand::parse("/edit");
        assert_eq!(cmd, Some(SlashCommand::Edit(0)));
    }

    #[test]
    fn parse_branch_command_with_index() {
        let cmd = SlashCommand::parse("/branch 3");
        assert_eq!(cmd, Some(SlashCommand::Branch(Some(3))));
    }

    #[test]
    fn parse_branch_command_defaults_to_none() {
        let cmd = SlashCommand::parse("/branch");
        assert_eq!(cmd, Some(SlashCommand::Branch(None)));
    }

    #[test]
    fn parse_load_command_with_name() {
        let cmd = SlashCommand::parse("/load my-session");
        assert_eq!(cmd, Some(SlashCommand::Load("my-session".to_string())));
    }

    #[test]
    fn parse_delete_command_with_name() {
        let cmd = SlashCommand::parse("/delete old-session");
        assert_eq!(cmd, Some(SlashCommand::Delete("old-session".to_string())));
    }

    #[test]
    fn parse_export_command_with_path() {
        let cmd = SlashCommand::parse("/export /path/to/chat.md");
        assert_eq!(cmd, Some(SlashCommand::Export("/path/to/chat.md".to_string())));
    }

    #[test]
    fn parse_title_command_with_name() {
        let cmd = SlashCommand::parse("/title My Chat");
        assert_eq!(cmd, Some(SlashCommand::Title("My Chat".to_string())));
    }

    #[test]
    fn parse_search_command_with_query() {
        let cmd = SlashCommand::parse("/search hello world");
        assert_eq!(cmd, Some(SlashCommand::Search("hello world".to_string())));
    }

    #[test]
    fn parse_avatar_with_name() {
        let cmd = SlashCommand::parse("/avatar robot");
        assert_eq!(cmd, Some(SlashCommand::Avatar(Some("robot".to_string()))));
    }

    #[test]
    fn parse_avatar_without_name() {
        let cmd = SlashCommand::parse("/avatar");
        assert_eq!(cmd, Some(SlashCommand::Avatar(None)));
    }

    #[test]
    fn parse_voice_device_with_name() {
        let cmd = SlashCommand::parse("/voice_device Bose");
        assert_eq!(cmd, Some(SlashCommand::VoiceDevice("Bose".to_string())));
    }

    #[test]
    fn parse_voice_device_without_name() {
        let cmd = SlashCommand::parse("/voice_device");
        assert_eq!(cmd, Some(SlashCommand::VoiceDevice(String::new())));
    }

    #[test]
    fn autocomplete_exact_match_excluded() {
        let suggestions = SlashCommand::autocomplete("/save");
        assert!(!suggestions.contains(&"/save"));
    }

    #[test]
    fn autocomplete_empty_prefix_returns_all() {
        let suggestions = SlashCommand::autocomplete("/");
        assert!(suggestions.len() > 5);
    }

    #[test]
    fn command_category_groups_correctly() {
        assert_eq!(command_category("/help"), "Info");
        assert_eq!(command_category("/model"), "Config");
        assert_eq!(command_category("/save"), "Session");
        assert_eq!(command_category("/search"), "Chat");
        assert_eq!(command_category("/agents"), "Agent");
        assert_eq!(command_category("/mcp"), "Tools");
        assert_eq!(command_category("/voice"), "Voice");
        assert_eq!(command_category("/quit"), "System");
        assert_eq!(command_category("/unknown"), "Other");
    }

    #[test]
    fn command_description_not_empty_for_known() {
        for cmd in SLASH_COMMANDS.iter() {
            assert!(
                !command_description(cmd).is_empty(),
                "{} should have a description",
                cmd
            );
        }
    }

    #[test]
    fn find_model_ambiguous_substring_returns_none() {
        let models = vec![
            Model {
                id: "gpt-4-turbo".to_string(),
                provider: ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
            Model {
                id: "gpt-4-mini".to_string(),
                provider: ProviderId::new("openai"),
                supports_tools: true,
                supports_voice: false,
                local: false,
            },
        ];
        // "gpt-4" matches both, so result should be None.
        let result = find_model(&models, "gpt-4");
        assert!(result.is_none());
    }

    #[test]
    fn build_help_message_includes_categories() {
        let help = build_help_message();
        assert!(help.contains("Slash Commands"));
        assert!(help.contains("/help"));
        assert!(help.contains("/quit"));
    }
}
