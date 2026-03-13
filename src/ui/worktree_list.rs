use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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
    filter_mode: bool,
) {
    let filter_lower = filter.to_lowercase();
    let filtered: Vec<&Worktree> = if filter.is_empty() {
        worktrees.iter().collect()
    } else {
        worktrees
            .iter()
            .filter(|wt| wt.name.to_lowercase().contains(&filter_lower))
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
                WorktreeStatus::Done => Span::styled(theme::ICON_DONE, theme::status_done_style()),
                WorktreeStatus::Shipping => {
                    Span::styled(theme::ICON_SHIPPING, theme::status_shipping_style())
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

            let name_text = format!(" {}", wt.name);

            // Highlight matching portion of the name when filtering
            let name_span = if !filter.is_empty() {
                let name_lower = wt.name.to_lowercase();
                if let Some(pos) = name_lower.find(&filter_lower) {
                    // Use char-boundary-aware slicing to avoid panics on multi-byte chars
                    // Find the byte offset in the original string that corresponds to the
                    // character position found in the lowercased string
                    let byte_start = wt.name
                        .char_indices()
                        .nth(name_lower[..pos].chars().count())
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let filter_char_count = filter_lower.chars().count();
                    let byte_end = wt.name
                        .char_indices()
                        .nth(name_lower[..pos].chars().count() + filter_char_count)
                        .map(|(i, _)| i)
                        .unwrap_or(wt.name.len());
                    let pre = format!(" {}", &wt.name[..byte_start]);
                    let matched = &wt.name[byte_start..byte_end];
                    let post = &wt.name[byte_end..];

                    // Return multiple spans combined in a vec
                    vec![
                        Span::styled(
                            pre,
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            matched.to_string(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                        ),
                        Span::styled(
                            post.to_string(),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                    ]
                } else {
                    vec![Span::styled(
                        name_text,
                        Style::default().add_modifier(Modifier::BOLD),
                    )]
                }
            } else {
                vec![Span::styled(
                    name_text,
                    Style::default().add_modifier(Modifier::BOLD),
                )]
            };

            let branch = Span::styled(
                format!("  {}", wt.branch),
                Style::default().fg(Color::DarkGray),
            );

            let mut line_spans = vec![Span::raw(" "), status_icon, Span::raw(" "), lifecycle_icon];
            line_spans.extend(name_span);

            // Show remote host indicator if this is a remote worktree
            if let Some(ref host_name) = wt.remote_host {
                line_spans.push(Span::styled(
                    format!(" [{}]", host_name),
                    Style::default().fg(Color::Magenta),
                ));
            }

            line_spans.push(branch);

            // Add PR/CI status icons
            let pr_ci = crate::ui::dialogs::ship::pr_ci_spans(&wt.pr_status, &wt.ci_status);
            line_spans.extend(pr_ci);

            // Add container status icon if present
            if let Some(ref container) = wt.container {
                let ctr_icon = container.status.icon();
                if !ctr_icon.is_empty() {
                    let ctr_style = match container.status {
                        crate::env::container::ContainerStatus::Running => {
                            theme::status_running_style()
                        }
                        crate::env::container::ContainerStatus::Building => {
                            theme::status_waiting_style()
                        }
                        crate::env::container::ContainerStatus::Failed => theme::error_style(),
                        _ => theme::status_idle_style(),
                    };
                    line_spans.push(Span::raw(" "));
                    line_spans.push(Span::styled(ctr_icon, ctr_style));
                }
            }

            // Add port info if allocated
            if let Some(ref port_alloc) = wt.ports {
                if let Some(primary) = port_alloc.primary_port() {
                    line_spans.push(Span::styled(
                        format!(" :{}", primary),
                        Style::default().fg(Color::Cyan),
                    ));
                }
            }

            let line = Line::from(line_spans);

            ListItem::new(line)
        })
        .collect();

    let border_style = if focused {
        theme::title_style()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = if filter_mode {
        format!(" Worktrees ({}) [/{}|] ", filtered.len(), filter)
    } else if !filter.is_empty() {
        format!(" Worktrees ({}) [/{}] ", filtered.len(), filter)
    } else {
        format!(" Worktrees ({}) ", filtered.len())
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
