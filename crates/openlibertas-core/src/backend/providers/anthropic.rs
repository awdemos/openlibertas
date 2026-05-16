use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::backend::multi_provider::MultiProvider;
use crate::backend::types::*;
use crate::domain::*;

#[derive(Debug, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

impl AnthropicContent {
    fn append_text(&mut self, text: &str) {
        match self {
            AnthropicContent::Text(s) => {
                if !s.is_empty() { s.push('\n'); }
                s.push_str(text);
            }
            AnthropicContent::Blocks(blocks) => {
                if let Some(AnthropicContentBlock::Text { text: last }) = blocks.last_mut() {
                    last.push('\n');
                    last.push_str(text);
                } else {
                    blocks.push(AnthropicContentBlock::Text { text: text.to_string() });
                }
            }
        }
    }

    fn append_block(&mut self, block: AnthropicContentBlock) {
        match self {
            AnthropicContent::Text(s) => {
                let mut blocks = Vec::new();
                if !s.is_empty() { blocks.push(AnthropicContentBlock::Text { text: s.clone() }); }
                blocks.push(block);
                *self = AnthropicContent::Blocks(blocks);
            }
            AnthropicContent::Blocks(blocks) => { blocks.push(block); }
        }
    }

    fn append_blocks(&mut self, new_blocks: Vec<AnthropicContentBlock>) {
        for block in new_blocks { self.append_block(block); }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: serde_json::Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

impl AnthropicMessage {
    fn append_text(&mut self, text: &str) { self.content.append_text(text); }
    fn append_block(&mut self, block: AnthropicContentBlock) { self.content.append_block(block); }
    fn append_blocks(&mut self, blocks: Vec<AnthropicContentBlock>) { self.content.append_blocks(blocks); }
}

#[derive(Debug, Serialize, Clone)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicModelsResponse {
    pub data: Vec<AnthropicModelItem>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicModelItem {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub delta: Option<AnthropicDelta>,
    #[serde(default)]
    pub content_block: Option<AnthropicContentBlock>,
    #[serde(default)]
    pub error: Option<AnthropicApiError>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicDelta {
    #[serde(rename = "type")]
    pub delta_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

pub(crate) fn chat_anthropic(
    mp: &MultiProvider,
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    tools: Option<Vec<ToolDefinition>>,
    cancel_token: CancellationToken,
    temperature: Option<f32>,
) -> mpsc::UnboundedReceiver<BackendEvent> {
    let (system, anthropic_messages) = convert_messages_anthropic(messages);

    let tools_for_req = if mp.capabilities.tool_format.sends_native_tools() {
        tools.map(|defs| defs.into_iter().map(|d| AnthropicTool {
            name: d.function.name,
            description: d.function.description,
            input_schema: d.function.parameters,
        }).collect())
    } else {
        None
    };

    let mut extra = mp.extra_params.clone().unwrap_or_default();
    for key in &["model", "messages", "stream", "max_tokens", "tools", "system"] {
        extra.remove(*key);
    }
    if temperature.is_some() { extra.remove("temperature"); }

    let req = AnthropicRequest {
        model: model.clone(),
        messages: anthropic_messages.clone(),
        max_tokens,
        system,
        tools: tools_for_req,
        stream: true,
        temperature,
    };

    let tools_count = req.tools.as_ref().map(std::vec::Vec::len).unwrap_or(0);
    info!(
        "Anthropic chat request: model={}, messages={}, max_tokens={}, tools={} ({} definitions)",
        model, anthropic_messages.len(), max_tokens, req.tools.is_some(), tools_count
    );
    if let Some(ref tools) = req.tools {
        for tool in tools { debug!("Tool available: {} - {}", tool.name, tool.description); }
    }

    let (tx, rx) = mpsc::unbounded_channel();
    let client = mp.client.clone();
    let base_url = mp.base_url.clone();
    let api_key = mp.api_key.clone();
    let extra_params = if extra.is_empty() { None } else { Some(extra) };

    tokio::spawn(async move {
        let url = format!("{base_url}/messages");

        let mut req_json = match serde_json::to_value(&req) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize Anthropic request: {}", e);
                let _ = tx.send(BackendEvent::Error(format!("[Serialization error: {e}]")));
                return;
            }
        };

        if let Some(ref extra) = extra_params {
            if let Some(obj) = req_json.as_object_mut() {
                for (k, v) in extra { obj.insert(k.clone(), v.clone()); }
            }
        }

        let resp = crate::backend::multi_provider::send_with_retries(
            || async {
                client.post(&url)
                    .header("x-api-key", api_key.expose_secret())
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .json(&req_json)
                    .send().await
            },
            crate::backend::multi_provider::RetryConfig {
                retry_label: "Anthropic",
                error_label: "Anthropic chat",
            },
            &tx,
            |_| false,
        ).await;

        if let Some(resp) = resp {
            stream_anthropic(resp, cancel_token, tx).await;
        }
    });

    rx
}

pub(crate) async fn fetch_models_anthropic(mp: &MultiProvider) -> Result<Vec<Model>> {
    let url = format!("{}/models", mp.base_url);
    let resp = mp.client.get(&url)
        .header("x-api-key", mp.api_key.expose_secret())
        .header("anthropic-version", "2023-06-01")
        .send().await.context("Failed to fetch Anthropic models")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        error!("Anthropic models HTTP error: {} -- {}", status, body);
        return Err(anyhow::anyhow!("Anthropic models HTTP {}: {}", status, if body.is_empty() { "Unknown error".to_string() } else { body }));
    }

    let data: AnthropicModelsResponse = resp.json().await.context("Failed to parse Anthropic models response")?;

    let mut models: Vec<Model> = data.data.into_iter().map(|m| Model {
        id: m.id,
        provider: ProviderId::new(""),
        supports_tools: mp.capabilities.tools,
        supports_voice: false,
        local: false,
    }).collect();

    Model::apply_capabilities(&mut models, &mp.capabilities);

    Ok(models)
}

async fn stream_anthropic(
    resp: reqwest::Response,
    cancel_token: CancellationToken,
    tx: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut _current_event_type: Option<String> = None;

    struct AccumulatedToolCall { id: String, name: String, partial_json: String }
    let mut accumulated_tool_calls: HashMap<usize, AccumulatedToolCall> = HashMap::new();

    loop {
        let chunk = tokio::select! {
            biased;
            _ = cancel_token.cancelled() => { let _ = tx.send(BackendEvent::Cancelled); return; }
            chunk = stream.next() => { match chunk { Some(c) => c, None => break, } }
        };

        match chunk {
            Ok(bytes) => {
                buf.extend_from_slice(&bytes);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line_bytes = if pos > 0 && buf[pos - 1] == b'\r' { &buf[..pos - 1] } else { &buf[..pos] };
                    let line = String::from_utf8_lossy(line_bytes);
                    let line = line.trim();

                    if line.is_empty() { _current_event_type = None; }
                    else if let Some(et) = line.strip_prefix("event: ") { _current_event_type = Some(et.to_string()); }
                    else if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { break; }

                        match serde_json::from_str::<AnthropicStreamEvent>(data) {
                            Ok(event) => match event.event_type.as_str() {
                                "content_block_delta" => {
                                    if let Some(delta) = event.delta {
                                        match delta.delta_type.as_deref() {
                                            Some("text_delta") => {
                                                if let Some(text) = delta.text {
                                                    if !text.is_empty() { let _ = tx.send(BackendEvent::Text(text)); }
                                                }
                                            }
                                            Some("input_json_delta") => {
                                                if let Some(partial) = delta.partial_json {
                                                    let entry = accumulated_tool_calls.entry(event.index).or_insert_with(|| AccumulatedToolCall {
                                                        id: String::new(), name: String::new(), partial_json: String::new(),
                                                    });
                                                    entry.partial_json.push_str(&partial);
                                                }
                                            }
                                            Some("thinking_delta") => {
                                                if let Some(text) = delta.text {
                                                    if !text.is_empty() { let _ = tx.send(BackendEvent::Reasoning(text)); }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_start" => {
                                    if let Some(AnthropicContentBlock::ToolUse { id, name, .. }) = event.content_block {
                                        let entry = accumulated_tool_calls.entry(event.index).or_insert_with(|| AccumulatedToolCall {
                                            id: String::new(), name: String::new(), partial_json: String::new(),
                                        });
                                        entry.id = id;
                                        entry.name = name;
                                    }
                                }
                                "message_stop" => { break; }
                                "error" => {
                                    if let Some(err) = event.error {
                                        let msg = format!("Anthropic API error ({}): {}", err.error_type, err.message);
                                        error!("{}", msg);
                                        let _ = tx.send(BackendEvent::Error(msg));
                                        return;
                                    }
                                }
                                _ => {}
                            },
                            Err(e) => { debug!("Failed to parse Anthropic SSE event: {} -- data: {}", e, data); }
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
                                arguments: if acc.partial_json.is_empty() { "{}".to_string() } else { acc.partial_json },
                            },
                        };
                        let _ = tx.send(BackendEvent::ToolCall(tool_call));
                    }
                }
                let _ = tx.send(BackendEvent::Error(format!("[Stream error: {e}]")));
                return;
            }
        }
    }

    let tool_count = accumulated_tool_calls.len();
    if tool_count > 0 { info!("Anthropic chat complete: {} tool calls", tool_count); } else { info!("Anthropic chat complete"); }

    for (_, acc) in accumulated_tool_calls.drain() {
        if !acc.name.is_empty() {
            let tool_call = ToolCall {
                id: acc.id,
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: acc.name,
                    arguments: if acc.partial_json.is_empty() { "{}".to_string() } else { acc.partial_json },
                },
            };
            let _ = tx.send(BackendEvent::ToolCall(tool_call));
        }
    }

    let _ = tx.send(BackendEvent::Done);
}

pub fn convert_messages_anthropic(messages: Vec<Message>) -> (Option<String>, Vec<AnthropicMessage>) {
    let system_parts: Vec<String> = messages.iter().filter(|m| m.role == Role::System).map(|m| m.content.clone()).collect();
    let system = if system_parts.is_empty() { None } else { Some(system_parts.join("\n\n")) };

    let non_system: Vec<Message> = messages.into_iter().filter(|m| m.role != Role::System).collect();
    let mut result: Vec<AnthropicMessage> = Vec::new();

    for msg in non_system {
        match msg.role {
            Role::User => {
                if let Some(last) = result.last_mut() {
                    if last.role == "user" { last.append_text(&msg.content); continue; }
                }
                result.push(AnthropicMessage { role: "user".to_string(), content: AnthropicContent::Text(msg.content) });
            }
            Role::Assistant => {
                let has_tool_calls = msg.tool_calls.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
                if has_tool_calls {
                    let mut blocks = Vec::new();
                    if !msg.content.is_empty() { blocks.push(AnthropicContentBlock::Text { text: msg.content }); }
                    if let Some(tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let input = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                            blocks.push(AnthropicContentBlock::ToolUse { id: tc.id, name: tc.function.name, input });
                        }
                    }
                    if let Some(last) = result.last_mut() {
                        if last.role == "assistant" { last.append_blocks(blocks); continue; }
                    }
                    result.push(AnthropicMessage { role: "assistant".to_string(), content: AnthropicContent::Blocks(blocks) });
                } else {
                    if let Some(last) = result.last_mut() {
                        if last.role == "assistant" { last.append_text(&msg.content); continue; }
                    }
                    result.push(AnthropicMessage { role: "assistant".to_string(), content: AnthropicContent::Text(msg.content) });
                }
            }
            Role::Tool => {
                let block = AnthropicContentBlock::ToolResult { tool_use_id: msg.tool_call_id.unwrap_or_default(), content: msg.content };
                if let Some(last) = result.last_mut() {
                    if last.role == "user" { last.append_block(block); continue; }
                }
                result.push(AnthropicMessage { role: "user".to_string(), content: AnthropicContent::Blocks(vec![block]) });
            }
            Role::System => {}
        }
    }

    (system, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_convert_simple_user_assistant() {
        let messages = vec![
            Message { role: Role::User, content: "Hello".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::Assistant, content: "Hi there".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
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
            Message { role: Role::System, content: "You are helpful".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::User, content: "Hello".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (system, converted) = convert_messages_anthropic(messages);
        assert_eq!(system, Some("You are helpful".to_string()));
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
    }

    #[test]
    fn anthropic_convert_multiple_system_prompts_joined() {
        let messages = vec![
            Message { role: Role::System, content: "First".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::System, content: "Second".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::User, content: "Hi".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (system, _) = convert_messages_anthropic(messages);
        assert_eq!(system, Some("First\n\nSecond".to_string()));
    }

    #[test]
    fn anthropic_convert_merges_consecutive_user_messages() {
        let messages = vec![
            Message { role: Role::User, content: "First".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::User, content: "Second".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 1);
        assert!(matches!(converted[0].content, AnthropicContent::Text(ref t) if t == "First\nSecond"));
    }

    #[test]
    fn anthropic_convert_tool_message_to_tool_result() {
        let messages = vec![
            Message { role: Role::Assistant, content: "".to_string(), tool_calls: Some(vec![ToolCall {
                id: "tool_1".to_string(), call_type: "function".to_string(),
                function: FunctionCall { name: "get_weather".to_string(), arguments: r##"{"location":"NYC"}"##.to_string() },
            }]), tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::Tool, content: "Sunny".to_string(), tool_calls: None, tool_call_id: Some("tool_1".to_string()), timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "assistant");
        assert!(matches!(converted[0].content, AnthropicContent::Blocks(_)));
        assert_eq!(converted[1].role, "user");
        if let AnthropicContent::Blocks(ref blocks) = converted[1].content {
            assert_eq!(blocks.len(), 1);
            assert!(matches!(blocks[0], AnthropicContentBlock::ToolResult { tool_use_id: ref id, content: ref c } if id == "tool_1" && c == "Sunny"));
        } else { panic!("Expected Blocks content"); }
    }

    #[test]
    fn anthropic_convert_multiple_tool_results_grouped() {
        let messages = vec![
            Message { role: Role::Tool, content: "Result A".to_string(), tool_calls: None, tool_call_id: Some("tool_a".to_string()), timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::Tool, content: "Result B".to_string(), tool_calls: None, tool_call_id: Some("tool_b".to_string()), timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
        if let AnthropicContent::Blocks(ref blocks) = converted[0].content {
            assert_eq!(blocks.len(), 2);
        } else { panic!("Expected Blocks content"); }
    }

    #[test]
    fn anthropic_convert_assistant_with_text_and_tool_use() {
        let messages = vec![Message { role: Role::Assistant, content: "Let me check".to_string(), tool_calls: Some(vec![ToolCall {
            id: "tu_1".to_string(), call_type: "function".to_string(),
            function: FunctionCall { name: "search".to_string(), arguments: "{}".to_string() },
        }]), tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false }];
        let (_, converted) = convert_messages_anthropic(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "assistant");
        if let AnthropicContent::Blocks(ref blocks) = converted[0].content {
            assert_eq!(blocks.len(), 2);
            assert!(matches!(blocks[0], AnthropicContentBlock::Text { .. }));
            assert!(matches!(blocks[1], AnthropicContentBlock::ToolUse { .. }));
        } else { panic!("Expected Blocks content"); }
    }

    #[test]
    fn anthropic_request_serializes_correctly() {
        let req = AnthropicRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![AnthropicMessage { role: "user".to_string(), content: AnthropicContent::Text("Hello".to_string()) }],
            max_tokens: 1024,
            system: Some("You are helpful".to_string()),
            tools: Some(vec![AnthropicTool { name: "get_weather".to_string(), description: "Get weather".to_string(), input_schema: serde_json::json!({"type": "object"}) }]),
            stream: true,
            temperature: Some(0.7),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r##""model""##));
        assert!(json.contains("claude-3-5-sonnet-20241022"));
        assert!(json.contains(r##""system""##));
        assert!(json.contains(r##""tools""##));
        assert!(json.contains(r##""stream":true"##));
        assert!(json.contains(r##""temperature":0.7"##));
    }

    #[test]
    fn anthropic_request_skips_null_system() {
        let req = AnthropicRequest {
            model: "claude-3-opus-20240229".to_string(), messages: vec![], max_tokens: 1024,
            system: None, tools: None, stream: false, temperature: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("system"));
        assert!(!json.contains("tools"));
        assert!(!json.contains("temperature"));
    }

    #[test]
    fn anthropic_sse_text_delta_parses() {
        let data = r##"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"##;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "content_block_delta");
        let delta = event.delta.unwrap();
        assert_eq!(delta.delta_type, Some("text_delta".to_string()));
        assert_eq!(delta.text, Some("Hello".to_string()));
    }

    #[test]
    fn anthropic_sse_input_json_delta_parses() {
        let data = r##"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\":\"test\"}"}}"##;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "content_block_delta");
        let delta = event.delta.unwrap();
        assert_eq!(delta.delta_type, Some("input_json_delta".to_string()));
        assert_eq!(delta.partial_json, Some(r##"{"q":"test"}"##.to_string()));
    }

    #[test]
    fn anthropic_sse_tool_use_start_parses() {
        let data = r##"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_123","name":"search","input":{}}}"##;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "content_block_start");
        if let Some(AnthropicContentBlock::ToolUse { id, name, .. }) = event.content_block {
            assert_eq!(id, "toolu_123");
            assert_eq!(name, "search");
        } else { panic!("Expected ToolUse content block"); }
    }

    #[test]
    fn anthropic_sse_error_event_parses() {
        let data = r##"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limit exceeded"}}"##;
        let event: AnthropicStreamEvent = serde_json::from_str(data).unwrap();
        assert_eq!(event.event_type, "error");
        let err = event.error.unwrap();
        assert_eq!(err.error_type, "rate_limit_error");
        assert_eq!(err.message, "Rate limit exceeded");
    }
}
