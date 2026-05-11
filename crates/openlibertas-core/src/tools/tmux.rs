use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::BuiltinTool;

const ALLOWED_TMUX_COMMANDS: &[&str] = &[
    "list-sessions",
    "list-windows",
    "list-panes",
    "capture-pane",
    "send-keys",
    "new-session",
    "kill-session",
    "attach-session",
    "detach-client",
    "has-session",
    "display-message",
];

fn validate_tmux_command(subcommand: &str) -> Result<()> {
    if !ALLOWED_TMUX_COMMANDS.contains(&subcommand.to_lowercase().as_str()) {
        return Err(anyhow::anyhow!(
            "Tmux subcommand '{}' is not in the allowlist",
            subcommand
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct TmuxArgs {
    subcommand: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    keys: Option<String>,
}

pub fn tmux_tool() -> BuiltinTool {
    crate::define_tool!(
        "tmux",
        "Interact with tmux sessions (list, capture pane, send keys).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "The tmux subcommand: list-sessions, capture-pane, send-keys"
                },
                "target": {
                    "type": "string",
                    "description": "Session or pane target (e.g., 'my-session:0.0')"
                },
                "keys": {
                    "type": "string",
                    "description": "Keys to send (for send-keys subcommand)"
                }
            },
            "required": ["subcommand"]
        }),
        tmux
    )
}

pub fn tmux(args: Value) -> Result<String> {
    let args: TmuxArgs = serde_json::from_value(args)?;
    validate_tmux_command(&args.subcommand)?;

    let mut cmd = std::process::Command::new("tmux");
    cmd.arg(&args.subcommand);

    if let Some(target) = args.target {
        cmd.arg("-t").arg(target);
    }

    match args.subcommand.as_str() {
        "capture-pane" => {
            cmd.arg("-p");
        }
        "send-keys" => {
            if let Some(keys) = args.keys {
                for key in keys.split_whitespace() {
                    cmd.arg(key);
                }
            }
        }
        _ => {}
    }

    crate::tools::run_command(&mut cmd).map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_list_sessions_runs() {
        let result = tmux(serde_json::json!({"subcommand": "list-sessions"}));
        assert!(result.is_ok());
    }
}
