use super::types::*;
use crate::domain::{Message, Role};

pub(crate) fn convert_messages_anthropic(
    messages: Vec<Message>,
) -> (Option<String>, Vec<AnthropicMessage>) {
    let system_parts: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.clone())
        .collect();
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    let non_system: Vec<Message> = messages
        .into_iter()
        .filter(|m| m.role != Role::System)
        .collect();

    let mut result: Vec<AnthropicMessage> = Vec::new();

    for msg in non_system {
        match msg.role {
            Role::User => {
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.append_text(&msg.content);
                        continue;
                    }
                }
                result.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Text(msg.content),
                });
            }
            Role::Assistant => {
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .map(|t| !t.is_empty())
                    .unwrap_or(false);
                if has_tool_calls {
                    let mut blocks = Vec::new();
                    if !msg.content.is_empty() {
                        blocks.push(AnthropicContentBlock::Text { text: msg.content });
                    }
                    if let Some(tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let input = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or_else(|_| {
                                    serde_json::Value::Object(serde_json::Map::new())
                                });
                            blocks.push(AnthropicContentBlock::ToolUse {
                                id: tc.id,
                                name: tc.function.name,
                                input,
                            });
                        }
                    }
                    if let Some(last) = result.last_mut() {
                        if last.role == "assistant" {
                            last.append_blocks(blocks);
                            continue;
                        }
                    }
                    result.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Blocks(blocks),
                    });
                } else {
                    if let Some(last) = result.last_mut() {
                        if last.role == "assistant" {
                            last.append_text(&msg.content);
                            continue;
                        }
                    }
                    result.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Text(msg.content),
                    });
                }
            }
            Role::Tool => {
                let block = AnthropicContentBlock::ToolResult {
                    tool_use_id: msg.tool_call_id.unwrap_or_default(),
                    content: msg.content,
                };
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.append_block(block);
                        continue;
                    }
                }
                result.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Blocks(vec![block]),
                });
            }
            Role::System => {}
        }
    }

    (system, result)
}

// ============================================================================
// Gemini message conversion
// ============================================================================

pub(crate) fn convert_messages_gemini(
    messages: Vec<Message>,
) -> (Option<GeminiContent>, Vec<GeminiContent>) {
    let system_parts: Vec<String> = messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.clone())
        .collect();
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart::Text {
                text: system_parts.join("\n\n"),
            }],
        })
    };

    let non_system: Vec<Message> = messages
        .into_iter()
        .filter(|m| m.role != Role::System)
        .collect();

    let mut result: Vec<GeminiContent> = Vec::new();

    for msg in non_system {
        match msg.role {
            Role::User => {
                let parts = vec![GeminiPart::Text { text: msg.content }];
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.parts.extend(parts);
                        continue;
                    }
                }
                result.push(GeminiContent {
                    role: "user".to_string(),
                    parts,
                });
            }
            Role::Assistant => {
                let has_tool_calls = msg
                    .tool_calls
                    .as_ref()
                    .map(|t| !t.is_empty())
                    .unwrap_or(false);
                let mut parts = Vec::new();
                if !msg.content.is_empty() {
                    parts.push(GeminiPart::Text { text: msg.content });
                }
                if has_tool_calls {
                    if let Some(tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            let args =
                                serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| {
                                    serde_json::Value::Object(serde_json::Map::new())
                                });
                            parts.push(GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    name: tc.function.name,
                                    args,
                                },
                            });
                        }
                    }
                }
                if let Some(last) = result.last_mut() {
                    if last.role == "model" {
                        last.parts.extend(parts);
                        continue;
                    }
                }
                result.push(GeminiContent {
                    role: "model".to_string(),
                    parts,
                });
            }
            Role::Tool => {
                let response = serde_json::json!({
                    "result": msg.content
                });
                let parts = vec![GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: msg.tool_call_id.clone().unwrap_or_default(),
                        response,
                    },
                }];
                if let Some(last) = result.last_mut() {
                    if last.role == "user" {
                        last.parts.extend(parts);
                        continue;
                    }
                }
                result.push(GeminiContent {
                    role: "user".to_string(),
                    parts,
                });
            }
            Role::System => {}
        }
    }

    (system, result)
}
