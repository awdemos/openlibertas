use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::{
    JsonRpcRequest, JsonRpcResponse, McpTool, ToolCallParams, ToolCallResult, ToolContent,
    ToolResult, ToolsListResult,
};
use crate::mcp::transport::{McpServerType, McpTransport, McpTransportError};

pub struct LocalTransport {
    command: Vec<String>,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<tokio::process::ChildStdin>>,
    stdout: Mutex<Option<tokio::process::ChildStdout>>,
}

impl LocalTransport {
    pub fn new(command: Vec<String>) -> Self {
        Self {
            command,
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            stdout: Mutex::new(None),
        }
    }
}

#[async_trait]
impl McpTransport for LocalTransport {
    async fn discover_tools(&self) -> Result<Vec<McpTool>, McpTransportError> {
        if self.command.is_empty() {
            return Err(McpTransportError::RequestFailed(
                "Empty command for MCP server".to_string(),
            ));
        }

        let mut child = Command::new(&self.command[0])
            .args(&self.command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                McpTransportError::RequestFailed(format!("Failed to spawn MCP server: {e}"))
            })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            McpTransportError::NotConnected("Failed to get stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpTransportError::NotConnected("Failed to get stdout".to_string())
        })?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/list".to_string(),
            params: serde_json::json!({}),
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?
            + "\n";
        let mut stdin_writer = stdin;
        stdin_writer
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
        stdin_writer
            .flush()
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;

        let reader = BufReader::new(stdout);
        let mut lines = AsyncBufReadExt::lines(reader);

        let line_opt: Option<String> = lines
            .next_line()
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
        if let Some(line) = line_opt {
            let response: JsonRpcResponse<ToolsListResult> = serde_json::from_str(&line)
                .map_err(|e| McpTransportError::InvalidResponse(e.to_string()))?;
            if let Some(result) = response.result {
                let mut child_lock = self.child.lock().await;
                let mut stdin_lock = self.stdin.lock().await;
                let mut stdout_lock = self.stdout.lock().await;
                *child_lock = Some(child);
                *stdin_lock = Some(stdin_writer);
                *stdout_lock = Some(lines.into_inner().into_inner());
                return Ok(result.tools);
            } else if let Some(error) = response.error {
                return Err(McpTransportError::RequestFailed(format!(
                    "MCP error: {}",
                    error.message
                )));
            }
        }

        Err(McpTransportError::InvalidResponse(
            "No response from MCP server".to_string(),
        ))
    }

    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolResult, McpTransportError> {
        let mut stdin_lock = self.stdin.lock().await;
        let mut stdout_lock = self.stdout.lock().await;
        let stdin = stdin_lock.as_mut().ok_or_else(|| {
            McpTransportError::NotConnected("MCP server not running".to_string())
        })?;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 2,
            method: "tools/call".to_string(),
            params: ToolCallParams {
                name: tool_name.to_string(),
                arguments,
            },
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?
            + "\n";
        stdin
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;

        let stdout = stdout_lock.as_mut().ok_or_else(|| {
            McpTransportError::NotConnected("Failed to get stdout".to_string())
        })?;
        let reader = BufReader::new(stdout);
        let mut lines = AsyncBufReadExt::lines(reader);

        let line_opt: Option<String> = lines
            .next_line()
            .await
            .map_err(|e| McpTransportError::RequestFailed(e.to_string()))?;
        if let Some(line) = line_opt {
            let response: JsonRpcResponse<ToolCallResult> = serde_json::from_str(&line)
                .map_err(|e| McpTransportError::InvalidResponse(e.to_string()))?;
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
                return Err(McpTransportError::RequestFailed(format!(
                    "MCP error: {}",
                    error.message
                )));
            }
        }

        Err(McpTransportError::InvalidResponse(
            "No response from MCP server".to_string(),
        ))
    }

    async fn health_check(&self) -> Result<bool, McpTransportError> {
        let mut child_lock = self.child.lock().await;
        if let Some(child) = child_lock.as_mut() {
            match child.try_wait() {
                Ok(None) => Ok(true),
                Ok(Some(_)) => Ok(false),
                Err(e) => Err(McpTransportError::RequestFailed(e.to_string())),
            }
        } else {
            Ok(false)
        }
    }

    fn server_type(&self) -> McpServerType {
        McpServerType::Local {
            command: self.command.first().cloned().unwrap_or_default(),
            args: self.command.iter().skip(1).cloned().collect(),
        }
    }
}
