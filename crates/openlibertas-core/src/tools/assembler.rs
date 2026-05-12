use crate::conversation::build_tool_result_messages;
use crate::domain::{Message, Role, ToolCall};

#[derive(Debug)]
pub struct MessageAssembler;

impl Default for MessageAssembler {
    fn default() -> Self {
        Self
    }
}

impl MessageAssembler {
    pub fn assemble(
        &self,
        messages: &[Message],
        agent_status: Option<&str>,
        agent_prompt: Option<&str>,
        pending_tool_calls: &[ToolCall],
        tool_results: &[String],
    ) -> Vec<Message> {
        let mut result = messages.to_vec();

        if let Some(prompt) = agent_prompt {
            if agent_status == Some("active") {
                let already_present = result
                    .iter()
                    .take_while(|m| m.role == Role::System)
                    .any(|m| m.content == prompt);
                if !already_present {
                    result.insert(
                        0,
                        Message { role: Role::System, content: prompt.to_string(), tool_calls: None, tool_call_id: None, timestamp: None, reasoning_content: None, is_prompt: false },
                    );
                }
            }
        }

        build_tool_result_messages(&result, pending_tool_calls, tool_results)
    }
}
