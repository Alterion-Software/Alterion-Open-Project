//! Who changed what, and when.
//!
//! Every edit a planner makes is recorded here as the command that made it,
//! written in the same script the macro recorder produces. Storing the command
//! rather than a description of it is what lets this be three things at once:
//!
//! - an audit trail, because each entry names its author and its moment;
//! - the unit of synchronisation, because two people's edits can be exchanged
//!   and replayed rather than their whole files being fought over;
//! - a replay, because a command that can be recorded can be run again.
//!
//! Storing whole-file snapshots instead would give the first of those poorly
//! and neither of the others: a file level sync can only ever be last writer
//! wins, and it can never say which of the two hundred rows actually moved.
//!
//! The text is the stored form on purpose, matching the macro format. It is
//! the thing a person reads in the history panel, so it is the thing kept,
//! rather than a serialised enum that would have to be turned back into text
//! every time anybody looked at it.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// One recorded edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    /// Position in this plan's own history. Monotonic, never reused, and the
    /// cursor a sync uses to ask for everything it has not seen.
    pub id: u64,
    pub at: NaiveDateTime,
    /// Who made it. The planner's name, or their account once a plan is shared.
    pub author: String,
    /// The command that made the change, in the macro script's own form.
    /// Usually one line; a run grouped as a single step may hold several.
    pub script: String,
    /// What it did, in words, for the history panel.
    pub summary: String,
}

impl Change {
    /// The first line of the script, for a narrow column.
    pub fn first_line(&self) -> &str {
        self.script.lines().next().unwrap_or("").trim()
    }

    /// How many commands this entry carries.
    ///
    /// A grouped run, such as a macro or a fill down over twenty rows, is one
    /// entry because it was one action, and the count is how the panel says so
    /// without printing all of it.
    pub fn command_count(&self) -> usize {
        self.script
            .lines()
            .filter(|line| {
                let line = line.trim();
                !line.is_empty() && !line.starts_with("//")
            })
            .count()
    }
}

/// How much history a plan keeps.
///
/// A long editing session produces thousands of entries, and a plan file that
/// grows without bound is its own problem. The oldest are dropped once the
/// limit is passed, which is safe for an audit trail of recent work and for a
/// sync that has already pushed what it dropped.
pub const KEEP: usize = 5_000;

/// Everything that has been done to a plan, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    changes: Vec<Change>,
    /// The next id to hand out. Kept rather than derived from the last entry,
    /// so ids stay unique after the oldest have been dropped.
    #[serde(default)]
    next_id: u64,
    /// The last id this plan has pushed to a server, if it has.
    ///
    /// Everything after it is unsent work, which is what a sync offers and
    /// what a rebase replays on top of whatever came back.
    #[serde(default)]
    pushed_through: Option<u64>,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Every entry, oldest first.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// The most recent entries, newest first, which is the order a history
    /// panel reads in.
    pub fn recent(&self, limit: usize) -> impl Iterator<Item = &Change> {
        self.changes.iter().rev().take(limit)
    }

    /// Add an entry, giving it the next id and the moment it happened.
    pub fn record(
        &mut self,
        author: impl Into<String>,
        script: impl Into<String>,
        summary: impl Into<String>,
        at: NaiveDateTime,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.changes.push(Change {
            id,
            at,
            author: author.into(),
            script: script.into(),
            summary: summary.into(),
        });

        // Dropping from the front keeps the newest, which is what anybody
        // looking at a history panel wants, and what a sync still needs.
        if self.changes.len() > KEEP {
            let excess = self.changes.len() - KEEP;
            self.changes.drain(..excess);
        }
        id
    }

    /// Everything recorded after `cursor`, which is what a sync asks for.
    ///
    /// `None` means everything still held. An id older than the oldest kept
    /// entry returns what is left rather than failing: the caller has fallen
    /// far enough behind that it needs the whole plan anyway, and
    /// `has_gap_since` is how it finds that out.
    pub fn since(&self, cursor: Option<u64>) -> &[Change] {
        let Some(cursor) = cursor else {
            return &self.changes;
        };
        let at = self
            .changes
            .partition_point(|change| change.id <= cursor);
        &self.changes[at..]
    }

    /// Whether the entries after `cursor` have been dropped, so replaying them
    /// would silently miss edits.
    ///
    /// A sync that gets true here has to take the whole plan instead. Saying so
    /// is the difference between a sync that is behind and one that is wrong.
    pub fn has_gap_since(&self, cursor: u64) -> bool {
        match self.changes.first() {
            // Nothing kept: only a cursor already at the end is safe.
            None => cursor + 1 < self.next_id,
            Some(oldest) => oldest.id > cursor + 1,
        }
    }

    /// What this plan has not sent yet.
    pub fn unsent(&self) -> &[Change] {
        self.since(self.pushed_through)
    }

    pub fn pushed_through(&self) -> Option<u64> {
        self.pushed_through
    }

    /// Mark everything up to and including `id` as sent.
    ///
    /// Only ever moves forward: an out of order acknowledgement from a server
    /// must not make already sent work look unsent and send it twice.
    pub fn mark_pushed(&mut self, id: u64) {
        self.pushed_through = Some(match self.pushed_through {
            Some(already) => already.max(id),
            None => id,
        });
    }

    /// Take entries that came from somewhere else, keeping the whole log in id
    /// order.
    ///
    /// Ids already present are ignored rather than duplicated, because a client
    /// that retries a pull must not end up applying the same edit twice.
    pub fn merge(&mut self, incoming: impl IntoIterator<Item = Change>) -> usize {
        let mut added = 0;
        for change in incoming {
            if self.changes.iter().any(|held| held.id == change.id) {
                continue;
            }
            self.next_id = self.next_id.max(change.id.saturating_add(1));
            let at = self.changes.partition_point(|held| held.id < change.id);
            self.changes.insert(at, change);
            added += 1;
        }
        if self.changes.len() > KEEP {
            let excess = self.changes.len() - KEEP;
            self.changes.drain(..excess);
        }
        added
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

    fn log() -> History {
        let mut history = History::new();
        for day in 1..=5 {
            history.record("Ada", "indent();", "Indented a task", at(day));
        }
        history
    }

    #[test]
    fn ids_are_handed_out_in_order_and_never_reused() {
        let history = log();
        let ids: Vec<u64> = history.changes().iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_sync_gets_only_what_it_has_not_seen() {
        let history = log();
        assert_eq!(history.since(None).len(), 5, "no cursor means everything");
        assert_eq!(history.since(Some(2)).len(), 2, "after id 2 leaves 3 and 4");
        assert!(history.since(Some(4)).is_empty(), "caught up means nothing");
    }

    #[test]
    fn dropping_the_oldest_does_not_reuse_an_id() {
        // Ids are the sync cursor. Reusing one after a trim would make a client
        // that asked for "everything after 4" silently skip real work.
        let mut history = History::new();
        for day in 1..=3 {
            history.record("Ada", "indent();", "Indented", at(day));
        }
        let before = history.next_id;
        history.changes.drain(..2);
        let fresh = history.record("Ada", "outdent();", "Outdented", at(4));

        assert_eq!(fresh, before, "the next id carries on past what was dropped");
        assert!(
            history.changes().iter().all(|c| c.id != 0),
            "and nothing reuses a dropped id"
        );
    }

    #[test]
    fn a_cursor_older_than_what_is_kept_is_reported_as_a_gap() {
        // Quietly returning what survives would look like a successful sync
        // that had lost edits, which is worse than saying it cannot be done.
        let mut history = History::new();
        for day in 1..=5 {
            history.record("Ada", "indent();", "Indented", at(day));
        }
        history.changes.drain(..3);

        assert!(history.has_gap_since(0), "0 is behind the oldest kept, which is 3");
        assert!(!history.has_gap_since(3), "3 is the oldest kept, so 4 onward is intact");
        assert!(!history.has_gap_since(4));
    }

    #[test]
    fn pushing_only_ever_moves_forward() {
        let mut history = log();
        history.mark_pushed(3);
        assert_eq!(history.unsent().len(), 1, "only id 4 is left");

        history.mark_pushed(1);
        assert_eq!(
            history.pushed_through(),
            Some(3),
            "an out of order acknowledgement must not resend settled work"
        );
    }

    #[test]
    fn the_same_change_arriving_twice_is_only_taken_once() {
        // A client that retries a pull must not apply the same edit again.
        let mut history = log();
        let repeat = history.changes()[2].clone();

        assert_eq!(history.merge([repeat.clone()]), 0, "already held");
        assert_eq!(history.len(), 5);

        let fresh = Change {
            id: 99,
            at: at(6),
            author: "Grace".into(),
            script: "link();".into(),
            summary: "Linked two tasks".into(),
        };
        assert_eq!(history.merge([fresh]), 1);
        assert_eq!(history.len(), 6);
        assert_eq!(
            history.record("Ada", "indent();", "Indented", at(7)),
            100,
            "the next id clears anything that arrived from elsewhere"
        );
    }

    #[test]
    fn a_merge_keeps_the_log_in_id_order() {
        let mut history = History::new();
        for id in [4u64, 1, 3] {
            history.merge([Change {
                id,
                at: at(1),
                author: "Ada".into(),
                script: "indent();".into(),
                summary: "Indented".into(),
            }]);
        }
        let ids: Vec<u64> = history.changes().iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 3, 4], "arrival order does not decide log order");
    }

    #[test]
    fn a_grouped_run_counts_its_commands_and_ignores_comments() {
        let change = Change {
            id: 0,
            at: at(1),
            author: "Ada".into(),
            script: "// filled down\nselect_rows(3, 7);\nfill_down();\n".into(),
            summary: "Filled a column down 5 rows".into(),
        };
        assert_eq!(change.command_count(), 2);
        assert_eq!(change.first_line(), "// filled down");
    }

    #[test]
    fn a_long_session_does_not_grow_the_plan_without_bound() {
        let mut history = History::new();
        for _ in 0..KEEP + 250 {
            history.record("Ada", "indent();", "Indented", at(1));
        }
        assert_eq!(history.len(), KEEP, "the oldest are dropped");
        assert_eq!(
            history.changes().last().map(|c| c.id),
            Some(KEEP as u64 + 249),
            "and the newest are the ones kept"
        );
    }
}
