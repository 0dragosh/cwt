use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// State for the create worktree dialog.
#[derive(Debug, Clone)]
pub struct CreateDialog {
    pub name_input: String,
    pub base_branch: String,
    pub branches: Vec<String>,
    pub branch_index: usize,
    pub carry_changes: bool,
    /// Which field is focused: 0=name, 1=branch, 2=carry, 3=confirm
    pub focus: usize,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl CreateDialog {
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
            name_input: String::new(),
            base_branch: default_branch,
            branches,
            branch_index,
            carry_changes: false,
            focus: 0,
            confirmed: false,
            cancelled: false,
        }
    }

    pub fn next_field(&mut self) {
        self.focus = (self.focus + 1) % 4;
    }

    pub fn prev_field(&mut self) {
        self.focus = if self.focus == 0 { 3 } else { self.focus - 1 };
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

    pub fn toggle_carry(&mut self) {
        self.carry_changes = !self.carry_changes;
    }
}

/// Render the create dialog as a centered popup.
pub fn render(f: &mut Frame, dialog: &CreateDialog) {
    let area = centered_rect(60, 14, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" New Worktree ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // name field
            Constraint::Length(2), // branch field
            Constraint::Length(2), // carry checkbox
            Constraint::Length(1), // spacer
            Constraint::Length(1), // buttons
        ])
        .margin(1)
        .split(inner);

    // Name field
    let name_style = if dialog.focus == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let name_label = Line::from(vec![
        Span::styled("Name: ", theme::help_key_style()),
        Span::styled(
            if dialog.name_input.is_empty() {
                "(auto-generate)".to_string()
            } else {
                dialog.name_input.clone()
            },
            name_style,
        ),
        if dialog.focus == 0 {
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK))
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(name_label), chunks[0]);

    // Branch field
    let branch_style = if dialog.focus == 1 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let branch_label = Line::from(vec![
        Span::styled("Base: ", theme::help_key_style()),
        Span::styled(&dialog.base_branch, branch_style),
        if dialog.focus == 1 {
            Span::raw(" (←/→ to change)")
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(branch_label), chunks[1]);

    // Carry checkbox
    let carry_style = if dialog.focus == 2 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let checkbox = if dialog.carry_changes { "[x]" } else { "[ ]" };
    let carry_label = Line::from(vec![
        Span::styled("Carry: ", theme::help_key_style()),
        Span::styled(
            format!("{} carry local changes", checkbox),
            carry_style,
        ),
    ]);
    f.render_widget(Paragraph::new(carry_label), chunks[2]);

    // Buttons
    let confirm_style = if dialog.focus == 3 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let buttons = Line::from(vec![
        Span::styled(" [Enter] Create ", confirm_style),
        Span::raw("  "),
        Span::styled("[Esc] Cancel", Style::default().fg(Color::Red)),
    ]);
    f.render_widget(Paragraph::new(buttons), chunks[4]);
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
