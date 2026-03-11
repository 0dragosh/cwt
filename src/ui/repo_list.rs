use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::forest::index::RepoStats;
use crate::ui::theme;

/// A repo entry for display in the repo list panel.
pub struct RepoDisplay {
    pub name: String,
    pub path: String,
    pub stats: RepoStats,
}

/// Render the repo list panel (leftmost panel in forest mode).
pub fn render(
    f: &mut Frame,
    area: Rect,
    repos: &[RepoDisplay],
    list_state: &mut ListState,
    focused: bool,
) {
    let items: Vec<ListItem> = repos
        .iter()
        .map(|repo| {
            let running = repo.stats.running_sessions;
            let waiting = repo.stats.waiting_sessions;
            let wt_count = repo.stats.worktree_count;

            // Status summary spans
            let mut status_parts: Vec<String> = Vec::new();
            if running > 0 {
                status_parts.push(format!("{}R", running));
            }
            if waiting > 0 {
                status_parts.push(format!("{}W", waiting));
            }
            let status_summary = if status_parts.is_empty() {
                String::new()
            } else {
                format!(" [{}]", status_parts.join("/"))
            };

            let activity_icon = if running > 0 {
                Span::styled(theme::ICON_RUNNING, theme::status_running_style())
            } else if waiting > 0 {
                Span::styled(theme::ICON_WAITING, theme::status_waiting_style())
            } else {
                Span::styled(theme::ICON_IDLE, theme::status_idle_style())
            };

            let name_span = Span::styled(
                format!(" {}", repo.name),
                Style::default().add_modifier(Modifier::BOLD),
            );

            let count_span = Span::styled(
                format!(" ({})", wt_count),
                Style::default().fg(Color::DarkGray),
            );

            let status_span = if running > 0 {
                Span::styled(status_summary, Style::default().fg(Color::Green))
            } else if waiting > 0 {
                Span::styled(status_summary, Style::default().fg(Color::Yellow))
            } else {
                Span::raw(status_summary)
            };

            let line = Line::from(vec![
                Span::raw(" "),
                activity_icon,
                name_span,
                count_span,
                status_span,
            ]);

            ListItem::new(line)
        })
        .collect();

    let border_style = if focused {
        theme::title_style()
    } else {
        Style::default()
    };

    let title = format!(" Repos ({}) ", repos.len());

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_style());

    f.render_stateful_widget(list, area, list_state);
}
