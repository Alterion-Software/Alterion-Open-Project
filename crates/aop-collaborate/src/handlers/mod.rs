//! The HTTP surface, and the access checks every part of it shares.

pub mod changes;
pub mod health;
pub mod live;
pub mod members;
pub mod projects;

use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::entity::{changes as change_rows, project_members, projects as project_rows, role};
use crate::error::SyncError;
use crate::sync::LogRange;

/// Mount everything. One function, so `main` reads as a list of what the
/// server is rather than a list of routes.
pub fn routes(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.service(health::health)
        .service(projects::list)
        .service(projects::create)
        .service(projects::get)
        .service(projects::delete)
        .service(projects::snapshot)
        .service(projects::put_snapshot)
        .service(changes::list)
        .service(changes::push)
        .service(members::list)
        .service(members::invite)
        .service(members::cancel)
        .service(members::remove)
        .service(members::claim)
        .service(live::live);
}

/// This subject's role on this project, or nothing, without deciding what
/// nothing means.
///
/// Almost every caller wants [`role_on`], which turns the absence into the
/// answer a non-member gets. This exists for the sharing routes, where the
/// absence and a role that is merely not the owner's are two different
/// refusals and the choice between them is made in [`crate::sharing`].
pub async fn held_by(
    db: &impl ConnectionTrait,
    project: Uuid,
    subject: &str,
) -> Result<Option<String>, SyncError> {
    Ok(project_members::Entity::find_by_id((project, subject.to_string()))
        .one(db)
        .await?
        .map(|member| member.role))
}

/// This subject's role on this project.
///
/// A subject who is not a member is told the project does not exist rather
/// than that they are not allowed in. Project ids are guessable enough that
/// the difference is an enumeration oracle: "forbidden" confirms the id is
/// real, and "not found" tells them nothing.
pub async fn role_on(
    db: &impl ConnectionTrait,
    project: Uuid,
    subject: &str,
) -> Result<String, SyncError> {
    held_by(db, project, subject).await?.ok_or(SyncError::NotFound)
}

/// The same, refusing a reader who is trying to write.
pub async fn writer_on(
    db: &impl ConnectionTrait,
    project: Uuid,
    subject: &str,
) -> Result<String, SyncError> {
    let held = role_on(db, project, subject).await?;
    if !role::may_write(&held) {
        return Err(SyncError::Forbidden);
    }
    Ok(held)
}

pub async fn load_project(
    db: &impl ConnectionTrait,
    project: Uuid,
) -> Result<project_rows::Model, SyncError> {
    project_rows::Entity::find_by_id(project)
        .one(db)
        .await?
        .ok_or(SyncError::NotFound)
}

/// What the log currently spans, which is both numbers the sync decisions
/// need. `head` comes off the project row so it is the same number a push
/// locks; `oldest` is a one row lookup down the primary key.
pub async fn log_range(
    db: &impl ConnectionTrait,
    project: &project_rows::Model,
) -> Result<LogRange, SyncError> {
    let oldest = change_rows::Entity::find()
        .filter(change_rows::Column::ProjectId.eq(project.id))
        .order_by_asc(change_rows::Column::Seq)
        .one(db)
        .await?
        .map(|row| row.seq);
    Ok(LogRange { head: project.head_seq, oldest })
}

/// The newest snapshot's seq, if there is one.
pub async fn newest_snapshot(
    db: &impl ConnectionTrait,
    project: Uuid,
) -> Result<Option<i64>, SyncError> {
    Ok(crate::entity::snapshots::Entity::find()
        .filter(crate::entity::snapshots::Column::ProjectId.eq(project))
        .order_by_desc(crate::entity::snapshots::Column::Seq)
        .one(db)
        .await?
        .map(|row| row.seq))
}
