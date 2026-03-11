use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// State for the broadcast prompt dialog.
#[derive(Debug, Clone)]
pub struct BroadcastDialog {
    /// The prompt text to broadcast.
    pub prompt_input: String,
    /// Number of running sessions that will receive the prompt.
    pub target_count: usize,
    /// Target worktree names.
    pub target_names: Vec<String>,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl BroadcastDialog {
    pub fn new(target_count: usize, target_names: Vec<String>) -> Self {
        Self {
            prompt_input: String::new(),
            target_count,
            target_names,
            confirmed: false,
            cancelled: false,
        }
    }
}

/// Render the broadcast dialog as a centered popup.
pub fn render(f: &mut Frame, dialog: &BroadcastDialog) {
    let target_lines = dialog.target_names.len().min(5);
    let height = (10 + target_lines) as u16;
    let area = centered_rect(60, height, f.area());
    f.render_widget(Clear, area);

    let title = format!(" Broadcast to {} session(s) ", dialog.target_count);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let target_display_height = (target_lines + 1) as u16;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                     // instructions
            Constraint::Length(target_display_height), // targets
            Constraint::Length(2),                     // input
            Constraint::Length(1),                     // spacer
            Constraint::Length(1),                     // buttons
        ])
        .margin(1)
        .split(inner);

    // Instructions
    let instructions = Line::from(Span::styled(
        "Type a prompt to send to all running sessions:",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(instructions), chunks[0]);

    // Target sessions
    let mut target_lines_vec: Vec<Line> = Vec::new();
    let display_count = dialog.target_names.len().min(5);
    for name in dialog.target_names.iter().take(display_count) {
        target_lines_vec.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(theme::ICON_RUNNING, theme::status_running_style()),
            Span::raw(format!(" {}", name)),
        ]));
    }
    if dialog.target_names.len() > 5 {
        target_lines_vec.push(Line::from(Span::styled(
            format!("  ... and {} more", dialog.target_names.len() - 5),
            Style::default().fg(Color::DarkGray),
        )));
    }
    if target_lines_vec.is_empty() {
        target_lines_vec.push(Line::from(Span::styled(
            "  (no running sessions)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(target_lines_vec), chunks[1]);

    // Prompt input
    let input_label = Line::from(vec![
        Span::styled("Prompt: ", theme::help_key_style()),
        Span::styled(&dialog.prompt_input, Style::default().fg(Color::Yellow)),
        Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
    ]);
    f.render_widget(Paragraph::new(input_label), chunks[2]);

    // Buttons
    let can_send = !dialog.prompt_input.trim().is_empty() && dialog.target_count > 0;
    let confirm_style = if can_send {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let buttons = Line::from(vec![
        Span::styled(" [Enter] Send ", confirm_style),
        Span::raw("  "),
        Span::styled("[Esc] Cancel", Style::default().fg(Color::Red)),
    ]);
    f.render_widget(Paragraph::new(buttons), chunks[4]);
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
