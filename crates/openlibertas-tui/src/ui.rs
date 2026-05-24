//! Ratatui rendering: all drawing logic for the terminal UI.
//!
//! Key functions:
//! - `draw` — routes to chat screen or model selection screen
//! - `draw_chat` — main chat layout: header, messages, input, status bar
//! - Popups are handled by the `panels/` module system
//! - `centered_rect` — helper for popup positioning
//! - Audio level meter during voice recording

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, ConnectionStatus, Overlay, Screen};
use crate::panels::Panel;
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
        let search_lower = app.models.search.to_lowercase();
        let filtered: Vec<(usize, &openlibertas_core::domain::Model)> = app
            .models
            .models
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                search_lower.is_empty()
                    || m.id.to_lowercase().contains(&search_lower)
                    || m.provider.as_str().to_lowercase().contains(&search_lower)
            })
            .collect();

        let mut items: Vec<ListItem> = Vec::new();
        let mut current_provider = "";

        for (i, m) in filtered.iter() {
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

            let is_selected = *i == app.models.selected;
            let is_current = app.models.current.as_deref() == Some(&m.id);
            let style = if is_selected {
                Style::default()
                    .bg(app.theme.primary())
                    .fg(app.theme.panel_bg())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.foreground())
            };

            let marker = if is_selected {
                "▸ "
            } else if is_current {
                "● "
            } else {
                "  "
            };
            let tool_indicator = if m.supports_tools { " ⚡" } else { "" };
            let voice_indicator = if m.supports_voice { " 🎤" } else { "" };
            let local_indicator = if m.local { " [local]" } else { "" };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(app.theme.primary())),
                Span::styled(m.id.clone(), style),
                Span::styled(
                    format!(" @ {}", provider),
                    Style::default()
                        .fg(app.theme.secondary())
                        .add_modifier(Modifier::DIM),
                ),
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

        let title = if app.models.search.is_empty() {
            format!(" Models ({}) ", app.models.models.len())
        } else {
            format!(" Models ({} / {}) ", filtered.len(), app.models.search)
        };
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .title_style(
                        Style::default()
                            .fg(app.theme.primary())
                            .add_modifier(Modifier::BOLD),
                    ),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(list, chunks[1]);
    }

    let footer_spans = if app.models.search.is_empty() {
        vec![
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
            Span::styled(" Back  ", Style::default().fg(app.theme.system_color())),
            Span::styled(
                "Type",
                Style::default()
                    .fg(app.theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to filter", Style::default().fg(app.theme.system_color())),
        ]
    } else {
        vec![
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
            Span::styled(
                " Clear filter  ",
                Style::default().fg(app.theme.system_color()),
            ),
            Span::styled(
                "Backspace",
                Style::default()
                    .fg(app.theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Delete char",
                Style::default().fg(app.theme.system_color()),
            ),
        ]
    };
    let help = Paragraph::new(Line::from(footer_spans))
        .style(Style::default().fg(app.theme.system_color()))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[2]);
}

fn draw_chat(frame: &mut Frame, app: &App) {
    let has_error = app.error_banner.is_some();
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            if has_error {
                Constraint::Length(1)
            } else {
                Constraint::Length(0)
            },
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .margin(1)
        .split(frame.area());

    draw_header(frame, app, main_chunks[0]);
    if has_error {
        draw_error_banner(frame, app, main_chunks[1]);
    }
    draw_messages(frame, app, main_chunks[if has_error { 2 } else { 1 }]);
    draw_status_bar(frame, app, main_chunks[if has_error { 3 } else { 2 }]);
    draw_input(frame, app, main_chunks[if has_error { 4 } else { 3 }]);

    let status_idx = if has_error { 3 } else { 2 };
    let input_idx = if has_error { 4 } else { 3 };

    match app.overlay {
        Overlay::Tools => {
            crate::panels::tools::ToolsPanelData::from_app(app).draw(
                frame,
                frame.area(),
                &app.theme,
            );
        }
        Overlay::Mcp => {
            crate::panels::mcp::McpPanelData::from_app(app).draw(frame, frame.area(), &app.theme);
        }
        Overlay::Sessions => {
            crate::panels::sessions::SessionsPanelData::from_app(app).draw(
                frame,
                frame.area(),
                &app.theme,
            );
        }
        Overlay::Palette => {
            if app.screen == Screen::Chat {
                crate::panels::palette::PalettePanelData::from_app(app, main_chunks[status_idx])
                    .draw(frame, main_chunks[status_idx], &app.theme);
            }
        }
        Overlay::Themes => {
            crate::panels::themes::ThemesPanelData::from_app(app).draw(
                frame,
                frame.area(),
                &app.theme,
            );
        }
        Overlay::Agents => {
            crate::panels::agents::AgentsPanelData::from_app(app).draw(
                frame,
                frame.area(),
                &app.theme,
            );
        }
        Overlay::Help => {
            crate::panels::help::HelpPanelData::from_app(app).draw(frame, frame.area(), &app.theme);
        }
        Overlay::AvatarMenu => {
            crate::panels::avatar::AvatarMenuPanelData::from_app(app).draw(
                frame,
                frame.area(),
                &app.theme,
            );
        }
        Overlay::None => {}
    }

    if app.completion_active() && app.screen == Screen::Chat {
        crate::panels::completions::CompletionsPopupData::from_app(app, main_chunks[input_idx])
            .draw(frame, main_chunks[input_idx], &app.theme);
    }

    if app.avatar_enabled {
        for avatar in &app.avatars {
            frame.render_widget(avatar, frame.area());
        }
    }
}

fn draw_error_banner(frame: &mut Frame, app: &App, area: Rect) {
    if let Some((ref msg, _)) = app.error_banner {
        let banner = Paragraph::new(Line::from(vec![
            Span::styled(
                " ✗ ",
                Style::default()
                    .fg(app.theme.error_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(msg.clone(), Style::default().fg(app.theme.error_color())),
        ]))
        .style(Style::default().bg(app.theme.panel_bg()))
        .alignment(Alignment::Center);
        frame.render_widget(banner, area);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let (conn_symbol, conn_color) = match app.connection_status {
        ConnectionStatus::Connected => ("●", app.theme.user_color()),
        ConnectionStatus::Disconnected => ("●", app.theme.error_color()),
        ConnectionStatus::Checking => ("◐", app.theme.secondary()),
    };

    let tool_indicator = if app.engine.tools().available_tools().is_empty() {
        ""
    } else {
        " [Tools: ✓]"
    };

    let mcp_indicator = {
        let client = app.engine.tools().client();
        if client.is_some() {
            let diagnostics = app.engine.tools().diagnostics();
            let total = diagnostics.len();
            let connected = diagnostics
                .values()
                .filter(|d| d.status == openlibertas_core::domain::McpServerStatus::Connected)
                .count();
            if total == 0 {
                " [MCP: ...]".to_string()
            } else {
                format!(" [MCP: {}/{}]", connected, total)
            }
        } else {
            " [MCP: ✗]".to_string()
        }
    };

    let plan_indicator = if app.engine.agents().mode == openlibertas_core::engine::AgentMode::Plan {
        " [Plan]"
    } else {
        ""
    };
    let agent_indicator = match app.engine.agents().status {
        openlibertas_core::engine::AgentModeStatus::Disabled => String::new(),
        openlibertas_core::engine::AgentModeStatus::Idle => {
            format!(
                " [Agents: ○ {} Ready{}]",
                app.engine.agents().persona,
                plan_indicator
            )
        }
        openlibertas_core::engine::AgentModeStatus::Active => {
            format!(
                " [Agents: ● {}/{} {}{}]",
                app.engine.agents().current_iteration,
                app.engine.agents().max_iterations,
                app.engine.agents().persona,
                plan_indicator
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
            .chat()
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

    let ctx = app
        .session_manager
        .as_ref()
        .map(|sm| sm.context())
        .cloned()
        .unwrap_or_default();

    let branch_indicator = if ctx.parent_id.is_some() {
        let parent_name = ctx.parent_id.as_deref().unwrap_or("unknown");
        let truncated = if parent_name.len() > 20 {
            format!("{}...", &parent_name[..17])
        } else {
            parent_name.to_string()
        };
        format!(" [branch of {}]", truncated)
    } else {
        String::new()
    };

    let branches_indicator = if ctx.has_branches { " ⎇" } else { "" };

    let rlm_indicator = if app.rlm_mode { " [RLM]" } else { "" };

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
        Span::styled(
            rlm_indicator,
            Style::default()
                .fg(app.theme.error_color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            branch_indicator,
            Style::default()
                .fg(app.theme.secondary())
                .add_modifier(Modifier::ITALIC),
        ),
        Span::styled(
            branches_indicator,
            Style::default()
                .fg(app.theme.primary())
                .add_modifier(Modifier::BOLD),
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

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let model_name = app.models.current.as_deref().unwrap_or("Unknown");

    let (conn_symbol, conn_color) = match app.connection_status {
        ConnectionStatus::Connected => ("●", app.theme.user_color()),
        ConnectionStatus::Disconnected => ("●", app.theme.error_color()),
        ConnectionStatus::Checking => ("◐", app.theme.secondary()),
    };

    let display_model =
        if app.engine.agents().status == openlibertas_core::engine::AgentModeStatus::Disabled {
            model_name.to_string()
        } else {
            format!("{}@{}", app.engine.agents().persona, model_name)
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
    } else if app.rlm_mode {
        "🐍 "
    } else if app.engine.agents().status == openlibertas_core::engine::AgentModeStatus::Disabled {
        "> "
    } else {
        "✨ "
    };
    let prompt_width = prompt_symbol.width();
    let viewport_width = area.width as usize;

    let safe_cursor_pos = {
        app.engine
            .input()
            .cursor_pos
            .min(app.engine.input().buffer.len())
    };

    let text_before_cursor = &app.engine.input().buffer[..safe_cursor_pos];
    let cursor_display_pos = prompt_width + text_before_cursor.width();

    let mut scroll_offset = app.engine.input().scroll_offset;
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

    for (byte_idx, ch) in app.engine.input().buffer.char_indices() {
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
                } else if app.engine.chat().streaming {
                    "Streaming...".to_string()
                } else if app.rlm_mode {
                    "RLM mode — model will execute Python code".to_string()
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

fn provider_color(provider: &str, theme: Theme) -> ratatui::style::Color {
    match provider.to_lowercase().as_str() {
        "kimi" => theme.primary(),
        "local" | "ollama" | "lm-studio" | "llamacpp" => theme.tool_color(),
        _ => theme.secondary(),
    }
}
