use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::backend::multi_provider::MultiProvider;
use crate::backend::types::*;
use crate::domain::*;

pub(crate) fn chat_openai(
    mp: &MultiProvider,
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    tools: Option<Vec<ToolDefinition>>,
    cancel_token: CancellationToken,
    temperature: Option<f32>,
) -> mpsc::UnboundedReceiver<BackendEvent> {
    let mut extra_params = mp.extra_params.clone().unwrap_or_default();
    if let Some(temp) = temperature {
        extra_params.insert("temperature".to_string(), serde_json::json!(temp));
    }
    for key in &["model", "messages", "stream", "max_tokens", "tools"] {
        extra_params.remove(*key);
    }
    let msg_count = messages.len();
    let api_messages: Vec<ApiMessage> = messages.into_iter().map(Into::into).collect();
    let tools_for_req = if mp.tool_format.sends_native_tools() {
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
    let client = mp.client.clone();
    let base_url = mp.base_url.clone();
    let api_key = mp.api_key.clone();
    let tool_format = mp.tool_format;

    let tools_count = req.tools.as_ref().map(std::vec::Vec::len).unwrap_or(0);
    info!(
        "Chat request: model={}, messages={}, max_tokens={}, tools={} ({} definitions)",
        model, msg_count, max_tokens, req.tools.is_some(), tools_count
    );
    if let Some(ref tools) = req.tools {
        for tool in tools {
            debug!("Tool available: {} - {}", tool.function.name, tool.function.description);
        }
    }

    tokio::spawn(async move {
        let url = format!("{base_url}/chat/completions");

        let resp = crate::backend::multi_provider::send_with_retries(
            || async {
                client.post(&url).bearer_auth(api_key.expose_secret()).json(&req).send().await
            },
            crate::backend::multi_provider::RetryConfig {
                retry_label: "Chat",
                error_label: "Chat",
            },
            &tx,
            |s| s == 404,
        ).await;

        if let Some(resp) = resp {
            if resp.status().as_u16() == 404 {
                let ollama_base = base_url.trim_end_matches("/v1");
                let ollama_url = format!("{ollama_base}/api/chat");
                let ollama_req = OllamaChatRequest {
                    model: model.clone(),
                    messages: api_messages,
                    stream: true,
                    options: Some(OllamaOptions {
                        num_predict: if max_tokens > 0 { Some(max_tokens as i32) } else { None },
                        temperature,
                    }),
                    tools: if tool_format.sends_native_tools() { tools } else { None },
                };
                info!("Falling back to Ollama native API: {}", ollama_url);
                match client.post(&ollama_url).bearer_auth(api_key.expose_secret()).json(&ollama_req).send().await {
                    Ok(r) => {
                        let status = r.status();
                        if !status.is_success() {
                            let body = r.text().await.unwrap_or_default();
                            error!("Ollama chat HTTP error: {} -- {}", status, body);
                            let _ = tx.send(BackendEvent::Error(format!("[Ollama HTTP {}: {}]", status, if body.is_empty() { "Unknown error".to_string() } else { body })));
                            return;
                        }
                        stream_ollama(r, cancel_token, tx).await;
                    }
                    Err(e) => {
                        error!("Ollama chat connection failed: {}", e);
                        let _ = tx.send(BackendEvent::Error(format!("[Ollama error: {e}]")));
                    }
                }
            } else {
                stream_openai(resp, cancel_token, tx).await;
            }
        }
    });

    rx
}

pub(crate) async fn fetch_models_openai(mp: &MultiProvider) -> Result<Vec<Model>> {
    let url = format!("{}/models", mp.base_url);
    let resp = mp.client.get(&url).bearer_auth(mp.api_key.expose_secret()).send().await.context("Failed to fetch models")?;

    let openai_status = resp.status();
    if openai_status.is_success() {
        if let Ok(data) = resp.json::<ModelsResponse>().await {
            let mut models = data.data;
            Model::apply_capabilities(&mut models, &mp.capabilities);
            return Ok(models);
        }
    }

    if openai_status.as_u16() != 404 {
        return Err(anyhow::anyhow!("HTTP {openai_status} from models endpoint"));
    }

    let ollama_base = mp.base_url.trim_end_matches("/v1");
    let ollama_url = format!("{ollama_base}/api/tags");
    let ollama_resp = mp.client.get(&ollama_url).bearer_auth(mp.api_key.expose_secret()).send().await.context("Failed to fetch models from Ollama endpoint")?;

    let ollama_status = ollama_resp.status();
    if !ollama_status.is_success() {
        return Err(anyhow::anyhow!("HTTP {ollama_status} from Ollama models endpoint"));
    }

    let ollama_data: OllamaModelsResponse = ollama_resp.json().await.context("Failed to parse Ollama models response")?;

    let mut models: Vec<Model> = ollama_data.models.into_iter().map(|m| Model {
        id: m.name,
        provider: ProviderId::new(""),
        supports_tools: mp.capabilities.tools,
        supports_voice: false,
        local: true,
    }).collect();

    Model::apply_capabilities(&mut models, &mp.capabilities);

    Ok(models)
}

async fn stream_openai(
    resp: reqwest::Response,
    cancel_token: CancellationToken,
    tx: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut accumulated_tool_calls: HashMap<usize, ToolCall> = HashMap::new();

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
                        if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) {
                            if let Some(choice) = chunk.choices.first() {
                                if let Some(content) = &choice.delta.content {
                                    if !content.is_empty() { let _ = tx.send(BackendEvent::Text(content.clone())); }
                                }
                                if let Some(reasoning) = &choice.delta.reasoning_content {
                                    if !reasoning.is_empty() { let _ = tx.send(BackendEvent::Reasoning(reasoning.clone())); }
                                } else if let Some(thinking) = &choice.delta.thinking {
                                    if !thinking.is_empty() { let _ = tx.send(BackendEvent::Reasoning(thinking.clone())); }
                                }
                                if let Some(tool_calls) = &choice.delta.tool_calls {
                                    for tc in tool_calls {
                                        let entry = accumulated_tool_calls.entry(tc.index).or_insert_with(|| ToolCall {
                                            id: tc.id.clone().unwrap_or_default(),
                                            call_type: tc.call_type.clone().unwrap_or_else(|| "function".to_string()),
                                            function: FunctionCall { name: String::new(), arguments: String::new() },
                                        });
                                        if let Some(id) = &tc.id { entry.id = id.clone(); }
                                        if let Some(func) = &tc.function {
                                            if let Some(name) = &func.name { entry.function.name.push_str(name); }
                                            if let Some(args) = &func.arguments { entry.function.arguments.push_str(args); }
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
                    info!("Stream error recovery: sending {} accumulated tool calls", tool_count);
                    for (_, tool_call) in accumulated_tool_calls { let _ = tx.send(BackendEvent::ToolCall(tool_call)); }
                }
                error!("Chat stream error: {}", e);
                let _ = tx.send(BackendEvent::Error(format!("[Stream error: {e}]")));
                return;
            }
        }
    }

    let tool_count = accumulated_tool_calls.len();
    if tool_count > 0 { info!("Chat complete: {} tool calls", tool_count); } else { info!("Chat complete"); }
    for (_, tool_call) in accumulated_tool_calls { let _ = tx.send(BackendEvent::ToolCall(tool_call)); }
    let _ = tx.send(BackendEvent::Done);
}

async fn stream_ollama(
    resp: reqwest::Response,
    cancel_token: CancellationToken,
    tx: mpsc::UnboundedSender<BackendEvent>,
) {
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();

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
                    if line.is_empty() { buf.drain(..=pos); continue; }
                    if let Ok(resp) = serde_json::from_str::<OllamaChatResponse>(line) {
                        if resp.done { break; }
                        if let Some(msg) = resp.message {
                            if !msg.content.is_empty() { let _ = tx.send(BackendEvent::Text(msg.content)); }
                            if let Some(tool_calls) = msg.tool_calls {
                                for tc in tool_calls {
                                    let args_str = serde_json::to_string(&tc.function.arguments).unwrap_or_default();
                                    let tool_call = ToolCall {
                                        id: format!("ollama_{}_{}", tc.function.name, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()),
                                        call_type: "function".to_string(),
                                        function: FunctionCall { name: tc.function.name, arguments: args_str },
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
                let _ = tx.send(BackendEvent::Error(format!("[Ollama stream error: {e}]")));
                return;
            }
        }
    }

    info!("Ollama chat complete");
    let _ = tx.send(BackendEvent::Done);
}
