//! Fallback tool call parser for models that don't support native function calling.

use crate::backend::{FunctionCall, ToolCall};
use serde_json::Value;

pub fn parse_tool_calls_from_text(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    calls.extend(parse_json_tool_calls(text));
    calls.extend(parse_xml_tool_calls(text));
    calls.extend(parse_codeblock_tool_calls(text));
    calls
}

fn parse_json_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('{') && line.contains("\"name\"") {
            if let Ok(json) = serde_json::from_str::<Value>(line) {
                if let (Some(name), Some(args)) = (
                    json.get("name").and_then(|v| v.as_str()),
                    json.get("arguments"),
                ) {
                    calls.push(ToolCall {
                        id: format!("call_{}", calls.len()),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: name.to_string(),
                            arguments: args.to_string(),
                        },
                    });
                }
            }
        }
    }
    calls
}

fn parse_xml_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut start = 0;
    while let Some(tag_start) = text[start..].find("<tool ") {
        let abs_start = start + tag_start;
        if let Some(name_start) = text[abs_start..].find("name=\"") {
            let name_abs = abs_start + name_start + 6;
            if let Some(name_end) = text[name_abs..].find('"') {
                let name = &text[name_abs..name_abs + name_end];
                if let Some(body_start) = text[abs_start..].find('>') {
                    let body_abs = abs_start + body_start + 1;
                    if let Some(tag_end) = text[body_abs..].find("</tool>") {
                        let body = &text[body_abs..body_abs + tag_end];
                        calls.push(ToolCall {
                            id: format!("call_{}", calls.len()),
                            call_type: "function".to_string(),
                            function: FunctionCall {
                                name: name.to_string(),
                                arguments: body.trim().to_string(),
                            },
                        });
                    }
                }
            }
        }
        start = abs_start + 1;
    }
    calls
}

fn parse_codeblock_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut start = 0;
    while let Some(cb_start) = text[start..].find("```json") {
        let abs_start = start + cb_start + 7;
        if let Some(cb_end) = text[abs_start..].find("```") {
            let block = &text[abs_start..abs_start + cb_end];
            calls.extend(parse_json_tool_calls(block));
        }
        start = abs_start + 1;
    }
    calls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_tool_call() {
        let text = r#"{"name": "glob", "arguments": {"pattern": "*.rs"}}"#;
        let calls = parse_tool_calls_from_text(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "glob");
    }

    #[test]
    fn returns_empty_for_no_tools() {
        let text = "I'll help you with that.";
        let calls = parse_tool_calls_from_text(text);
        assert!(calls.is_empty());
    }
}
