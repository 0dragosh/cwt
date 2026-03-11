use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::ui::theme;
use crate::worktree::model::{Lifecycle, Worktree, WorktreeStatus};

/// Render the worktree list panel.
pub fn render(
    f: &mut Frame,
    area: Rect,
    worktrees: &[Worktree],
    list_state: &mut ListState,
    focused: bool,
    filter: &str,
) {
    let filtered: Vec<&Worktree> = if filter.is_empty() {
        worktrees.iter().collect()
    } else {
        worktrees
            .iter()
            .filter(|wt| wt.name.contains(filter))
            .collect()
    };

    let items: Vec<ListItem> = filtered
        .iter()
        .map(|wt| {
            let status_icon = match wt.status {
                WorktreeStatus::Idle => Span::styled(theme::ICON_IDLE, theme::status_idle_style()),
                WorktreeStatus::Running => {
                    Span::styled(theme::ICON_RUNNING, theme::status_running_style())
                }
                WorktreeStatus::Waiting => {
                    Span::styled(theme::ICON_WAITING, theme::status_waiting_style())
                }
                WorktreeStatus::Done => {
                    Span::styled(theme::ICON_DONE, theme::status_done_style())
                }
            };

            let lifecycle_icon = match wt.lifecycle {
                Lifecycle::Ephemeral => {
                    Span::styled(theme::ICON_EPHEMERAL, theme::ephemeral_style())
                }
                Lifecycle::Permanent => {
                    Span::styled(theme::ICON_PERMANENT, theme::permanent_style())
                }
            };

            let name = Span::styled(
                format!(" {}", wt.name),
                Style::default().add_modifier(Modifier::BOLD),
            );

            let branch = Span::styled(
                format!("  {}", wt.branch),
                Style::default().fg(ratatui::style::Color::DarkGray),
            );

            let line = Line::from(vec![
                Span::raw(" "),
                status_icon,
                Span::raw(" "),
                lifecycle_icon,
                name,
                branch,
            ]);

            ListItem::new(line)
        })
        .collect();

    let border_style = if focused {
        theme::title_style()
    } else {
        Style::default()
    };

    let title = if filter.is_empty() {
        format!(" Worktrees ({}) ", filtered.len())
    } else {
        format!(" Worktrees ({}) [/{}] ", filtered.len(), filter)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_style());

    f.render_stateful_widget(list, area, list_state);
}
