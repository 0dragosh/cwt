use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// State for the dispatch (multi-task creation) dialog.
#[derive(Debug, Clone)]
pub struct DispatchDialog {
    /// The current line being typed.
    pub current_input: String,
    /// All tasks entered so far.
    pub tasks: Vec<String>,
    /// Base branch for all worktrees.
    pub base_branch: String,
    pub branches: Vec<String>,
    pub branch_index: usize,
    /// Which section has focus: 0=task input, 1=branch, 2=confirm
    pub focus: usize,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl DispatchDialog {
    pub fn new(branches: Vec<String>) -> Self {
        let default_branch = branches
            .iter()
            .find(|b| *b == "main" || *b == "master")
            .cloned()
            .unwrap_or_else(|| branches.first().cloned().unwrap_or_else(|| "main".to_string()));

        let branch_index = branches
            .iter()
            .position(|b| b == &default_branch)
            .unwrap_or(0);

        Self {
            current_input: String::new(),
            tasks: Vec::new(),
            base_branch: default_branch,
            branches,
            branch_index,
            focus: 0,
            confirmed: false,
            cancelled: false,
        }
    }

    pub fn add_task(&mut self) {
        let task = self.current_input.trim().to_string();
        if !task.is_empty() {
            self.tasks.push(task);
            self.current_input.clear();
        }
    }

    pub fn remove_last_task(&mut self) {
        if self.current_input.is_empty() {
            self.tasks.pop();
        }
    }

    pub fn next_field(&mut self) {
        self.focus = (self.focus + 1) % 3;
    }

    pub fn prev_field(&mut self) {
        self.focus = if self.focus == 0 { 2 } else { self.focus - 1 };
    }

    pub fn next_branch(&mut self) {
        if !self.branches.is_empty() {
            self.branch_index = (self.branch_index + 1) % self.branches.len();
            self.base_branch = self.branches[self.branch_index].clone();
        }
    }

    pub fn prev_branch(&mut self) {
        if !self.branches.is_empty() {
            self.branch_index = if self.branch_index == 0 {
                self.branches.len() - 1
            } else {
                self.branch_index - 1
            };
            self.base_branch = self.branches[self.branch_index].clone();
        }
    }

    pub fn total_tasks(&self) -> usize {
        let pending = if self.current_input.trim().is_empty() {
            0
        } else {
            1
        };
        self.tasks.len() + pending
    }
}

/// Render the dispatch dialog as a centered popup.
pub fn render(f: &mut Frame, dialog: &DispatchDialog) {
    // Dynamic height based on number of tasks
    let task_lines = dialog.tasks.len().clamp(1, 8);
    let height = (10 + task_lines) as u16;
    let area = centered_rect(65, height, f.area());
    f.render_widget(Clear, area);

    let title = format!(" Dispatch Tasks ({} queued) ", dialog.tasks.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let task_display_height = task_lines as u16 + 1;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                  // instructions
            Constraint::Length(task_display_height), // task list
            Constraint::Length(2),                  // current input
            Constraint::Length(2),                  // branch selector
            Constraint::Length(1),                  // spacer
            Constraint::Length(1),                  // buttons
        ])
        .margin(1)
        .split(inner);

    // Instructions
    let instructions = Line::from(vec![
        Span::styled(
            "Enter tasks one per line. Press Enter to add each task.",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(instructions), chunks[0]);

    // Task list
    let mut task_lines_vec: Vec<Line> = Vec::new();
    for (i, task) in dialog.tasks.iter().enumerate() {
        let truncated = if task.len() > 55 {
            format!("{}...", &task[..52])
        } else {
            task.clone()
        };
        task_lines_vec.push(Line::from(vec![
            Span::styled(
                format!("  {}. ", i + 1),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(truncated),
        ]));
    }
    if task_lines_vec.is_empty() {
        task_lines_vec.push(Line::from(Span::styled(
            "  (no tasks yet)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(task_lines_vec), chunks[1]);

    // Current input
    let input_style = if dialog.focus == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let input_label = Line::from(vec![
        Span::styled("Task: ", theme::help_key_style()),
        Span::styled(&dialog.current_input, input_style),
        if dialog.focus == 0 {
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK))
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(input_label), chunks[2]);

    // Branch selector
    let branch_style = if dialog.focus == 1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let branch_label = Line::from(vec![
        Span::styled("Base: ", theme::help_key_style()),
        Span::styled(&dialog.base_branch, branch_style),
        if dialog.focus == 1 {
            Span::raw(" (left/right to change)")
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(branch_label), chunks[3]);

    // Buttons
    let confirm_style = if dialog.focus == 2 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let task_count = dialog.total_tasks();
    let buttons = Line::from(vec![
        Span::styled(
            format!(" [Tab] Dispatch {} task(s) ", task_count),
            confirm_style,
        ),
        Span::raw("  "),
        Span::styled("[Esc] Cancel", Style::default().fg(Color::Red)),
    ]);
    f.render_widget(Paragraph::new(buttons), chunks[5]);
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
