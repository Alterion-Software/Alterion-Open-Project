//! Projects: what exists, who owns it, and the plan itself.

use actix_web::{HttpResponse, delete, get, post, put, web};
use aop_core::Project;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::Authenticated;
use crate::entity::{changes as change_rows, project_members, projects as project_rows, role, snapshots};
use crate::error::SyncError;
use crate::state::AppState;

/// GET /api/projects
#[get("/api/projects")]
pub async fn list(
    state: web::Data<AppState>,
    who: Authenticated,
) -> Result<HttpResponse, SyncError> {
    // Membership is the whole of the answer, including for the owner: the
    // owner gets a member row at creation so there is exactly one place that
    // decides what somebody can see.
    let memberships = project_members::Entity::find()
        .filter(project_members::Column::Subject.eq(&who.subject))
        .all(&state.db)
        .await?;

    let ids: Vec<Uuid> = memberships.iter().map(|m| m.project_id).collect();
    if ids.is_empty() {
        return Ok(HttpResponse::Ok().json(json!({ "projects": [] })));
    }

    let rows = project_rows::Entity::find()
        .filter(project_rows::Column::Id.is_in(ids))
        .order_by_desc(project_rows::Column::UpdatedAt)
        .all(&state.db)
        .await?;

    let listed: Vec<_> = rows
        .into_iter()
        .map(|project| {
            let held = memberships
                .iter()
                .find(|m| m.project_id == project.id)
                .map(|m| m.role.clone())
                .unwrap_or_else(|| role::VIEWER.to_string());
            json!({
                "id": project.id,
                "name": project.name,
                "owner": project.owner_subject,
                "role": held,
                "head": project.head_seq,
                "created_at": project.created_at,
                "updated_at": project.updated_at,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(json!({ "projects": listed })))
}

#[derive(Deserialize)]
pub struct NewProject {
    #[serde(default)]
    pub name: Option<String>,
    /// The whole plan, which becomes the snapshot at seq 0.
    pub plan: serde_json::Value,
}

/// POST /api/projects
///
/// A project starts as a snapshot and an empty log. That is why `head_seq`
/// begins at zero rather than at one: zero is a real position meaning "the
/// plan as it was handed over, and nothing since".
#[post("/api/projects")]
pub async fn create(
    state: web::Data<AppState>,
    who: Authenticated,
    body: web::Json<NewProject>,
) -> Result<HttpResponse, SyncError> {
    let body = body.into_inner();

    // Parsed only to be sure it is a plan, then stored as it arrived. Storing
    // a reserialised copy would quietly drop any field this server's copy of
    // aop-core does not know about yet, which is how a newer client loses
    // work to an older server.
    let parsed: Project = serde_json::from_value(body.plan.clone())
        .map_err(|e| SyncError::BadRequest(format!("plan is not a project: {e}")))?;

    let name = body
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| parsed.name.clone());
    let now = Utc::now().fixed_offset();
    let id = Uuid::new_v4();

    let txn = state.db.begin().await?;
    project_rows::ActiveModel {
        id: Set(id),
        name: Set(name.clone()),
        owner_subject: Set(who.subject.clone()),
        head_seq: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&txn)
    .await?;

    project_members::ActiveModel {
        project_id: Set(id),
        subject: Set(who.subject.clone()),
        role: Set(role::OWNER.to_string()),
        added_at: Set(now),
        // Nobody invited them, so there is no address this server was given.
        // Filling it in from their sign in would mean keeping an identity that
        // nothing here has been asked to keep.
        email: Set(None),
    }
    .insert(&txn)
    .await?;

    snapshots::ActiveModel {
        project_id: Set(id),
        seq: Set(0),
        plan: Set(body.plan),
        created_at: Set(now),
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

    Ok(HttpResponse::Created().json(json!({
        "id": id,
        "name": name,
        "head": 0,
        "snapshot_seq": 0,
    })))
}

/// GET /api/projects/{id}
#[get("/api/projects/{id}")]
pub async fn get(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    let held = super::role_on(&state.db, id, &who.subject).await?;
    let project = super::load_project(&state.db, id).await?;
    let range = super::log_range(&state.db, &project).await?;

    Ok(HttpResponse::Ok().json(json!({
        "id": project.id,
        "name": project.name,
        "owner": project.owner_subject,
        "role": held,
        "head": range.head,
        "oldest": range.oldest,
        "snapshot_seq": super::newest_snapshot(&state.db, id).await?,
        "connected": state.hub.connected(id),
        "created_at": project.created_at,
        "updated_at": project.updated_at,
    })))
}

/// GET /api/projects/{id}/snapshot
///
/// The snapshot alone is not enough: it is a plan as of some seq, and the log
/// has almost certainly moved since. So the changes after it come with it,
/// and the client applies one then the other. Sending them separately would
/// leave a window in which a change appended between the two calls belongs to
/// neither answer.
#[get("/api/projects/{id}/snapshot")]
pub async fn snapshot(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    super::role_on(&state.db, id, &who.subject).await?;

    // One transaction, so the snapshot and the tail of the log are the same
    // moment. A repeatable read is what makes "apply this, then these" safe.
    let txn = state.db.begin().await?;
    let newest = snapshots::Entity::find()
        .filter(snapshots::Column::ProjectId.eq(id))
        .order_by_desc(snapshots::Column::Seq)
        .one(&txn)
        .await?
        .ok_or(SyncError::NotFound)?;
    let project = super::load_project(&txn, id).await?;
    let after = change_rows::Entity::find()
        .filter(change_rows::Column::ProjectId.eq(id))
        .filter(change_rows::Column::Seq.gt(newest.seq))
        .order_by_asc(change_rows::Column::Seq)
        .all(&txn)
        .await?;
    txn.commit().await?;

    let changes: Vec<_> = after.iter().map(change_rows::Model::to_change).collect();
    Ok(HttpResponse::Ok().json(json!({
        "seq": newest.seq,
        "plan": newest.plan,
        "head": project.head_seq,
        "changes": changes,
    })))
}

#[derive(Deserialize)]
pub struct NewSnapshot {
    /// The seq this plan already includes.
    pub seq: i64,
    pub plan: serde_json::Value,
}

/// PUT /api/projects/{id}/snapshot
///
/// Snapshots have to come from a client. The server stores commands and has
/// no engine to replay them with, by design, so it cannot fold its own log
/// into a plan. It asks instead: a push whose answer carries
/// `snapshot_wanted` is the server saying the log has run far enough past the
/// newest snapshot that a first sync is getting expensive.
#[put("/api/projects/{id}/snapshot")]
pub async fn put_snapshot(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
    body: web::Json<NewSnapshot>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    super::writer_on(&state.db, id, &who.subject).await?;
    let body = body.into_inner();

    let _: Project = serde_json::from_value(body.plan.clone())
        .map_err(|e| SyncError::BadRequest(format!("plan is not a project: {e}")))?;

    let project = super::load_project(&state.db, id).await?;
    // A snapshot claiming to include work the server has never seen would
    // make a later client skip straight past changes that do not exist yet.
    if body.seq < 0 || body.seq > project.head_seq {
        return Err(SyncError::BadRequest(format!(
            "snapshot seq {} is not within the log, whose head is {}",
            body.seq, project.head_seq
        )));
    }

    let already = snapshots::Entity::find_by_id((id, body.seq))
        .one(&state.db)
        .await?
        .is_some();
    if !already {
        snapshots::ActiveModel {
            project_id: Set(id),
            seq: Set(body.seq),
            plan: Set(body.plan),
            created_at: Set(Utc::now().fixed_offset()),
        }
        .insert(&state.db)
        .await?;
    }

    Ok(HttpResponse::Ok().json(json!({ "seq": body.seq, "stored": !already })))
}

/// DELETE /api/projects/{id}
#[delete("/api/projects/{id}")]
pub async fn delete(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    // Membership first, so a non-member gets the same "not found" they would
    // get for an id that was never real.
    super::role_on(&state.db, id, &who.subject).await?;
    let project = super::load_project(&state.db, id).await?;
    if project.owner_subject != who.subject {
        return Err(SyncError::Forbidden);
    }

    // The members, the log and the snapshots go with it: the foreign keys
    // cascade, so this one delete is the whole story.
    project_rows::Entity::delete_by_id(id).exec(&state.db).await?;
    Ok(HttpResponse::Ok().json(json!({ "deleted": id })))
}
