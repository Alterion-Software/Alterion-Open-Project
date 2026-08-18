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

/// A point in the log somebody saved at.
///
/// A single command is too fine grained to put in front of a person: nobody
/// decides about `indent()`. A whole file is too coarse to merge, because it
/// can only ever be last writer wins. A save is the unit in between, the batch
/// of commands since the last one, and it is the thing somebody actually
/// decides about when a sync asks them what to take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Save {
    /// The last change this save covers. Everything after the marker before
    /// it, up to and including this, is the batch.
    pub change_id: u64,
    pub at: NaiveDateTime,
    pub author: String,
    /// What they called it. Like a commit message, but never required: a
    /// planner pressing Ctrl+S will not write one, and demanding one would
    /// only produce a log full of "wip".
    #[serde(default)]
    pub note: Option<String>,
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
    /// Where the saves fall in the log, oldest first.
    ///
    /// A list of its own rather than a flag on `Change`, for two reasons. A
    /// flag would be read far more often than it is set, since a save marks
    /// one change in a few hundred and every enumeration of saves would have
    /// to walk the whole log to find them. And a save carries its own author,
    /// moment and note, which are not the author, moment and note of the
    /// command it happens to land on: pressing Ctrl+S is not the edit before
    /// it, and a plan that has been merged can easily have somebody else's
    /// command sitting under your save.
    ///
    /// The cost of a separate list is that it points into the log by id, so
    /// trimming has to be careful. `trim` says how.
    #[serde(default)]
    saves: Vec<Save>,
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

        self.trim();
        id
    }

    /// Drop the oldest entries once the limit is passed, and any marker left
    /// with nothing under it.
    ///
    /// Dropping from the front keeps the newest, which is what anybody looking
    /// at a history panel wants, and what a sync still needs.
    ///
    /// A marker survives for as long as any of the commands it covers do. Once
    /// the last of them has gone it goes too, because a marker pointing at an
    /// id the log no longer holds is a dangling reference, and `changes_in`
    /// would answer it with somebody else's batch. Dropping it cannot move any
    /// surviving save's boundary either: the commands between it and the next
    /// marker have gone with it, so the next marker's batch is the same slice
    /// whether the older marker is there or not.
    fn trim(&mut self) {
        if self.changes.len() > KEEP {
            let excess = self.changes.len() - KEEP;
            self.changes.drain(..excess);
        }
        if let Some(oldest) = self.changes.first().map(|change| change.id) {
            self.saves.retain(|save| save.change_id >= oldest);
        }
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
        self.trim();
        added
    }

    // ---- saves ----------------------------------------------------------

    /// Mark the newest entry as a save.
    ///
    /// Returns the change the marker landed on, or `None` when there is
    /// nothing to mark: an empty log, or a second Ctrl+S with no edit in
    /// between. Two markers over the same nothing would put an empty batch in
    /// front of somebody deciding what to take.
    pub fn mark_saved(
        &mut self,
        author: impl Into<String>,
        at: NaiveDateTime,
        note: Option<String>,
    ) -> Option<u64> {
        let newest = self.changes.last().map(|change| change.id)?;
        if self.saves.last().is_some_and(|save| save.change_id >= newest) {
            return None;
        }
        self.saves.push(Save {
            change_id: newest,
            at,
            author: author.into(),
            note,
        });
        Some(newest)
    }

    /// Every save, oldest first.
    pub fn saves(&self) -> &[Save] {
        &self.saves
    }

    pub fn last_save(&self) -> Option<&Save> {
        self.saves.last()
    }

    /// The marker before the one at `change_id`, which is where its batch
    /// starts.
    fn marker_before(&self, change_id: u64) -> Option<u64> {
        self.saves
            .iter()
            .rev()
            .find(|save| save.change_id < change_id)
            .map(|save| save.change_id)
    }

    /// The commands one save covers: everything after the marker before it, up
    /// to and including its own change.
    ///
    /// Sliced by id against what is actually held, so a marker whose oldest
    /// commands have been trimmed away gives back the part that survives
    /// rather than reaching into the batch before it. `save_is_complete` is
    /// how a caller finds out that is what happened.
    pub fn changes_in(&self, change_id: u64) -> &[Change] {
        let from = match self.marker_before(change_id) {
            Some(previous) => self.changes.partition_point(|change| change.id <= previous),
            None => 0,
        };
        let to = self.changes.partition_point(|change| change.id <= change_id);
        &self.changes[from..to.max(from)]
    }

    /// Whether every command a save covers is still held.
    ///
    /// False means the log was trimmed part way through it, so showing the
    /// batch would show part of one while looking like the whole. Same
    /// distinction `has_gap_since` draws for a sync, and for the same reason.
    pub fn save_is_complete(&self, save: &Save) -> bool {
        match self.marker_before(save.change_id) {
            Some(previous) => !self.has_gap_since(previous),
            // Nothing before it, so its batch runs from the very first command
            // this plan ever recorded. Complete only while that is still here.
            None => self.changes.first().is_some_and(|change| change.id == 0),
        }
    }

    /// The commands since the last save, which is the work that would be lost.
    pub fn unsaved(&self) -> &[Change] {
        self.since(self.saves.last().map(|save| save.change_id))
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
    fn a_save_covers_exactly_what_came_after_the_one_before_it() {
        let mut history = log();
        history.mark_saved("Ada", at(2), None);
        for day in 6..=8 {
            history.record("Ada", "outdent();", "Outdented", at(day));
        }
        let second = history
            .mark_saved("Ada", at(3), Some("ready for review".into()))
            .expect("there is work to mark");

        assert_eq!(history.saves().len(), 2);
        assert_eq!(second, 7);

        let first = history.saves()[0].change_id;
        let ids: Vec<u64> = history.changes_in(first).iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4], "the first save is everything up to it");

        let ids: Vec<u64> = history.changes_in(second).iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![5, 6, 7], "and the second only what followed");

        assert_eq!(
            history.saves()[1].note.as_deref(),
            Some("ready for review"),
            "a note rides with the marker, not with the command"
        );
    }

    #[test]
    fn unsaved_is_what_came_after_the_last_marker() {
        let mut history = log();
        assert_eq!(history.unsaved().len(), 5, "nothing saved yet");

        history.mark_saved("Ada", at(2), None);
        assert!(history.unsaved().is_empty());

        history.record("Ada", "link();", "Linked two tasks", at(6));
        let ids: Vec<u64> = history.unsaved().iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![5]);
    }

    #[test]
    fn pressing_save_twice_over_nothing_does_not_add_a_second_marker() {
        let mut history = log();
        assert!(history.mark_saved("Ada", at(2), None).is_some());
        assert!(
            history.mark_saved("Ada", at(2), None).is_none(),
            "an empty batch is not a save"
        );
        assert_eq!(history.saves().len(), 1);

        let mut empty = History::new();
        assert!(empty.mark_saved("Ada", at(1), None).is_none());
    }

    #[test]
    fn save_markers_survive_a_trim_and_none_of_them_dangles() {
        // The marker points into the log by id, so a trim has to leave every
        // surviving marker naming its own batch and no marker naming a batch
        // that has gone.
        let mut history = History::new();
        for _ in 0..100 {
            history.record("Ada", "indent();", "Indented", at(1));
        }
        let early = history
            .mark_saved("Ada", at(1), None)
            .expect("there is work to mark");
        for _ in 0..KEEP - 100 {
            history.record("Ada", "outdent();", "Outdented", at(2));
        }
        let middle = history
            .mark_saved("Ada", at(2), None)
            .expect("there is work to mark");
        for _ in 0..300 {
            history.record("Ada", "link();", "Linked", at(3));
        }
        let late = history
            .mark_saved("Ada", at(3), None)
            .expect("there is work to mark");

        assert_eq!(history.len(), KEEP, "the log was trimmed");
        let oldest = history.changes().first().map(|c| c.id).expect("kept work");
        assert!(oldest > early, "the early save's whole batch went with it");

        let kept: Vec<u64> = history.saves().iter().map(|save| save.change_id).collect();
        assert_eq!(
            kept,
            vec![middle, late],
            "a marker survives a trim for as long as any of its work does"
        );
        assert!(
            history.saves().iter().all(|save| save.change_id >= oldest),
            "and no marker is left pointing at a change the log has dropped"
        );

        let batch: Vec<u64> = history.changes_in(late).iter().map(|c| c.id).collect();
        assert_eq!(batch.len(), 300, "the last save still covers its own work");
        assert_eq!(batch.first(), Some(&(middle + 1)));
        assert!(history.save_is_complete(&history.saves()[1]));

        let cut = history.saves()[0].clone();
        assert!(
            !history.save_is_complete(&cut),
            "the middle save lost its oldest commands with the early marker"
        );
    }

    #[test]
    fn a_save_cut_in_half_by_a_trim_says_so() {
        let mut history = History::new();
        for _ in 0..KEEP + 200 {
            history.record("Ada", "indent();", "Indented", at(1));
        }
        let id = history
            .mark_saved("Ada", at(1), None)
            .expect("there is work to mark");
        let save = history.saves()[0].clone();

        assert!(
            !history.save_is_complete(&save),
            "its oldest commands were trimmed, so it is only part of a save"
        );
        assert!(!history.changes_in(id).is_empty(), "and the rest is still there");

        let mut whole = History::new();
        whole.record("Ada", "indent();", "Indented", at(1));
        whole.mark_saved("Ada", at(1), None);
        assert!(whole.save_is_complete(&whole.saves()[0]));
    }

    #[test]
    fn a_plan_saved_before_markers_existed_still_opens() {
        // Every new field is defaulted, so a file written before any of this
        // has to read back as a log with no saves rather than fail.
        let older = r#"{"changes":[{"id":0,"at":"2026-01-01T09:00:00","author":"Ada",
            "script":"indent();","summary":"Indented"}],"next_id":1}"#;
        let history: History = serde_json::from_str(older).expect("an older plan opens");

        assert_eq!(history.len(), 1);
        assert!(history.saves().is_empty());
        assert_eq!(history.unsaved().len(), 1, "none of it has been saved");
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
