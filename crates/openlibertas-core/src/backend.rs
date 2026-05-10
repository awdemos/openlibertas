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

#[derive(Clone)]
pub struct OpenAiBackend {
    client: Client,
    base_url: String,
    api_key: String,
    supports_tools: bool,
    extra_params: Option<serde_json::Map<String, serde_json::Value>>,
}

impl OpenAiBackend {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self::with_tools(base_url, api_key, true)
    }

    pub fn with_tools(base_url: String, api_key: String, supports_tools: bool) -> Self {
        Self::with_tools_and_params(base_url, api_key, supports_tools, None)
    }

    pub fn with_tools_and_params(
        base_url: String,
        api_key: String,
        supports_tools: bool,
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
            supports_tools,
            extra_params,
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
        let data: ModelsResponse = resp
            .json()
            .await
            .context("Failed to parse models response")?;
        let mut models = data.data;
        for model in &mut models {
            let (inferred_tools, inferred_voice) = Model::infer_capabilities(&model.id);
            model.supports_tools = self.supports_tools || inferred_tools;
            model.supports_voice = inferred_voice;
        }
        Ok(models)
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
            extra_params: self.extra_params.clone(),
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let api_key = self.api_key.clone();

        tokio::spawn(async move {
            let url = format!("{}/chat/completions", base_url);
            let mut retries = 0;
            const MAX_RETRIES: u32 = 3;
            let resp;

            loop {
                match client
                    .post(&url)
                    .bearer_auth(&api_key)
                    .json(&req)
                    .send()
                    .await
                {
                    Ok(r) => {
                        if r.status().is_success() {
                            resp = Some(r);
                            break;
                        }
                        let status = r.status();
                        let is_transient =
                            status.as_u16() == 429 || (502..=504).contains(&status.as_u16());
                        if is_transient && retries < MAX_RETRIES {
                            retries += 1;
                            let delay = std::time::Duration::from_secs(2_u64.pow(retries));
                            let _ = tx.send(ChatEvent::Error(format!(
                                "[HTTP {} — retrying {}/{} in {:?}]",
                                status, retries, MAX_RETRIES, delay
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        let body = r.text().await.unwrap_or_default();
                        let _ = tx.send(ChatEvent::Error(format!(
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
                            let _ = tx.send(ChatEvent::Error(format!(
                                "[Connection error — retrying {}/{} in {:?}: {}]",
                                retries, MAX_RETRIES, delay, e
                            )));
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        let _ = tx.send(ChatEvent::Error(format!("[Error: {}]", e)));
                        return;
                    }
                }
            }

            let resp = resp.expect("loop invariant: resp always Some after break");

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
                                        if let Some(reasoning) = &choice.delta.reasoning_content {
                                            if !reasoning.is_empty() {
                                                let _ = tx.send(ChatEvent::Reasoning(reasoning.clone()));
                                            }
                                        } else if let Some(thinking) = &choice.delta.thinking {
                                            if !thinking.is_empty() {
                                                let _ = tx.send(ChatEvent::Reasoning(thinking.clone()));
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
                                                            .unwrap_or_else(|| {
                                                                "function".to_string()
                                                            }),
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

#[derive(Clone)]
pub enum Backend {
    OpenAi(OpenAiBackend),
}

impl Backend {
    pub async fn fetch_models(&self) -> Result<Vec<Model>> {
        match self {
            Backend::OpenAi(backend) => backend.fetch_models().await,
        }
    }

    pub fn chat(
        &self,
        model: String,
        messages: Vec<Message>,
        max_tokens: u32,
        tools: Option<Vec<ToolDefinition>>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> mpsc::UnboundedReceiver<ChatEvent> {
        match self {
            Backend::OpenAi(backend) => {
                backend.chat(model, messages, max_tokens, tools, cancel_token)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_serializes_correctly() {
        let msg = Message { role: Role::User, content: "hello".to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hello\""));
    }

    #[test]
    fn message_with_tool_calls_serializes() {
        let msg = Message { role: Role::Assistant, content: "".to_string(), tool_calls: Some(vec![ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: "test".to_string(),
                arguments: "{}".to_string(),
            },
        }]), tool_call_id: None, timestamp: None, reasoning_content: None };
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
                timestamp: None,
reasoning_content: None,
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
            messages: vec![Message {
                role: Role::User,
                content: "hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
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
            messages: vec![Message {
                role: Role::User,
                content: "hi".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
reasoning_content: None,
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
}
