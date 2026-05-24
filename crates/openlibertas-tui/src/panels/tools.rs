use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;
use openlibertas_core::mcp::McpTool;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub struct ToolsPanelData {
    pub available_tools: Vec<McpTool>,
}

impl ToolsPanelData {
    pub fn from_app(app: &crate::app::App) -> Self {
        Self {
            available_tools: app.engine.tools().available_tools(),
        }
    }
}

impl Panel for ToolsPanelData {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(80, 80, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" MCP Tools ({}) ", self.available_tools.len()))
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

        if self.available_tools.is_empty() {
            let content = Paragraph::new(
                "No tools available.\nCheck MCP server configuration in ~/.config/opencode/opencode.json",
            )
            .alignment(Alignment::Center);
            frame.render_widget(content, content_area);
        } else {
            let tool_lines: Vec<Line> = self
                .available_tools
                .iter()
                .flat_map(|tool| {
                    vec![
                        Line::from(vec![
                            Span::styled("• ", Style::default().fg(theme.primary())),
                            Span::styled(
                                &tool.name,
                                Style::default()
                                    .fg(theme.secondary())
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::from(vec![Span::styled(
                            format!("  {}", tool.description),
                            Style::default().fg(theme.system_color()),
                        )]),
                        Line::from(""),
                    ]
                })
                .collect();

            let content = Paragraph::new(Text::from(tool_lines)).wrap(Wrap { trim: true });
            frame.render_widget(content, content_area);
        }

        frame.render_widget(block, popup_area);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to close  |  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "/tools",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to toggle", Style::default().fg(theme.system_color())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn tools_panel_renders_empty() {
        let data = ToolsPanelData {
            available_tools: vec![],
        };
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| data.draw(f, f.area(), &Theme::default()))
            .unwrap();
    }

    #[test]
    fn tools_panel_renders_with_tools() {
        let data = ToolsPanelData {
            available_tools: vec![McpTool {
                name: "search".to_string(),
                description: "Search the web".to_string(),
                input_schema: serde_json::Value::Null,
            }],
        };
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| data.draw(f, f.area(), &Theme::default()))
            .unwrap();
    }
}
