use chrono::Utc;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::session::transcript::TranscriptUsage;
use crate::ui::theme;
use crate::worktree::model::{Lifecycle, Worktree, WorktreeStatus};

/// Info to display in the inspector panel.
#[derive(Debug, Clone, Default)]
pub struct InspectorInfo {
    pub diff_stat_text: String,
    pub last_message: String,
    pub usage: TranscriptUsage,
    pub session_id: Option<String>,
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

    // Calculate dynamic metadata height: base 7 lines + optional session/usage lines
    let mut meta_height: u16 = 7;
    if wt.tmux_pane.is_some() {
        meta_height += 1; // pane line
    }
    if info.usage.input_tokens > 0 || info.usage.output_tokens > 0 {
        meta_height += 1; // tokens line
    }
    if info.usage.total_cost_usd.is_some() {
        meta_height += 1; // cost line
    }

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(meta_height),
            Constraint::Length(1),  // separator
            Constraint::Min(5),    // diff stat / last message
        ])
        .split(inner);

    // Metadata section
    let (status_str, status_style) = format_status(wt);

    let lifecycle_str = match wt.lifecycle {
        Lifecycle::Ephemeral => "ephemeral",
        Lifecycle::Permanent => "permanent",
    };

    let age = format_age(wt.created_at);

    let mut meta_lines = vec![
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
            Span::styled(status_str, status_style),
        ]),
        Line::from(vec![
            Span::styled("Lifecycle: ", theme::help_key_style()),
            Span::raw(lifecycle_str),
        ]),
        Line::from(vec![
            Span::styled("Created:   ", theme::help_key_style()),
            Span::raw(format!("{} ({})", wt.created_at.format("%Y-%m-%d %H:%M"), age)),
        ]),
        Line::from(vec![
            Span::styled("Path:      ", theme::help_key_style()),
            Span::raw(wt.path.display().to_string()),
        ]),
    ];

    // Show pane info if a session has been launched
    if let Some(ref pane_id) = wt.tmux_pane {
        let session_label = if let Some(ref sid) = info.session_id {
            format!("{} (session: {})", pane_id, truncate_str(sid, 16))
        } else {
            pane_id.clone()
        };
        meta_lines.push(Line::from(vec![
            Span::styled("Session:   ", theme::help_key_style()),
            Span::raw(session_label),
        ]));
    }

    // Show token usage if available
    if info.usage.input_tokens > 0 || info.usage.output_tokens > 0 {
        meta_lines.push(Line::from(vec![
            Span::styled("Tokens:    ", theme::help_key_style()),
            Span::raw(format!(
                "{}in / {}out ({} msgs)",
                format_tokens(info.usage.input_tokens),
                format_tokens(info.usage.output_tokens),
                info.usage.message_count,
            )),
        ]));
    }

    if let Some(cost) = info.usage.total_cost_usd {
        meta_lines.push(Line::from(vec![
            Span::styled("Cost:      ", theme::help_key_style()),
            Span::raw(format!("${:.4}", cost)),
        ]));
    }

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

/// Format session status with contextual information.
fn format_status(wt: &Worktree) -> (String, Style) {
    let now = Utc::now();
    let age = now.signed_duration_since(wt.created_at);

    match wt.status {
        WorktreeStatus::Idle => ("idle".to_string(), theme::status_idle_style()),
        WorktreeStatus::Running => {
            let duration = format_duration(age);
            (format!("running ({})", duration), theme::status_running_style())
        }
        WorktreeStatus::Waiting => (
            "waiting for input".to_string(),
            theme::status_waiting_style(),
        ),
        WorktreeStatus::Done => ("done".to_string(), theme::status_done_style()),
    }
}

/// Format a chrono duration into a human-readable string.
fn format_age(created: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(created);
    format_duration(duration)
}

/// Format a duration into a compact string like "5m", "2h", "3d".
fn format_duration(duration: chrono::TimeDelta) -> String {
    let secs = duration.num_seconds();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{}h", h)
        } else {
            format!("{}h{}m", h, m)
        }
    } else {
        let d = secs / 86400;
        format!("{}d", d)
    }
}

/// Format token counts with K/M suffixes.
fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M ", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K ", count as f64 / 1_000.0)
    } else {
        format!("{} ", count)
    }
}

/// Truncate a string to max_len, appending ".." if truncated.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
