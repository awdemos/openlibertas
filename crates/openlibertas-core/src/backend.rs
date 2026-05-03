use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;

pub use crate::domain::*;

pub mod registry;

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

#[derive(Clone)]
pub struct OpenAiBackend {
    client: Client,
    base_url: String,
    api_key: String,
    supports_tools: bool,
}

impl OpenAiBackend {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self::with_tools(base_url, api_key, true)
    }

    pub fn with_tools(base_url: String, api_key: String, supports_tools: bool) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            client,
            base_url,
            api_key,
            supports_tools,
        }
    }

    pub async fn fetch_models(&self) -> Result<Vec<Model>> {
        let url = format!("{}/models", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("Failed to fetch models")?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("HTTP {}", resp.status()));
        }
        let data: ModelsResponse = resp.json().await.context("Failed to parse models response")?;
        Ok(data.data)
    }

    pub fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> mpsc::UnboundedReceiver<ChatEvent> {
        let req = ChatRequest {
            model,
            messages,
            stream: true,
            max_tokens: Some(max_tokens),
            tools: if self.supports_tools { tools } else { None },
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();

        tokio::spawn(async move {
            let url = format!("{}/chat/completions", base_url);
            let resp = match client
                .post(&url)
                .bearer_auth(&api_key)
                .json(&req)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ChatEvent::Error(format!("[Error: {}]", e)));
                    return;
                }
            };

            let mut stream = resp.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            let mut accumulated_tool_calls: HashMap<usize, ToolCall> = HashMap::new();

            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => {
                        let _ = tx.send(ChatEvent::Cancelled);
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
                                if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data)
                                {
                                    if let Some(choice) = chunk.choices.first() {
                                        if let Some(content) = &choice.delta.content {
                                            if !content.is_empty() {
                                                let _ = tx.send(ChatEvent::Text(content.clone()));
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
                        let _ = tx.send(ChatEvent::Error(format!("[Stream error: {}]", e)));
                        return;
                    }
                }
            }

            for (_, tool_call) in accumulated_tool_calls {
                let _ = tx.send(ChatEvent::ToolCall(tool_call));
            }
            let _ = tx.send(ChatEvent::Done);
        });

        rx
    }
}

pub trait Backend: Send + Sync {
    fn fetch_models(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Model>>> + Send + '_>>;

    fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> mpsc::UnboundedReceiver<ChatEvent>;
}

impl Backend for OpenAiBackend {
    fn fetch_models(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Model>>> + Send + '_>> {
        Box::pin(self.fetch_models())
    }

    fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> mpsc::UnboundedReceiver<ChatEvent> {
        self.chat(model, messages, max_tokens, tools, cancel_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_serializes_correctly() {
        let msg = Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
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
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"tool_calls\""));
        assert!(json.contains("\"call_1\""));
    }

    #[test]
    fn chat_request_serializes_tools() {
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message {
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
}
