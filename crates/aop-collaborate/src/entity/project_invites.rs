//! An invitation waiting to be claimed.
//!
//! A row here is not access. It is an address, a role, and the statement that
//! whoever proves they hold that address may have that role. The proof is the
//! invitee's own token: nothing in this server ever looks an address up, so no
//! request to it can be used to find out whether an account exists.
//!
//! The row is deleted the moment it is claimed, in the same transaction that
//! writes the membership. That is what makes an invite single use without a
//! `claimed` flag to get wrong: there is nothing left to replay.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(schema_name = "aop", table_name = "project_invites")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub project_id: Uuid,
    /// Normalised before it is ever written: see [`crate::sharing::address`].
    /// The key is the pair, so inviting the same person twice is one invite
    /// whose role is whatever was said last, rather than two rows racing to
    /// decide what they get.
    #[sea_orm(primary_key, auto_increment = false)]
    pub email: String,
    pub role: String,
    /// The `sub` of the owner who sent it.
    pub invited_by: String,
    pub invited_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
