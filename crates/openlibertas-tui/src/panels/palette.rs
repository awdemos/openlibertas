use crate::panels::Panel;
use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub struct PalettePanelData<'a> {
    pub commands: &'a [(String, String)],
    pub selected: usize,
    pub input_area: Rect,
}

impl<'a> PalettePanelData<'a> {
    pub fn from_app(app: &'a crate::app::App, input_area: Rect) -> Self {
        Self {
            commands: &app.palette_commands,
            selected: app.palette_selected,
            input_area,
        }
    }
}

impl Panel for PalettePanelData<'_> {
    fn draw(&self, frame: &mut Frame, _area: Rect, theme: &Theme) {
        let cmd_count = self.commands.len() as u16;
        if cmd_count == 0 {
            return;
        }
        let height = (cmd_count + 3).min(14);
        let area = Rect {
            x: self.input_area.x,
            y: self.input_area.y.saturating_sub(height),
            width: self.input_area.width,
            height,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Commands ")
            .title_style(
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.border_color()));

        let inner = area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };

        let items: Vec<ListItem> = self
            .commands
            .iter()
            .enumerate()
            .map(|(i, (cmd, desc))| {
                let style = if i == self.selected {
                    Style::default()
                        .bg(theme.primary())
                        .fg(theme.panel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(format!("{}  {}", cmd, desc)).style(style)
            })
            .collect();

        let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(list, content_area);
        frame.render_widget(block, area);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled(
                "↑/↓",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" or ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Navigate  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Select  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Close", Style::default().fg(theme.system_color())),
        ]))
        .alignment(Alignment::Center);
        let footer_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(footer, footer_area);
    }
}
