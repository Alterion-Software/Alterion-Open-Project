//! The tables, as sea-orm sees them. The DDL that creates them lives in
//! [`crate::schema`], and these are deliberately thin: this server stores
//! commands and plans, and has no opinion about either.

pub mod changes;
pub mod project_invites;
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

    /// The role an invite may name, or nothing.
    ///
    /// Owner is refused on purpose. Handing it out through an invite would let
    /// a plan acquire a second owner, each able to remove the other, and it
    /// would make "the owner" two things at once: the subject on the project
    /// row, and a role on the members table. Passing a plan to somebody else
    /// is a real feature, and it is not this one.
    pub fn invitable(role: &str) -> Option<&'static str> {
        match role.trim().to_ascii_lowercase().as_str() {
            EDITOR => Some(EDITOR),
            VIEWER => Some(VIEWER),
            _ => None,
        }
    }
}
