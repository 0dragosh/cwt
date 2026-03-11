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
    pub confirmed: bool,
    pub cancelled: bool,
}

impl HandoffDialog {
    pub fn new(worktree_name: String, diff_preview: String, files_changed: usize) -> Self {
        Self {
            worktree_name,
            direction: HandoffDirection::WorktreeToLocal,
            diff_preview,
            files_changed,
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
    let area = centered_rect(65, 18, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" Handoff — {} ", dialog.worktree_name))
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // direction selector
            Constraint::Length(1), // separator
            Constraint::Min(5),   // diff preview
            Constraint::Length(2), // buttons
        ])
        .margin(1)
        .split(inner);

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
    f.render_widget(Paragraph::new(direction_lines), chunks[0]);

    // Separator
    let sep = Paragraph::new("─".repeat(chunks[1].width as usize))
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(sep, chunks[1]);

    // Diff preview
    let mut diff_lines: Vec<Line> = Vec::new();
    diff_lines.push(Line::from(Span::styled(
        format!("{} file(s) to transfer:", dialog.files_changed),
        theme::help_key_style(),
    )));
    for line in dialog.diff_preview.lines().take(chunks[2].height as usize - 1) {
        let style = if line.starts_with('+') {
            theme::diff_add_style()
        } else if line.starts_with('-') {
            theme::diff_remove_style()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        diff_lines.push(Line::from(Span::styled(line.to_string(), style)));
    }

    let diff = Paragraph::new(diff_lines).wrap(Wrap { trim: false });
    f.render_widget(diff, chunks[2]);

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
    f.render_widget(Paragraph::new(buttons), chunks[3]);
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
