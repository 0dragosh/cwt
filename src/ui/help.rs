use ratatui::layout::{Constraint, Direction, Layout, Rect};

use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// Render the help overlay — a scrollable keybinding reference.
pub fn render(f: &mut Frame, scroll: u16) {
    let height = f.area().height.saturating_sub(2);
    let area = centered_rect(80, height, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" cwt Keybindings — j/k scroll, any other key to close ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let sections = vec![
        (
            "Worktree Actions",
            vec![
                ("n", "New worktree (Enter to quick-create)"),
                ("Enter", "Launch/resume provider session"),
                ("s", "Launch/resume provider session"),
                ("e", "Open shell in worktree (tmux tab)"),
                ("h", "Handoff changes (worktree <-> local)"),
                ("p", "Promote ephemeral to permanent"),
                ("d", "Delete worktree (saves snapshot)"),
                ("g", "Run garbage collection"),
                ("r", "Restore from snapshot"),
            ],
        ),
        (
            "Orchestration",
            vec![
                ("t", "Dispatch tasks (multi-worktree)"),
                ("b", "Broadcast prompt to all sessions"),
            ],
        ),
        (
            "Ship Pipeline",
            vec![
                ("P", "Create PR (push + gh pr create)"),
                ("S", "Ship it (push + PR + mark shipping)"),
                ("c", "Open CI logs in browser"),
            ],
        ),
        (
            "Navigation",
            vec![
                ("j / Down", "Move down / scroll inspector"),
                ("k / Up", "Move up / scroll inspector"),
                ("Tab", "Switch panel focus (fwd)"),
                ("Shift+Tab", "Switch panel focus (back)"),
                ("R", "Switch to repo panel (forest mode)"),
                ("/", "Filter/search worktrees"),
                ("Esc", "Clear filter / close dialog"),
                ("Mouse", "Click to select, scroll to navigate"),
            ],
        ),
        (
            "Permissions",
            vec![
                ("m", "Cycle mode (Normal/Unsandboxed/Elevated Unsandboxed)"),
                ("M", "Save current mode as default"),
                ("o", "Cycle provider (Claude/Codex)"),
                ("O", "Save current provider as default"),
            ],
        ),
        (
            "General",
            vec![
                ("?", "Toggle this help"),
                ("q", "Quit cwt"),
                ("Ctrl+C", "Force quit"),
            ],
        ),
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
                Span::styled(format!("{:>10}", key), theme::help_key_style()),
                Span::raw("  "),
                Span::styled(*desc, theme::help_desc_style()),
            ]));
        }

        lines.push(Line::default());
    }

    let help = Paragraph::new(lines).scroll((scroll, 0));
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
