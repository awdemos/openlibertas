//! OpenLibertas HTTP server — Axum-based REST API for remote access.

#![cfg_attr(not(test), warn(clippy::unwrap_used))]
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(not(test), warn(clippy::panic))]

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

async fn health_check() -> impl IntoResponse {
    ok(serde_json::json!({ "status": "ok" }))
}

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

            is_prompt: false,});
    };

    Sse::new(sse_stream)
}

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

    let api_key = std::env::var("SERVER_API_KEY").ok().map(SecretString::new);

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

    let app = Router::new()
        .route("/health", get(health_check))
        .nest(
            "/api",
            Router::new()
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
                .route("/voice/tts", post(voice_tts))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                )),
        )
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::exact(
                    "http://localhost:3000".parse().unwrap(),
                ))
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
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
