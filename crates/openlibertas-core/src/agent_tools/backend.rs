use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

use crate::domain::ToolDefinition;
use crate::mcp::McpClient;

#[derive(thiserror::Error, Debug, Clone)]
pub enum ToolBackendError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("approval required")]
    ApprovalRequired,
}

#[async_trait]
pub trait ToolBackend: Send + Sync {
    fn can_execute(&self, tool_name: &str) -> bool;
    async fn execute(&self, tool_name: &str, args: Value) -> Result<String, ToolBackendError>;
    fn tool_definitions(&self) -> Vec<ToolDefinition>;
    fn mcp_client(&self) -> Option<Arc<McpClient>> {
        None
    }
    fn as_any(&self) -> &dyn std::any::Any;
}

#[derive(Debug, Clone)]
pub struct ApprovalPolicy {
    pub yolo_mode: bool,
    pub destructive_tools: HashSet<String>,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        let mut destructive_tools = HashSet::new();
        for tool in [
            "edit_file",
            "editfile",
            "editFile",
            "delete_file",
            "deletefile",
            "deleteFile",
            "remove_file",
            "patch",
            "apply_patch",
            "applyPatch",
            "str_replace_file",
            "strreplacefile",
            "strReplaceFile",
            "shell",
            "exec",
            "execute",
        ] {
            destructive_tools.insert(tool.to_string());
        }
        Self {
            yolo_mode: false,
            destructive_tools,
        }
    }
}

impl ApprovalPolicy {
    pub fn new(yolo_mode: bool) -> Self {
        Self {
            yolo_mode,
            ..Default::default()
        }
    }

    pub fn needs_approval(&self, tool_name: &str, is_destructive: bool) -> bool {
        !self.yolo_mode
            && (is_destructive
                || self
                    .destructive_tools
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(tool_name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_policy_allows_in_yolo() {
        let policy = ApprovalPolicy::new(true);
        assert!(!policy.needs_approval("shell", true));
        assert!(!policy.needs_approval("read_file", false));
    }

    #[test]
    fn approval_policy_blocks_destructive() {
        let policy = ApprovalPolicy::new(false);
        assert!(policy.needs_approval("shell", true));
        assert!(policy.needs_approval("str_replace_file", true));
        assert!(policy.needs_approval("editFile", false));
    }

    #[test]
    fn approval_policy_allows_safe() {
        let policy = ApprovalPolicy::new(false);
        assert!(!policy.needs_approval("read_file", false));
        assert!(!policy.needs_approval("glob", false));
    }

    #[test]
    fn approval_policy_case_insensitive() {
        let policy = ApprovalPolicy::new(false);
        assert!(policy.needs_approval("EXECUTE", false));
        assert!(policy.needs_approval("Delete_File", false));
    }
}
