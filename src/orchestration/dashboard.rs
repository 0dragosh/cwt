use std::path::Path;

use crate::session;
use crate::session::transcript::TranscriptUsage;
use crate::worktree::model::{Worktree, WorktreeStatus};

/// Aggregate statistics across all sessions.
#[derive(Debug, Clone, Default)]
pub struct AggregateStats {
    pub total_sessions: usize,
    pub running_sessions: usize,
    pub waiting_sessions: usize,
    pub done_sessions: usize,
    pub idle_sessions: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: Option<f64>,
    pub total_messages: usize,
    /// Per-worktree progress info.
    pub session_progress: Vec<SessionProgress>,
}

/// Progress info for a single session, derived from transcript analysis.
#[derive(Debug, Clone)]
pub struct SessionProgress {
    pub worktree_name: String,
    pub status: WorktreeStatus,
    pub message_count: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    pub last_message_preview: String,
}

/// Compute aggregate dashboard stats from a list of worktrees.
/// `resolve_abs_path` is a function that converts a worktree to its absolute path.
pub fn compute_aggregate_stats<F>(worktrees: &[Worktree], resolve_abs_path: F) -> AggregateStats
where
    F: Fn(&Worktree) -> std::path::PathBuf,
{
    let mut stats = AggregateStats {
        total_sessions: worktrees.len(),
        ..Default::default()
    };

    for wt in worktrees {
        match wt.status {
            WorktreeStatus::Running => stats.running_sessions += 1,
            WorktreeStatus::Waiting => stats.waiting_sessions += 1,
            WorktreeStatus::Done => stats.done_sessions += 1,
            WorktreeStatus::Idle => stats.idle_sessions += 1,
            WorktreeStatus::Shipping => stats.done_sessions += 1,
        }

        let wt_abs = resolve_abs_path(wt);
        let (usage, last_msg) = read_session_usage(&wt_abs);

        stats.total_input_tokens += usage.input_tokens;
        stats.total_output_tokens += usage.output_tokens;
        stats.total_messages += usage.message_count;
        if let Some(cost) = usage.total_cost_usd {
            *stats.total_cost_usd.get_or_insert(0.0) += cost;
        }

        stats.session_progress.push(SessionProgress {
            worktree_name: wt.name.clone(),
            status: wt.status.clone(),
            message_count: usage.message_count,
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_usd: usage.total_cost_usd,
            last_message_preview: last_msg,
        });
    }

    stats
}

/// Read token usage and last message for a single worktree's session.
fn read_session_usage(worktree_abs_path: &Path) -> (TranscriptUsage, String) {
    let project_dir = session::tracker::find_project_dir(worktree_abs_path)
        .ok()
        .flatten();

    match project_dir {
        Some(dir) => {
            let info = session::transcript::read_transcript_info(&dir, 1).unwrap_or_default();
            (info.usage, info.last_message)
        }
        None => (TranscriptUsage::default(), String::new()),
    }
}

/// Format token count with K/M suffixes.
pub fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
    }
}

/// Format a cost value.
pub fn format_cost(cost: Option<f64>) -> String {
    match cost {
        Some(c) => format!("${:.4}", c),
        None => "--".to_string(),
    }
}
