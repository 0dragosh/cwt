use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::time::Duration;

use crate::git;
use crate::hooks::event::HookEvent;
use crate::orchestration;
use crate::session;
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
    Help,
}

/// Which panel has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPanel {
    WorktreeList,
    Inspector,
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
}

impl App {
    pub fn new(manager: Manager) -> Result<Self> {
        let worktrees = manager.list()?;
        let mut list_state = ListState::default();
        if !worktrees.is_empty() {
            list_state.select(Some(0));
        }

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
                let new_status = session::tracker::check_status(wt.tmux_pane.as_deref());

                // If session just finished (was Running, now Done), clear the pane
                // but preserve last_session_id for potential resume
                if wt.status == WorktreeStatus::Running && new_status == WorktreeStatus::Done {
                    // Try to capture the session ID before clearing
                    if wt.last_session_id.is_none() {
                        let wt_abs = self.manager.worktree_abs_path(wt);
                        if let Ok(Some(dir)) = session::tracker::find_project_dir(&wt_abs) {
                            if let Ok(Some(sid)) =
                                session::tracker::find_latest_session_id(&dir)
                            {
                                wt.last_session_id = Some(sid);
                            }
                        }
                    }
                }

                wt.status = new_status;
            }
            self.worktrees = updated;

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

    /// Update aggregate dashboard stats across all sessions.
    pub fn update_dashboard(&mut self) {
        let manager = &self.manager;
        self.dashboard = orchestration::dashboard::compute_aggregate_stats(
            &self.worktrees,
            |wt| manager.worktree_abs_path(wt),
        );
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
                self.status_message =
                    format!("Worktree '{}' removed externally", worktree);
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
                if let Some(wt) = self
                    .worktrees
                    .iter_mut()
                    .find(|wt| wt.name == worktree)
                {
                    wt.status = WorktreeStatus::Done;
                    if let Some(sid) = session_id {
                        wt.last_session_id = Some(sid);
                    }
                }
                self.status_message =
                    format!("Session stopped for '{}'", worktree);
                self.update_badge_counts();
                self.update_inspector();

                // Persist status change
                self.persist_status_change(&worktree, WorktreeStatus::Done);
            }
            HookEvent::SessionNotification {
                worktree, message, ..
            } => {
                // Update the worktree status to Waiting
                if let Some(wt) = self
                    .worktrees
                    .iter_mut()
                    .find(|wt| wt.name == worktree)
                {
                    wt.status = WorktreeStatus::Waiting;
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
                self.status_message =
                    format!("Subagent stopped in '{}'", worktree);
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

    /// Get the currently selected worktree.
    pub fn selected_worktree(&self) -> Option<&Worktree> {
        let filtered = self.filtered_worktrees();
        self.list_state.selected().and_then(|i| filtered.get(i).copied())
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
            let wt_abs = self.manager.worktree_abs_path(wt);
            let diff_stat_text = git::diff::diff_stat(&wt_abs)
                .map(|s| s.raw)
                .unwrap_or_default();

            let project_dir = session::tracker::find_project_dir(&wt_abs)
                .ok()
                .flatten();

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
        // Reset scroll when changing worktree
        self.inspector_scroll = 0;
    }

    /// Render the full UI.
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let (top_bar_area, list_area, inspector_area, status_area) =
            ui::layout::main_layout(frame.area());

        // Store list area for mouse click handling
        self.last_list_area = Some(list_area);

        // Render the top bar with notification badges and aggregate stats
        let total_tokens = if self.dashboard.total_input_tokens > 0
            || self.dashboard.total_output_tokens > 0
        {
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

        // Render the status bar
        ui::status_bar::render(
            frame,
            status_area,
            &self.status_message,
            self.worktrees.len(),
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
            ActiveDialog::Help => ui::help::render(frame),
        }
    }

    /// Handle a single tick: poll for events and process them.
    pub fn tick(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => {
                    self.handle_key(key)?;
                }
                Event::Mouse(mouse) => {
                    self.handle_mouse(mouse);
                }
                _ => {}
            }
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
                // Any key dismisses help
                self.dialog = ActiveDialog::None;
                Ok(())
            }
            ActiveDialog::Create(_) => self.handle_create_key(key),
            ActiveDialog::Delete(_) => self.handle_delete_key(key),
            ActiveDialog::Handoff(_) => self.handle_handoff_key(key),
            ActiveDialog::Gc(_) => self.handle_gc_key(key),
            ActiveDialog::Restore(_) => self.handle_restore_key(key),
            ActiveDialog::Dispatch(_) => self.handle_dispatch_key(key),
            ActiveDialog::Broadcast(_) => self.handle_broadcast_key(key),
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
                self.open_delete_dialog()?;
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
            KeyCode::Enter => {
                self.open_shell()?;
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
                if dialog.focus == 3 {
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
            KeyCode::Char(' ') if dialog.focus == 2 => {
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

            match self.manager.create(name, &dialog_clone.base_branch, dialog_clone.carry_changes)
            {
                Ok(wt) => {
                    self.status_message = format!("Created worktree '{}'", wt.name);
                    self.refresh();
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
            match self.manager.delete(&dialog_clone.worktree_name) {
                Ok(()) => {
                    self.status_message = format!(
                        "Deleted '{}' (snapshot saved)",
                        dialog_clone.worktree_name
                    );
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
        if count == 0 {
            self.list_state.select(None);
        } else if let Some(i) = self.list_state.selected() {
            if i >= count {
                self.list_state.select(Some(count - 1));
            }
        }
    }

    fn open_create_dialog(&mut self) -> Result<()> {
        let branches: Vec<String> = git::branch::list_branches(&self.manager.repo_root)?
            .into_iter()
            .map(|b| b.name)
            .collect();
        let dialog = ui::dialogs::create::CreateDialog::new(branches);
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

        let (stat_raw, files_changed, has_commits, commit_count, gitignore_warnings) =
            match preview {
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
                    session::tracker::find_latest_session_id(&dir).ok().flatten()
                })
        });

        let launch_result = if let Some(ref sid) = session_id {
            // Try to resume a previous session
            session::launcher::resume_session(
                &wt,
                &wt_abs,
                sid,
                &self.manager.config.session,
            )
        } else {
            // Fresh launch
            session::launcher::launch_session(&wt, &wt_abs, &self.manager.config.session)
        };

        match launch_result {
            Ok(pane_id) => {
                let action = if session_id.is_some() { "Resumed" } else { "Launched" };

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

                self.status_message =
                    format!("{} session for '{}' ({})", action, wt.name, pane_id);
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
            Ok(()) => {
                self.status_message = format!("Promoted '{}' to permanent", name);
                self.refresh();
                self.update_inspector();
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
                        self.status_message =
                            format!("Restored '{}' from snapshot", wt.name);
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
                let results =
                    orchestration::dispatch::dispatch_tasks(&self.manager, &tasks, &base);
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
            let results =
                orchestration::broadcast::broadcast_prompt(&self.worktrees, &prompt);
            let success_count = results.iter().filter(|r| r.success).count();
            let fail_count = results.iter().filter(|r| !r.success).count();

            if fail_count == 0 {
                self.status_message =
                    format!("Broadcast sent to {} session(s)", success_count);
            } else {
                self.status_message = format!(
                    "Broadcast: {} sent, {} failed",
                    success_count, fail_count
                );
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
        }

        Ok(())
    }

    fn open_shell(&mut self) -> Result<()> {
        let Some(wt) = self.selected_worktree() else {
            self.status_message = "No worktree selected".to_string();
            return Ok(());
        };

        if !crate::tmux::pane::is_inside_tmux() {
            self.status_message = "tmux required for shell panes".to_string();
            return Ok(());
        }

        let wt_abs = self.manager.worktree_abs_path(wt);
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
}

impl ForestApp {
    pub fn new(forest_config: &forest::ForestConfig) -> Result<Self> {
        let mut repos = Vec::new();

        for entry in &forest_config.repo {
            if !entry.path.exists() {
                eprintln!("warning: repo path {} does not exist, skipping", entry.path.display());
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

    /// Refresh worktree lists and stats for all repos.
    pub fn refresh(&mut self) {
        for repo in &mut self.repos {
            if let Ok(mut worktrees) = repo.manager.list() {
                for wt in &mut worktrees {
                    let new_status = session::tracker::check_status(wt.tmux_pane.as_deref());

                    if wt.status == WorktreeStatus::Running
                        && new_status == WorktreeStatus::Done
                        && wt.last_session_id.is_none()
                    {
                        let wt_abs = repo.manager.worktree_abs_path(wt);
                        if let Ok(Some(dir)) = session::tracker::find_project_dir(&wt_abs) {
                            if let Ok(Some(sid)) =
                                session::tracker::find_latest_session_id(&dir)
                            {
                                wt.last_session_id = Some(sid);
                            }
                        }
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

            let project_dir = session::tracker::find_project_dir(&wt_abs)
                .ok()
                .flatten();

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
                    repo.worktrees.iter().filter(|wt| wt.name.to_lowercase().contains(&fl)).collect()
                };
                self.worktree_list_state.selected().and_then(|wi| filtered.get(wi).copied())
            });

        ui::inspector::render(
            frame,
            inspector_area,
            selected_wt,
            &self.inspector_info,
            insp_focus,
            self.inspector_scroll,
        );

        // Render the status bar
        let total_wt: usize = self.repos.iter().map(|r| r.worktrees.len()).sum();
        ui::status_bar::render(
            frame,
            status_area,
            &self.status_message,
            total_wt,
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
            ActiveDialog::Help => ui::help::render(frame),
        }
    }

    /// Handle a single tick: poll for events and process them.
    pub fn tick(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => {
                    self.handle_key(key)?;
                }
                Event::Mouse(mouse) => {
                    self.handle_mouse(mouse);
                }
                _ => {}
            }
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
                self.dialog = ActiveDialog::None;
                Ok(())
            }
            ActiveDialog::Create(_) => self.handle_create_key(key),
            ActiveDialog::Delete(_) => self.handle_delete_key(key),
            ActiveDialog::Handoff(_) => self.handle_handoff_key(key),
            ActiveDialog::Gc(_) => self.handle_gc_key(key),
            ActiveDialog::Restore(_) => self.handle_restore_key(key),
            ActiveDialog::Dispatch(_) => self.handle_dispatch_key(key),
            ActiveDialog::Broadcast(_) => self.handle_broadcast_key(key),
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
            KeyCode::Char('j') | KeyCode::Down => {
                match self.focus {
                    ForestFocusPanel::RepoList => self.move_repo_selection(1),
                    ForestFocusPanel::WorktreeList => self.move_wt_selection(1),
                    ForestFocusPanel::Inspector => {
                        self.inspector_scroll = self.inspector_scroll.saturating_add(1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                match self.focus {
                    ForestFocusPanel::RepoList => self.move_repo_selection(-1),
                    ForestFocusPanel::WorktreeList => self.move_wt_selection(-1),
                    ForestFocusPanel::Inspector => {
                        self.inspector_scroll = self.inspector_scroll.saturating_sub(1);
                    }
                }
            }
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
                    self.open_shell()?;
                }
            }
            // Worktree actions (only work when a worktree can be resolved)
            KeyCode::Char('n') => {
                self.open_create_dialog()?;
            }
            KeyCode::Char('s') => {
                self.launch_session()?;
            }
            KeyCode::Char('d') => {
                self.open_delete_dialog()?;
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
                if dialog.focus == 3 {
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
            KeyCode::Char(' ') if dialog.focus == 2 => {
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
                match repo.manager.create(name, &dialog_clone.base_branch, dialog_clone.carry_changes)
                {
                    Ok(wt) => {
                        self.status_message = format!("Created worktree '{}'", wt.name);
                        self.refresh();
                        self.update_inspector();
                    }
                    Err(e) => {
                        self.status_message = format!("Error: {}", e);
                    }
                }
            } else {
                self.status_message = "No repo selected".to_string();
            }
            self.dialog = ActiveDialog::None;
        } else if dialog_clone.cancelled {
            self.dialog = ActiveDialog::None;
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
                        self.status_message = format!(
                            "Deleted '{}' (snapshot saved)",
                            dialog_clone.worktree_name
                        );
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
                let repo_prune: Vec<String> = dialog_clone.to_prune.iter()
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
                            self.status_message =
                                format!("Restored '{}' from snapshot", wt.name);
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
        if count == 0 {
            self.worktree_list_state.select(None);
        } else if let Some(i) = self.worktree_list_state.selected() {
            if i >= count {
                self.worktree_list_state.select(Some(count - 1));
            }
        }
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

        let (stat_raw, files_changed, has_commits, commit_count, gitignore_warnings) =
            match preview {
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
                    session::tracker::find_latest_session_id(&dir).ok().flatten()
                })
        });

        let launch_result = if let Some(ref sid) = session_id {
            session::launcher::resume_session(&wt, &wt_abs, sid, &repo.manager.config.session)
        } else {
            session::launcher::launch_session(&wt, &wt_abs, &repo.manager.config.session)
        };

        match launch_result {
            Ok(pane_id) => {
                let action = if session_id.is_some() { "Resumed" } else { "Launched" };

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

                self.status_message =
                    format!("{} session for '{}' ({})", action, wt.name, pane_id);
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
            Ok(()) => {
                self.status_message = format!("Promoted '{}' to permanent", name);
                self.refresh();
                self.update_inspector();
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
                let results =
                    orchestration::dispatch::dispatch_tasks(&repo.manager, &tasks, &base);
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
                let results =
                    orchestration::broadcast::broadcast_prompt(&repo.worktrees, &prompt);
                let success_count = results.iter().filter(|r| r.success).count();
                let fail_count = results.iter().filter(|r| !r.success).count();

                if fail_count == 0 {
                    self.status_message =
                        format!("Broadcast sent to {} session(s)", success_count);
                } else {
                    self.status_message = format!(
                        "Broadcast: {} sent, {} failed",
                        success_count, fail_count
                    );
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
            self.status_message = "tmux required for shell panes".to_string();
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
}
