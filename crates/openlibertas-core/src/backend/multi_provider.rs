use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::backend::Provider;
use crate::capability::{ProviderCapabilities, ProviderKind};
use crate::domain::*;
use crate::tool_format::ToolFormat;

// ============================================================================
// OpenAI-compatible types
// ============================================================================

#[derive(Debug, Deserialize)]
struct OllamaModelInfo {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OllamaModelsResponse {
    models: Vec<OllamaModelInfo>,
}

#[derive(Debug, serde::Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<ApiMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, serde::Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: Option<OllamaMessage>,
    #[serde(default)]
    done: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    _role: String,
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCall {
    function: OllamaFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OllamaFunctionCall {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, Deserialize)]
struct ChunkChoice {
    delta: Delta,
}

#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<FunctionCallDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionCallDelta {
    name: Option<String>,
    arguments: Option<String>,
}

// ============================================================================
// Anthropic types
// ============================================================================

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl AnthropicContent {
    fn append_text(&mut self, text: &str) {
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

    fn append_block(&mut self, block: AnthropicContentBlock) {
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

    fn append_blocks(&mut self, new_blocks: Vec<AnthropicContentBlock>) {
        for block in new_blocks {
            self.append_block(block);
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
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
    fn append_text(&mut self, text: &str) {
        self.content.append_text(text);
    }

    fn append_block(&mut self, block: AnthropicContentBlock) {
        self.content.append_block(block);
    }

    fn append_blocks(&mut self, blocks: Vec<AnthropicContentBlock>) {
        self.content.append_blocks(blocks);
    }
}

#[derive(Debug, Serialize, Clone)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModelItem>,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelItem {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    index: usize,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    content_block: Option<AnthropicContentBlock>,
    #[serde(default)]
    error: Option<AnthropicApiError>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicApiError {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

// ============================================================================
// Gemini types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
enum GeminiPart {
    Text { text: String },
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
struct GeminiFunctionCall {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
struct GeminiFunctionResponse {
    name: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerationConfig {
    max_output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiTool {
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GeminiStreamResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    error: Option<GeminiErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
}

#[derive(Debug, Deserialize)]
struct GeminiContentResponse {
    #[serde(default)]
    parts: Vec<GeminiPartResponse>,
    #[serde(default)]
    #[allow(dead_code)]
    role: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum GeminiPartResponse {
    Text { text: String },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCallResponse,
    },
}

#[derive(Debug, Deserialize, Clone)]
struct GeminiFunctionCallResponse {
    name: String,
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    code: i32,
    message: String,
    #[allow(dead_code)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModelItem>,
}

#[derive(Debug, Deserialize)]
struct GeminiModelItem {
    name: String,
}

// ============================================================================
// MultiProvider
// ============================================================================

#[derive(Clone)]
pub struct MultiProvider {
    client: Client,
    base_url: String,
    api_key: crate::config::SecretString,
    provider_kind: ProviderKind,
    tool_format: ToolFormat,
    capabilities: ProviderCapabilities,
    extra_params: Option<serde_json::Map<String, serde_json::Value>>,
}

impl MultiProvider {
    pub fn with_capabilities(
        base_url: String,
        api_key: crate::config::SecretString,
        provider_kind: ProviderKind,
        capabilities: ProviderCapabilities,
        extra_params: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            base_url,
            api_key,
            provider_kind,
            tool_format: capabilities.tool_format,
            capabilities,
            extra_params,
        }
    }

    // ========================================================================
    // Chat
    // ========================================================================

    pub fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        match self.provider_kind {
            ProviderKind::Anthropic => {
                self.chat_anthropic(model, messages, max_tokens, tools, cancel_token, temperature)
            }
            ProviderKind::Gemini => {
                self.chat_gemini(model, messages, max_tokens, tools, cancel_token, temperature)
            }
            ProviderKind::OpenAiCompatible | ProviderKind::Ollama => {
                self.chat_openai(model, messages, max_tokens, tools, cancel_token, temperature)
            }
        }
    }

    // ========================================================================
    // OpenAI-compatible chat (default path)
    // ========================================================================

    fn chat_openai(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        let mut extra_params = self.extra_params.clone().unwrap_or_default();
        if let Some(temp) = temperature {
            extra_params.insert("temperature".to_string(), serde_json::json!(temp));
        }
        for key in &["model", "messages", "stream", "max_tokens", "tools"] {
            extra_params.remove(*key);
        }
        let msg_count = messages.len();
        let api_messages: Vec<ApiMessage> = messages.into_iter().map(Into::into).collect();
        let tools_for_req = if self.tool_format.sends_native_tools() {
            tools.clone()
        } else {
            None
        };
        let req = ChatRequest {
            model: model.clone(),
            messages: api_messages.clone(),
            stream: true,
            max_tokens: Some(max_tokens),
            tools: tools_for_req,
            extra_params: Some(extra_params),
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let tool_format = self.tool_format;

        let tools_count = req.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        info!(
            "Chat request: model={}, messages={}, max_tokens={}, tools={} ({} definitions)",
            model,
            msg_count,
            max_tokens,
            req.tools.is_some(),
            tools_count
        );
        if let Some(ref tools) = req.tools {
            for tool in tools {
                debug!(
                    "Tool available: {} - {}",
                    tool.function.name, tool.function.description
                );
            }
        }

        tokio::spawn(async move {
            let url = format!("{}/chat/completions", base_url);
            let mut retries = 0;
            const MAX_RETRIES: u32 = 3;
            let mut use_ollama = false;
            let mut resp = None;

            loop {
                match client
                    .post(&url)
                    .bearer_auth(api_key.expose_secret())
                    .json(&req)
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        if status.is_success() {
                            info!("Chat response: HTTP {}", status);
                            resp = Some(r);
                            break;
                        }
                        if status.as_u16() == 404 {
                            use_ollama = true;
                            break;
                        }
                        let is_transient =
                            status.as_u16() == 429 || (502..=504).contains(&status.as_u16());
                        if is_transient && retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            warn!(
                                "Chat HTTP {} -- retrying {}/{} in {:?}",
                                status, retries, MAX_RETRIES, delay
                            );
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[HTTP {} -- retrying {}/{} in {:?}]",
                                status, retries, MAX_RETRIES, delay
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        let body = r.text().await.unwrap_or_default();
                        error!("Chat HTTP error: {} -- {}", status, body);
                        let _ = tx.send(BackendEvent::Error(format!(
                            "[HTTP {}: {}]",
                            status,
                            if body.is_empty() {
                                "Unknown error".to_string()
                            } else {
                                body
                            }
                        )));
                        return;
                    }
                    Err(e) => {
                        if retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            warn!(
                                "Chat connection error -- retrying {}/{} in {:?}: {}",
                                retries, MAX_RETRIES, delay, e
                            );
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[Connection error -- retrying {}/{} in {:?}: {}]",
                                retries, MAX_RETRIES, delay, e
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        error!(
                            "Chat connection failed after {} retries: {}",
                            MAX_RETRIES, e
                        );
                        let _ = tx.send(BackendEvent::Error(format!("[Error: {}]", e)));
                        return;
                    }
                }
            }

            if use_ollama {
                let ollama_base = base_url.trim_end_matches("/v1");
                let ollama_url = format!("{}/api/chat", ollama_base);
                let ollama_req = OllamaChatRequest {
                    model: model.clone(),
                    messages: api_messages,
                    stream: true,
                    options: Some(OllamaOptions {
                        num_predict: if max_tokens > 0 {
                            Some(max_tokens as i32)
                        } else {
                            None
                        },
                        temperature,
                    }),
                    tools: if tool_format.sends_native_tools() {
                        tools
                    } else {
                        None
                    },
                };
                info!("Falling back to Ollama native API: {}", ollama_url);
                match client
                    .post(&ollama_url)
                    .bearer_auth(api_key.expose_secret())
                    .json(&ollama_req)
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        if !status.is_success() {
                            let body = r.text().await.unwrap_or_default();
                            error!("Ollama chat HTTP error: {} -- {}", status, body);
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[Ollama HTTP {}: {}]",
                                status,
                                if body.is_empty() {
                                    "Unknown error".to_string()
                                } else {
                                    body
                                }
                            )));
                            return;
                        }
                        Self::stream_ollama(r, cancel_token, tx).await;
                        return;
                    }
                    Err(e) => {
                        error!("Ollama chat connection failed: {}", e);
                        let _ = tx.send(BackendEvent::Error(format!("[Ollama error: {}]", e)));
                        return;
                    }
                }
            }

            if let Some(resp) = resp {
                Self::stream_openai(resp, cancel_token, tx).await;
            }
        });

        rx
    }

    // ========================================================================
    // Anthropic chat
    // ========================================================================

    fn chat_anthropic(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        let (system, anthropic_messages) = convert_messages_anthropic(messages);

        let tools_for_req = if self.capabilities.tool_format.sends_native_tools() {
            tools.map(|defs| {
                defs.into_iter()
                    .map(|d| AnthropicTool {
                        name: d.function.name,
                        description: d.function.description,
                        input_schema: d.function.parameters,
                    })
                    .collect()
            })
        } else {
            None
        };

        // Build extra params, filtering out keys that are set explicitly.
        let mut extra = self.extra_params.clone().unwrap_or_default();
        for key in &["model", "messages", "stream", "max_tokens", "tools", "system"] {
            extra.remove(*key);
        }
        if temperature.is_some() {
            extra.remove("temperature");
        }

        let req = AnthropicRequest {
            model: model.clone(),
            messages: anthropic_messages.clone(),
            max_tokens,
            system,
            tools: tools_for_req,
            stream: true,
            temperature,
        };

        let tools_count = req.tools.as_ref().map(|t| t.len()).unwrap_or(0);
        info!(
            "Anthropic chat request: model={}, messages={}, max_tokens={}, tools={} ({} definitions)",
            model,
            anthropic_messages.len(),
            max_tokens,
            req.tools.is_some(),
            tools_count
        );
        if let Some(ref tools) = req.tools {
            for tool in tools {
                debug!("Tool available: {} - {}", tool.name, tool.description);
            }
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let extra_params = if extra.is_empty() { None } else { Some(extra) };

        tokio::spawn(async move {
            let url = format!("{}/messages", base_url);
            let mut retries = 0;
            const MAX_RETRIES: u32 = 3;

            loop {
                let mut builder = client
                    .post(&url)
                    .header("x-api-key", api_key.expose_secret())
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json");

                let mut req_json = match serde_json::to_value(&req) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("Failed to serialize Anthropic request: {}", e);
                        let _ = tx.send(BackendEvent::Error(format!(
                            "[Serialization error: {}]",
                            e
                        )));
                        return;
                    }
                };

                if let Some(ref extra) = extra_params {
                    if let Some(obj) = req_json.as_object_mut() {
                        for (k, v) in extra {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                }

                builder = builder.json(&req_json);

                match builder.send().await {
                    Ok(r) => {
                        let status = r.status();
                        if status.is_success() {
                            info!("Anthropic chat response: HTTP {}", status);
                            Self::stream_anthropic(r, cancel_token, tx).await;
                            return;
                        }
                        let is_transient =
                            status.as_u16() == 429 || (502..=504).contains(&status.as_u16());
                        if is_transient && retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            warn!(
                                "Anthropic HTTP {} -- retrying {}/{} in {:?}",
                                status, retries, MAX_RETRIES, delay
                            );
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[HTTP {} -- retrying {}/{} in {:?}]",
                                status, retries, MAX_RETRIES, delay
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        let body = r.text().await.unwrap_or_default();
                        error!("Anthropic chat HTTP error: {} -- {}", status, body);
                        let _ = tx.send(BackendEvent::Error(format!(
                            "[HTTP {}: {}]",
                            status,
                            if body.is_empty() {
                                "Unknown error".to_string()
                            } else {
                                body
                            }
                        )));
                        return;
                    }
                    Err(e) => {
                        if retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            warn!(
                                "Anthropic connection error -- retrying {}/{} in {:?}: {}",
                                retries, MAX_RETRIES, delay, e
                            );
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[Connection error -- retrying {}/{} in {:?}: {}]",
                                retries, MAX_RETRIES, delay, e
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        error!(
                            "Anthropic chat connection failed after {} retries: {}",
                            MAX_RETRIES, e
                        );
                        let _ = tx.send(BackendEvent::Error(format!("[Error: {}]", e)));
                        return;
                    }
                }
            }
        });

        rx
    }

    // ========================================================================
    // Gemini chat
    // ========================================================================

    fn chat_gemini(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        let (system, gemini_contents) = convert_messages_gemini(messages);

        let tools_for_req = if self.capabilities.tool_format.sends_native_tools() {
            tools.map(|defs| {
                vec![GeminiTool {
                    function_declarations: defs
                        .into_iter()
                        .map(|d| GeminiFunctionDeclaration {
                            name: d.function.name,
                            description: d.function.description,
                            parameters: d.function.parameters,
                        })
                        .collect(),
                }]
            })
        } else {
            None
        };

        let mut extra = self.extra_params.clone().unwrap_or_default();
        for key in &["model", "contents", "systemInstruction", "generationConfig", "tools"] {
            extra.remove(*key);
        }
        if temperature.is_some() {
            extra.remove("temperature");
        }

        let req = GeminiRequest {
            system_instruction: system,
            contents: gemini_contents.clone(),
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: max_tokens,
                temperature,
            }),
            tools: tools_for_req,
        };

        let tools_count = req.tools.as_ref().map(|t| t[0].function_declarations.len()).unwrap_or(0);
        info!(
            "Gemini chat request: model={}, messages={}, max_tokens={}, tools={} ({} definitions)",
            model,
            gemini_contents.len(),
            max_tokens,
            req.tools.is_some(),
            tools_count
        );
        if let Some(ref tools) = req.tools {
            for tool in &tools[0].function_declarations {
                debug!("Tool available: {} - {}", tool.name, tool.description);
            }
        }

        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();
        let model = model.clone();
        let extra_params = if extra.is_empty() { None } else { Some(extra) };

        tokio::spawn(async move {
            let url = format!(
                "{}/models/{}:streamGenerateContent?key={}",
                base_url.trim_end_matches('/'),
                model,
                api_key.expose_secret()
            );
            let mut retries = 0;
            const MAX_RETRIES: u32 = 3;

            loop {
                let mut req_json = match serde_json::to_value(&req) {
                    Ok(v) => v,
                    Err(e) => {
                        error!("Failed to serialize Gemini request: {}", e);
                        let _ = tx.send(BackendEvent::Error(format!(
                            "[Serialization error: {}]",
                            e
                        )));
                        return;
                    }
                };

                if let Some(ref extra) = extra_params {
                    if let Some(obj) = req_json.as_object_mut() {
                        for (k, v) in extra {
                            obj.insert(k.clone(), v.clone());
                        }
                    }
                }

                match client
                    .post(&url)
                    .header("content-type", "application/json")
                    .json(&req_json)
                    .send()
                    .await
                {
                    Ok(r) => {
                        let status = r.status();
                        if status.is_success() {
                            info!("Gemini chat response: HTTP {}", status);
                            Self::stream_gemini(r, cancel_token, tx).await;
                            return;
                        }
                        let is_transient =
                            status.as_u16() == 429 || (502..=504).contains(&status.as_u16());
                        if is_transient && retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            warn!(
                                "Gemini HTTP {} -- retrying {}/{} in {:?}",
                                status, retries, MAX_RETRIES, delay
                            );
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[HTTP {} -- retrying {}/{} in {:?}]",
                                status, retries, MAX_RETRIES, delay
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        let body = r.text().await.unwrap_or_default();
                        error!("Gemini chat HTTP error: {} -- {}", status, body);
                        let _ = tx.send(BackendEvent::Error(format!(
                            "[HTTP {}: {}]",
                            status,
                            if body.is_empty() {
                                "Unknown error".to_string()
                            } else {
                                body
                            }
                        )));
                        return;
                    }
                    Err(e) => {
                        if retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            warn!(
                                "Gemini connection error -- retrying {}/{} in {:?}: {}",
                                retries, MAX_RETRIES, delay, e
                            );
                            let _ = tx.send(BackendEvent::Error(format!(
                                "[Connection error -- retrying {}/{} in {:?}: {}]",
                                retries, MAX_RETRIES, delay, e
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        error!(
                            "Gemini chat connection failed after {} retries: {}",
                            MAX_RETRIES, e
                        );
                        let _ = tx.send(BackendEvent::Error(format!("[Error: {}]", e)));
                        return;
                    }
                }
            }
        });

        rx
    }

    // ========================================================================
    // Stream handlers
    // ========================================================================

    async fn stream_openai(
        resp: reqwest::Response,
        cancel_token: tokio_util::sync::CancellationToken,
        tx: mpsc::UnboundedSender<BackendEvent>,
    ) {
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut accumulated_tool_calls: HashMap<usize, ToolCall> = HashMap::new();

        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    let _ = tx.send(BackendEvent::Cancelled);
                    return;
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(c) => c,
                        None => break,
                    }
                }
            };
            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line_bytes = if pos > 0 && buf[pos - 1] == b'\r' {
                            &buf[..pos - 1]
                        } else {
                            &buf[..pos]
                        };
                        let line = String::from_utf8_lossy(line_bytes);
                        let line = line.trim();
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                break;
                            }
                            if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
                                if let Some(choice) = chunk.choices.first() {
                                    if let Some(content) = &choice.delta.content {
                                        if !content.is_empty() {
                                            let _ = tx.send(BackendEvent::Text(content.clone()));
                                        }
                                    }
                                    if let Some(reasoning) = &choice.delta.reasoning_content {
                                        if !reasoning.is_empty() {
                                            let _ =
                                                tx.send(BackendEvent::Reasoning(reasoning.clone()));
                                        }
                                    } else if let Some(thinking) = &choice.delta.thinking {
                                        if !thinking.is_empty() {
                                            let _ = tx.send(BackendEvent::Reasoning(thinking.clone()));
                                        }
                                    }
                                    if let Some(tool_calls) = &choice.delta.tool_calls {
                                        for tc in tool_calls {
                                            let entry = accumulated_tool_calls
                                                .entry(tc.index)
                                                .or_insert_with(|| ToolCall {
                                                    id: tc.id.clone().unwrap_or_default(),
                                                    call_type: tc
                                                        .call_type
                                                        .clone()
                                                        .unwrap_or_else(|| "function".to_string()),
                                                    function: FunctionCall {
                                                        name: String::new(),
                                                        arguments: String::new(),
                                                    },
                                                });
                                            if let Some(id) = &tc.id {
                                                entry.id = id.clone();
                                            }
                                            if let Some(func) = &tc.function {
                                                if let Some(name) = &func.name {
                                                    entry.function.name.push_str(name);
                                                }
                                                if let Some(args) = &func.arguments {
                                                    entry.function.arguments.push_str(args);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        buf.drain(..=pos);
                    }
                }
                Err(e) => {
                    let tool_count = accumulated_tool_calls.len();
                    if tool_count > 0 {
                        info!(
                            "Stream error recovery: sending {} accumulated tool calls",
                            tool_count
                        );
                        for (_, tool_call) in accumulated_tool_calls {
                            let _ = tx.send(BackendEvent::ToolCall(tool_call));
                        }
                    }
                    error!("Chat stream error: {}", e);
                    let _ = tx.send(BackendEvent::Error(format!("[Stream error: {}]", e)));
                    return;
                }
            }
        }

        let tool_count = accumulated_tool_calls.len();
        if tool_count > 0 {
            info!("Chat complete: {} tool calls", tool_count);
        } else {
            info!("Chat complete");
        }
        for (_, tool_call) in accumulated_tool_calls {
            let _ = tx.send(BackendEvent::ToolCall(tool_call));
        }
        let _ = tx.send(BackendEvent::Done);
    }

    async fn stream_ollama(
        resp: reqwest::Response,
        cancel_token: tokio_util::sync::CancellationToken,
        tx: mpsc::UnboundedSender<BackendEvent>,
    ) {
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();

        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    let _ = tx.send(BackendEvent::Cancelled);
                    return;
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(c) => c,
                        None => break,
                    }
                }
            };
            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line_bytes = if pos > 0 && buf[pos - 1] == b'\r' {
                            &buf[..pos - 1]
                        } else {
                            &buf[..pos]
                        };
                        let line = String::from_utf8_lossy(line_bytes);
                        let line = line.trim();
                        if line.is_empty() {
                            buf.drain(..=pos);
                            continue;
                        }
                        if let Ok(resp) = serde_json::from_str::<OllamaChatResponse>(line) {
                            if resp.done {
                                break;
                            }
                            if let Some(msg) = resp.message {
                                if !msg.content.is_empty() {
                                    let _ = tx.send(BackendEvent::Text(msg.content));
                                }
                                if let Some(tool_calls) = msg.tool_calls {
                                    for tc in tool_calls {
                                        let args_str =
                                            serde_json::to_string(&tc.function.arguments)
                                                .unwrap_or_default();
                                        let tool_call = ToolCall {
                                            id: format!(
                                                "ollama_{}_{}",
                                                tc.function.name,
                                                std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_millis()
                                            ),
                                            call_type: "function".to_string(),
                                            function: FunctionCall {
                                                name: tc.function.name,
                                                arguments: args_str,
                                            },
                                        };
                                        let _ = tx.send(BackendEvent::ToolCall(tool_call));
                                    }
                                }
                            }
                        }
                        buf.drain(..=pos);
                    }
                }
                Err(e) => {
                    error!("Ollama stream error: {}", e);
                    let _ = tx.send(BackendEvent::Error(format!("[Ollama stream error: {}]", e)));
                    return;
                }
            }
        }

        info!("Ollama chat complete");
        let _ = tx.send(BackendEvent::Done);
    }

    async fn stream_anthropic(
        resp: reqwest::Response,
        cancel_token: tokio_util::sync::CancellationToken,
        tx: mpsc::UnboundedSender<BackendEvent>,
    ) {
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut _current_event_type: Option<String> = None;

        struct AccumulatedToolCall {
            id: String,
            name: String,
            partial_json: String,
        }
        let mut accumulated_tool_calls: HashMap<usize, AccumulatedToolCall> = HashMap::new();

        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    let _ = tx.send(BackendEvent::Cancelled);
                    return;
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(c) => c,
                        None => break,
                    }
                }
            };

            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line_bytes = if pos > 0 && buf[pos - 1] == b'\r' {
                            &buf[..pos - 1]
                        } else {
                            &buf[..pos]
                        };
                        let line = String::from_utf8_lossy(line_bytes);
                        let line = line.trim();

                        if line.is_empty() {
                            _current_event_type = None;
                        } else if let Some(et) = line.strip_prefix("event: ") {
                            _current_event_type = Some(et.to_string());
                        } else if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                break;
                            }

                            match serde_json::from_str::<AnthropicStreamEvent>(data) {
                                Ok(event) => {
                                    match event.event_type.as_str() {
                                        "content_block_delta" => {
                                            if let Some(delta) = event.delta {
                                                match delta.delta_type.as_deref() {
                                                    Some("text_delta") => {
                                                        if let Some(text) = delta.text {
                                                            if !text.is_empty() {
                                                                let _ = tx.send(BackendEvent::Text(text));
                                                            }
                                                        }
                                                    }
                                                    Some("input_json_delta") => {
                                                        if let Some(partial) = delta.partial_json {
                                                            let entry = accumulated_tool_calls
                                                                .entry(event.index)
                                                                .or_insert_with(|| AccumulatedToolCall {
                                                                    id: String::new(),
                                                                    name: String::new(),
                                                                    partial_json: String::new(),
                                                                });
                                                            entry.partial_json.push_str(&partial);
                                                        }
                                                    }
                                                    Some("thinking_delta") => {
                                                        if let Some(text) = delta.text {
                                                            if !text.is_empty() {
                                                                let _ = tx.send(BackendEvent::Reasoning(text));
                                                            }
                                                        }
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        "content_block_start" => {
                                            if let Some(AnthropicContentBlock::ToolUse { id, name, .. }) = event.content_block {
                                                let entry = accumulated_tool_calls
                                                    .entry(event.index)
                                                    .or_insert_with(|| AccumulatedToolCall {
                                                        id: String::new(),
                                                        name: String::new(),
                                                        partial_json: String::new(),
                                                    });
                                                entry.id = id;
                                                entry.name = name;
                                            }
                                        }
                                        "message_stop" => {
                                            break;
                                        }
                                        "error" => {
                                            if let Some(err) = event.error {
                                                let msg = format!(
                                                    "Anthropic API error ({}): {}",
                                                    err.error_type, err.message
                                                );
                                                error!("{}", msg);
                                                let _ = tx.send(BackendEvent::Error(msg));
                                                return;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                Err(e) => {
                                    debug!("Failed to parse Anthropic SSE event: {} -- data: {}", e, data);
                                }
                            }
                        }

                        buf.drain(..=pos);
                    }
                }
                Err(e) => {
                    error!("Anthropic stream error: {}", e);
                    for (_, acc) in accumulated_tool_calls.drain() {
                        if !acc.name.is_empty() {
                            let tool_call = ToolCall {
                                id: acc.id,
                                call_type: "function".to_string(),
                                function: FunctionCall {
                                    name: acc.name,
                                    arguments: if acc.partial_json.is_empty() {
                                        "{}".to_string()
                                    } else {
                                        acc.partial_json
                                    },
                                },
                            };
                            let _ = tx.send(BackendEvent::ToolCall(tool_call));
                        }
                    }
                    let _ = tx.send(BackendEvent::Error(format!("[Stream error: {}]", e)));
                    return;
                }
            }
        }

        let tool_count = accumulated_tool_calls.len();
        if tool_count > 0 {
            info!("Anthropic chat complete: {} tool calls", tool_count);
        } else {
            info!("Anthropic chat complete");
        }

        for (_, acc) in accumulated_tool_calls.drain() {
            if !acc.name.is_empty() {
                let tool_call = ToolCall {
                    id: acc.id,
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: acc.name,
                        arguments: if acc.partial_json.is_empty() {
                            "{}".to_string()
                        } else {
                            acc.partial_json
                        },
                    },
                };
                let _ = tx.send(BackendEvent::ToolCall(tool_call));
            }
        }

        let _ = tx.send(BackendEvent::Done);
    }

    async fn stream_gemini(
        resp: reqwest::Response,
        cancel_token: tokio_util::sync::CancellationToken,
        tx: mpsc::UnboundedSender<BackendEvent>,
    ) {
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut accumulated_tool_calls: HashMap<usize, (String, String, serde_json::Value)> =
            HashMap::new();
        let mut tool_call_index: usize = 0;

        loop {
            let chunk = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    let _ = tx.send(BackendEvent::Cancelled);
                    return;
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(c) => c,
                        None => break,
                    }
                }
            };

            match chunk {
                Ok(bytes) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line_bytes = if pos > 0 && buf[pos - 1] == b'\r' {
                            &buf[..pos - 1]
                        } else {
                            &buf[..pos]
                        };
                        let line = String::from_utf8_lossy(line_bytes);
                        let line = line.trim();

                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                break;
                            }

                            match serde_json::from_str::<GeminiStreamResponse>(data) {
                                Ok(response) => {
                                    if let Some(error) = response.error {
                                        let msg = format!(
                                            "Gemini API error ({}): {}",
                                            error.code, error.message
                                        );
                                        error!("{}", msg);
                                        let _ = tx.send(BackendEvent::Error(msg));
                                        return;
                                    }

                                    for candidate in response.candidates {
                                        for part in candidate.content.parts {
                                            match part {
                                                GeminiPartResponse::Text { text } => {
                                                    if !text.is_empty() {
                                                        let _ = tx.send(BackendEvent::Text(text));
                                                    }
                                                }
                                                GeminiPartResponse::FunctionCall {
                                                    function_call,
                                                } => {
                                                    let id = format!(
                                                        "gemini_{}_{}",
                                                        function_call.name,
                                                        tool_call_index
                                                    );
                                                    accumulated_tool_calls.insert(
                                                        tool_call_index,
                                                        (
                                                            id,
                                                            function_call.name,
                                                            function_call.args,
                                                        ),
                                                    );
                                                    tool_call_index += 1;
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    debug!(
                                        "Failed to parse Gemini SSE event: {} -- data: {}",
                                        e, data
                                    );
                                }
                            }
                        }

                        buf.drain(..=pos);
                    }
                }
                Err(e) => {
                    error!("Gemini stream error: {}", e);
                    for (_idx, (id, name, args)) in accumulated_tool_calls.drain() {
                        let args_str = serde_json::to_string(&args).unwrap_or_default();
                        let tool_call = ToolCall {
                            id,
                            call_type: "function".to_string(),
                            function: FunctionCall {
                                name,
                                arguments: args_str,
                            },
                        };
                        let _ = tx.send(BackendEvent::ToolCall(tool_call));
                    }
                    let _ = tx.send(BackendEvent::Error(format!("[Stream error: {}]", e)));
                    return;
                }
            }
        }

        let tool_count = accumulated_tool_calls.len();
        if tool_count > 0 {
            info!("Gemini chat complete: {} tool calls", tool_count);
        } else {
            info!("Gemini chat complete");
        }

        for (_, (id, name, args)) in accumulated_tool_calls {
            let args_str = serde_json::to_string(&args).unwrap_or_default();
            let tool_call = ToolCall {
                id,
                call_type: "function".to_string(),
                function: FunctionCall {
                    name,
                    arguments: args_str,
                },
            };
            let _ = tx.send(BackendEvent::ToolCall(tool_call));
        }

        let _ = tx.send(BackendEvent::Done);
    }

    // ========================================================================
    // Fetch models
    // ========================================================================

    pub async fn fetch_models(&self) -> Result<Vec<Model>> {
        match self.provider_kind {
            ProviderKind::Anthropic => self.fetch_models_anthropic().await,
            ProviderKind::Gemini => self.fetch_models_gemini().await,
            ProviderKind::OpenAiCompatible | ProviderKind::Ollama => {
                self.fetch_models_openai().await
            }
        }
    }

    async fn fetch_models_openai(&self) -> Result<Vec<Model>> {
        let url = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(self.api_key.expose_secret())
            .send()
            .await
            .context("Failed to fetch models")?;

        let openai_status = resp.status();
        if openai_status.is_success() {
            if let Ok(data) = resp.json::<ModelsResponse>().await {
                let mut models = data.data;
                for model in &mut models {
                    let (inferred_tools, inferred_voice) = Model::infer_capabilities(&model.id);
                    model.supports_tools = self.capabilities.tools || inferred_tools;
                    model.supports_voice = inferred_voice;
                }
                return Ok(models);
            }
        }

        let ollama_base = self.base_url.trim_end_matches("/v1");
        let ollama_url = format!("{}/api/tags", ollama_base);
        let ollama_resp = self
            .client
            .get(&ollama_url)
            .bearer_auth(self.api_key.expose_secret())
            .send()
            .await
            .context("Failed to fetch models from Ollama endpoint")?;

        let ollama_status = ollama_resp.status();
        if !ollama_status.is_success() {
            return Err(anyhow::anyhow!(
                "HTTP {} (OpenAI) / HTTP {} (Ollama)",
                openai_status,
                ollama_status
            ));
        }

        let ollama_data: OllamaModelsResponse = ollama_resp
            .json()
            .await
            .context("Failed to parse Ollama models response")?;

        let mut models: Vec<Model> = ollama_data
            .models
            .into_iter()
            .map(|m| Model {
                id: m.name,
                provider: ProviderId::new(""),
                supports_tools: self.capabilities.tools,
                supports_voice: false,
                local: true,
            })
            .collect();

        for model in &mut models {
            let (inferred_tools, inferred_voice) = Model::infer_capabilities(&model.id);
            model.supports_tools = self.capabilities.tools || inferred_tools;
            model.supports_voice = inferred_voice;
        }

        Ok(models)
    }

    async fn fetch_models_anthropic(&self) -> Result<Vec<Model>> {
        let url = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .context("Failed to fetch Anthropic models")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            error!("Anthropic models HTTP error: {} -- {}", status, body);
            return Err(anyhow::anyhow!(
                "Anthropic models HTTP {}: {}",
                status,
                if body.is_empty() {
                    "Unknown error".to_string()
                } else {
                    body
                }
            ));
        }

        let data: AnthropicModelsResponse = resp
            .json()
            .await
            .context("Failed to parse Anthropic models response")?;

        let mut models: Vec<Model> = data
            .data
            .into_iter()
            .map(|m| Model {
                id: m.id,
                provider: ProviderId::new(""),
                supports_tools: self.capabilities.tools,
                supports_voice: false,
                local: false,
            })
            .collect();

        for model in &mut models {
            let (inferred_tools, inferred_voice) = Model::infer_capabilities(&model.id);
            model.supports_tools = self.capabilities.tools || inferred_tools;
            model.supports_voice = inferred_voice;
        }

        Ok(models)
    }

    async fn fetch_models_gemini(&self) -> Result<Vec<Model>> {
        let url = format!(
            "{}/models?key={}",
            self.base_url.trim_end_matches('/'),
            self.api_key.expose_secret()
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Gemini models")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            error!("Gemini models HTTP error: {} -- {}", status, body);
            return Err(anyhow::anyhow!(
                "Gemini models HTTP {}: {}",
                status,
                if body.is_empty() {
                    "Unknown error".to_string()
                } else {
                    body
                }
            ));
        }

        let data: GeminiModelsResponse = resp
            .json()
            .await
            .context("Failed to parse Gemini models response")?;

        let mut models: Vec<Model> = data
            .models
            .into_iter()
            .map(|m| {
                let id = m.name.strip_prefix("models/").unwrap_or(&m.name).to_string();
                Model {
                    id,
                    provider: ProviderId::new(""),
                    supports_tools: self.capabilities.tools,
                    supports_voice: false,
                    local: false,
                }
            })
            .collect();

        for model in &mut models {
            let (inferred_tools, inferred_voice) = Model::infer_capabilities(&model.id);
            model.supports_tools = self.capabilities.tools || inferred_tools;
            model.supports_voice = inferred_voice;
        }

        Ok(models)
    }
}

// ============================================================================
// Provider trait implementation
// ============================================================================

impl Provider for MultiProvider {
    fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
        temperature: Option<f32>,
    ) -> mpsc::UnboundedReceiver<BackendEvent> {
        self.chat(
            model,
            messages,
            max_tokens,
            tools,
            cancel_token,
            temperature,
        )
    }

    fn fetch_models(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Model>>> + Send + '_>>
    {
        Box::pin(self.fetch_models())
    }

    fn health_check(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move { self.fetch_models().await.map(|_| ()) })
    }

    fn cancel_token(&self) -> tokio_util::sync::CancellationToken {
        tokio_util::sync::CancellationToken::new()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }
}

// ============================================================================
// Anthropic message conversion
// ============================================================================

fn convert_messages_anthropic(
    messages: Vec<Message>,
) -> (Option<String>, Vec<AnthropicMessage>) {
    let system_parts: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.clone())
        .collect();
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    let non_system: Vec<Message> = messages
        .into_iter()
        .filter(|m| m.role != Role::System)
        .collect();

    let mut result: Vec<AnthropicMessage> = Vec::new();

    for msg in non_system {
        match msg.role {
            Role::User => {
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.append_text(&msg.content);
                        continue;
                    }
                }
                result.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Text(msg.content),
                });
            }
            Role::Assistant => {
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .map(|t| !t.is_empty())
                    .unwrap_or(false);
                if has_tool_calls {
                    let mut blocks = Vec::new();
                    if !msg.content.is_empty() {
                        blocks.push(AnthropicContentBlock::Text {
                            text: msg.content,
                        });
                    }
                    if let Some(tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let input = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                            blocks.push(AnthropicContentBlock::ToolUse {
                                id: tc.id,
                                name: tc.function.name,
                                input,
                            });
                        }
                    }
                    if let Some(last) = result.last_mut() {
                        if last.role == "assistant" {
                            last.append_blocks(blocks);
                            continue;
                        }
                    }
                    result.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Blocks(blocks),
                    });
                } else {
                    if let Some(last) = result.last_mut() {
                        if last.role == "assistant" {
                            last.append_text(&msg.content);
                            continue;
                        }
                    }
                    result.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Text(msg.content),
                    });
                }
            }
            Role::Tool => {
                let block = AnthropicContentBlock::ToolResult {
                    tool_use_id: msg.tool_call_id.unwrap_or_default(),
                    content: msg.content,
                };
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.append_block(block);
                        continue;
                    }
                }
                result.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Blocks(vec![block]),
                });
            }
            Role::System => {}
        }
    }

    (system, result)
}

// ============================================================================
// Gemini message conversion
// ============================================================================

fn convert_messages_gemini(
    messages: Vec<Message>,
) -> (Option<GeminiContent>, Vec<GeminiContent>) {
    let system_parts: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.clone())
        .collect();
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text {
                text: system_parts.join("\n\n"),
            }],
        })
    };

    let non_system: Vec<Message> = messages
        .into_iter()
        .filter(|m| m.role != Role::System)
        .collect();

    let mut result: Vec<GeminiContent> = Vec::new();

    for msg in non_system {
        match msg.role {
            Role::User => {
                let parts = vec![GeminiPart::Text { text: msg.content }];
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.parts.extend(parts);
                        continue;
                    }
                }
                result.push(GeminiContent {
                    role: "user".to_string(),
                    parts,
                });
            }
            Role::Assistant => {
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .map(|t| !t.is_empty())
                    .unwrap_or(false);
                let mut parts = Vec::new();
                if !msg.content.is_empty() {
                    parts.push(GeminiPart::Text { text: msg.content });
                }
                if has_tool_calls {
                    if let Some(tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let args = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                            parts.push(GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    name: tc.function.name,
                                    args,
                                },
                            });
                        }
                    }
                }
                if let Some(last) = result.last_mut() {
                    if last.role == "model" {
                        last.parts.extend(parts);
                        continue;
                    }
                }
                result.push(GeminiContent {
                    role: "model".to_string(),
                    parts,
                });
            }
            Role::Tool => {
                let response = serde_json::json!({
                    "result": msg.content
                });
                let parts = vec![GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: msg.tool_call_id.clone().unwrap_or_default(),
                        response,
                    },
                }];
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.parts.extend(parts);
                        continue;
                    }
                }
                result.push(GeminiContent {
                    role: "user".to_string(),
                    parts,
                });
            }
            Role::System => {}
        }
    }

    (system, result)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // OpenAI tests
    // -------------------------------------------------------------------------

    #[test]
    fn message_serializes_correctly() {
        let msg = Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hello\""));
    }

    #[test]
    fn message_with_tool_calls_serializes() {
        let msg = Message {
            role: Role::Assistant,
            content: "".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: "test".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"tool_calls\""));
        assert!(json.contains("\"call_1\""));
    }

    #[test]
    fn chat_request_serializes_tools() {
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![ApiMessage {
                role: Role::User,
                content: "hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: true,
            max_tokens: Some(100),
            tools: Some(vec![ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: "search".to_string(),
                    description: "Search the web".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }]),
            extra_params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\":\"gpt-4\""));
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"search\""));
    }

    #[test]
    fn models_response_deserializes() {
        let json = r#"{"data":[{"id":"gpt-4"},{"id":"gpt-3.5"}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 2);
        assert_eq!(resp.data[0].id, "gpt-4");
    }

    #[test]
    fn sse_chunk_parses_text_delta() {
        let json = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        let chunk: ChatCompletionChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.choices[0].delta.content, Some("Hello".to_string()));
    }

    #[test]
    fn sse_chunk_parses_tool_call_delta() {
        let json = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"search"}}]}}]}"#;
        let chunk: ChatCompletionChunk = serde_json::from_str(json).unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 0);
        assert_eq!(tc.id, Some("call_1".to_string()));
        assert_eq!(
            tc.function.as_ref().unwrap().name,
            Some("search".to_string())
        );
    }

    #[test]
    fn chat_request_with_extra_params_serializes() {
        let mut extra = serde_json::Map::new();
        extra.insert("temperature".to_string(), serde_json::json!(0.7));
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![ApiMessage {
                role: Role::User,
                content: "hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: false,
            max_tokens: None,
            tools: None,
            extra_params: Some(extra),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"temperature\":0.7"));
    }

    #[test]
    fn chat_request_skips_null_fields() {
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![ApiMessage {
                role: Role::User,
                content: "hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
            }],
            stream: true,
            max_tokens: None,
            tools: None,
            extra_params: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("tools"));
    }

    // -------------------------------------------------------------------------
    // Anthropic conversion tests
    // -------------------------------------------------------------------------

    #[test]
    fn anthropic_convert_simple_user_assistant() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Assistant,
                content: "Hi there".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (system, converted) = convert_messages_anthropic(messages);
        assert!(system.is_none());
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "user");
        assert!(matches!(converted[0].content, AnthropicContent::Text(ref t) if t == "Hello"));
        assert_eq!(converted[1].role, "assistant");
        assert!(matches!(converted[1].content, AnthropicContent::Text(ref t) if t == "Hi there"));
    }

    #[test]
    fn anthropic_convert_extracts_system_prompt() {
        let messages = vec![
            Message {
                role: Role::System,
                content: "You are helpful".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (system, converted) = convert_messages_anthropic(messages);
        assert_eq!(system, Some("You are helpful".to_string()));
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
    }

    #[test]
    fn anthropic_convert_multiple_system_prompts_joined() {
        let messages = vec![
            Message {
                role: Role::System,
                content: "First".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::System,
                content: "Second".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::User,
                content: "Hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (system, _) = convert_messages_anthropic(messages);
        assert_eq!(system, Some("First\n\nSecond".to_string()));
    }

    #[test]
    fn anthropic_convert_merges_consecutive_user_messages() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "First".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::User,
                content: "Second".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 1);
        assert!(matches!(converted[0].content, AnthropicContent::Text(ref t) if t == "First\nSecond"));
    }

    #[test]
    fn anthropic_convert_tool_message_to_tool_result() {
        let messages = vec![
            Message {
                role: Role::Assistant,
                content: "".to_string(),
                tool_calls: Some(vec![ToolCall {
                    id: "tool_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "get_weather".to_string(),
                        arguments: r#"{"location":"NYC"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Tool,
                content: "Sunny".to_string(),
                tool_calls: None,
                tool_call_id: Some("tool_1".to_string()),
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "assistant");
        assert!(matches!(converted[0].content, AnthropicContent::Blocks(_)));

        assert_eq!(converted[1].role, "user");
        if let AnthropicContent::Blocks(ref blocks) = converted[1].content {
            assert_eq!(blocks.len(), 1);
            assert!(matches!(
                blocks[0],
                AnthropicContentBlock::ToolResult {
                    tool_use_id: ref id,
                    content: ref c,
                } if id == "tool_1" && c == "Sunny"
            ));
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn anthropic_convert_multiple_tool_results_grouped() {
        let messages = vec![
            Message {
                role: Role::Tool,
                content: "Result A".to_string(),
                tool_calls: None,
                tool_call_id: Some("tool_a".to_string()),
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Tool,
                content: "Result B".to_string(),
                tool_calls: None,
                tool_call_id: Some("tool_b".to_string()),
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
        if let AnthropicContent::Blocks(ref blocks) = converted[0].content {
            assert_eq!(blocks.len(), 2);
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn anthropic_convert_assistant_with_text_and_tool_use() {
        let messages = vec![Message {
            role: Role::Assistant,
            content: "Let me check".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "tu_1".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: "search".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "assistant");
        if let AnthropicContent::Blocks(ref blocks) = converted[0].content {
            assert_eq!(blocks.len(), 2);
            assert!(matches!(blocks[0], AnthropicContentBlock::Text { .. }));
            assert!(matches!(blocks[1], AnthropicContentBlock::ToolUse { .. }));
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn anthropic_request_serializes_correctly() {
        let req = AnthropicRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicContent::Text("Hello".to_string()),
            }],
            max_tokens: 1024,
            system: Some("You are helpful".to_string()),
            tools: Some(vec![AnthropicTool {
                name: "get_weather".to_string(),
                description: "Get weather".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }]),
            stream: true,
            temperature: Some(0.7),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\""));
        assert!(json.contains("claude-3-5-sonnet-20241022"));
        assert!(json.contains("\"system\""));
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"stream\":true"));
        assert!(json.contains("\"temperature\":0.7"));
    }

    #[test]
    fn anthropic_request_skips_null_system() {
        let req = AnthropicRequest {
            model: "claude-3-opus-20240229".to_string(),
            messages: vec![],
            max_tokens: 1024,
            system: None,
            tools: None,
            stream: false,
            temperature: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("system"));
        assert!(!json.contains("tools"));
        assert!(!json.contains("temperature"));
    }

    #[test]
    fn anthropic_sse_text_delta_parses() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "content_block_delta");
        let delta = event.delta.unwrap();
        assert_eq!(delta.delta_type, Some("text_delta".to_string()));
        assert_eq!(delta.text, Some("Hello".to_string()));
    }

    #[test]
    fn anthropic_sse_input_json_delta_parses() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\":\"test\"}"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "content_block_delta");
        let delta = event.delta.unwrap();
        assert_eq!(delta.delta_type, Some("input_json_delta".to_string()));
        assert_eq!(delta.partial_json, Some(r#"{"q":"test"}"#.to_string()));
    }

    #[test]
    fn anthropic_sse_tool_use_start_parses() {
        let data = r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_123","name":"search","input":{}}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "content_block_start");
        if let Some(AnthropicContentBlock::ToolUse { id, name, .. }) = event.content_block {
            assert_eq!(id, "toolu_123");
            assert_eq!(name, "search");
        } else {
            panic!("Expected ToolUse content block");
        }
    }

    #[test]
    fn anthropic_sse_error_event_parses() {
        let data = r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limit exceeded"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "error");
        let err = event.error.unwrap();
        assert_eq!(err.error_type, "rate_limit_error");
        assert_eq!(err.message, "Rate limit exceeded");
    }

    // -------------------------------------------------------------------------
    // Gemini conversion tests
    // -------------------------------------------------------------------------

    #[test]
    fn gemini_convert_simple_user_assistant() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Assistant,
                content: "Hi there".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (system, converted) = convert_messages_gemini(messages);
        assert!(system.is_none());
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "user");
        assert_eq!(converted[1].role, "model");
    }

    #[test]
    fn gemini_convert_extracts_system_prompt() {
        let messages = vec![
            Message {
                role: Role::System,
                content: "You are helpful".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (system, converted) = convert_messages_gemini(messages);
        assert!(system.is_some());
        let sys = system.unwrap();
        assert_eq!(sys.role, "user");
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
    }

    #[test]
    fn gemini_convert_merges_consecutive_user_messages() {
        let messages = vec![
            Message {
                role: Role::User,
                content: "First".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::User,
                content: "Second".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (_, converted) = convert_messages_gemini(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
        assert_eq!(converted[0].parts.len(), 2);
    }

    #[test]
    fn gemini_convert_assistant_with_tool_calls() {
        let messages = vec![Message {
            role: Role::Assistant,
            content: "Let me check".to_string(),
            tool_calls: Some(vec![ToolCall {
                id: "tu_1".to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: "search".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];
        let (_, converted) = convert_messages_gemini(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "model");
        assert_eq!(converted[0].parts.len(), 2); // text + functionCall
    }

    #[test]
    fn gemini_convert_tool_message_to_function_response() {
        let messages = vec![
            Message {
                role: Role::Assistant,
                content: "".to_string(),
                tool_calls: Some(vec![ToolCall {
                    id: "tool_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "get_weather".to_string(),
                        arguments: r#"{"location":"NYC"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Tool,
                content: "Sunny".to_string(),
                tool_calls: None,
                tool_call_id: Some("tool_1".to_string()),
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ];
        let (_, converted) = convert_messages_gemini(messages);
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "model");
        assert_eq!(converted[1].role, "user");
        assert_eq!(converted[1].parts.len(), 1);
        assert!(matches!(
            converted[1].parts[0],
            GeminiPart::FunctionResponse { .. }
        ));
    }

    #[test]
    fn gemini_request_serializes_correctly() {
        let req = GeminiRequest {
            system_instruction: Some(GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::Text {
                    text: "You are helpful".to_string(),
                }],
            }),
            contents: vec![GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart::Text {
                    text: "Hello".to_string(),
                }],
            }],
            generation_config: Some(GeminiGenerationConfig {
                max_output_tokens: 1024,
                temperature: Some(0.7),
            }),
            tools: Some(vec![GeminiTool {
                function_declarations: vec![GeminiFunctionDeclaration {
                    name: "get_weather".to_string(),
                    description: "Get weather".to_string(),
                    parameters: serde_json::json!({"type": "object"}),
                }],
            }]),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"systemInstruction\""));
        assert!(json.contains("\"contents\""));
        assert!(json.contains("\"generationConfig\""));
        assert!(json.contains("\"maxOutputTokens\""));
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"functionDeclarations\""));
    }

    #[test]
    fn gemini_request_skips_null_fields() {
        let req = GeminiRequest {
            system_instruction: None,
            contents: vec![],
            generation_config: None,
            tools: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("systemInstruction"));
        assert!(!json.contains("generationConfig"));
        assert!(!json.contains("tools"));
    }

    #[test]
    fn gemini_stream_response_parses_text() {
        let data = r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"}}]}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].content.parts.len(), 1);
        assert!(matches!(
            resp.candidates[0].content.parts[0],
            GeminiPartResponse::Text { ref text } if text == "Hello"
        ));
    }

    #[test]
    fn gemini_stream_response_parses_function_call() {
        let data = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_weather","args":{"location":"NYC"}}}]}}]}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert!(matches!(
            resp.candidates[0].content.parts[0],
            GeminiPartResponse::FunctionCall { ref function_call } if function_call.name == "get_weather"
        ));
    }

    #[test]
    fn gemini_models_response_deserializes() {
        let json = r#"{"models":[{"name":"models/gemini-1.5-pro"},{"name":"models/gemini-1.5-flash"}]}"#;
        let resp: GeminiModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.models.len(), 2);
        assert_eq!(resp.models[0].name, "models/gemini-1.5-pro");
    }

    #[test]
    fn gemini_error_response_parses() {
        let data = r#"{"error":{"code":400,"message":"Invalid request","status":"INVALID_ARGUMENT"}}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, 400);
        assert_eq!(err.message, "Invalid request");
    }

    #[test]
    fn multi_provider_backend_has_provider_kind() {
        let backend = MultiProvider::with_capabilities(
            "http://test".to_string(),
            crate::config::SecretString::new("sk-test".to_string()),
            ProviderKind::Anthropic,
            ProviderCapabilities::default(),
            None,
        );
        assert_eq!(backend.provider_kind, ProviderKind::Anthropic);
    }

    #[test]
    fn multi_provider_backend_capabilities_passthrough() {
        let mut caps = ProviderCapabilities::default();
        caps.tools = true;
        caps.streaming = true;
        let backend = MultiProvider::with_capabilities(
            "http://test".to_string(),
            crate::config::SecretString::new("sk-test".to_string()),
            ProviderKind::Gemini,
            caps,
            None,
        );
        let got = backend.capabilities();
        assert!(got.tools);
        assert!(got.streaming);
    }
}
