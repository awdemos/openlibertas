use crate::config::PermissionPolicy;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

/// A request for user permission to execute a dangerous tool operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub session_id: String,
    pub tool_name: String,
    pub description: String,
    pub action: String,
    /// Tool-specific parameters for UI preview (e.g., command for bash, diff for write).
    pub params: serde_json::Value,
    /// Working directory or file path relevant to the operation.
    pub path: String,
}

impl PermissionRequest {
    pub fn new(
        session_id: impl Into<String>,
        tool_name: impl Into<String>,
        description: impl Into<String>,
        action: impl Into<String>,
        params: serde_json::Value,
        path: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            tool_name: tool_name.into(),
            description: description.into(),
            action: action.into(),
            params,
            path: path.into(),
        }
    }
}

/// Response from the user for a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionResponse {
    /// Allow this operation once.
    Allow,
    /// Allow this operation and remember for the session.
    AllowForSession,
    /// Deny this operation.
    Deny,
}

/// Internal message for the permission service channel.
#[derive(Debug)]
enum PermissionMessage {
    SetNotifier {
        tx: mpsc::UnboundedSender<PermissionRequest>,
    },
    Request {
        request: PermissionRequest,
        respond_to: oneshot::Sender<bool>,
    },
    Respond {
        request_id: String,
        response: PermissionResponse,
    },
    AutoApproveSession {
        session_id: String,
    },
    GrantSessionPermission {
        session_id: String,
        tool_name: String,
        action: String,
    },
}

/// Service for managing dangerous tool permissions.
///
/// Similar to opencode's permission system, but using tokio channels instead of pubsub.
/// Tools call `request()` which blocks (async) until the user responds via the UI.
#[derive(Clone)]
pub struct PermissionService {
    tx: Option<mpsc::UnboundedSender<PermissionMessage>>,
    auto_approve_tools: Arc<RwLock<Vec<String>>>,
    permission_policy: Arc<RwLock<PermissionPolicy>>,
}

impl Default for PermissionService {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionService {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<PermissionMessage>();

        let tx = if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                let mut session_permissions: HashSet<(String, String, String)> = HashSet::new();
                let mut auto_approve_sessions: HashSet<String> = HashSet::new();
                let mut pending_requests: std::collections::HashMap<String, PermissionRequest> =
                    std::collections::HashMap::new();
                let mut pending_responders: std::collections::HashMap<String, oneshot::Sender<bool>> =
                    std::collections::HashMap::new();
                let mut notify_tx: Option<mpsc::UnboundedSender<PermissionRequest>> = None;

                while let Some(msg) = rx.recv().await {
                    match msg {
                        PermissionMessage::SetNotifier { tx } => {
                            notify_tx = Some(tx);
                        }
                        PermissionMessage::Request { request, respond_to } => {
                            if auto_approve_sessions.contains(&request.session_id) {
                                debug!("Auto-approving for session {}", request.session_id);
                                let _ = respond_to.send(true);
                                continue;
                            }

                            let key = (
                                request.session_id.clone(),
                                request.tool_name.clone(),
                                request.action.clone(),
                            );
                            if session_permissions.contains(&key) {
                                debug!(
                                    "Session permission found for {}:{}",
                                    request.tool_name, request.action
                                );
                                let _ = respond_to.send(true);
                                continue;
                            }

                            info!(
                                "Permission required: {} ({})",
                                request.tool_name, request.description
                            );
                            if let Some(ref notifier) = notify_tx {
                                let _ = notifier.send(request.clone());
                            }
                            let request_id = request.id.clone();
                            pending_requests.insert(request_id.clone(), request);
                            pending_responders.insert(request_id, respond_to);
                        }
                        PermissionMessage::Respond {
                            request_id,
                            response,
                        } => {
                            if let Some(responder) = pending_responders.remove(&request_id) {
                                let approved = response != PermissionResponse::Deny;
                                let _ = responder.send(approved);

                                if response == PermissionResponse::AllowForSession {
                                    if let Some(request) = pending_requests.remove(&request_id) {
                                        let key = (
                                            request.session_id,
                                            request.tool_name,
                                            request.action,
                                        );
                                        session_permissions.insert(key);
                                    }
                                } else {
                                    pending_requests.remove(&request_id);
                                }
                            } else {
                                warn!("No pending responder for request {}", request_id);
                            }
                        }
                        PermissionMessage::AutoApproveSession { session_id } => {
                            info!("Auto-approving all tools for session {}", session_id);
                            auto_approve_sessions.insert(session_id);
                        }
                        PermissionMessage::GrantSessionPermission {
                            session_id,
                            tool_name,
                            action,
                        } => {
                            let key = (session_id, tool_name, action);
                            session_permissions.insert(key);
                        }
                    }
                }
            });
            Some(tx)
        } else {
            None
        };

        Self {
            tx,
            auto_approve_tools: Arc::new(RwLock::new(Vec::new())),
            permission_policy: Arc::new(RwLock::new(PermissionPolicy::Ask)),
        }
    }

    pub fn set_auto_approve_tools(&self, tools: Vec<String>) {
        if let Ok(mut guard) = self.auto_approve_tools.write() {
            *guard = tools;
        }
    }

    pub fn set_permission_policy(&self, policy: PermissionPolicy) {
        if let Ok(mut guard) = self.permission_policy.write() {
            *guard = policy;
        }
    }

    pub fn set_notifier(&self, tx: mpsc::UnboundedSender<PermissionRequest>) {
        if let Some(ref sender) = self.tx {
            let _ = sender.send(PermissionMessage::SetNotifier { tx });
        }
    }

    pub async fn request(&self, request: PermissionRequest) -> bool {
        if let Ok(policy) = self.permission_policy.read() {
            match *policy {
                PermissionPolicy::AutoApprove => return true,
                PermissionPolicy::Deny => return false,
                PermissionPolicy::Ask => {}
            }
        }

        if let Ok(tools) = self.auto_approve_tools.read() {
            if tools.iter().any(|t| t.eq_ignore_ascii_case(&request.tool_name)) {
                debug!("Auto-approving tool {} from config", request.tool_name);
                return true;
            }
        }

        let Some(ref sender) = self.tx else {
            return true;
        };

        let (tx, rx) = oneshot::channel();
        let msg = PermissionMessage::Request {
            request,
            respond_to: tx,
        };

        if sender.send(msg).is_err() {
            warn!("Permission service channel closed");
            return false;
        }

        match rx.await {
            Ok(approved) => approved,
            Err(_) => {
                warn!("Permission response channel dropped");
                false
            }
        }
    }

    pub fn respond(&self, request_id: String, response: PermissionResponse) {
        if let Some(ref sender) = self.tx {
            let msg = PermissionMessage::Respond {
                request_id,
                response,
            };
            let _ = sender.send(msg);
        }
    }

    pub fn grant_session_permission(
        &self,
        session_id: String,
        tool_name: String,
        action: String,
    ) {
        if let Some(ref sender) = self.tx {
            let msg = PermissionMessage::GrantSessionPermission {
                session_id,
                tool_name,
                action,
            };
            let _ = sender.send(msg);
        }
    }

    pub fn auto_approve_session(&self, session_id: String) {
        if let Some(ref sender) = self.tx {
            let msg = PermissionMessage::AutoApproveSession { session_id };
            let _ = sender.send(msg);
        }
    }
}

/// Check if a tool name requires permission approval.
/// Matches opencode's approach: some tools are always dangerous.
pub fn tool_requires_permission(tool_name: &str) -> bool {
    let dangerous = [
        "shell",
        "exec",
        "execute",
        "write_file",
        "writefile",
        "writeFile",
        "edit_file",
        "editfile",
        "editFile",
        "str_replace_file",
        "strreplacefile",
        "strReplaceFile",
        "delete_file",
        "deletefile",
        "deleteFile",
        "remove_file",
        "patch",
        "apply_patch",
        "applyPatch",
        "git", // git can be destructive
        "tmux", // tmux can execute arbitrary commands
    ];
    dangerous
        .iter()
        .any(|&d| tool_name.eq_ignore_ascii_case(d))
}

/// Check if a shell command is safe read-only (doesn't need permission).
/// Based on opencode's safeReadOnlyCommands list.
pub fn is_safe_readonly_command(command: &str) -> bool {
    let safe_prefixes = [
        "ls", "echo", "pwd", "date", "cal", "uptime", "whoami", "id", "groups",
        "env", "printenv", "which", "type", "whereis", "whatis", "uname", "hostname",
        "df", "du", "free", "top", "ps", "kill", "killall", "nice", "nohup", "time",
        "git status", "git log", "git diff", "git show", "git branch", "git tag",
        "git remote", "git ls-files", "git ls-remote", "git rev-parse",
        "git config --get", "git config --list", "git describe", "git blame",
        "git grep", "git shortlog",
        "go version", "go help", "go list", "go env", "go doc", "go vet", "go fmt",
        "go mod", "go test", "go build", "go run", "go install", "go clean",
        "cargo --version", "cargo check", "cargo test", "cargo build", "cargo run",
        "cargo clippy", "cargo fmt",
    ];

    let cmd_lower = command.trim().to_lowercase();
    for prefix in &safe_prefixes {
        let prefix_lower = prefix.to_lowercase();
        if cmd_lower == *prefix_lower {
            return true;
        }
        if cmd_lower.starts_with(&prefix_lower) {
            let next_char = cmd_lower.chars().nth(prefix_lower.len());
            if next_char.is_none() || next_char == Some(' ') || next_char == Some('-') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_requires_permission() {
        assert!(tool_requires_permission("shell"));
        assert!(tool_requires_permission("write_file"));
        assert!(tool_requires_permission("editFile"));
        assert!(!tool_requires_permission("read_file"));
        assert!(!tool_requires_permission("glob"));
        assert!(!tool_requires_permission("search"));
    }

    #[test]
    fn test_is_safe_readonly_command() {
        assert!(is_safe_readonly_command("ls -la"));
        assert!(is_safe_readonly_command("git status"));
        assert!(is_safe_readonly_command("git log --oneline"));
        assert!(!is_safe_readonly_command("git push"));
        assert!(!is_safe_readonly_command("rm -rf /"));
        assert!(is_safe_readonly_command("cargo check"));
        assert!(!is_safe_readonly_command("cargo publish"));
    }

    #[tokio::test]
    async fn test_permission_service_allow() {
        let service = PermissionService::new();
        let request = PermissionRequest::new(
            "session-1",
            "shell",
            "Execute: ls -la",
            "execute",
            serde_json::json!({"command": "ls -la"}),
            "/home/user",
        );

        // Respond with allow
        let request_id = request.id.clone();
        let service_clone = service.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            service_clone.respond(request_id, PermissionResponse::Allow);
        });

        let approved = service.request(request).await;
        assert!(approved);
    }

    #[tokio::test]
    async fn test_permission_service_deny() {
        let service = PermissionService::new();
        let request = PermissionRequest::new(
            "session-1",
            "shell",
            "Execute: rm -rf /",
            "execute",
            serde_json::json!({"command": "rm -rf /"}),
            "/",
        );

        let request_id = request.id.clone();
        let service_clone = service.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            service_clone.respond(request_id, PermissionResponse::Deny);
        });

        let approved = service.request(request).await;
        assert!(!approved);
    }

    #[tokio::test]
    async fn test_permission_service_auto_approve_session() {
        let service = PermissionService::new();
        service.auto_approve_session("session-auto".to_string());

        // Give time for auto_approve to register
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let request = PermissionRequest::new(
            "session-auto",
            "shell",
            "Execute: anything",
            "execute",
            serde_json::json!({}),
            "/",
        );

        let approved = service.request(request).await;
        assert!(approved);
    }
}
