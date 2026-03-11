use ratatui::layout::{Constraint, Direction, Layout, Rect};

use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// Render the help overlay.
pub fn render(f: &mut Frame) {
    let area = centered_rect(50, 20, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Keybindings ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let bindings = vec![
        ("n", "New worktree"),
        ("s", "Launch/resume session"),
        ("h", "Handoff changes"),
        ("p", "Promote to permanent"),
        ("d", "Delete (with snapshot)"),
        ("g", "Run garbage collection"),
        ("r", "Restore from snapshot"),
        ("Enter", "Open shell in worktree"),
        ("j/↓", "Move down"),
        ("k/↑", "Move up"),
        ("Tab", "Switch panel focus"),
        ("/", "Filter/search worktrees"),
        ("?", "Toggle this help"),
        ("q", "Quit"),
    ];

    let lines: Vec<Line> = bindings
        .into_iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{:>8}", key),
                    theme::help_key_style(),
                ),
                Span::raw("  "),
                Span::styled(desc, theme::help_desc_style()),
            ])
        })
        .collect();

    let help = Paragraph::new(lines);
    f.render_widget(help, inner);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
