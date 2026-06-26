use super::help::SLASH_COMMANDS;

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
        assert_eq!(
            cmd,
            Some(SlashCommand::Export("/path/to/chat.md".to_string()))
        );
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
}
