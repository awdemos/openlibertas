//! Agent loop orchestration for autonomous multi-step tool execution.
//!
//! `AgentLoop` encapsulates the post-stream agent logic:
//! - Iteration limit checking
//! - Tool execution
//! - Persona switching with isolation
//! - Context compaction
//! - Message assembly for the next turn

use crate::domain::{Message, Role, ToolExecutionResult};
use crate::engine::{AgentModeStatus, ChatEngine};
use tracing::info;

/// Action returned by the agent loop after processing a completed stream.
pub enum LoopAction {
    /// Continue the agent loop with these messages for the next chat request.
    Continue(Vec<Message>),
    /// The agent loop has finished (no more tools or iteration limit reached).
    Stop,
    /// The agent loop encountered an unrecoverable error.
    Error(String),
}

/// Resolves persona names to their system prompts.
pub trait PersonaResolver {
    /// Returns the system prompt for the given persona name, if found.
    fn resolve(&self, name: &str) -> Option<String>;
}

impl<F> PersonaResolver for F
where
    F: Fn(&str) -> Option<String>,
{
    fn resolve(&self, name: &str) -> Option<String> {
        (self)(name)
    }
}

/// Run one iteration of the agent loop.
///
/// This should be called after `BackendEvent::Done` is received. It handles
/// finishing the stream, executing pending tools, persona switches,
/// context compaction, and preparing messages for the next turn.
pub async fn run_agent_loop(
        engine: &mut ChatEngine,
        persona_resolver: &dyn PersonaResolver,
    ) -> LoopAction {
        engine.finish_stream();
        engine.sanitize_assistant_content();
        engine.increment_agent_iteration();

        if engine.agent_iteration_exceeded() {
            engine.finish_agent_loop();
            let max_iterations = engine.agents().max_iterations;
            engine.add_system_message(format!(
                "[Agent stopped after {max_iterations} iterations. Provide more specific instructions if needed.]"
            ));
            return LoopAction::Stop;
        }

        if engine.tool_executor().has_pending_tool_calls() {
            let results = engine.execute_pending_tools().await;

            for result in &results {
                match result {
                    ToolExecutionResult::Success { tool_name, .. } => {
                        if tool_name != "switch_persona" {
                            info!("Tool: {} executed successfully", tool_name);
                        }
                    }
                    ToolExecutionResult::Error {
                        tool_name, error, ..
                    } => {
                        info!("Tool: {} failed: {}", tool_name, error);
                    }
                    ToolExecutionResult::Skipped { tool_name, reason } => {
                        info!("Tool: {} skipped: {}", tool_name, reason);
                    }
                }
            }

            let switch_requests: Vec<(String, bool)> = engine
                .tool_executor()
                .pending_tool_calls()
                .iter()
                .filter_map(|tc| {
                    if tc.function.name == "switch_persona" {
                        serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                            .ok()
                            .and_then(|args| {
                                let persona = args
                                    .get("persona")
                                    .and_then(|v| v.as_str())
                                    .map(String::from)?;
                                let isolate = args
                                    .get("isolate")
                                    .and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false);
                                Some((persona, isolate))
                            })
                    } else {
                        None
                    }
                })
                .collect();

            for (persona, isolate) in switch_requests {
                let current_persona = engine.agents().persona.clone();
                if persona.eq_ignore_ascii_case(&current_persona) {
                    continue;
                }
                if let Some(prompt) = persona_resolver.resolve(&persona) {
                    engine.switch_persona(&persona, &prompt);
                    if isolate {
                        engine.isolate_session();
                        info!("Session isolated for '{}'", persona);
                    }
                } else {
                    info!(
                        "Agent: persona '{persona}' not found, staying as '{current_persona}'"
                    );
                }
            }

            let mut tool_messages = engine.assemble_tool_result_messages();

            engine.tool_executor_mut().clear_pending_tool_calls();
            engine.tool_executor_mut().clear_tool_results();

            if engine.agents().status == AgentModeStatus::Active {
                let compacted = engine.chat_mut().compactor.compact(&tool_messages);
                if compacted.len() < tool_messages.len() {
                    info!(
                        "Context compacted: {} → {} messages",
                        tool_messages.len(),
                        compacted.len()
                    );
                }
                tool_messages = compacted;
            }

            engine.chat_mut().messages.push(Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            });
            engine.chat_mut().streaming = true;

            LoopAction::Continue(tool_messages)
        } else {
            engine.finish_agent_loop();
            LoopAction::Stop
        }
    }
