use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::ui::theme;
use crate::worktree::handoff::HandoffDirection;

/// State for the handoff dialog.
#[derive(Debug, Clone)]
pub struct HandoffDialog {
    pub worktree_name: String,
    pub direction: HandoffDirection,
    pub diff_preview: String,
    pub files_changed: usize,
    pub has_commits: bool,
    pub commit_count: usize,
    pub gitignore_warnings: Vec<String>,
    pub base_commit: Option<String>,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl HandoffDialog {
    pub fn new(
        worktree_name: String,
        diff_preview: String,
        files_changed: usize,
        has_commits: bool,
        commit_count: usize,
        gitignore_warnings: Vec<String>,
        base_commit: Option<String>,
    ) -> Self {
        Self {
            worktree_name,
            direction: HandoffDirection::WorktreeToLocal,
            diff_preview,
            files_changed,
            has_commits,
            commit_count,
            gitignore_warnings,
            base_commit,
            confirmed: false,
            cancelled: false,
        }
    }

    pub fn toggle_direction(&mut self) {
        self.direction = match self.direction {
            HandoffDirection::WorktreeToLocal => HandoffDirection::LocalToWorktree,
            HandoffDirection::LocalToWorktree => HandoffDirection::WorktreeToLocal,
        };
    }
}

/// Render the handoff dialog.
pub fn render(f: &mut Frame, dialog: &HandoffDialog) {
    let has_warnings = !dialog.gitignore_warnings.is_empty();
    let has_commits_info = dialog.has_commits && dialog.direction == HandoffDirection::WorktreeToLocal;
    let extra_height: u16 = if has_warnings { 3 } else { 0 } + if has_commits_info { 1 } else { 0 };
    let area = centered_rect(65, 18 + extra_height, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" Handoff — {} ", dialog.worktree_name))
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut constraints = vec![
        Constraint::Length(3), // direction selector
        Constraint::Length(1), // separator
    ];
    if has_commits_info {
        constraints.push(Constraint::Length(1)); // commit info
    }
    constraints.push(Constraint::Min(5)); // diff preview
    if has_warnings {
        constraints.push(Constraint::Length(3)); // gitignore warnings
    }
    constraints.push(Constraint::Length(2)); // buttons

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .margin(1)
        .split(inner);

    let mut chunk_idx = 0;

    // Direction selector
    let (left_style, right_style) = match dialog.direction {
        HandoffDirection::WorktreeToLocal => (
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        ),
        HandoffDirection::LocalToWorktree => (
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let direction_lines = vec![
        Line::from(Span::styled("Direction (Tab to toggle):", theme::help_key_style())),
        Line::from(vec![
            Span::styled(" Worktree → Local ", left_style),
            Span::raw("  "),
            Span::styled(" Local → Worktree ", right_style),
        ]),
    ];
    f.render_widget(Paragraph::new(direction_lines), chunks[chunk_idx]);
    chunk_idx += 1;

    // Separator
    let sep = Paragraph::new("─".repeat(chunks[chunk_idx].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(sep, chunks[chunk_idx]);
    chunk_idx += 1;

    // Commit info (only for WT→Local when there are commits)
    if has_commits_info {
        let commit_info = Line::from(vec![
            Span::styled(
                format!("{} commit(s)", dialog.commit_count),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" will be applied via format-patch/am"),
        ]);
        f.render_widget(Paragraph::new(commit_info), chunks[chunk_idx]);
        chunk_idx += 1;
    }

    // Diff preview
    let diff_area = chunks[chunk_idx];
    let mut diff_lines: Vec<Line> = Vec::new();
    diff_lines.push(Line::from(Span::styled(
        format!("{} file(s) to transfer:", dialog.files_changed),
        theme::help_key_style(),
    )));
    for line in dialog.diff_preview.lines().take(diff_area.height as usize - 1) {
        let style = if line.starts_with('+') {
            theme::diff_add_style()
        } else if line.starts_with('-') {
            theme::diff_remove_style()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        diff_lines.push(Line::from(Span::styled(line.to_string(), style)));
    }
    f.render_widget(Paragraph::new(diff_lines).wrap(Wrap { trim: false }), diff_area);
    chunk_idx += 1;

    // Gitignore warnings
    if has_warnings {
        let warn_count = dialog.gitignore_warnings.len();
        let mut warn_lines = vec![Line::from(Span::styled(
            format!("{} untracked file(s) won't transfer:", warn_count),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))];
        for file in dialog.gitignore_warnings.iter().take(2) {
            warn_lines.push(Line::from(Span::styled(
                format!("  {}", file),
                Style::default().fg(Color::Yellow),
            )));
        }
        if warn_count > 2 {
            warn_lines.push(Line::from(Span::styled(
                format!("  ...and {} more", warn_count - 2),
                Style::default().fg(Color::DarkGray),
            )));
        }
        f.render_widget(Paragraph::new(warn_lines), chunks[chunk_idx]);
        chunk_idx += 1;
    }

    // Buttons
    let buttons = Line::from(vec![
        Span::styled(
            " [Enter] Apply ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("[Esc] Cancel", Style::default().fg(Color::Gray)),
    ]);
    f.render_widget(Paragraph::new(buttons), chunks[chunk_idx]);
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
