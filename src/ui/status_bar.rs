use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Render the status bar at the bottom.
pub fn render(f: &mut Frame, area: Rect, message: &str, worktree_count: usize) {
    render_with_remotes(f, area, message, worktree_count, &[]);
}

/// Render the status bar with optional remote host status indicators.
pub fn render_with_remotes(
    f: &mut Frame,
    area: Rect,
    message: &str,
    worktree_count: usize,
    remote_statuses: &[crate::remote::host::RemoteHostStatus],
) {
    let line = if message.is_empty() {
        let mut spans = vec![
            Span::styled(
                " cwt ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " {} worktree(s) | n:new Enter:session e:shell h:handoff d:delete ?:help q:quit",
                worktree_count
            )),
        ];

        // Add remote host status indicators
        if !remote_statuses.is_empty() {
            spans.push(Span::raw("  "));
            for status in remote_statuses {
                let (icon_style, icon_text) = match &status.network {
                    crate::remote::host::NetworkStatus::Connected(d) => (
                        Style::default().fg(Color::Green),
                        format!(" {} {}ms ", status.name, d.as_millis()),
                    ),
                    crate::remote::host::NetworkStatus::Disconnected => (
                        Style::default().fg(Color::Red),
                        format!(" {} !! ", status.name),
                    ),
                    crate::remote::host::NetworkStatus::Unknown => (
                        Style::default().fg(Color::DarkGray),
                        format!(" {} ?? ", status.name),
                    ),
                };
                spans.push(Span::styled(icon_text, icon_style));
            }
        }

        Line::from(spans)
    } else {
        Line::from(vec![
            Span::styled(
                " cwt ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(message),
        ])
    };

    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(bar, area);
}
