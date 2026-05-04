use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ConnectionStatus, Screen};
use openlibertas_core::domain::Role;
use crate::theme::Theme;

pub fn draw(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Models => draw_models(frame, app),
        Screen::Chat => draw_chat(frame, app),
    }
}

fn draw_models(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(3)])
        .margin(1)
        .split(area);

    let title = Paragraph::new("OpenLibertas")
        .style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    frame.render_widget(title, chunks[0]);

    let sub_title = Paragraph::new("Select a Model")
        .style(Style::default().fg(app.theme.secondary()))
        .alignment(Alignment::Center);
    let sub_area = Rect {
        x: chunks[0].x,
        y: chunks[0].y + 1,
        width: chunks[0].width,
        height: 1,
    };
    frame.render_widget(sub_title, sub_area);

    if app.loading {
        let loading = Paragraph::new("Loading models...")
            .style(Style::default().fg(app.theme.secondary()))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(loading, chunks[1]);
    } else if let Some(ref err) = app.error {
        let error_msg = Paragraph::new(format!("Error: {}", err))
            .style(Style::default().fg(app.theme.error_color()))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL));
        frame.render_widget(error_msg, chunks[1]);
    } else if app.models.models.is_empty() {
        let empty = Paragraph::new(
            "No models found.\nMake sure your provider is running.",
        )
        .style(Style::default().fg(app.theme.secondary()))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(empty, chunks[1]);
    } else {
        let mut items: Vec<ListItem> = Vec::new();
        let mut current_provider = "";

        for (i, m) in app.models.models.iter().enumerate() {
            let provider = m.provider.as_str();
            if provider != current_provider {
                if !items.is_empty() {
                    items.push(ListItem::new(Line::from(Span::styled(
                        "─".repeat((chunks[1].width as usize).saturating_sub(2)),
                        Style::default().fg(app.theme.border_color()),
                    ))));
                }
                let provider_color = provider_color(provider, app.theme);
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(format!("[{}]", provider.to_uppercase()),
                        Style::default().fg(provider_color).add_modifier(Modifier::BOLD)),
                ])).style(Style::default().add_modifier(Modifier::ITALIC)));
                current_provider = provider;
            }

            let is_selected = i == app.models.selected;
            let style = if is_selected {
                Style::default()
                    .bg(app.theme.primary())
                    .fg(app.theme.panel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground())
            };

            let marker = if is_selected { "▸ " } else { "  " };
            let tool_indicator = if m.supports_tools { " ⚡" } else { "" };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(app.theme.primary())),
                Span::styled(m.id.clone(), style),
                Span::styled(tool_indicator, Style::default().fg(app.theme.secondary()).add_modifier(Modifier::DIM)),
            ])));
        }

        let total_models = app.models.models.len();
        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(format!(" Models ({}) ", total_models))
                .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(list, chunks[1]);
    }

    let footer_spans = vec![
        Span::styled("↑/↓", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Enter", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Select  ", Style::default().fg(app.theme.system_color())),
        Span::styled("q", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Quit (press twice)", Style::default().fg(app.theme.system_color())),
    ];
    let help = Paragraph::new(Line::from(footer_spans))
        .style(Style::default().fg(app.theme.system_color()))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[2]);
}

fn draw_chat(frame: &mut Frame, app: &App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .margin(1)
        .split(frame.area());

    let model_name = app.models.current.as_deref().unwrap_or("Unknown");

    let (conn_symbol, conn_color) = match app.connection_status {
        ConnectionStatus::Connected => ("●", app.theme.user_color()),
        ConnectionStatus::Disconnected => ("●", app.theme.error_color()),
        ConnectionStatus::Checking => ("◐", app.theme.secondary()),
    };

    let display_model = if app.agents.status == crate::app::AgentStatus::Disabled {
        model_name.to_string()
    } else {
        format!("{}@{}", app.agents.persona, model_name)
    };

    let tool_indicator = if app.mcp.available_tools.is_empty() {
        ""
    } else {
        " [Tools: ✓]"
    };

    let mcp_indicator = if app.mcp.client.is_some() {
        " [MCP: ✓]"
    } else {
        " [MCP: ✗]"
    };

    let agent_indicator = match app.agents.status {
        crate::app::AgentStatus::Disabled => String::new(),
        crate::app::AgentStatus::Idle => {
            format!(" [Agents: ○ Ready]")
        }
        crate::app::AgentStatus::Active => {
            format!(" [Agents: ● {}/{}]", app.agents.current_iteration, app.agents.max_iterations)
        }
    };

    let (search_indicator, search_snippet) = if app.search.active && !app.search.matches.is_empty() {
        let m = &app.search.matches[app.search.index];
        let snippet = app.chat.messages.get(m.message_index)
            .map(|msg| m.snippet(&msg.content))
            .unwrap_or("");
        let indicator = format!("Search: {}/{}", app.search.index + 1, app.search.matches.len());
        let preview = if snippet.len() > 20 {
            format!(" ...'{}'...", &snippet[..20])
        } else {
            format!(" ...'{}'...", snippet)
        };
        (indicator, preview)
    } else {
        (String::new(), String::new())
    };

    let base_url = app.config.providers.first()
        .map(|p| {
            let url = p.base_url.as_str();
            if url.len() > 30 {
                format!("{}...", &url[..27])
            } else {
                url.to_string()
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let left_spans = vec![
        Span::styled("OpenLibertas ", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{} ", conn_symbol), Style::default().fg(conn_color)),
        Span::styled(display_model.clone(), Style::default().fg(app.theme.foreground()).add_modifier(Modifier::BOLD)),
    ];

    let mut right_parts = vec![base_url.clone()];
    if !tool_indicator.is_empty() {
        right_parts.push(tool_indicator.trim_start().to_string());
    }
    if !mcp_indicator.is_empty() {
        right_parts.push(mcp_indicator.trim_start().to_string());
    }
    if !agent_indicator.is_empty() {
        right_parts.push(agent_indicator.trim_start().to_string());
    }
    if !search_indicator.is_empty() {
        right_parts.push(search_indicator);
    }
    let right_text = right_parts.join(" | ");

    let header_lines = if search_snippet.is_empty() {
        vec![Line::from(
            left_spans.into_iter()
                .chain(std::iter::once(Span::styled(
                    format!("{:>1$}", right_text, main_chunks[0].width as usize - 16),
                    Style::default().fg(app.theme.system_color()),
                )))
                .collect::<Vec<_>>()
        )]
    } else {
        vec![
            Line::from(
                left_spans.into_iter()
                    .chain(std::iter::once(Span::styled(
                        format!("{:>1$}", right_text, main_chunks[0].width as usize - 16),
                        Style::default().fg(app.theme.system_color()),
                    )))
                    .collect::<Vec<_>>()
            ),
            Line::from(Span::styled(search_snippet, Style::default().fg(app.theme.secondary()))),
        ]
    };

    let header = Paragraph::new(Text::from(header_lines));
    frame.render_widget(header, main_chunks[0]);

    let current_match_msg = app.search.matches.get(app.search.index).map(|m| m.message_index);

    let is_last_msg_streaming = app.chat.streaming && app.chat.messages.last().map(|m| m.role == Role::Assistant).unwrap_or(false);
    let spinner_frame = app.chat.spinner_frame % crate::app::SPINNER_FRAMES.len();

    let messages_text: Vec<Line> = app.chat.messages
        .iter()
        .enumerate()
        .flat_map(|(msg_idx, msg)| {
            let model_name = app.models.current.as_deref().unwrap_or("AI");
            let (label, color) = match msg.role {
                Role::User => ("You", app.theme.user_color()),
                Role::Assistant => (model_name, app.theme.assistant_color()),
                Role::Tool => ("🔧 Tool", app.theme.tool_color()),
                Role::System => ("System", app.theme.system_color()),
            };

            let is_current_match = current_match_msg == Some(msg_idx);
            let has_match = app.search.matches.iter().any(|m| m.message_index == msg_idx);
            let is_last = msg_idx == app.chat.messages.len().saturating_sub(1);

            let bg_style = if is_current_match {
                Style::default().bg(app.theme.secondary()).fg(app.theme.panel_bg())
            } else if has_match {
                Style::default().bg(app.theme.border_color())
            } else {
                Style::default()
            };

            if let Some(ref tool_calls) = msg.tool_calls {
                let mut lines = vec![];
                for tc in tool_calls {
                    let key_arg = App::extract_key_argument(&tc.function.name, &tc.function.arguments);
                    let display = if key_arg.is_empty() {
                        format!("Using tool: {}", tc.function.name)
                    } else {
                        format!("{}: {}", tc.function.name, key_arg)
                    };
                    lines.push(Line::from(vec![
                        Span::styled("🔧 ", Style::default().fg(app.theme.tool_color())),
                        Span::styled(
                            display,
                            Style::default().fg(app.theme.tool_color()).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    if key_arg.is_empty() {
                        lines.push(Line::from(vec![Span::styled(
                            format!("   Args: {}", tc.function.arguments),
                            Style::default().fg(app.theme.system_color()),
                        )]));
                    }
                }
                lines.push(Line::from(""));
                if is_current_match || has_match {
                    for line in &mut lines {
                        *line = Line::from(line.spans.clone()).style(bg_style);
                    }
                }
                lines
            } else {
                let mut lines = vec![Line::from(vec![Span::styled(
                    format!("{}: ", label),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )])];
                let viewport_width = main_chunks[1].width.saturating_sub(2) as usize;
                let wrapped_content = if viewport_width > 10 {
                    crate::markdown::wrap_markdown(&msg.content, viewport_width)
                } else {
                    msg.content.clone()
                };
                let rendered = app.markdown_renderer.render(&wrapped_content, app.theme);
                for line in rendered.lines {
                    lines.push(line);
                }
                if is_last && is_last_msg_streaming && msg.role == Role::Assistant {
                    lines.push(Line::from(vec![
                        Span::styled(crate::app::SPINNER_FRAMES[spinner_frame], Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
                        Span::styled(" generating...", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::ITALIC)),
                    ]));
                }
                lines.push(Line::from(""));
                if is_current_match || has_match {
                    for line in &mut lines {
                        *line = Line::from(line.spans.clone()).style(bg_style);
                    }
                }
                lines
            }
        })
        .collect();

    let total_lines = messages_text.len();
    let viewport_height = main_chunks[1].height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let scroll = if app.chat.auto_scroll {
        max_scroll
    } else {
        app.chat.scroll.min(max_scroll)
    };

    let messages_widget = if app.chat.messages.is_empty() && !app.chat.streaming {
        let model = app.models.current.as_deref().unwrap_or("AI");
        let welcome_lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("Welcome to ", Style::default().fg(app.theme.foreground())),
                Span::styled("OpenLibertas", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Model: ", Style::default().fg(app.theme.system_color())),
                Span::styled(model, Style::default().fg(app.theme.assistant_color()).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("Agent: ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    if app.agents.status == crate::app::AgentStatus::Disabled {
                        "Disabled".to_string()
                    } else {
                        format!("{} ({})", app.agents.persona, match app.agents.status {
                            crate::app::AgentStatus::Idle => "Ready",
                            crate::app::AgentStatus::Active => "Running",
                            _ => "",
                        })
                    },
                    Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("• Type a message and press ", Style::default().fg(app.theme.system_color())),
                Span::styled("Enter", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
                Span::styled(" to chat", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Use ", Style::default().fg(app.theme.system_color())),
                Span::styled("@path/to/file", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
                Span::styled(" to attach files", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Press ", Style::default().fg(app.theme.system_color())),
                Span::styled("/", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
                Span::styled(" for slash commands", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Press ", Style::default().fg(app.theme.system_color())),
                Span::styled("F1", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
                Span::styled(" or ", Style::default().fg(app.theme.system_color())),
                Span::styled("?", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
                Span::styled(" for help", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Press ", Style::default().fg(app.theme.system_color())),
                Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
                Span::styled(" to switch models", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Start typing below...", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::ITALIC)),
            ]),
        ];
        Paragraph::new(Text::from(welcome_lines))
            .block(Block::default().borders(Borders::ALL).title(" Chat ").title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)))
            .alignment(Alignment::Center)
    } else {
        Paragraph::new(Text::from(messages_text))
            .block(Block::default().borders(Borders::ALL).title(" Chat ").title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)))
            .scroll((scroll as u16, 0))
    };
    frame.render_widget(messages_widget, main_chunks[1]);

    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    frame.render_stateful_widget(
        scrollbar,
        main_chunks[1].inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut scrollbar_state,
    );

    let status_spans = vec![
        Span::styled(format!("{} ", conn_symbol), Style::default().fg(conn_color)),
        Span::styled(display_model.clone(), Style::default().fg(app.theme.foreground()).add_modifier(Modifier::BOLD)),
        Span::styled(" | ", Style::default().fg(app.theme.border_color())),
        Span::styled(base_url.clone(), Style::default().fg(app.theme.system_color())),
    ];
    let status_bar = Paragraph::new(Line::from(status_spans))
        .style(Style::default().fg(app.theme.system_color()));
    frame.render_widget(status_bar, main_chunks[2]);

    let prompt_symbol = if app.agents.status == crate::app::AgentStatus::Disabled {
        "> "
    } else {
        "✨ "
    };
    let input_text = format!("{}{}", prompt_symbol, app.input.buffer);
    let input = Paragraph::new(input_text.clone())
        .style(Style::default().fg(app.theme.foreground()));
    frame.render_widget(input, main_chunks[3]);

    let safe_cursor_pos = {
        let pos = app.input.cursor_pos.min(app.input.buffer.len());
        if app.input.buffer.is_char_boundary(pos) {
            pos
        } else {
            app.input.buffer.char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < pos)
                .last()
                .unwrap_or(0)
        }
    };
    let prompt_width = prompt_symbol.width() as u16;
    let cursor_x = main_chunks[3].x + prompt_width + app.input.buffer[..safe_cursor_pos].width() as u16;
    frame.set_cursor_position((cursor_x, main_chunks[3].y));

    if app.panels.show_tools {
        draw_tools_panel(frame, app);
    }

    if app.panels.show_mcp {
        draw_mcp_panel(frame, app);
    }

    if app.panels.show_sessions {
        draw_sessions_panel(frame, app);
    }

    if app.panels.show_palette && app.screen == Screen::Chat {
        draw_command_palette(frame, app, main_chunks[2]);
    }

    if app.panels.show_themes {
        draw_themes_panel(frame, app);
    }

    if app.panels.show_agents {
        draw_agents_panel(frame, app);
    }

    if app.panels.show_help {
        draw_help_panel(frame, app);
    }
}

fn draw_tools_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(80, 80, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" MCP Tools ({}) ", app.mcp.available_tools.len()))
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let inner = popup_area.inner(Margin { horizontal: 1, vertical: 1 });
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    if app.mcp.available_tools.is_empty() {
        let content = Paragraph::new("No tools available.\nCheck MCP server configuration in ~/.config/opencode/opencode.json")
            .alignment(Alignment::Center);
        frame.render_widget(content, content_area);
    } else {
        let tool_lines: Vec<Line> = app.mcp.available_tools
            .iter()
            .flat_map(|tool| {
                vec![
                    Line::from(vec![
                        Span::styled("• ", Style::default().fg(app.theme.primary())),
                        Span::styled(
                            &tool.name,
                            Style::default()
                                .fg(app.theme.secondary())
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![Span::styled(
                        format!("  {}", tool.description),
                        Style::default().fg(app.theme.system_color()),
                    )]),
                    Line::from(""),
                ]
            })
            .collect();

        let content = Paragraph::new(Text::from(tool_lines))
            .wrap(Wrap { trim: true });
        frame.render_widget(content, content_area);
    }

    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to close  |  ", Style::default().fg(app.theme.system_color())),
        Span::styled("/tools", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to toggle", Style::default().fg(app.theme.system_color())),
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

fn draw_mcp_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(80, 60, area);

    frame.render_widget(Clear, popup_area);

    let servers: Vec<String> = app.mcp.client.as_ref().map_or(Vec::new(), |c| c.server_names());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" MCP Servers ({}) ", servers.len()))
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let inner = popup_area.inner(Margin { horizontal: 1, vertical: 1 });
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    if servers.is_empty() {
        let content = Paragraph::new("No MCP servers configured.\nAdd servers to ~/.config/opencode/opencode.json")
            .alignment(Alignment::Center);
        frame.render_widget(content, content_area);
    } else {
        let lines: Vec<Line> = servers
            .into_iter()
            .map(|name| {
                let status = app.mcp.server_statuses.get(&name);
                let (indicator, color) = match status {
                    Some(openlibertas_core::domain::McpServerStatus::Connected) => ("●", app.theme.user_color()),
                    Some(openlibertas_core::domain::McpServerStatus::Connecting) => ("◐", app.theme.secondary()),
                    Some(openlibertas_core::domain::McpServerStatus::Failed) => ("✗", app.theme.error_color()),
                    Some(openlibertas_core::domain::McpServerStatus::Disabled) => ("○", app.theme.system_color()),
                    Some(openlibertas_core::domain::McpServerStatus::Pending) => ("○", app.theme.system_color()),
                    None => ("?", app.theme.system_color()),
                };
                let status_text = match status {
                    Some(s) => format!("{:?}", s),
                    None => "Unknown".to_string(),
                };
                Line::from(vec![
                    Span::styled(format!("{} ", indicator), Style::default().fg(color)),
                    Span::styled(name.clone(), Style::default().fg(app.theme.foreground())),
                    Span::styled(
                        format!(" ({})", status_text.to_lowercase()),
                        Style::default().fg(app.theme.system_color()),
                    ),
                ])
            })
            .collect();
        let content = Paragraph::new(Text::from(lines));
        frame.render_widget(content, content_area);
    }

    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to close  |  ", Style::default().fg(app.theme.system_color())),
        Span::styled("/mcp", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to toggle", Style::default().fg(app.theme.system_color())),
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

fn draw_sessions_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(80, 70, area);

    frame.render_widget(Clear, popup_area);

    let sessions = app.store.as_ref().map_or(Vec::new(), |store| {
        store.list_with_meta().unwrap_or_default()
    });

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Saved Sessions ({}) ", sessions.len()))
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let inner = popup_area.inner(Margin { horizontal: 1, vertical: 1 });
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    if sessions.is_empty() {
        let content = Paragraph::new("No saved sessions.\nUse /save to save the current conversation.")
            .alignment(Alignment::Center);
        frame.render_widget(content, content_area);
    } else {
        let lines: Vec<Line> = sessions
            .into_iter()
            .map(|(id, model, created)| {
                let model_str = model.as_deref().unwrap_or("unknown");
                let date = &created[..created.find('T').unwrap_or(created.len())];
                let time = &created[created.find('T').map(|i| i + 1).unwrap_or(0)..
                    created.find('.').unwrap_or(created.len())];
                Line::from(vec![
                    Span::styled("● ", Style::default().fg(app.theme.user_color())),
                    Span::styled(
                        id.to_string(),
                        Style::default()
                            .fg(app.theme.secondary())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  [{} | {} {}]", model_str, date, time),
                        Style::default().fg(app.theme.system_color()),
                    ),
                ])
            })
            .collect();
        let content = Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: true });
        frame.render_widget(content, content_area);
    }

    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to close  |  ", Style::default().fg(app.theme.system_color())),
        Span::styled("/load <name>", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to load  |  ", Style::default().fg(app.theme.system_color())),
        Span::styled("/sessions", Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to toggle", Style::default().fg(app.theme.system_color())),
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

fn draw_command_palette(frame: &mut Frame, app: &App, input_area: Rect) {
    let cmd_count = app.palette_commands.len() as u16;
    if cmd_count == 0 {
        return;
    }
    let height = (cmd_count + 3).min(14);
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width: input_area.width,
        height,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Commands ")
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let inner = area.inner(Margin { horizontal: 1, vertical: 1 });
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    let items: Vec<ListItem> = app
        .palette_commands
        .iter()
        .enumerate()
        .map(|(i, (cmd, desc))| {
            let style = if i == app.palette_selected {
                Style::default()
                    .bg(app.theme.primary())
                    .fg(app.theme.panel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{}  {}", cmd, desc)).style(style)
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, content_area);
    frame.render_widget(block, area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" or ", Style::default().fg(app.theme.system_color())),
        Span::styled("Tab", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Enter", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Select  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(app.theme.system_color())),
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

fn draw_themes_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(50, 70, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Theme ")
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let inner = popup_area.inner(Margin { horizontal: 1, vertical: 1 });
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
            let is_current = i == app.theme_selected;
            let style = if is_current {
                Style::default()
                    .bg(app.theme.primary())
                    .fg(app.theme.panel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground())
            };
            let marker = if *name == app.theme.name() { "● " } else { "  " };
            ListItem::new(format!("{}{}", marker, name)).style(style)
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, content_area);
    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Enter", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Select  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to close", Style::default().fg(app.theme.system_color())),
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

fn draw_agents_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(50, 50, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Agent Configuration ")
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let inner = popup_area.inner(Margin { horizontal: 2, vertical: 1 });
    let content_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: inner.height.saturating_sub(1),
    };

    let options = vec![
        ("Status", format!("{}", match app.agents.status {
            crate::app::AgentStatus::Disabled => "Disabled",
            crate::app::AgentStatus::Idle => "Enabled",
            crate::app::AgentStatus::Active => "Active",
        })),
        ("Persona", app.agents.persona.clone()),
        ("Max Iterations", format!("{}", app.agents.max_iterations)),
    ];

    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let is_selected = i == app.agent_selected;
            let style = if is_selected {
                Style::default()
                    .bg(app.theme.primary())
                    .fg(app.theme.panel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground())
            };
            let marker = if is_selected { "▸ " } else { "  " };
            ListItem::new(format!("{}{}: {}", marker, label, value)).style(style)
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, content_area);
    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Enter", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Change  ", Style::default().fg(app.theme.system_color())),
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" Close", Style::default().fg(app.theme.system_color())),
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

fn draw_help_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(85, 85, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keyboard Shortcuts ")
        .title_style(Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(app.theme.border_color()));

    let sections = vec![
        ("Navigation", vec![
            ("Esc", "Close panels / Go to model selection"),
            ("↑/↓", "Navigate history, lists, or themes"),
            ("PgUp/PgDn", "Scroll chat output"),
            ("Mouse scroll", "Scroll chat output"),
        ]),
        ("Chat", vec![
            ("Enter", "Send message or select command"),
            ("Tab (on /cmd)", "Cycle slash command autocomplete"),
            ("Tab (empty)", "Enable agents or cycle persona"),
            ("/", "Start slash command"),
            ("Ctrl+W", "Delete word backward"),
            ("Ctrl+A/E", "Move cursor to start/end"),
        ]),
        ("Info", vec![
            ("/help", "Show this help panel"),
            ("/version", "Show version info"),
        ]),
        ("Config", vec![
            ("/model [name]", "Switch model or open picker"),
            ("/theme [name]", "Change color theme"),
        ]),
        ("Session", vec![
            ("/new", "Start new session"),
            ("/clear", "Clear conversation"),
            ("/save [name]", "Save session to disk"),
            ("/load [name]", "Load session from disk"),
            ("/sessions", "List saved sessions"),
            ("/delete [name]", "Delete a saved session"),
            ("/export [file]", "Export to markdown/json/txt"),
            ("/undo", "Undo last turn"),
            ("/title <name>", "Rename current session"),
        ]),
        ("Chat Commands", vec![
            ("/search [query]", "Search in conversation"),
            ("/edit <n>", "Edit a message by index"),
            ("/remove <n>", "Remove a message by index"),
        ]),
        ("Agent", vec![
            ("/agents", "Open agent configuration"),
            ("/yolo", "Toggle auto-approval for tools"),
            ("Tab (empty input)", "Enable agents or cycle persona"),
            ("", "When enabled, LLM uses tools repeatedly"),
            ("", "to complete multi-step tasks automatically."),
        ]),
        ("Tools", vec![
            ("/mcp", "Show MCP server status"),
            ("/tools", "Toggle tools panel"),
        ]),
        ("System", vec![
            ("/quit", "Quit application"),
        ]),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (section_name, items) in sections {
        lines.push(Line::from(vec![
            Span::styled(section_name, Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
        ]));
        lines.push(Line::from(""));
        for (key, desc) in items {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<18}", key), Style::default().fg(app.theme.secondary()).add_modifier(Modifier::BOLD)),
                Span::styled(desc, Style::default().fg(app.theme.foreground())),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("Press ", Style::default().fg(app.theme.system_color())),
        Span::styled("Esc", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" or ", Style::default().fg(app.theme.system_color())),
        Span::styled("F1/?", Style::default().fg(app.theme.primary()).add_modifier(Modifier::BOLD)),
        Span::styled(" to close this panel", Style::default().fg(app.theme.system_color())),
    ]));

    let content = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: true });
    frame.render_widget(content, popup_area);
}

fn provider_color(provider: &str, theme: Theme) -> ratatui::style::Color {
    match provider.to_lowercase().as_str() {
        "kimi" => theme.primary(),
        "local" | "ollama" | "lm-studio" | "llamacpp" => theme.tool_color(),
        _ => theme.secondary(),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
