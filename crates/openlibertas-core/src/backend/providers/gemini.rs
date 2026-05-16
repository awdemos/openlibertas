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
#[serde(rename_all = "camelCase")]
pub struct GeminiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<GeminiTool>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum GeminiPart {
    Text { text: String },
    FunctionCall { #[serde(rename = "functionCall")] function_call: GeminiFunctionCall },
    FunctionResponse { #[serde(rename = "functionResponse")] function_response: GeminiFunctionResponse },
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiFunctionCall {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Serialize, Clone)]
pub struct GeminiFunctionResponse {
    pub name: String,
    pub response: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    pub max_output_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiTool {
    pub function_declarations: Vec<GeminiFunctionDeclaration>,
}

#[derive(Debug, Serialize)]
pub struct GeminiFunctionDeclaration {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct GeminiStreamResponse {
    #[serde(default)]
    pub candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    pub error: Option<GeminiErrorDetail>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiCandidate {
    pub content: GeminiContentResponse,
}

#[derive(Debug, Deserialize)]
pub struct GeminiContentResponse {
    #[serde(default)]
    pub parts: Vec<GeminiPartResponse>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum GeminiPartResponse {
    Text { text: String },
    FunctionCall { #[serde(rename = "functionCall")] function_call: GeminiFunctionCallResponse },
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeminiFunctionCallResponse {
    pub name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct GeminiErrorDetail {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct GeminiModelsResponse {
    pub models: Vec<GeminiModelItem>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiModelItem {
    pub name: String,
}

pub(crate) fn chat_gemini(
    mp: &MultiProvider,
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    tools: Option<Vec<ToolDefinition>>,
    cancel_token: CancellationToken,
    temperature: Option<f32>,
) -> mpsc::UnboundedReceiver<BackendEvent> {
    let (system, gemini_contents) = convert_messages_gemini(messages);

    let tools_for_req = if mp.capabilities.tool_format.sends_native_tools() {
        tools.map(|defs| vec![GeminiTool {
            function_declarations: defs.into_iter().map(|d| GeminiFunctionDeclaration {
                name: d.function.name,
                description: d.function.description,
                parameters: d.function.parameters,
            }).collect(),
        }])
    } else {
        None
    };

    let mut extra = mp.extra_params.clone().unwrap_or_default();
    for key in &["model", "contents", "systemInstruction", "generationConfig", "tools"] {
        extra.remove(*key);
    }
    if temperature.is_some() { extra.remove("temperature"); }

    let req = GeminiRequest {
        system_instruction: system,
        contents: gemini_contents.clone(),
        generation_config: Some(GeminiGenerationConfig { max_output_tokens: max_tokens, temperature }),
        tools: tools_for_req,
    };

    let tools_count = req.tools.as_ref().map(|t| t[0].function_declarations.len()).unwrap_or(0);
    info!(
        "Gemini chat request: model={}, messages={}, max_tokens={}, tools={} ({} definitions)",
        model, gemini_contents.len(), max_tokens, req.tools.is_some(), tools_count
    );
    if let Some(ref tools) = req.tools {
        for tool in &tools[0].function_declarations { debug!("Tool available: {} - {}", tool.name, tool.description); }
    }

    let (tx, rx) = mpsc::unbounded_channel();
    let client = mp.client.clone();
    let base_url = mp.base_url.clone();
    let api_key = mp.api_key.clone();
    let model = model.clone();
    let extra_params = if extra.is_empty() { None } else { Some(extra) };

    tokio::spawn(async move {
        let url = format!("{}/models/{}:streamGenerateContent?key={}", base_url.trim_end_matches('/'), model, api_key.expose_secret());

        let mut req_json = match serde_json::to_value(&req) {
            Ok(v) => v,
            Err(e) => {
                error!("Failed to serialize Gemini request: {}", e);
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
                client.post(&url).header("content-type", "application/json").json(&req_json).send().await
            },
            crate::backend::multi_provider::RetryConfig {
                retry_label: "Gemini",
                error_label: "Gemini chat",
            },
            &tx,
            |_| false,
        ).await;

        if let Some(resp) = resp {
            stream_gemini(resp, cancel_token, tx).await;
        }
    });

    rx
}

pub(crate) async fn fetch_models_gemini(mp: &MultiProvider) -> Result<Vec<Model>> {
    let url = format!("{}/models?key={}", mp.base_url.trim_end_matches('/'), mp.api_key.expose_secret());
    let resp = mp.client.get(&url).send().await.context("Failed to fetch Gemini models")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        error!("Gemini models HTTP error: {} -- {}", status, body);
        return Err(anyhow::anyhow!("Gemini models HTTP {}: {}", status, if body.is_empty() { "Unknown error".to_string() } else { body }));
    }

    let data: GeminiModelsResponse = resp.json().await.context("Failed to parse Gemini models response")?;

    let mut models: Vec<Model> = data.models.into_iter().map(|m| {
        let id = m.name.strip_prefix("models/").unwrap_or(&m.name).to_string();
        Model { id, provider: ProviderId::new(""), supports_tools: mp.capabilities.tools, supports_voice: false, local: false }
    }).collect();

    Model::apply_capabilities(&mut models, &mp.capabilities);

    Ok(models)
}

async fn stream_gemini(
    resp: reqwest::Response,
    cancel_token: CancellationToken,
    tx: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut accumulated_tool_calls: HashMap<usize, (String, String, serde_json::Value)> = HashMap::new();
    let mut tool_call_index: usize = 0;

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

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { break; }

                        match serde_json::from_str::<GeminiStreamResponse>(data) {
                            Ok(response) => {
                                if let Some(error) = response.error {
                                    let msg = format!("Gemini API error ({}): {}", error.code, error.message);
                                    error!("{}", msg);
                                    let _ = tx.send(BackendEvent::Error(msg));
                                    return;
                                }

                                for candidate in response.candidates {
                                    for part in candidate.content.parts {
                                        match part {
                                            GeminiPartResponse::Text { text } => {
                                                if !text.is_empty() { let _ = tx.send(BackendEvent::Text(text)); }
                                            }
                                            GeminiPartResponse::FunctionCall { function_call } => {
                                                let id = format!("gemini_{}_{}", function_call.name, tool_call_index);
                                                accumulated_tool_calls.insert(tool_call_index, (id, function_call.name, function_call.args));
                                                tool_call_index += 1;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => { debug!("Failed to parse Gemini SSE event: {} -- data: {}", e, data); }
                        }
                    }

                    buf.drain(..=pos);
                }
            }
            Err(e) => {
                error!("Gemini stream error: {}", e);
                for (_idx, (id, name, args)) in accumulated_tool_calls.drain() {
                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                    let tool_call = ToolCall { id, call_type: "function".to_string(), function: FunctionCall { name, arguments: args_str } };
                    let _ = tx.send(BackendEvent::ToolCall(tool_call));
                }
                let _ = tx.send(BackendEvent::Error(format!("[Stream error: {e}]")));
                return;
            }
        }
    }

    let tool_count = accumulated_tool_calls.len();
    if tool_count > 0 { info!("Gemini chat complete: {} tool calls", tool_count); } else { info!("Gemini chat complete"); }

    for (_, (id, name, args)) in accumulated_tool_calls {
        let args_str = serde_json::to_string(&args).unwrap_or_default();
        let tool_call = ToolCall { id, call_type: "function".to_string(), function: FunctionCall { name, arguments: args_str } };
        let _ = tx.send(BackendEvent::ToolCall(tool_call));
    }

    let _ = tx.send(BackendEvent::Done);
}

pub fn convert_messages_gemini(messages: Vec<Message>) -> (Option<GeminiContent>, Vec<GeminiContent>) {
    let system_parts: Vec<String> = messages.iter().filter(|m| m.role == Role::System).map(|m| m.content.clone()).collect();
    let system = if system_parts.is_empty() { None } else {
        Some(GeminiContent { role: "user".to_string(), parts: vec![GeminiPart::Text { text: system_parts.join("\n\n") }] })
    };

    let non_system: Vec<Message> = messages.into_iter().filter(|m| m.role != Role::System).collect();
    let mut result: Vec<GeminiContent> = Vec::new();

    for msg in non_system {
        match msg.role {
            Role::User => {
                let parts = vec![GeminiPart::Text { text: msg.content }];
                if let Some(last) = result.last_mut() {
                    if last.role == "user" { last.parts.extend(parts); continue; }
                }
                result.push(GeminiContent { role: "user".to_string(), parts });
            }
            Role::Assistant => {
                let has_tool_calls = msg.tool_calls.as_ref().map(|t| !t.is_empty()).unwrap_or(false);
                let mut parts = Vec::new();
                if !msg.content.is_empty() { parts.push(GeminiPart::Text { text: msg.content }); }
                if has_tool_calls {
                    if let Some(tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let args = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                            parts.push(GeminiPart::FunctionCall { function_call: GeminiFunctionCall { name: tc.function.name, args } });
                        }
                    }
                }
                if let Some(last) = result.last_mut() {
                    if last.role == "model" { last.parts.extend(parts); continue; }
                }
                result.push(GeminiContent { role: "model".to_string(), parts });
            }
            Role::Tool => {
                let response = serde_json::json!({ "result": msg.content });
                let parts = vec![GeminiPart::FunctionResponse { function_response: GeminiFunctionResponse { name: msg.tool_call_id.clone().unwrap_or_default(), response } }];
                if let Some(last) = result.last_mut() {
                    if last.role == "user" { last.parts.extend(parts); continue; }
                }
                result.push(GeminiContent { role: "user".to_string(), parts });
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
    fn gemini_convert_simple_user_assistant() {
        let messages = vec![
            Message { role: Role::User, content: "Hello".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::Assistant, content: "Hi there".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
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
            Message { role: Role::System, content: "You are helpful".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::User, content: "Hello".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
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
            Message { role: Role::User, content: "First".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::User, content: "Second".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (_, converted) = convert_messages_gemini(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "user");
        assert_eq!(converted[0].parts.len(), 2);
    }

    #[test]
    fn gemini_convert_assistant_with_tool_calls() {
        let messages = vec![Message { role: Role::Assistant, content: "Let me check".to_string(), tool_calls: Some(vec![ToolCall {
            id: "tu_1".to_string(), call_type: "function".to_string(),
            function: FunctionCall { name: "search".to_string(), arguments: "{}".to_string() },
        }]), tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false }];
        let (_, converted) = convert_messages_gemini(messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, "model");
        assert_eq!(converted[0].parts.len(), 2);
    }

    #[test]
    fn gemini_convert_tool_message_to_function_response() {
        let messages = vec![
            Message { role: Role::Assistant, content: "".to_string(), tool_calls: Some(vec![ToolCall {
                id: "tool_1".to_string(), call_type: "function".to_string(),
                function: FunctionCall { name: "get_weather".to_string(), arguments: r##"{"location":"NYC"}"##.to_string() },
            }]), tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
            Message { role: Role::Tool, content: "Sunny".to_string(), tool_calls: None, tool_call_id: Some("tool_1".to_string()), timestamp: None, reasoning_content: None, is_prompt: false },
        ];
        let (_, converted) = convert_messages_gemini(messages);
        assert_eq!(converted.len(), 2);
        assert_eq!(converted[0].role, "model");
        assert_eq!(converted[1].role, "user");
        assert_eq!(converted[1].parts.len(), 1);
        assert!(matches!(converted[1].parts[0], GeminiPart::FunctionResponse { .. }));
    }

    #[test]
    fn gemini_request_serializes_correctly() {
        let req = GeminiRequest {
            system_instruction: Some(GeminiContent { role: "user".to_string(), parts: vec![GeminiPart::Text { text: "You are helpful".to_string() }] }),
            contents: vec![GeminiContent { role: "user".to_string(), parts: vec![GeminiPart::Text { text: "Hello".to_string() }] }],
            generation_config: Some(GeminiGenerationConfig { max_output_tokens: 1024, temperature: Some(0.7) }),
            tools: Some(vec![GeminiTool { function_declarations: vec![GeminiFunctionDeclaration { name: "get_weather".to_string(), description: "Get weather".to_string(), parameters: serde_json::json!({"type": "object"}) }] }]),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r##""systemInstruction""##));
        assert!(json.contains(r##""contents""##));
        assert!(json.contains(r##""generationConfig""##));
        assert!(json.contains(r##""maxOutputTokens""##));
        assert!(json.contains(r##""tools""##));
        assert!(json.contains(r##""functionDeclarations""##));
    }

    #[test]
    fn gemini_request_skips_null_fields() {
        let req = GeminiRequest { system_instruction: None, contents: vec![], generation_config: None, tools: None };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("systemInstruction"));
        assert!(!json.contains("generationConfig"));
        assert!(!json.contains("tools"));
    }

    #[test]
    fn gemini_stream_response_parses_text() {
        let data = r##"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"}}]}"##;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].content.parts.len(), 1);
        assert!(matches!(resp.candidates[0].content.parts[0], GeminiPartResponse::Text { ref text } if text == "Hello"));
    }

    #[test]
    fn gemini_stream_response_parses_function_call() {
        let data = r##"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_weather","args":{"location":"NYC"}}}]}}]}"##;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert_eq!(resp.candidates.len(), 1);
        assert!(matches!(resp.candidates[0].content.parts[0], GeminiPartResponse::FunctionCall { ref function_call } if function_call.name == "get_weather"));
    }

    #[test]
    fn gemini_models_response_deserializes() {
        let json = r##"{"models":[{"name":"models/gemini-1.5-pro"},{"name":"models/gemini-1.5-flash"}]}"##;
        let resp: GeminiModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.models.len(), 2);
        assert_eq!(resp.models[0].name, "models/gemini-1.5-pro");
    }

    #[test]
    fn gemini_error_response_parses() {
        let data = r##"{"error":{"code":400,"message":"Invalid request","status":"INVALID_ARGUMENT"}}"##;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, 400);
        assert_eq!(err.message, "Invalid request");
    }
}
