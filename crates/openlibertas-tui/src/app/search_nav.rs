use crate::app::App;
use openlibertas_core::domain::Role;

impl App {
    pub fn search_next(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        self.search.index = (self.search.index + 1) % self.search.matches.len();
        self.scroll_to_match();
    }

    pub fn search_prev(&mut self) {
        if self.search.matches.is_empty() {
            return;
        }
        if self.search.index == 0 {
            self.search.index = self.search.matches.len() - 1;
        } else {
            self.search.index -= 1;
        }
        self.scroll_to_match();
    }

    fn scroll_to_match(&mut self) {
        if let Some(m) = self.search.matches.get(self.search.index) {
            let target = m.message_index;
            let mut line_count = 0;
            for (i, msg) in self.engine.chat_mut().messages.iter().enumerate() {
                if i == target {
                    self.engine.chat_mut().scroll = line_count;
                    self.engine.chat_mut().auto_scroll = false;
                    break;
                }
                if msg.role == Role::System && msg.is_prompt {
                    continue;
                }
                line_count += 3;
                if msg.tool_calls.is_some() {
                    line_count += 2;
                }
                let content_lines = msg.content.lines().count();
                line_count += content_lines.max(1);
            }
        }
    }
}
