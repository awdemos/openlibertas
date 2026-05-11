use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

use crate::tools::BuiltinTool;

const ALLOWED_GIT_COMMANDS: &[&str] = &[
    "status", "diff", "log", "branch", "show", "blame", "stash", "remote", "add", "commit", "push",
    "pull", "fetch", "merge", "rebase", "checkout", "init", "clone", "reset", "clean", "tag",
    "config", "grep", "bisect",
];

fn validate_git_command(subcommand: &str) -> Result<()> {
    if !ALLOWED_GIT_COMMANDS.contains(&subcommand.to_lowercase().as_str()) {
        return Err(anyhow::anyhow!(
            "Git subcommand '{}' is not in the allowlist",
            subcommand
        ));
    }
    Ok(())
}

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

    validate_git_command(&args.subcommand)?;

    let mut cmd = std::process::Command::new("git");
    cmd.arg(&args.subcommand);

    if let Some(additional) = args.args {
        for arg in additional.split_whitespace() {
            cmd.arg(arg);
        }
    }

    if let Some(path) = args.path {
        let sandbox = std::env::current_dir().context("Failed to get working directory")?;
        let target = Path::new(&path);
        if target.is_absolute() {
            return Err(anyhow::anyhow!(
                "Absolute paths are not allowed: {}. Use a relative path.",
                path
            ));
        }
        let resolved = sandbox.join(target);
        let canonical = resolved.canonicalize().unwrap_or(resolved.clone());
        let canonical_sandbox = sandbox.canonicalize().unwrap_or(sandbox);
        if !canonical.starts_with(&canonical_sandbox) {
            return Err(anyhow::anyhow!("Path escapes working directory: {}", path));
        }
        cmd.current_dir(resolved);
    }

    crate::tools::run_command(&mut cmd).map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_status_runs() {
        let result = git(serde_json::json!({"subcommand": "status", "args": "--short"}));
        assert!(result.is_ok(), "git status failed: {:?}", result);
    }
}
