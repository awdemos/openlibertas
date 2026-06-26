use crate::domain::{ApiMessage, ToolDefinition};
use serde::{Deserialize, Serialize};

// ============================================================================
// OpenAI-compatible types
// ============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct OllamaModelInfo {
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OllamaModelsResponse {
    pub(crate) models: Vec<OllamaModelInfo>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct OllamaChatRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<ApiMessage>,
    pub(crate) stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OllamaChatResponse {
    pub(crate) message: Option<OllamaMessage>,
    #[serde(default)]
    pub(crate) done: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OllamaMessage {
    pub(crate) _role: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OllamaToolCall {
    pub(crate) function: OllamaFunctionCall,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OllamaFunctionCall {
    pub(crate) name: String,
    pub(crate) arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChatCompletionChunk {
    pub(crate) choices: Vec<ChunkChoice>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ChunkChoice {
    pub(crate) delta: Delta,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Delta {
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) reasoning_content: Option<String>,
    #[serde(default)]
    pub(crate) thinking: Option<String>,
    #[serde(default)]
    pub(crate) tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ToolCallDelta {
    pub(crate) index: usize,
    pub(crate) id: Option<String>,
    #[serde(rename = "type")]
    pub(crate) call_type: Option<String>,
    pub(crate) function: Option<FunctionCallDelta>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FunctionCallDelta {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

// ============================================================================
// Anthropic types
// ============================================================================

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicRequest {
    pub(crate) model: String,
    pub(crate) messages: Vec<AnthropicMessage>,
    pub(crate) max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<AnthropicTool>>,
    pub(crate) stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct AnthropicMessage {
    pub(crate) role: String,
    pub(crate) content: AnthropicContent,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub(crate) enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl AnthropicContent {
    pub(crate) fn append_text(&mut self, text: &str) {
        match self {
            AnthropicContent::Text(s) => {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(text);
            }
            AnthropicContent::Blocks(blocks) => {
                if let Some(AnthropicContentBlock::Text { text: last }) = blocks.last_mut() {
                    last.push('\n');
                    last.push_str(text);
                } else {
                    blocks.push(AnthropicContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }
        }
    }

    pub(crate) fn append_block(&mut self, block: AnthropicContentBlock) {
        match self {
            AnthropicContent::Text(s) => {
                let mut blocks = Vec::new();
                if !s.is_empty() {
                    blocks.push(AnthropicContentBlock::Text { text: s.clone() });
                }
                blocks.push(block);
                *self = AnthropicContent::Blocks(blocks);
            }
            AnthropicContent::Blocks(blocks) => {
                blocks.push(block);
            }
        }
    }

    pub(crate) fn append_blocks(&mut self, new_blocks: Vec<AnthropicContentBlock>) {
        for block in new_blocks {
            self.append_block(block);
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub(crate) enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

impl AnthropicMessage {
    pub(crate) fn append_text(&mut self, text: &str) {
        self.content.append_text(text);
    }

    pub(crate) fn append_block(&mut self, block: AnthropicContentBlock) {
        self.content.append_block(block);
    }

    pub(crate) fn append_blocks(&mut self, blocks: Vec<AnthropicContentBlock>) {
        self.content.append_blocks(blocks);
    }
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct AnthropicTool {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicModelsResponse {
    pub(crate) data: Vec<AnthropicModelItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicModelItem {
    pub(crate) id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub(crate) event_type: String,
    #[serde(default)]
    pub(crate) index: usize,
    #[serde(default)]
    pub(crate) delta: Option<AnthropicDelta>,
    #[serde(default)]
    pub(crate) content_block: Option<AnthropicContentBlock>,
    #[serde(default)]
    pub(crate) error: Option<AnthropicApiError>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicDelta {
    #[serde(rename = "type")]
    pub(crate) delta_type: Option<String>,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) partial_json: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicApiError {
    #[serde(rename = "type")]
    pub(crate) error_type: String,
    pub(crate) message: String,
}

// ============================================================================
// Gemini types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system_instruction: Option<GeminiContent>,
    pub(crate) contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<GeminiTool>>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct GeminiContent {
    pub(crate) role: String,
    pub(crate) parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub(crate) enum GeminiPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct GeminiFunctionCall {
    pub(crate) name: String,
    pub(crate) args: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct GeminiFunctionResponse {
    pub(crate) name: String,
    pub(crate) response: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiGenerationConfig {
    pub(crate) max_output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeminiTool {
    pub(crate) function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeminiFunctionDeclaration {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiStreamResponse {
    #[serde(default)]
    pub(crate) candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    pub(crate) error: Option<GeminiErrorDetail>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiCandidate {
    pub(crate) content: GeminiContentResponse,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiContentResponse {
    #[serde(default)]
    pub(crate) parts: Vec<GeminiPartResponse>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) role: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum GeminiPartResponse {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCallResponse,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct GeminiFunctionCallResponse {
    pub(crate) name: String,
    pub(crate) args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiErrorDetail {
    pub(crate) code: i32,
    pub(crate) message: String,
    #[allow(dead_code)]
    pub(crate) status: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiModelsResponse {
    pub(crate) models: Vec<GeminiModelItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GeminiModelItem {
    pub(crate) name: String,
}

// ============================================================================
// MultiProvider
// ============================================================================
