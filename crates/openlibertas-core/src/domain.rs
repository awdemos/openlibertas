use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::System => write!(f, "system"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "user" => Ok(Role::User),
            "assistant" => Ok(Role::Assistant),
            "system" => Ok(Role::System),
            "tool" => Ok(Role::Tool),
            _ => Err(format!("Unknown role: {}", s)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub struct ProviderId(pub String);

impl ProviderId {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into().to_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ProviderId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for ProviderId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ModelsResponse {
    pub data: Vec<Model>,
}

#[derive(Debug, Deserialize, Clone)]
#[non_exhaustive]
pub struct Model {
    pub id: String,
    #[serde(skip)]
    pub provider: ProviderId,
    #[serde(skip)]
    pub supports_tools: bool,
    #[serde(skip)]
    pub supports_voice: bool,
    #[serde(skip)]
    pub local: bool,
}

impl Model {
    /// Infer capabilities from model name patterns.
    /// Returns (supports_tools, supports_voice).
    pub fn infer_capabilities(id: &str) -> (bool, bool) {
        let lower = id.to_lowercase();

        // Voice/audio-capable model families (local GGUF)
        let voice_patterns = [
            "omni",       // Qwen2.5-Omni (audio+vision+text)
            "multimodal", // Phi-4-multimodal (audio+vision+text)
            "audio",      // Qwen2-Audio, SeaLLM-Audio
            "ultravox",   // Ultravox (audio understanding)
            "voxtral",    // Mistral Voxtral (audio)
            "speech",     // Speech models
            " whisper",   // Whisper (STT)
        ];
        let supports_voice = voice_patterns.iter().any(|p| lower.contains(p));

        // Tool-capable model families (heuristic for local models)
        let tool_patterns = [
            "instruct",  // Most instruct models support tools
            "coder",     // Code models usually tool-capable
            "tool",      // Explicitly fine-tuned for tools
            "function",  // Function-calling variants
            "agent",     // Agent-tuned models
            "qwen2.5",   // Qwen2.5 series has native tool support
            "qwen3",     // Qwen3 series
            "qwen3.5",   // Qwen3.5 series
            "llama3",    // Llama 3 instruct variants
            "llama-3",   // Alternate naming
            "phi4",      // Phi-4 series
            "phi-4",     // Alternate naming
            "gemma3",    // Gemma 3
            "gemma-3",   // Alternate naming
            "mistral",   // Mistral instruct
            "mixtral",   // Mixtral instruct
            "nemotron",  // NVIDIA Nemotron
            "glm4",      // GLM-4
            "command-r", // Cohere Command-R
        ];
        let supports_tools = tool_patterns.iter().any(|p| lower.contains(p));

        (supports_tools, supports_voice)
    }
}

#[derive(Debug, Serialize, Clone)]
#[non_exhaustive]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub extra_params: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

pub fn now_timestamp() -> String {
    let now = std::time::SystemTime::now();
    let dt: chrono::DateTime<chrono::Local> = now.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[derive(Debug, Serialize, Clone)]
#[non_exhaustive]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Serialize, Clone)]
#[non_exhaustive]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Text(String),
    Reasoning(String),
    ToolCall(ToolCall),
    Done,
    Error(String),
    Cancelled,
}

/// Result of executing a tool call
#[derive(Debug, Clone)]
pub enum ToolExecutionResult {
    Success {
        tool_name: String,
        key_arg: String,
        output: String,
    },
    Error {
        tool_name: String,
        key_arg: String,
        error: String,
    },
    Skipped {
        tool_name: String,
        reason: String,
    },
}

impl ToolExecutionResult {
    pub fn tool_name(&self) -> &str {
        match self {
            ToolExecutionResult::Success { tool_name, .. } => tool_name,
            ToolExecutionResult::Error { tool_name, .. } => tool_name,
            ToolExecutionResult::Skipped { tool_name, .. } => tool_name,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, ToolExecutionResult::Success { .. })
    }

    pub fn content_for_message(&self) -> String {
        match self {
            ToolExecutionResult::Success { output, .. } => output.clone(),
            ToolExecutionResult::Error { error, .. } => format!("Error: {}", error),
            ToolExecutionResult::Skipped { reason, .. } => format!("Skipped: {}", reason),
        }
    }
}

/// Tracks the status of an MCP server connection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpServerStatus {
    Pending,
    Connecting,
    Connected,
    Failed,
    Disabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serializes_to_lowercase() {
        let role = Role::User;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"user\"");
    }

    #[test]
    fn role_deserializes_from_lowercase() {
        let role: Role = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(role, Role::Assistant);
    }

    #[test]
    fn role_roundtrip() {
        for role in [Role::User, Role::Assistant, Role::System, Role::Tool] {
            let json = serde_json::to_string(&role).unwrap();
            let parsed: Role = serde_json::from_str(&json).unwrap();
            assert_eq!(role, parsed);
        }
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::User.to_string(), "user");
        assert_eq!(Role::Assistant.to_string(), "assistant");
        assert_eq!(Role::System.to_string(), "system");
        assert_eq!(Role::Tool.to_string(), "tool");
    }

    #[test]
    fn role_from_str() {
        assert_eq!("user".parse::<Role>().unwrap(), Role::User);
        assert_eq!("ASSISTANT".parse::<Role>().unwrap(), Role::Assistant);
        assert!("unknown".parse::<Role>().is_err());
    }

    #[test]
    fn message_with_role_serializes() {
        let msg = Message { role: Role::User, content: "hello".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\""));
        assert!(json.contains("\"user\""));
        assert!(json.contains("\"content\""));
        assert!(json.contains("\"hello\""));
    }

    #[test]
    fn message_deserializes_with_role() {
        let json = r#"{"role":"assistant","content":"hi","tool_calls":null,"tool_call_id":null}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content, "hi");
    }

    #[test]
    fn provider_id_creation() {
        let id = ProviderId::new("OpenAI");
        assert_eq!(id.as_str(), "openai");
    }

    #[test]
    fn provider_id_from_str() {
        let id: ProviderId = "Kimi".into();
        assert_eq!(id.as_str(), "kimi");
    }
}
