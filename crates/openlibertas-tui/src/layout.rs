//! UI layout helpers for calculating message positions and line wrapping.
//!
//! These functions were moved from `openlibertas-core::engine` because they
//! are pure UI rendering calculations that belong in the TUI crate.

use openlibertas_core::agent_tools::extract_key_argument;
use openlibertas_core::domain::{Message, Role};

/// Count how many lines a piece of content will occupy when wrapped to
/// the given viewport width, accounting for code blocks and paragraphs.
pub fn count_wrapped_lines(content: &str, width: usize) -> usize {
    if width == 0 {
        return content.lines().count().max(1);
    }

    let mut in_code = false;
    let mut paragraph_chars = 0usize;
    let mut lines = 0usize;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed.starts_with("```") {
            if paragraph_chars > 0 {
                lines += paragraph_chars.div_ceil(width);
                paragraph_chars = 0;
            }
            in_code = !in_code;
            lines += 1;
        } else if in_code {
            lines += 1;
        } else if raw_line.trim().is_empty() {
            if paragraph_chars > 0 {
                lines += paragraph_chars.div_ceil(width);
                paragraph_chars = 0;
            }
            lines += 1;
        } else {
            paragraph_chars += raw_line.trim().chars().count() + 1;
        }
    }

    if paragraph_chars > 0 {
        lines += paragraph_chars.div_ceil(width);
    }

    lines.max(1)
}

/// Find the message index at the given y coordinate in the viewport.
///
/// Takes the message list, scroll offset, viewport width, and whether
/// the chat is currently streaming as parameters.
pub fn message_at_y(
    messages: &[Message],
    scroll: usize,
    y: usize,
    viewport_width: usize,
    streaming: bool,
) -> Option<usize> {
    let mut line = 0usize;
    for (idx, msg) in messages.iter().enumerate() {
        if let Some(ref tool_calls) = msg.tool_calls {
            let mut tool_lines = 0;
            for tc in tool_calls {
                tool_lines += 1;
                let key_arg = extract_key_argument(&tc.function.name, &tc.function.arguments);
                if key_arg.is_empty() {
                    tool_lines += 1;
                }
            }
            tool_lines += 1;
            if line + tool_lines > y + scroll {
                return Some(idx);
            }
            line += tool_lines;
        } else {
            let mut msg_lines = 1;

            if msg.role == Role::Assistant {
                if let Some(ref reasoning) = msg.reasoning_content {
                    if !reasoning.is_empty() && viewport_width > 10 {
                        msg_lines += 2;
                        msg_lines +=
                            count_wrapped_lines(reasoning, viewport_width.saturating_sub(2));
                        msg_lines += 1;
                    }
                }
            }

            if viewport_width > 10 {
                msg_lines += count_wrapped_lines(&msg.content, viewport_width);
            } else {
                msg_lines += msg.content.lines().count().max(1);
            }

            let is_last = idx == messages.len().saturating_sub(1);
            if is_last && streaming && msg.role == Role::Assistant {
                msg_lines += 1;
            }

            msg_lines += 1;

            if line + msg_lines > y + scroll {
                return Some(idx);
            }
            line += msg_lines;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use openlibertas_core::domain::{Message, Role};

    #[test]
    fn count_wrapped_lines_empty_string() {
        assert_eq!(count_wrapped_lines("", 10), 1);
    }

    #[test]
    fn count_wrapped_lines_zero_width() {
        assert_eq!(count_wrapped_lines("hello", 0), 1);
    }

    #[test]
    fn count_wrapped_lines_multiple_paragraphs() {
        let text = "first paragraph\n\nsecond paragraph";
        assert_eq!(count_wrapped_lines(text, 20), 3);
    }

    #[test]
    fn message_at_y_returns_none_for_empty_chat() {
        assert!(message_at_y(&[], 0, 0, 80, false).is_none());
    }

    #[test]
    fn message_at_y_finds_first_message() {
        let messages = vec![Message {
            role: Role::User,
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }];
        assert_eq!(message_at_y(&messages, 0, 0, 80, false), Some(0));
    }
}
