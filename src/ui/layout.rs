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

/// Render the top bar with project name and notification badges.
pub fn render_top_bar(
    f: &mut Frame,
    area: Rect,
    repo_root: &Path,
    waiting_count: usize,
    done_count: usize,
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

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(bar, area);
}
