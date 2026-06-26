use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

use crate::app::App;
use openlibertas_core::domain::Role;

pub fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let current_match_msg = app
        .search
        .matches
        .get(app.search.index)
        .map(|m| m.message_index);

    let is_last_msg_streaming = app.engine.chat().streaming
        && app
            .engine
            .chat()
            .messages
            .last()
            .map(|m| m.role == Role::Assistant)
            .unwrap_or(false);
    let spinner_frame =
        app.engine.chat().spinner_frame % openlibertas_core::engine::SPINNER_FRAMES.len();

    let last_visible_idx = app
        .engine
        .chat()
        .messages
        .iter()
        .enumerate()
        .filter(|(_, msg)| !(msg.role == Role::System && msg.is_prompt))
        .map(|(idx, _)| idx)
        .next_back();

    let messages_text: Vec<Line> = app
        .engine
        .chat()
        .messages
        .iter()
        .enumerate()
        .flat_map(|(msg_idx, msg)| {
            if msg.role == Role::System && msg.is_prompt {
                return vec![];
            }

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
            let is_last = Some(msg_idx) == last_visible_idx;

            let bg_style = if is_current_match {
                Style::default()
                    .bg(app.theme.secondary())
                    .fg(app.theme.panel_bg())
            } else if has_match {
                Style::default().bg(app.theme.border_color())
            } else {
                Style::default()
            };

            let mut lines = vec![Line::from(vec![Span::styled(
                format!("{}: ", label),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )])];

            if msg.tool_calls.is_some() && msg.content.is_empty() {
                vec![]
            } else {
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
                                Span::styled(" ─", Style::default().fg(app.theme.system_color())),
                            ]));
                            let wrapped_reasoning = if viewport_width > 10 {
                                crate::markdown::wrap_markdown(
                                    reasoning,
                                    viewport_width.saturating_sub(2),
                                )
                            } else {
                                reasoning.clone()
                            };
                            let reasoning_rendered =
                                app.markdown_renderer.render(&wrapped_reasoning, app.theme);
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

                {
                    if msg.role == Role::Tool {
                        for text_line in msg.content.lines() {
                            lines.push(Line::from(Span::raw(text_line)));
                        }
                    } else {
                        let wrapped_content = if viewport_width > 10 {
                            crate::markdown::wrap_markdown(&msg.content, viewport_width)
                        } else {
                            msg.content.clone()
                        };
                        let rendered = app.markdown_renderer.render(&wrapped_content, app.theme);
                        for line in rendered.lines {
                            lines.push(line);
                        }
                    }
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
                let branch_point = app
                    .session_manager
                    .as_ref()
                    .and_then(|sm| sm.context().branch_point);
                if branch_point == Some(msg_idx) {
                    let sep = "─".repeat(viewport_width.min(80));
                    lines.push(Line::from(vec![Span::styled(
                        sep.clone(),
                        Style::default().fg(app.theme.border_color()),
                    )]));
                    lines.push(Line::from(vec![Span::styled(
                        "  Branch point — messages below diverge from parent",
                        Style::default()
                            .fg(app.theme.secondary())
                            .add_modifier(Modifier::ITALIC),
                    )]));
                    lines.push(Line::from(vec![Span::styled(
                        sep,
                        Style::default().fg(app.theme.border_color()),
                    )]));
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
    let scroll = if app.engine.chat().auto_scroll {
        max_scroll
    } else {
        app.engine.chat().scroll.min(max_scroll)
    };

    let messages_widget = if app.engine.chat().messages.is_empty() && !app.engine.chat().streaming {
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
