use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::ui::theme;

/// State for the delete confirmation dialog.
#[derive(Debug, Clone)]
pub struct DeleteDialog {
    pub worktree_name: String,
    pub diff_preview: String,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl DeleteDialog {
    pub fn new(worktree_name: String, diff_preview: String) -> Self {
        Self {
            worktree_name,
            diff_preview,
            confirmed: false,
            cancelled: false,
        }
    }
}

/// Render the delete confirmation dialog.
pub fn render(f: &mut Frame, dialog: &DeleteDialog) {
    let area = centered_rect(60, 16, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" Delete '{}' ", dialog.worktree_name))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // warning
            Constraint::Min(5),   // diff preview
            Constraint::Length(2), // buttons
        ])
        .margin(1)
        .split(inner);

    // Warning
    let warning = Paragraph::new(Line::from(vec![
        Span::styled(
            "A snapshot will be saved before deletion.",
            theme::help_desc_style(),
        ),
    ]));
    f.render_widget(warning, chunks[0]);

    // Diff preview
    let mut diff_lines: Vec<Line> = Vec::new();
    diff_lines.push(Line::from(Span::styled(
        "Changes in worktree:",
        theme::help_key_style(),
    )));
    for line in dialog.diff_preview.lines().take(chunks[1].height as usize - 1) {
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
    f.render_widget(diff, chunks[1]);

    // Buttons
    let buttons = Line::from(vec![
        Span::styled(
            " [y] Delete ",
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("[n/Esc] Cancel", Style::default().fg(Color::Gray)),
    ]);
    f.render_widget(Paragraph::new(buttons), chunks[2]);
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
