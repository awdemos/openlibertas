use anyhow::Result;
use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::{IntoResponse, Sse},
    routing::{get, post},
    Router,
};
use futures_util::stream::Stream;
use openlibertas_core::{
    backend::{registry::BackendRegistry, ChatEvent, Message},
    config::Config,
    domain::{ProviderId, Role},
    store::ConversationStore,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct AppState {
    registry: Arc<BackendRegistry>,
    config: Config,
    conversation: Arc<RwLock<Vec<Message>>>,
    current_model: Arc<RwLock<Option<String>>>,
    current_provider: Arc<RwLock<ProviderId>>,
    store: Option<ConversationStore>,
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
    (StatusCode::OK, Json(ApiResponse {
        success: true,
        data: Some(value),
        error: None,
    }))
}

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<ApiResponse<serde_json::Value>>) {
    (status, Json(ApiResponse {
        success: false,
        data: None,
        error: Some(msg.into()),
    }))
}

async fn health_check() -> impl IntoResponse {
    ok(serde_json::json!({ "status": "ok" }))
}

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let messages = state.conversation.read().await;
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
    let messages = state.conversation.read().await;
    let views: Vec<MessageView> = messages.iter().map(|m| MessageView {
        role: m.role.to_string(),
        content: m.content.clone(),
        tool_calls: m.tool_calls.as_ref().map(|tc| {
            tc.iter().map(|t| serde_json::json!({
                "id": t.id,
                "type": t.call_type,
                "function": {
                    "name": t.function.name,
                    "arguments": t.function.arguments,
                }
            })).collect()
        }),
    }).collect();
    
    ok(HistoryResponse { messages: views })
}

async fn clear_conversation(State(state): State<AppState>) -> impl IntoResponse {
    let mut messages = state.conversation.write().await;
    messages.clear();
    ok(serde_json::json!({ "cleared": true }))
}

async fn post_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let default_provider = state.current_provider.read().await.clone();
    let provider_id = req.provider
        .map(|p| ProviderId::new(&p))
        .unwrap_or(default_provider);
    
    let backend = match state.registry.get(&provider_id)
        .or_else(|| state.registry.default_backend())
        .cloned() {
        Some(b) => b,
        None => return err(StatusCode::SERVICE_UNAVAILABLE, "No backend available"),
    };
    
    let current_model = state.current_model.read().await.clone();
    let model = req.model.or(current_model).unwrap_or_else(|| "default".to_string());
    
    {
        let mut messages = state.conversation.write().await;
        messages.push(Message {
            role: Role::User,
            content: req.message.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    
    let messages = {
        let msgs = state.conversation.read().await.clone();
        msgs
    };
    
    let cancel_token = CancellationToken::new();
    let mut stream = backend.chat(
        model,
        messages,
        state.config.max_tokens,
        None,
        cancel_token,
    );
    
    let mut response_text = String::new();
    let mut tool_calls = Vec::new();
    
    while let Some(event) = stream.recv().await {
        match event {
            ChatEvent::Text(text) => response_text.push_str(&text),
            ChatEvent::ToolCall(tc) => {
                tool_calls.push(serde_json::json!({
                    "id": tc.id,
                    "type": tc.call_type,
                    "function": {
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    }
                }));
            }
            ChatEvent::Done | ChatEvent::Cancelled => break,
            ChatEvent::Error(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }
    
    {
        let mut messages = state.conversation.write().await;
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
            content: response_text.clone(),
            tool_calls: tool_calls_data,
            tool_call_id: None,
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
    let provider_id = req.provider
        .map(|p| ProviderId::new(&p))
        .unwrap_or(default_provider);
    
    let backend_opt = state.registry.get(&provider_id)
        .or_else(|| state.registry.default_backend())
        .cloned();
    
    let current_model = state.current_model.read().await.clone();
    let model = req.model.or(current_model).unwrap_or_else(|| "default".to_string());
    
    {
        let mut messages = state.conversation.write().await;
        messages.push(Message {
            role: Role::User,
            content: req.message.clone(),
            tool_calls: None,
            tool_call_id: None,
        });
    }
    
    let messages = {
        let msgs = state.conversation.read().await.clone();
        msgs
    };
    
    let state_clone = state.clone();
    let sse_stream = async_stream::stream! {
        let backend = match backend_opt {
            Some(b) => b,
            None => {
                yield Ok(Event::default().data("{\"error\": \"No backend available\"}"));
                return;
            }
        };
        
        let cancel_token = CancellationToken::new();
        let mut stream = backend.chat(
            model,
            messages,
            state_clone.config.max_tokens,
            None,
            cancel_token,
        );
        
        let mut response_text = String::new();
        let mut tool_calls = Vec::new();
        
        while let Some(event) = stream.recv().await {
            match event {
                ChatEvent::Text(text) => {
                    response_text.push_str(&text);
                    yield Ok(Event::default().data(format!("{{\"type\": \"text\", \"content\": {} }}", serde_json::to_string(&text).unwrap())));
                }
                ChatEvent::ToolCall(tc) => {
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
                ChatEvent::Done => {
                    yield Ok(Event::default().data("{\"type\": \"done\"}"));
                    break;
                }
                ChatEvent::Cancelled => {
                    yield Ok(Event::default().data("{\"type\": \"cancelled\"}"));
                    break;
                }
                ChatEvent::Error(e) => {
                    yield Ok(Event::default().data(format!("{{\"type\": \"error\", \"message\": {} }}", serde_json::to_string(&e).unwrap())));
                    break;
                }
            }
        }
        
        let mut messages = state_clone.conversation.write().await;
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
        });
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
        None => return err(StatusCode::SERVICE_UNAVAILABLE, "Session storage not available"),
    };
    
    let messages = state.conversation.read().await.clone();
    let model = state.current_model.read().await.clone();
    
    match store.save(&req.name, model.as_deref(), &messages) {
        Ok(_) => ok(serde_json::json!({ "saved": true })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save: {}", e)),
    }
}

async fn load_session(
    State(state): State<AppState>,
    Json(req): Json<SessionRequest>,
) -> impl IntoResponse {
    let store = match state.store {
        Some(ref s) => s,
        None => return err(StatusCode::SERVICE_UNAVAILABLE, "Session storage not available"),
    };
    
    match store.load(&req.name) {
        Ok(messages) => {
            let mut conv = state.conversation.write().await;
            *conv = messages;
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
        None => return err(StatusCode::SERVICE_UNAVAILABLE, "Session storage not available"),
    };
    
    match store.delete(&req.name) {
        Ok(_) => ok(serde_json::json!({ "deleted": true })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to delete: {}", e)),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    
    let config = Config::load().unwrap_or_default();
    let registry = Arc::new(BackendRegistry::new(&config.providers));
    let store = Config::data_dir()
        .and_then(|d| ConversationStore::new(d).ok());
    
    let default_provider = config.providers.first()
        .map(|p| ProviderId::new(&p.name))
        .unwrap_or_else(|| ProviderId::new("local"));
    
    let state = AppState {
        registry,
        config,
        conversation: Arc::new(RwLock::new(Vec::new())),
        current_model: Arc::new(RwLock::new(None)),
        current_provider: Arc::new(RwLock::new(default_provider)),
        store,
    };
    
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/status", get(get_status))
        .route("/api/history", get(get_history))
        .route("/api/clear", post(clear_conversation))
        .route("/api/chat", post(post_chat))
        .route("/api/chat/stream", post(stream_chat))
        .route("/api/sessions", get(list_sessions))
        .route("/api/session/save", post(save_session))
        .route("/api/session/load", post(load_session))
        .route("/api/session/delete", post(delete_session))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);
    
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    
    tracing::info!("OpenLibertas server listening on http://0.0.0.0:{}", port);
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
