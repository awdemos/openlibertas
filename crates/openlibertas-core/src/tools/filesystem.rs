use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::BuiltinTool;

/// Verify that `path` stays within the current working directory.
/// Rejects absolute paths and paths that traverse above the working directory.
fn verify_sandbox(path: &Path) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to get working directory")?;
    let resolved = if path.is_absolute() {
        return Err(anyhow::anyhow!(
            "Absolute paths are not allowed: {}. Use a relative path.",
            path.display()
        ));
    } else {
        cwd.join(path)
    };
    let canonical = resolved.canonicalize().unwrap_or(resolved.clone());
    let canonical_cwd = cwd.canonicalize().unwrap_or(cwd);
    if !canonical.starts_with(&canonical_cwd) {
        return Err(anyhow::anyhow!(
            "Path escapes working directory: {}",
            path.display()
        ));
    }
    Ok(resolved)
}

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
    #[serde(default)]
    append: bool,
}

pub fn read_file_tool() -> BuiltinTool {
    crate::define_tool!(
        "read_file",
        "Read the contents of a file. Optionally specify offset and limit for partial reads.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line offset to start reading from (0-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read"
                }
            },
            "required": ["path"]
        }),
        read_file
    )
}

pub fn write_file_tool() -> BuiltinTool {
    crate::define_tool!(
        "write_file",
        "Write content to a file. Can optionally append instead of overwrite.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "append": {
                    "type": "boolean",
                    "description": "If true, append to file instead of overwriting"
                }
            },
            "required": ["path", "content"]
        }),
        write_file
    )
}

pub fn read_file(args: Value) -> Result<String> {
    let args: ReadFileArgs = serde_json::from_value(args)?;
    let path = verify_sandbox(Path::new(&args.path))?;

    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", args.path));
    }

    if !path.is_file() {
        return Err(anyhow::anyhow!("Not a file: {}", args.path));
    }

    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read file: {}", args.path))?;

    let lines: Vec<&str> = content.lines().collect();

    let offset = args.offset.unwrap_or(0);
    let limit = args.limit.unwrap_or(lines.len());

    if offset >= lines.len() {
        return Ok(String::new());
    }

    let end = (offset + limit).min(lines.len());
    let selected = &lines[offset..end];

    Ok(selected.join("\n"))
}

pub fn write_file(args: Value) -> Result<String> {
    let args: WriteFileArgs = serde_json::from_value(args)?;
    let path = verify_sandbox(Path::new(&args.path))?;

    if args.append {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open file for append: {}", args.path))?
            .write_all(args.content.as_bytes())
            .with_context(|| format!("Failed to append to file: {}", args.path))?;
    } else {
        fs::write(&path, &args.content)
            .with_context(|| format!("Failed to write file: {}", args.path))?;
    }

    Ok(format!("Successfully wrote to {}", args.path))
}

#[derive(Debug, Deserialize)]
struct StrReplaceFileArgs {
    path: String,
    old_str: String,
    new_str: String,
}

pub fn str_replace_file_tool() -> BuiltinTool {
    crate::define_tool!(
        "str_replace_file",
        "Replace a string in a file. The old_str must match exactly (including whitespace).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file"
                },
                "old_str": {
                    "type": "string",
                    "description": "Exact string to find and replace"
                },
                "new_str": {
                    "type": "string",
                    "description": "Replacement string"
                }
            },
            "required": ["path", "old_str", "new_str"]
        }),
        str_replace_file
    )
}

pub fn str_replace_file(args: Value) -> Result<String> {
    let args: StrReplaceFileArgs = serde_json::from_value(args)?;
    let path = verify_sandbox(Path::new(&args.path))?;

    if !path.exists() {
        return Err(anyhow::anyhow!("File not found: {}", args.path));
    }

    let content =
        fs::read_to_string(&path).with_context(|| format!("Failed to read file: {}", args.path))?;

    if !content.contains(&args.old_str) {
        return Err(anyhow::anyhow!(
            "old_str not found in file. The string must match exactly (including whitespace)."
        ));
    }

    let count = content.matches(&args.old_str).count();
    if count > 1 {
        return Err(anyhow::anyhow!(
            "old_str appears {} times in the file. Must be unique for replacement.",
            count
        ));
    }

    let new_content = content.replace(&args.old_str, &args.new_str);
    fs::write(path, new_content).with_context(|| format!("Failed to write file: {}", args.path))?;

    Ok(format!("Successfully replaced text in {}", args.path))
}

use std::io::Write;

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn read_file_reads_content() {
        let temp_path = "test_read_file_tmp.txt";
        std::fs::write(temp_path, "hello world").unwrap();
        let result = read_file(serde_json::json!({"path": temp_path}));
        assert!(result.is_ok());
        assert!(result.unwrap().contains("hello world"));
        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn read_file_missing_fails() {
        let result = read_file(serde_json::json!({"path": "/nonexistent/file.txt"}));
        assert!(result.is_err());
    }
}
