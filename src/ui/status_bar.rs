use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Render the status bar at the bottom.
pub fn render(f: &mut Frame, area: Rect, message: &str, worktree_count: usize) {
    let line = if message.is_empty() {
        Line::from(vec![
            Span::styled(
                " cwt ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " {} worktree(s) | n:new s:session P:pr S:ship d:delete c:ci ?:help q:quit",
                worktree_count
            )),
        ])
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
