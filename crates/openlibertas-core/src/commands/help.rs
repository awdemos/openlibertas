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

/// Detailed help for a specific command
pub fn command_detailed_help(cmd: &str) -> Option<String> {
    let cmd = if cmd.starts_with('/') {
        cmd
    } else {
        &format!("/{cmd}")
    };
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
    Some(format!(
        "{}\n\n{}\n\n{}\n\nCategory: {}",
        cmd,
        desc,
        usage,
        command_category(cmd)
    ))
}

/// Build the help message with categorized commands
pub fn build_help_message() -> String {
    use std::fmt::Write;
    let mut output = String::from("Slash Commands\n\n");

    let categories = [
        ("Info", &["/help"][..]),
        (
            "Config",
            &["/model", "/avatar", "/theme", "/temp", "/set-key"][..],
        ),
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
        let _ = writeln!(output, "  [{category}]");
        for cmd in *commands {
            let desc = command_description(cmd);
            let _ = writeln!(output, "    {cmd:14} - {desc}");
        }
        let _ = writeln!(output);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn build_help_message_includes_categories() {
        let help = build_help_message();
        assert!(help.contains("Slash Commands"));
        assert!(help.contains("/help"));
        assert!(help.contains("/quit"));
    }
}
