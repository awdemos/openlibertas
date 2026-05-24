use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::panels::{centered_rect, Panel};
use crate::theme::Theme;

pub struct HelpPanelData;

impl HelpPanelData {
    pub fn from_app(_app: &crate::app::App) -> Self {
        Self
    }
}

impl Panel for HelpPanelData {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let popup_area = centered_rect(85, 85, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Keyboard Shortcuts ")
            .title_style(
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme.border_color()));

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
            ("Info", vec![("/help", "Show this help panel")]),
            (
                "Config",
                vec![
                    ("/model [name]", "Switch model or open picker"),
                    ("/theme [name]", "Change color theme"),
                    ("/temp [0.0-2.0]", "Set LLM temperature"),
                    ("/set-key <provider> <key>", "Store API key in OS keyring"),
                    ("/avatar [on/off]", "Toggle avatar display"),
                    ("/avatar-menu", "Open avatar configuration"),
                ],
            ),
            (
                "Session",
                vec![
                    ("/new", "Start new session"),
                    ("/clear", "Clear session"),
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
                    ("/search [query]", "Search in session"),
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
                "RLM",
                vec![
                    ("/rlm", "Toggle recursive language model mode"),
                    ("", "Model executes Python code via rlm_repl tool"),
                    ("", "Outputs FINAL(answer) when done."),
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
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )]));
            lines.push(Line::from(""));
            for (key, desc) in items {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {:<18}", key),
                        Style::default()
                            .fg(theme.secondary())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(desc, Style::default().fg(theme.foreground())),
                ]));
            }
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(theme.system_color())),
            Span::styled(
                "Esc",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" or ", Style::default().fg(theme.system_color())),
            Span::styled(
                "F1/?",
                Style::default()
                    .fg(theme.primary())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " to close this panel",
                Style::default().fg(theme.system_color()),
            ),
        ]));

        let content = Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(content, popup_area);
    }
}
