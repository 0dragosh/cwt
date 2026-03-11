use ratatui::style::{Color, Modifier, Style};

/// Status icons for worktree states.
pub const ICON_IDLE: &str = "○";
pub const ICON_RUNNING: &str = "●";
pub const ICON_WAITING: &str = "◑";
pub const ICON_DONE: &str = "✓";
pub const ICON_SHIPPING: &str = "^";
pub const ICON_EPHEMERAL: &str = "ε";
pub const ICON_PERMANENT: &str = "π";

/// Colors used throughout the TUI.
pub fn title_style() -> Style {
    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

pub fn status_running_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn status_waiting_style() -> Style {
    Style::default().fg(Color::Yellow)
}

pub fn status_done_style() -> Style {
    Style::default().fg(Color::Blue)
}

pub fn status_shipping_style() -> Style {
    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
}

pub fn status_idle_style() -> Style {
    Style::default().fg(Color::Gray)
}

pub fn ephemeral_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn permanent_style() -> Style {
    Style::default().fg(Color::Magenta)
}

pub fn diff_add_style() -> Style {
    Style::default().fg(Color::Green)
}

pub fn diff_remove_style() -> Style {
    Style::default().fg(Color::Red)
}

pub fn help_key_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

pub fn help_desc_style() -> Style {
    Style::default().fg(Color::Gray)
}

pub fn dialog_border_style() -> Style {
    Style::default().fg(Color::Cyan)
}

pub fn error_style() -> Style {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
}

pub fn success_style() -> Style {
    Style::default().fg(Color::Green)
}
