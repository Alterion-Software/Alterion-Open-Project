//! Projects, membership, the log, and snapshots.
//!
//! Raw SQL rather than the sea-query builder, matching alterion-auth: the
//! composite keys and the partial ordering index read better as DDL than as
//! twenty lines of builder calls, and this is the file a self-hoster opens
//! when they want to know what is in their database.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("CREATE SCHEMA IF NOT EXISTS aop;").await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS aop.projects (
                id            uuid        PRIMARY KEY,
                name          text        NOT NULL,
                owner_subject text        NOT NULL,
                -- The last seq handed out. Zero means the log is empty, which
                -- is a real state: a project starts as a snapshot alone.
                head_seq      bigint      NOT NULL DEFAULT 0,
                created_at    timestamptz NOT NULL DEFAULT now(),
                updated_at    timestamptz NOT NULL DEFAULT now()
            );
            CREATE INDEX IF NOT EXISTS projects_owner_idx
                ON aop.projects (owner_subject);
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS aop.project_members (
                project_id uuid        NOT NULL REFERENCES aop.projects(id) ON DELETE CASCADE,
                subject    text        NOT NULL,
                role       varchar(16) NOT NULL,
                added_at   timestamptz NOT NULL DEFAULT now(),
                PRIMARY KEY (project_id, subject)
            );
            -- Listing is "what can this subject see", so the index leads with
            -- the subject rather than the project.
            CREATE INDEX IF NOT EXISTS project_members_subject_idx
                ON aop.project_members (subject);
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS aop.changes (
                project_id     uuid        NOT NULL REFERENCES aop.projects(id) ON DELETE CASCADE,
                seq            bigint      NOT NULL,
                at             timestamptz NOT NULL,
                author_subject text        NOT NULL,
                author_name    text        NOT NULL,
                script         text        NOT NULL,
                summary        text        NOT NULL,
                -- The unique index the sync depends on: two pushes racing must
                -- not both be given seq 43, and the primary key is what says so
                -- even if the application logic is ever wrong.
                PRIMARY KEY (project_id, seq)
            );
            "#,
        )
        .await?;

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS aop.snapshots (
                project_id uuid        NOT NULL REFERENCES aop.projects(id) ON DELETE CASCADE,
                seq        bigint      NOT NULL,
                plan       jsonb       NOT NULL,
                created_at timestamptz NOT NULL DEFAULT now(),
                PRIMARY KEY (project_id, seq)
            );
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            DROP TABLE IF EXISTS aop.snapshots;
            DROP TABLE IF EXISTS aop.changes;
            DROP TABLE IF EXISTS aop.project_members;
            DROP TABLE IF EXISTS aop.projects;
            "#,
        )
        .await?;
        Ok(())
    }
}
