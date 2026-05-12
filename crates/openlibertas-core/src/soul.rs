use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::backend::Backend;
use crate::domain::{ChatEvent, Message, Role, ToolDefinition};
use crate::engine::{AgentMode, ChatEngine};

#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        output: String,
    },
}

#[derive(Debug, Clone)]
pub enum WireMessage {
    TurnStarted { turn_id: String },
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStarted { id: String, name: String },
    ToolCallDelta { id: String, arguments: String },
    ToolCallDone { id: String },
    ToolExecuting { id: String, name: String },
    ToolResult { id: String, output: String },
    TurnFinished { turn_id: String },
    Error(String),
    Cancelled,
}

pub type WireSender = mpsc::UnboundedSender<WireMessage>;

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Running { turn_id: String },
    ProcessingTools { turn_id: String },
    Stopped,
    Error(String),
}

#[derive(Debug)]
pub struct UserInput {
    pub text: String,
    pub file_context: Vec<String>,
    pub mode: AgentMode,
}

#[derive(Debug)]
pub struct TurnResult {
    pub turn_id: String,
    pub message_count: usize,
    pub tool_calls_executed: usize,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum AgentError {
    #[error("backend error: {0}")]
    BackendError(String),
    #[error("tool error: {tool} — {error}")]
    ToolError { tool: String, error: String },
    #[error("turn was cancelled")]
    Cancelled,
    #[error("maximum iterations reached")]
    MaxIterationsReached,
    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn status(&self) -> AgentStatus;
    async fn run_turn(
        &mut self,
        input: UserInput,
        wire: WireSender,
    ) -> Result<TurnResult, AgentError>;
    async fn stop(&mut self);
    fn tools(&self) -> Vec<ToolDefinition>;
    fn set_persona(&mut self, persona: Option<String>);
}

pub struct ChatAgent {
    id: String,
    name: String,
    engine: ChatEngine,
    backend: Arc<dyn Backend>,
    model: String,
    max_tokens: u32,
    temperature: Option<f32>,
    status: AgentStatus,
    turn_id: Option<String>,
}

impl ChatAgent {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        mut engine: ChatEngine,
        backend: Arc<dyn Backend>,
        model: impl Into<String>,
        max_tokens: u32,
    ) -> Self {
        if engine.agents().status == crate::engine::AgentStatus::Disabled {
            engine.agents_mut().status = crate::engine::AgentStatus::Idle;
        }
        Self {
            id: id.into(),
            name: name.into(),
            engine,
            backend,
            model: model.into(),
            max_tokens,
            temperature: None,
            status: AgentStatus::Idle,
            turn_id: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn engine(&self) -> &ChatEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut ChatEngine {
        &mut self.engine
    }
}

#[async_trait]
impl Agent for ChatAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    async fn run_turn(
        &mut self,
        input: UserInput,
        wire: WireSender,
    ) -> Result<TurnResult, AgentError> {
        let turn_id = format!(
            "turn_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        self.turn_id = Some(turn_id.clone());
        self.status = AgentStatus::Running {
            turn_id: turn_id.clone(),
        };

        let _ = wire.send(WireMessage::TurnStarted {
            turn_id: turn_id.clone(),
        });

        self.engine.agents_mut().mode = input.mode;
        self.engine.chat_mut().cancel_token =
            tokio_util::sync::CancellationToken::new();
        self.engine.start_agent_loop();

        let mut text = input.text;
        for file in &input.file_context {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!("@{}", file));
        }

        let message_count_before = self.engine.chat().messages.len();
        let mut api_messages = self.engine.push_user_message(text);
        let mut total_tool_calls = 0usize;

        loop {
            let cancel_token = self.engine.chat().cancel_token.clone();
            let tools = self.engine.tools_for_request();

            let mut rx = self.backend.chat(
                self.model.clone(),
                api_messages,
                self.max_tokens,
                tools,
                cancel_token,
                self.temperature,
            );

            let mut stream_error: Option<String> = None;
            let mut done_received = false;

            while let Some(event) = rx.recv().await {
                match event {
                    ChatEvent::Text(text) => {
                        self.engine.append_stream_chunk(&text);
                        let _ = wire.send(WireMessage::TextDelta(text));
                    }
                    ChatEvent::Reasoning(text) => {
                        self.engine.append_reasoning_chunk(&text);
                        let _ = wire.send(WireMessage::ReasoningDelta(text));
                    }
                    ChatEvent::ToolCall(tool_call) => {
                        self.engine.add_tool_call(tool_call.clone());
                        let _ = wire.send(WireMessage::ToolCallStarted {
                            id: tool_call.id.clone(),
                            name: tool_call.function.name.clone(),
                        });
                    }
                    ChatEvent::Done => {
                        self.engine.finish_stream();
                        self.engine.sanitize_assistant_content();
                        done_received = true;
                        break;
                    }
                    ChatEvent::Error(err) => {
                        stream_error = Some(err.clone());
                        self.engine.finish_stream();
                        let _ = wire.send(WireMessage::Error(err));
                        break;
                    }
                    ChatEvent::Cancelled => {
                        self.engine.finish_stream();
                        self.engine.finish_agent_loop();
                        self.engine.tool_executor_mut().clear_pending_tool_calls();
                        self.status = AgentStatus::Stopped;
                        let _ = wire.send(WireMessage::Cancelled);
                        let _ = wire.send(WireMessage::TurnFinished {
                            turn_id: turn_id.clone(),
                        });
                        return Err(AgentError::Cancelled);
                    }
                }
            }

            if !done_received && stream_error.is_none() {
                self.engine.finish_stream();
                self.engine.sanitize_assistant_content();
            }

            if let Some(err) = stream_error {
                self.engine.finish_agent_loop();
                self.status = AgentStatus::Error(err.clone());
                let _ = wire.send(WireMessage::TurnFinished {
                    turn_id: turn_id.clone(),
                });
                return Err(AgentError::BackendError(err));
            }

            self.engine.increment_agent_iteration();

            if self.engine.agent_iteration_exceeded() {
                self.engine.finish_agent_loop();
                let max = self.engine.agents().max_iterations;
                self.engine.add_system_message(format!(
                    "[Agent stopped after {} iterations. Provide more specific instructions if needed.]",
                    max
                ));
                self.status = AgentStatus::Error(format!(
                    "Max iterations ({}) reached",
                    max
                ));
                let _ = wire.send(WireMessage::TurnFinished {
                    turn_id: turn_id.clone(),
                });
                return Err(AgentError::MaxIterationsReached);
            }

            if self.engine.tool_executor().has_pending_tool_calls() {
                self.status = AgentStatus::ProcessingTools {
                    turn_id: turn_id.clone(),
                };

                let pending = self.engine.tool_executor().pending_tool_calls().to_vec();
                total_tool_calls += pending.len();

                for tc in &pending {
                    let _ = wire.send(WireMessage::ToolExecuting {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                    });
                }

                let results = self.engine.execute_pending_tools().await;

                for (i, result) in results.iter().enumerate() {
                    if let Some(tc) = pending.get(i) {
                        let output = result.content_for_message();
                        let _ = wire.send(WireMessage::ToolResult {
                            id: tc.id.clone(),
                            output: output.clone(),
                        });
                    }
                }

                self.engine.tool_executor_mut().clear_pending_tool_calls();
                self.engine.tool_executor_mut().clear_tool_results();

                api_messages = self.engine.chat().messages.clone();
                self.engine.chat_mut().messages.push(Message {
                    role: Role::Assistant,
                    content: String::new(),
                    tool_calls: None,
                    tool_call_id: None,
                    timestamp: None,
                    reasoning_content: None,
                    is_prompt: false,
                });
                self.engine.chat_mut().streaming = true;
                self.status = AgentStatus::Running {
                    turn_id: turn_id.clone(),
                };
            } else {
                self.engine.finish_agent_loop();
                break;
            }
        }

        let message_count = self
            .engine
            .chat()
            .messages
            .len()
            .saturating_sub(message_count_before);
        self.status = AgentStatus::Idle;
        let _ = wire.send(WireMessage::TurnFinished {
            turn_id: turn_id.clone(),
        });

        Ok(TurnResult {
            turn_id,
            message_count,
            tool_calls_executed: total_tool_calls,
        })
    }

    async fn stop(&mut self) {
        self.engine.chat_mut().cancel_token.cancel();
        self.status = AgentStatus::Stopped;
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.engine
            .tools()
            .definitions_for_api(false)
            .unwrap_or_default()
    }

    fn set_persona(&mut self, persona: Option<String>) {
        if let Some(name) = persona {
            self.engine.agents_mut().persona = name.clone();
            self.engine
                .set_agent_prompt(format!("You are the {} agent.", name));
        } else {
            self.engine.set_agent_prompt("");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use super::*;
    use crate::backend::Backend;
    use crate::capability::ProviderCapabilities;
    use crate::domain::Model;

    struct MockAgent {
        id: String,
        name: String,
        status: AgentStatus,
        persona: Option<String>,
    }

    #[async_trait]
    impl Agent for MockAgent {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn status(&self) -> AgentStatus {
            self.status.clone()
        }

        async fn run_turn(
            &mut self,
            _input: UserInput,
            _wire: WireSender,
        ) -> Result<TurnResult, AgentError> {
            Ok(TurnResult {
                turn_id: "mock".to_string(),
                message_count: 0,
                tool_calls_executed: 0,
            })
        }

        async fn stop(&mut self) {
            self.status = AgentStatus::Stopped;
        }

        fn tools(&self) -> Vec<ToolDefinition> {
            vec![]
        }

        fn set_persona(&mut self, persona: Option<String>) {
            self.persona = persona;
        }
    }

    #[test]
    fn mock_agent_implements_trait() {
        let agent = MockAgent {
            id: "mock-1".to_string(),
            name: "Mock".to_string(),
            status: AgentStatus::Idle,
            persona: None,
        };
        assert_eq!(agent.id(), "mock-1");
        assert_eq!(agent.name(), "Mock");
        assert_eq!(agent.status(), AgentStatus::Idle);
        assert!(agent.tools().is_empty());
    }

    struct MockBackend {
        events: std::sync::Mutex<Vec<ChatEvent>>,
    }

    impl Backend for MockBackend {
        fn chat(
            &self,
            _model: String,
            _messages: Vec<Message>,
            _max_tokens: u32,
            _tools: Option<Vec<ToolDefinition>>,
            _cancel_token: tokio_util::sync::CancellationToken,
            _temperature: Option<f32>,
        ) -> mpsc::UnboundedReceiver<ChatEvent> {
            let (tx, rx) = mpsc::unbounded_channel();
            let events: Vec<ChatEvent> = self
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            for event in events {
                let _ = tx.send(event);
            }
            rx
        }

        fn fetch_models(
            &self,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<Model>>> + Send + '_>>
        {
            Box::pin(async { Ok(vec![]) })
        }

        fn health_check(
            &self,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn cancel_token(&self) -> tokio_util::sync::CancellationToken {
            tokio_util::sync::CancellationToken::new()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::default()
        }
    }

    #[test]
    fn chat_agent_construction() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![]),
        });
        let agent = ChatAgent::new("agent-1", "Test Agent", engine, backend, "gpt-4", 2048);
        assert_eq!(agent.id(), "agent-1");
        assert_eq!(agent.name(), "Test Agent");
        assert_eq!(agent.status(), AgentStatus::Idle);
    }

    #[test]
    fn chat_agent_tools_returns_definitions() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![]),
        });
        let agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        let tools = agent.tools();
        assert!(!tools.is_empty(), "builtin tools should be present");
    }

    #[test]
    fn chat_agent_set_persona() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        agent.set_persona(Some("Research".to_string()));
        assert_eq!(agent.engine().agents().persona, "Research");
        assert!(agent.engine().agent_prompt().is_some());

        agent.set_persona(None);
        assert_eq!(agent.engine().agent_prompt(), Some(""));
    }

    #[test]
    fn chat_agent_engine_access() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        agent.engine_mut().set_system_prompt("hello");
        assert_eq!(agent.engine().system_prompt(), Some("hello"));
    }

    #[tokio::test]
    async fn chat_agent_run_turn_basic() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![
                ChatEvent::Text("Hello".to_string()),
                ChatEvent::Done,
            ]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let input = UserInput {
            text: "hi".to_string(),
            file_context: vec![],
            mode: AgentMode::Auto,
        };

        let result = agent.run_turn(input, tx).await.unwrap();
        assert!(result.turn_id.starts_with("turn_"));
        assert_eq!(result.message_count, 2);
        assert_eq!(result.tool_calls_executed, 0);
        assert_eq!(agent.status(), AgentStatus::Idle);

        let mut found_text = false;
        let mut found_start = false;
        let mut found_finish = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                WireMessage::TextDelta(t) if t == "Hello" => found_text = true,
                WireMessage::TurnStarted { .. } => found_start = true,
                WireMessage::TurnFinished { .. } => found_finish = true,
                _ => {}
            }
        }
        assert!(found_text);
        assert!(found_start);
        assert!(found_finish);
    }

    #[tokio::test]
    async fn chat_agent_stop_cancels_turn() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        agent.stop().await;
        assert_eq!(agent.status(), AgentStatus::Stopped);
    }

    #[tokio::test]
    async fn chat_agent_run_turn_with_error() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![ChatEvent::Error("boom".to_string())]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        let (tx, _rx) = mpsc::unbounded_channel();
        let input = UserInput {
            text: "hi".to_string(),
            file_context: vec![],
            mode: AgentMode::Auto,
        };

        let err = agent.run_turn(input, tx).await.unwrap_err();
        assert!(matches!(err, AgentError::BackendError(ref e) if e == "boom"));
        assert!(matches!(agent.status(), AgentStatus::Error(_)));
    }

    #[tokio::test]
    async fn chat_agent_run_turn_cancelled_event() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![ChatEvent::Cancelled]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        let (tx, _rx) = mpsc::unbounded_channel();
        let input = UserInput {
            text: "hi".to_string(),
            file_context: vec![],
            mode: AgentMode::Auto,
        };

        let err = agent.run_turn(input, tx).await.unwrap_err();
        assert!(matches!(err, AgentError::Cancelled));
        assert_eq!(agent.status(), AgentStatus::Stopped);
    }

    #[tokio::test]
    async fn chat_agent_run_turn_with_file_context() {
        let engine = ChatEngine::new();
        let backend = Arc::new(MockBackend {
            events: std::sync::Mutex::new(vec![
                ChatEvent::Text("ok".to_string()),
                ChatEvent::Done,
            ]),
        });
        let mut agent = ChatAgent::new("a", "A", engine, backend, "m", 1024);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let input = UserInput {
            text: "read this".to_string(),
            file_context: vec!["src/main.rs".to_string()],
            mode: AgentMode::Auto,
        };

        let result = agent.run_turn(input, tx).await.unwrap();
        assert_eq!(result.message_count, 2);

        let last_user = agent
            .engine()
            .chat()
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User);
        assert!(last_user.is_some());
        let content = &last_user.unwrap().content;
        assert!(content.contains("src/main.rs"), "file context should be present in message");

        while rx.try_recv().is_ok() {}
    }
}
