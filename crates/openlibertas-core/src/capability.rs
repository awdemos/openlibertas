use serde::{Deserialize, Serialize};

use crate::tool_format::ToolFormat;

/// Capability flags for a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub tools: bool,
    pub vision: bool,
    pub json_mode: bool,
    pub streaming: bool,
    pub reasoning: bool,
    pub max_tokens: Option<u32>,
    pub context_window: Option<u32>,
    pub tool_format: ToolFormat,
}

/// Identifies the API family of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Generic OpenAI-compatible API (default).
    #[default]
    OpenAiCompatible,
    /// Anthropic Claude API.
    Anthropic,
    /// Google Gemini API.
    Gemini,
    /// Native Ollama API.
    Ollama,
}

impl ProviderKind {
    /// Return default capabilities for this provider kind.
    pub fn default_capabilities(self) -> ProviderCapabilities {
        match self {
            ProviderKind::OpenAiCompatible => ProviderCapabilities {
                tools: true,
                vision: true,
                json_mode: true,
                streaming: true,
                reasoning: false,
                max_tokens: None,
                context_window: None,
                tool_format: ToolFormat::Native,
            },
            ProviderKind::Anthropic => ProviderCapabilities {
                tools: true,
                vision: true,
                json_mode: false,
                streaming: true,
                reasoning: true,
                max_tokens: None,
                context_window: None,
                tool_format: ToolFormat::Native,
            },
            ProviderKind::Gemini => ProviderCapabilities {
                tools: true,
                vision: true,
                json_mode: true,
                streaming: true,
                reasoning: false,
                max_tokens: None,
                context_window: None,
                tool_format: ToolFormat::Native,
            },
            ProviderKind::Ollama => ProviderCapabilities {
                tools: true,
                vision: false,
                json_mode: false,
                streaming: true,
                reasoning: false,
                max_tokens: None,
                context_window: None,
                tool_format: ToolFormat::Native,
            },
        }
    }
}
