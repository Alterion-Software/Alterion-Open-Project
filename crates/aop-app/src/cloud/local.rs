//! Where a plan that came off a server lives on this machine.
//!
//! A plan opened from a link has no file behind it. Before this, it existed in
//! the window and nowhere else: its tasks, its change log, and anything in
//! that log which had not reached the server yet. Closing the application lost
//! all of it, and there was nothing to reopen. That was survivable while a
//! shared plan meant "fetch it, edit it, press Sync"; it stopped being
//! survivable the moment edits stream, because streaming is what makes people
//! work in a shared plan for an afternoon without ever pressing Save, exactly
//! as they would in a web application, and expect the same guarantees.
//!
//! So a plan opened from a server gets a copy here, keyed by the project id:
//!
//! ```text
//!   <config root>/plans/<project id>.aprj
//! ```
//!
//! An ordinary `.aprj`, not a second format. It is the format that already
//! carries a plan and its change log, and the log is what carries the unsent
//! work: whatever was waiting when the window closed is still waiting when it
//! opens, and goes when the socket next comes up.
//!
//! **This is a home, not a backup.** It holds one copy, it is overwritten as
//! work happens, and it keeps no history of itself. The crash snapshot in
//! [`crate::recovery`] is a different thing for a different moment. Nothing
//! here should ever be described to somebody as their backup.
//!
//! **The cursor does not live in the file.** It lives where every other
//! cursor lives, in [`crate::cloud::link`], keyed by this file's path. That is
//! deliberate: a cursor inside a `.aprj` would travel with the file, and a
//! copy handed to somebody else would then claim to have already read work it
//! has never seen. Keying the link by the local copy's path means the plan and
//! the cursor are stored apart but can only ever be found together, so the two
//! cannot drift.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aop_core::{Project, persist};

/// The folder the copies live in.
fn directory() -> Option<PathBuf> {
    Some(crate::settings::config_root()?.join("plans"))
}

/// A project id reduced to something safe to build a path out of.
///
/// The id comes off a server, so it is not this application's to trust with a
/// filename. Anything that is not a plain identifier character goes, which
/// takes `..` and every separator with it, and an id left with nothing is no
/// id at all rather than a file called nothing.
fn file_stem(project: &str) -> Option<String> {
    let stem: String = project
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(128)
        .collect();
    (!stem.is_empty()).then_some(stem)
}

/// Where this project's local copy belongs, whether or not it is there yet.
pub fn path_for(project: &str) -> Option<PathBuf> {
    Some(directory()?.join(format!("{}.aprj", file_stem(project)?)))
}

/// The local copy of this project, if there is one to open.
///
/// A copy that will not open is treated as absent rather than as an error.
/// The server still has the plan, so the worst case is the download this was
/// meant to avoid, and a dialog about a cache file is a dialog about something
/// the planner never asked for.
pub fn load(project: &str) -> Option<(PathBuf, Project)> {
    let path = path_for(project)?;
    let plan = persist::open(&path).ok()?;
    Some((path, plan))
}

/// Write the plan to its local copy, on a thread of its own.
///
/// Off the interface thread because serialising and deflating a plan of a few
/// thousand tasks is real work and this runs while somebody is typing. `busy`
/// is held for the length of the write so two of these cannot be in flight at
/// once: the later one could finish first and leave the older plan on disk.
pub fn write_in_background(path: PathBuf, plan: Project, busy: Arc<AtomicBool>) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let done = Arc::clone(&busy);
    let started = std::thread::Builder::new()
        .name("aop-local-copy".into())
        .spawn(move || {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // Quiet on failure, like the settings and the recovery snapshot
            // beside it. This runs behind the planner's back, and the next
            // tick tries again.
            let _ = persist::save(&path, &plan);
            done.store(false, Ordering::SeqCst);
        });
    if started.is_err() {
        // A thread that would not start must not leave the flag set, or the
        // local copy silently stops being written from here on, which is the
        // one failure this whole file exists to prevent.
        busy.store(false, Ordering::SeqCst);
    }
}

/// Drop a local copy, and the cursor kept beside it.
///
/// Both together, always. A plan file with no link is one that would open
/// with no idea where it had got to; a link with no plan file is a line
/// pointing at nothing that a later copy could inherit.
pub fn discard(path: &Path) {
    let _ = std::fs::remove_file(path);
    crate::cloud::link::forget(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_id_cannot_name_a_file_outside_the_folder() {
        // The id comes off a server. A server that sends one full of
        // separators must not get to choose where this application writes.
        assert_eq!(file_stem("0198f0c2-1111-4222-8333-444455556666").as_deref(),
                   Some("0198f0c2-1111-4222-8333-444455556666"));
        assert_eq!(file_stem("../../etc/passwd").as_deref(), Some("etcpasswd"));
        assert_eq!(file_stem("..").as_deref(), None);
        assert_eq!(file_stem("/").as_deref(), None);
        assert_eq!(file_stem(""), None);
    }

    #[test]
    fn an_absurdly_long_id_does_not_become_an_absurdly_long_name() {
        let stem = file_stem(&"a".repeat(4_000)).expect("still an id");
        assert_eq!(stem.len(), 128);
    }
}
