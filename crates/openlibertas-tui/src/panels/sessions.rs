use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;
use openlibertas_core::store::SessionMeta;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub struct SessionsPanelData<'a> {
    pub sessions: Vec<SessionMeta>,
    pub session_search: &'a str,
    pub session_selected: usize,
}

impl<'a> SessionsPanelData<'a> {
    pub fn from_app(app: &'a crate::app::App) -> Self {
        Self {
            sessions: app.filtered_sessions(),
            session_search: &app.session_search,
            session_selected: app.session_selected,
        }
    }
}

impl Panel for SessionsPanelData<'_> {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(80, 70, area);

        frame.render_widget(Clear, popup_area);

        let total_sessions = self.sessions.len();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(if self.session_search.is_empty() {
                format!(" Saved Sessions ({}) ", total_sessions)
            } else {
                format!(
                    " Saved Sessions ({} / {}) ",
                    total_sessions, self.session_search
                )
            })
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

        if self.sessions.is_empty() {
            let msg = if self.session_search.is_empty() {
                "No saved sessions.\nUse /save to save the current session."
            } else {
                "No sessions match your search."
            };
            let content = Paragraph::new(msg)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.system_color()));
            frame.render_widget(content, content_area);
        } else {
            let tree_mode = self.session_search.is_empty();
            let display_items: Vec<(usize, SessionMeta, usize)> = if tree_mode {
                let tree = build_session_tree(&self.sessions);
                tree.into_iter()
                    .enumerate()
                    .map(|(i, node)| (i, node.meta, node.depth))
                    .collect()
            } else {
                self.sessions
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, meta)| (i, meta, 0usize))
                    .collect()
            };

            let items: Vec<ListItem> = display_items
                .into_iter()
                .map(|(i, meta, depth)| {
                    let is_selected = i == self.session_selected;

                    let title = meta.title.as_deref().unwrap_or("Untitled");
                    let model_str = meta.model.as_deref().unwrap_or("unknown");
                    let time_str = meta
                        .updated_at
                        .as_ref()
                        .or(Some(&meta.created_at))
                        .map(|s| openlibertas_core::store::format_relative_time(s))
                        .unwrap_or_default();

                    let title_style = if is_selected {
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

                    let preview_style = if is_selected {
                        Style::default()
                            .bg(theme.primary())
                            .fg(theme.panel_bg())
                            .add_modifier(Modifier::ITALIC)
                    } else {
                        Style::default()
                            .fg(theme.secondary())
                            .add_modifier(Modifier::ITALIC)
                    };

                    let indent = "  ".repeat(depth);
                    let branch_prefix = if depth > 0 { "└─ " } else { "" };
                    let marker = if is_selected { "▸ " } else { "  " };

                    let title_line = Line::from(vec![
                        Span::styled(indent.clone(), Style::default()),
                        Span::styled(marker, Style::default().fg(theme.primary())),
                        Span::styled(
                            branch_prefix.to_string(),
                            Style::default().fg(theme.secondary()),
                        ),
                        Span::styled(title.to_string(), title_style),
                        Span::styled(
                            format!("  {}  {} msgs  {}", model_str, meta.message_count, time_str),
                            meta_style,
                        ),
                    ]);

                    let preview_text = if meta.preview.len() > 80 {
                        format!("{}...", &meta.preview[..80])
                    } else {
                        meta.preview.clone()
                    };
                    let preview_indent = indent.clone() + "    ";
                    let preview_line = Line::from(vec![
                        Span::styled(preview_indent, Style::default()),
                        Span::styled(preview_text, preview_style),
                    ]);

                    ListItem::new(Text::from(vec![title_line, preview_line]))
                })
                .collect();

            let list =
                List::new(items).highlight_style(Style::default().add_modifier(Modifier::BOLD));
            frame.render_widget(list, content_area);
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
                "Enter",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Load  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Del",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Delete  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Close  ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Type",
                Style::default()
                    .fg(theme.secondary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to search", Style::default().fg(theme.system_color())),
        ]))
        .alignment(Alignment::Center);
        let footer_area = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(footer_height),
            width: inner.width,
            height: footer_height,
        };
        frame.render_widget(footer, footer_area);
    }
}

struct SessionTreeNode {
    meta: SessionMeta,
    depth: usize,
}

fn build_session_tree(sessions: &[SessionMeta]) -> Vec<SessionTreeNode> {
    use std::collections::HashMap;
    let mut by_parent: HashMap<Option<String>, Vec<SessionMeta>> = HashMap::new();
    for s in sessions {
        by_parent
            .entry(s.parent_id.clone())
            .or_default()
            .push(s.clone());
    }
    for children in by_parent.values_mut() {
        children.sort_by(|a, b| {
            let a_time = a.updated_at.as_ref().unwrap_or(&a.created_at);
            let b_time = b.updated_at.as_ref().unwrap_or(&b.created_at);
            b_time.cmp(a_time)
        });
    }

    let mut result = Vec::new();
    if let Some(roots) = by_parent.remove(&None) {
        for root in roots {
            add_tree_node(&by_parent, &root, 0, &mut result);
        }
    }
    for (parent_id, children) in &by_parent {
        if parent_id.is_some() {
            for child in children {
                add_tree_node(&by_parent, child, 0, &mut result);
            }
        }
    }
    result
}

fn add_tree_node(
    by_parent: &std::collections::HashMap<Option<String>, Vec<SessionMeta>>,
    meta: &SessionMeta,
    depth: usize,
    result: &mut Vec<SessionTreeNode>,
) {
    result.push(SessionTreeNode {
        meta: meta.clone(),
        depth,
    });
    if let Some(children) = by_parent.get(&Some(meta.id.clone())) {
        for child in children {
            add_tree_node(by_parent, child, depth + 1, result);
        }
    }
}
