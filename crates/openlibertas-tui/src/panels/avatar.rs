use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;

pub struct AvatarMenuPanelData {
    pub avatar_enabled: bool,
    pub avatar_menu_selected: usize,
    pub anim_speed: Option<f32>,
    pub velocity_x: Option<f32>,
    pub velocity_y: Option<f32>,
}

impl AvatarMenuPanelData {
    pub fn from_app(app: &crate::app::App) -> Self {
        let first = app.avatars.first();
        Self {
            avatar_enabled: app.avatar_enabled,
            avatar_menu_selected: app.avatar_menu_selected,
            anim_speed: first.map(|a| a.anim_speed),
            velocity_x: first.map(|a| a.velocity.0),
            velocity_y: first.map(|a| a.velocity.1),
        }
    }
}

impl Panel for AvatarMenuPanelData {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(50, 50, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Avatar Configuration ")
            .title_style(
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.border_color()));

        let inner = popup_area.inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(1),
        };

        let options = [
            (
                "Status",
                if self.avatar_enabled {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .to_string(),
            ),
            (
                "Animation Speed",
                self.anim_speed
                    .map(|a| format!("{:.0} ms", a))
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Velocity X",
                self.velocity_x
                    .map(|a| format!("{:.2}", a))
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
            (
                "Velocity Y",
                self.velocity_y
                    .map(|a| format!("{:.2}", a))
                    .unwrap_or_else(|| "N/A".to_string()),
            ),
        ];

        let items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(i, (label, value))| {
                let is_selected = i == self.avatar_menu_selected;
                let style = if is_selected {
                    Style::default()
                        .bg(theme.primary())
                        .fg(theme.panel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground())
                };
                let marker = if is_selected { "▸ " } else { "  " };
                ListItem::new(format!("{}{}: {}", marker, label, value)).style(style)
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
            Span::styled(
                " Toggle/Adjust  ",
                Style::default().fg(theme.system_color()),
            ),
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
