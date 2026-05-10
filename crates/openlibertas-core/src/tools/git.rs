use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::process::Stdio;

use crate::tools::BuiltinTool;

#[derive(Debug, Deserialize)]
struct GitArgs {
    subcommand: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    args: Option<String>,
}

pub fn git_tool() -> BuiltinTool {
    crate::define_tool!(
        "git",
        "Run git commands (status, diff, log, branch, add, commit, etc.).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "The git subcommand to run (e.g., status, diff, log, branch)"
                },
                "path": {
                    "type": "string",
                    "description": "Optional path to the git repository (default: current directory)"
                },
                "args": {
                    "type": "string",
                    "description": "Additional arguments for the git command"
                }
            },
            "required": ["subcommand"]
        }),
        git
    )
}

pub fn git(args: Value) -> Result<String> {
    let args: GitArgs = serde_json::from_value(args)?;

    let mut cmd = std::process::Command::new("git");
    cmd.arg(&args.subcommand);

    if let Some(additional) = args.args {
        for arg in additional.split_whitespace() {
            cmd.arg(arg);
        }
    }

    if let Some(path) = args.path {
        cmd.current_dir(path);
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run git {}", args.subcommand))?;

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
    fn git_status_runs() {
        let result = run(serde_json::json!({"subcommand": "status", "args": "--short"})).unwrap();
        assert!(!result.is_empty());
    }
}
