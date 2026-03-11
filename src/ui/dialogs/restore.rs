use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::state::SnapshotEntry;
use crate::ui::theme;

/// State for the restore-from-snapshot dialog.
#[derive(Debug, Clone)]
pub struct RestoreDialog {
    pub snapshots: Vec<SnapshotEntry>,
    pub selected: usize,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl RestoreDialog {
    pub fn new(snapshots: Vec<SnapshotEntry>) -> Self {
        Self {
            snapshots,
            selected: 0,
            confirmed: false,
            cancelled: false,
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.snapshots.is_empty() {
            return;
        }
        if delta > 0 {
            self.selected = (self.selected + 1).min(self.snapshots.len() - 1);
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    pub fn selected_snapshot(&self) -> Option<&SnapshotEntry> {
        self.snapshots.get(self.selected)
    }
}

/// Render the restore dialog.
pub fn render(f: &mut Frame, dialog: &RestoreDialog) {
    let area = centered_rect(60, 20, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Restore from Snapshot ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(8),   // snapshot list
            Constraint::Length(3), // details
            Constraint::Length(2), // buttons
        ])
        .margin(1)
        .split(inner);

    if dialog.snapshots.is_empty() {
        let msg = Paragraph::new("No snapshots available.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, chunks[0]);

        let buttons = Line::from(vec![Span::styled(
            "[Esc] Close",
            Style::default().fg(Color::Gray),
        )]);
        f.render_widget(Paragraph::new(buttons), chunks[3]);
        return;
    }

    // Header
    let header = Paragraph::new(Line::from(vec![Span::styled(
        format!("{} snapshot(s) available:", dialog.snapshots.len()),
        theme::help_key_style(),
    )]));
    f.render_widget(header, chunks[0]);

    // Snapshot list
    let items: Vec<ListItem> = dialog
        .snapshots
        .iter()
        .enumerate()
        .map(|(i, snap)| {
            let age = chrono::Utc::now()
                .signed_duration_since(snap.deleted_at)
                .num_hours();
            let age_str = if age < 1 {
                "< 1h ago".to_string()
            } else if age < 24 {
                format!("{}h ago", age)
            } else {
                format!("{}d ago", age / 24)
            };

            let style = if i == dialog.selected {
                theme::selected_style()
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    if i == dialog.selected { "> " } else { "  " },
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(&snap.name, style),
                Span::raw("  "),
                Span::styled(age_str, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(dialog.selected));
    let list = List::new(items);
    f.render_stateful_widget(list, chunks[1], &mut list_state);

    // Details for selected snapshot
    if let Some(snap) = dialog.selected_snapshot() {
        let details = vec![
            Line::from(vec![
                Span::styled("Base: ", Style::default().fg(Color::DarkGray)),
                Span::raw(&snap.base_branch),
                Span::raw(" ("),
                Span::raw(&snap.base_commit[..8.min(snap.base_commit.len())]),
                Span::raw(")"),
            ]),
            Line::from(vec![
                Span::styled("File: ", Style::default().fg(Color::DarkGray)),
                Span::raw(
                    snap.patch_file
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default(),
                ),
            ]),
        ];
        f.render_widget(Paragraph::new(details), chunks[2]);
    }

    // Buttons
    let buttons = Line::from(vec![
        Span::styled(
            " [Enter] Restore ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("[j/k] Navigate  ", Style::default().fg(Color::Gray)),
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
