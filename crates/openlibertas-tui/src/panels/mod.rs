use crate::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

pub trait Panel {
    fn draw(&self, frame: &mut Frame, area: Rect, theme: &Theme);
}

/// Center a rectangle inside another by percent.
pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

pub mod agents;
pub mod avatar;
pub mod completions;
pub mod help;
pub mod mcp;
pub mod palette;
pub mod permission;
pub mod sessions;
pub mod themes;
pub mod tools;
