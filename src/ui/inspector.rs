use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::ui::theme;
use crate::worktree::model::{Lifecycle, Worktree, WorktreeStatus};

/// Info to display in the inspector panel.
#[derive(Debug, Clone, Default)]
pub struct InspectorInfo {
    pub diff_stat_text: String,
    pub last_message: String,
}

/// Render the inspector panel showing details of the selected worktree.
pub fn render(
    f: &mut Frame,
    area: Rect,
    worktree: Option<&Worktree>,
    info: &InspectorInfo,
    focused: bool,
) {
    let border_style = if focused {
        theme::title_style()
    } else {
        Style::default()
    };

    let Some(wt) = worktree else {
        let block = Block::default()
            .title(" Inspector ")
            .borders(Borders::ALL)
            .border_style(border_style);
        let paragraph = Paragraph::new("No worktree selected")
            .block(block)
            .style(Style::default().fg(ratatui::style::Color::DarkGray));
        f.render_widget(paragraph, area);
        return;
    };

    let block = Block::default()
        .title(format!(" {} ", wt.name))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(8),  // metadata
            Constraint::Length(1),  // separator
            Constraint::Min(5),    // diff stat / last message
        ])
        .split(inner);

    // Metadata section
    let status_str = match wt.status {
        WorktreeStatus::Idle => "idle",
        WorktreeStatus::Running => "running",
        WorktreeStatus::Waiting => "waiting",
        WorktreeStatus::Done => "done",
    };

    let lifecycle_str = match wt.lifecycle {
        Lifecycle::Ephemeral => "ephemeral",
        Lifecycle::Permanent => "permanent",
    };

    let created = wt.created_at.format("%Y-%m-%d %H:%M");

    let meta_lines = vec![
        Line::from(vec![
            Span::styled("Branch:    ", theme::help_key_style()),
            Span::raw(&wt.branch),
        ]),
        Line::from(vec![
            Span::styled("Base:      ", theme::help_key_style()),
            Span::raw(format!(
                "{} ({})",
                &wt.base_branch,
                &wt.base_commit[..8.min(wt.base_commit.len())]
            )),
        ]),
        Line::from(vec![
            Span::styled("Status:    ", theme::help_key_style()),
            Span::raw(status_str),
        ]),
        Line::from(vec![
            Span::styled("Lifecycle: ", theme::help_key_style()),
            Span::raw(lifecycle_str),
        ]),
        Line::from(vec![
            Span::styled("Created:   ", theme::help_key_style()),
            Span::raw(created.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Path:      ", theme::help_key_style()),
            Span::raw(wt.path.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled("Pane:      ", theme::help_key_style()),
            Span::raw(wt.tmux_pane.as_deref().unwrap_or("none")),
        ]),
    ];

    let meta = Paragraph::new(meta_lines);
    f.render_widget(meta, chunks[0]);

    // Separator
    let sep = Paragraph::new("─".repeat(chunks[1].width as usize))
        .style(Style::default().fg(ratatui::style::Color::DarkGray));
    f.render_widget(sep, chunks[1]);

    // Diff stat + last message section
    let mut detail_lines = Vec::new();

    if !info.diff_stat_text.is_empty() {
        detail_lines.push(Line::from(Span::styled(
            "Changes:",
            theme::help_key_style(),
        )));
        for line in info.diff_stat_text.lines() {
            let style = if line.contains('+') && line.contains('-') {
                Style::default()
            } else if line.contains('+') {
                theme::diff_add_style()
            } else if line.contains('-') {
                theme::diff_remove_style()
            } else {
                Style::default()
            };
            detail_lines.push(Line::from(Span::styled(line.to_string(), style)));
        }
    }

    if !info.last_message.is_empty() {
        detail_lines.push(Line::default());
        detail_lines.push(Line::from(Span::styled(
            "Last message:",
            theme::help_key_style(),
        )));
        for line in info.last_message.lines().take(10) {
            detail_lines.push(Line::from(line.to_string()));
        }
    }

    if detail_lines.is_empty() {
        detail_lines.push(Line::from(Span::styled(
            "No changes",
            Style::default().fg(ratatui::style::Color::DarkGray),
        )));
    }

    let details = Paragraph::new(detail_lines).wrap(Wrap { trim: false });
    f.render_widget(details, chunks[2]);
}
