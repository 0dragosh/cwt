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
    /// Available remote host names (empty = no remotes configured)
    pub remote_hosts: Vec<String>,
    /// Index into remote_hosts, 0 = "(local)"
    pub remote_index: usize,
    /// Selected remote host name, None = local
    pub selected_remote: Option<String>,
    /// Number of fields: 4 if no remotes, 5 if remotes available
    field_count: usize,
    /// Which field is focused: 0=name, 1=branch, 2=remote (if available), then carry, then confirm
    pub focus: usize,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl CreateDialog {
    pub fn new(branches: Vec<String>) -> Self {
        Self::with_remotes(branches, Vec::new())
    }

    pub fn with_remotes(branches: Vec<String>, remote_hosts: Vec<String>) -> Self {
        let default_branch = branches
            .iter()
            .find(|b| *b == "main" || *b == "master")
            .cloned()
            .unwrap_or_else(|| branches.first().cloned().unwrap_or_else(|| "main".to_string()));

        let branch_index = branches
            .iter()
            .position(|b| b == &default_branch)
            .unwrap_or(0);

        let has_remotes = !remote_hosts.is_empty();
        let field_count = if has_remotes { 5 } else { 4 };

        Self {
            name_input: String::new(),
            base_branch: default_branch,
            branches,
            branch_index,
            carry_changes: false,
            remote_hosts,
            remote_index: 0,
            selected_remote: None,
            field_count,
            focus: 0,
            confirmed: false,
            cancelled: false,
        }
    }

    /// Whether this dialog has remote host options.
    pub fn has_remotes(&self) -> bool {
        !self.remote_hosts.is_empty()
    }

    /// Get the field index for the carry checkbox.
    pub fn carry_field(&self) -> usize {
        if self.has_remotes() { 3 } else { 2 }
    }

    /// Get the field index for the confirm button.
    pub fn confirm_field(&self) -> usize {
        if self.has_remotes() { 4 } else { 3 }
    }

    /// Get the field index for the remote selector.
    pub fn remote_field(&self) -> Option<usize> {
        if self.has_remotes() { Some(2) } else { None }
    }

    pub fn next_field(&mut self) {
        self.focus = (self.focus + 1) % self.field_count;
    }

    pub fn prev_field(&mut self) {
        self.focus = if self.focus == 0 { self.field_count - 1 } else { self.focus - 1 };
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

    /// Cycle to the next remote host option.
    pub fn next_remote(&mut self) {
        if self.remote_hosts.is_empty() {
            return;
        }
        // Options: 0 = "(local)", 1..N = remote hosts
        let total = self.remote_hosts.len() + 1;
        self.remote_index = (self.remote_index + 1) % total;
        self.selected_remote = if self.remote_index == 0 {
            None
        } else {
            Some(self.remote_hosts[self.remote_index - 1].clone())
        };
    }

    /// Cycle to the previous remote host option.
    pub fn prev_remote(&mut self) {
        if self.remote_hosts.is_empty() {
            return;
        }
        let total = self.remote_hosts.len() + 1;
        self.remote_index = if self.remote_index == 0 {
            total - 1
        } else {
            self.remote_index - 1
        };
        self.selected_remote = if self.remote_index == 0 {
            None
        } else {
            Some(self.remote_hosts[self.remote_index - 1].clone())
        };
    }

    /// Get the current remote label for display.
    pub fn remote_label(&self) -> &str {
        if self.remote_index == 0 || self.remote_hosts.is_empty() {
            "(local)"
        } else {
            &self.remote_hosts[self.remote_index - 1]
        }
    }
}

/// Render the create dialog as a centered popup.
pub fn render(f: &mut Frame, dialog: &CreateDialog) {
    let has_remotes = dialog.has_remotes();
    let dialog_height: u16 = if has_remotes { 16 } else { 14 };
    let area = centered_rect(60, dialog_height, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" New Worktree ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let constraints = if has_remotes {
        vec![
            Constraint::Length(2), // name field
            Constraint::Length(2), // branch field
            Constraint::Length(2), // remote host field
            Constraint::Length(2), // carry checkbox
            Constraint::Length(1), // spacer
            Constraint::Length(1), // buttons
        ]
    } else {
        vec![
            Constraint::Length(2), // name field
            Constraint::Length(2), // branch field
            Constraint::Length(2), // carry checkbox
            Constraint::Length(1), // spacer
            Constraint::Length(1), // buttons
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .margin(1)
        .split(inner);

    // Name field
    let name_style = if dialog.focus == 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let name_label = Line::from(vec![
        Span::styled("Name:   ", theme::help_key_style()),
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
        Span::styled("Base:   ", theme::help_key_style()),
        Span::styled(&dialog.base_branch, branch_style),
        if dialog.focus == 1 {
            Span::raw(" (left/right to change)")
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(branch_label), chunks[1]);

    // Determine chunk indices for fields after branch
    let (remote_chunk, carry_chunk, _spacer_chunk, button_chunk) = if has_remotes {
        (Some(2), 3, 4, 5)
    } else {
        (None, 2, 3, 4)
    };

    // Remote host field (only if remotes are configured)
    if let Some(rc) = remote_chunk {
        let remote_field_idx = dialog.remote_field().unwrap_or(2);
        let remote_style = if dialog.focus == remote_field_idx {
            Style::default().fg(Color::Magenta)
        } else {
            Style::default()
        };
        let remote_label = Line::from(vec![
            Span::styled("Remote: ", theme::help_key_style()),
            Span::styled(dialog.remote_label(), remote_style),
            if dialog.focus == remote_field_idx {
                Span::raw(" (left/right to change)")
            } else {
                Span::raw("")
            },
        ]);
        f.render_widget(Paragraph::new(remote_label), chunks[rc]);
    }

    // Carry checkbox
    let carry_field_idx = dialog.carry_field();
    let carry_style = if dialog.focus == carry_field_idx {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let checkbox = if dialog.carry_changes { "[x]" } else { "[ ]" };
    let carry_label = Line::from(vec![
        Span::styled("Carry:  ", theme::help_key_style()),
        Span::styled(
            format!("{} carry local changes", checkbox),
            carry_style,
        ),
    ]);
    f.render_widget(Paragraph::new(carry_label), chunks[carry_chunk]);

    // Buttons
    let confirm_field_idx = dialog.confirm_field();
    let confirm_style = if dialog.focus == confirm_field_idx {
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
    f.render_widget(Paragraph::new(buttons), chunks[button_chunk]);
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
