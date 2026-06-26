use crate::app::App;
use openlibertas_core::completion::CompletionItem;

impl App {
    pub fn refresh_completions(&mut self) {
        self.engine.refresh_completions(&self.models.models);
    }

    pub fn clear_completions(&mut self) {
        self.engine.clear_completions();
    }

    pub fn cycle_completion_next(&mut self) {
        self.engine.cycle_completion_next();
    }

    pub fn cycle_completion_prev(&mut self) {
        self.engine.cycle_completion_prev();
    }

    pub fn apply_completion(&mut self) -> bool {
        self.engine.apply_completion()
    }

    pub fn completion_active(&self) -> bool {
        self.engine.input().completion_active
    }

    pub fn completion_items(&self) -> &[CompletionItem] {
        &self.engine.input().completion_items
    }

    pub fn completion_selected(&self) -> usize {
        self.engine.input().autocomplete_index
    }
}
