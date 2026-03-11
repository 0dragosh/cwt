use ratatui::layout::{Constraint, Direction, Layout, Rect};

use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// Render the help overlay — a full-screen keybinding reference.
pub fn render(f: &mut Frame) {
    let area = centered_rect(60, 28, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" cwt Keybindings — press any key to close ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let sections = vec![
        ("Worktree Actions", vec![
            ("n", "New worktree"),
            ("s", "Launch/resume Claude session"),
            ("h", "Handoff changes (worktree <-> local)"),
            ("p", "Promote ephemeral to permanent"),
            ("d", "Delete worktree (saves snapshot)"),
            ("g", "Run garbage collection"),
            ("r", "Restore from snapshot"),
            ("Enter", "Open shell in worktree (tmux pane)"),
        ]),
        ("Navigation", vec![
            ("j / Down", "Move down / scroll inspector"),
            ("k / Up", "Move up / scroll inspector"),
            ("Tab", "Switch panel focus (fwd)"),
            ("Shift+Tab", "Switch panel focus (back)"),
            ("R", "Switch to repo panel (forest mode)"),
            ("/", "Filter/search worktrees"),
            ("Esc", "Clear filter / close dialog"),
            ("Mouse", "Click to select, scroll to navigate"),
        ]),
        ("General", vec![
            ("?", "Toggle this help"),
            ("q", "Quit cwt"),
            ("Ctrl+C", "Force quit"),
        ]),
    ];

    let mut lines: Vec<Line> = Vec::new();

    for (section_title, bindings) in &sections {
        // Section header
        lines.push(Line::from(Span::styled(
            format!("  {}", section_title),
            theme::title_style(),
        )));

        for (key, desc) in bindings {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("{:>10}", key),
                    theme::help_key_style(),
                ),
                Span::raw("  "),
                Span::styled(*desc, theme::help_desc_style()),
            ]));
        }

        lines.push(Line::default());
    }

    let help = Paragraph::new(lines);
    f.render_widget(help, inner);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let actual_height = height.min(area.height.saturating_sub(2));
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(actual_height)) / 2),
            Constraint::Length(actual_height),
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
