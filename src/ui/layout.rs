use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::path::Path;

/// Three-row layout: top bar + two-panel content + status bar.
/// Returns (top_bar, list_panel, inspector_panel, status_bar).
pub fn main_layout(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // top bar
            Constraint::Min(3),    // main content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(outer[1]);

    (outer[0], panels[0], panels[1], outer[2])
}

/// Three-row layout for forest mode: top bar + three-panel content + status bar.
/// Returns (top_bar, repo_panel, worktree_panel, inspector_panel, status_bar).
pub fn forest_layout(area: Rect) -> (Rect, Rect, Rect, Rect, Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // top bar
            Constraint::Min(3),    // main content
            Constraint::Length(1), // status bar
        ])
        .split(area);

    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),  // repos
            Constraint::Percentage(35),  // worktrees
            Constraint::Percentage(45),  // inspector
        ])
        .split(outer[1]);

    (outer[0], panels[0], panels[1], panels[2], outer[2])
}

/// Render the top bar with project name, notification badges, and aggregate stats.
pub fn render_top_bar(
    f: &mut Frame,
    area: Rect,
    repo_root: &Path,
    waiting_count: usize,
    done_count: usize,
) {
    render_top_bar_with_stats(f, area, repo_root, waiting_count, done_count, None, None);
}

/// Render the top bar with optional aggregate token/cost stats.
pub fn render_top_bar_with_stats(
    f: &mut Frame,
    area: Rect,
    repo_root: &Path,
    waiting_count: usize,
    done_count: usize,
    total_tokens: Option<(u64, u64)>,
    total_cost: Option<f64>,
) {
    let project_name = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string());

    let mut spans = vec![
        Span::styled(
            " cwt ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            &project_name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({})", repo_root.display()),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    // Add notification badges
    if waiting_count > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} waiting ", waiting_count),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if done_count > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} done ", done_count),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Aggregate token/cost stats
    if let Some((input_t, output_t)) = total_tokens {
        if input_t > 0 || output_t > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!(
                    " {}in/{}out ",
                    format_top_bar_tokens(input_t),
                    format_top_bar_tokens(output_t),
                ),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    if let Some(cost) = total_cost {
        if cost > 0.0 {
            spans.push(Span::styled(
                format!(" ${:.2} ", cost),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green),
            ));
        }
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(bar, area);
}

/// Format token count compactly for the top bar.
fn format_top_bar_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
    }
}

/// Render the forest mode top bar with global dashboard stats.
pub fn render_forest_top_bar(
    f: &mut Frame,
    area: Rect,
    selected_repo_name: Option<&str>,
    total_repos: usize,
    total_running: usize,
    total_waiting: usize,
    total_done: usize,
) {
    let mut spans = vec![
        Span::styled(
            " cwt forest ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(name) = selected_repo_name {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            name,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Global dashboard summary
    let mut summary_parts: Vec<String> = Vec::new();
    if total_running > 0 {
        summary_parts.push(format!("{} running", total_running));
    }
    if total_waiting > 0 {
        summary_parts.push(format!("{} waiting", total_waiting));
    }
    if total_done > 0 {
        summary_parts.push(format!("{} done", total_done));
    }

    let summary = if summary_parts.is_empty() {
        format!("  {} repos, no active sessions", total_repos)
    } else {
        format!("  {} across {} repos", summary_parts.join(", "), total_repos)
    };

    spans.push(Span::styled(
        summary,
        Style::default().fg(Color::DarkGray),
    ));

    // Notification badges
    if total_waiting > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} waiting ", total_waiting),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if total_done > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} done ", total_done),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(bar, area);
}
