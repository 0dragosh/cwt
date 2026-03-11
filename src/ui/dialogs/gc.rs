use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::ui::theme;

/// State for the GC preview dialog.
#[derive(Debug, Clone)]
pub struct GcDialog {
    pub to_prune: Vec<String>,
    pub confirmed: bool,
    pub cancelled: bool,
}

impl GcDialog {
    pub fn new(to_prune: Vec<String>) -> Self {
        Self {
            to_prune,
            confirmed: false,
            cancelled: false,
        }
    }
}

/// Render the GC preview dialog.
pub fn render(f: &mut Frame, dialog: &GcDialog) {
    let area = centered_rect(50, 16, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Garbage Collection ")
        .borders(Borders::ALL)
        .border_style(theme::dialog_border_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(5),    // list
            Constraint::Length(2), // buttons
        ])
        .margin(1)
        .split(inner);

    if dialog.to_prune.is_empty() {
        let msg = Paragraph::new("Nothing to GC — ephemeral count is within limit.")
            .style(theme::success_style());
        f.render_widget(msg, chunks[0]);

        let buttons = Line::from(vec![Span::styled(
            "[Esc] Close",
            Style::default().fg(Color::Gray),
        )]);
        f.render_widget(Paragraph::new(buttons), chunks[2]);
        return;
    }

    // Header
    let header = Paragraph::new(Line::from(vec![Span::styled(
        format!(
            "{} ephemeral worktree(s) will be pruned:",
            dialog.to_prune.len()
        ),
        theme::help_key_style(),
    )]));
    f.render_widget(header, chunks[0]);

    // List of worktrees to prune
    let items: Vec<ListItem> = dialog
        .to_prune
        .iter()
        .map(|name| {
            ListItem::new(Line::from(vec![
                Span::styled("  - ", Style::default().fg(Color::Red)),
                Span::raw(name.as_str()),
            ]))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, chunks[1]);

    // Buttons
    let buttons = Line::from(vec![
        Span::styled(
            " [y] Prune ",
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
