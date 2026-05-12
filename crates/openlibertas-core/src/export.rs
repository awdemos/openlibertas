use crate::domain::Message;
use crate::domain::Role;

pub enum ExportFormat {
    Markdown,
    Json,
    PlainText,
}

impl ExportFormat {
    pub fn from_extension(filename: &str) -> Self {
        if filename.ends_with(".json") {
            ExportFormat::Json
        } else if filename.ends_with(".txt") {
            ExportFormat::PlainText
        } else {
            ExportFormat::Markdown
        }
    }
}

pub fn export_messages(messages: &[Message], model: Option<&str>, format: ExportFormat) -> String {
    match format {
        ExportFormat::Markdown => to_markdown(messages, model),
        ExportFormat::Json => to_json(messages, model),
        ExportFormat::PlainText => to_plaintext(messages),
    }
}

fn to_markdown(messages: &[Message], model: Option<&str>) -> String {
    let mut md = String::new();
    md.push_str("# Chat Session\n\n");
    md.push_str(&format!("Model: {}\n\n", model.unwrap_or("Unknown")));
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => model.unwrap_or("Assistant"),
            Role::System => "System",
            Role::Tool => "Tool",
        };
        let ts = msg.timestamp.as_deref().unwrap_or("");
        let ts_line = if ts.is_empty() {
            String::new()
        } else {
            format!(" *{}*\n", ts)
        };
        md.push_str(&format!("## {}{}\n{}", role, ts_line, msg.content));
        if let Some(ref tool_calls) = msg.tool_calls {
            for tc in tool_calls {
                md.push_str(&format!("\n\n**Tool Call:** `{}`", tc.function.name));
                md.push_str(&format!("\n```json\n{}\n```", tc.function.arguments));
            }
        }
        md.push_str("\n\n---\n\n");
    }
    md
}

fn to_json(messages: &[Message], model: Option<&str>) -> String {
    let export = serde_json::json!({
        "model": model,
        "message_count": messages.len(),
        "messages": messages,
    });
    serde_json::to_string_pretty(&export).unwrap_or_else(|_| "{}".to_string())
}

fn to_plaintext(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|msg| {
            let role = match msg.role {
                Role::User => "User",
                Role::Assistant => "Assistant",
                Role::System => "System",
                Role::Tool => "Tool",
            };
            let ts = msg.timestamp.as_deref().unwrap_or("");
            if ts.is_empty() {
                format!("{}: {}", role, msg.content)
            } else {
                format!("[{}] {}: {}", ts, role, msg.content)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_messages() -> Vec<Message> {
        vec![
            Message {
                role: Role::User,
                content: "Hello".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
            Message {
                role: Role::Assistant,
                content: "Hi there!".to_string(),
                tool_calls: None,
                tool_call_id: None,
                timestamp: None,
                reasoning_content: None,
                is_prompt: false,
            },
        ]
    }

    #[test]
    fn export_markdown_contains_model() {
        let messages = test_messages();
        let md = to_markdown(&messages, Some("gpt-4"));
        assert!(md.contains("gpt-4"));
        assert!(md.contains("# Chat Session"));
    }

    #[test]
    fn export_json_valid_format() {
        let messages = test_messages();
        let json = to_json(&messages, Some("gpt-4"));
        assert!(json.contains("gpt-4"));
        assert!(json.contains("Hello"));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["message_count"], 2);
    }

    #[test]
    fn export_plaintext_simple() {
        let messages = test_messages();
        let text = to_plaintext(&messages);
        assert!(text.contains("User: Hello"));
        assert!(text.contains("Assistant: Hi there!"));
    }

    #[test]
    fn format_from_extension() {
        assert!(matches!(
            ExportFormat::from_extension("chat.md"),
            ExportFormat::Markdown
        ));
        assert!(matches!(
            ExportFormat::from_extension("chat.json"),
            ExportFormat::Json
        ));
        assert!(matches!(
            ExportFormat::from_extension("chat.txt"),
            ExportFormat::PlainText
        ));
    }

    #[test]
    fn format_defaults_to_markdown() {
        assert!(matches!(
            ExportFormat::from_extension("chat.pdf"),
            ExportFormat::Markdown
        ));
    }

    #[test]
    fn export_markdown_with_timestamp() {
        let messages = vec![Message {
            role: Role::User,
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some("2024-01-01".to_string()),
            reasoning_content: None,

            is_prompt: false,
        }];
        let md = to_markdown(&messages, Some("test"));
        assert!(md.contains("2024-01-01"));
    }

    #[test]
    fn export_plaintext_with_timestamp() {
        let messages = vec![Message {
            role: Role::User,
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: Some("12:00".to_string()),
            reasoning_content: None,

            is_prompt: false,
        }];
        let text = to_plaintext(&messages);
        assert!(text.contains("[12:00]"));
    }

    #[test]
    fn export_empty_messages() {
        let messages: Vec<Message> = vec![];
        let md = to_markdown(&messages, None);
        assert!(md.contains("Unknown"));
        let json = to_json(&messages, None);
        assert!(json.contains("\"message_count\": 0"));
    }
}
