use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::time::Duration;

use crate::git;
use crate::session;
use crate::ui;
use crate::worktree::handoff::{self, HandoffDirection};
use crate::worktree::model::{Worktree, WorktreeStatus};
use crate::worktree::Manager;

/// Which dialog is currently active.
#[derive(Debug, Clone)]
pub enum ActiveDialog {
    None,
    Create(ui::dialogs::create::CreateDialog),
    Delete(ui::dialogs::delete::DeleteDialog),
    Handoff(ui::dialogs::handoff::HandoffDialog),
    Gc(ui::dialogs::gc::GcDialog),
    Restore(ui::dialogs::restore::RestoreDialog),
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
    pub should_quit: bool,
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
            should_quit: false,
        };

        app.update_inspector();
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
            self.worktrees
                .iter()
                .filter(|wt| wt.name.contains(&self.filter))
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
    }

    /// Render the full UI.
    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let (list_area, inspector_area, status_area) = ui::layout::main_layout(frame.area());

        // Render the worktree list
        ui::worktree_list::render(
            frame,
            list_area,
            &self.worktrees,
            &mut self.list_state,
            self.focus == FocusPanel::WorktreeList,
            &self.filter,
        );

        // Render the inspector
        ui::inspector::render(
            frame,
            inspector_area,
            self.selected_worktree(),
            &self.inspector_info,
            self.focus == FocusPanel::Inspector,
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
            ActiveDialog::Help => ui::help::render(frame),
        }
    }

    /// Handle a single tick: poll for events and process them.
    pub fn tick(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                self.handle_key(key)?;
            }
        }
        Ok(())
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
                self.move_selection(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
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
            KeyCode::Enter => {
                self.open_shell()?;
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
            }
            KeyCode::Enter => {
                self.filter_mode = false;
                self.status_message.clear();
                // Keep filter active
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.status_message = format!("Filter: {}", self.filter);
                self.clamp_selection();
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.status_message = format!("Filter: {}", self.filter);
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
                            HandoffDirection::WorktreeToLocal => "worktree → local",
                            HandoffDirection::LocalToWorktree => "local → worktree",
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
