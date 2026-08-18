//! One plan being synced.

use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(schema_name = "aop", table_name = "projects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    /// The `sub` claim of whoever created it. The only identity this server
    /// keeps, because the identity provider owns everything else about them.
    pub owner_subject: String,
    /// The last seq handed out. Held on the row rather than derived from
    /// `MAX(seq)` so a push can lock one row and know that no other push can
    /// hand out the same number.
    pub head_seq: i64,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
