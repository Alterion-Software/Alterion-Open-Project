//! A whole plan as it stood at one seq.
//!
//! This is what a client gets when it has never synced, or when it has fallen
//! so far behind that the commands it would need to replay are gone. The
//! server cannot produce one itself, having no engine to run the commands
//! with, so snapshots arrive from clients.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(schema_name = "aop", table_name = "snapshots")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub project_id: Uuid,
    /// The seq this plan already includes. A client applies the snapshot and
    /// then asks for everything after this number.
    #[sea_orm(primary_key, auto_increment = false)]
    pub seq: i64,
    /// The serialised plan, stored as it arrived. The server does not parse
    /// it beyond checking that it is a plan, because the moment it does, it
    /// owns a schema the client is free to evolve.
    #[sea_orm(column_type = "JsonBinary")]
    pub plan: Json,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
