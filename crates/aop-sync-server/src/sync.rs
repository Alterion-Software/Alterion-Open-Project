//! The decision a push turns on, with no database and no HTTP in sight.
//!
//! A client says "here is work I made after cursor N". The server holds a log
//! whose last position is `head`. There are only four honest answers, and the
//! point of putting them here is that each one can be shown to be right
//! without a Postgres running.
//!
//! ```text
//!   client cursor N            server log
//!   ---------------            ------------------------------------------
//!   N == head                  ... 40 41 42(head)      append at 43
//!   N <  head                  ... 40 41 42(head)      41, 42 came back
//!        N = 40                                        with the refusal
//!   N <  oldest kept - 1       ... 40 41 42(head)      41 onwards is all
//!        N = 12, oldest = 38                           that is left: gap
//!   N >  head                  ... 40 41 42(head)      the client is from
//!        N = 60                                        a log we do not have
//! ```
//!
//! `seq` counts from 1, so a cursor of 0 means "I have nothing", and `head`
//! of 0 means "the log is empty". That keeps the arithmetic free of options
//! everywhere except the wire, where `null` is friendlier than 0.

use serde_json::{Value, json};

/// What the server currently holds for one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRange {
    /// The last seq handed out. Zero when nothing has ever been appended.
    pub head: i64,
    /// The oldest seq still stored, if anything is. `None` means the log has
    /// been emptied, not that it was never written to: `head` says which.
    pub oldest: Option<i64>,
}

impl LogRange {
    pub fn empty() -> Self {
        Self { head: 0, oldest: None }
    }

    /// Whether everything after `cursor` is still here to be replayed.
    ///
    /// This is the server side of `aop_core::history::History::has_gap_since`,
    /// deliberately the same rule: a sync that quietly returns the survivors
    /// looks like it worked and has lost edits, which is worse than one that
    /// refuses.
    pub fn has_gap_since(&self, cursor: i64) -> bool {
        match self.oldest {
            // Nothing kept, so only a cursor already at the end is safe.
            None => cursor < self.head,
            Some(oldest) => oldest > cursor + 1,
        }
    }
}

/// What to do with a push, and what to tell the client if the answer is no.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushDecision {
    /// The client is up to date. Its changes take `first_seq` onwards.
    Append { first_seq: i64 },
    /// Somebody else pushed first. The client has to replay its own commands
    /// on top of `missed_after` and try again.
    Behind { head: i64, missed_after: i64 },
    /// The client is so far back that the log it needs no longer exists. It
    /// has to take a snapshot instead.
    Gap { head: i64, oldest: Option<i64> },
    /// The client's cursor is past the end of the server's log, which means
    /// they are not the same log: a restore from backup, or a client pointed
    /// at the wrong instance. Appending would interleave two histories.
    Ahead { head: i64, cursor: i64 },
}

/// The whole of the push protocol's thinking.
///
/// `after` is the cursor the client made its changes against, `None` meaning
/// it has never synced. The order of the tests matters: a client that is both
/// behind and beyond the retained log needs to hear "gap", because rebasing
/// on an incomplete answer is exactly the silent data loss this avoids.
pub fn decide(range: LogRange, after: Option<i64>) -> PushDecision {
    let cursor = after.unwrap_or(0);

    if cursor > range.head {
        return PushDecision::Ahead { head: range.head, cursor };
    }
    if range.has_gap_since(cursor) {
        return PushDecision::Gap { head: range.head, oldest: range.oldest };
    }
    if cursor < range.head {
        return PushDecision::Behind { head: range.head, missed_after: cursor };
    }
    PushDecision::Append { first_seq: range.head + 1 }
}

impl PushDecision {
    /// The conflict body, without the changes that go in it.
    ///
    /// Handlers fill `changes` in for the behind case, because fetching them
    /// needs the database and this does not. The shape is fixed here so the
    /// client has one thing to match on: `status` always says which of the
    /// four happened, and `head` is always the number to sync to.
    pub fn body(&self) -> Value {
        match self {
            Self::Append { first_seq } => json!({
                "status": "applied",
                "head": first_seq - 1,
            }),
            Self::Behind { head, missed_after } => json!({
                "status": "behind",
                "head": head,
                "after": missed_after,
                "changes": [],
            }),
            Self::Gap { head, oldest } => json!({
                "status": "gap",
                "head": head,
                "oldest": oldest,
                "message": "the log this cursor needs has been trimmed, fetch a snapshot",
            }),
            Self::Ahead { head, cursor } => json!({
                "status": "ahead",
                "head": head,
                "cursor": cursor,
                "message": "this cursor is past the server's head, fetch a snapshot",
            }),
        }
    }

    /// Whether the client should be answered 409 rather than 200. Only the
    /// append case is a success, and the three refusals are all conflicts
    /// rather than errors: nothing is broken, the client just has to catch up.
    pub fn is_conflict(&self) -> bool {
        !matches!(self, Self::Append { .. })
    }
}

/// Seq numbers a run of `count` pushed changes will be given.
///
/// Trivial on its own, and worth naming because getting it wrong by one is
/// how a sync silently drops the first change of every push.
pub fn assign(first_seq: i64, count: usize) -> impl Iterator<Item = i64> {
    (0..count as i64).map(move |offset| first_seq + offset)
}

/// Whether a fresh snapshot would be worth asking a client for.
///
/// The server cannot make one itself: it stores commands and has no engine to
/// replay them with, which is the price of not reimplementing the scheduler
/// here. So it asks whoever pushes next, once the log has run far enough past
/// the newest snapshot that a first sync would mean replaying thousands of
/// commands.
pub fn wants_snapshot(head: i64, newest_snapshot: Option<i64>, every: i64) -> bool {
    head - newest_snapshot.unwrap_or(0) >= every
}
