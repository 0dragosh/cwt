use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Two-panel layout: worktree list (left) + inspector (right).
pub fn main_layout(area: Rect) -> (Rect, Rect, Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // main content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(outer[0]);

    (panels[0], panels[1], outer[1])
}
