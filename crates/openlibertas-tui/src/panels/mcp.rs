use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;
use openlibertas_core::domain::McpServerStatus;
use openlibertas_core::mcp::McpTool;
use ratatui::{
    layout::{Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};

pub struct McpPanelData<'a> {
    pub servers: Vec<String>,
    pub diagnostics:
        std::collections::HashMap<String, openlibertas_core::mcp::McpServerDiagnostics>,
    pub health: &'a std::collections::HashMap<String, bool>,
    pub tools_for_server: std::collections::HashMap<String, Vec<McpTool>>,
    pub test_result: Option<&'a String>,
    pub selected_server: usize,
    pub selected_tool: usize,
    pub scroll: usize,
    pub show_detail: bool,
}

impl<'a> McpPanelData<'a> {
    pub fn from_app(app: &'a crate::app::App) -> Self {
        let servers = app.mcp_server_names();
        let diagnostics = app.engine.tools().diagnostics();
        let mut tools_map = std::collections::HashMap::new();
        for name in &servers {
            tools_map.insert(name.clone(), app.mcp_tools_for_server(name));
        }
        Self {
            servers,
            diagnostics: diagnostics.clone(),
            health: &app.mcp_health,
            tools_for_server: tools_map,
            test_result: app.mcp_test_result.as_ref(),
            selected_server: app.mcp_selected_server,
            selected_tool: app.mcp_selected_tool,
            scroll: app.mcp_scroll,
            show_detail: app.mcp_show_detail,
        }
    }
}

impl Panel for McpPanelData<'_> {
    fn draw(&self, frame: &mut Frame, _area: Rect, theme: &Theme) {
        let area = frame.area();
        let popup_area = centered_rect(80, 70, area);

        frame.render_widget(Clear, popup_area);

        let total_servers = self.servers.len();
        let connected_count = self
            .diagnostics
            .values()
            .filter(|d| d.status == McpServerStatus::Connected)
            .count();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " MCP Servers ({}/{}) ",
                connected_count, total_servers
            ))
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
        let footer_height = 1u16;
        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: inner.height.saturating_sub(footer_height),
        };

        if self.servers.is_empty() {
            let content = Paragraph::new(
                "No MCP servers configured.\nAdd servers to ~/.config/opencode/opencode.json",
            )
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(theme.system_color()));
            frame.render_widget(content, content_area);
        } else {
            let mut lines: Vec<Line> = Vec::new();

            for (i, name) in self.servers.iter().enumerate() {
                let is_selected = i == self.selected_server;
                let diag = self.diagnostics.get(name);
                let status = diag.map(|d| d.status);

                let (indicator, color) = match status {
                    Some(McpServerStatus::Connected) => ("●", theme.user_color()),
                    Some(McpServerStatus::Connecting) => ("◐", theme.secondary()),
                    Some(McpServerStatus::Failed) => ("✗", theme.error_color()),
                    Some(McpServerStatus::Disabled) => ("○", theme.system_color()),
                    Some(McpServerStatus::Pending) => ("○", theme.system_color()),
                    None => ("?", theme.system_color()),
                };

                let server_type = diag.map(|d| d.server_type.as_str()).unwrap_or("unknown");
                let tool_count = diag.map(|d| d.tool_count).unwrap_or(0);
                let health = self.health.get(name);

                let name_style = if is_selected {
                    Style::default()
                        .bg(theme.primary())
                        .fg(theme.panel_bg())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground())
                };
                let meta_style = if is_selected {
                    Style::default().bg(theme.primary()).fg(theme.panel_bg())
                } else {
                    Style::default().fg(theme.system_color())
                };

                let marker = if is_selected { "▸ " } else { "  " };
                let health_indicator = match health {
                    Some(true) => " ✓",
                    Some(false) => " ✗",
                    None => "",
                };

                lines.push(Line::from(vec![
                    Span::styled(marker, Style::default().fg(theme.primary())),
                    Span::styled(format!("{} ", indicator), Style::default().fg(color)),
                    Span::styled(name.clone(), name_style),
                    Span::styled(
                        format!(" [{}]", server_type),
                        meta_style.add_modifier(Modifier::ITALIC),
                    ),
                    Span::styled(
                        format!(" {} tools{}", tool_count, health_indicator),
                        meta_style,
                    ),
                ]));

                if let Some(diag) = diag {
                    if let Some(ref err) = diag.last_error {
                        let err_text = if err.len() > 60 {
                            format!("{}...", &err[..57])
                        } else {
                            err.clone()
                        };
                        lines.push(Line::from(vec![
                            Span::styled("     ", Style::default()),
                            Span::styled(
                                format!("Error: {}", err_text),
                                if is_selected {
                                    Style::default().bg(theme.primary()).fg(theme.error_color())
                                } else {
                                    Style::default().fg(theme.error_color())
                                },
                            ),
                        ]));
                    }
                }

                if is_selected {
                    let server_tools: Vec<_> =
                        self.tools_for_server.get(name).cloned().unwrap_or_default();

                    if !server_tools.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            "     Tools:",
                            if is_selected {
                                Style::default()
                                    .bg(theme.primary())
                                    .fg(theme.panel_bg())
                                    .add_modifier(Modifier::BOLD)
                            } else {
                                Style::default()
                                    .fg(theme.secondary())
                                    .add_modifier(Modifier::BOLD)
                            },
                        )]));

                        for (ti, tool) in server_tools.iter().enumerate() {
                            let is_tool_selected = ti == self.selected_tool;
                            let tool_marker = if is_tool_selected { "   ▸ " } else { "     " };
                            let tool_style = if is_selected {
                                if is_tool_selected {
                                    Style::default()
                                        .bg(theme.primary())
                                        .fg(theme.panel_bg())
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().bg(theme.primary()).fg(theme.panel_bg())
                                }
                            } else {
                                Style::default().fg(theme.foreground())
                            };
                            lines.push(Line::from(vec![
                                Span::styled(tool_marker, Style::default().fg(theme.primary())),
                                Span::styled(tool.name.clone(), tool_style),
                            ]));
                        }
                    }
                }

                lines.push(Line::from(""));
            }

            if let Some(ref result) = self.test_result {
                lines.push(Line::from(vec![Span::styled(
                    "─".repeat(content_area.width as usize),
                    Style::default().fg(theme.border_color()),
                )]));
                lines.push(Line::from(vec![
                    Span::styled(
                        "Test: ",
                        Style::default()
                            .fg(theme.primary())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        result.to_string(),
                        if result.starts_with("✓") {
                            Style::default().fg(theme.user_color())
                        } else {
                            Style::default().fg(theme.error_color())
                        },
                    ),
                ]));
            }

            let total_lines = lines.len();
            let viewport_height = content_area.height as usize;
            let max_scroll = total_lines.saturating_sub(viewport_height);
            let scroll = self.scroll.min(max_scroll);

            let content = Paragraph::new(Text::from(lines))
                .scroll((scroll as u16, 0))
                .wrap(Wrap { trim: true });
            frame.render_widget(content, content_area);

            if total_lines > viewport_height {
                let scrollbar = Scrollbar::default()
                    .orientation(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("↑"))
                    .end_symbol(Some("↓"));
                let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
                frame.render_stateful_widget(
                    scrollbar,
                    content_area.inner(Margin {
                        horizontal: 0,
                        vertical: 0,
                    }),
                    &mut scrollbar_state,
                );
            }
        }

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
                "r",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Refresh  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "t",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Test tool  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "d",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Detail  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Close", Style::default().fg(theme.system_color())),
        ]))
        .alignment(ratatui::layout::Alignment::Center);
        let footer_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        frame.render_widget(footer, footer_area);

        if self.show_detail {
            draw_mcp_detail_popup(frame, self, theme);
        }
    }
}

fn draw_mcp_detail_popup(frame: &mut Frame, data: &McpPanelData, theme: &Theme) {
    let area = frame.area();
    let popup_area = centered_rect(70, 50, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" MCP Server Details ")
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

    let content = if let Some(name) = data.servers.get(data.selected_server) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                "Server: ",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(name.clone(), Style::default().fg(theme.foreground())),
        ]));

        if let Some(diag) = data.diagnostics.get(name) {
            let status_text = format!("{:?}", diag.status);
            let status_color = match diag.status {
                McpServerStatus::Connected => theme.user_color(),
                McpServerStatus::Failed => theme.error_color(),
                _ => theme.secondary(),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    "Status: ",
                    Style::default()
                        .fg(theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(status_text, Style::default().fg(status_color)),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "Type: ",
                    Style::default()
                        .fg(theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    diag.server_type.clone(),
                    Style::default().fg(theme.foreground()),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "Tools: ",
                    Style::default()
                        .fg(theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}", diag.tool_count),
                    Style::default().fg(theme.foreground()),
                ),
            ]));

            if let Some(health) = data.health.get(name) {
                lines.push(Line::from(vec![
                    Span::styled(
                        "Health: ",
                        Style::default()
                            .fg(theme.primary())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        if *health { "Alive" } else { "Dead" },
                        if *health {
                            Style::default().fg(theme.user_color())
                        } else {
                            Style::default().fg(theme.error_color())
                        },
                    ),
                ]));
            }

            if let Some(ref err) = diag.last_error {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "Last Error:",
                    Style::default()
                        .fg(theme.error_color())
                        .add_modifier(Modifier::BOLD),
                )]));
                for line in err.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(theme.error_color()),
                    )));
                }
            }
        }

        let tools: Vec<_> = data.tools_for_server.get(name).cloned().unwrap_or_default();
        if !tools.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Available Tools:",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )]));
            for tool in tools {
                lines.push(Line::from(vec![
                    Span::styled("  • ", Style::default().fg(theme.primary())),
                    Span::styled(
                        tool.name.clone(),
                        Style::default()
                            .fg(theme.secondary())
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(vec![Span::styled(
                    format!("    {}", tool.description),
                    Style::default().fg(theme.system_color()),
                )]));
            }
        }

        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true })
    } else {
        Paragraph::new("No server selected.").alignment(ratatui::layout::Alignment::Center)
    };

    frame.render_widget(content, inner);
    frame.render_widget(block, popup_area);
}
