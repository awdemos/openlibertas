use anyhow::Result;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Sse},
    routing::{get, post},
    Router,
};
use futures_util::stream::Stream;
use openlibertas_core::{
    backend::registry::ProviderRegistry,
    config::{Config, SecretString},
    domain::{BackendEvent, Message, ProviderId, Role},
    store::SessionStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct AppState {
    registry: Arc<ProviderRegistry>,
    config: Config,
    session: Arc<RwLock<Vec<Message>>>,
    current_model: Arc<RwLock<Option<String>>>,
    current_provider: Arc<RwLock<ProviderId>>,
    store: Option<SessionStore>,
    http_client: reqwest::Client,
    api_key: Option<SecretString>,
}

#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    model: Option<String>,
    provider: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    tool_calls: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct HistoryResponse {
    messages: Vec<MessageView>,
}

#[derive(Serialize)]
struct MessageView {
    role: String,
    content: String,
    tool_calls: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct SessionRequest {
    name: String,
}

#[derive(Deserialize)]
struct TtsRequest {
    text: String,
    voice_id: Option<String>,
}

#[derive(Serialize)]
struct TtsResponse {
    audio: String,
}

#[derive(Serialize)]
struct SttResponse {
    text: String,
}

#[derive(Serialize)]
struct SessionListResponse {
    sessions: Vec<String>,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    model: Option<String>,
    provider: String,
    message_count: usize,
}

// ---------------------------------------------------------------------------
// New OpenAI-style chat completion types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChatCompletionRequest {
    messages: Vec<Message>,
    model: String,
    #[serde(default)]
    provider: Option<String>,
}

#[derive(Serialize)]
struct ChatCompletionResponse {
    model: String,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tool_calls: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Model listing types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ModelInfo {
    id: String,
    provider: String,
    supports_tools: bool,
    supports_voice: bool,
}

#[derive(Serialize)]
struct ModelsResponse {
    models: Vec<ModelInfo>,
}

// ---------------------------------------------------------------------------
// Health check types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ProviderHealthStatus {
    name: String,
    healthy: bool,
    model_count: Option<usize>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    providers: Vec<ProviderHealthStatus>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ok<T: Serialize>(data: T) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    let value = serde_json::to_value(data).unwrap_or(serde_json::Value::Null);
    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(value),
            error: None,
        }),
    )
}

fn err(
    status: StatusCode,
    msg: impl Into<String>,
) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    (
        status,
        Json(ApiResponse {
            success: false,
            data: None,
            error: Some(msg.into()),
        }),
    )
}

async fn auth_middleware(
    State(state): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    if let Some(expected) = &state.api_key {
        let auth_header = req
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok());
        let provided = auth_header.and_then(|h| h.strip_prefix("Bearer "));
        if provided != Some(expected.expose_secret()) {
            return err(StatusCode::UNAUTHORIZED, "Invalid or missing API key").into_response();
        }
    }
    next.run(req).await
}

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    let provider_ids: Vec<ProviderId> = state
        .registry
        .list_providers()
        .into_iter()
        .cloned()
        .collect();

    let mut providers = Vec::new();
    for provider_id in &provider_ids {
        if let Some(provider) = state.registry.get(provider_id) {
            let healthy = provider.health_check().await.is_ok();
            let model_count = provider.fetch_models().await.ok().map(|m| m.len());
            providers.push(ProviderHealthStatus {
                name: provider_id.0.clone(),
                healthy,
                model_count,
            });
        }
    }

    let status = if providers.iter().all(|p| p.healthy) {
        "ok"
    } else {
        "degraded"
    }
    .to_string();

    ok(HealthResponse { status, providers })
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

async fn list_models(State(state): State<AppState>) -> impl IntoResponse {
    let provider_ids: Vec<ProviderId> = state
        .registry
        .list_providers()
        .into_iter()
        .cloned()
        .collect();

    let mut models = Vec::new();
    for provider_id in &provider_ids {
        if let Some(provider) = state.registry.get(provider_id) {
            match provider.fetch_models().await {
                Ok(provider_models) => {
                    for model in provider_models {
                        let (supports_tools, supports_voice) =
                            openlibertas_core::domain::Model::infer_capabilities(&model.id);
                        models.push(ModelInfo {
                            id: model.id,
                            provider: provider_id.0.clone(),
                            supports_tools,
                            supports_voice,
                        });
                    }
                }
                Err(_) => {
                    // If fetch_models fails, at least list the provider name
                    // as a fallback model identifier so the UI isn't empty.
                    models.push(ModelInfo {
                        id: provider_id.0.clone(),
                        provider: provider_id.0.clone(),
                        supports_tools: provider.capabilities().tools,
                        supports_voice: false,
                    });
                }
            }
        }
    }

    ok(ModelsResponse { models })
}

// ---------------------------------------------------------------------------
// Legacy status & history
// ---------------------------------------------------------------------------

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let messages = state.session.read().await;
    let model = state.current_model.read().await.clone();
    let provider = state.current_provider.read().await.clone();

    ok(StatusResponse {
        status: "running".to_string(),
        model,
        provider: provider.0,
        message_count: messages.len(),
    })
}

async fn get_history(State(state): State<AppState>) -> impl IntoResponse {
    let messages = state.session.read().await;
    let views: Vec<MessageView> = messages
        .iter()
        .map(|m| MessageView {
            role: m.role.to_string(),
            content: m.content.clone(),
            tool_calls: m.tool_calls.as_ref().map(|tc| {
                tc.iter()
                    .map(|t| {
                        serde_json::json!({
                            "id": t.id,
                            "type": t.call_type,
                            "function": {
                                "name": t.function.name,
                                "arguments": t.function.arguments,
                            }
                        })
                    })
                    .collect()
            }),
        })
        .collect();

    ok(HistoryResponse { messages: views })
}

async fn clear_session(State(state): State<AppState>) -> impl IntoResponse {
    let mut messages = state.session.write().await;
    messages.clear();
    ok(serde_json::json!({ "cleared": true }))
}

// ---------------------------------------------------------------------------
// Legacy non-streaming chat (/api/chat)
// ---------------------------------------------------------------------------

async fn post_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let default_provider = state.current_provider.read().await.clone();
    let provider_id = req
        .provider
        .map(|p| ProviderId::new(&p))
        .unwrap_or(default_provider);

    let provider = match state
        .registry
        .get(&provider_id)
        .or_else(|| state.registry.default_provider())
        .cloned()
    {
        Some(b) => b,
        None => return err(StatusCode::SERVICE_UNAVAILABLE, "No provider available"),
    };

    let current_model = state.current_model.read().await.clone();
    let model = req
        .model
        .or(current_model)
        .unwrap_or_else(|| "default".to_string());

    {
        let mut messages = state.session.write().await;
        messages.push(Message {
            role: Role::User,
            content: req.message.clone(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        });
    }

    let messages = {
        let msgs = state.session.read().await.clone();
        msgs
    };

    let cancel_token = CancellationToken::new();
    let mut stream = provider.chat(
        model,
        messages,
        state.config.max_tokens,
        None,
        cancel_token,
        None,
    );

    let mut response_text = String::new();
    let mut tool_calls = Vec::new();

    while let Some(event) = stream.recv().await {
        match event {
            BackendEvent::Text(text) => response_text.push_str(&text),
            BackendEvent::Reasoning(_) => {}
            BackendEvent::ToolCall(tc) => {
                tool_calls.push(serde_json::json!({
                    "id": tc.id,
                    "type": tc.call_type,
                    "function": {
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    }
                }));
            }
            BackendEvent::Done | BackendEvent::Cancelled => break,
            BackendEvent::Error(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }

    {
        let mut messages = state.session.write().await;
        let tool_calls_data = if tool_calls.is_empty() {
            None
        } else {
            Some(
                tool_calls
                    .iter()
                    .map(|tc| {
                        serde_json::from_value(tc.clone()).unwrap_or_else(|_| {
                            openlibertas_core::domain::ToolCall {
                                id: "".to_string(),
                                call_type: "function".to_string(),
                                function: openlibertas_core::domain::FunctionCall {
                                    name: "".to_string(),
                                    arguments: "".to_string(),
                                },
                            }
                        })
                    })
                    .collect(),
            )
        };
        messages.push(Message {
            role: Role::Assistant,
            content: response_text.clone(),
            tool_calls: tool_calls_data,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        });
    }

    ok(ChatResponse {
        response: response_text,
        tool_calls,
    })
}

// ---------------------------------------------------------------------------
// Legacy streaming chat (/api/chat/stream)
// ---------------------------------------------------------------------------

async fn stream_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::Event;

    let default_provider = state.current_provider.read().await.clone();
    let provider_id = req
        .provider
        .map(|p| ProviderId::new(&p))
        .unwrap_or(default_provider);

    let provider_opt = state
        .registry
        .get(&provider_id)
        .or_else(|| state.registry.default_provider())
        .cloned();

    let current_model = state.current_model.read().await.clone();
    let model = req
        .model
        .or(current_model)
        .unwrap_or_else(|| "default".to_string());

    {
        let mut messages = state.session.write().await;
        messages.push(Message {
            role: Role::User,
            content: req.message.clone(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        });
    }

    let messages = {
        let msgs = state.session.read().await.clone();
        msgs
    };

    let state_clone = state.clone();
    let sse_stream = async_stream::stream! {
        let provider = match provider_opt {
            Some(b) => b,
            None => {
                yield Ok(Event::default().data("{\"error\": \"No provider available\"}"));
                return;
            }
        };

        let cancel_token = CancellationToken::new();
        let mut stream = provider.chat(
            model,
            messages,
            state_clone.config.max_tokens,
            None,
            cancel_token,
            None,
        );

        let mut response_text = String::new();
        let mut tool_calls = Vec::new();

        while let Some(event) = stream.recv().await {
            match event {
                BackendEvent::Text(text) => {
                    response_text.push_str(&text);
                    let text_json = serde_json::to_string(&text).unwrap_or_else(|_| format!("{:?}", text));
                    yield Ok(Event::default().data(format!("{{\"type\": \"text\", \"content\": {} }}", text_json)));
                }
                BackendEvent::Reasoning(text) => {
                    let text_json = serde_json::to_string(&text).unwrap_or_else(|_| format!("{:?}", text));
                    yield Ok(Event::default().data(format!("{{\"type\": \"reasoning\", \"content\": {} }}", text_json)));
                }
                BackendEvent::ToolCall(tc) => {
                    let tc_json = serde_json::json!({
                        "id": tc.id,
                        "type": tc.call_type,
                        "function": {
                            "name": tc.function.name,
                            "arguments": tc.function.arguments,
                        }
                    });
                    tool_calls.push(tc_json.clone());
                    yield Ok(Event::default().data(format!("{{\"type\": \"tool_call\", \"data\": {} }}", tc_json)));
                }
                BackendEvent::Done => {
                    yield Ok(Event::default().data("{\"type\": \"done\"}"));
                    break;
                }
                BackendEvent::Cancelled => {
                    yield Ok(Event::default().data("{\"type\": \"cancelled\"}"));
                    break;
                }
                BackendEvent::Error(e) => {
                    let err_json = serde_json::to_string(&e).unwrap_or_else(|_| format!("{:?}", e));
                    yield Ok(Event::default().data(format!("{{\"type\": \"error\", \"message\": {} }}", err_json)));
                    break;
                }
            }
        }

        let mut messages = state_clone.session.write().await;
        let tool_calls_data = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls.iter().map(|tc| {
                serde_json::from_value(tc.clone()).unwrap_or_else(|_| openlibertas_core::domain::ToolCall {
                    id: "".to_string(),
                    call_type: "function".to_string(),
                    function: openlibertas_core::domain::FunctionCall {
                        name: "".to_string(),
                        arguments: "".to_string(),
                    },
                })
            }).collect())
        };
        messages.push(Message {
            role: Role::Assistant,
            content: response_text,
            tool_calls: tool_calls_data,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        });
    };

    Sse::new(sse_stream)
}

// ---------------------------------------------------------------------------
// New OpenAI-style non-streaming chat (/chat)
// ---------------------------------------------------------------------------

async fn chat_completion(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let default_provider = state.current_provider.read().await.clone();
    let provider_id = req
        .provider
        .map(|p| ProviderId::new(&p))
        .unwrap_or(default_provider);

    let provider = match state
        .registry
        .get(&provider_id)
        .or_else(|| state.registry.default_provider())
        .cloned()
    {
        Some(b) => b,
        None => return err(StatusCode::SERVICE_UNAVAILABLE, "No provider available"),
    };

    let cancel_token = CancellationToken::new();
    let mut stream = provider.chat(
        req.model.clone(),
        req.messages,
        state.config.max_tokens,
        None,
        cancel_token,
        None,
    );

    let mut content = String::new();
    let mut tool_calls = Vec::new();

    while let Some(event) = stream.recv().await {
        match event {
            BackendEvent::Text(text) => content.push_str(&text),
            BackendEvent::Reasoning(_) => {}
            BackendEvent::ToolCall(tc) => {
                tool_calls.push(serde_json::json!({
                    "id": tc.id,
                    "type": tc.call_type,
                    "function": {
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    }
                }));
            }
            BackendEvent::Done | BackendEvent::Cancelled => break,
            BackendEvent::Error(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }

    ok(ChatCompletionResponse {
        model: req.model,
        content,
        tool_calls,
    })
}

// ---------------------------------------------------------------------------
// New OpenAI-style streaming chat (/chat/stream)
// ---------------------------------------------------------------------------

async fn chat_completion_stream(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::Event;

    let default_provider = state.current_provider.read().await.clone();
    let provider_id = req
        .provider
        .map(|p| ProviderId::new(&p))
        .unwrap_or(default_provider);

    let provider_opt = state
        .registry
        .get(&provider_id)
        .or_else(|| state.registry.default_provider())
        .cloned();

    let model = req.model;
    let messages = req.messages;

    let sse_stream = async_stream::stream! {
        let provider = match provider_opt {
            Some(b) => b,
            None => {
                yield Ok(Event::default().data("{\"error\": \"No provider available\"}"));
                return;
            }
        };

        let cancel_token = CancellationToken::new();
        let mut stream = provider.chat(
            model,
            messages,
            state.config.max_tokens,
            None,
            cancel_token,
            None,
        );

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        while let Some(event) = stream.recv().await {
            match event {
                BackendEvent::Text(text) => {
                    content.push_str(&text);
                    let text_json = serde_json::to_string(&text).unwrap_or_else(|_| format!("{:?}", text));
                    yield Ok(Event::default().data(format!("{{\"type\": \"text\", \"content\": {} }}", text_json)));
                }
                BackendEvent::Reasoning(text) => {
                    let text_json = serde_json::to_string(&text).unwrap_or_else(|_| format!("{:?}", text));
                    yield Ok(Event::default().data(format!("{{\"type\": \"reasoning\", \"content\": {} }}", text_json)));
                }
                BackendEvent::ToolCall(tc) => {
                    let tc_json = serde_json::json!({
                        "id": tc.id,
                        "type": tc.call_type,
                        "function": {
                            "name": tc.function.name,
                            "arguments": tc.function.arguments,
                        }
                    });
                    tool_calls.push(tc_json.clone());
                    yield Ok(Event::default().data(format!("{{\"type\": \"tool_call\", \"data\": {} }}", tc_json)));
                }
                BackendEvent::Done => {
                    yield Ok(Event::default().data("{\"type\": \"done\"}"));
                    break;
                }
                BackendEvent::Cancelled => {
                    yield Ok(Event::default().data("{\"type\": \"cancelled\"}"));
                    break;
                }
                BackendEvent::Error(e) => {
                    let err_json = serde_json::to_string(&e).unwrap_or_else(|_| format!("{:?}", e));
                    yield Ok(Event::default().data(format!("{{\"type\": \"error\", \"message\": {} }}", err_json)));
                    break;
                }
            }
        }
    };

    Sse::new(sse_stream)
}

// ---------------------------------------------------------------------------
// Session endpoints
// ---------------------------------------------------------------------------

async fn list_sessions(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = if let Some(ref store) = state.store {
        store.list().unwrap_or_default()
    } else {
        Vec::new()
    };

    ok(SessionListResponse { sessions })
}

async fn save_session(
    State(state): State<AppState>,
    Json(req): Json<SessionRequest>,
) -> impl IntoResponse {
    let store = match state.store {
        Some(ref s) => s,
        None => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "Session storage not available",
            )
        }
    };

    let messages = state.session.read().await.clone();
    let model = state.current_model.read().await.clone();

    match store.save(&req.name, model.as_deref(), &messages) {
        Ok(_) => ok(serde_json::json!({ "saved": true })),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save: {}", e),
        ),
    }
}

async fn load_session(
    State(state): State<AppState>,
    Json(req): Json<SessionRequest>,
) -> impl IntoResponse {
    let store = match state.store {
        Some(ref s) => s,
        None => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "Session storage not available",
            )
        }
    };

    match store.load(&req.name) {
        Ok(messages) => {
            let mut session = state.session.write().await;
            *session = messages;
            ok(serde_json::json!({ "loaded": true }))
        }
        Err(e) => err(StatusCode::NOT_FOUND, format!("Session not found: {}", e)),
    }
}

async fn delete_session(
    State(state): State<AppState>,
    Json(req): Json<SessionRequest>,
) -> impl IntoResponse {
    let store = match state.store {
        Some(ref s) => s,
        None => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "Session storage not available",
            )
        }
    };

    match store.delete(&req.name) {
        Ok(_) => ok(serde_json::json!({ "deleted": true })),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete: {}", e),
        ),
    }
}

// ---------------------------------------------------------------------------
// Voice endpoints
// ---------------------------------------------------------------------------

async fn voice_stt(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let api_key = match state.config.elevenlabs_api_key {
        Some(key) => key,
        None => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "ElevenLabs API key not configured",
            )
        }
    };

    let mut audio_bytes = Vec::new();
    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name().unwrap_or("") == "audio" {
            audio_bytes = field.bytes().await.unwrap_or_default().to_vec();
            break;
        }
    }

    if audio_bytes.is_empty() {
        return err(StatusCode::BAD_REQUEST, "No audio data provided");
    }

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(audio_bytes).file_name("audio.wav"),
        )
        .text("model_id", "scribe_v1");

    match state
        .http_client
        .post("https://api.elevenlabs.io/v1/speech-to-text")
        .header("xi-api-key", api_key.expose_secret())
        .multipart(form)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let text = json.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        ok(SttResponse {
                            text: text.to_string(),
                        })
                    }
                    Err(e) => err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to parse STT response: {}", e),
                    ),
                }
            } else {
                err(
                    StatusCode::BAD_GATEWAY,
                    format!("ElevenLabs STT error: {}", resp.status()),
                )
            }
        }
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("STT request failed: {}", e),
        ),
    }
}

async fn voice_tts(
    State(state): State<AppState>,
    Json(req): Json<TtsRequest>,
) -> impl IntoResponse {
    let api_key = match state.config.elevenlabs_api_key {
        Some(key) => key,
        None => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "ElevenLabs API key not configured",
            )
        }
    };

    let voice_id = req
        .voice_id
        .or(state.config.elevenlabs_voice_id)
        .unwrap_or_else(|| "21m00Tcm4TlvDq8ikWAM".to_string());

    let body = serde_json::json!({
        "text": req.text,
        "model_id": "eleven_multilingual_v2",
    });

    match state
        .http_client
        .post(format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}/stream",
            voice_id
        ))
        .header("xi-api-key", api_key.expose_secret())
        .header("Content-Type", "application/json")
        .query(&[("output_format", "mp3_44100_128")])
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.bytes().await {
                    Ok(bytes) => {
                        use base64::Engine;
                        let base64_audio = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        ok(TtsResponse {
                            audio: base64_audio,
                        })
                    }
                    Err(e) => err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to read TTS audio: {}", e),
                    ),
                }
            } else {
                err(
                    StatusCode::BAD_GATEWAY,
                    format!("ElevenLabs TTS error: {}", resp.status()),
                )
            }
        }
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("TTS request failed: {}", e),
        ),
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::load().unwrap_or_default();
    let registry = Arc::new(ProviderRegistry::new(&config.providers));
    let store = Config::data_dir().and_then(|d| SessionStore::new(d).ok());

    let default_provider = config
        .providers
        .first()
        .map(|p| ProviderId::new(&p.name))
        .unwrap_or_else(|| ProviderId::new("local"));

    let api_key = std::env::var("SERVER_API_KEY")
        .ok()
        .map(|raw| openlibertas_core::config::resolve_env_ref(&raw))
        .filter(|s| !s.is_empty())
        .map(SecretString::new);

    let state = AppState {
        registry,
        config,
        session: Arc::new(RwLock::new(Vec::new())),
        current_model: Arc::new(RwLock::new(None)),
        current_provider: Arc::new(RwLock::new(default_provider)),
        store,
        http_client: reqwest::Client::new(),
        api_key,
    };

    let api_routes = Router::new()
        .route("/status", get(get_status))
        .route("/history", get(get_history))
        .route("/clear", post(clear_session))
        .route("/chat", post(post_chat))
        .route("/chat/stream", post(stream_chat))
        .route("/sessions", get(list_sessions))
        .route("/session/save", post(save_session))
        .route("/session/load", post(load_session))
        .route("/session/delete", post(delete_session))
        .route("/voice/stt", post(voice_stt))
        .route("/voice/tts", post(voice_tts));

    let v1_routes = Router::new()
        .route("/chat", post(chat_completion))
        .route("/chat/stream", post(chat_completion_stream))
        .route("/models", get(list_models))
        .route("/sessions", get(list_sessions))
        .route("/session/save", post(save_session))
        .route("/session/load", post(load_session))
        .route("/session/delete", post(delete_session));

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api", api_routes)
        .nest("/v1", v1_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                ]),
        )
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;

    tracing::info!("OpenLibertas server listening on http://0.0.0.0:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let config = Config::default();
        let registry = Arc::new(ProviderRegistry::new(&config.providers));
        let store = Config::data_dir().and_then(|d| SessionStore::new(d).ok());
        let default_provider = config
            .providers
            .first()
            .map(|p| ProviderId::new(&p.name))
            .unwrap_or_else(|| ProviderId::new("local"));

        AppState {
            registry,
            config,
            session: Arc::new(RwLock::new(Vec::new())),
            current_model: Arc::new(RwLock::new(None)),
            current_provider: Arc::new(RwLock::new(default_provider)),
            store,
            http_client: reqwest::Client::new(),
            api_key: None,
        }
    }

    fn test_router() -> Router {
        let state = test_state();
        let api_routes = Router::new()
            .route("/status", get(get_status))
            .route("/history", get(get_history))
            .route("/clear", post(clear_session))
            .route("/chat", post(post_chat))
            .route("/chat/stream", post(stream_chat))
            .route("/sessions", get(list_sessions))
            .route("/session/save", post(save_session))
            .route("/session/load", post(load_session))
            .route("/session/delete", post(delete_session));

        let v1_routes = Router::new()
            .route("/chat", post(chat_completion))
            .route("/chat/stream", post(chat_completion_stream))
            .route("/models", get(list_models))
            .route("/sessions", get(list_sessions))
            .route("/session/save", post(save_session))
            .route("/session/load", post(load_session))
            .route("/session/delete", post(delete_session));

        Router::new()
            .route("/health", get(health_check))
            .nest("/api", api_routes)
            .nest("/v1", v1_routes)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .layer(
                tower_http::cors::CorsLayer::new()
                    .allow_origin(tower_http::cors::Any)
                    .allow_methods([
                        axum::http::Method::GET,
                        axum::http::Method::POST,
                        axum::http::Method::DELETE,
                        axum::http::Method::OPTIONS,
                    ])
                    .allow_headers([
                        axum::http::header::CONTENT_TYPE,
                        axum::http::header::AUTHORIZATION,
                    ]),
            )
            .with_state(state)
    }

    #[tokio::test]
    async fn health_check_returns_ok() {
        let app = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_check_returns_provider_status() {
        let app = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["data"]["status"].is_string());
        assert!(json["data"]["providers"].is_array());
    }

    #[tokio::test]
    async fn models_endpoint_returns_list() {
        let app = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["data"]["models"].is_array());
    }

    #[tokio::test]
    async fn api_status_returns_running() {
        let app = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn api_history_returns_empty() {
        let app = test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/history")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn v1_chat_accepts_message_array() {
        let app = test_router();
        let body = serde_json::json!({
            "messages": [
                { "role": "user", "content": "hello" }
            ],
            "model": "default"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Will fail with SERVICE_UNAVAILABLE because no real provider is up,
        // but the request parsing should succeed.
        assert!(
            response.status() == StatusCode::SERVICE_UNAVAILABLE
                || response.status() == StatusCode::OK
                || response.status() == StatusCode::INTERNAL_SERVER_ERROR,
            "expected SERVICE_UNAVAILABLE, OK, or INTERNAL_SERVER_ERROR, got {:?}",
            response.status()
        );
    }

    #[tokio::test]
    async fn auth_middleware_blocks_without_key() {
        let mut state = test_state();
        state.api_key = Some(SecretString::new("secret".to_string()));

        let api_routes = Router::new().route("/status", get(get_status));
        let app = Router::new()
            .nest("/api", api_routes)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_middleware_allows_with_valid_key() {
        let mut state = test_state();
        state.api_key = Some(SecretString::new("secret".to_string()));

        let api_routes = Router::new().route("/status", get(get_status));
        let app = Router::new()
            .nest("/api", api_routes)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/status")
                    .header("authorization", "Bearer secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
