//! Pending invites, and the address a member came in by.
//!
//! Sharing needs one thing the first migration deliberately did not store: an
//! email address. The reason it is needed is that nobody knows anybody else's
//! `sub`. It is a UUID minted by the identity provider, it appears in no
//! interface, and an endpoint that took one would be asking a person to look
//! up something they cannot look up.
//!
//! Resolving an address to a subject on this side would mean asking the
//! provider "who is ada@example.com", which is an endpoint that answers
//! whether an account exists to anybody who can ask. That is not built here
//! and is not asked for there. Instead the address sits in this table until
//! the person it names turns up holding their own token, and the provider's
//! own answer about who that token belongs to is what matches them to it.
//!
//! ```text
//!   owner invites ada@example.com          -> aop.project_invites row
//!   Ada signs in and presents her token    -> userinfo says ada@example.com
//!   the two agree                          -> aop.project_members row,
//!                                             and the invite row is gone
//! ```
//!
//! So an address lives in `project_invites` only while the invite is pending.
//! What survives the claim is one copy of it on the member row, which exists
//! so the owner can tell one member from another: without it a sharing list
//! is a column of UUIDs and "remove this person" is a guess.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS aop.project_invites (
                project_id uuid        NOT NULL REFERENCES aop.projects(id) ON DELETE CASCADE,
                -- Already trimmed and lower cased when it arrives. Storing the
                -- normalised form rather than what was typed is what makes the
                -- primary key mean "one pending invite per person per plan",
                -- and it is what the owner sees: showing them the address that
                -- will actually be matched is more honest than showing them
                -- their own capitalisation.
                email      text        NOT NULL,
                role       varchar(16) NOT NULL,
                -- Who sent it. Kept because an invite is an act by an account
                -- and an owner reading the list is entitled to know which one,
                -- on a plan that has changed hands.
                invited_by text        NOT NULL,
                invited_at timestamptz NOT NULL DEFAULT now(),
                PRIMARY KEY (project_id, email)
            );
            -- A claim asks "is there an invite here for this address", which
            -- is the primary key, so no second index is added for it. This one
            -- is for the opposite question, asked by nothing yet and cheap to
            -- have when something does: which plans are waiting for a person.
            CREATE INDEX IF NOT EXISTS project_invites_email_idx
                ON aop.project_invites (email);
            "#,
        )
        .await?;

        // Nullable, and null is the ordinary case for whoever created the
        // plan: they were never invited, so there is no address to record.
        // Defaulting it to their sign in address would mean storing an
        // identity nobody asked this server to keep.
        db.execute_unprepared(
            r#"
            ALTER TABLE aop.project_members
                ADD COLUMN IF NOT EXISTS email text;
            "#,
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            ALTER TABLE aop.project_members DROP COLUMN IF EXISTS email;
            DROP TABLE IF EXISTS aop.project_invites;
            "#,
        )
        .await?;
        Ok(())
    }
}
