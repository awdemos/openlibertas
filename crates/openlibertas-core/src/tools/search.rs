use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

use crate::tools::BuiltinTool;

#[derive(Debug, Deserialize)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
}

pub fn glob_tool() -> BuiltinTool {
    BuiltinTool {
        name: "glob".to_string(),
        description: "Find files matching a glob pattern.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g., '*.rs', 'src/**/*.toml')"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory to search in (default: current directory)"
                }
            },
            "required": ["pattern"]
        }),
        handler: glob,
    }
}

pub fn grep_tool() -> BuiltinTool {
    BuiltinTool {
        name: "grep".to_string(),
        description: "Search file contents for a regex pattern.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in"
                },
                "glob": {
                    "type": "string",
                    "description": "Optional glob filter for files (e.g., '*.rs')"
                }
            },
            "required": ["pattern"]
        }),
        handler: grep,
    }
}

pub fn glob(args: Value) -> Result<String> {
    let args: GlobArgs = serde_json::from_value(args)?;
    let base = args.path.as_deref().unwrap_or(".");
    let pattern = format!("{}/{}", base, args.pattern);

    let entries =
        glob::glob(&pattern).with_context(|| format!("Invalid glob pattern: {}", args.pattern))?;

    let mut results = Vec::new();
    for entry in entries {
        match entry {
            Ok(path) => results.push(path.to_string_lossy().to_string()),
            Err(e) => eprintln!("Glob error: {}", e),
        }
    }

    if results.is_empty() {
        Ok("No files found.".to_string())
    } else {
        Ok(results.join("\n"))
    }
}

pub fn grep(args: Value) -> Result<String> {
    let args: GrepArgs = serde_json::from_value(args)?;
    let pattern = args.pattern;
    let base = args.path.as_deref().unwrap_or(".");

    let regex = regex::Regex::new(&pattern)
        .with_context(|| format!("Invalid regex pattern: {}", pattern))?;

    let mut results = Vec::new();
    let base_path = Path::new(base);

    if base_path.is_file() {
        search_file(base_path, &regex, &mut results)?;
    } else if base_path.is_dir() {
        let glob_pattern = args.glob.as_deref().unwrap_or("*");
        let pattern_str = format!("{}/{}", base, glob_pattern);

        let glob_iter = match glob::glob(&pattern_str) {
            Ok(iter) => iter,
            Err(_) => match glob::glob("*") {
                Ok(iter) => iter,
                Err(_) => return Ok("No matches found.".to_string()),
            },
        };
        for path in glob_iter.flatten() {
            if path.is_file() {
                if let Err(e) = search_file(&path, &regex, &mut results) {
                    eprintln!("Error searching {}: {}", path.display(), e);
                }
            }
        }
    } else {
        return Err(anyhow::anyhow!("Path not found: {}", base));
    }

    if results.is_empty() {
        Ok("No matches found.".to_string())
    } else {
        Ok(results.join("\n"))
    }
}

fn search_file(path: &Path, regex: &regex::Regex, results: &mut Vec<String>) -> Result<()> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;

    for (line_num, line) in content.lines().enumerate() {
        if regex.is_match(line) {
            results.push(format!("{}:{}: {}", path.display(), line_num + 1, line));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_finds_files() {
        let result = glob(serde_json::json!({"pattern": "*.rs", "path": "/tmp"}));
        assert!(result.is_ok());
    }

    #[test]
    fn grep_finds_pattern() {
        let result = grep(serde_json::json!({"pattern": "test", "path": "/tmp"}));
        assert!(result.is_ok());
    }
}
