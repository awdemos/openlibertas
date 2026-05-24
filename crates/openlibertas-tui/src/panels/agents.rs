use openlibertas_core::engine::AgentModeStatus;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;

pub struct AgentsPanelData {
    pub agent_selected: usize,
    pub status: AgentModeStatus,
    pub persona: String,
    pub max_iterations: usize,
    pub _current_iteration: usize,
    pub personas: Vec<(String, String)>,
}

impl AgentsPanelData {
    pub fn from_app(app: &crate::app::App) -> Self {
        let agents = app.engine.agents();
        Self {
            agent_selected: app.agent_selected,
            status: agents.status,
            persona: agents.persona.clone(),
            max_iterations: agents.max_iterations,
            _current_iteration: agents.current_iteration,
            personas: app.agent_personas(),
        }
    }
}

impl Panel for AgentsPanelData {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(50, 50, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Agent Configuration ")
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
                (match self.status {
                    AgentModeStatus::Disabled => "Disabled",
                    AgentModeStatus::Idle => "Enabled",
                    AgentModeStatus::Active => "Active",
                })
                .to_string(),
            ),
            ("Persona", self.persona.clone()),
            ("Max Iterations", format!("{}", self.max_iterations)),
        ];

        let mut items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(i, (label, value))| {
                let is_selected = i == self.agent_selected;
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

        items.push(ListItem::new(""));
        items.push(
            ListItem::new("Available Specialists:").style(
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
        );

        for (name, _) in self.personas.iter().filter(|(n, _)| n != &self.persona) {
            items.push(
                ListItem::new(format!("  • {}", name))
                    .style(Style::default().fg(theme.secondary())),
            );
        }

        items.push(ListItem::new(""));
        items.push(
            ListItem::new("Agents can delegate to specialists using the switch_persona tool.")
                .style(Style::default().fg(theme.system_color())),
        );

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
            Span::styled(" Change  ", Style::default().fg(theme.system_color())),
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
