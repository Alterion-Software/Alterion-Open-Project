//! Sharing: who is in a plan, who has been invited, and how somebody gets in.
//!
//! The decisions all live in [`crate::sharing`], where they can be shown to be
//! right without a Postgres or an identity provider running. What is here is
//! the storage around them and the one call this server makes to the provider
//! that introspection does not already cover.
//!
//! ```text
//!   GET    /api/projects/{id}/members        any member
//!   POST   /api/projects/{id}/invites        the owner
//!   DELETE /api/projects/{id}/invites?email= the owner
//!   DELETE /api/projects/{id}/members?subject= the owner
//!   POST   /api/projects/{id}/claim          whoever was invited
//! ```
//!
//! **Addresses.** An invitation is an address somebody else typed, belonging
//! to a person who has not agreed to anything yet. The owner sent it, so the
//! owner sees it; nobody else on the plan does. Once it is claimed the row is
//! gone, and what is left is one copy on the member's own row, still shown to
//! the owner alone. So the rule is one sentence: addresses are for the owner.
//!
//! **Not found, never forbidden.** Every route here starts by asking what the
//! caller's role is, and a caller with no row is told the plan is not there.
//! That is the same answer an id that was never real gets, which is what stops
//! anybody learning which ids exist by trying them. A member who is not the
//! owner gets a real refusal, because a member already knows the plan is real.

use actix_web::{HttpRequest, HttpResponse, delete, get, post, web};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{Authenticated, bearer};
use crate::entity::{project_invites, project_members, role};
use crate::error::SyncError;
use crate::sharing::{self, Claim, Offered, Presented};
use crate::state::AppState;

/// GET /api/projects/{id}/members
///
/// What a member is entitled to know: who else is in, and what they may do.
/// The owner is additionally shown the addresses, because the owner is the
/// only person who can act on them.
#[get("/api/projects/{id}/members")]
pub async fn list(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    let held = super::role_on(&state.db, id, &who.subject).await?;
    let project = super::load_project(&state.db, id).await?;
    let owner = held == role::OWNER;

    let members = project_members::Entity::find()
        .filter(project_members::Column::ProjectId.eq(id))
        .order_by_asc(project_members::Column::AddedAt)
        .all(&state.db)
        .await?;

    let listed: Vec<_> = members
        .iter()
        .map(|member| {
            json!({
                "subject": member.subject,
                "role": member.role,
                "added_at": member.added_at,
                // Not merely hidden from the interface: absent from the
                // answer, so a member reading the response by hand learns no
                // more than a member reading the screen.
                "email": owner.then(|| member.email.clone()).flatten(),
            })
        })
        .collect();

    // Null rather than an empty list. "There are none" and "you are not the
    // one who gets to see them" are different, and a client that showed the
    // second as the first would be quietly lying to an editor.
    let invites = if owner {
        let pending = project_invites::Entity::find()
            .filter(project_invites::Column::ProjectId.eq(id))
            .order_by_asc(project_invites::Column::InvitedAt)
            .all(&state.db)
            .await?;
        json!(
            pending
                .iter()
                .map(|row| json!({
                    "email": row.email,
                    "role": row.role,
                    "invited_by": row.invited_by,
                    "invited_at": row.invited_at,
                }))
                .collect::<Vec<_>>()
        )
    } else {
        json!(null)
    };

    Ok(HttpResponse::Ok().json(json!({
        "you": who.subject,
        "role": held,
        "owner": project.owner_subject,
        "members": listed,
        "invites": invites,
    })))
}

#[derive(Deserialize)]
pub struct NewInvite {
    pub email: String,
    pub role: String,
}

/// POST /api/projects/{id}/invites
///
/// Creates a pending invitation and nothing else. No account is looked up, so
/// this call cannot be used to find out whether an address belongs to anybody:
/// it answers the same way for an address with an account behind it and one
/// with nothing behind it at all.
#[post("/api/projects/{id}/invites")]
pub async fn invite(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
    body: web::Json<NewInvite>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    sharing::manager(super::held_by(&state.db, id, &who.subject).await?)?;
    let body = body.into_inner();

    let email = sharing::address(&body.email).ok_or_else(|| {
        SyncError::BadRequest(
            "that is not an email address. One address, with an @ in it and no spaces".into(),
        )
    })?;
    let role = role::invitable(&body.role).ok_or_else(|| {
        SyncError::BadRequest(format!(
            "an invitation can be for {} or {}, not \"{}\". A plan has one owner, and \
             passing it on is not something an invitation does",
            role::EDITOR,
            role::VIEWER,
            body.role
        ))
    })?;

    // Inviting the same address twice is one invitation whose role is whatever
    // was said last, not two rows racing to decide what somebody gets. The
    // existing row is read first so the answer can say which of the two
    // happened, since "you already invited them" is worth knowing.
    let already = project_invites::Entity::find_by_id((id, email.clone()))
        .one(&state.db)
        .await?;
    let replaced = already.is_some();
    let now = Utc::now().fixed_offset();
    match already {
        Some(row) => {
            let mut row: project_invites::ActiveModel = row.into();
            row.role = Set(role.to_string());
            row.invited_by = Set(who.subject.clone());
            row.invited_at = Set(now);
            row.update(&state.db).await?;
        }
        None => {
            project_invites::ActiveModel {
                project_id: Set(id),
                email: Set(email.clone()),
                role: Set(role.to_string()),
                invited_by: Set(who.subject.clone()),
                invited_at: Set(now),
            }
            .insert(&state.db)
            .await?;
        }
    }

    // The address goes back normalised, which is the form that will actually
    // be matched rather than the form that was typed. Showing it is how
    // somebody notices they have invited a typo.
    Ok(HttpResponse::Created().json(json!({
        "email": email,
        "role": role,
        "invited_at": now,
        "replaced": replaced,
    })))
}

#[derive(Deserialize)]
pub struct ByEmail {
    pub email: String,
}

/// DELETE /api/projects/{id}/invites?email=ada@example.com
///
/// The address is a query value rather than a path segment. A local part may
/// be quoted, and a quoted one may contain a slash, so a path segment would be
/// a shape that works until the day somebody has an unusual address.
#[delete("/api/projects/{id}/invites")]
pub async fn cancel(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
    query: web::Query<ByEmail>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    sharing::manager(super::held_by(&state.db, id, &who.subject).await?)?;

    let email = sharing::address(&query.email)
        .ok_or_else(|| SyncError::BadRequest("that is not an email address".into()))?;
    let gone = project_invites::Entity::delete_by_id((id, email.clone()))
        .exec(&state.db)
        .await?;
    // A caller who got this far is the owner and already knows the plan is
    // real, so a plain "there is no such invitation" gives nothing away.
    if gone.rows_affected == 0 {
        return Err(SyncError::NotFound);
    }

    Ok(HttpResponse::Ok().json(json!({ "cancelled": email })))
}

#[derive(Deserialize)]
pub struct BySubject {
    pub subject: String,
}

/// DELETE /api/projects/{id}/members?subject=...
///
/// Nothing is done to the copy on their machine, and nothing could be: they
/// hold a whole plan and this server has no reach into it. What changes is
/// that their next sync is answered the way a plan that is not theirs is
/// answered, and the client already says exactly that.
#[delete("/api/projects/{id}/members")]
pub async fn remove(
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
    query: web::Query<BySubject>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();
    sharing::manager(super::held_by(&state.db, id, &who.subject).await?)?;

    let project = super::load_project(&state.db, id).await?;
    let subject = query.subject.trim().to_string();
    sharing::removable(&project.owner_subject, &subject)?;

    let gone = project_members::Entity::delete_by_id((id, subject.clone()))
        .exec(&state.db)
        .await?;
    if gone.rows_affected == 0 {
        return Err(SyncError::NotFound);
    }

    Ok(HttpResponse::Ok().json(json!({ "removed": subject })))
}

/// POST /api/projects/{id}/claim
///
/// The one place somebody who is not a member becomes one, and the only place
/// this server asks the identity provider anything beyond "is this token
/// real".
///
/// **Why this is its own endpoint.** Doing it quietly inside the call that
/// opens a plan would make a read into a write, would put a round trip to the
/// provider on the path of every request that misses, and would leave the
/// answer to a plan that is not there depending on whether the provider was
/// reachable. Here the caller has said what they are doing, so the three
/// outcomes can be told apart honestly: you are in, there is nothing here for
/// you, or nobody could check. The client makes the link feel automatic by
/// calling this when opening a plan comes back not found, which is the one
/// moment it can possibly help.
#[post("/api/projects/{id}/claim")]
pub async fn claim(
    request: HttpRequest,
    state: web::Data<AppState>,
    who: Authenticated,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, SyncError> {
    let id = path.into_inner();

    // Membership first, and cheaply: somebody who is already in needs no
    // address read and no call to the provider. `claim_in` asks the same
    // question again, because it is the whole of the operation when it is
    // called directly; the answer here is what saves the round trip.
    if let Some(held) = super::held_by(&state.db, id, &who.subject).await? {
        return Ok(HttpResponse::Ok().json(json!({ "status": "already", "role": held })));
    }

    let token = bearer(&request).ok_or(SyncError::Unauthenticated)?;
    let presented = match state.idp.userinfo(&token).await {
        Ok(info) => sharing::presented(info.email.as_deref(), info.email_verified),
        // Kept as a state rather than returned here, so that failing closed is
        // a decision in one readable place rather than an early return that a
        // later edit can slip past.
        Err(SyncError::Idp(why)) => Presented::Unavailable(why),
        Err(other) => return Err(other),
    };

    match claim_in(&state.db, id, &who.subject, &presented).await? {
        Claim::Already(held) => {
            Ok(HttpResponse::Ok().json(json!({ "status": "already", "role": held })))
        }
        Claim::Grant { role, .. } => {
            Ok(HttpResponse::Ok().json(json!({ "status": "joined", "role": role })))
        }
        // The same answer a plan that does not exist gives, on purpose.
        Claim::NoInvite => Err(SyncError::NotFound),
        // About the caller's own account and not about this plan, so saying it
        // plainly reveals nothing about which plans are real.
        Claim::NotVerified(why) => Err(SyncError::BadRequest(why.to_string())),
        Claim::CannotCheck(why) => Err(SyncError::Idp(why)),
    }
}

/// The storage half of claiming, apart from the HTTP and the provider.
///
/// Its own function so that the part which writes a membership can be driven
/// against a real database without an identity provider in the way: the
/// provider's answer arrives as [`Presented`], which a test can simply state.
pub async fn claim_in(
    db: &DatabaseConnection,
    project: Uuid,
    subject: &str,
    presented: &Presented,
) -> Result<Claim, SyncError> {
    if let Some(held) = super::held_by(db, project, subject).await? {
        return Ok(Claim::Already(held));
    }
    // An answer about the caller's own account is settled before the plan is
    // touched at all, so that it reads the same whether or not the plan is
    // real.
    let Presented::Verified(address) = presented else {
        return Ok(sharing::decide(None, presented, None));
    };

    // The invitation is locked rather than merely read. Two claims arriving
    // together would otherwise both see it and both grant, which is one
    // invitation admitting two accounts. The loser blocks, then finds the row
    // deleted, and is told there is nothing here.
    let txn = db.begin().await?;
    let waiting = project_invites::Entity::find_by_id((project, address.clone()))
        .lock_exclusive()
        .one(&txn)
        .await?;
    let offered = waiting.as_ref().map(|row| Offered {
        email: row.email.clone(),
        role: row.role.clone(),
    });

    let decision = sharing::decide(None, presented, offered.as_ref());
    match &decision {
        Claim::Grant { role, email } => {
            project_members::ActiveModel {
                project_id: Set(project),
                subject: Set(subject.to_string()),
                role: Set(role.clone()),
                added_at: Set(Utc::now().fixed_offset()),
                email: Set(Some(email.clone())),
            }
            .insert(&txn)
            .await?;
            // Consumed in the same transaction that admits them, which is what
            // makes an invitation single use: somebody removed from a plan
            // cannot walk back in through the one they came by.
            project_invites::Entity::delete_by_id((project, email.clone()))
                .exec(&txn)
                .await?;
            txn.commit().await?;
        }
        _ => txn.rollback().await?,
    }
    Ok(decision)
}
