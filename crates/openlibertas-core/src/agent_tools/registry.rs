use crate::agent_tools;
use crate::agent_tools::backend::ToolBackend;
use crate::domain::ToolDefinition;
use crate::mcp::{McpClient, McpServerDiagnostics, McpTool};
use crate::tool_format::ToolFormat;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistry {
    backends: Vec<Box<dyn ToolBackend>>,
    server_statuses: HashMap<String, crate::domain::McpServerStatus>,
    diagnostics: HashMap<String, McpServerDiagnostics>,
    tool_server_map: HashMap<String, String>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("backends", &self.backends.len())
            .field("server_statuses", &self.server_statuses)
            .field("diagnostics", &self.diagnostics)
            .field("tool_server_map", &self.tool_server_map)
            .finish()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            backends: vec![Box::new(agent_tools::builtin_backend::BuiltinBackend::new())],
            server_statuses: HashMap::new(),
            diagnostics: HashMap::new(),
            tool_server_map: HashMap::new(),
        }
    }
}

impl ToolRegistry {
    pub fn with_client(mut self, client: McpClient) -> Self {
        self.add_backend(Box::new(agent_tools::mcp_backend::McpBackend::new(
            Arc::new(client),
        )));
        self
    }

    pub fn client(&self) -> Option<Arc<McpClient>> {
        for backend in &self.backends {
            if let Some(client) = backend.mcp_client() {
                return Some(client);
            }
        }
        None
    }

    pub fn set_client(&mut self, client: Option<Arc<McpClient>>) {
        self.backends.retain(|b| b.mcp_client().is_none());
        if let Some(client) = client {
            self.add_backend(Box::new(agent_tools::mcp_backend::McpBackend::new(client)));
        }
    }

    pub fn add_backend(&mut self, backend: Box<dyn ToolBackend>) {
        self.backends.push(backend);
    }

    pub fn available_tools(&self) -> Vec<McpTool> {
        for backend in &self.backends {
            if let Some(mcp) = backend
                .as_any()
                .downcast_ref::<agent_tools::mcp_backend::McpBackend>()
            {
                return mcp.available_tools();
            }
        }
        Vec::new()
    }

    pub fn set_available_tools(&self, tools: Vec<McpTool>) {
        for backend in &self.backends {
            if let Some(mcp) = backend
                .as_any()
                .downcast_ref::<agent_tools::mcp_backend::McpBackend>()
            {
                mcp.set_available_tools(tools);
                return;
            }
        }
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

    pub fn diagnostics(&self) -> &HashMap<String, McpServerDiagnostics> {
        &self.diagnostics
    }

    pub fn set_diagnostics(&mut self, diagnostics: HashMap<String, McpServerDiagnostics>) {
        self.diagnostics = diagnostics;
    }

    pub fn tool_server_map(&self) -> &HashMap<String, String> {
        &self.tool_server_map
    }

    pub fn set_tool_server_map(&mut self, map: HashMap<String, String>) {
        self.tool_server_map = map;
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.backends.iter().any(|b| b.can_execute(name))
    }

    pub fn definitions_for_api(&self, plan_mode: bool) -> Option<Vec<ToolDefinition>> {
        let mut all_tools = Vec::new();
        for backend in &self.backends {
            if plan_mode && backend.mcp_client().is_some() {
                continue;
            }
            let defs = backend.tool_definitions();
            if plan_mode && backend.mcp_client().is_none() {
                let read_only_builtins: &[&str] = &[
                    "read_file",
                    "glob",
                    "grep",
                    "web_search",
                    "fetch_url",
                    "think",
                    "git",
                    "rlm_repl",
                ];
                all_tools.extend(
                    defs.into_iter()
                        .filter(|d| read_only_builtins.contains(&d.function.name.as_str())),
                );
            } else {
                all_tools.extend(defs);
            }
        }
        if all_tools.is_empty() {
            None
        } else {
            Some(all_tools)
        }
    }

    pub fn tool_instructions(&self, tool_format: ToolFormat) -> Option<String> {
        let mut tool_lines = Vec::new();
        for backend in &self.backends {
            for tool in backend.tool_definitions() {
                let params = serde_json::to_string(&tool.function.parameters).unwrap_or_default();
                tool_lines.push(format!(
                    "  - {}: {}\n    Parameters: {}",
                    tool.function.name, tool.function.description, params
                ));
            }
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
        assert!(registry.available_tools().is_empty());
    }

    #[test]
    fn definitions_for_api_includes_builtins() {
        let registry = ToolRegistry::default();
        let result = registry.definitions_for_api(false).unwrap();
        assert!(!result.is_empty(), "builtin tools should be present");
    }

    #[test]
    fn definitions_for_api_converts_available_tools() {
        let tmp = std::env::temp_dir().join("test_registry_api.json");
        std::fs::write(&tmp, r#"{"mcp": {}}"#).unwrap();
        let client = crate::mcp::McpClient::from_config_file(&tmp).unwrap();
        std::fs::remove_file(&tmp).unwrap();

        let registry = ToolRegistry::with_client(ToolRegistry::default(), client);
        let builtin_count = registry.definitions_for_api(false).unwrap().len();
        registry.set_available_tools(vec![crate::mcp::McpTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({}),
        }]);
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
