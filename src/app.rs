use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::model::PermissionLevel;
use crate::config::{self, ConfigMeta};
use crate::env;
use crate::git;
use crate::hooks::event::HookEvent;
use crate::orchestration;
use crate::remote;
use crate::session;
use crate::session::provider::SessionProvider;
use crate::ship;
use crate::ui;
use crate::worktree::handoff::{self, HandoffDirection};
use crate::worktree::model::{Worktree, WorktreeStatus};
use crate::worktree::Manager;

use crate::forest;
use crate::forest::index::RepoStats;

/// Which dialog is currently active.
#[derive(Debug, Clone)]
pub enum ActiveDialog {
    None,
    Create(ui::dialogs::create::CreateDialog),
    Delete(ui::dialogs::delete::DeleteDialog),
    Handoff(ui::dialogs::handoff::HandoffDialog),
    Gc(ui::dialogs::gc::GcDialog),
    Restore(ui::dialogs::restore::RestoreDialog),
    Dispatch(ui::dialogs::dispatch::DispatchDialog),
    Broadcast(ui::dialogs::broadcast::BroadcastDialog),
    Ship(ui::dialogs::ship::ShipDialog),
    Help,
}

/// Which panel has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    WorktreeList,
    Inspector,
}

fn clamp_selected_index(current: Option<usize>, count: usize) -> Option<usize> {
    if count == 0 {
        None
    } else {
        match current {
            Some(i) if i < count => Some(i),
            Some(_) => Some(count - 1),
            None => Some(0),
        }
    }
}

fn should_process_key_event(key: &KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn parse_context_percent_from_message(message: &str) -> Option<u8> {
    let lowered = message.to_ascii_lowercase();
    if !lowered.contains("context") {
        return None;
    }

    let bytes = message.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx].is_ascii_digit() {
            let start = idx;
            while idx < bytes.len() && bytes[idx].is_ascii_digit() {
                idx += 1;
            }
            if idx < bytes.len() && bytes[idx] == b'%' {
                let value = message[start..idx].parse::<u16>().ok()?;
                return Some(value.min(100) as u8);
            }
            continue;
        }
        idx += 1;
    }
    None
}

fn should_ignore_delete_shortcut(suppress_delete_until: &mut Option<Instant>) -> bool {
    let Some(until) = *suppress_delete_until else {
        return false;
    };

    if Instant::now() <= until {
        true
    } else {
        *suppress_delete_until = None;
        false
    }
}

fn arm_post_create_delete_guard(
    suppress_delete_until: &mut Option<Instant>,
    awaiting_focus_return: &mut bool,
) {
    *suppress_delete_until = Some(Instant::now() + Duration::from_secs(2));
    *awaiting_focus_return = true;
}

fn refresh_post_create_delete_guard_on_focus_return(
    suppress_delete_until: &mut Option<Instant>,
    awaiting_focus_return: &mut bool,
) -> bool {
    if *awaiting_focus_return {
        *suppress_delete_until = Some(Instant::now() + Duration::from_secs(2));
        *awaiting_focus_return = false;
        true
    } else {
        false
    }
}

fn capture_last_session_id(manager: &Manager, wt: &mut Worktree) {
    if wt.last_session_id.is_some() {
        return;
    }

    let wt_abs = manager.worktree_abs_path(wt);
    if let Ok(Some(dir)) = session::tracker::find_project_dir(&wt_abs) {
        if let Ok(Some(sid)) = session::tracker::find_latest_session_id(&dir) {
            wt.last_session_id = Some(sid);
        }
    }
}

fn mark_session_done(manager: &Manager, wt: &mut Worktree) {
    capture_last_session_id(manager, wt);
    wt.status = WorktreeStatus::Done;
    wt.tmux_pane = None;
}

fn drain_pending_terminal_events() -> Result<usize> {
    drain_pending_terminal_events_with(|| event::poll(Duration::from_millis(0)), event::read)
        .map_err(Into::into)
}

fn drain_pending_terminal_events_with<E>(
    mut poll: impl FnMut() -> std::result::Result<bool, E>,
    mut read: impl FnMut() -> std::result::Result<Event, E>,
) -> std::result::Result<usize, E> {
    let mut drained = 0;
    while poll()? {
        let _ = read()?;
        drained += 1;
    }
    Ok(drained)
}

/// Top-level application state.
pub struct App {
    pub manager: Manager,
    pub worktrees: Vec<Worktree>,
    pub list_state: ListState,
    pub focus: FocusPanel,
    pub dialog: ActiveDialog,
    pub filter: String,
    pub filter_mode: bool,
    pub status_message: String,
    pub inspector_info: ui::inspector::InspectorInfo,
    pub inspector_scroll: u16,
    pub should_quit: bool,
    /// Count of worktrees in "waiting" state (for notification badge).
    pub waiting_count: usize,
    /// Count of worktrees in "done" state that haven't been acknowledged.
    pub done_count: usize,
    /// Aggregate dashboard stats across all sessions.
    pub dashboard: orchestration::dashboard::AggregateStats,
    /// Track the areas for mouse click handling.
    pub last_list_area: Option<ratatui::layout::Rect>,
    /// Port manager for allocating non-conflicting ports per worktree.
    pub port_manager: env::ports::PortManager,
    /// Resource warnings from the last check.
    pub resource_warnings: Vec<env::resources::ResourceWarning>,
    /// Cached status of remote hosts (updated periodically, not every tick).
    pub remote_statuses: Vec<remote::host::RemoteHostStatus>,
    /// Scroll offset for the help overlay.
    pub help_scroll: u16,
    /// Runtime override for the permission level (None = use config default).
    pub permission_override: Option<PermissionLevel>,
    /// Runtime override for session provider (None = use config default).
    pub provider_override: Option<SessionProvider>,
    /// Metadata about the loaded config file.
    pub config_meta: ConfigMeta,
    /// Ignore a stray delete shortcut immediately after a successful create.
    pub suppress_delete_until: Option<Instant>,
    /// Re-arm the post-create delete guard if focus returns after zellij/tab switching.
    pub awaiting_focus_return_after_create: bool,
}

impl App {
    pub fn new(manager: Manager, config_meta: ConfigMeta) -> Result<Self> {
        let worktrees = manager.list()?;
        let mut list_state = ListState::default();
        if !worktrees.is_empty() {
            list_state.select(Some(0));
        }

        // Rebuild port manager from existing worktree port allocations
        let existing_ports: Vec<env::ports::PortAllocation> =
            worktrees.iter().filter_map(|wt| wt.ports.clone()).collect();
        let port_manager = if existing_ports.is_empty() {
            env::ports::PortManager::new(
                manager.config.container.app_base_port,
                manager.config.container.db_base_port,
            )
        } else {
            env::ports::PortManager::from_existing(existing_ports)
        };

        // Initialize remote host statuses as unknown (will be checked periodically)
        let remote_statuses: Vec<remote::host::RemoteHostStatus> = manager
            .config
            .remote
            .iter()
            .map(|h| remote::host::RemoteHostStatus::unknown(&h.name))
            .collect();

        let mut app = Self {
            manager,
            worktrees,
            list_state,
            focus: FocusPanel::WorktreeList,
            dialog: ActiveDialog::None,
            filter: String::new(),
            filter_mode: false,
            status_message: String::new(),
            inspector_info: ui::inspector::InspectorInfo::default(),
            inspector_scroll: 0,
            should_quit: false,
            waiting_count: 0,
            done_count: 0,
            dashboard: orchestration::dashboard::AggregateStats::default(),
            last_list_area: None,
            port_manager,
            resource_warnings: Vec::new(),
            remote_statuses,
            help_scroll: 0,
            permission_override: None,
            provider_override: None,
            config_meta,
            suppress_delete_until: None,
            awaiting_focus_return_after_create: false,
        };

        app.update_inspector();
        app.update_badge_counts();
        app.update_dashboard();
        Ok(app)
    }

    /// Refresh worktree list and update session statuses.
    pub fn refresh(&mut self) {
        if let Ok(worktrees) = self.manager.list() {
            let mut updated = worktrees;
            for wt in &mut updated {
                // Don't override "Shipping" status — it's managed by the ship pipeline
                if wt.status == WorktreeStatus::Shipping {
                    continue;
                }

                // Skip local session tracking for remote worktrees
                // (remote statuses are polled separately via poll_remote_statuses)
                if wt.is_remote() {
                    continue;
                }

                let new_status = session::tracker::check_status(wt.tmux_pane.as_deref());

                // If session just finished (was Running, now Done), clear the pane
                // but preserve last_session_id for potential resume
                if wt.status == WorktreeStatus::Running && new_status == WorktreeStatus::Done {
                    mark_session_done(&self.manager, wt);
                    continue;
                }

                wt.status = new_status;
            }
            self.worktrees = updated;

            // Reconcile port allocations: release ports for worktrees that no longer exist
            let current_names: std::collections::HashSet<&str> =
                self.worktrees.iter().map(|wt| wt.name.as_str()).collect();
            let stale_ports: Vec<String> = self
                .port_manager
                .allocations()
                .keys()
                .filter(|name| !current_names.contains(name.as_str()))
                .cloned()
                .collect();
            for name in stale_ports {
                self.port_manager.release(&name);
            }

            // Persist status changes (best-effort)
            if let Ok(mut state) = self.manager.load_state() {
                for wt in &self.worktrees {
                    if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                        stored.status = wt.status.clone();
                        stored.tmux_pane = wt.tmux_pane.clone();
                        if wt.last_session_id.is_some() {
                            stored.last_session_id = wt.last_session_id.clone();
                        }
                    }
                }
                let _ = self.manager.save_state(&state);
            }

            self.update_badge_counts();
        }
    }

    /// Update the notification badge counts.
    fn update_badge_counts(&mut self) {
        self.waiting_count = self
            .worktrees
            .iter()
            .filter(|wt| wt.status == WorktreeStatus::Waiting)
            .count();
        self.done_count = self
            .worktrees
            .iter()
            .filter(|wt| wt.status == WorktreeStatus::Done)
            .count();
    }

    /// Get the active permission level (runtime override or config default).
    fn active_permission(&self) -> PermissionLevel {
        self.permission_override
            .unwrap_or(self.manager.config.session.default_permission)
    }

    fn active_provider(&self) -> SessionProvider {
        self.provider_override
            .unwrap_or(self.manager.config.session.provider)
    }

    /// Update aggregate dashboard stats across all sessions.
    pub fn update_dashboard(&mut self) {
        let manager = &self.manager;
        self.dashboard = orchestration::dashboard::compute_aggregate_stats(&self.worktrees, |wt| {
            manager.worktree_abs_path(wt)
        });
    }

    /// Handle a hook event received from the Unix socket.
    pub fn handle_hook_event(&mut self, event: HookEvent) {
        match event {
            HookEvent::WorktreeCreated { worktree, .. } => {
                self.status_message = format!("Worktree '{}' created externally", worktree);
                self.refresh();
                self.update_inspector();
            }
            HookEvent::WorktreeRemoved { worktree, .. } => {
                self.status_message = format!("Worktree '{}' removed externally", worktree);
                self.refresh();
                self.clamp_selection();
                self.update_inspector();
            }
            HookEvent::SessionStopped {
                worktree,
                session_id,
                ..
            } => {
                // Update the worktree status to Done
                if let Some(wt) = self.worktrees.iter_mut().find(|wt| wt.name == worktree) {
                    wt.status = WorktreeStatus::Done;
                    wt.tmux_pane = None;
                    if let Some(ref sid) = session_id {
                        wt.last_session_id = Some(sid.clone());
                    }
                }
                self.status_message = format!("Session stopped for '{}'", worktree);
                self.update_badge_counts();
                self.update_inspector();

                // Persist status change
                self.persist_session_stop(&worktree, session_id.as_deref());
            }
            HookEvent::SessionNotification {
                worktree,
                message,
                context_usage_percent,
                ..
            } => {
                let context_percent = context_usage_percent
                    .or_else(|| message.as_deref().and_then(parse_context_percent_from_message));
                // Update the worktree status to Waiting
                if let Some(wt) = self.worktrees.iter_mut().find(|wt| wt.name == worktree) {
                    wt.status = WorktreeStatus::Waiting;
                    if let Some(pct) = context_percent {
                        wt.context_usage_percent = Some(pct);
                    }
                }
                let msg = message.unwrap_or_default();
                self.status_message = if msg.is_empty() {
                    format!("'{}' is waiting for input", worktree)
                } else {
                    format!("'{}': {}", worktree, msg)
                };
                self.update_badge_counts();
                self.update_inspector();

                // Persist status change
                self.persist_status_change(&worktree, WorktreeStatus::Waiting);
            }
            HookEvent::SubagentStopped { worktree, .. } => {
                self.status_message = format!("Subagent stopped in '{}'", worktree);
                // Refresh to pick up any state changes
                self.refresh();
                self.update_inspector();
            }
        }
    }

    /// Persist a status change for a worktree to state.json (best-effort).
    fn persist_status_change(&self, worktree_name: &str, status: WorktreeStatus) {
        if let Ok(mut state) = self.manager.load_state() {
            if let Some(stored) = state.worktrees.get_mut(worktree_name) {
                stored.status = status;
            }
            let _ = self.manager.save_state(&state);
        }
    }

    fn persist_session_stop(&self, worktree_name: &str, session_id: Option<&str>) {
        if let Ok(mut state) = self.manager.load_state() {
            if let Some(stored) = state.worktrees.get_mut(worktree_name) {
                stored.status = WorktreeStatus::Done;
                stored.tmux_pane = None;
                if let Some(sid) = session_id {
                    stored.last_session_id = Some(sid.to_string());
                }
            }
            let _ = self.manager.save_state(&state);
        }
    }

    /// Get the currently selected worktree.
    pub fn selected_worktree(&self) -> Option<&Worktree> {
        let filtered = self.filtered_worktrees();
        self.list_state
            .selected()
            .and_then(|i| filtered.get(i).copied())
    }

    /// Get filtered worktree list.
    fn filtered_worktrees(&self) -> Vec<&Worktree> {
        if self.filter.is_empty() {
            self.worktrees.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.worktrees
                .iter()
                .filter(|wt| wt.name.to_lowercase().contains(&filter_lower))
                .collect()
        }
    }

    /// Update inspector info for the currently selected worktree.
    pub fn update_inspector(&mut self) {
        let info = if let Some(wt) = self.selected_worktree() {
            if wt.is_remote() {
                // Remote worktree: get diff stat from remote host
                self.build_remote_inspector_info(wt)
            } else {
                // Local worktree
                let wt_abs = self.manager.worktree_abs_path(wt);
                let diff_stat_text = git::diff::diff_stat(&wt_abs)
                    .map(|s| s.raw)
                    .unwrap_or_default();

                let project_dir = session::tracker::find_project_dir(&wt_abs).ok().flatten();

                let transcript_info = project_dir
                    .as_ref()
                    .and_then(|dir| session::transcript::read_transcript_info(dir, 1).ok())
                    .unwrap_or_default();

                let session_id = project_dir
                    .as_ref()
                    .and_then(|dir| session::tracker::find_latest_session_id(dir).ok().flatten());

                ui::inspector::InspectorInfo {
                    diff_stat_text,
                    last_message: transcript_info.last_message,
                    usage: transcript_info.usage,
                    session_id,
                }
            }
        } else {
            ui::inspector::InspectorInfo::default()
        };

        self.inspector_info = info;
        // Reset scroll when changing worktree
        self.inspector_scroll = 0;
    }

    /// Build inspector info for a remote worktree.
    /// This avoids blocking the UI by returning cached/minimal info.
    fn build_remote_inspector_info(&self, wt: &Worktree) -> ui::inspector::InspectorInfo {
        let remote_name = match wt.remote_host {
            Some(ref name) => name,
            None => return ui::inspector::InspectorInfo::default(),
        };

        let host = match self
            .manager
            .config
            .remote
            .iter()
            .find(|r| r.name == *remote_name)
        {
            Some(h) => h,
            None => return ui::inspector::InspectorInfo::default(),
        };

        let repo_name = remote::sync::repo_name_from_path(&self.manager.repo_root);

        // Try to get remote diff stat (may be slow over network)
        let diff_stat_text = host
            .diff_stat(&repo_name, &wt.name)
            .unwrap_or_else(|_| "(remote -- unable to fetch diff)".to_string());

        ui::inspector::InspectorInfo {
            diff_stat_text,
            last_message: "(remote session -- transcript not available locally)".to_string(),
            usage: Default::default(),
            session_id: None,
        }
    }

    /// Render the full UI.
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let (top_bar_area, list_area, inspector_area, status_area) =
            ui::layout::main_layout(frame.area());

        // Store list area for mouse click handling
        self.last_list_area = Some(list_area);

        // Render the top bar with notification badges and aggregate stats
        let total_tokens =
            if self.dashboard.total_input_tokens > 0 || self.dashboard.total_output_tokens > 0 {
                Some((
                    self.dashboard.total_input_tokens,
                    self.dashboard.total_output_tokens,
                ))
            } else {
                None
            };
        ui::layout::render_top_bar_with_stats(
            frame,
            top_bar_area,
            &self.manager.repo_root,
            self.waiting_count,
            self.done_count,
            total_tokens,
            self.dashboard.total_cost_usd,
            self.active_permission(),
            self.active_provider(),
        );

        // Render the worktree list
        ui::worktree_list::render(
            frame,
            list_area,
            &self.worktrees,
            &mut self.list_state,
            self.focus == FocusPanel::WorktreeList,
            &self.filter,
            self.filter_mode,
        );

        // Render the inspector
        ui::inspector::render(
            frame,
            inspector_area,
            self.selected_worktree(),
            &self.inspector_info,
            self.focus == FocusPanel::Inspector,
            self.inspector_scroll,
        );

        // Render the status bar (with remote host indicators)
        ui::status_bar::render_with_remotes(
            frame,
            status_area,
            &self.status_message,
            self.worktrees.len(),
            &self.remote_statuses,
            self.selected_worktree()
                .and_then(|wt| wt.context_usage_percent),
        );

        // Render active dialog on top
        match &self.dialog {
            ActiveDialog::None => {}
            ActiveDialog::Create(d) => ui::dialogs::create::render(frame, d),
            ActiveDialog::Delete(d) => ui::dialogs::delete::render(frame, d),
            ActiveDialog::Handoff(d) => ui::dialogs::handoff::render(frame, d),
            ActiveDialog::Gc(d) => ui::dialogs::gc::render(frame, d),
            ActiveDialog::Restore(d) => ui::dialogs::restore::render(frame, d),
            ActiveDialog::Dispatch(d) => ui::dialogs::dispatch::render(frame, d),
            ActiveDialog::Broadcast(d) => ui::dialogs::broadcast::render(frame, d),
            ActiveDialog::Ship(d) => ui::dialogs::ship::render(frame, d),
            ActiveDialog::Help => ui::help::render(frame, self.help_scroll),
        }
    }

    /// Handle a single tick: poll for events and process them.
    pub fn tick(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(250))? {
            self.handle_event(event::read()?)?;
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(key) => {
                if should_process_key_event(&key) {
                    self.handle_key(key)?;
                }
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
            }
            Event::FocusGained => {
                if refresh_post_create_delete_guard_on_focus_return(
                    &mut self.suppress_delete_until,
                    &mut self.awaiting_focus_return_after_create,
                ) {
                    let _ = drain_pending_terminal_events();
                }
            }
            Event::FocusLost => {}
            _ => {}
        }
        Ok(())
    }

    /// Handle mouse events.
    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is in the worktree list area
                if let Some(list_area) = self.last_list_area {
                    if mouse.column >= list_area.x
                        && mouse.column < list_area.x + list_area.width
                        && mouse.row >= list_area.y
                        && mouse.row < list_area.y + list_area.height
                    {
                        // Calculate which item was clicked
                        // Account for the border (1 row at top)
                        let relative_row = mouse.row.saturating_sub(list_area.y + 1);
                        let filtered = self.filtered_worktrees();
                        let index = relative_row as usize;
                        if index < filtered.len() {
                            self.list_state.select(Some(index));
                            self.focus = FocusPanel::WorktreeList;
                            self.update_inspector();
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if self.focus == FocusPanel::Inspector {
                    self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
                } else {
                    self.move_selection(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.focus == FocusPanel::Inspector {
                    self.inspector_scroll = self.inspector_scroll.saturating_add(1);
                } else {
                    self.move_selection(1);
                }
            }
            _ => {}
        }
    }

    /// Route key events to the appropriate handler.
    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        // If in filter mode, handle filter input
        if self.filter_mode {
            return self.handle_filter_key(key);
        }

        // If a dialog is active, route to dialog handler
        match &self.dialog {
            ActiveDialog::None => self.handle_global_key(key),
            ActiveDialog::Help => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.help_scroll = self.help_scroll.saturating_add(1);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                    }
                    _ => {
                        self.dialog = ActiveDialog::None;
                        self.help_scroll = 0;
                    }
                }
                Ok(())
            }
            ActiveDialog::Create(_) => self.handle_create_key(key),
            ActiveDialog::Delete(_) => self.handle_delete_key(key),
            ActiveDialog::Handoff(_) => self.handle_handoff_key(key),
            ActiveDialog::Gc(_) => self.handle_gc_key(key),
            ActiveDialog::Restore(_) => self.handle_restore_key(key),
            ActiveDialog::Dispatch(_) => self.handle_dispatch_key(key),
            ActiveDialog::Broadcast(_) => self.handle_broadcast_key(key),
            ActiveDialog::Ship(_) => self.handle_ship_key(key),
        }
    }

    /// Handle keys when no dialog is active.
    fn handle_global_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.dialog = ActiveDialog::Help;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.focus == FocusPanel::Inspector {
                    self.inspector_scroll = self.inspector_scroll.saturating_add(1);
                } else {
                    self.move_selection(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.focus == FocusPanel::Inspector {
                    self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
                } else {
                    self.move_selection(-1);
                }
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    FocusPanel::WorktreeList => FocusPanel::Inspector,
                    FocusPanel::Inspector => FocusPanel::WorktreeList,
                };
            }
            KeyCode::Char('/') => {
                self.filter_mode = true;
                self.filter.clear();
                self.status_message = "Filter: ".to_string();
            }
            KeyCode::Char('n') => {
                self.open_create_dialog()?;
            }
            KeyCode::Char('s') => {
                self.launch_session()?;
            }
            KeyCode::Char('d') => {
                if !should_ignore_delete_shortcut(&mut self.suppress_delete_until) {
                    self.open_delete_dialog()?;
                }
            }
            KeyCode::Char('h') => {
                self.open_handoff_dialog()?;
            }
            KeyCode::Char('p') => {
                self.promote_selected()?;
            }
            KeyCode::Char('g') => {
                self.open_gc_dialog()?;
            }
            KeyCode::Char('r') => {
                self.open_restore_dialog()?;
            }
            KeyCode::Char('t') => {
                self.open_dispatch_dialog()?;
            }
            KeyCode::Char('b') => {
                self.open_broadcast_dialog()?;
            }
            KeyCode::Char('P') => {
                self.open_ship_dialog()?;
            }
            KeyCode::Char('S') => {
                self.execute_ship()?;
            }
            KeyCode::Char('c') => {
                self.open_ci_logs()?;
            }
            KeyCode::Enter => {
                self.launch_session()?;
            }
            KeyCode::Char('e') => {
                self.open_shell()?;
            }
            KeyCode::Char('m') => {
                let current = self.active_permission();
                let next = current.cycle_next();
                self.permission_override = Some(next);
                self.status_message = format!(
                    "Mode: {} ({}) for {}",
                    self.active_provider().mode_label(next),
                    next.short_label(),
                    self.active_provider().label()
                );
            }
            KeyCode::Char('o') => {
                let current = self.active_provider();
                let next = current.cycle_next();
                self.provider_override = Some(next);
                self.status_message = format!("Provider: {}", next.label());
            }
            KeyCode::Char('M') => {
                if self.config_meta.nix_managed {
                    self.status_message =
                        "Config is Nix-managed (read-only). Update your home-manager config instead.".to_string();
                } else {
                    let level = self.active_permission();
                    self.manager.config.session.default_permission = level;
                    let path =
                        self.config_meta.source_path.clone().unwrap_or_else(|| {
                            config::project_config_path(&self.manager.repo_root)
                        });
                    match config::save_config(&self.manager.config, &path) {
                        Ok(()) => {
                            self.status_message = format!(
                                "Saved default permission '{}' to {}",
                                level.label(),
                                path.display()
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to save config: {}", e);
                        }
                    }
                }
            }
            KeyCode::Char('O') => {
                if self.config_meta.nix_managed {
                    self.status_message =
                        "Config is Nix-managed (read-only). Update your home-manager config instead.".to_string();
                } else {
                    let provider = self.active_provider();
                    self.manager.config.session.provider = provider;
                    let path =
                        self.config_meta.source_path.clone().unwrap_or_else(|| {
                            config::project_config_path(&self.manager.repo_root)
                        });
                    match config::save_config(&self.manager.config, &path) {
                        Ok(()) => {
                            self.status_message = format!(
                                "Saved default provider '{}' to {}",
                                provider.label(),
                                path.display()
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to save config: {}", e);
                        }
                    }
                }
            }
            KeyCode::Esc => {
                // Clear filter if active
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.status_message.clear();
                    self.clamp_selection();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle keys in filter mode.
    fn handle_filter_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.filter_mode = false;
                self.filter.clear();
                self.status_message.clear();
                self.clamp_selection();
            }
            KeyCode::Enter => {
                self.filter_mode = false;
                if self.filter.is_empty() {
                    self.status_message.clear();
                } else {
                    self.status_message = format!("Filter active: {} (Esc to clear)", self.filter);
                }
                // Keep filter active
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.status_message = format!("Filter: {}_", self.filter);
                self.clamp_selection();
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.status_message = format!("Filter: {}_", self.filter);
                self.clamp_selection();
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle keys in the create dialog.
    fn handle_create_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Create(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            KeyCode::Tab | KeyCode::Down => {
                dialog.next_field();
            }
            KeyCode::BackTab | KeyCode::Up => {
                dialog.prev_field();
            }
            KeyCode::Enter => {
                if dialog.focus == dialog.confirm_field() {
                    dialog.confirmed = true;
                } else if dialog.focus == 0 && dialog.name_input.is_empty() {
                    // Quick-create: Enter on empty name → create with all defaults
                    dialog.confirmed = true;
                } else {
                    dialog.next_field();
                }
            }
            KeyCode::Left if dialog.focus == 1 => {
                dialog.prev_branch();
            }
            KeyCode::Right if dialog.focus == 1 => {
                dialog.next_branch();
            }
            KeyCode::Left if dialog.remote_field() == Some(dialog.focus) => {
                dialog.prev_remote();
            }
            KeyCode::Right if dialog.remote_field() == Some(dialog.focus) => {
                dialog.next_remote();
            }
            KeyCode::Char(' ') if dialog.focus == dialog.carry_field() => {
                dialog.toggle_carry();
            }
            KeyCode::Backspace if dialog.focus == 0 => {
                dialog.name_input.pop();
            }
            KeyCode::Char(c) if dialog.focus == 0 => {
                dialog.name_input.push(c);
            }
            _ => {}
        }

        // Check if dialog is done
        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            let name = if dialog_clone.name_input.is_empty() {
                None
            } else {
                Some(dialog_clone.name_input.as_str())
            };

            if let Some(ref remote_name) = dialog_clone.selected_remote {
                // Remote worktree creation
                self.create_remote_worktree(name, &dialog_clone.base_branch, remote_name);
            } else {
                // Local worktree creation
                match self.manager.create(
                    name,
                    &dialog_clone.base_branch,
                    dialog_clone.carry_changes,
                ) {
                    Ok(wt) => {
                        let wt_name = wt.name.clone();
                        self.setup_worktree_env(&wt_name);
                        self.status_message = format!("Created worktree '{}'", wt_name);
                        self.refresh();
                        // Select the newly created worktree
                        self.select_worktree_by_name(&wt_name);
                        self.update_inspector();
                        // Auto-launch session if configured
                        if self.manager.config.session.auto_launch {
                            let _ = self.launch_session();
                        }
                        arm_post_create_delete_guard(
                            &mut self.suppress_delete_until,
                            &mut self.awaiting_focus_return_after_create,
                        );
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            self.dialog = ActiveDialog::None;
            let _ = drain_pending_terminal_events();
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
            let _ = drain_pending_terminal_events();
        }

        Ok(())
    }

    /// Handle keys in the delete dialog.
    fn handle_delete_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Delete(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('y') => {
                dialog.confirmed = true;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            // Tear down container and release ports before deleting
            self.teardown_worktree_env(&dialog_clone.worktree_name);

            match self.manager.delete(&dialog_clone.worktree_name) {
                Ok(()) => {
                    self.status_message =
                        format!("Deleted '{}' (snapshot saved)", dialog_clone.worktree_name);
                    self.refresh();
                    self.clamp_selection();
                    self.update_inspector();
                }
                Err(e) => {
                    self.status_message = format!("Error: {}", e);
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the handoff dialog.
    fn handle_handoff_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Handoff(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Tab => {
                dialog.toggle_direction();
            }
            KeyCode::Enter => {
                dialog.confirmed = true;
            }
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let Some(wt) = self.selected_worktree() {
                let wt_abs = self.manager.worktree_abs_path(wt);
                match handoff::execute(
                    dialog_clone.direction,
                    &wt_abs,
                    &self.manager.repo_root,
                    dialog_clone.base_commit.as_deref(),
                ) {
                    Ok(()) => {
                        let dir_str = match dialog_clone.direction {
                            HandoffDirection::WorktreeToLocal => "worktree -> local",
                            HandoffDirection::LocalToWorktree => "local -> worktree",
                        };
                        self.status_message = format!("Handoff complete ({})", dir_str);
                        self.update_inspector();
                    }
                    Err(e) => {
                        self.status_message = format!("Handoff error: {}", e);
                    }
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the GC dialog.
    fn handle_gc_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Gc(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('y') => {
                dialog.confirmed = true;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed && !dialog_clone.to_prune.is_empty() {
            // Tear down containers and release ports for pruned worktrees
            for name in &dialog_clone.to_prune {
                self.teardown_worktree_env(name);
            }

            match self.manager.gc_execute(&dialog_clone.to_prune) {
                Ok(deleted) => {
                    self.status_message =
                        format!("GC complete: {} worktree(s) pruned", deleted.len());
                    self.refresh();
                    self.clamp_selection();
                    self.update_inspector();
                }
                Err(e) => {
                    self.status_message = format!("GC error: {}", e);
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled || dialog_clone.to_prune.is_empty() {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    // --- Action methods ---

    fn move_selection(&mut self, delta: i32) {
        let count = self.filtered_worktrees().len();
        if count == 0 {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let new = if delta > 0 {
            (current + 1).min(count - 1)
        } else {
            current.saturating_sub(1)
        };
        self.list_state.select(Some(new));
        self.update_inspector();
    }

    fn clamp_selection(&mut self) {
        let count = self.filtered_worktrees().len();
        self.list_state
            .select(clamp_selected_index(self.list_state.selected(), count));
    }

    /// Select a worktree by name, accounting for any active filter.
    fn select_worktree_by_name(&mut self, name: &str) {
        let filtered = self.filtered_worktrees();
        if let Some(idx) = filtered.iter().position(|wt| wt.name == name) {
            self.list_state.select(Some(idx));
        }
    }

    fn open_create_dialog(&mut self) -> Result<()> {
        let branches: Vec<String> = git::branch::list_branches(&self.manager.repo_root)?
            .into_iter()
            .map(|b| b.name)
            .collect();
        let remote_names: Vec<String> = self
            .manager
            .config
            .remote
            .iter()
            .map(|r| r.name.clone())
            .collect();
        let dialog = ui::dialogs::create::CreateDialog::with_remotes(branches, remote_names);
        self.dialog = ActiveDialog::Create(dialog);
        Ok(())
    }

    fn open_delete_dialog(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        let wt_abs = self.manager.worktree_abs_path(wt);
        let diff_preview = git::diff::diff_stat(&wt_abs)
            .map(|s| s.raw)
            .unwrap_or_else(|_| "Unable to get diff".to_string());

        let dialog = ui::dialogs::delete::DeleteDialog::new(wt.name.clone(), diff_preview);
        self.dialog = ActiveDialog::Delete(dialog);
        Ok(())
    }

    fn open_handoff_dialog(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        let wt_abs = self.manager.worktree_abs_path(wt);
        let base_commit = Some(wt.base_commit.clone());

        let preview = handoff::preview(
            HandoffDirection::WorktreeToLocal,
            &wt_abs,
            &self.manager.repo_root,
            base_commit.as_deref(),
        );

        let (stat_raw, files_changed, has_commits, commit_count, gitignore_warnings) = match preview
        {
            Ok(p) => (
                p.diff_stat.raw,
                p.diff_stat.files_changed,
                p.has_commits,
                p.commit_count,
                p.gitignore_warnings,
            ),
            Err(_) => {
                let stat = git::diff::diff_stat(&wt_abs).unwrap_or_default();
                (stat.raw, stat.files_changed, false, 0, Vec::new())
            }
        };

        let dialog = ui::dialogs::handoff::HandoffDialog::new(
            wt.name.clone(),
            stat_raw,
            files_changed,
            has_commits,
            commit_count,
            gitignore_warnings,
            base_commit,
        );
        self.dialog = ActiveDialog::Handoff(dialog);
        Ok(())
    }

    fn open_gc_dialog(&mut self) -> Result<()> {
        let to_prune = self.manager.gc_preview()?;
        let dialog = ui::dialogs::gc::GcDialog::new(to_prune);
        self.dialog = ActiveDialog::Gc(dialog);
        Ok(())
    }

    fn launch_session(&mut self) -> Result<()> {
        // Check if selected worktree is remote - delegate to remote session handler
        if let Some(wt) = self.selected_worktree() {
            if wt.is_remote() {
                return self.launch_remote_session();
            }
        }

        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        // If there's an existing pane, try to focus it
        if let Some(ref pane_id) = wt.tmux_pane {
            if crate::tmux::pane::pane_exists(pane_id) {
                match session::launcher::focus_session(pane_id) {
                    Ok(()) => {
                        self.status_message = format!("Focused session for '{}'", wt.name);
                        return Ok(());
                    }
                    Err(_) => {
                        // Pane gone, fall through to launch/resume
                    }
                }
            }
        }

        let wt_abs = self.manager.worktree_abs_path(&wt);

        // Check if we have a previous session ID to resume
        let session_id = wt.last_session_id.clone().or_else(|| {
            session::tracker::find_project_dir(&wt_abs)
                .ok()
                .flatten()
                .and_then(|dir| {
                    session::tracker::find_latest_session_id(&dir)
                        .ok()
                        .flatten()
                })
        });

        let permission = self.active_permission();
        let permissions = &self.manager.config.session.permissions;
        let mut session_cfg = self.manager.config.session.clone();
        session_cfg.provider = self.active_provider();
        let launch_result = if let Some(ref sid) = session_id {
            // Try to resume a previous session
            session::launcher::resume_session(
                &wt,
                &wt_abs,
                sid,
                &session_cfg,
                permission,
                permissions,
            )
        } else {
            // Fresh launch
            session::launcher::launch_session(&wt, &wt_abs, &session_cfg, permission, permissions)
        };

        match launch_result {
            Ok(pane_id) => {
                let action = if session_id.is_some() {
                    "Resumed"
                } else {
                    "Launched"
                };

                // Update state with pane ID and session ID
                if let Ok(mut state) = self.manager.load_state() {
                    if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                        stored.tmux_pane = Some(pane_id.clone());
                        stored.status = WorktreeStatus::Running;
                        if session_id.is_some() {
                            stored.last_session_id = session_id.clone();
                        }
                    }
                    let _ = self.manager.save_state(&state);
                }

                self.status_message = format!(
                    "{} session for '{}' [{}] ({})",
                    action,
                    wt.name,
                    permission.label(),
                    pane_id,
                );
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("Session error: {}", e);
            }
        }

        Ok(())
    }

    fn promote_selected(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        let name = wt.name.clone();
        match self.manager.promote(&name) {
            Ok(true) => {
                self.status_message = format!("Promoted '{}' to permanent", name);
                self.refresh();
                self.update_inspector();
            }
            Ok(false) => {
                self.status_message = format!("'{}' is already permanent", name);
            }
            Err(e) => {
                self.status_message = format!("Error: {}", e);
            }
        }

        Ok(())
    }

    fn open_restore_dialog(&mut self) -> Result<()> {
        let snapshots = self.manager.list_snapshots()?;
        let dialog = ui::dialogs::restore::RestoreDialog::new(snapshots);
        self.dialog = ActiveDialog::Restore(dialog);
        Ok(())
    }

    /// Handle keys in the restore dialog.
    fn handle_restore_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Restore(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                dialog.move_selection(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                dialog.move_selection(-1);
            }
            KeyCode::Enter => {
                if !dialog.snapshots.is_empty() {
                    dialog.confirmed = true;
                }
            }
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let Some(snap) = dialog_clone.selected_snapshot() {
                match self.manager.restore_snapshot(snap) {
                    Ok(wt) => {
                        self.status_message = format!("Restored '{}' from snapshot", wt.name);
                        self.refresh();
                        self.update_inspector();
                    }
                    Err(e) => {
                        self.status_message = format!("Restore error: {}", e);
                    }
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    fn open_dispatch_dialog(&mut self) -> Result<()> {
        let branches: Vec<String> = git::branch::list_branches(&self.manager.repo_root)?
            .into_iter()
            .map(|b| b.name)
            .collect();
        let dialog = ui::dialogs::dispatch::DispatchDialog::new(branches);
        self.dialog = ActiveDialog::Dispatch(dialog);
        Ok(())
    }

    fn open_broadcast_dialog(&mut self) -> Result<()> {
        let running: Vec<String> = self
            .worktrees
            .iter()
            .filter(|wt| wt.status == WorktreeStatus::Running && wt.tmux_pane.is_some())
            .map(|wt| wt.name.clone())
            .collect();
        let count = running.len();

        if count == 0 {
            self.status_message = "No running sessions to broadcast to".to_string();
            return Ok(());
        }

        let dialog = ui::dialogs::broadcast::BroadcastDialog::new(count, running);
        self.dialog = ActiveDialog::Broadcast(dialog);
        Ok(())
    }

    /// Handle keys in the dispatch dialog.
    fn handle_dispatch_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Dispatch(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            KeyCode::Tab => {
                // If we're on the input field and there's text, add it as a task first
                if dialog.focus == 0 && !dialog.current_input.trim().is_empty() {
                    dialog.add_task();
                }
                dialog.next_field();
            }
            KeyCode::BackTab => {
                dialog.prev_field();
            }
            KeyCode::Enter => {
                if dialog.focus == 0 {
                    // Add current input as a task
                    dialog.add_task();
                } else if dialog.focus == 2 {
                    // Confirm dispatch
                    // First add any pending input
                    if !dialog.current_input.trim().is_empty() {
                        dialog.add_task();
                    }
                    if !dialog.tasks.is_empty() {
                        dialog.confirmed = true;
                    }
                } else {
                    dialog.next_field();
                }
            }
            KeyCode::Left if dialog.focus == 1 => {
                dialog.prev_branch();
            }
            KeyCode::Right if dialog.focus == 1 => {
                dialog.next_branch();
            }
            KeyCode::Backspace if dialog.focus == 0 => {
                if dialog.current_input.is_empty() {
                    dialog.remove_last_task();
                } else {
                    dialog.current_input.pop();
                }
            }
            KeyCode::Char(c) if dialog.focus == 0 => {
                dialog.current_input.push(c);
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            let tasks = dialog_clone.tasks;
            let base = dialog_clone.base_branch;

            if tasks.is_empty() {
                self.status_message = "No tasks to dispatch".to_string();
            } else {
                let permission = self.active_permission();
                let results = orchestration::dispatch::dispatch_tasks(
                    &self.manager,
                    &tasks,
                    &base,
                    permission,
                );
                let success_count = results.iter().filter(|r| r.error.is_none()).count();
                let fail_count = results.iter().filter(|r| r.error.is_some()).count();

                if fail_count == 0 {
                    self.status_message =
                        format!("Dispatched {} task(s) successfully", success_count);
                } else {
                    self.status_message = format!(
                        "Dispatched {} task(s), {} failed",
                        success_count, fail_count
                    );
                }

                self.refresh();
                self.update_inspector();
                self.update_dashboard();
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the broadcast dialog.
    fn handle_broadcast_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Broadcast(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            KeyCode::Enter => {
                if !dialog.prompt_input.trim().is_empty() && dialog.target_count > 0 {
                    dialog.confirmed = true;
                }
            }
            KeyCode::Backspace => {
                dialog.prompt_input.pop();
            }
            KeyCode::Char(c) => {
                dialog.prompt_input.push(c);
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            let prompt = dialog_clone.prompt_input.trim().to_string();
            let results = orchestration::broadcast::broadcast_prompt(&self.worktrees, &prompt);
            let success_count = results.iter().filter(|r| r.success).count();
            let fail_count = results.iter().filter(|r| !r.success).count();

            if fail_count == 0 {
                self.status_message = format!("Broadcast sent to {} session(s)", success_count);
            } else {
                self.status_message =
                    format!("Broadcast: {} sent, {} failed", success_count, fail_count);
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    fn open_shell(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        if !crate::tmux::pane::is_inside_tmux() {
            self.status_message = "zellij or tmux required for shell panes".to_string();
            return Ok(());
        }

        // Handle remote worktrees: open SSH shell
        if let Some(ref remote_name) = wt.remote_host {
            let host = match self
                .manager
                .config
                .remote
                .iter()
                .find(|r| r.name == *remote_name)
                .cloned()
            {
                Some(h) => h,
                None => {
                    self.status_message =
                        format!("Remote host '{}' not found in config", remote_name);
                    return Ok(());
                }
            };

            let repo_name = remote::sync::repo_name_from_path(&self.manager.repo_root);
            match remote::session::open_remote_shell(&host, &repo_name, &wt.name) {
                Ok(pane_id) => {
                    self.status_message = format!(
                        "Opened remote shell for '{}' on '{}' ({})",
                        wt.name, host.name, pane_id
                    );
                }
                Err(e) => {
                    self.status_message = format!("Remote shell error: {}", e);
                }
            }
            return Ok(());
        }

        let wt_abs = self.manager.worktree_abs_path(&wt);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        let title = format!("cwt:shell:{}", wt.name);

        match crate::tmux::pane::create_pane(&wt_abs, &shell, &title) {
            Ok(pane_id) => {
                self.status_message = format!("Opened shell for '{}' ({})", wt.name, pane_id);
            }
            Err(e) => {
                self.status_message = format!("Shell error: {}", e);
            }
        }

        Ok(())
    }

    /// Open the ship/PR dialog for the selected worktree.
    fn open_ship_dialog(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        if !ship::pr::gh_available() {
            self.status_message = "gh CLI not found (install: https://cli.github.com/)".to_string();
            return Ok(());
        }

        let wt_abs = self.manager.worktree_abs_path(wt);
        let diff_preview = git::diff::diff_stat(&wt_abs)
            .map(|s| s.raw)
            .unwrap_or_else(|_| "Unable to get diff".to_string());
        let files_changed = git::diff::diff_stat(&wt_abs)
            .map(|s| s.files_changed)
            .unwrap_or(0);

        let dialog = ui::dialogs::ship::ShipDialog::new(
            wt.name.clone(),
            wt.branch.clone(),
            wt.base_branch.clone(),
            diff_preview,
            files_changed,
        );
        self.dialog = ActiveDialog::Ship(dialog);
        Ok(())
    }

    /// Handle keys in the ship dialog.
    fn handle_ship_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Ship(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Tab => {
                dialog.toggle_mode();
            }
            KeyCode::Enter => {
                dialog.confirmed = true;
            }
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let Some(wt) = self.selected_worktree().cloned() {
                let wt_abs = self.manager.worktree_abs_path(&wt);

                if dialog_clone.mode == 0 {
                    // "Create PR only" — just push and create PR
                    self.do_create_pr(&wt, &wt_abs);
                } else {
                    // "Ship it" — push + PR + mark shipping
                    self.do_ship(&wt, &wt_abs);
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Create a PR for the worktree (P key -> confirm).
    fn do_create_pr(&mut self, wt: &Worktree, wt_abs: &std::path::Path) {
        match ship::pr::commit_and_push(wt_abs, &wt.branch) {
            Ok(push_msg) => {
                self.status_message = push_msg;
            }
            Err(e) => {
                self.status_message = format!("Push failed: {}", e);
                return;
            }
        }

        let body = ship::pr::generate_pr_body(wt_abs, wt);
        let title = ship::pr::generate_pr_title(wt);

        match ship::pr::create_pr(wt_abs, &wt.branch, &wt.base_branch, &title, &body) {
            Ok(result) => {
                // Update worktree state with PR info
                if let Ok(mut state) = self.manager.load_state() {
                    if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                        stored.pr_number = Some(result.pr_number);
                        stored.pr_url = Some(result.pr_url.clone());
                        stored.pr_status = ship::pr::PrStatus::Open;
                    }
                    let _ = self.manager.save_state(&state);
                }

                self.status_message =
                    format!("PR #{} created: {}", result.pr_number, result.pr_url);
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("PR creation failed: {}", e);
            }
        }
    }

    /// Execute the "ship it" flow: push + PR + mark shipping.
    fn do_ship(&mut self, wt: &Worktree, wt_abs: &std::path::Path) {
        match ship::pipeline::ship(wt, wt_abs) {
            Ok(result) => {
                // Update worktree state with PR info and shipping status
                if let Ok(mut state) = self.manager.load_state() {
                    if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                        stored.pr_number = Some(result.pr_number);
                        stored.pr_url = Some(result.pr_url.clone());
                        stored.pr_status = ship::pr::PrStatus::Open;
                        stored.status = WorktreeStatus::Shipping;
                    }
                    let _ = self.manager.save_state(&state);
                }

                self.status_message =
                    format!("Shipped! PR #{}: {}", result.pr_number, result.pr_url);
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("Ship failed: {}", e);
            }
        }
    }

    /// Execute "ship it" macro directly from the S key (skips dialog).
    fn execute_ship(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        if !ship::pr::gh_available() {
            self.status_message = "gh CLI not found (install: https://cli.github.com/)".to_string();
            return Ok(());
        }

        let wt_abs = self.manager.worktree_abs_path(&wt);
        self.do_ship(&wt, &wt_abs);
        Ok(())
    }

    /// Open CI logs in browser for the selected worktree.
    fn open_ci_logs(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        if !ship::pr::gh_available() {
            self.status_message = "gh CLI not found".to_string();
            return Ok(());
        }

        match ship::ci::open_ci_logs(&self.manager.repo_root, &wt.branch) {
            Ok(()) => {
                self.status_message = format!("Opening CI logs for '{}'", wt.name);
            }
            Err(e) => {
                self.status_message = format!("CI logs: {}", e);
            }
        }

        Ok(())
    }

    /// Set up environment (container, ports) for a newly created worktree.
    fn setup_worktree_env(&mut self, worktree_name: &str) {
        let config = &self.manager.config.container;

        // Allocate ports if container support is enabled and auto_ports is on
        if config.enabled && config.auto_ports {
            let port_names: Vec<&str> = config.port_names.iter().map(|s| s.as_str()).collect();
            let port_names_ref = if port_names.is_empty() {
                vec!["app"]
            } else {
                port_names
            };

            match self.port_manager.allocate(worktree_name, &port_names_ref) {
                Ok(alloc) => {
                    // Save port allocation to state
                    if let Ok(mut state) = self.manager.load_state() {
                        if let Some(stored) = state.worktrees.get_mut(worktree_name) {
                            stored.ports = Some(alloc.clone());
                        }
                        let _ = self.manager.save_state(&state);
                    }
                }
                Err(e) => {
                    self.status_message = format!("Port allocation warning: {}", e);
                }
            }
        }

        // Set up container if container support is enabled
        if !config.enabled {
            return;
        }

        // Find the worktree to get its path
        let wt = match self.worktrees.iter().find(|wt| wt.name == worktree_name) {
            Some(wt) => wt.clone(),
            None => return,
        };
        let wt_abs = self.manager.worktree_abs_path(&wt);

        // Determine the containerfile to use
        let containerfile = if !config.containerfile.is_empty() {
            Some(config.containerfile.clone())
        } else {
            // Auto-detect: check for devcontainer.json first, then Containerfile
            if let Some(dc_path) = env::devcontainer::find_devcontainer(&wt_abs) {
                if let Ok(dc_config) = env::devcontainer::parse_devcontainer(&dc_path) {
                    if let Some((dockerfile, _context)) =
                        env::devcontainer::resolve_containerfile(&dc_config, &dc_path)
                    {
                        Some(dockerfile)
                    } else if dc_config.image.is_some() {
                        // Has an image but no Dockerfile -- skip container build
                        None
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                env::devcontainer::find_containerfile(&wt_abs)
                    .map(|p| p.to_string_lossy().to_string())
            }
        };

        let Some(containerfile) = containerfile else {
            return;
        };

        // Gather env vars (ports + devcontainer env)
        let mut env_vars: Vec<(String, String)> = Vec::new();
        if let Some(alloc) = self.port_manager.get(worktree_name) {
            env_vars.extend(alloc.env_vars());
        }

        // Port mappings
        let port_mappings: Vec<(u16, u16)> = self
            .port_manager
            .get(worktree_name)
            .map(|alloc| alloc.ports.values().map(|&p| (p, p)).collect())
            .unwrap_or_default();

        // Build and run the container
        match env::container::setup_container(
            worktree_name,
            &wt_abs,
            &containerfile,
            &env_vars,
            &port_mappings,
        ) {
            Ok(container_info) => {
                // Save container info to state
                if let Ok(mut state) = self.manager.load_state() {
                    if let Some(stored) = state.worktrees.get_mut(worktree_name) {
                        stored.container = Some(container_info);
                    }
                    let _ = self.manager.save_state(&state);
                }
            }
            Err(e) => {
                self.status_message = format!(
                    "Container setup warning for '{}': {} (falling back to bare worktree)",
                    worktree_name, e
                );
            }
        }
    }

    /// Create a worktree on a remote host.
    fn create_remote_worktree(&mut self, name: Option<&str>, base_branch: &str, remote_name: &str) {
        let host = match self
            .manager
            .config
            .remote
            .iter()
            .find(|r| r.name == remote_name)
            .cloned()
        {
            Some(h) => h,
            None => {
                self.status_message = format!("Remote host '{}' not found in config", remote_name);
                return;
            }
        };

        let wt_name = match name {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => crate::worktree::slug::generate_slug(),
        };

        let repo_name = remote::sync::repo_name_from_path(&self.manager.repo_root);

        // Ensure remote has the repo
        let repo_url = match remote::sync::get_repo_remote_url(&self.manager.repo_root) {
            Ok(url) => url,
            Err(e) => {
                self.status_message = format!("Error getting repo URL: {}", e);
                return;
            }
        };

        if let Err(e) = host.ensure_repo(&repo_url, &repo_name) {
            self.status_message = format!("Error setting up remote repo: {}", e);
            return;
        }

        // Get base commit from remote
        let base_commit = match host.head_commit(&repo_name) {
            Ok(c) => c,
            Err(e) => {
                self.status_message = format!("Error getting remote HEAD: {}", e);
                return;
            }
        };

        // Create worktree on remote
        let branch_name = format!("wt/{}", wt_name);
        let remote_path =
            match host.create_worktree(&repo_name, &wt_name, &branch_name, base_branch) {
                Ok(p) => p,
                Err(e) => {
                    self.status_message = format!("Error creating remote worktree: {}", e);
                    return;
                }
            };

        // Register in local state
        let wt_rel_path =
            std::path::PathBuf::from(&self.manager.config.worktree.dir).join(&wt_name);
        let wt = Worktree::new_remote(
            wt_name.clone(),
            wt_rel_path,
            branch_name,
            base_branch.to_string(),
            base_commit,
            crate::worktree::Lifecycle::Ephemeral,
            host.name.clone(),
            remote_path,
        );

        if let Ok(mut state) = self.manager.load_state() {
            state.worktrees.insert(wt_name.clone(), wt);
            let _ = self.manager.save_state(&state);
        }

        self.status_message = format!("Created remote worktree '{}' on '{}'", wt_name, host.name);
        self.refresh();
        self.update_inspector();
    }

    /// Launch or focus a remote session.
    fn launch_remote_session(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        let remote_name = match wt.remote_host {
            Some(ref name) => name.clone(),
            None => {
                self.status_message = "Not a remote worktree".to_string();
                return Ok(());
            }
        };

        let host = match self
            .manager
            .config
            .remote
            .iter()
            .find(|r| r.name == remote_name)
            .cloned()
        {
            Some(h) => h,
            None => {
                self.status_message = format!("Remote host '{}' not found in config", remote_name);
                return Ok(());
            }
        };

        // Check if we already have a local pane attached to this remote session
        if let Some(ref pane_id) = wt.tmux_pane {
            if crate::tmux::pane::pane_exists(pane_id) {
                match crate::session::launcher::focus_session(pane_id) {
                    Ok(()) => {
                        self.status_message = format!("Focused remote session for '{}'", wt.name);
                        return Ok(());
                    }
                    Err(_) => {
                        // Pane gone, fall through to create new attachment
                    }
                }
            }
        }

        let repo_name = remote::sync::repo_name_from_path(&self.manager.repo_root);

        // Check if remote session exists, if not launch it
        let remote_status = remote::session::check_remote_session_status(&host, &wt.name);

        if remote_status == remote::session::RemoteSessionStatus::NoSession {
            let permission = self.active_permission();
            let permissions = &self.manager.config.session.permissions;
            // Launch a new remote session
            let remote_cmd_cfg = remote::session::RemoteCommandConfig {
                provider: self.active_provider(),
                command: &self.manager.config.session.command,
                provider_args: &self.manager.config.session.provider_args,
                permission,
                permissions,
            };

            match remote::session::launch_remote_session(
                &host,
                &repo_name,
                &wt.name,
                &remote_cmd_cfg,
            ) {
                Ok(_tmux_session) => {
                    self.status_message = format!(
                        "Launched remote session for '{}' on '{}'",
                        wt.name, host.name
                    );
                }
                Err(e) => {
                    self.status_message = format!("Remote session error: {}", e);
                    return Ok(());
                }
            }
        }

        // Now attach to the remote session via a local tmux pane
        match remote::session::focus_remote_session(&host, &wt.name) {
            Ok(pane_id) => {
                // Update state with the local pane ID
                if let Ok(mut state) = self.manager.load_state() {
                    if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                        stored.tmux_pane = Some(pane_id.clone());
                        stored.status = WorktreeStatus::Running;
                    }
                    let _ = self.manager.save_state(&state);
                }

                self.status_message = format!(
                    "Attached to remote session for '{}' on '{}' ({})",
                    wt.name, host.name, pane_id
                );
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("Remote session attach error: {}", e);
            }
        }

        Ok(())
    }

    /// Poll remote worktree statuses. Called less frequently than local refresh.
    pub fn poll_remote_statuses(&mut self) {
        if self.manager.config.remote.is_empty() {
            return;
        }

        // Update network status for remote hosts
        self.remote_statuses = self
            .manager
            .config
            .remote
            .iter()
            .map(remote::host::RemoteHostStatus::check)
            .collect();

        // Update status of remote worktrees
        let remote_wts: Vec<(String, String)> = self
            .worktrees
            .iter()
            .filter_map(|wt| {
                wt.remote_host
                    .as_ref()
                    .map(|h| (wt.name.clone(), h.clone()))
            })
            .collect();

        if remote_wts.is_empty() {
            return;
        }

        let _repo_name = remote::sync::repo_name_from_path(&self.manager.repo_root);
        let mut status_updates: Vec<(String, WorktreeStatus)> = Vec::new();

        for (wt_name, host_name) in &remote_wts {
            let host = match self
                .manager
                .config
                .remote
                .iter()
                .find(|r| r.name == *host_name)
            {
                Some(h) => h,
                None => continue,
            };

            // Check if host is reachable first
            let host_status = self.remote_statuses.iter().find(|s| s.name == *host_name);
            if let Some(s) = host_status {
                if s.network == remote::host::NetworkStatus::Disconnected {
                    continue; // Skip unreachable hosts
                }
            }

            let session_status = remote::session::check_remote_session_status(host, wt_name);

            let new_status = match session_status {
                remote::session::RemoteSessionStatus::Running => WorktreeStatus::Running,
                remote::session::RemoteSessionStatus::Done => WorktreeStatus::Done,
                remote::session::RemoteSessionStatus::NoSession => WorktreeStatus::Idle,
                remote::session::RemoteSessionStatus::Unknown => {
                    // Keep existing status
                    continue;
                }
            };

            // Only update if status changed
            if let Some(wt) = self.worktrees.iter().find(|w| w.name == *wt_name) {
                if wt.status != new_status {
                    status_updates.push((wt_name.clone(), new_status));
                }
            }
        }

        // Apply updates
        if !status_updates.is_empty() {
            if let Ok(mut state) = self.manager.load_state() {
                for (name, status) in &status_updates {
                    if let Some(stored) = state.worktrees.get_mut(name) {
                        stored.status = status.clone();
                    }
                }
                let _ = self.manager.save_state(&state);
            }

            for (name, status) in status_updates {
                if let Some(wt) = self.worktrees.iter_mut().find(|w| w.name == name) {
                    wt.status = status;
                }
            }

            self.update_badge_counts();
        }
    }

    /// Tear down environment (container, ports) for a worktree being deleted.
    fn teardown_worktree_env(&mut self, worktree_name: &str) {
        // Tear down remote session and worktree if this is a remote worktree
        if let Some(wt) = self.worktrees.iter().find(|wt| wt.name == worktree_name) {
            if let Some(ref remote_name) = wt.remote_host {
                if let Some(host) = self
                    .manager
                    .config
                    .remote
                    .iter()
                    .find(|r| r.name == *remote_name)
                    .cloned()
                {
                    // Kill remote session
                    let _ = remote::session::kill_remote_session(&host, worktree_name);

                    // Remove remote worktree
                    let repo_name = remote::sync::repo_name_from_path(&self.manager.repo_root);
                    if let Err(e) = host.remove_worktree(&repo_name, worktree_name) {
                        self.status_message = format!(
                            "Remote worktree cleanup warning for '{}': {}",
                            worktree_name, e
                        );
                    }
                }
            }
        }

        // Tear down container if present
        if let Some(wt) = self.worktrees.iter().find(|wt| wt.name == worktree_name) {
            if let Some(ref container) = wt.container {
                if let Err(e) = env::container::teardown_container(container) {
                    // Non-fatal: log warning but continue with deletion
                    self.status_message =
                        format!("Container teardown warning for '{}': {}", worktree_name, e);
                }
            }
        }

        // Release ports
        self.port_manager.release(worktree_name);
    }

    /// Update resource usage for the currently selected worktree.
    /// Called periodically from the refresh loop.
    pub fn update_resource_usage(&mut self) {
        if !self.manager.config.container.track_resources {
            return;
        }

        let mut usage_data: Vec<(String, env::resources::ResourceUsage)> = Vec::new();

        for wt in &self.worktrees {
            let wt_abs = self.manager.worktree_abs_path(wt);
            let container_id = wt.container_id();
            let runtime = wt
                .container
                .as_ref()
                .map(|c| &c.runtime)
                .unwrap_or(&env::container::ContainerRuntime::None);

            let usage = env::resources::get_resource_usage(&wt_abs, container_id, runtime);
            usage_data.push((wt.name.clone(), usage));
        }

        // Check for warnings
        self.resource_warnings = env::resources::check_warnings(&usage_data);

        // Update worktree resource_usage fields in state
        if let Ok(mut state) = self.manager.load_state() {
            for (name, usage) in &usage_data {
                if let Some(stored) = state.worktrees.get_mut(name) {
                    stored.resource_usage = Some(usage.clone());
                }
            }
            let _ = self.manager.save_state(&state);
        }

        // Update local worktree list
        for (name, usage) in usage_data {
            if let Some(wt) = self.worktrees.iter_mut().find(|wt| wt.name == name) {
                wt.resource_usage = Some(usage);
            }
        }

        // Show the most critical warning in the status bar
        if let Some(warning) = self.resource_warnings.first() {
            if warning.severity == env::resources::WarningSeverity::Critical {
                self.status_message =
                    format!("WARNING: {} -- {}", warning.worktree_name, warning.message);
            }
        }
    }

    /// Update container statuses for worktrees with containers.
    pub fn update_container_statuses(&mut self) {
        let mut updates: Vec<(String, env::container::ContainerStatus)> = Vec::new();

        for wt in &self.worktrees {
            if let Some(ref container) = wt.container {
                if container.status == env::container::ContainerStatus::None {
                    continue;
                }
                let cid = container
                    .container_id
                    .as_deref()
                    .or(container.container_name.as_deref());
                if let Some(cid) = cid {
                    let new_status =
                        env::container::inspect_container_status(&container.runtime, cid);
                    if new_status != container.status {
                        updates.push((wt.name.clone(), new_status));
                    }
                }
            }
        }

        if updates.is_empty() {
            return;
        }

        // Persist changes
        if let Ok(mut state) = self.manager.load_state() {
            for (name, status) in &updates {
                if let Some(stored) = state.worktrees.get_mut(name) {
                    if let Some(ref mut container) = stored.container {
                        container.status = status.clone();
                    }
                }
            }
            let _ = self.manager.save_state(&state);
        }

        // Update local worktree list
        for (name, status) in updates {
            if let Some(wt) = self.worktrees.iter_mut().find(|wt| wt.name == name) {
                if let Some(ref mut container) = wt.container {
                    container.status = status;
                }
            }
        }
    }

    /// Poll PR/CI status for worktrees that have open PRs.
    /// Called periodically from the refresh loop.
    pub fn poll_ship_status(&mut self) {
        if !ship::pr::gh_available() {
            return;
        }

        let mut updates: Vec<(
            String,
            ship::pr::PrStatus,
            ship::pr::CiStatus,
            Option<String>,
        )> = Vec::new();
        let mut merged_worktrees: Vec<String> = Vec::new();

        for wt in &self.worktrees {
            if wt.pr_number.is_none() {
                continue;
            }

            let (pr_status, ci_status, pr_url) =
                ship::pipeline::poll_status(&self.manager.repo_root, wt);

            // Check if PR was just merged
            if pr_status == ship::pr::PrStatus::Merged && wt.pr_status != ship::pr::PrStatus::Merged
            {
                merged_worktrees.push(wt.name.clone());
            }

            updates.push((wt.name.clone(), pr_status, ci_status, pr_url));
        }

        // Apply updates
        if !updates.is_empty() {
            if let Ok(mut state) = self.manager.load_state() {
                for (name, pr_status, ci_status, pr_url) in &updates {
                    if let Some(stored) = state.worktrees.get_mut(name) {
                        stored.pr_status = pr_status.clone();
                        stored.ci_status = ci_status.clone();
                        if let Some(url) = pr_url {
                            stored.pr_url = Some(url.clone());
                        }
                    }
                }
                let _ = self.manager.save_state(&state);
            }

            // Update local worktree list
            for (name, pr_status, ci_status, pr_url) in updates {
                if let Some(wt) = self.worktrees.iter_mut().find(|wt| wt.name == name) {
                    wt.pr_status = pr_status;
                    wt.ci_status = ci_status;
                    if let Some(url) = pr_url {
                        wt.pr_url = Some(url);
                    }
                }
            }
        }

        // Auto-cleanup merged worktrees
        for name in merged_worktrees {
            self.status_message = format!("PR merged for '{}' -- auto-cleaning up", name);
            self.teardown_worktree_env(&name);
            match self.manager.delete(&name) {
                Ok(()) => {
                    self.status_message =
                        format!("PR merged for '{}' -- worktree cleaned up", name);
                }
                Err(e) => {
                    self.status_message =
                        format!("PR merged for '{}' but cleanup failed: {}", name, e);
                }
            }
            self.refresh();
            self.clamp_selection();
            self.update_inspector();
        }
    }
}

// --- Forest Mode ---

/// Which panel has focus in forest mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForestFocusPanel {
    RepoList,
    WorktreeList,
    Inspector,
}

/// Per-repo state cached in the forest app.
pub struct RepoState {
    pub name: String,
    pub path: PathBuf,
    pub manager: Manager,
    pub worktrees: Vec<Worktree>,
    pub stats: RepoStats,
}

/// Top-level application state for forest (multi-repo) mode.
pub struct ForestApp {
    pub repos: Vec<RepoState>,
    pub repo_list_state: ListState,
    pub worktree_list_state: ListState,
    pub focus: ForestFocusPanel,
    pub dialog: ActiveDialog,
    pub filter: String,
    pub filter_mode: bool,
    pub status_message: String,
    pub inspector_info: ui::inspector::InspectorInfo,
    pub inspector_scroll: u16,
    pub should_quit: bool,
    /// Aggregate counts across all repos.
    pub total_running: usize,
    pub total_waiting: usize,
    pub total_done: usize,
    /// Track the areas for mouse click handling.
    pub last_repo_list_area: Option<ratatui::layout::Rect>,
    pub last_wt_list_area: Option<ratatui::layout::Rect>,
    /// Scroll offset for the help overlay.
    pub help_scroll: u16,
    /// Runtime override for the permission level.
    pub permission_override: Option<PermissionLevel>,
    /// Runtime override for provider.
    pub provider_override: Option<SessionProvider>,
    /// Ignore a stray delete shortcut immediately after a successful create.
    pub suppress_delete_until: Option<Instant>,
    /// Re-arm the post-create delete guard if focus returns after zellij/tab switching.
    pub awaiting_focus_return_after_create: bool,
}

impl ForestApp {
    pub fn new(forest_config: &forest::ForestConfig) -> Result<Self> {
        let mut repos = Vec::new();

        for entry in &forest_config.repo {
            if !entry.path.exists() {
                eprintln!(
                    "warning: repo path {} does not exist, skipping",
                    entry.path.display()
                );
                continue;
            }

            let cfg = crate::config::load_config(&entry.path).unwrap_or_default();
            let manager = Manager::new(entry.path.clone(), cfg);
            let worktrees = manager.list().unwrap_or_default();
            let stats = forest::index::compute_repo_stats(&entry.path);

            repos.push(RepoState {
                name: entry.name.clone(),
                path: entry.path.clone(),
                manager,
                worktrees,
                stats,
            });
        }

        let mut repo_list_state = ListState::default();
        if !repos.is_empty() {
            repo_list_state.select(Some(0));
        }

        let mut worktree_list_state = ListState::default();
        if !repos.is_empty() && !repos[0].worktrees.is_empty() {
            worktree_list_state.select(Some(0));
        }

        let mut app = Self {
            repos,
            repo_list_state,
            worktree_list_state,
            focus: ForestFocusPanel::RepoList,
            dialog: ActiveDialog::None,
            filter: String::new(),
            filter_mode: false,
            status_message: String::new(),
            inspector_info: ui::inspector::InspectorInfo::default(),
            inspector_scroll: 0,
            should_quit: false,
            total_running: 0,
            total_waiting: 0,
            total_done: 0,
            last_repo_list_area: None,
            last_wt_list_area: None,
            help_scroll: 0,
            permission_override: None,
            provider_override: None,
            suppress_delete_until: None,
            awaiting_focus_return_after_create: false,
        };

        app.update_aggregate_counts();
        app.update_inspector();
        Ok(app)
    }

    /// Get the currently selected repo index.
    pub fn selected_repo_index(&self) -> Option<usize> {
        self.repo_list_state.selected()
    }

    /// Get the currently selected repo state.
    pub fn selected_repo(&self) -> Option<&RepoState> {
        self.selected_repo_index().and_then(|i| self.repos.get(i))
    }

    /// Get the currently selected repo state mutably.
    pub fn selected_repo_mut(&mut self) -> Option<&mut RepoState> {
        let idx = self.repo_list_state.selected()?;
        self.repos.get_mut(idx)
    }

    /// Get the currently selected worktree.
    pub fn selected_worktree(&self) -> Option<&Worktree> {
        let repo = self.selected_repo()?;
        let filtered = self.filtered_worktrees_for(repo);
        self.worktree_list_state
            .selected()
            .and_then(|i| filtered.get(i).copied())
    }

    /// Get filtered worktree list for a given repo.
    fn filtered_worktrees_for<'a>(&self, repo: &'a RepoState) -> Vec<&'a Worktree> {
        if self.filter.is_empty() {
            repo.worktrees.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            repo.worktrees
                .iter()
                .filter(|wt| wt.name.to_lowercase().contains(&filter_lower))
                .collect()
        }
    }

    /// Get the currently selected repo's manager.
    pub fn selected_manager(&self) -> Option<&Manager> {
        self.selected_repo().map(|r| &r.manager)
    }

    /// Update aggregate counts across all repos.
    pub fn update_aggregate_counts(&mut self) {
        self.total_running = 0;
        self.total_waiting = 0;
        self.total_done = 0;

        for repo in &self.repos {
            self.total_running += repo.stats.running_sessions;
            self.total_waiting += repo.stats.waiting_sessions;
            self.total_done += repo.stats.done_sessions;
        }
    }

    /// Get the active permission level for the selected repo.
    fn active_permission(&self) -> PermissionLevel {
        if let Some(ovr) = self.permission_override {
            return ovr;
        }
        // Use the selected repo's config default, or fall back to Normal
        self.selected_repo()
            .map(|r| r.manager.config.session.default_permission)
            .unwrap_or_default()
    }

    fn active_provider(&self) -> SessionProvider {
        if let Some(ovr) = self.provider_override {
            return ovr;
        }
        self.selected_repo()
            .map(|r| r.manager.config.session.provider)
            .unwrap_or_default()
    }

    /// Refresh worktree lists and stats for all repos.
    pub fn refresh(&mut self) {
        for repo in &mut self.repos {
            if let Ok(mut worktrees) = repo.manager.list() {
                for wt in &mut worktrees {
                    // Don't override "Shipping" status — it's managed by the ship pipeline
                    if wt.status == WorktreeStatus::Shipping {
                        continue;
                    }

                    let new_status = session::tracker::check_status(wt.tmux_pane.as_deref());

                    if wt.status == WorktreeStatus::Running && new_status == WorktreeStatus::Done {
                        mark_session_done(&repo.manager, wt);
                        continue;
                    }

                    wt.status = new_status;
                }
                repo.worktrees = worktrees;

                // Persist status changes (best-effort)
                if let Ok(mut state) = repo.manager.load_state() {
                    for wt in &repo.worktrees {
                        if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                            stored.status = wt.status.clone();
                            stored.tmux_pane = wt.tmux_pane.clone();
                            if wt.last_session_id.is_some() {
                                stored.last_session_id = wt.last_session_id.clone();
                            }
                        }
                    }
                    let _ = repo.manager.save_state(&state);
                }
            }

            repo.stats = forest::index::compute_repo_stats(&repo.path);
        }

        self.update_aggregate_counts();
    }

    /// Update inspector info for the currently selected worktree.
    pub fn update_inspector(&mut self) {
        let info = if let (Some(repo), Some(wt)) = (self.selected_repo(), self.selected_worktree())
        {
            let wt_abs = repo.manager.worktree_abs_path(wt);
            let diff_stat_text = git::diff::diff_stat(&wt_abs)
                .map(|s| s.raw)
                .unwrap_or_default();

            let project_dir = session::tracker::find_project_dir(&wt_abs).ok().flatten();

            let transcript_info = project_dir
                .as_ref()
                .and_then(|dir| session::transcript::read_transcript_info(dir, 1).ok())
                .unwrap_or_default();

            let session_id = project_dir
                .as_ref()
                .and_then(|dir| session::tracker::find_latest_session_id(dir).ok().flatten());

            ui::inspector::InspectorInfo {
                diff_stat_text,
                last_message: transcript_info.last_message,
                usage: transcript_info.usage,
                session_id,
            }
        } else {
            ui::inspector::InspectorInfo::default()
        };

        self.inspector_info = info;
        self.inspector_scroll = 0;
    }

    /// Render the full forest mode UI.
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let (top_bar_area, repo_area, wt_area, inspector_area, status_area) =
            ui::layout::forest_layout(frame.area());

        // Store areas for mouse click handling
        self.last_repo_list_area = Some(repo_area);
        self.last_wt_list_area = Some(wt_area);

        // Render the forest top bar
        let selected_name = self.selected_repo().map(|r| r.name.as_str());
        ui::layout::render_forest_top_bar(
            frame,
            top_bar_area,
            selected_name,
            self.repos.len(),
            self.total_running,
            self.total_waiting,
            self.total_done,
        );

        // Render the repo list
        let repo_displays: Vec<ui::repo_list::RepoDisplay> = self
            .repos
            .iter()
            .map(|r| ui::repo_list::RepoDisplay {
                name: r.name.clone(),
                path: r.path.to_string_lossy().to_string(),
                stats: r.stats.clone(),
            })
            .collect();

        ui::repo_list::render(
            frame,
            repo_area,
            &repo_displays,
            &mut self.repo_list_state,
            self.focus == ForestFocusPanel::RepoList,
        );

        // Render the worktree list for the selected repo
        // Use direct indexing to avoid borrow conflicts with worktree_list_state
        let selected_idx = self.repo_list_state.selected();
        let wt_focus = self.focus == ForestFocusPanel::WorktreeList;
        let insp_focus = self.focus == ForestFocusPanel::Inspector;
        let filter_ref = self.filter.clone();
        let filter_mode = self.filter_mode;

        if let Some(idx) = selected_idx {
            if let Some(repo) = self.repos.get(idx) {
                ui::worktree_list::render(
                    frame,
                    wt_area,
                    &repo.worktrees,
                    &mut self.worktree_list_state,
                    wt_focus,
                    &filter_ref,
                    filter_mode,
                );
            } else {
                ui::worktree_list::render(
                    frame,
                    wt_area,
                    &[],
                    &mut self.worktree_list_state,
                    wt_focus,
                    &filter_ref,
                    filter_mode,
                );
            }
        } else {
            ui::worktree_list::render(
                frame,
                wt_area,
                &[],
                &mut self.worktree_list_state,
                wt_focus,
                &filter_ref,
                filter_mode,
            );
        }

        // Render the inspector — resolve selected worktree via direct indexing
        let selected_wt: Option<&Worktree> = selected_idx
            .and_then(|ri| self.repos.get(ri))
            .and_then(|repo| {
                let filtered: Vec<&Worktree> = if self.filter.is_empty() {
                    repo.worktrees.iter().collect()
                } else {
                    let fl = self.filter.to_lowercase();
                    repo.worktrees
                        .iter()
                        .filter(|wt| wt.name.to_lowercase().contains(&fl))
                        .collect()
                };
                self.worktree_list_state
                    .selected()
                    .and_then(|wi| filtered.get(wi).copied())
            });

        ui::inspector::render(
            frame,
            inspector_area,
            selected_wt,
            &self.inspector_info,
            insp_focus,
            self.inspector_scroll,
        );

        // Render the status bar — show selected repo's worktree count
        let repo_wt_count = selected_idx
            .and_then(|idx| self.repos.get(idx))
            .map(|r| r.worktrees.len())
            .unwrap_or(0);
        let selected_ctx = selected_wt.and_then(|wt| wt.context_usage_percent);
        ui::status_bar::render(
            frame,
            status_area,
            &self.status_message,
            repo_wt_count,
            selected_ctx,
        );

        // Render active dialog on top
        match &self.dialog {
            ActiveDialog::None => {}
            ActiveDialog::Create(d) => ui::dialogs::create::render(frame, d),
            ActiveDialog::Delete(d) => ui::dialogs::delete::render(frame, d),
            ActiveDialog::Handoff(d) => ui::dialogs::handoff::render(frame, d),
            ActiveDialog::Gc(d) => ui::dialogs::gc::render(frame, d),
            ActiveDialog::Restore(d) => ui::dialogs::restore::render(frame, d),
            ActiveDialog::Dispatch(d) => ui::dialogs::dispatch::render(frame, d),
            ActiveDialog::Broadcast(d) => ui::dialogs::broadcast::render(frame, d),
            ActiveDialog::Ship(d) => ui::dialogs::ship::render(frame, d),
            ActiveDialog::Help => ui::help::render(frame, self.help_scroll),
        }
    }

    /// Handle a single tick: poll for events and process them.
    pub fn tick(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(250))? {
            self.handle_event(event::read()?)?;
        }
        Ok(())
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(key) => {
                if should_process_key_event(&key) {
                    self.handle_key(key)?;
                }
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
            }
            Event::FocusGained => {
                if refresh_post_create_delete_guard_on_focus_return(
                    &mut self.suppress_delete_until,
                    &mut self.awaiting_focus_return_after_create,
                ) {
                    let _ = drain_pending_terminal_events();
                }
            }
            Event::FocusLost => {}
            _ => {}
        }
        Ok(())
    }

    /// Handle mouse events.
    fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is in the repo list area
                if let Some(repo_area) = self.last_repo_list_area {
                    if mouse.column >= repo_area.x
                        && mouse.column < repo_area.x + repo_area.width
                        && mouse.row >= repo_area.y
                        && mouse.row < repo_area.y + repo_area.height
                    {
                        let relative_row = mouse.row.saturating_sub(repo_area.y + 1);
                        let index = relative_row as usize;
                        if index < self.repos.len() {
                            self.repo_list_state.select(Some(index));
                            self.focus = ForestFocusPanel::RepoList;
                            self.on_repo_changed();
                        }
                        return;
                    }
                }

                // Check if click is in the worktree list area
                if let Some(wt_area) = self.last_wt_list_area {
                    if mouse.column >= wt_area.x
                        && mouse.column < wt_area.x + wt_area.width
                        && mouse.row >= wt_area.y
                        && mouse.row < wt_area.y + wt_area.height
                    {
                        if let Some(repo) = self.selected_repo() {
                            let relative_row = mouse.row.saturating_sub(wt_area.y + 1);
                            let filtered = self.filtered_worktrees_for(repo);
                            let index = relative_row as usize;
                            if index < filtered.len() {
                                self.worktree_list_state.select(Some(index));
                                self.focus = ForestFocusPanel::WorktreeList;
                                self.update_inspector();
                            }
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if self.focus == ForestFocusPanel::Inspector {
                    self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
                } else if self.focus == ForestFocusPanel::WorktreeList {
                    self.move_wt_selection(-1);
                } else {
                    self.move_repo_selection(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if self.focus == ForestFocusPanel::Inspector {
                    self.inspector_scroll = self.inspector_scroll.saturating_add(1);
                } else if self.focus == ForestFocusPanel::WorktreeList {
                    self.move_wt_selection(1);
                } else {
                    self.move_repo_selection(1);
                }
            }
            _ => {}
        }
    }

    /// Route key events to the appropriate handler.
    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        // If in filter mode, handle filter input
        if self.filter_mode {
            return self.handle_filter_key(key);
        }

        // If a dialog is active, route to dialog handler
        match &self.dialog {
            ActiveDialog::None => self.handle_global_key(key),
            ActiveDialog::Help => {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.help_scroll = self.help_scroll.saturating_add(1);
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                    }
                    _ => {
                        self.dialog = ActiveDialog::None;
                        self.help_scroll = 0;
                    }
                }
                Ok(())
            }
            ActiveDialog::Create(_) => self.handle_create_key(key),
            ActiveDialog::Delete(_) => self.handle_delete_key(key),
            ActiveDialog::Handoff(_) => self.handle_handoff_key(key),
            ActiveDialog::Gc(_) => self.handle_gc_key(key),
            ActiveDialog::Restore(_) => self.handle_restore_key(key),
            ActiveDialog::Dispatch(_) => self.handle_dispatch_key(key),
            ActiveDialog::Broadcast(_) => self.handle_broadcast_key(key),
            ActiveDialog::Ship(_) => self.handle_ship_key(key),
        }
    }

    /// Handle keys when no dialog is active.
    fn handle_global_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.dialog = ActiveDialog::Help;
            }
            // R switches to repo panel
            KeyCode::Char('R') => {
                self.focus = ForestFocusPanel::RepoList;
            }
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                ForestFocusPanel::RepoList => self.move_repo_selection(1),
                ForestFocusPanel::WorktreeList => self.move_wt_selection(1),
                ForestFocusPanel::Inspector => {
                    self.inspector_scroll = self.inspector_scroll.saturating_add(1);
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                ForestFocusPanel::RepoList => self.move_repo_selection(-1),
                ForestFocusPanel::WorktreeList => self.move_wt_selection(-1),
                ForestFocusPanel::Inspector => {
                    self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
                }
            },
            KeyCode::Tab => {
                self.focus = match self.focus {
                    ForestFocusPanel::RepoList => ForestFocusPanel::WorktreeList,
                    ForestFocusPanel::WorktreeList => ForestFocusPanel::Inspector,
                    ForestFocusPanel::Inspector => ForestFocusPanel::RepoList,
                };
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    ForestFocusPanel::RepoList => ForestFocusPanel::Inspector,
                    ForestFocusPanel::WorktreeList => ForestFocusPanel::RepoList,
                    ForestFocusPanel::Inspector => ForestFocusPanel::WorktreeList,
                };
            }
            KeyCode::Char('/') => {
                self.filter_mode = true;
                self.filter.clear();
                self.status_message = "Filter: ".to_string();
            }
            KeyCode::Enter => {
                // If on repo list, switch focus to worktree list
                if self.focus == ForestFocusPanel::RepoList {
                    self.focus = ForestFocusPanel::WorktreeList;
                } else {
                    self.launch_session()?;
                }
            }
            KeyCode::Char('e') => {
                self.open_shell()?;
            }
            // Worktree actions (only work when a worktree can be resolved)
            KeyCode::Char('n') => {
                self.open_create_dialog()?;
            }
            KeyCode::Char('s') => {
                self.launch_session()?;
            }
            KeyCode::Char('d') => {
                if !should_ignore_delete_shortcut(&mut self.suppress_delete_until) {
                    self.open_delete_dialog()?;
                }
            }
            KeyCode::Char('h') => {
                self.open_handoff_dialog()?;
            }
            KeyCode::Char('p') => {
                self.promote_selected()?;
            }
            KeyCode::Char('g') => {
                self.open_gc_dialog()?;
            }
            KeyCode::Char('r') => {
                self.open_restore_dialog()?;
            }
            KeyCode::Char('t') => {
                self.open_dispatch_dialog()?;
            }
            KeyCode::Char('b') => {
                self.open_broadcast_dialog()?;
            }
            KeyCode::Char('P') => {
                self.open_ship_dialog()?;
            }
            KeyCode::Char('S') => {
                self.execute_ship()?;
            }
            KeyCode::Char('c') => {
                self.open_ci_logs()?;
            }
            KeyCode::Char('m') => {
                let current = self.active_permission();
                let next = current.cycle_next();
                self.permission_override = Some(next);
                self.status_message = format!(
                    "Mode: {} ({}) for {}",
                    self.active_provider().mode_label(next),
                    next.short_label(),
                    self.active_provider().label()
                );
            }
            KeyCode::Char('o') => {
                let current = self.active_provider();
                let next = current.cycle_next();
                self.provider_override = Some(next);
                self.status_message = format!("Provider: {}", next.label());
            }
            KeyCode::Char('M') => {
                if let Some(repo) = self.selected_repo() {
                    let repo_root = repo.manager.repo_root.clone();
                    let path = config::project_config_path(&repo_root);
                    let level = self.active_permission();
                    let mut cfg = repo.manager.config.clone();
                    cfg.session.default_permission = level;
                    match config::save_config(&cfg, &path) {
                        Ok(()) => {
                            self.status_message = format!(
                                "Saved default permission '{}' to {}",
                                level.label(),
                                path.display()
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to save config: {}", e);
                        }
                    }
                } else {
                    self.status_message = "No repo selected".to_string();
                }
            }
            KeyCode::Char('O') => {
                if let Some(repo) = self.selected_repo() {
                    let repo_root = repo.manager.repo_root.clone();
                    let path = config::project_config_path(&repo_root);
                    let provider = self.active_provider();
                    let mut cfg = repo.manager.config.clone();
                    cfg.session.provider = provider;
                    match config::save_config(&cfg, &path) {
                        Ok(()) => {
                            self.status_message = format!(
                                "Saved default provider '{}' to {}",
                                provider.label(),
                                path.display()
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to save config: {}", e);
                        }
                    }
                } else {
                    self.status_message = "No repo selected".to_string();
                }
            }
            KeyCode::Esc => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                    self.status_message.clear();
                    self.clamp_wt_selection();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle keys in filter mode.
    fn handle_filter_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.filter_mode = false;
                self.filter.clear();
                self.status_message.clear();
                self.clamp_wt_selection();
            }
            KeyCode::Enter => {
                self.filter_mode = false;
                if self.filter.is_empty() {
                    self.status_message.clear();
                } else {
                    self.status_message = format!("Filter active: {} (Esc to clear)", self.filter);
                }
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.status_message = format!("Filter: {}_", self.filter);
                self.clamp_wt_selection();
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.status_message = format!("Filter: {}_", self.filter);
                self.clamp_wt_selection();
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle keys in the create dialog.
    fn handle_create_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Create(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            KeyCode::Tab | KeyCode::Down => {
                dialog.next_field();
            }
            KeyCode::BackTab | KeyCode::Up => {
                dialog.prev_field();
            }
            KeyCode::Enter => {
                if dialog.focus == dialog.confirm_field() {
                    dialog.confirmed = true;
                } else if dialog.focus == 0 && dialog.name_input.is_empty() {
                    // Quick-create: Enter on empty name → create with all defaults
                    dialog.confirmed = true;
                } else {
                    dialog.next_field();
                }
            }
            KeyCode::Left if dialog.focus == 1 => {
                dialog.prev_branch();
            }
            KeyCode::Right if dialog.focus == 1 => {
                dialog.next_branch();
            }
            KeyCode::Left if dialog.remote_field() == Some(dialog.focus) => {
                dialog.prev_remote();
            }
            KeyCode::Right if dialog.remote_field() == Some(dialog.focus) => {
                dialog.next_remote();
            }
            KeyCode::Char(' ') if dialog.focus == dialog.carry_field() => {
                dialog.toggle_carry();
            }
            KeyCode::Backspace if dialog.focus == 0 => {
                dialog.name_input.pop();
            }
            KeyCode::Char(c) if dialog.focus == 0 => {
                dialog.name_input.push(c);
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            let name = if dialog_clone.name_input.is_empty() {
                None
            } else {
                Some(dialog_clone.name_input.as_str())
            };

            if let Some(repo) = self.selected_repo() {
                // Note: remote creation not yet supported in forest mode — use local
                match repo.manager.create(
                    name,
                    &dialog_clone.base_branch,
                    dialog_clone.carry_changes,
                ) {
                    Ok(wt) => {
                        self.status_message = format!("Created worktree '{}'", wt.name);
                        self.refresh();
                        self.update_inspector();
                        arm_post_create_delete_guard(
                            &mut self.suppress_delete_until,
                            &mut self.awaiting_focus_return_after_create,
                        );
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            } else {
                self.status_message = "No repo selected".to_string();
            }
            self.dialog = ActiveDialog::None;
            let _ = drain_pending_terminal_events();
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
            let _ = drain_pending_terminal_events();
        }

        Ok(())
    }

    /// Handle keys in the delete dialog.
    fn handle_delete_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Delete(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('y') => {
                dialog.confirmed = true;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let Some(repo) = self.selected_repo() {
                match repo.manager.delete(&dialog_clone.worktree_name) {
                    Ok(()) => {
                        self.status_message =
                            format!("Deleted '{}' (snapshot saved)", dialog_clone.worktree_name);
                        self.refresh();
                        self.clamp_wt_selection();
                        self.update_inspector();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the handoff dialog.
    fn handle_handoff_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Handoff(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Tab => {
                dialog.toggle_direction();
            }
            KeyCode::Enter => {
                dialog.confirmed = true;
            }
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let (Some(repo), Some(wt)) = (self.selected_repo(), self.selected_worktree()) {
                let wt_abs = repo.manager.worktree_abs_path(wt);
                match handoff::execute(
                    dialog_clone.direction,
                    &wt_abs,
                    &repo.manager.repo_root,
                    dialog_clone.base_commit.as_deref(),
                ) {
                    Ok(()) => {
                        let dir_str = match dialog_clone.direction {
                            HandoffDirection::WorktreeToLocal => "worktree -> local",
                            HandoffDirection::LocalToWorktree => "local -> worktree",
                        };
                        self.status_message = format!("Handoff complete ({})", dir_str);
                        self.update_inspector();
                    }
                    Err(e) => {
                        self.status_message = format!("Handoff error: {}", e);
                    }
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the GC dialog.
    fn handle_gc_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Gc(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('y') => {
                dialog.confirmed = true;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed && !dialog_clone.to_prune.is_empty() {
            // Global GC: prune across all repos
            let mut total_deleted = 0;
            for repo in &self.repos {
                // Filter prune names that belong to this repo
                let repo_prune: Vec<String> = dialog_clone
                    .to_prune
                    .iter()
                    .filter(|name| repo.worktrees.iter().any(|wt| &wt.name == *name))
                    .cloned()
                    .collect();
                if !repo_prune.is_empty() {
                    match repo.manager.gc_execute(&repo_prune) {
                        Ok(deleted) => total_deleted += deleted.len(),
                        Err(e) => {
                            eprintln!("GC error for {}: {}", repo.name, e);
                        }
                    }
                }
            }
            self.status_message = format!("GC complete: {} worktree(s) pruned", total_deleted);
            self.refresh();
            self.clamp_wt_selection();
            self.update_inspector();
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled || dialog_clone.to_prune.is_empty() {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the restore dialog.
    fn handle_restore_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Restore(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                dialog.move_selection(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                dialog.move_selection(-1);
            }
            KeyCode::Enter => {
                if !dialog.snapshots.is_empty() {
                    dialog.confirmed = true;
                }
            }
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let Some(snap) = dialog_clone.selected_snapshot() {
                if let Some(repo) = self.selected_repo() {
                    match repo.manager.restore_snapshot(snap) {
                        Ok(wt) => {
                            self.status_message = format!("Restored '{}' from snapshot", wt.name);
                            self.refresh();
                            self.update_inspector();
                        }
                        Err(e) => {
                            self.status_message = format!("Restore error: {}", e);
                        }
                    }
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    // --- Navigation methods ---

    fn move_repo_selection(&mut self, delta: i32) {
        let count = self.repos.len();
        if count == 0 {
            return;
        }
        let current = self.repo_list_state.selected().unwrap_or(0);
        let new = if delta > 0 {
            (current + 1).min(count - 1)
        } else {
            current.saturating_sub(1)
        };
        if new != current {
            self.repo_list_state.select(Some(new));
            self.on_repo_changed();
        }
    }

    fn move_wt_selection(&mut self, delta: i32) {
        let count = self
            .selected_repo()
            .map(|r| self.filtered_worktrees_for(r).len())
            .unwrap_or(0);
        if count == 0 {
            return;
        }
        let current = self.worktree_list_state.selected().unwrap_or(0);
        let new = if delta > 0 {
            (current + 1).min(count - 1)
        } else {
            current.saturating_sub(1)
        };
        self.worktree_list_state.select(Some(new));
        self.update_inspector();
    }

    fn clamp_wt_selection(&mut self) {
        let count = self
            .selected_repo()
            .map(|r| self.filtered_worktrees_for(r).len())
            .unwrap_or(0);
        self.worktree_list_state.select(clamp_selected_index(
            self.worktree_list_state.selected(),
            count,
        ));
    }

    /// Called when the selected repo changes — reset worktree selection.
    fn on_repo_changed(&mut self) {
        self.filter.clear();
        self.filter_mode = false;
        if let Some(repo) = self.selected_repo() {
            if repo.worktrees.is_empty() {
                self.worktree_list_state.select(None);
            } else {
                self.worktree_list_state.select(Some(0));
            }
        } else {
            self.worktree_list_state.select(None);
        }
        self.update_inspector();
    }

    // --- Action methods ---

    fn open_create_dialog(&mut self) -> Result<()> {
        let Some(repo) = self.selected_repo() else {
            self.status_message = "No repo selected".to_string();
            return Ok(());
        };
        let branches: Vec<String> = git::branch::list_branches(&repo.manager.repo_root)?
            .into_iter()
            .map(|b| b.name)
            .collect();
        let dialog = ui::dialogs::create::CreateDialog::new(branches);
        self.dialog = ActiveDialog::Create(dialog);
        Ok(())
    }

    fn open_delete_dialog(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };
        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };

        let wt_abs = repo.manager.worktree_abs_path(&wt);
        let diff_preview = git::diff::diff_stat(&wt_abs)
            .map(|s| s.raw)
            .unwrap_or_else(|_| "Unable to get diff".to_string());

        let dialog = ui::dialogs::delete::DeleteDialog::new(wt.name.clone(), diff_preview);
        self.dialog = ActiveDialog::Delete(dialog);
        Ok(())
    }

    fn open_handoff_dialog(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };
        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };

        let wt_abs = repo.manager.worktree_abs_path(&wt);
        let base_commit = Some(wt.base_commit.clone());

        let preview = handoff::preview(
            HandoffDirection::WorktreeToLocal,
            &wt_abs,
            &repo.manager.repo_root,
            base_commit.as_deref(),
        );

        let (stat_raw, files_changed, has_commits, commit_count, gitignore_warnings) = match preview
        {
            Ok(p) => (
                p.diff_stat.raw,
                p.diff_stat.files_changed,
                p.has_commits,
                p.commit_count,
                p.gitignore_warnings,
            ),
            Err(_) => {
                let stat = git::diff::diff_stat(&wt_abs).unwrap_or_default();
                (stat.raw, stat.files_changed, false, 0, Vec::new())
            }
        };

        let dialog = ui::dialogs::handoff::HandoffDialog::new(
            wt.name.clone(),
            stat_raw,
            files_changed,
            has_commits,
            commit_count,
            gitignore_warnings,
            base_commit,
        );
        self.dialog = ActiveDialog::Handoff(dialog);
        Ok(())
    }

    fn open_gc_dialog(&mut self) -> Result<()> {
        // Global GC: collect prune candidates from all repos
        let mut all_to_prune = Vec::new();
        for repo in &self.repos {
            if let Ok(to_prune) = repo.manager.gc_preview() {
                all_to_prune.extend(to_prune);
            }
        }
        let dialog = ui::dialogs::gc::GcDialog::new(all_to_prune);
        self.dialog = ActiveDialog::Gc(dialog);
        Ok(())
    }

    fn open_restore_dialog(&mut self) -> Result<()> {
        let Some(repo) = self.selected_repo() else {
            self.status_message = "No repo selected".to_string();
            return Ok(());
        };
        let snapshots = repo.manager.list_snapshots()?;
        let dialog = ui::dialogs::restore::RestoreDialog::new(snapshots);
        self.dialog = ActiveDialog::Restore(dialog);
        Ok(())
    }

    fn launch_session(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        // If there's an existing pane, try to focus it
        if let Some(ref pane_id) = wt.tmux_pane {
            if crate::tmux::pane::pane_exists(pane_id) {
                if let Ok(()) = session::launcher::focus_session(pane_id) {
                    self.status_message = format!("Focused session for '{}'", wt.name);
                    return Ok(());
                }
            }
        }

        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };
        let wt_abs = repo.manager.worktree_abs_path(&wt);

        let session_id = wt.last_session_id.clone().or_else(|| {
            session::tracker::find_project_dir(&wt_abs)
                .ok()
                .flatten()
                .and_then(|dir| {
                    session::tracker::find_latest_session_id(&dir)
                        .ok()
                        .flatten()
                })
        });

        let permission = self.active_permission();
        let permissions = &repo.manager.config.session.permissions;
        let mut session_cfg = repo.manager.config.session.clone();
        session_cfg.provider = self.active_provider();
        let launch_result = if let Some(ref sid) = session_id {
            session::launcher::resume_session(
                &wt,
                &wt_abs,
                sid,
                &session_cfg,
                permission,
                permissions,
            )
        } else {
            session::launcher::launch_session(&wt, &wt_abs, &session_cfg, permission, permissions)
        };

        match launch_result {
            Ok(pane_id) => {
                let action = if session_id.is_some() {
                    "Resumed"
                } else {
                    "Launched"
                };

                if let Ok(mut state) = repo.manager.load_state() {
                    if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                        stored.tmux_pane = Some(pane_id.clone());
                        stored.status = WorktreeStatus::Running;
                        if session_id.is_some() {
                            stored.last_session_id = session_id.clone();
                        }
                    }
                    let _ = repo.manager.save_state(&state);
                }

                self.status_message = format!(
                    "{} session for '{}' [{}] ({})",
                    action,
                    wt.name,
                    permission.label(),
                    pane_id,
                );
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("Session error: {}", e);
            }
        }

        Ok(())
    }

    fn promote_selected(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };
        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };

        let name = wt.name.clone();
        match repo.manager.promote(&name) {
            Ok(true) => {
                self.status_message = format!("Promoted '{}' to permanent", name);
                self.refresh();
                self.update_inspector();
            }
            Ok(false) => {
                self.status_message = format!("'{}' is already permanent", name);
            }
            Err(e) => {
                self.status_message = format!("Error: {}", e);
            }
        }

        Ok(())
    }

    fn open_dispatch_dialog(&mut self) -> Result<()> {
        let Some(repo) = self.selected_repo() else {
            self.status_message = "No repo selected".to_string();
            return Ok(());
        };
        let branches: Vec<String> = git::branch::list_branches(&repo.manager.repo_root)?
            .into_iter()
            .map(|b| b.name)
            .collect();
        let dialog = ui::dialogs::dispatch::DispatchDialog::new(branches);
        self.dialog = ActiveDialog::Dispatch(dialog);
        Ok(())
    }

    fn open_broadcast_dialog(&mut self) -> Result<()> {
        let Some(repo) = self.selected_repo() else {
            self.status_message = "No repo selected".to_string();
            return Ok(());
        };
        let running: Vec<String> = repo
            .worktrees
            .iter()
            .filter(|wt| wt.status == WorktreeStatus::Running && wt.tmux_pane.is_some())
            .map(|wt| wt.name.clone())
            .collect();
        let count = running.len();

        if count == 0 {
            self.status_message = "No running sessions to broadcast to".to_string();
            return Ok(());
        }

        let dialog = ui::dialogs::broadcast::BroadcastDialog::new(count, running);
        self.dialog = ActiveDialog::Broadcast(dialog);
        Ok(())
    }

    /// Handle keys in the dispatch dialog.
    fn handle_dispatch_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Dispatch(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            KeyCode::Tab => {
                if dialog.focus == 0 && !dialog.current_input.trim().is_empty() {
                    dialog.add_task();
                }
                dialog.next_field();
            }
            KeyCode::BackTab => {
                dialog.prev_field();
            }
            KeyCode::Enter => {
                if dialog.focus == 0 {
                    dialog.add_task();
                } else if dialog.focus == 2 {
                    if !dialog.current_input.trim().is_empty() {
                        dialog.add_task();
                    }
                    if !dialog.tasks.is_empty() {
                        dialog.confirmed = true;
                    }
                } else {
                    dialog.next_field();
                }
            }
            KeyCode::Left if dialog.focus == 1 => {
                dialog.prev_branch();
            }
            KeyCode::Right if dialog.focus == 1 => {
                dialog.next_branch();
            }
            KeyCode::Backspace if dialog.focus == 0 => {
                if dialog.current_input.is_empty() {
                    dialog.remove_last_task();
                } else {
                    dialog.current_input.pop();
                }
            }
            KeyCode::Char(c) if dialog.focus == 0 => {
                dialog.current_input.push(c);
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            let tasks = dialog_clone.tasks;
            let base = dialog_clone.base_branch;

            if tasks.is_empty() {
                self.status_message = "No tasks to dispatch".to_string();
            } else if let Some(repo) = self.selected_repo() {
                let permission = self.active_permission();
                let results = orchestration::dispatch::dispatch_tasks(
                    &repo.manager,
                    &tasks,
                    &base,
                    permission,
                );
                let success_count = results.iter().filter(|r| r.error.is_none()).count();
                let fail_count = results.iter().filter(|r| r.error.is_some()).count();

                if fail_count == 0 {
                    self.status_message =
                        format!("Dispatched {} task(s) successfully", success_count);
                } else {
                    self.status_message = format!(
                        "Dispatched {} task(s), {} failed",
                        success_count, fail_count
                    );
                }

                self.refresh();
                self.update_inspector();
            } else {
                self.status_message = "No repo selected".to_string();
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Handle keys in the broadcast dialog.
    fn handle_broadcast_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Broadcast(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            KeyCode::Enter => {
                if !dialog.prompt_input.trim().is_empty() && dialog.target_count > 0 {
                    dialog.confirmed = true;
                }
            }
            KeyCode::Backspace => {
                dialog.prompt_input.pop();
            }
            KeyCode::Char(c) => {
                dialog.prompt_input.push(c);
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            let prompt = dialog_clone.prompt_input.trim().to_string();
            if let Some(repo) = self.selected_repo() {
                let results = orchestration::broadcast::broadcast_prompt(&repo.worktrees, &prompt);
                let success_count = results.iter().filter(|r| r.success).count();
                let fail_count = results.iter().filter(|r| !r.success).count();

                if fail_count == 0 {
                    self.status_message = format!("Broadcast sent to {} session(s)", success_count);
                } else {
                    self.status_message =
                        format!("Broadcast: {} sent, {} failed", success_count, fail_count);
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    fn open_shell(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };
        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };

        if !crate::tmux::pane::is_inside_tmux() {
            self.status_message = "zellij or tmux required for shell panes".to_string();
            return Ok(());
        }

        let wt_abs = repo.manager.worktree_abs_path(&wt);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        let title = format!("cwt:shell:{}", wt.name);

        match crate::tmux::pane::create_pane(&wt_abs, &shell, &title) {
            Ok(pane_id) => {
                self.status_message = format!("Opened shell for '{}' ({})", wt.name, pane_id);
            }
            Err(e) => {
                self.status_message = format!("Shell error: {}", e);
            }
        }

        Ok(())
    }

    /// Open the ship/PR dialog for the selected worktree.
    fn open_ship_dialog(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };
        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };

        if !ship::pr::gh_available() {
            self.status_message = "gh CLI not found (install: https://cli.github.com/)".to_string();
            return Ok(());
        }

        let wt_abs = repo.manager.worktree_abs_path(&wt);
        let diff_preview = git::diff::diff_stat(&wt_abs)
            .map(|s| s.raw)
            .unwrap_or_else(|_| "Unable to get diff".to_string());
        let files_changed = git::diff::diff_stat(&wt_abs)
            .map(|s| s.files_changed)
            .unwrap_or(0);

        let dialog = ui::dialogs::ship::ShipDialog::new(
            wt.name.clone(),
            wt.branch.clone(),
            wt.base_branch.clone(),
            diff_preview,
            files_changed,
        );
        self.dialog = ActiveDialog::Ship(dialog);
        Ok(())
    }

    /// Handle keys in the ship dialog.
    fn handle_ship_key(&mut self, key: KeyEvent) -> Result<()> {
        let ActiveDialog::Ship(ref mut dialog) = self.dialog else {
            return Ok(());
        };

        match key.code {
            KeyCode::Tab => {
                dialog.toggle_mode();
            }
            KeyCode::Enter => {
                dialog.confirmed = true;
            }
            KeyCode::Esc => {
                dialog.cancelled = true;
            }
            _ => {}
        }

        let dialog_clone = dialog.clone();
        if dialog_clone.confirmed {
            if let (Some(wt), Some(repo)) =
                (self.selected_worktree().cloned(), self.selected_repo())
            {
                let wt_abs = repo.manager.worktree_abs_path(&wt);

                if dialog_clone.mode == 0 {
                    self.do_create_pr(&wt, &wt_abs);
                } else {
                    self.do_ship(&wt, &wt_abs);
                }
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    /// Create a PR for the worktree.
    fn do_create_pr(&mut self, wt: &Worktree, wt_abs: &std::path::Path) {
        match ship::pr::commit_and_push(wt_abs, &wt.branch) {
            Ok(push_msg) => {
                self.status_message = push_msg;
            }
            Err(e) => {
                self.status_message = format!("Push failed: {}", e);
                return;
            }
        }

        let body = ship::pr::generate_pr_body(wt_abs, wt);
        let title = ship::pr::generate_pr_title(wt);

        match ship::pr::create_pr(wt_abs, &wt.branch, &wt.base_branch, &title, &body) {
            Ok(result) => {
                if let Some(repo) = self.selected_repo() {
                    if let Ok(mut state) = repo.manager.load_state() {
                        if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                            stored.pr_number = Some(result.pr_number);
                            stored.pr_url = Some(result.pr_url.clone());
                            stored.pr_status = ship::pr::PrStatus::Open;
                        }
                        let _ = repo.manager.save_state(&state);
                    }
                }

                self.status_message =
                    format!("PR #{} created: {}", result.pr_number, result.pr_url);
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("PR creation failed: {}", e);
            }
        }
    }

    /// Execute the "ship it" flow.
    fn do_ship(&mut self, wt: &Worktree, wt_abs: &std::path::Path) {
        match ship::pipeline::ship(wt, wt_abs) {
            Ok(result) => {
                if let Some(repo) = self.selected_repo() {
                    if let Ok(mut state) = repo.manager.load_state() {
                        if let Some(stored) = state.worktrees.get_mut(&wt.name) {
                            stored.pr_number = Some(result.pr_number);
                            stored.pr_url = Some(result.pr_url.clone());
                            stored.pr_status = ship::pr::PrStatus::Open;
                            stored.status = WorktreeStatus::Shipping;
                        }
                        let _ = repo.manager.save_state(&state);
                    }
                }

                self.status_message =
                    format!("Shipped! PR #{}: {}", result.pr_number, result.pr_url);
                self.refresh();
                self.update_inspector();
            }
            Err(e) => {
                self.status_message = format!("Ship failed: {}", e);
            }
        }
    }

    /// Execute "ship it" macro directly from the S key.
    fn execute_ship(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree().cloned() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        if !ship::pr::gh_available() {
            self.status_message = "gh CLI not found (install: https://cli.github.com/)".to_string();
            return Ok(());
        }

        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };
        let wt_abs = repo.manager.worktree_abs_path(&wt);
        self.do_ship(&wt, &wt_abs);
        Ok(())
    }

    /// Open CI logs in browser.
    fn open_ci_logs(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };
        let Some(repo) = self.selected_repo() else {
            return Ok(());
        };

        if !ship::pr::gh_available() {
            self.status_message = "gh CLI not found".to_string();
            return Ok(());
        }

        match ship::ci::open_ci_logs(&repo.manager.repo_root, &wt.branch) {
            Ok(()) => {
                self.status_message = format!("Opening CI logs for '{}'", wt.name);
            }
            Err(e) => {
                self.status_message = format!("CI logs: {}", e);
            }
        }

        Ok(())
    }

    /// Poll PR/CI status for worktrees that have open PRs across all repos.
    pub fn poll_ship_status(&mut self) {
        if !ship::pr::gh_available() {
            return;
        }

        for repo in &mut self.repos {
            let mut updates: Vec<(
                String,
                ship::pr::PrStatus,
                ship::pr::CiStatus,
                Option<String>,
            )> = Vec::new();
            let mut merged_names: Vec<String> = Vec::new();

            for wt in &repo.worktrees {
                if wt.pr_number.is_none() {
                    continue;
                }

                let (pr_status, ci_status, pr_url) =
                    ship::pipeline::poll_status(&repo.manager.repo_root, wt);

                if pr_status == ship::pr::PrStatus::Merged
                    && wt.pr_status != ship::pr::PrStatus::Merged
                {
                    merged_names.push(wt.name.clone());
                }

                updates.push((wt.name.clone(), pr_status, ci_status, pr_url));
            }

            // Apply updates
            if !updates.is_empty() {
                if let Ok(mut state) = repo.manager.load_state() {
                    for (name, pr_status, ci_status, pr_url) in &updates {
                        if let Some(stored) = state.worktrees.get_mut(name) {
                            stored.pr_status = pr_status.clone();
                            stored.ci_status = ci_status.clone();
                            if let Some(url) = pr_url {
                                stored.pr_url = Some(url.clone());
                            }
                        }
                    }
                    let _ = repo.manager.save_state(&state);
                }

                for (name, pr_status, ci_status, pr_url) in updates {
                    if let Some(wt) = repo.worktrees.iter_mut().find(|wt| wt.name == name) {
                        wt.pr_status = pr_status;
                        wt.ci_status = ci_status;
                        if let Some(url) = pr_url {
                            wt.pr_url = Some(url);
                        }
                    }
                }
            }

            // Auto-cleanup merged worktrees
            for name in merged_names {
                let _ = repo.manager.delete(&name);
            }
        }

        self.refresh();
    }
}

#[cfg(test)]
mod selection_tests {
    use super::{
        arm_post_create_delete_guard, clamp_selected_index, drain_pending_terminal_events_with,
        refresh_post_create_delete_guard_on_focus_return, should_ignore_delete_shortcut,
        should_process_key_event, ActiveDialog, App,
    };
    use crate::config::{Config, ConfigMeta};
    use crate::hooks::event::HookEvent;
    use crate::worktree::model::{Lifecycle, Worktree, WorktreeStatus};
    use crate::worktree::Manager;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command should start");
        assert!(
            out.status.success(),
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr),
        );
    }

    fn make_test_repo() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("create tempdir");
        let root = tmp.path().to_path_buf();

        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@cwt.dev"]);
        run_git(&root, &["config", "user.name", "cwt-test"]);
        std::fs::write(root.join("README.md"), "# test repo\n").expect("write README");
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "initial commit"]);

        (tmp, root)
    }

    fn make_test_app(auto_launch: bool) -> (TempDir, App) {
        let (tmp, root) = make_test_repo();
        let mut cfg = Config::default();
        cfg.session.auto_launch = auto_launch;
        cfg.worktree.dir = tmp.path().join("worktrees").to_string_lossy().into_owned();
        let manager = Manager::new(root, cfg);
        let app = App::new(manager, ConfigMeta::default()).expect("create app");
        (tmp, app)
    }

    fn seed_selected_worktree(app: &mut App, name: &str) {
        app.worktrees.push(Worktree::new(
            name.to_string(),
            PathBuf::from(name),
            format!("wt/{name}"),
            "main".to_string(),
            "HEAD".to_string(),
            Lifecycle::Ephemeral,
        ));
        app.list_state.select(Some(app.worktrees.len() - 1));
    }

    #[test]
    fn clamp_selected_index_handles_empty_lists() {
        assert_eq!(clamp_selected_index(Some(3), 0), None);
        assert_eq!(clamp_selected_index(None, 0), None);
    }

    #[test]
    fn clamp_selected_index_defaults_to_first_item_when_missing_selection() {
        assert_eq!(clamp_selected_index(None, 4), Some(0));
    }

    #[test]
    fn clamp_selected_index_clamps_out_of_bounds_to_last_item() {
        assert_eq!(clamp_selected_index(Some(9), 3), Some(2));
    }

    #[test]
    fn clamp_selected_index_keeps_valid_selection() {
        assert_eq!(clamp_selected_index(Some(1), 3), Some(1));
    }

    #[test]
    fn process_key_events_accepts_presses_and_repeats() {
        let press = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        };
        let repeat = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Repeat,
            state: KeyEventState::NONE,
        };

        assert!(should_process_key_event(&press));
        assert!(should_process_key_event(&repeat));
    }

    #[test]
    fn process_key_events_ignores_key_releases() {
        let release = KeyEvent {
            code: KeyCode::Char('d'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };

        assert!(!should_process_key_event(&release));
    }

    #[test]
    fn ignore_delete_shortcut_shortly_after_create() {
        let mut suppress_delete_until = Some(Instant::now() + Duration::from_secs(2));
        assert!(should_ignore_delete_shortcut(&mut suppress_delete_until));
        assert!(suppress_delete_until.is_some());
        assert!(should_ignore_delete_shortcut(&mut suppress_delete_until));
    }

    #[test]
    fn allow_delete_shortcut_long_after_create() {
        let mut suppress_delete_until = Some(Instant::now() - Duration::from_secs(1));
        assert!(!should_ignore_delete_shortcut(&mut suppress_delete_until));
        assert!(suppress_delete_until.is_none());
    }

    #[test]
    fn session_stopped_event_clears_attached_pane() {
        let (_tmp, mut app) = make_test_app(false);
        let mut wt = Worktree::new(
            "wt-session".to_string(),
            PathBuf::from("wt-session"),
            "wt/wt-session".to_string(),
            "main".to_string(),
            "HEAD".to_string(),
            Lifecycle::Ephemeral,
        );
        wt.status = WorktreeStatus::Running;
        wt.tmux_pane = Some("%12".to_string());
        app.worktrees.push(wt);
        app.list_state.select(Some(0));

        app.handle_hook_event(HookEvent::SessionStopped {
            worktree: "wt-session".to_string(),
            session_id: Some("sess-123".to_string()),
            timestamp: None,
            data: None,
        });

        let wt = app
            .worktrees
            .iter()
            .find(|wt| wt.name == "wt-session")
            .unwrap();
        assert_eq!(wt.status, WorktreeStatus::Done);
        assert_eq!(wt.last_session_id.as_deref(), Some("sess-123"));
        assert_eq!(wt.tmux_pane, None);
    }

    #[test]
    fn arm_post_create_delete_guard_sets_window_and_focus_flag() {
        let mut suppress_delete_until = None;
        let mut awaiting_focus_return = false;

        arm_post_create_delete_guard(&mut suppress_delete_until, &mut awaiting_focus_return);

        assert!(suppress_delete_until.is_some());
        assert!(awaiting_focus_return);
    }

    #[test]
    fn refresh_post_create_delete_guard_on_focus_return_rearms_window() {
        let mut suppress_delete_until = Some(Instant::now() - Duration::from_secs(1));
        let mut awaiting_focus_return = true;

        assert!(refresh_post_create_delete_guard_on_focus_return(
            &mut suppress_delete_until,
            &mut awaiting_focus_return,
        ));
        assert!(suppress_delete_until.is_some());
        assert!(!awaiting_focus_return);
    }

    #[test]
    fn drain_pending_terminal_events_consumes_buffered_input() {
        let events = RefCell::new(VecDeque::from([
            Event::Key(press(KeyCode::Char('d'))),
            Event::Key(press(KeyCode::Enter)),
        ]));

        let drained = drain_pending_terminal_events_with(
            || Ok::<bool, std::io::Error>(!events.borrow().is_empty()),
            || Ok::<Event, std::io::Error>(events.borrow_mut().pop_front().unwrap()),
        )
        .unwrap();

        assert_eq!(drained, 2);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn zellij_like_focus_return_keeps_stray_delete_from_opening_dialog() {
        let (_tmp, mut app) = make_test_app(true);
        seed_selected_worktree(&mut app, "fresh-wt");

        app.suppress_delete_until = Some(Instant::now() - Duration::from_secs(1));
        app.awaiting_focus_return_after_create = true;

        app.handle_event(Event::FocusGained)
            .expect("handle focus gained");
        app.handle_event(Event::Key(press(KeyCode::Char('d'))))
            .expect("handle stray delete key");

        assert!(
            !matches!(app.dialog, ActiveDialog::Delete(_)),
            "delete dialog should not open after zellij-like focus return"
        );
    }

    #[test]
    fn delete_still_opens_after_post_create_focus_guard_expires() {
        let (_tmp, mut app) = make_test_app(false);
        seed_selected_worktree(&mut app, "fresh-wt");

        app.suppress_delete_until = Some(Instant::now() - Duration::from_secs(1));
        app.awaiting_focus_return_after_create = false;

        app.handle_event(Event::Key(press(KeyCode::Char('d'))))
            .expect("handle delete key");

        assert!(matches!(app.dialog, ActiveDialog::Delete(_)));
    }
}
