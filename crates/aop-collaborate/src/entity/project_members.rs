//! Who can see a plan. The table is here from the start because retrofitting
//! access control onto rows that were only ever owned by one person means
//! backfilling every one of them.
//!
//! A row is written when a plan is created, and when somebody claims an
//! invite. There is no third way in: see [`crate::sharing`].

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(schema_name = "aop", table_name = "project_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub project_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub subject: String,
    pub role: String,
    pub added_at: DateTimeWithTimeZone,
    /// The address the invite was claimed with, kept so that an owner can tell
    /// one member from another. A sharing list keyed by `sub` alone is a
    /// column of UUIDs, and "remove this person" against one of those is a
    /// guess.
    ///
    /// `None` for whoever created the plan: they were never invited, so there
    /// is no address this server was ever given. It is sent to the owner and
    /// to nobody else, which is the whole of the rule about addresses here.
    pub email: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
