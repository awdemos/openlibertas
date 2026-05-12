use crate::completion::{CompletionEngine, CompletionType};

use super::ChatEngine;

impl ChatEngine {
    pub fn refresh_completions(&mut self, models: &[crate::domain::Model]) {
        let buffer = self.input.buffer.clone();
        let cursor = self.input.cursor_pos;
        self.input.completion_items = self.completion_engine.complete(&buffer, cursor, models);
        self.input.completion_active = !self.input.completion_items.is_empty();
        self.input.autocomplete_index = 0;
    }

    pub fn clear_completions(&mut self) {
        self.input.completion_active = false;
        self.input.completion_items.clear();
        self.input.autocomplete_index = 0;
    }

    pub fn cycle_completion_next(&mut self) {
        if !self.input.completion_items.is_empty() {
            self.input.autocomplete_index =
                (self.input.autocomplete_index + 1) % self.input.completion_items.len();
        }
    }

    pub fn cycle_completion_prev(&mut self) {
        if !self.input.completion_items.is_empty() {
            if self.input.autocomplete_index == 0 {
                self.input.autocomplete_index = self.input.completion_items.len() - 1;
            } else {
                self.input.autocomplete_index -= 1;
            }
        }
    }

    pub fn apply_completion(&mut self) -> bool {
        if !self.input.completion_active || self.input.completion_items.is_empty() {
            return false;
        }
        let idx = self.input.autocomplete_index;
        if let Some(item) = self.input.completion_items.get(idx).cloned() {
            crate::completion::accept_completion(
                &mut self.input.buffer,
                &mut self.input.cursor_pos,
                &item,
            );
            // For file paths, keep input mode active but clear completions
            // For slash commands and models, also clear
            self.input.completion_active = false;
            self.input.completion_items.clear();
            self.input.autocomplete_index = 0;
            true
        } else {
            false
        }
    }

    pub fn completion_type(&self) -> Option<CompletionType> {
        CompletionEngine::detect_completion_type(&self.input.buffer, self.input.cursor_pos)
    }
}
