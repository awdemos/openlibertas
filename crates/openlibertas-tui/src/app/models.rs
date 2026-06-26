use crate::app::App;

impl App {
    pub fn select_next_model(&mut self) {
        if !self.models.models.is_empty() {
            self.models.selected = (self.models.selected + 1).min(self.models.models.len() - 1);
        }
    }

    pub fn select_prev_model(&mut self) {
        if !self.models.models.is_empty() {
            self.models.selected = self.models.selected.saturating_sub(1);
        }
    }

    pub fn select_current_model(&mut self) {
        if let Some(model) = self.models.models.get(self.models.selected) {
            let provider = model.provider.clone();
            self.models.current = Some(model.id.clone());
            self.set_provider(provider);
            self.screen = crate::app::Screen::Chat;
        }
    }
}
