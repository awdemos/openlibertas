//! Ratatui rendering: all drawing logic for the terminal UI.
//!
//! Key functions:
//! - `draw` — routes to chat screen or model selection screen
//! - `draw_chat` — main chat layout: header, messages, input, status bar
//! - Popups: tools, MCP, sessions, themes, help, agents, command palette
//! - `centered_rect` — helper for popup positioning
//! - Audio level meter during voice recording

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, ConnectionStatus, Overlay, Screen};
use crate::theme::Theme;
use openlibertas_core::domain::Role;

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
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(area);

    let title = Paragraph::new("OpenLibertas")
        .style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
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
        let empty = Paragraph::new("No models found.\nMake sure your provider is running.")
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
                items.push(
                    ListItem::new(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            format!("[{}]", provider.to_uppercase()),
                            Style::default()
                                .fg(provider_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]))
                    .style(Style::default().add_modifier(Modifier::ITALIC)),
                );
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
            let voice_indicator = if m.supports_voice { " 🎤" } else { "" };
            let local_indicator = if m.local { " [local]" } else { "" };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(app.theme.primary())),
                Span::styled(m.id.clone(), style),
                Span::styled(
                    local_indicator,
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    tool_indicator,
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    voice_indicator,
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::DIM),
                ),
            ])));
        }

        let total_models = app.models.models.len();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Models ({}) ", total_models))
                    .title_style(
                        Style::default()
                            .fg(app.theme.primary())
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(list, chunks[1]);
    }

    let footer_spans = vec![
        Span::styled(
            "↑/↓",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Enter",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Select  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "q",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " Quit (press twice)",
            Style::default().fg(app.theme.system_color()),
        ),
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
            Constraint::Length(3),
        ])
        .margin(1)
        .split(frame.area());

    draw_header(frame, app, main_chunks[0]);
    draw_messages(frame, app, main_chunks[1]);
    draw_status_bar(frame, app, main_chunks[2]);
    draw_input(frame, app, main_chunks[3]);

    match app.overlay {
        Overlay::Tools => draw_tools_panel(frame, app),
        Overlay::Mcp => draw_mcp_panel(frame, app),
        Overlay::Sessions => draw_sessions_panel(frame, app),
        Overlay::Palette => {
            if app.screen == Screen::Chat {
                draw_command_palette(frame, app, main_chunks[2]);
            }
        }
        Overlay::Themes => draw_themes_panel(frame, app),
        Overlay::Agents => draw_agents_panel(frame, app),
        Overlay::Help => draw_help_panel(frame, app),
        Overlay::None => {}
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (conn_symbol, conn_color) = match app.connection_status {
        ConnectionStatus::Connected => ("●", app.theme.user_color()),
        ConnectionStatus::Disconnected => ("●", app.theme.error_color()),
        ConnectionStatus::Checking => ("◐", app.theme.secondary()),
    };

    let tool_indicator = if app.engine.tools.available_tools.is_empty() {
        ""
    } else {
        " [Tools: ✓]"
    };

    let mcp_indicator = if app.engine.tools.client.is_some() {
        " [MCP: ✓]"
    } else {
        " [MCP: ✗]"
    };

    let agent_indicator = match app.engine.agents.status {
        openlibertas_core::engine::AgentStatus::Disabled => String::new(),
        openlibertas_core::engine::AgentStatus::Idle => {
            format!(" [Agents: ○ {} Ready]", app.engine.agents.persona)
        }
        openlibertas_core::engine::AgentStatus::Active => {
            format!(
                " [Agents: ● {}/{} {}]",
                app.engine.agents.current_iteration,
                app.engine.agents.max_iterations,
                app.engine.agents.persona
            )
        }
    };

    let voice_indicator = if app.voice.is_enabled() {
        let state_label = app.voice.state().label();
        let icon = match app.voice.state() {
            openlibertas_core::voice::VoiceState::Recording => "🔴",
            openlibertas_core::voice::VoiceState::ProcessingStt => "⏳",
            openlibertas_core::voice::VoiceState::ProcessingTts => "🔊",
            openlibertas_core::voice::VoiceState::Playing => "🔊",
            openlibertas_core::voice::VoiceState::Error => "⚠",
            _ => "🎙",
        };
        format!(" [{} {}]", icon, state_label)
    } else {
        String::new()
    };

    let (search_indicator, search_snippet) = if app.search.active && !app.search.matches.is_empty()
    {
        let m = &app.search.matches[app.search.index];
        let snippet = app
            .engine
            .chat
            .messages
            .get(m.message_index)
            .map(|msg| m.snippet(&msg.content))
            .unwrap_or("");
        let indicator = format!(
            "Search: {}/{}",
            app.search.index + 1,
            app.search.matches.len()
        );
        let preview = if snippet.len() > 20 {
            format!(" ...'{}'...", &snippet[..20])
        } else {
            format!(" ...'{}'...", snippet)
        };
        (indicator, preview)
    } else {
        (String::new(), String::new())
    };

    let left_spans = vec![
        Span::styled(
            "OpenLibertas",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", conn_symbol),
            Style::default().fg(conn_color),
        ),
    ];
    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();

    let mut right_parts = Vec::new();
    if !tool_indicator.is_empty() {
        right_parts.push(tool_indicator.trim_start().to_string());
    }
    if !mcp_indicator.is_empty() {
        right_parts.push(mcp_indicator.trim_start().to_string());
    }
    if !agent_indicator.is_empty() {
        right_parts.push(agent_indicator.trim_start().to_string());
    }
    if !voice_indicator.is_empty() {
        right_parts.push(voice_indicator);
    }
    if !search_indicator.is_empty() {
        right_parts.push(search_indicator);
    }
    let right_text = right_parts.join(" | ");

    let max_right_width = (area.width as usize).saturating_sub(left_width + 1);
    let right_text_display = if right_text.width() > max_right_width && max_right_width > 3 {
        let truncate_to = max_right_width.saturating_sub(3);
        let mut byte_idx = 0;
        let mut visual_width = 0;
        for (i, c) in right_text.char_indices() {
            let w = c.width().unwrap_or(0);
            if visual_width + w > truncate_to {
                break;
            }
            visual_width += w;
            byte_idx = i + c.len_utf8();
        }
        format!("{}...", &right_text[..byte_idx])
    } else {
        right_text
    };

    let right_pad = max_right_width.saturating_sub(right_text_display.width());
    let right_span = Span::styled(
        format!("{}{}", " ".repeat(right_pad), right_text_display),
        Style::default().fg(app.theme.system_color()),
    );

    let header_lines = if search_snippet.is_empty() {
        vec![Line::from(
            left_spans
                .into_iter()
                .chain(std::iter::once(right_span))
                .collect::<Vec<_>>(),
        )]
    } else {
        vec![
            Line::from(
                left_spans
                    .into_iter()
                    .chain(std::iter::once(right_span))
                    .collect::<Vec<_>>(),
            ),
            Line::from(Span::styled(
                search_snippet,
                Style::default().fg(app.theme.secondary()),
            )),
        ]
    };

    let header = Paragraph::new(Text::from(header_lines));
    frame.render_widget(header, area);
}

fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let current_match_msg = app
        .search
        .matches
        .get(app.search.index)
        .map(|m| m.message_index);

    let is_last_msg_streaming = app.engine.chat.streaming
        && app
            .engine
            .chat
            .messages
            .last()
            .map(|m| m.role == Role::Assistant)
            .unwrap_or(false);
    let spinner_frame =
        app.engine.chat.spinner_frame % openlibertas_core::engine::SPINNER_FRAMES.len();

    let messages_text: Vec<Line> = app
        .engine
        .chat
        .messages
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
            let has_match = app
                .search
                .matches
                .iter()
                .any(|m| m.message_index == msg_idx);
            let is_last = msg_idx == app.engine.chat.messages.len().saturating_sub(1);

            let bg_style = if is_current_match {
                Style::default()
                    .bg(app.theme.secondary())
                    .fg(app.theme.panel_bg())
            } else if has_match {
                Style::default().bg(app.theme.border_color())
            } else {
                Style::default()
            };

            if let Some(ref tool_calls) = msg.tool_calls {
                let mut lines = vec![];
                for tc in tool_calls {
                    let key_arg = openlibertas_core::tool_registry::extract_key_argument(
                        &tc.function.name,
                        &tc.function.arguments,
                    );
                    let display = if key_arg.is_empty() {
                        format!("Using tool: {}", tc.function.name)
                    } else {
                        format!("{}: {}", tc.function.name, key_arg)
                    };
                    lines.push(Line::from(vec![
                        Span::styled("🔧 ", Style::default().fg(app.theme.tool_color())),
                        Span::styled(
                            display,
                            Style::default()
                                .fg(app.theme.tool_color())
                                .add_modifier(Modifier::BOLD),
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
                let viewport_width = area.width.saturating_sub(2) as usize;

                if msg.role == Role::Assistant {
                    if let Some(ref reasoning) = msg.reasoning_content {
                        if !reasoning.is_empty() {
                            lines.push(Line::from(vec![
                                Span::styled("┌─ ", Style::default().fg(app.theme.system_color())),
                                Span::styled(
                                    "Thinking",
                                    Style::default()
                                        .fg(app.theme.system_color())
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    " ─",
                                    Style::default().fg(app.theme.system_color()),
                                ),
                            ]));
                            let wrapped_reasoning = if viewport_width > 10 {
                                crate::markdown::wrap_markdown(reasoning, viewport_width.saturating_sub(2))
                            } else {
                                reasoning.clone()
                            };
                            let reasoning_rendered = app.markdown_renderer.render(&wrapped_reasoning, app.theme);
                            for line in reasoning_rendered.lines {
                                let dimmed_line = Line::from(
                                    line.spans
                                        .into_iter()
                                        .map(|span| {
                                            Span::styled(
                                                span.content.to_string(),
                                                span.style
                                                    .fg(app.theme.system_color())
                                                    .add_modifier(Modifier::ITALIC),
                                            )
                                        })
                                        .collect::<Vec<_>>(),
                                );
                                lines.push(dimmed_line);
                            }
                            lines.push(Line::from(vec![Span::styled(
                                "└",
                                Style::default().fg(app.theme.system_color()),
                            )]));
                            lines.push(Line::from(""));
                        }
                    }
                }

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
                        Span::styled(
                            openlibertas_core::engine::SPINNER_FRAMES[spinner_frame],
                            Style::default()
                                .fg(app.theme.primary())
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            " generating...",
                            Style::default()
                                .fg(app.theme.secondary())
                                .add_modifier(Modifier::ITALIC),
                        ),
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
    let viewport_height = area.height.saturating_sub(2) as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let scroll = if app.engine.chat.auto_scroll {
        max_scroll
    } else {
        app.engine.chat.scroll.min(max_scroll)
    };

    let messages_widget = if app.engine.chat.messages.is_empty() && !app.engine.chat.streaming {
        let welcome_lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("Welcome to ", Style::default().fg(app.theme.foreground())),
                Span::styled(
                    "OpenLibertas",
                    Style::default()
                        .fg(app.theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "• Type a message and press ",
                    Style::default().fg(app.theme.system_color()),
                ),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(app.theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to chat", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Use ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "@path/to/file",
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " to attach files",
                    Style::default().fg(app.theme.system_color()),
                ),
            ]),
            Line::from(vec![
                Span::styled("• Press ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "/",
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " for slash commands",
                    Style::default().fg(app.theme.system_color()),
                ),
            ]),
            Line::from(vec![
                Span::styled("• Press ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "F1",
                    Style::default()
                        .fg(app.theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" or ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "?",
                    Style::default()
                        .fg(app.theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" for help", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Use ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "/voice",
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" then hold ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "Ctrl+Space",
                    Style::default()
                        .fg(app.theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to talk", Style::default().fg(app.theme.system_color())),
            ]),
            Line::from(vec![
                Span::styled("• Press ", Style::default().fg(app.theme.system_color())),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(app.theme.primary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " to switch models",
                    Style::default().fg(app.theme.system_color()),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Start typing below...",
                Style::default()
                    .fg(app.theme.secondary())
                    .add_modifier(Modifier::ITALIC),
            )]),
        ];
        Paragraph::new(Text::from(welcome_lines))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Chat ")
                    .title_style(
                        Style::default()
                            .fg(app.theme.primary())
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
    } else {
        Paragraph::new(Text::from(messages_text))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Chat ")
                    .title_style(
                        Style::default()
                            .fg(app.theme.primary())
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .scroll((scroll as u16, 0))
            .wrap(Wrap { trim: true })
    };
    frame.render_widget(messages_widget, area);

    let scrollbar = Scrollbar::default()
        .orientation(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("↑"))
        .end_symbol(Some("↓"));
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    frame.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        }),
        &mut scrollbar_state,
    );
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let model_name = app.models.current.as_deref().unwrap_or("Unknown");

    let (conn_symbol, conn_color) = match app.connection_status {
        ConnectionStatus::Connected => ("●", app.theme.user_color()),
        ConnectionStatus::Disconnected => ("●", app.theme.error_color()),
        ConnectionStatus::Checking => ("◐", app.theme.secondary()),
    };

    let display_model =
        if app.engine.agents.status == openlibertas_core::engine::AgentStatus::Disabled {
            model_name.to_string()
        } else {
            format!("{}@{}", app.engine.agents.persona, model_name)
        };

    let base_url = app
        .config
        .providers
        .first()
        .map(|p| {
            let url = p.base_url.as_str();
            if url.len() > 30 {
                format!("{}...", &url[..27])
            } else {
                url.to_string()
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let status_spans = vec![
        Span::styled(format!("{} ", conn_symbol), Style::default().fg(conn_color)),
        Span::styled(
            display_model.clone(),
            Style::default()
                .fg(app.theme.foreground())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" | ", Style::default().fg(app.theme.border_color())),
        Span::styled(
            base_url.clone(),
            Style::default().fg(app.theme.system_color()),
        ),
    ];
    let status_bar = Paragraph::new(Line::from(status_spans))
        .style(Style::default().fg(app.theme.system_color()));
    frame.render_widget(status_bar, area);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let prompt_symbol = if app.voice.is_enabled() {
        "🎙 "
    } else if app.engine.agents.status == openlibertas_core::engine::AgentStatus::Disabled {
        "> "
    } else {
        "✨ "
    };
    let prompt_width = prompt_symbol.width();
    let viewport_width = area.width as usize;

    let safe_cursor_pos = {
        let pos = app
            .engine
            .input
            .cursor_pos
            .min(app.engine.input.buffer.len());
        if app.engine.input.buffer.is_char_boundary(pos) {
            pos
        } else {
            app.engine
                .input
                .buffer
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < pos)
                .last()
                .unwrap_or(0)
        }
    };

    let text_before_cursor = &app.engine.input.buffer[..safe_cursor_pos];
    let cursor_display_pos = prompt_width + text_before_cursor.width();

    let mut scroll_offset = app.engine.input.scroll_offset;
    if cursor_display_pos < scroll_offset {
        scroll_offset = cursor_display_pos.saturating_sub(1);
    } else if cursor_display_pos >= scroll_offset + viewport_width.saturating_sub(1) {
        scroll_offset = cursor_display_pos
            .saturating_sub(viewport_width)
            .saturating_add(2);
    }

    let selection = app.engine.selection();
    let mut spans: Vec<Span> = vec![Span::styled(
        prompt_symbol,
        Style::default().fg(app.theme.foreground()),
    )];

    let mut current_width = 0;
    let mut in_visible_region = false;

    for (byte_idx, ch) in app.engine.input.buffer.char_indices() {
        let ch_width = ch.width().unwrap_or(1);

        if current_width >= scroll_offset && current_width < scroll_offset + viewport_width {
            in_visible_region = true;
        } else if current_width >= scroll_offset + viewport_width {
            break;
        }

        if in_visible_region {
            let is_selected =
                selection.is_some_and(|(start, end)| byte_idx >= start && byte_idx < end);
            let style = if is_selected {
                Style::default()
                    .fg(app.theme.panel_bg())
                    .bg(app.theme.primary())
            } else {
                Style::default().fg(app.theme.foreground())
            };
            spans.push(Span::styled(ch.to_string(), style));
        }

        current_width += ch_width;
    }

    let line = Line::from(spans);
    let input = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border_color()))
            .title({
                let base_status = if let Some(ref status) = app.voice_status {
                    status.clone()
                } else if app.engine.chat.streaming {
                    "Streaming...".to_string()
                } else {
                    String::new()
                };
                if app.voice.is_enabled()
                    && matches!(
                        app.voice.state(),
                        openlibertas_core::voice::VoiceState::Recording
                    )
                {
                    let level_bar = app
                        .voice
                        .recording_stats()
                        .map(|stats| {
                            let level = (stats.peak_amplitude * 2.0).min(1.0);
                            let filled = (level * 10.0) as usize;
                            let empty = 10_usize.saturating_sub(filled);
                            format!(
                                "{} {}{}{}",
                                base_status,
                                "█".repeat(filled),
                                "░".repeat(empty),
                                if stats.is_silence { " [SILENCE]" } else { "" }
                            )
                        })
                        .unwrap_or_else(|| format!("{} ░░░░░░░░░░", base_status));
                    format!(" {} ", level_bar)
                } else if !base_status.is_empty() {
                    format!(" {} ", base_status)
                } else {
                    base_status
                }
            })
            .title_style(
                Style::default()
                    .fg(app.theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(input, area);

    let cursor_x = area.x
        + 1
        + (cursor_display_pos.saturating_sub(scroll_offset)).min(viewport_width.saturating_sub(3))
            as u16;
    frame.set_cursor_position((cursor_x, area.y + 1));
}

fn draw_tools_panel(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let popup_area = centered_rect(80, 80, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " MCP Tools ({}) ",
            app.engine.tools.available_tools.len()
        ))
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

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

    if app.engine.tools.available_tools.is_empty() {
        let content = Paragraph::new("No tools available.\nCheck MCP server configuration in ~/.config/opencode/opencode.json")
            .alignment(Alignment::Center);
        frame.render_widget(content, content_area);
    } else {
        let tool_lines: Vec<Line> = app
            .engine
            .tools
            .available_tools
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

        let content = Paragraph::new(Text::from(tool_lines)).wrap(Wrap { trim: true });
        frame.render_widget(content, content_area);
    }

    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to close  |  ",
            Style::default().fg(app.theme.system_color()),
        ),
        Span::styled(
            "/tools",
            Style::default()
                .fg(app.theme.secondary())
                .add_modifier(Modifier::BOLD),
        ),
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

    let servers: Vec<String> = app
        .engine
        .tools
        .client
        .as_ref()
        .map_or(Vec::new(), |c| c.server_names());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" MCP Servers ({}) ", servers.len()))
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

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

    if servers.is_empty() {
        let content = Paragraph::new(
            "No MCP servers configured.\nAdd servers to ~/.config/opencode/opencode.json",
        )
        .alignment(Alignment::Center);
        frame.render_widget(content, content_area);
    } else {
        let lines: Vec<Line> = servers
            .into_iter()
            .map(|name| {
                let status = app.engine.tools.server_statuses.get(&name);
                let (indicator, color) = match status {
                    Some(openlibertas_core::domain::McpServerStatus::Connected) => {
                        ("●", app.theme.user_color())
                    }
                    Some(openlibertas_core::domain::McpServerStatus::Connecting) => {
                        ("◐", app.theme.secondary())
                    }
                    Some(openlibertas_core::domain::McpServerStatus::Failed) => {
                        ("✗", app.theme.error_color())
                    }
                    Some(openlibertas_core::domain::McpServerStatus::Disabled) => {
                        ("○", app.theme.system_color())
                    }
                    Some(openlibertas_core::domain::McpServerStatus::Pending) => {
                        ("○", app.theme.system_color())
                    }
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
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to close  |  ",
            Style::default().fg(app.theme.system_color()),
        ),
        Span::styled(
            "/mcp",
            Style::default()
                .fg(app.theme.secondary())
                .add_modifier(Modifier::BOLD),
        ),
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
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

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

    if sessions.is_empty() {
        let content =
            Paragraph::new("No saved sessions.\nUse /save to save the current conversation.")
                .alignment(Alignment::Center);
        frame.render_widget(content, content_area);
    } else {
        let lines: Vec<Line> = sessions
            .into_iter()
            .map(|(id, model, created)| {
                let model_str = model.as_deref().unwrap_or("unknown");
                let date = &created[..created.find('T').unwrap_or(created.len())];
                let time = &created[created.find('T').map(|i| i + 1).unwrap_or(0)
                    ..created.find('.').unwrap_or(created.len())];
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
        let content = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true });
        frame.render_widget(content, content_area);
    }

    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to close  |  ",
            Style::default().fg(app.theme.system_color()),
        ),
        Span::styled(
            "/load <name>",
            Style::default()
                .fg(app.theme.secondary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to load  |  ",
            Style::default().fg(app.theme.system_color()),
        ),
        Span::styled(
            "/sessions",
            Style::default()
                .fg(app.theme.secondary())
                .add_modifier(Modifier::BOLD),
        ),
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
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

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

    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, content_area);
    frame.render_widget(block, area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            "↑/↓",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" or ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Tab",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Enter",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Select  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
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
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

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
            let is_current = i == app.theme_selected;
            let style = if is_current {
                Style::default()
                    .bg(app.theme.primary())
                    .fg(app.theme.panel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground())
            };
            let marker = if *name == app.theme.name() {
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
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Enter",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Select  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
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
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

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

    let options = [(
            "Status",
            (match app.engine.agents.status {
                    openlibertas_core::engine::AgentStatus::Disabled => "Disabled",
                    openlibertas_core::engine::AgentStatus::Idle => "Enabled",
                    openlibertas_core::engine::AgentStatus::Active => "Active",
                }).to_string(),
        ),
        ("Persona", app.engine.agents.persona.clone()),
        (
            "Max Iterations",
            format!("{}", app.engine.agents.max_iterations),
        )];

    let mut items: Vec<ListItem> = options
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

    items.push(ListItem::new(""));
    items.push(ListItem::new("Available Specialists:").style(
        Style::default()
            .fg(app.theme.primary())
            .add_modifier(Modifier::BOLD),
    ));

    let personas = app.agent_personas();
    for (name, _) in personas.iter().filter(|(n, _)| n != &app.engine.agents.persona) {
        items.push(ListItem::new(format!("  • {}", name)).style(
            Style::default().fg(app.theme.secondary()),
        ));
    }

    items.push(ListItem::new(""));
    items.push(ListItem::new("Agents can delegate to specialists using the switch_persona tool.").style(
        Style::default().fg(app.theme.system_color()),
    ));

    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(list, content_area);
    frame.render_widget(block, popup_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            "↑/↓",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Navigate  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Enter",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Change  ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
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
        .title_style(
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(app.theme.border_color()));

    let sections = vec![
        (
            "Navigation",
            vec![
                ("Esc", "Close panels / Go to model selection"),
                ("↑/↓", "Navigate history, lists, or themes"),
                ("PgUp/PgDn", "Scroll chat output"),
                ("Mouse scroll", "Scroll chat output"),
            ],
        ),
        (
            "Chat",
            vec![
                ("Enter", "Send message or select command"),
                ("Tab (on /cmd)", "Cycle slash command autocomplete"),
                ("Tab (empty)", "Enable agents or cycle persona"),
                ("/", "Start slash command"),
                ("Ctrl+W", "Delete word backward"),
                ("Ctrl+A/E", "Move cursor to start/end"),
            ],
        ),
        (
            "Info",
            vec![
                ("/help", "Show this help panel"),
                ("/version", "Show version info"),
            ],
        ),
        (
            "Config",
            vec![
                ("/model [name]", "Switch model or open picker"),
                ("/theme [name]", "Change color theme"),
            ],
        ),
        (
            "Session",
            vec![
                ("/new", "Start new session"),
                ("/clear", "Clear conversation"),
                ("/save [name]", "Save session to disk"),
                ("/load [name]", "Load session from disk"),
                ("/sessions", "List saved sessions"),
                ("/delete [name]", "Delete a saved session"),
                ("/export [file]", "Export to markdown/json/txt"),
                ("/undo", "Undo last turn"),
                ("/title <name>", "Rename current session"),
            ],
        ),
        (
            "Chat Commands",
            vec![
                ("/search [query]", "Search in conversation"),
                ("/edit <n>", "Edit a message by index"),
                ("/remove <n>", "Remove a message by index"),
            ],
        ),
        (
            "Agent",
            vec![
                ("/agents", "Open agent configuration"),
                ("/yolo", "Toggle auto-approval for tools"),
                ("Tab (empty input)", "Enable agents or cycle persona"),
                ("", "When enabled, LLM uses tools repeatedly"),
                ("", "to complete multi-step tasks automatically."),
            ],
        ),
        (
            "Tools",
            vec![
                ("/mcp", "Show MCP server status"),
                ("/tools", "Toggle tools panel"),
            ],
        ),
        (
            "Voice",
            vec![
                ("/voice", "Toggle voice chat mode"),
                ("Ctrl+Space", "Push-to-talk (when voice enabled)"),
            ],
        ),
        ("System", vec![("/quit", "Quit application")]),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (section_name, items) in sections {
        lines.push(Line::from(vec![Span::styled(
            section_name,
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )]));
        lines.push(Line::from(""));
        for (key, desc) in items {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<18}", key),
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(desc, Style::default().fg(app.theme.foreground())),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::styled("Press ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "Esc",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" or ", Style::default().fg(app.theme.system_color())),
        Span::styled(
            "F1/?",
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to close this panel",
            Style::default().fg(app.theme.system_color()),
        ),
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
