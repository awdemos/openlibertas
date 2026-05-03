use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{ThemeSet, Style as SyntectStyle},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use crate::theme::Theme;

pub struct MarkdownRenderer {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    pub fn render(&self, markdown: &str, theme: Theme) -> Text<'_> {
        let mut lines = Vec::new();
        let mut current_spans = Vec::new();
        let mut current_style = Style::default();
        let mut in_code_block = false;
        let mut code_language = String::new();
        let mut code_content = String::new();
        let mut list_stack = Vec::new();

        let parser = Parser::new(markdown);
        let mut events = Vec::new();
        for event in parser {
            events.push(event);
        }

        let mut i = 0;
        while i < events.len() {
            let event = &events[i];
            match event {
                Event::Start(tag) => match tag {
                    Tag::CodeBlock(kind) => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        in_code_block = true;
                        code_language = match kind {
                            pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                            pulldown_cmark::CodeBlockKind::Indented => String::new(),
                        };
                        code_content.clear();
                    }
                    Tag::Strong => {
                        current_style = current_style.add_modifier(Modifier::BOLD);
                    }
                    Tag::Emphasis => {
                        current_style = current_style.add_modifier(Modifier::ITALIC);
                    }
                    Tag::List(start) => {
                        list_stack.push(*start);
                    }
                    Tag::Heading { level, .. } => {
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                        let color = theme.heading_color(*level as u8);
                        current_style = Style::default()
                            .fg(color)
                            .add_modifier(Modifier::BOLD);
                    }
                    Tag::BlockQuote(_) => {
                        current_style = Style::default().fg(theme.system_color());
                    }
                    _ => {}
                },
                Event::End(tag_end) => match tag_end {
                    TagEnd::CodeBlock => {
                        in_code_block = false;
                        if !code_content.is_empty() {
                            let highlighted = self.highlight_code(&code_language, &code_content);
                            lines.extend(highlighted);
                        }
                        code_language.clear();
                        code_content.clear();
                    }
                    TagEnd::Heading(_) => {
                        current_style = Style::default();
                        if !current_spans.is_empty() {
                            lines.push(Line::from(std::mem::take(&mut current_spans)));
                        }
                    }
                    TagEnd::Strong => {
                        current_style = current_style.remove_modifier(Modifier::BOLD);
                    }
                    TagEnd::Emphasis => {
                        current_style = current_style.remove_modifier(Modifier::ITALIC);
                    }
                    TagEnd::BlockQuote(_) => {
                        current_style = Style::default();
                    }
                    TagEnd::List(_) => {
                        list_stack.pop();
                    }
                    _ => {}
                },
                Event::Text(text) => {
                    if in_code_block {
                        code_content.push_str(text);
                    } else {
                        let prefix = if !list_stack.is_empty() {
                            "• "
                        } else {
                            ""
                        };
                        if !prefix.is_empty() && current_spans.is_empty() {
                            current_spans.push(Span::styled(prefix, current_style));
                        }
                        current_spans.push(Span::styled(text.to_string(), current_style));
                    }
                }
                Event::Code(code) => {
                    current_spans.push(Span::styled(
                        code.to_string(),
                        Style::default().bg(theme.code_block_bg()).fg(theme.foreground()),
                    ));
                }
                Event::Html(html) => {
                    current_spans.push(Span::styled(html.to_string(), current_style));
                }
                Event::SoftBreak | Event::HardBreak => {
                    if !current_spans.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current_spans)));
                    } else {
                        lines.push(Line::from(""));
                    }
                }
                Event::Rule => {
                    if !current_spans.is_empty() {
                        lines.push(Line::from(std::mem::take(&mut current_spans)));
                    }
                    lines.push(Line::from(vec![Span::styled(
                        "─".repeat(40),
                        Style::default().fg(theme.border_color()),
                    )]));
                }
                _ => {}
            }
            i += 1;
        }

        if !current_spans.is_empty() {
            lines.push(Line::from(current_spans));
        }

        if lines.is_empty() {
            lines.push(Line::from(""));
        }

        Text::from(lines)
    }

    fn highlight_code(&self, language: &str, code: &str) -> Vec<Line<'_>> {
        let theme = self.theme_set.themes.get("base16-ocean.dark").unwrap_or_else(|| {
            self.theme_set.themes.values().next()
                .expect("No themes available")
        });

        let syntax = if language.is_empty() {
            self.syntax_set.find_syntax_plain_text()
        } else {
            self.syntax_set.find_syntax_by_token(language)
                .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text())
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();

        for line in LinesWithEndings::from(code) {
            let highlighted = highlighter.highlight_line(line, &self.syntax_set);
            match highlighted {
                Ok(regions) => {
                    let spans: Vec<Span> = regions
                        .iter()
                        .map(|(style, text)| {
                            Span::styled(
                                text.to_string(),
                                syntect_style_to_ratatui(*style),
                            )
                        })
                        .collect();
                    lines.push(Line::from(spans));
                }
                Err(_) => {
                    lines.push(Line::from(line.to_string()));
                }
            }
        }

        lines
    }
}

fn syntect_style_to_ratatui(style: SyntectStyle) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let mut ratatui_style = Style::default().fg(fg);
    
    if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(syntect::highlighting::FontStyle::UNDERLINE) {
        ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
    }
    
    ratatui_style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_plain_text() {
        let renderer = MarkdownRenderer::new();
        let text = renderer.render("Hello world", crate::theme::Theme::Default);
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].spans.len(), 1);
        assert_eq!(text.lines[0].spans[0].content, "Hello world");
    }

    #[test]
    fn render_bold_text() {
        let renderer = MarkdownRenderer::new();
        let text = renderer.render("**bold**", crate::theme::Theme::Default);
        let has_bold = text.lines.iter().any(|line| {
            line.spans.iter().any(|span| {
                span.content == "bold" && span.style.add_modifier == Modifier::BOLD
            })
        });
        assert!(has_bold, "Should have bold text");
    }

    #[test]
    fn render_code_block() {
        let renderer = MarkdownRenderer::new();
        let text = renderer.render("```rust\nlet x = 1;\n```", crate::theme::Theme::Default);
        assert!(!text.lines.is_empty(), "Should have code block lines");
    }

    #[test]
    fn render_heading() {
        let renderer = MarkdownRenderer::new();
        let text = renderer.render("# Title", crate::theme::Theme::Default);
        assert!(!text.lines.is_empty());
        let first_line = &text.lines[0];
        assert!(first_line.spans.iter().any(|s| s.content == "Title"));
    }
}
