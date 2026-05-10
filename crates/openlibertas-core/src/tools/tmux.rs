use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;

use crate::tools::BuiltinTool;

#[derive(Debug, Deserialize)]
struct TmuxArgs {
    subcommand: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    keys: Option<String>,
}

pub fn tool_definition() -> BuiltinTool {
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
        run
    )
}

pub fn run(args: Value) -> Result<String> {
    let args: TmuxArgs = serde_json::from_value(args)?;

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

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run tmux {}", args.subcommand))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        result.push_str(&format!("stderr: {}\n", stderr));
    }

    Ok(result.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_list_sessions_runs() {
        let result = run(serde_json::json!({"subcommand": "list-sessions"}));
        assert!(result.is_ok());
    }
}
