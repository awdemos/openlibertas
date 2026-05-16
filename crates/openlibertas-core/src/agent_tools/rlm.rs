use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::agent_tools::BuiltinTool;

#[derive(Debug, Deserialize)]
struct RlmArgs {
    code: String,
}

pub fn rlm_repl_tool() -> BuiltinTool {
    crate::define_tool!(
        "rlm_repl",
        "Execute Python code in a sandboxed environment and return the output. Use this to compute, analyze data, or verify reasoning. When you have your final answer, output 'FINAL(answer)' on its own line.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Python code to execute"
                }
            },
            "required": ["code"]
        }),
        rlm_repl
    )
}

fn validate_python_code(code: &str) -> Result<()> {
    let forbidden = [
        "eval",
        "exec",
        "compile",
        "__import__",
        "open",
        "os",
        "sys",
        "subprocess",
        "socket",
        "urllib",
        "requests",
        "pathlib",
        "shutil",
    ];
    let normalized = code.to_lowercase();
    for pattern in &forbidden {
        if normalized.contains(pattern) {
            return Err(anyhow::anyhow!(
                "Code blocked by sandbox: contains forbidden keyword '{pattern}'"
            ));
        }
    }
    Ok(())
}

pub fn rlm_repl(args: Value) -> Result<String> {
    let args: RlmArgs = serde_json::from_value(args)?;
    let timeout_secs = 30;

    if args.code.trim().is_empty() {
        return Err(anyhow::anyhow!("Empty code"));
    }

    validate_python_code(&args.code)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let code = args.code.clone();
    std::thread::spawn(move || {
        let result = std::process::Command::new("python3")
            .arg("-c")
            .arg(&code)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        let _ = tx.send(result);
    });

    let output = match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
        Ok(result) => result.with_context(|| "Failed to execute Python code")?,
        Err(_) => {
            return Err(anyhow::anyhow!(
                "Python execution timed out after {timeout_secs} seconds"
            ))
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("stderr: ");
        result.push_str(&stderr);
    }
    if output.status.code() != Some(0) {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!(
            "[exit code: {}]",
            output.status.code().unwrap_or(-1)
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rlm_basic_math() {
        let result = rlm_repl(serde_json::json!({"code": "print(2 + 2)"})).unwrap();
        assert!(result.contains("4"));
    }

    #[test]
    fn rlm_empty_code_fails() {
        let result = rlm_repl(serde_json::json!({"code": ""}));
        assert!(result.is_err());
    }

    #[test]
    fn rlm_eval_blocked() {
        let result = rlm_repl(serde_json::json!({"code": "eval('1+1')"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("eval"));
    }

    #[test]
    fn rlm_import_blocked() {
        let result = rlm_repl(serde_json::json!({"code": "import os"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("os"));
    }

    #[test]
    fn rlm_subprocess_blocked() {
        let result = rlm_repl(serde_json::json!({"code": "import subprocess"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("subprocess"));
    }

    #[test]
    fn rlm_open_blocked() {
        let result = rlm_repl(serde_json::json!({"code": "open('/etc/passwd')"}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("open"));
    }
}
