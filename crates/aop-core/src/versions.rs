//! Points a plan can be put back to.
//!
//! The change log says what was done. This says what the plan looked like, and
//! the two answer different questions. A log lets you read the work; a
//! snapshot lets you return to it. Replaying a log backwards would be the
//! third option and is not one: half the commands in the vocabulary are not
//! invertible without knowing what they overwrote, which is exactly what a
//! snapshot already holds.
//!
//! Two moments are worth keeping, and only two:
//!
//! ```text
//!   save            the planner said "this is a version of the plan"
//!   before a rebase somebody else's work is about to be replayed underneath
//!                   this planner's, and this is the last moment that was
//!                   entirely theirs
//! ```
//!
//! The second is the reason the store exists. A rebase is the only thing in
//! the application that rewrites a planner's own work against someone else's,
//! and it is the thing they will want undone if it turns out wrong. Taking the
//! snapshot before it means there is always somewhere to go back to.
//!
//! A snapshot is a whole plan, so the number kept is bounded and the bound is
//! reported rather than quietly enforced: a store that drops the version
//! somebody was counting on, and says nothing, is worse than one that never
//! offered it.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::Project;
use crate::compare::{Difference, compare};

/// How many snapshots a plan keeps.
///
/// Each one is a whole plan, so this is measured in copies of the file rather
/// than in rows: twenty versions of a two thousand task plan is already tens
/// of megabytes. Twenty is enough to cover a working day of saves and every
/// rebase in it, and few enough that the store stays something a person could
/// open in an editor.
pub const KEEP: usize = 20;

/// Why a snapshot was taken.
///
/// Kept rather than derived from the timing, because the two read completely
/// differently in a list: one is a version the planner made on purpose, the
/// other is a safety net put out on their behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Taken {
    /// The plan was saved.
    Save,
    /// Work from somewhere else was about to be replayed under this planner's.
    BeforeRebase,
}

impl Taken {
    pub fn label(self) -> &'static str {
        match self {
            Taken::Save => "Saved",
            Taken::BeforeRebase => "Before a sync",
        }
    }

    /// What returning to it would mean, for the button that offers it.
    pub fn describe(self) -> &'static str {
        match self {
            Taken::Save => "The plan as it was written to disk.",
            Taken::BeforeRebase => {
                "The plan as it was before other people's changes were brought in."
            }
        }
    }
}

/// One whole plan, and when it was that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub at: NaiveDateTime,
    /// Whoever was at the keyboard. Not necessarily the author of the last
    /// command in the plan: a rebase snapshot is taken on behalf of the person
    /// syncing, over work that may be somebody else's.
    pub author: String,
    pub taken: Taken,
    /// The newest change the log had reached, so a snapshot can be lined up
    /// against the log a planner is reading beside it.
    pub through: Option<u64>,
    pub plan: Project,
}

/// Every snapshot a plan holds, oldest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Versions {
    #[serde(default)]
    kept: Vec<Snapshot>,
    /// How many have been dropped to stay inside [`KEEP`]. Counted so the
    /// list can say that older versions existed, rather than presenting the
    /// oldest one held as though it were the beginning.
    #[serde(default)]
    dropped: usize,
}

impl Versions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// How many were dropped to stay inside the bound.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// Oldest first, which is the order they are stored in.
    pub fn all(&self) -> &[Snapshot] {
        &self.kept
    }

    pub fn get(&self, index: usize) -> Option<&Snapshot> {
        self.kept.get(index)
    }

    pub fn newest(&self) -> Option<&Snapshot> {
        self.kept.last()
    }

    /// Keep the plan as it is now.
    ///
    /// Returns whether anything was stored. A plan identical to the newest
    /// snapshot is not kept again: two versions with nothing between them put
    /// an empty difference in front of somebody trying to choose one, which is
    /// the same reason `History::mark_saved` refuses a second marker over the
    /// same nothing.
    pub fn take(
        &mut self,
        plan: &Project,
        author: impl Into<String>,
        at: NaiveDateTime,
        taken: Taken,
    ) -> bool {
        if let Some(newest) = self.kept.last()
            && compare(&newest.plan, plan).is_empty()
        {
            return false;
        }

        self.kept.push(Snapshot {
            at,
            author: author.into(),
            taken,
            through: plan.history.changes().last().map(|change| change.id),
            plan: plan.clone(),
        });

        if self.kept.len() > KEEP {
            let excess = self.kept.len() - KEEP;
            self.kept.drain(..excess);
            self.dropped += excess;
        }
        true
    }

    /// What changed between one snapshot and whatever came after it.
    ///
    /// The one after it, or the plan as it stands now for the newest, because
    /// the question a person asks of the last version is what has happened
    /// since, not what happened before it.
    pub fn changed_after(&self, index: usize, now: &Project) -> Vec<Difference> {
        let Some(snapshot) = self.kept.get(index) else {
            return Vec::new();
        };
        let after = match self.kept.get(index + 1) {
            Some(next) => &next.plan,
            None => now,
        };
        compare(&snapshot.plan, after)
    }

    /// What the list should call the thing a snapshot is being compared with.
    pub fn compared_with(&self, index: usize) -> &'static str {
        if index + 1 < self.kept.len() {
            "the version after it"
        } else {
            "the plan as it is now"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 1, day)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap()
    }

    fn plan(tasks: usize) -> Project {
        let mut project = Project::blank(at(1));
        for index in 0..tasks {
            project.push_task(format!("Task {index}"), 480);
        }
        project
    }

    #[test]
    fn a_snapshot_holds_the_plan_as_it_was() {
        let mut versions = Versions::new();
        assert!(versions.take(&plan(2), "Ada", at(1), Taken::Save));
        // The plan moving on does not move the snapshot.
        assert_eq!(versions.newest().map(|s| s.plan.tasks.len()), Some(2));
        assert_eq!(versions.len(), 1);
    }

    #[test]
    fn an_unchanged_plan_is_not_kept_twice() {
        // Two versions with nothing between them offer an empty difference to
        // whoever is trying to choose one.
        let mut versions = Versions::new();
        let project = plan(2);
        assert!(versions.take(&project, "Ada", at(1), Taken::Save));
        assert!(!versions.take(&project, "Ada", at(2), Taken::Save));
        assert_eq!(versions.len(), 1);
    }

    #[test]
    fn a_rebase_snapshot_is_kept_even_where_a_save_would_not_be() {
        // It is taken because of what is about to happen, not because of what
        // has changed, so it earns its place either way.
        let mut versions = Versions::new();
        let project = plan(2);
        versions.take(&project, "Ada", at(1), Taken::Save);
        let mut moved = project.clone();
        moved.push_task("One more", 480);
        assert!(versions.take(&moved, "Ada", at(2), Taken::BeforeRebase));
        assert_eq!(versions.newest().map(|s| s.taken), Some(Taken::BeforeRebase));
    }

    #[test]
    fn the_bound_is_counted_rather_than_silently_enforced() {
        let mut versions = Versions::new();
        for index in 0..KEEP + 3 {
            versions.take(&plan(index + 1), "Ada", at(1), Taken::Save);
        }
        assert_eq!(versions.len(), KEEP);
        assert_eq!(versions.dropped(), 3, "the list can say older ones existed");
    }

    #[test]
    fn a_version_is_compared_with_the_one_after_it() {
        let mut versions = Versions::new();
        versions.take(&plan(1), "Ada", at(1), Taken::Save);
        versions.take(&plan(3), "Ada", at(2), Taken::Save);

        let differences = versions.changed_after(0, &plan(9));
        assert!(
            !differences.is_empty(),
            "one task became three between those two versions"
        );
        assert_eq!(versions.compared_with(0), "the version after it");
    }

    #[test]
    fn the_newest_version_is_compared_with_the_plan_as_it_stands() {
        // The question asked of the last version is what has happened since.
        let mut versions = Versions::new();
        versions.take(&plan(1), "Ada", at(1), Taken::Save);
        assert!(!versions.changed_after(0, &plan(4)).is_empty());
        assert!(versions.changed_after(0, &plan(1)).is_empty());
        assert_eq!(versions.compared_with(0), "the plan as it is now");
    }
}
