//! The log. Append only: nothing in this server ever updates or deletes a
//! row here, and the day something needs to, it is a new row saying so.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(schema_name = "aop", table_name = "changes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub project_id: Uuid,
    /// The server's own ordering, and the sync cursor. Counts from 1.
    #[sea_orm(primary_key, auto_increment = false)]
    pub seq: i64,
    /// When the planner made the edit, which is not when the server received
    /// it: a client that worked offline for a day pushes yesterday's moment.
    pub at: DateTimeWithTimeZone,
    /// The account that pushed it, from the token. Not from the body, so a
    /// client cannot sign somebody else's name to a command.
    pub author_subject: String,
    /// The name the history panel shows, which the client does send, because
    /// the token has no display name in it and a plan is read by people.
    pub author_name: String,
    pub script: String,
    pub summary: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// The log entry as the client's own history holds it.
    ///
    /// `id` is the server's seq, not whatever the client called it locally.
    /// The two logs are the same shape so that after a sync they are also the
    /// same numbers, and one cursor means one thing on both sides.
    pub fn to_change(&self) -> aop_core::history::Change {
        aop_core::history::Change {
            id: self.seq.max(0) as u64,
            at: self.at.naive_utc(),
            author: self.author_name.clone(),
            script: self.script.clone(),
            summary: self.summary.clone(),
        }
    }
}
