//! Keeping unsaved work through a crash, a kill, or a forgotten window.
//!
//! Saving is something a person has to remember to do, and the moments when
//! they most need the plan back are exactly the ones where they were never
//! asked: a crash, a power cut, `kill -9`, a compositor restart. So the plan is
//! written to a snapshot beside the settings on a timer, entirely separately
//! from wherever the user saves it, and that snapshot is offered back on the
//! next start.
//!
//! A snapshot is not a save. It never touches the user's file, it is not what
//! Save As writes, and accepting one leaves the plan unsaved so that the user
//! still chooses where it lands. Its only job is that nothing is ever lost
//! between one save and the next.

use std::path::{Path, PathBuf};

use aop_core::{persist, Project};

/// How long between snapshots while there is unsaved work.
///
/// Short enough that a crash costs little, long enough that a plan of a few
/// hundred tasks is not being serialised constantly. Nothing is written at all
/// while the plan is unchanged.
pub const INTERVAL_SECONDS: u64 = 30;

/// Where snapshots live: beside the other application state, not beside the
/// user's plans, so a recovery file never turns up in their documents.
fn directory() -> Option<PathBuf> {
    let base = crate::settings::config_root()?;
    Some(base.join("recovery"))
}

/// This session's snapshot, named for the process that owns it.
///
/// Naming it after the process is what lets a later start tell a snapshot left
/// by a crash from one belonging to a copy of the application that is still
/// running, so two open windows never offer each other's work back.
fn snapshot_path() -> Option<PathBuf> {
    Some(directory()?.join(format!("session-{}.aprj", std::process::id())))
}

/// Where the plan came from, remembered alongside the snapshot.
fn origin_path(snapshot: &Path) -> PathBuf {
    snapshot.with_extension("origin")
}

/// A snapshot left behind by a session that did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovered {
    /// The snapshot itself.
    pub snapshot: PathBuf,
    /// The file the plan had been opened from, if it had one.
    pub origin: Option<PathBuf>,
    /// What the plan was called, for the sake of asking about it.
    pub name: String,
}

/// Write the plan to this session's snapshot.
///
/// Failures are deliberately quiet. This runs on a timer behind the user's
/// back, and a warning about a snapshot they did not ask for would interrupt
/// work to report something that has not gone wrong from their point of view.
/// The next tick tries again.
pub fn write(project: &Project, origin: Option<&Path>) {
    let Some(path) = snapshot_path() else { return };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }

    if persist::save(&path, project).is_err() {
        return;
    }
    // Only once the plan itself is safely down is it worth recording where it
    // came from, so the two can never disagree.
    match origin {
        Some(origin) => {
            let _ = std::fs::write(origin_path(&path), origin.to_string_lossy().as_bytes());
        }
        None => {
            let _ = std::fs::remove_file(origin_path(&path));
        }
    }
}

/// Drop this session's snapshot, once there is nothing left to recover.
pub fn discard() {
    let Some(path) = snapshot_path() else { return };
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(origin_path(&path));
}

/// Whether a process is still running.
///
/// A snapshot belonging to a live process is another open window's work in
/// progress, not something to offer back.
#[cfg(target_os = "linux")]
fn is_running(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "linux"))]
fn is_running(_pid: u32) -> bool {
    // Without a cheap way to ask, assume the session has gone. A snapshot is
    // only ever offered, never applied on its own, so the cost of being wrong
    // is a question the user can decline.
    false
}

/// Pull the process id back out of a snapshot's name.
fn pid_of(path: &Path) -> Option<u32> {
    path.file_stem()?
        .to_string_lossy()
        .strip_prefix("session-")?
        .parse()
        .ok()
}

/// Find work left behind by sessions that never finished.
pub fn find_abandoned() -> Vec<Recovered> {
    let Some(dir) = directory() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(persist::FILE_EXTENSION) {
            continue;
        }
        // Our own snapshot, and those of other running copies, are not lost.
        match pid_of(&path) {
            Some(pid) if pid == std::process::id() || is_running(pid) => continue,
            Some(_) => {}
            None => continue,
        }

        // A snapshot that will not open is worse than none: offering it would
        // promise work back and then fail to produce it.
        let Ok(project) = persist::open(&path) else {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(origin_path(&path));
            continue;
        };

        let origin = std::fs::read_to_string(origin_path(&path))
            .ok()
            .map(|text| PathBuf::from(text.trim()))
            .filter(|p| !p.as_os_str().is_empty());

        found.push(Recovered {
            snapshot: path,
            origin,
            name: project.name.clone(),
        });
    }

    found
}

/// Forget a snapshot the user has decided against, or has already taken back.
pub fn clear(snapshot: &Path) {
    let _ = std::fs::remove_file(snapshot);
    let _ = std::fs::remove_file(origin_path(snapshot));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_named_for_the_session_that_owns_it() {
        // Two copies of the application running at once must not offer each
        // other's work back as though it had been lost.
        let path = snapshot_path().expect("a config directory");
        assert_eq!(pid_of(&path), Some(std::process::id()));
    }

    #[test]
    fn a_name_without_a_session_is_not_treated_as_a_snapshot() {
        assert_eq!(pid_of(Path::new("/tmp/notes.aprj")), None);
        assert_eq!(pid_of(Path::new("/tmp/session-abc.aprj")), None);
    }

    #[test]
    fn the_origin_sits_beside_the_snapshot() {
        let snapshot = PathBuf::from("/tmp/session-42.aprj");
        assert_eq!(origin_path(&snapshot), PathBuf::from("/tmp/session-42.origin"));
    }

    #[test]
    fn this_sessions_own_work_is_never_offered_back_to_it() {
        // It is not lost; it is on screen.
        let ours = snapshot_path().expect("a config directory");
        let pid = pid_of(&ours).expect("a session id");
        assert!(is_running(pid), "the process asking is certainly running");
    }
}
