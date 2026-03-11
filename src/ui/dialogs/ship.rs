use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::ship::pr::{CiStatus, PrStatus};
use crate::ui::theme;

/// State for the ship confirmation dialog.
#[derive(Debug, Clone)]
pub struct ShipDialog {
    pub worktree_name: String,
    pub branch: String,
    pub base_branch: String,
    pub diff_preview: String,
    pub files_changed: usize,
    /// Which mode: 0 = "Create PR", 1 = "Ship it (push + PR)"
    pub mode: usize,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl ShipDialog {
    pub fn new(
        worktree_name: String,
        branch: String,
        base_branch: String,
        diff_preview: String,
        files_changed: usize,
    ) -> Self {
        Self {
            worktree_name,
            branch,
            base_branch,
            diff_preview,
            files_changed,
            mode: 1, // default to "Ship it"
            confirmed: false,
            cancelled: false,
        }
    }

    pub fn toggle_mode(&mut self) {
        self.mode = if self.mode == 0 { 1 } else { 0 };
    }
}

/// Render the ship dialog.
pub fn render(f: &mut Frame, dialog: &ShipDialog) {
    let area = centered_rect(65, 18, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" Ship -- {} ", dialog.worktree_name))
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // branch info
            Constraint::Length(1), // separator
            Constraint::Length(3), // mode selector
            Constraint::Length(1), // separator
            Constraint::Min(3),    // diff preview
            Constraint::Length(2), // buttons
        ])
        .margin(1)
        .split(inner);

    // Branch info
    let branch_lines = vec![
        Line::from(vec![
            Span::styled("Branch:    ", theme::help_key_style()),
            Span::raw(&dialog.branch),
        ]),
        Line::from(vec![
            Span::styled("Base:      ", theme::help_key_style()),
            Span::raw(&dialog.base_branch),
        ]),
    ];
    f.render_widget(Paragraph::new(branch_lines), chunks[0]);

    // Separator
    let sep = Paragraph::new("\u{2500}".repeat(chunks[1].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(sep, chunks[1]);

    // Mode selector
    let (pr_style, ship_style) = if dialog.mode == 0 {
        (
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::DarkGray),
        )
    } else {
        (
            Style::default().fg(Color::DarkGray),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mode_lines = vec![
        Line::from(Span::styled(
            "Action (Tab to toggle):",
            theme::help_key_style(),
        )),
        Line::from(vec![
            Span::styled(" Create PR only ", pr_style),
            Span::raw("  "),
            Span::styled(" Ship it (push + PR) ", ship_style),
        ]),
    ];
    f.render_widget(Paragraph::new(mode_lines), chunks[2]);

    // Separator
    let sep2 = Paragraph::new("\u{2500}".repeat(chunks[3].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(sep2, chunks[3]);

    // Diff preview
    let diff_area = chunks[4];
    let mut diff_lines: Vec<Line> = Vec::new();
    diff_lines.push(Line::from(Span::styled(
        format!("{} file(s) changed:", dialog.files_changed),
        theme::help_key_style(),
    )));
    for line in dialog
        .diff_preview
        .lines()
        .take(diff_area.height as usize - 1)
    {
        let style = if line.starts_with('+') {
            theme::diff_add_style()
        } else if line.starts_with('-') {
            theme::diff_remove_style()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        diff_lines.push(Line::from(Span::styled(line.to_string(), style)));
    }
    f.render_widget(
        Paragraph::new(diff_lines).wrap(Wrap { trim: false }),
        diff_area,
    );

    // Buttons
    let buttons = Line::from(vec![
        Span::styled(
            " [Enter] Confirm ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("[Esc] Cancel", Style::default().fg(Color::Gray)),
    ]);
    f.render_widget(Paragraph::new(buttons), chunks[5]);
}

/// Render PR/CI status inline in the worktree list as a compact span.
pub fn pr_ci_spans(pr_status: &PrStatus, ci_status: &CiStatus) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    match pr_status {
        PrStatus::None => {}
        PrStatus::Draft => {
            spans.push(Span::styled(
                " PR:draft",
                Style::default().fg(Color::DarkGray),
            ));
        }
        PrStatus::Open => {
            spans.push(Span::styled(" PR:open", Style::default().fg(Color::Blue)));
        }
        PrStatus::Approved => {
            spans.push(Span::styled(
                " PR:ok",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        PrStatus::Merged => {
            spans.push(Span::styled(
                " PR:merged",
                Style::default().fg(Color::Magenta),
            ));
        }
        PrStatus::Closed => {
            spans.push(Span::styled(" PR:closed", Style::default().fg(Color::Red)));
        }
    }

    match ci_status {
        CiStatus::None => {}
        CiStatus::Pending => {
            spans.push(Span::styled(" CI:...", Style::default().fg(Color::Yellow)));
        }
        CiStatus::Passed => {
            spans.push(Span::styled(" CI:ok", Style::default().fg(Color::Green)));
        }
        CiStatus::Failed => {
            spans.push(Span::styled(
                " CI:fail",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }
    }

    spans
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
