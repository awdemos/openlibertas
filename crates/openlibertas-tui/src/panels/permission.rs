use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;
use openlibertas_core::permission::PermissionRequest;

pub struct PermissionPanelData {
    pub request: PermissionRequest,
}

impl PermissionPanelData {
    pub fn from_app(app: &crate::app::App) -> Option<Self> {
        app.pending_permission_request
            .clone()
            .map(|request| Self { request })
    }
}

impl Panel for PermissionPanelData {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(70, 50, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Permission Required ")
            .title_style(
                Style::default()
                    .fg(theme.error_color())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.error_color()));

        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                "Tool: ",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &self.request.tool_name,
                Style::default().fg(theme.foreground()),
            ),
        ]));

        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled(
                "Action: ",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &self.request.action,
                Style::default().fg(theme.foreground()),
            ),
        ]));

        lines.push(Line::from(""));

        if let Some(desc) = self.request.description.strip_prefix("Execute: ") {
            lines.push(Line::from(vec![
                Span::styled(
                    "Command: ",
                    Style::default()
                        .fg(theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(desc, Style::default().fg(theme.primary())),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    "Description: ",
                    Style::default()
                        .fg(theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &self.request.description,
                    Style::default().fg(theme.foreground()),
                ),
            ]));
        }

        if let Some(params) = self.request.params.as_object() {
            let has_displayable = params.iter().any(|(k, v)| k != "command" && !v.is_null());
            if has_displayable {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Parameters:",
                    Style::default()
                        .fg(theme.secondary())
                        .add_modifier(Modifier::BOLD),
                )]));
                for (key, value) in params {
                    if key == "command" || value.is_null() {
                        continue;
                    }
                    let value_str = match value {
                        serde_json::Value::String(s) => s.clone(),
                        _ => value.to_string(),
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("  {}: ", key),
                            Style::default().fg(theme.secondary()),
                        ),
                        Span::styled(value_str, Style::default().fg(theme.foreground())),
                    ]));
                }
            }
        }

        if !self.request.path.is_empty() && self.request.path != "/" {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "Path: ",
                    Style::default()
                        .fg(theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(&self.request.path, Style::default().fg(theme.foreground())),
            ]));
        }

        if let Some(ref diff) = self.request.diff {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Diff Preview:",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            )]));
            for line in diff.lines() {
                let style = if line.starts_with('+') {
                    Style::default().fg(theme.primary())
                } else if line.starts_with('-') {
                    Style::default().fg(theme.error_color())
                } else if line.starts_with("@@") {
                    Style::default()
                        .fg(theme.secondary())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground())
                };
                lines.push(Line::from(Span::styled(line.to_string(), style)));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "─".repeat(popup_area.width.saturating_sub(4) as usize),
            Style::default().fg(theme.border_color()),
        )]));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("[", Style::default().fg(theme.system_color())),
            Span::styled(
                "a",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("] Allow once  ", Style::default().fg(theme.system_color())),
            Span::styled("[", Style::default().fg(theme.system_color())),
            Span::styled(
                "s",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "] Allow session  ",
                Style::default().fg(theme.system_color()),
            ),
            Span::styled("[", Style::default().fg(theme.system_color())),
            Span::styled(
                "d",
                Style::default()
                    .fg(theme.error_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("] Deny", Style::default().fg(theme.system_color())),
        ]));

        let content = Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(content, popup_area);
    }
}
