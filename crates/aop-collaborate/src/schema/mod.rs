//! The crate owns its schema and ships its migrations, the way alterion-auth
//! does, so deploying is running the binary and not running the binary after
//! finding the right `.sql` files.
//!
//! Everything lives in a Postgres schema called `aop`, so a self-hoster who
//! puts the identity provider and the sync server in one database still has
//! two clearly separated sets of tables.

use sea_orm_migration::prelude::*;

mod m20260818_000001_init_sync;
mod m20260819_000002_sharing;

/// Ordered migration list, for a host that wants to splice these into a
/// Migrator of its own.
pub fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
        Box::new(m20260818_000001_init_sync::Migration),
        Box::new(m20260819_000002_sharing::Migration),
    ]
}

/// What the binary runs on startup.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        migrations()
    }
}
