use crate::domain::Message;

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub message_index: usize,
    pub char_start: usize,
    pub char_end: usize,
}

impl SearchMatch {
    pub fn snippet<'a>(&self, content: &'a str) -> &'a str {
        let end = self.char_end.min(content.len());
        if self.char_start < end {
            &content[self.char_start..end]
        } else {
            ""
        }
    }
}

pub fn search_messages(messages: &[Message], query: &str) -> Vec<SearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut matches = Vec::new();

    for (msg_idx, msg) in messages.iter().enumerate() {
        let content_lower = msg.content.to_lowercase();
        let mut start = 0;

        while let Some(pos) = content_lower[start..].find(&query_lower) {
            let char_start = start + pos;
            let char_end = char_start + query_lower.len();
            matches.push(SearchMatch {
                message_index: msg_idx,
                char_start,
                char_end,
            });
            start = char_end;
            if start >= content_lower.len() {
                break;
            }
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Message;
    use crate::domain::Role;

    fn create_message(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            timestamp: None,
            reasoning_content: None,
            is_prompt: false,
        }
    }

    #[test]
    fn search_finds_matches() {
        let messages = vec![create_message("Hello world"), create_message("Hello again")];
        let matches = search_messages(&messages, "Hello");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].message_index, 0);
        assert_eq!(matches[1].message_index, 1);
    }

    #[test]
    fn search_case_insensitive() {
        let messages = vec![create_message("Hello World")];
        let matches = search_messages(&messages, "world");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn search_empty_query_returns_none() {
        let messages = vec![create_message("Hello")];
        let matches = search_messages(&messages, "");
        assert!(matches.is_empty());
    }

    #[test]
    fn search_no_match_returns_empty() {
        let messages = vec![create_message("Hello")];
        let matches = search_messages(&messages, "xyz");
        assert!(matches.is_empty());
    }

    #[test]
    fn search_multiple_matches_same_message() {
        let messages = vec![create_message("Hello hello hello")];
        let matches = search_messages(&messages, "hello");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn search_snippet_extracts_match() {
        let messages = vec![create_message("Hello world")];
        let matches = search_messages(&messages, "world");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].snippet(&messages[0].content), "world");
    }

    #[test]
    fn search_snippet_empty_on_bounds() {
        let m = SearchMatch {
            message_index: 0,
            char_start: 10,
            char_end: 5,
        };
        assert_eq!(m.snippet("short"), "");
    }

    #[test]
    fn search_across_multiple_messages() {
        let messages = vec![
            create_message("first msg"),
            create_message("second msg"),
            create_message("third msg"),
        ];
        let matches = search_messages(&messages, "msg");
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].message_index, 0);
        assert_eq!(matches[1].message_index, 1);
        assert_eq!(matches[2].message_index, 2);
    }

    #[test]
    fn search_unicode_content() {
        let messages = vec![create_message("Hello 世界")];
        let matches = search_messages(&messages, "世界");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].snippet(&messages[0].content), "世界");
    }
}
