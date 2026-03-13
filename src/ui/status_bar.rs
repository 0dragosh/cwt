use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const SHORTCUTS: &str =
    " {count} worktree(s) | n:new Ent:session h:handoff P:pr S:ship e:shell p:perm d:del g:gc r:restore t:tasks b:bcast m:mode ?:help q:quit";

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
    let mut spans = vec![
        Span::styled(
            " cwt ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(SHORTCUTS.replace("{count}", &worktree_count.to_string())),
    ];

    // Keep remote status indicators visible without displacing the shortcut strip.
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

    if !message.is_empty() {
        spans.push(Span::raw(" | "));
        spans.push(Span::raw(message));
    }

    let line = Line::from(spans);

    let bar = Paragraph::new(line).style(Style::default().bg(Color::DarkGray));
    f.render_widget(bar, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_line(message: &str) -> String {
        let backend = TestBackend::new(240, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| render(frame, frame.area(), message, 3))
            .expect("render status bar");

        let buffer = terminal.backend().buffer();
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, 0)].symbol());
        }
        line
    }

    #[test]
    fn keeps_shortcuts_visible_when_showing_a_status_message() {
        let line = render_line("Sync in progress");

        assert!(line.contains("h:handoff"));
        assert!(line.contains("P:pr"));
        assert!(line.contains("Sync in progress"));
    }
}
