use tracing::warn;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::ChatEngine;

impl ChatEngine {
    pub fn push_to_history(&mut self, input: String) {
        if input.trim().is_empty() {
            self.input.history_index = None;
            self.input.history_stash.clear();
            return;
        }
        // Deduplication: skip consecutive duplicates
        if self.input.history.last() == Some(&input) {
            self.input.history_index = None;
            self.input.history_stash.clear();
            return;
        }
        self.input.history.push(input.clone());
        self.input.history_index = None;
        self.input.history_stash.clear();

        // Persist asynchronously to avoid blocking the UI thread
        if let Some(ref store) = self.history_store {
            let store = store.clone();
            std::thread::spawn(move || {
                if let Err(e) = store.append(&input) {
                    warn!("Failed to append history entry: {}", e);
                }
            });
        }
    }

    pub fn history_prev(&mut self) {
        if self.input.history.is_empty() {
            return;
        }
        self.input.selection_anchor = None;
        if self.input.history_index.is_none() {
            self.input.history_stash = self.input.buffer.clone();
            self.input.history_index = Some(self.input.history.len() - 1);
        } else if let Some(idx) = self.input.history_index {
            if idx > 0 {
                self.input.history_index = Some(idx - 1);
            }
        }
        if let Some(idx) = self.input.history_index {
            self.input.buffer = self.input.history[idx].clone();
            self.input.cursor_pos = self.input.buffer.len();
        }
    }

    pub fn history_next(&mut self) {
        if self.input.history.is_empty() {
            return;
        }
        self.input.selection_anchor = None;
        if let Some(idx) = self.input.history_index {
            if idx + 1 < self.input.history.len() {
                self.input.history_index = Some(idx + 1);
                self.input.buffer = self.input.history[idx + 1].clone();
            } else {
                self.input.history_index = None;
                self.input.buffer = self.input.history_stash.clone();
            }
            self.input.cursor_pos = self.input.buffer.len();
        }
    }

    pub fn move_cursor_left(&mut self) {
        self.input.selection_anchor = None;
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        self.input.cursor_pos = self.input.buffer[..pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }

    pub fn move_cursor_right(&mut self) {
        self.input.selection_anchor = None;
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        self.input.cursor_pos = self.input.buffer[pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| pos + i)
            .unwrap_or(self.input.buffer.len());
    }

    pub fn move_cursor_to_start(&mut self) {
        self.input.selection_anchor = None;
        self.input.cursor_pos = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.input.selection_anchor = None;
        self.input.cursor_pos = self.input.buffer.len();
    }

    pub fn delete_word_backward(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.input.cursor_pos == 0 {
            return;
        }
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        let before = &self.input.buffer[..safe_pos];
        let mut chars = before.char_indices().rev().peekable();
        while let Some((_, ch)) = chars.peek() {
            if !ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        while let Some((_, ch)) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        let pos = chars.next().map(|(i, ch)| i + ch.len_utf8()).unwrap_or(0);
        self.input.buffer.replace_range(pos..safe_pos, "");
        self.input.cursor_pos = pos;
    }

    pub fn insert_char(&mut self, c: char) {
        if self.has_selection() {
            self.delete_selection();
        }
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        self.input.buffer.insert(safe_pos, c);
        self.input.cursor_pos = safe_pos + c.len_utf8();
        self.input.show_autocomplete = false;
        self.input.autocomplete_index = 0;
        self.input.history_index = None;
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.input.cursor_pos > 0 {
            let pos = self.input.cursor_pos.min(self.input.buffer.len());
            let safe_pos = pos;
            let prev = self.input.buffer[..safe_pos]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.buffer.remove(prev);
            self.input.cursor_pos = prev;
        }
        self.input.show_autocomplete = false;
        self.input.autocomplete_index = 0;
    }

    pub fn clear_input(&mut self) {
        self.input.buffer.clear();
        self.input.cursor_pos = 0;
        self.input.selection_anchor = None;
        self.input.scroll_offset = 0;
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.input.selection_anchor?;
        let cursor = self.input.cursor_pos.min(self.input.buffer.len());
        let start = anchor.min(cursor);
        let end = anchor.max(cursor);
        if start == end {
            None
        } else {
            Some((start, end))
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    pub fn select_all(&mut self) {
        self.input.selection_anchor = Some(0);
        self.input.cursor_pos = self.input.buffer.len();
    }

    pub fn clear_selection(&mut self) {
        self.input.selection_anchor = None;
    }

    pub fn start_selection(&mut self) {
        self.input.selection_anchor = Some(self.input.cursor_pos);
    }

    pub fn extend_selection_left(&mut self) {
        if self.input.selection_anchor.is_none() {
            self.input.selection_anchor = Some(self.input.cursor_pos);
        }
        self.move_cursor_left();
    }

    pub fn extend_selection_right(&mut self) {
        if self.input.selection_anchor.is_none() {
            self.input.selection_anchor = Some(self.input.cursor_pos);
        }
        self.move_cursor_right();
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection()
            .map(|(start, end)| self.input.buffer[start..end].to_string())
    }

    pub fn delete_selection(&mut self) -> Option<String> {
        let (start, end) = self.selection()?;
        let text = self.input.buffer[start..end].to_string();
        self.input.buffer.replace_range(start..end, "");
        self.input.cursor_pos = start;
        self.input.selection_anchor = None;
        Some(text)
    }

    pub fn move_cursor_word_left(&mut self) {
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        let before = &self.input.buffer[..safe_pos];
        let mut chars = before.char_indices().rev().peekable();
        while let Some((_, ch)) = chars.peek() {
            if !ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        while let Some((_, ch)) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        self.input.cursor_pos = chars.next().map(|(i, ch)| i + ch.len_utf8()).unwrap_or(0);
    }

    pub fn move_cursor_word_right(&mut self) {
        let pos = self.input.cursor_pos.min(self.input.buffer.len());
        let safe_pos = pos;
        let after = &self.input.buffer[safe_pos..];
        let mut chars = after.char_indices().peekable();
        while let Some((_, ch)) = chars.peek() {
            if !ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        while let Some((_, ch)) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        self.input.cursor_pos = chars
            .next()
            .map(|(i, _)| safe_pos + i)
            .unwrap_or(self.input.buffer.len());
    }

    pub fn copy_selection(&mut self) -> Option<String> {
        self.selected_text()
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        self.delete_selection()
    }

    pub fn ensure_cursor_visible(&mut self, viewport_width: usize, prompt_width: usize) {
        let text_before_cursor =
            &self.input.buffer[..self.input.cursor_pos.min(self.input.buffer.len())];
        let cursor_display_pos = text_before_cursor.width() + prompt_width;
        if cursor_display_pos < self.input.scroll_offset {
            self.input.scroll_offset = cursor_display_pos.saturating_sub(1);
        } else if cursor_display_pos >= self.input.scroll_offset + viewport_width {
            self.input.scroll_offset = cursor_display_pos
                .saturating_sub(viewport_width)
                .saturating_add(1);
        }
    }

    pub fn set_cursor_from_click(&mut self, click_x: usize, prompt_width: usize) {
        let text_x = click_x.saturating_sub(prompt_width);
        let mut accumulated_width = 0;
        let mut byte_pos = 0;
        for (i, ch) in self.input.buffer.char_indices() {
            let ch_width = ch.width().unwrap_or(0);
            if accumulated_width + ch_width / 2 > text_x {
                break;
            }
            accumulated_width += ch_width;
            byte_pos = i + ch.len_utf8();
        }
        self.input.cursor_pos = byte_pos.min(self.input.buffer.len());
    }

    pub fn scroll_page_up(&mut self) {
        self.chat.scroll = self.chat.scroll.saturating_sub(10);
        self.chat.auto_scroll = false;
    }

    pub fn scroll_page_down(&mut self) {
        self.chat.scroll += 10;
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::ChatEngine;

    #[test]
    fn input_history_navigation() {
        let mut engine = ChatEngine::new();
        engine.push_to_history("first".to_string());
        engine.push_to_history("second".to_string());
        engine.history_prev();
        assert_eq!(engine.input.buffer, "second");
        engine.history_prev();
        assert_eq!(engine.input.buffer, "first");
        engine.history_next();
        assert_eq!(engine.input.buffer, "second");
    }

    #[test]
    fn cursor_moves_left_and_right() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "abc".to_string();
        engine.input.cursor_pos = 3;
        engine.move_cursor_left();
        assert_eq!(engine.input.cursor_pos, 2);
        engine.move_cursor_right();
        assert_eq!(engine.input.cursor_pos, 3);
    }

    #[test]
    fn cursor_moves_to_start_and_end() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "hello".to_string();
        engine.input.cursor_pos = 3;
        engine.move_cursor_to_start();
        assert_eq!(engine.input.cursor_pos, 0);
        engine.move_cursor_to_end();
        assert_eq!(engine.input.cursor_pos, 5);
    }

    #[test]
    fn insert_char_adds_text() {
        let mut engine = ChatEngine::new();
        engine.insert_char('h');
        engine.insert_char('i');
        assert_eq!(engine.input.buffer, "hi");
        assert_eq!(engine.input.cursor_pos, 2);
    }

    #[test]
    fn backspace_removes_char() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "ab".to_string();
        engine.input.cursor_pos = 2;
        engine.backspace();
        assert_eq!(engine.input.buffer, "a");
        assert_eq!(engine.input.cursor_pos, 1);
    }

    #[test]
    fn backspace_at_start_does_nothing() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "a".to_string();
        engine.input.cursor_pos = 0;
        engine.backspace();
        assert_eq!(engine.input.buffer, "a");
    }

    #[test]
    fn selection_works() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "hello world".to_string();
        engine.input.cursor_pos = 0;
        engine.start_selection();
        engine.input.cursor_pos = 5;
        assert_eq!(engine.selection(), Some((0, 5)));
        assert!(engine.has_selection());
    }

    #[test]
    fn select_all_selects_everything() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "test".to_string();
        engine.select_all();
        assert_eq!(engine.selection(), Some((0, 4)));
    }

    #[test]
    fn delete_selection_removes_text() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "hello world".to_string();
        engine.input.cursor_pos = 0;
        engine.start_selection();
        engine.input.cursor_pos = 5;
        let deleted = engine.delete_selection();
        assert_eq!(deleted, Some("hello".to_string()));
        assert_eq!(engine.input.buffer, " world");
    }

    #[test]
    fn clear_input_resets_state() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "text".to_string();
        engine.input.cursor_pos = 2;
        engine.input.selection_anchor = Some(0);
        engine.input.scroll_offset = 5;
        engine.clear_input();
        assert_eq!(engine.input.buffer, "");
        assert_eq!(engine.input.cursor_pos, 0);
        assert!(engine.input.selection_anchor.is_none());
        assert_eq!(engine.input.scroll_offset, 0);
    }

    #[test]
    fn empty_history_prev_does_nothing() {
        let mut engine = ChatEngine::new();
        engine.history_prev();
        assert_eq!(engine.input.buffer, "");
    }

    #[test]
    fn history_next_at_end_restores_stash() {
        let mut engine = ChatEngine::new();
        engine.input.buffer = "draft".to_string();
        engine.push_to_history("first".to_string());
        engine.history_prev();
        assert_eq!(engine.input.buffer, "first");
        engine.history_next();
        assert_eq!(engine.input.buffer, "draft");
    }

    #[test]
    fn scroll_page_up_down() {
        let mut engine = ChatEngine::new();
        engine.chat.scroll = 20;
        engine.scroll_page_up();
        assert_eq!(engine.chat.scroll, 10);
        engine.scroll_page_down();
        assert_eq!(engine.chat.scroll, 20);
    }
}
