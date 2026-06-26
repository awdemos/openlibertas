use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::app::App;

pub fn draw_models(frame: &mut Frame, app: &App) {
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
                let provider_color = super::provider_color(provider, app.theme);
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
