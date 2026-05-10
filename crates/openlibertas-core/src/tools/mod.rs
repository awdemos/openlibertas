use crate::domain::{FunctionDefinition, ToolDefinition};
use anyhow::Result;
use serde_json::Value;

pub mod agent;
pub mod filesystem;
pub mod git;
pub mod meta;
pub mod search;
pub mod shell;
pub mod tmux;
pub mod web;

/// A built-in tool that can be executed directly without MCP.
#[derive(Debug, Clone)]
pub struct BuiltinTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub handler: fn(Value) -> Result<String>,
}

impl BuiltinTool {
    pub fn to_tool_definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: self.name.clone(),
                description: self.description.clone(),
                parameters: self.parameters.clone(),
            },
        }
    }
}

/// Helper macro to reduce boilerplate in tool definitions.
#[macro_export]
macro_rules! define_tool {
    ($name:expr, $description:expr, $parameters:expr, $handler:expr) => {
        $crate::tools::BuiltinTool {
            name: $name.to_string(),
            description: $description.to_string(),
            parameters: $parameters,
            handler: $handler,
        }
    };
}

/// Execute a built-in tool by name with the given arguments.
pub fn execute_builtin(name: &str, args: Value) -> Result<String> {
    match name {
        "shell" => shell::run(args),
        "read_file" => filesystem::read_file(args),
        "write_file" => filesystem::write_file(args),
        "str_replace_file" => filesystem::str_replace_file(args),
        "glob" => search::glob(args),
        "grep" => search::grep(args),
        "web_search" => web::web_search(args),
        "fetch_url" => web::fetch_url(args),
        "think" => meta::think(args),
        "git" => git::run(args),
        "tmux" => tmux::run(args),
        "switch_persona" => agent::switch_persona(args),
        "spawn_subagent" => agent::spawn_subagent(args),
        _ => Err(anyhow::anyhow!("Unknown built-in tool: {}", name)),
    }
}

/// Get all built-in tool definitions.
pub fn builtin_tools() -> Vec<BuiltinTool> {
    vec![
        agent::switch_persona_tool(),
        agent::spawn_subagent_tool(),
        shell::tool_definition(),
        filesystem::read_file_tool(),
        filesystem::write_file_tool(),
        filesystem::str_replace_file_tool(),
        search::glob_tool(),
        search::grep_tool(),
        web::web_search_tool(),
        web::fetch_url_tool(),
        meta::think_tool(),
        git::tool_definition(),
        tmux::tool_definition(),
    ]
}

/// Check if a tool name is a built-in tool.
pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "switch_persona"
            | "spawn_subagent"
            | "shell"
            | "read_file"
            | "write_file"
            | "str_replace_file"
            | "glob"
            | "grep"
            | "web_search"
            | "fetch_url"
            | "think"
            | "git"
            | "tmux"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tools_list_not_empty() {
        let tools = builtin_tools();
        assert!(!tools.is_empty());
    }

    #[test]
    fn is_builtin_recognizes_known_tools() {
        assert!(is_builtin("shell"));
        assert!(is_builtin("read_file"));
        assert!(!is_builtin("unknown"));
    }

    #[test]
    fn execute_unknown_builtin_fails() {
        let result = execute_builtin("unknown", serde_json::json!({}));
        assert!(result.is_err());
    }
}
