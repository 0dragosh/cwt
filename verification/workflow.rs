use vstd::prelude::*;
use vstd::set::*;

verus! {

/// Abstract lifecycle for a managed worktree.
pub enum Lifecycle {
    Ephemeral,
    Permanent,
}

/// Abstract runtime status for a managed worktree.
pub enum Status {
    Idle,
    Running,
    Waiting,
    Done,
    Shipping,
}

/// Abstract cwt workflow state.
///
/// A worktree is present when its id is in `worktrees`. Marker sets encode the
/// lifecycle/status/git facts that matter for workflow correctness. Entries
/// absent from a marker set use the safe/default value: ephemeral, idle, clean,
/// pushed, and no source changes.
pub struct State {
    pub worktrees: Set<int>,
    pub permanent: Set<int>,
    pub running: Set<int>,
    pub dirty: Set<int>,
    pub unpushed: Set<int>,
    pub snapshots: Set<int>,
    pub changes: Set<int>,
}

pub open spec fn wf(s: State) -> bool {
    &&& forall|id: int| #[trigger] s.permanent.contains(id) ==> s.worktrees.contains(id)
    &&& forall|id: int| #[trigger] s.running.contains(id) ==> s.worktrees.contains(id)
    &&& forall|id: int| #[trigger] s.dirty.contains(id) ==> s.worktrees.contains(id)
    &&& forall|id: int| #[trigger] s.unpushed.contains(id) ==> s.worktrees.contains(id)
    &&& forall|id: int| #[trigger] s.changes.contains(id) ==> s.worktrees.contains(id)
}

pub open spec fn is_ephemeral(s: State, id: int) -> bool {
    s.worktrees.contains(id) && !s.permanent.contains(id)
}

pub open spec fn is_gc_safe(s: State, id: int) -> bool {
    &&& is_ephemeral(s, id)
    &&& !s.running.contains(id)
    &&& !s.dirty.contains(id)
    &&& !s.unpushed.contains(id)
}

pub open spec fn gc_preview_valid(s: State, selected: Set<int>, excess: nat) -> bool {
    &&& selected.finite()
    &&& selected.len() <= excess
    &&& forall|id: int| #[trigger] selected.contains(id) ==> is_gc_safe(s, id)
}

pub open spec fn create(s: State, id: int) -> State
    recommends
        wf(s),
        !s.worktrees.contains(id),
{
    State {
        worktrees: s.worktrees.insert(id),
        permanent: s.permanent.remove(id),
        running: s.running.remove(id),
        dirty: s.dirty.remove(id),
        unpushed: s.unpushed.remove(id),
        snapshots: s.snapshots,
        changes: s.changes.remove(id),
    }
}

pub open spec fn snapshot_success(s: State, id: int) -> State
    recommends
        wf(s),
        s.worktrees.contains(id),
{
    State {
        worktrees: s.worktrees,
        permanent: s.permanent,
        running: s.running,
        dirty: s.dirty,
        unpushed: s.unpushed,
        snapshots: s.snapshots.insert(id),
        changes: s.changes,
    }
}

pub open spec fn delete_after_snapshot(s: State, id: int) -> State
    recommends
        wf(s),
        s.worktrees.contains(id),
{
    let snap = snapshot_success(s, id);
    State {
        worktrees: snap.worktrees.remove(id),
        permanent: snap.permanent.remove(id),
        running: snap.running.remove(id),
        dirty: snap.dirty.remove(id),
        unpushed: snap.unpushed.remove(id),
        snapshots: snap.snapshots,
        changes: snap.changes.remove(id),
    }
}

pub open spec fn promote(s: State, id: int) -> State
    recommends
        wf(s),
        s.worktrees.contains(id),
{
    State {
        worktrees: s.worktrees,
        permanent: s.permanent.insert(id),
        running: s.running,
        dirty: s.dirty,
        unpushed: s.unpushed,
        snapshots: s.snapshots,
        changes: s.changes,
    }
}

pub open spec fn restore(s: State, id: int) -> State
    recommends
        wf(s),
        s.snapshots.contains(id),
        !s.worktrees.contains(id),
{
    State {
        worktrees: s.worktrees.insert(id),
        permanent: s.permanent.remove(id),
        running: s.running.remove(id),
        dirty: s.dirty.remove(id),
        unpushed: s.unpushed.remove(id),
        snapshots: s.snapshots,
        changes: s.changes.insert(id),
    }
}

pub open spec fn handoff_success(s: State, source: int, target: int) -> State
    recommends
        wf(s),
        source != target,
        s.worktrees.contains(source),
        s.worktrees.contains(target),
        s.changes.contains(source),
{
    State {
        worktrees: s.worktrees,
        permanent: s.permanent,
        running: s.running,
        dirty: s.dirty,
        unpushed: s.unpushed,
        snapshots: s.snapshots,
        changes: s.changes.insert(target),
    }
}

pub proof fn lemma_create_adds_fresh_idle_ephemeral(s: State, id: int)
    requires
        wf(s),
        !s.worktrees.contains(id),
    ensures
        wf(create(s, id)),
        create(s, id).worktrees.contains(id),
        !create(s, id).permanent.contains(id),
        !create(s, id).running.contains(id),
        !create(s, id).dirty.contains(id),
        !create(s, id).unpushed.contains(id),
        !create(s, id).changes.contains(id),
        forall|other: int| other != id ==> (
            #[trigger] create(s, id).worktrees.contains(other) <==> s.worktrees.contains(other)
        ),
        forall|other: int| other != id ==> (
            #[trigger] create(s, id).snapshots.contains(other) <==> s.snapshots.contains(other)
        ),
{
}

pub proof fn lemma_delete_removes_exactly_one_after_snapshot(s: State, id: int)
    requires
        wf(s),
        s.worktrees.contains(id),
    ensures
        wf(delete_after_snapshot(s, id)),
        !delete_after_snapshot(s, id).worktrees.contains(id),
        delete_after_snapshot(s, id).snapshots.contains(id),
        !delete_after_snapshot(s, id).permanent.contains(id),
        !delete_after_snapshot(s, id).running.contains(id),
        !delete_after_snapshot(s, id).dirty.contains(id),
        !delete_after_snapshot(s, id).unpushed.contains(id),
        forall|other: int| other != id ==> (
            #[trigger] delete_after_snapshot(s, id).worktrees.contains(other)
                <==> s.worktrees.contains(other)
        ),
        forall|other: int| other != id ==> (
            #[trigger] delete_after_snapshot(s, id).snapshots.contains(other)
                <==> s.snapshots.contains(other)
        ),
{
}

pub proof fn lemma_promote_is_idempotent(s: State, id: int)
    requires
        wf(s),
        s.worktrees.contains(id),
    ensures
        wf(promote(s, id)),
        promote(s, id).permanent.contains(id),
        forall|other: int| #[trigger] promote(s, id).worktrees.contains(other)
            <==> promote(promote(s, id), id).worktrees.contains(other),
        forall|other: int| #[trigger] promote(s, id).permanent.contains(other)
            <==> promote(promote(s, id), id).permanent.contains(other),
        forall|other: int| #[trigger] s.permanent.contains(other)
            ==> promote(s, id).permanent.contains(other),
{
}

pub proof fn lemma_gc_preview_selects_only_safe_excess(
    s: State,
    selected: Set<int>,
    excess: nat,
)
    requires
        wf(s),
        gc_preview_valid(s, selected, excess),
    ensures
        selected.len() <= excess,
        forall|id: int| #[trigger] selected.contains(id) ==> s.worktrees.contains(id),
        forall|id: int| #[trigger] selected.contains(id) ==> !s.permanent.contains(id),
        forall|id: int| #[trigger] selected.contains(id) ==> !s.running.contains(id),
        forall|id: int| #[trigger] selected.contains(id) ==> !s.dirty.contains(id),
        forall|id: int| #[trigger] selected.contains(id) ==> !s.unpushed.contains(id),
{
}

pub proof fn lemma_restore_preserves_snapshot_metadata(s: State, id: int)
    requires
        wf(s),
        s.snapshots.contains(id),
        !s.worktrees.contains(id),
    ensures
        wf(restore(s, id)),
        restore(s, id).worktrees.contains(id),
        restore(s, id).snapshots.contains(id),
        !restore(s, id).permanent.contains(id),
        !restore(s, id).running.contains(id),
        !restore(s, id).dirty.contains(id),
        !restore(s, id).unpushed.contains(id),
        forall|snapshot: int| #[trigger] restore(s, id).snapshots.contains(snapshot)
            <==> s.snapshots.contains(snapshot),
{
}

pub proof fn lemma_handoff_success_only_mutates_target(s: State, source: int, target: int)
    requires
        wf(s),
        source != target,
        s.worktrees.contains(source),
        s.worktrees.contains(target),
        s.changes.contains(source),
    ensures
        wf(handoff_success(s, source, target)),
        handoff_success(s, source, target).changes.contains(source),
        handoff_success(s, source, target).changes.contains(target),
        forall|id: int| id != target ==> (
            #[trigger] handoff_success(s, source, target).changes.contains(id)
                <==> s.changes.contains(id)
        ),
        forall|id: int| #[trigger] handoff_success(s, source, target).worktrees.contains(id)
            <==> s.worktrees.contains(id),
        forall|id: int| #[trigger] handoff_success(s, source, target).snapshots.contains(id)
            <==> s.snapshots.contains(id),
{
}

}

