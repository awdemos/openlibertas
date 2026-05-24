use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub struct ThemesPanelData {
    pub theme_selected: usize,
    pub current_theme_name: &'static str,
}

impl ThemesPanelData {
    pub fn from_app(app: &crate::app::App) -> Self {
        Self {
            theme_selected: app.theme_selected,
            current_theme_name: app.theme.name(),
        }
    }
}

impl Panel for ThemesPanelData {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(50, 70, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Select Theme ")
            .title_style(
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.border_color()));

        let inner = popup_area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };

        let themes = Theme::all();
        let items: Vec<ListItem> = themes
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                let is_current = i == self.theme_selected;
                let style = if is_current {
                    Style::default()
                        .bg(theme.primary())
                        .fg(theme.panel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground())
                };
                let marker = if *name == self.current_theme_name {
                    "● "
                } else {
                    "  "
                };
                ListItem::new(format!("{}{}", marker, name)).style(style)
            })
            .collect();

        let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(list, content_area);
        frame.render_widget(block, popup_area);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled(
                "↑/↓",
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
            Span::styled(" to close", Style::default().fg(theme.system_color())),
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
