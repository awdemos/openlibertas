use crate::domain::ToolDefinition;
use crate::mcp::{McpClient, McpTool};
use crate::tool_format::ToolFormat;
use crate::tools;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct ToolRegistry {
    client: Option<Arc<McpClient>>,
    available_tools: Vec<McpTool>,
    builtin_tools: Vec<tools::BuiltinTool>,
    server_statuses: HashMap<String, crate::domain::McpServerStatus>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            client: None,
            available_tools: Vec::new(),
            builtin_tools: tools::builtin_tools(),
            server_statuses: HashMap::new(),
        }
    }
}

impl ToolRegistry {
    pub fn with_client(mut self, client: McpClient) -> Self {
        self.client = Some(Arc::new(client));
        self
    }

    pub fn client(&self) -> Option<Arc<McpClient>> {
        self.client.clone()
    }

    pub fn set_client(&mut self, client: Option<Arc<McpClient>>) {
        self.client = client;
    }

    pub fn available_tools(&self) -> &[McpTool] {
        &self.available_tools
    }

    pub fn set_available_tools(&mut self, tools: Vec<McpTool>) {
        self.available_tools = tools;
    }

    pub fn builtin_tools(&self) -> &[tools::BuiltinTool] {
        &self.builtin_tools
    }

    pub fn server_statuses(&self) -> &HashMap<String, crate::domain::McpServerStatus> {
        &self.server_statuses
    }

    pub fn set_server_statuses(
        &mut self,
        statuses: HashMap<String, crate::domain::McpServerStatus>,
    ) {
        self.server_statuses = statuses;
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.available_tools.iter().any(|t| t.name == name)
            || self.builtin_tools.iter().any(|t| t.name == name)
    }

    pub fn definitions_for_api(&self, plan_mode: bool) -> Option<Vec<ToolDefinition>> {
        let mut all_tools = Vec::new();
        if !plan_mode {
            for tool in &self.available_tools {
                all_tools.push(ToolDefinition {
                    tool_type: "function".to_string(),
                    function: crate::domain::FunctionDefinition {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.input_schema.clone(),
                    },
                });
            }
        }
        let read_only_builtins: &[&str] = &[
            "read_file",
            "glob",
            "grep",
            "web_search",
            "fetch_url",
            "think",
            "git",
        ];
        for tool in &self.builtin_tools {
            if plan_mode && !read_only_builtins.contains(&tool.name.as_str()) {
                continue;
            }
            all_tools.push(tool.to_tool_definition());
        }
        if all_tools.is_empty() {
            None
        } else {
            Some(all_tools)
        }
    }

    pub fn tool_instructions(&self, tool_format: ToolFormat) -> Option<String> {
        let mut tool_lines = Vec::new();
        for tool in &self.builtin_tools {
            let params = serde_json::to_string(&tool.parameters).unwrap_or_default();
            tool_lines.push(format!(
                "  - {}: {}\n    Parameters: {}",
                tool.name, tool.description, params
            ));
        }
        for tool in &self.available_tools {
            let schema = serde_json::to_string(&tool.input_schema).unwrap_or_default();
            tool_lines.push(format!(
                "  - {}: {}\n    Parameters: {}",
                tool.name, tool.description, schema
            ));
        }
        if tool_lines.is_empty() {
            None
        } else {
            let mut instructions = format!(
                "You have access to the following tools. When a tool can help answer the user's question, you MUST invoke it by producing a tool_calls array containing the function name and arguments.\n\n\
                Available tools:\n{}\n\n\
                RULES:\n\
                1. Output ONLY the tool_calls JSON array. Do not add explanatory text before or after.\n\
                2. Each tool call must have this exact structure:\n\
                   {{\"id\": \"call_1\", \"type\": \"function\", \"function\": {{\"name\": \"tool_name\", \"arguments\": \"{{\\\"param\\\": \\\"value\\\"}}\"}}}}\n\
                3. The 'arguments' field must be a JSON-encoded string, not a raw object.\n\
                4. Do not describe what the tool does. Do not write example code. Do not say 'I will use'.\n\
                5. Call the tool directly — results will be provided to you automatically.",
                tool_lines.join("\n")
            );
            instructions.push_str(tool_format.tool_instructions_suffix());
            Some(instructions)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_starts_empty() {
        let registry = ToolRegistry::default();
        assert!(registry.available_tools.is_empty());
    }

    #[test]
    fn definitions_for_api_includes_builtins() {
        let registry = ToolRegistry::default();
        let result = registry.definitions_for_api(false).unwrap();
        assert!(!result.is_empty(), "builtin tools should be present");
    }

    #[test]
    fn definitions_for_api_converts_available_tools() {
        let mut registry = ToolRegistry::default();
        let builtin_count = registry.definitions_for_api(false).unwrap().len();
        registry.available_tools.push(crate::mcp::McpTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({}),
        });
        let result = registry.definitions_for_api(false).unwrap();
        assert_eq!(result.len(), builtin_count + 1);
        assert!(result.iter().any(|t| t.function.name == "test_tool"));
    }

    #[test]
    fn tool_instructions_native_returns_some() {
        let registry = ToolRegistry::default();
        assert!(registry.tool_instructions(ToolFormat::Native).is_some());
    }

    #[test]
    fn tool_instructions_content_json_appends_suffix() {
        let registry = ToolRegistry::default();
        let instructions = registry.tool_instructions(ToolFormat::ContentJson).unwrap();
        assert!(instructions.contains("JSON object or array"));
    }

    #[test]
    fn tool_instructions_xml_appends_suffix() {
        let registry = ToolRegistry::default();
        let instructions = registry.tool_instructions(ToolFormat::Xml).unwrap();
        assert!(instructions.contains("<tool_call>"));
    }

    #[test]
    fn tool_instructions_none_same_as_native_suffix() {
        let registry = ToolRegistry::default();
        let native = registry.tool_instructions(ToolFormat::Native).unwrap();
        let none = registry.tool_instructions(ToolFormat::None).unwrap();
        assert_eq!(native, none);
    }
}
