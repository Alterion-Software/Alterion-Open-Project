//! The log: reading it, and the push that appends to it.
//!
//! This is where the sync lives. Everything else in the server is storage
//! around it.

use actix_web::{HttpResponse, get, post, web};
use aop_core::history::Change;
use chrono::{TimeZone, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::Authenticated;
use crate::entity::{changes as change_rows, projects as project_rows};
use crate::error::SyncError;
use crate::live::{ConnId, ServerMessage};
use crate::state::AppState;
use crate::sync::{self, Assigned, PushDecision};

/// How many log entries one answer carries.
///
/// A client that has been away for a month should get its history in pages
/// rather than in one response that times out halfway through. The cursor
/// makes paging free: ask again with the last seq received.
pub const MAX_PAGE: u64 = 1000;

#[derive(Deserialize)]
pub struct Window {
    /// The cursor. Absent means from the beginning of what is kept.
    #[serde(default)]
    pub after: Option<i64>,
    #[serde(default)]
    pub limit: Option<u64>,
}

/// GET /api/projects/{id}/changes?after=N
#[get("/api/projects/{id}/changes")]
pub async fn list(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
    window: web::Query<Window>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    super::role_on(&state.db, id, &who.subject).await?;
    let project = super::load_project(&state.db, id).await?;
    let range = super::log_range(&state.db, &project).await?;
    let after = window.after.unwrap_or(0);

    // The same refusal a push gets, for the same reason: an answer that
    // quietly skips trimmed entries looks like a successful sync.
    if range.has_gap_since(after) {
        let decision = PushDecision::Gap { head: range.head, oldest: range.oldest };
        return Ok(HttpResponse::Conflict().json(decision.body()));
    }

    let limit = window.limit.unwrap_or(MAX_PAGE).clamp(1, MAX_PAGE);
    let page = fetch_after(&state.db, id, after, limit + 1).await?;
    let more = page.len() as u64 > limit;
    let changes: Vec<Change> = page
        .iter()
        .take(limit as usize)
        .map(change_rows::Model::to_change)
        .collect();

    Ok(HttpResponse::Ok().json(json!({
        "head": range.head,
        "after": after,
        "changes": changes,
        "more": more,
    })))
}

#[derive(Deserialize)]
pub struct Push {
    /// The cursor these changes were made against. Absent means the client
    /// has never synced, which is only correct if the log is empty.
    #[serde(default)]
    pub after: Option<i64>,
    /// The commands, in the order they were made, in the same shape the
    /// client's own history holds them.
    pub changes: Vec<Change>,
    /// This client's live connection, if it has one, so it is not sent back
    /// the change it just pushed.
    #[serde(default)]
    pub connection: Option<ConnId>,
}

/// POST /api/projects/{id}/changes
///
/// The whole protocol in one place:
///
/// ```text
///   client: after = 42, here are 3 commands
///
///   head == 42   ->  200 applied, they become 43, 44, 45
///   head == 45   ->  409 behind, and 43..45 come back so the client can
///                    replay its 3 commands on top and push again
///   43 is gone   ->  409 gap, take a snapshot instead
///   head == 12   ->  409 ahead, this is not the same log
/// ```
///
/// The refusals never write anything. The server's log is never overwritten
/// by a client's, and a client is never told "fine" when its work was built
/// on a history that has moved.
#[post("/api/projects/{id}/changes")]
pub async fn push(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
    body: web::Json<Push>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    let body = body.into_inner();
    let landed = land(&state, id, &who.subject, body.after, &body.changes, body.connection).await?;

    match landed {
        Landed::Applied { head, applied, snapshot_wanted } => {
            Ok(HttpResponse::Ok().json(json!({
                "status": "applied",
                "head": head,
                "applied": applied,
                "snapshot_wanted": snapshot_wanted,
            })))
        }
        Landed::Behind { head, after, changes, more } => {
            let mut answer = PushDecision::Behind { head, missed_after: after }.body();
            if let Some(object) = answer.as_object_mut() {
                object.insert("changes".into(), json!(changes));
                object.insert("more".into(), json!(more));
            }
            Ok(HttpResponse::Conflict().json(answer))
        }
        Landed::Refused(decision) => Ok(HttpResponse::Conflict().json(decision.body())),
    }
}

/// What became of a batch of somebody's work, whichever transport carried it.
///
/// Shaped so neither transport has to know how the other says things: the
/// REST handler turns this into a status code and a body, and the socket
/// turns it into a message, and both are describing the same event.
#[derive(Debug, Clone)]
pub enum Landed {
    Applied {
        head: i64,
        applied: Vec<Assigned>,
        /// The server asking for a fresh whole plan, because its log has run
        /// far enough past the newest stored one that a first sync would mean
        /// replaying thousands of commands.
        snapshot_wanted: bool,
    },
    Behind {
        head: i64,
        after: i64,
        changes: Vec<Change>,
        more: bool,
    },
    /// Gap or ahead: nothing was written and nothing can be until the client
    /// takes a whole plan.
    Refused(PushDecision),
}

/// Append a client's work, or say why not.
///
/// **This is the only place in the server that writes to the log**, and that
/// is deliberate. The websocket and the REST push are two ways of reaching it
/// and not two implementations of it: [`sync::decide`] is the whole protocol,
/// and a second copy of it would be the one that is quietly wrong on the day
/// two people edit the same plan.
///
/// `except` is the connection that offered the work, so it is not sent back
/// the change it just made.
pub async fn land(
    state: &AppState,
    id: Uuid,
    subject: &str,
    after: Option<i64>,
    changes: &[Change],
    except: Option<ConnId>,
) -> Result<Landed, SyncError> {
    // Checked here rather than at the socket handshake, because a role can be
    // taken away while a socket is held open and the check that counts is the
    // one in front of the write.
    super::writer_on(&state.db, id, subject).await?;

    if changes.len() as u64 > MAX_PAGE {
        return Err(SyncError::BadRequest(format!(
            "a push carries at most {MAX_PAGE} changes"
        )));
    }

    let txn = state.db.begin().await?;

    // The row lock is what makes seq assignment safe. Two pushes arriving
    // together both read head 42 without it, both decide to append at 43, and
    // one of them loses to the primary key. Locking makes the second one read
    // 45 and be told it is behind, which is the answer it should have had.
    let project = project_rows::Entity::find_by_id(id)
        .lock_exclusive()
        .one(&txn)
        .await?
        .ok_or(SyncError::NotFound)?;
    let range = super::log_range(&txn, &project).await?;
    let decision = sync::decide(range, after);

    let first_seq = match &decision {
        PushDecision::Append { first_seq } => *first_seq,
        PushDecision::Behind { head, missed_after } => {
            let missed = fetch_after(&txn, id, *missed_after, MAX_PAGE + 1).await?;
            txn.rollback().await?;

            let more = missed.len() as u64 > MAX_PAGE;
            return Ok(Landed::Behind {
                head: *head,
                after: *missed_after,
                changes: missed
                    .iter()
                    .take(MAX_PAGE as usize)
                    .map(change_rows::Model::to_change)
                    .collect(),
                more,
            });
        }
        PushDecision::Gap { .. } | PushDecision::Ahead { .. } => {
            txn.rollback().await?;
            return Ok(Landed::Refused(decision));
        }
    };

    // An empty push is how a client asks "am I still current?", and the
    // answer is the same shape as a real one.
    let mut applied = Vec::with_capacity(changes.len());
    let mut broadcast = Vec::with_capacity(changes.len());
    for (change, seq) in changes.iter().zip(sync::assign(first_seq, changes.len())) {
        let row = change_rows::ActiveModel {
            project_id: Set(id),
            seq: Set(seq),
            // The client's own moment, not the server's. A plan edited on a
            // plane is pushed hours later and the history panel should still
            // say when the planner did it.
            at: Set(Utc.from_utc_datetime(&change.at).fixed_offset()),
            // From the token, never from the body: nobody signs another
            // account's name to a command.
            author_subject: Set(subject.to_string()),
            author_name: Set(change.author.clone()),
            script: Set(change.script.clone()),
            summary: Set(change.summary.clone()),
        };
        let stored = row.insert(&txn).await?;
        applied.push(Assigned { local_id: change.id, seq });
        broadcast.push(stored);
    }

    let head = first_seq + changes.len() as i64 - 1;
    if !changes.is_empty() {
        let mut project: project_rows::ActiveModel = project.into();
        project.head_seq = Set(head);
        project.updated_at = Set(Utc::now().fixed_offset());
        project.update(&txn).await?;
    }
    txn.commit().await?;

    // Only after the commit. Telling live clients about a change that a
    // rollback then removed would leave them holding an edit nobody else has.
    for row in &broadcast {
        state.hub.broadcast(
            id,
            &ServerMessage::Change { seq: row.seq, change: row.to_change() },
            except,
        );
    }

    let head = if changes.is_empty() { range.head } else { head };
    let snapshot_wanted = sync::wants_snapshot(
        head,
        super::newest_snapshot(&state.db, id).await?,
        state.config.snapshot_every,
    );
    Ok(Landed::Applied { head, applied, snapshot_wanted })
}

/// One page of the log after a cursor.
pub async fn fetch_after(
    db: &impl ConnectionTrait,
    project: Uuid,
    after: i64,
    limit: u64,
) -> Result<Vec<change_rows::Model>, SyncError> {
    Ok(change_rows::Entity::find()
        .filter(change_rows::Column::ProjectId.eq(project))
        .filter(change_rows::Column::Seq.gt(after))
        .order_by_asc(change_rows::Column::Seq)
        .limit(limit)
        .all(db)
        .await?)
}
