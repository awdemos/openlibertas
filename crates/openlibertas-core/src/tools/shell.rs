use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;

use crate::tools::BuiltinTool;

#[derive(Debug, Deserialize)]
struct ShellArgs {
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
}

pub fn shell_tool() -> BuiltinTool {
    crate::define_tool!(
        "shell",
        "Execute a shell command and return its output. Use with caution.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Optional timeout in seconds (default: 30)",
                    "minimum": 1,
                    "maximum": 300
                }
            },
            "required": ["command"]
        }),
        shell
    )
}

pub fn shell(args: Value) -> Result<String> {
    let args: ShellArgs = serde_json::from_value(args)?;
    let _timeout = args.timeout.unwrap_or(30);

    if args.command.trim().is_empty() {
        return Err(anyhow::anyhow!("Empty command"));
    }

    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&args.command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("Failed to execute command: {}", args.command))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();

    if !stdout.is_empty() {
        result.push_str(&format!("stdout:\n{}\n", stdout));
    }

    if !stderr.is_empty() {
        result.push_str(&format!("stderr:\n{}\n", stderr));
    }

    if output.status.code() != Some(0) {
        result.push_str(&format!(
            "exit code: {}\n",
            output.status.code().unwrap_or(-1)
        ));
    }

    Ok(result.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_echo_works() {
        let result = shell(serde_json::json!({"command": "echo hello"})).unwrap();
        assert!(result.contains("hello"));
    }

    #[test]
    fn shell_empty_command_fails() {
        let result = shell(serde_json::json!({"command": ""}));
        assert!(result.is_err());
    }
}
