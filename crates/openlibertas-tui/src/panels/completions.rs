use crate::panels::Panel;
use crate::theme::Theme;
use openlibertas_core::completion::{CompletionItem, CompletionType};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem},
    Frame,
};

pub struct CompletionsPopupData<'a> {
    pub items: &'a [CompletionItem],
    pub selected: usize,
    pub input_area: Rect,
}

impl<'a> CompletionsPopupData<'a> {
    pub fn from_app(app: &'a crate::app::App, input_area: Rect) -> Self {
        Self {
            items: app.completion_items(),
            selected: app.completion_selected(),
            input_area,
        }
    }
}

impl Panel for CompletionsPopupData<'_> {
    fn draw(&self, frame: &mut Frame, _area: Rect, theme: &Theme) {
        if self.items.is_empty() {
            return;
        }

        let count = self.items.len().min(10) as u16;
        let height = count + 2;
        let area = Rect {
            x: self.input_area.x,
            y: self.input_area.y.saturating_sub(height),
            width: self.input_area.width,
            height,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(match self.items.first().map(|i| i.kind) {
                Some(CompletionType::SlashCommand) => " Commands ",
                Some(CompletionType::FilePath) => " Files ",
                Some(CompletionType::Model) => " Models ",
                None => " ",
            })
            .title_style(
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.border_color()));

        let inner = area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 0,
        });

        let list_items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let style = if i == self.selected {
                    Style::default()
                        .bg(theme.primary())
                        .fg(theme.panel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground())
                };

                let marker = if i == self.selected { "▸ " } else { "  " };
                let text = if item.description.is_empty() {
                    format!("{}{}", marker, item.label)
                } else {
                    format!("{}{}  {}", marker, item.label, item.description)
                };
                ListItem::new(text).style(style)
            })
            .collect();

        let list =
            List::new(list_items).highlight_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(Clear, area);
        frame.render_widget(list, inner);
        frame.render_widget(block, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn completions_popup_renders() {
        let items = vec![CompletionItem {
            label: "/help".to_string(),
            description: "Show help".to_string(),
            kind: CompletionType::SlashCommand,
        }];
        let data = CompletionsPopupData {
            items: &items,
            selected: 0,
            input_area: Rect::new(0, 10, 80, 3),
        };
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| data.draw(f, f.area(), &Theme::default()))
            .unwrap();
    }
}
