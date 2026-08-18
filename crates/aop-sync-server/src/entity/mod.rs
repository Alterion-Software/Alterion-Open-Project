//! The tables, as sea-orm sees them. The DDL that creates them lives in
//! [`crate::schema`], and these are deliberately thin: this server stores
//! commands and plans, and has no opinion about either.

pub mod changes;
pub mod project_members;
pub mod projects;
pub mod snapshots;

/// Roles a member can hold. Kept as strings in the database rather than an
/// enum type, so adding one later is a code change and not a migration that
/// locks the table.
pub mod role {
    pub const OWNER: &str = "owner";
    pub const EDITOR: &str = "editor";
    pub const VIEWER: &str = "viewer";

    /// Whether this role may append to the log.
    pub fn may_write(role: &str) -> bool {
        matches!(role, OWNER | EDITOR)
    }
}
