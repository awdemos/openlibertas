use crate::config::SecretString;
use crate::domain::McpServerStatus;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Deserialize)]
pub struct McpServerConfig {
    #[serde(rename = "type")]
    pub server_type: String,
    pub command: Option<Vec<String>>,
    pub url: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Optional API key for remote MCP server authentication.
    #[serde(default)]
    pub api_key: Option<SecretString>,
    /// Optional additional headers for remote MCP requests.
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ToolContentRaw {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ToolContent {
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest<T> {
    jsonrpc: String,
    id: u64,
    method: String,
    params: T,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    #[serde(rename = "id")]
    _id: u64,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[serde(rename = "code")]
    _code: i32,
    message: String,
}

#[derive(Debug, Deserialize, Default)]
struct ToolsListResult {
    #[serde(default)]
    tools: Vec<McpTool>,
}

#[derive(Debug, Serialize)]
struct ToolCallParams {
    name: String,
    arguments: Value,
}

#[derive(Debug, Deserialize, Default)]
struct ToolCallResult {
    #[serde(default)]
    content: Vec<ToolContentRaw>,
    #[serde(default, rename = "isError")]
    is_error: bool,
}

#[derive(Debug, Clone)]
pub struct McpServerDiagnostics {
    pub status: McpServerStatus,
    pub last_error: Option<String>,
    pub tool_count: usize,
    pub server_type: String,
}

#[derive(Debug)]
pub struct McpClient {
    servers: HashMap<String, McpServerConfig>,
    tools: Mutex<HashMap<String, (String, McpTool)>>,
    processes: Mutex<HashMap<String, Child>>,
    server_statuses: Mutex<HashMap<String, McpServerStatus>>,
    server_errors: Mutex<HashMap<String, String>>,
}

impl McpClient {
    pub fn from_opencode_config() -> Result<Self> {
        let config_path = directories::BaseDirs::new()
            .map(|b| b.home_dir().join(".config/opencode/opencode.json"))
            .context("Could not determine home directory")?;
        Self::from_config_file(&config_path)
    }

    pub fn from_config_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {path:?}"))?;
        let config: Value = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse config from {path:?}"))?;

        let mcp_section = config
            .get("mcp")
            .cloned()
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));

        let servers: HashMap<String, McpServerConfig> =
            serde_json::from_value(mcp_section).unwrap_or_default();

        let mut statuses = HashMap::new();
        for (name, config) in &servers {
            statuses.insert(
                name.clone(),
                if config.enabled {
                    McpServerStatus::Pending
                } else {
                    McpServerStatus::Disabled
                },
            );
        }
        Ok(Self {
            servers,
            tools: Mutex::new(HashMap::new()),
            processes: Mutex::new(HashMap::new()),
            server_statuses: Mutex::new(statuses),
            server_errors: Mutex::new(HashMap::new()),
        })
    }

    pub async fn discover_tools(&self) -> Result<Vec<McpTool>> {
        let mut all_tools = Vec::new();
        let mut tool_map = HashMap::new();
        let mut statuses = self.server_statuses.lock().await;
        let mut errors = self.server_errors.lock().await;
        errors.clear();

        for (name, config) in &self.servers {
            if !config.enabled {
                statuses.insert(name.clone(), McpServerStatus::Disabled);
                continue;
            }

            statuses.insert(name.clone(), McpServerStatus::Connecting);

            match config.server_type.as_str() {
                "local" => match self.discover_local_tools(name, config).await {
                    Ok(tools) => {
                        statuses.insert(name.clone(), McpServerStatus::Connected);
                        for tool in tools {
                            tool_map.insert(tool.name.clone(), (name.clone(), tool.clone()));
                            all_tools.push(tool);
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("{e}");
                        tracing::error!("MCP server '{}' discovery failed: {}", name, err_msg);
                        errors.insert(name.clone(), err_msg);
                        statuses.insert(name.clone(), McpServerStatus::Failed);
                    }
                },
                "remote" => match self.discover_remote_tools(name, config).await {
                    Ok(tools) => {
                        statuses.insert(name.clone(), McpServerStatus::Connected);
                        for tool in tools {
                            tool_map.insert(tool.name.clone(), (name.clone(), tool.clone()));
                            all_tools.push(tool);
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("{e}");
                        tracing::error!("MCP server '{}' discovery failed: {}", name, err_msg);
                        errors.insert(name.clone(), err_msg);
                        statuses.insert(name.clone(), McpServerStatus::Failed);
                    }
                },
                _ => {
                    let err_msg = format!("Unknown server type: {}", config.server_type);
                    errors.insert(name.clone(), err_msg);
                    statuses.insert(name.clone(), McpServerStatus::Failed);
                }
            }
        }

        let mut tools = self.tools.lock().await;
        *tools = tool_map;

        Ok(all_tools)
    }

    async fn discover_local_tools(
        &self,
        name: &str,
        config: &McpServerConfig,
    ) -> Result<Vec<McpTool>> {
        let cmd = config
            .command
            .as_ref()
            .context("Local MCP server missing command")?;
        if cmd.is_empty() {
            return Err(anyhow::anyhow!("Empty command for MCP server {name}"));
        }

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("Failed to spawn MCP server {name}"))?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/list".to_string(),
            params: serde_json::json!({}),
        };

        let request_json = serde_json::to_string(&request)? + "\n";
        let mut stdin = stdin;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.flush().await?;

        let reader = BufReader::new(stdout);
        let mut lines = AsyncBufReadExt::lines(reader);

        let line_opt: Option<String> = lines.next_line().await?;
        if let Some(line) = line_opt {
            let response: JsonRpcResponse<ToolsListResult> = serde_json::from_str(&line)?;
            if let Some(result) = response.result {
                let mut processes = self.processes.lock().await;
                processes.insert(name.to_string(), child);
                return Ok(result.tools);
            } else if let Some(error) = response.error {
                return Err(anyhow::anyhow!("MCP error: {}", error.message));
            }
        }

        Err(anyhow::anyhow!("No response from MCP server {name}"))
    }

    async fn discover_remote_tools(
        &self,
        _name: &str,
        config: &McpServerConfig,
    ) -> Result<Vec<McpTool>> {
        let url = config
            .url
            .as_ref()
            .context("Remote MCP server missing URL")?;
        let client = reqwest::Client::new();
        let mut req = client.get(format!("{}/tools", url.trim_end_matches("/mcp")));
        if let Some(key) = &config.api_key {
            req = req.header("Authorization", format!("Bearer {}", key.expose_secret()));
        }
        for (k, v) in &config.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await;

        match resp {
            Ok(resp) => {
                if resp.status().is_success() {
                    let tools: Vec<McpTool> = resp.json().await?;
                    Ok(tools)
                } else {
                    Err(anyhow::anyhow!("HTTP {}", resp.status()))
                }
            }
            Err(e) => Err(anyhow::anyhow!("Failed to discover remote tools: {e}")),
        }
    }

    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<ToolResult> {
        let tools = self.tools.lock().await;
        let (server_name, _) = tools
            .get(tool_name)
            .context(format!("Tool {tool_name} not found"))?;
        let server_name = server_name.clone();
        drop(tools);

        let servers = &self.servers;
        let config = servers
            .get(&server_name)
            .context(format!("Server {server_name} not found"))?;

        match config.server_type.as_str() {
            "local" => {
                self.call_local_tool(&server_name, tool_name, arguments)
                    .await
            }
            "remote" => {
                self.call_remote_tool(&server_name, config, tool_name, arguments)
                    .await
            }
            _ => Err(anyhow::anyhow!("Unknown server type")),
        }
    }

    async fn call_local_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult> {
        let mut processes = self.processes.lock().await;
        let child = processes
            .get_mut(server_name)
            .context(format!("MCP server {server_name} not running"))?;

        let stdin = child.stdin.as_mut().context("Failed to get stdin")?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 2,
            method: "tools/call".to_string(),
            params: ToolCallParams {
                name: tool_name.to_string(),
                arguments,
            },
        };

        let request_json = serde_json::to_string(&request)? + "\n";
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.flush().await?;

        let stdout = child.stdout.as_mut().context("Failed to get stdout")?;
        let reader = BufReader::new(stdout);
        let mut lines = AsyncBufReadExt::lines(reader);

        let line_opt: Option<String> = lines.next_line().await?;
        if let Some(line) = line_opt {
            let response: JsonRpcResponse<ToolCallResult> = serde_json::from_str(&line)?;
            if let Some(result) = response.result {
                return Ok(ToolResult {
                    content: result
                        .content
                        .into_iter()
                        .map(|c| ToolContent {
                            content_type: "text".to_string(),
                            text: c.text,
                        })
                        .collect(),
                    is_error: result.is_error,
                });
            } else if let Some(error) = response.error {
                return Err(anyhow::anyhow!("MCP error: {}", error.message));
            }
        }

        Err(anyhow::anyhow!("No response from MCP server"))
    }

    async fn call_remote_tool(
        &self,
        _server_name: &str,
        config: &McpServerConfig,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult> {
        let url = config
            .url
            .as_ref()
            .context("Remote MCP server missing URL")?;
        let client = reqwest::Client::new();
        let mut req = client
            .post(format!("{}/call", url.trim_end_matches("/mcp")))
            .json(&serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            }));
        if let Some(key) = &config.api_key {
            req = req.header("Authorization", format!("Bearer {}", key.expose_secret()));
        }
        for (k, v) in &config.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await?;

        if resp.status().is_success() {
            let result: ToolCallResult = resp.json().await?;
            Ok(ToolResult {
                content: result
                    .content
                    .into_iter()
                    .map(|c| ToolContent {
                        content_type: "text".to_string(),
                        text: c.text,
                    })
                    .collect(),
                is_error: result.is_error,
            })
        } else {
            Err(anyhow::anyhow!("HTTP {}", resp.status()))
        }
    }

    pub async fn server_names(&self) -> Vec<String> {
        self.servers
            .iter()
            .filter(|(_, c)| c.enabled)
            .map(|(n, _)| n.clone())
            .collect()
    }

    pub async fn server_statuses(&self) -> HashMap<String, McpServerStatus> {
        self.server_statuses.lock().await.clone()
    }

    pub async fn server_errors(&self) -> HashMap<String, String> {
        self.server_errors.lock().await.clone()
    }

    pub async fn tool_server_map(&self) -> HashMap<String, String> {
        let tools = self.tools.lock().await;
        tools
            .iter()
            .map(|(name, (server, _))| (name.clone(), server.clone()))
            .collect()
    }

    pub async fn get_diagnostics(&self) -> HashMap<String, McpServerDiagnostics> {
        let statuses = self.server_statuses.lock().await;
        let errors = self.server_errors.lock().await;
        let tools = self.tools.lock().await;
        let mut diagnostics = HashMap::new();

        for (name, config) in &self.servers {
            let tool_count = tools.values().filter(|(server, _)| server == name).count();
            diagnostics.insert(
                name.clone(),
                McpServerDiagnostics {
                    status: statuses
                        .get(name)
                        .cloned()
                        .unwrap_or(McpServerStatus::Pending),
                    last_error: errors.get(name).cloned(),
                    tool_count,
                    server_type: config.server_type.clone(),
                },
            );
        }
        diagnostics
    }

    pub async fn health_check(&self) -> HashMap<String, bool> {
        let mut processes = self.processes.lock().await;
        let mut statuses = self.server_statuses.lock().await;
        let mut results = HashMap::new();

        for (name, child) in processes.iter_mut() {
            match child.try_wait() {
                Ok(None) => {
                    results.insert(name.clone(), true);
                }
                Ok(Some(exit)) => {
                    results.insert(name.clone(), false);
                    statuses.insert(name.clone(), McpServerStatus::Failed);
                    tracing::warn!("MCP server '{}' exited with status: {:?}", name, exit);
                }
                Err(e) => {
                    results.insert(name.clone(), false);
                    tracing::warn!("MCP server '{}' health check error: {}", name, e);
                }
            }
        }
        results
    }

    pub async fn test_tool(&self, tool_name: &str) -> Result<String> {
        let tools = self.tools.lock().await;
        let (server_name, tool) = tools
            .get(tool_name)
            .context(format!("Tool {tool_name} not found"))?;
        let server_name = server_name.clone();
        let tool = tool.clone();
        drop(tools);

        let test_args = match tool.input_schema.get("properties") {
            Some(props) => {
                let mut args = serde_json::Map::new();
                if let Some(obj) = props.as_object() {
                    for (key, schema) in obj {
                        let default_value = schema.get("default").cloned().unwrap_or_else(|| {
                            if let Some(type_str) = schema.get("type").and_then(|v| v.as_str()) {
                                match type_str {
                                    "string" => serde_json::Value::String("test".to_string()),
                                    "number" => serde_json::json!(0),
                                    "integer" => serde_json::json!(0),
                                    "boolean" => serde_json::json!(false),
                                    "array" => serde_json::json!([]),
                                    "object" => serde_json::json!({}),
                                    _ => serde_json::Value::Null,
                                }
                            } else {
                                serde_json::Value::Null
                            }
                        });
                        args.insert(key.clone(), default_value);
                    }
                }
                serde_json::Value::Object(args)
            }
            None => serde_json::json!({}),
        };

        match self.call_tool(tool_name, test_args).await {
            Ok(result) => {
                let content_text: Vec<String> =
                    result.content.iter().map(|c| c.text.clone()).collect();
                Ok(format!(
                    "✓ Tool '{}' responded (server: {})\n{}",
                    tool_name,
                    server_name,
                    content_text.join("\n")
                ))
            }
            Err(e) => Err(anyhow::anyhow!(
                "✗ Tool '{tool_name}' test failed (server: {server_name}): {e}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_server_config_deserialization() {
        let json = r#"{
            "type": "local",
            "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem"],
            "enabled": true
        }"#;
        let config: McpServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.server_type, "local");
        assert_eq!(
            config.command,
            Some(vec![
                "npx".to_string(),
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string()
            ])
        );
        assert!(config.enabled);
        assert!(config.url.is_none());
    }

    #[test]
    fn mcp_server_config_remote_deserialization() {
        let json = r#"{
            "type": "remote",
            "url": "http://localhost:3000/mcp",
            "enabled": true
        }"#;
        let config: McpServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.server_type, "remote");
        assert_eq!(config.url, Some("http://localhost:3000/mcp".to_string()));
        assert!(config.command.is_none());
    }

    #[test]
    fn mcp_server_config_enabled_defaults_to_true() {
        let json = r#"{"type": "local", "command": ["test"]}"#;
        let config: McpServerConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
    }

    #[test]
    fn mcp_tool_serialization_roundtrip() {
        let tool = McpTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: McpTool = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test_tool");
        assert_eq!(deserialized.description, "A test tool");
    }

    #[test]
    fn mcp_tool_deserialization_with_input_schema() {
        let json = r#"{
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }
        }"#;
        let tool: McpTool = serde_json::from_str(json).unwrap();
        assert_eq!(tool.name, "read_file");
        assert!(tool.input_schema.is_object());
    }

    #[test]
    fn tool_result_creation() {
        let result = ToolResult {
            content: vec![ToolContent {
                content_type: "text".to_string(),
                text: "result text".to_string(),
            }],
            is_error: false,
        };
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].text, "result text");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn mcp_client_from_config_file() {
        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("test_mcp_config.json");
        let config_json = r#"{
            "mcp": {
                "filesystem": {
                    "type": "local",
                    "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem"],
                    "enabled": true
                },
                "disabled_server": {
                    "type": "local",
                    "command": ["test"],
                    "enabled": false
                }
            }
        }"#;
        std::fs::write(&config_path, config_json).unwrap();

        let client = McpClient::from_config_file(&config_path).unwrap();
        let names = client.server_names().await;
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "filesystem");

        std::fs::remove_file(&config_path).unwrap();
    }

    #[tokio::test]
    async fn mcp_client_server_names_filters_disabled() {
        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("test_mcp_filter.json");
        let config_json = r#"{
            "mcp": {
                "enabled_server": {
                    "type": "local",
                    "command": ["test"],
                    "enabled": true
                },
                "disabled_server": {
                    "type": "local",
                    "command": ["test"],
                    "enabled": false
                }
            }
        }"#;
        std::fs::write(&config_path, config_json).unwrap();

        let client = McpClient::from_config_file(&config_path).unwrap();
        let names = client.server_names().await;
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "enabled_server");

        std::fs::remove_file(&config_path).unwrap();
    }
}
