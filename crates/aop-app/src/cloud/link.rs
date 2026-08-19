//! Which plan on the server a plan on this machine is.
//!
//! Two numbers have to survive between runs for a sync to mean anything: the
//! project this plan is on the server, and how far down the server's log this
//! copy has read. Neither belongs in the plan file. A plan sent to somebody
//! else, or copied to another machine, is a different client of the same
//! project and starts its own cursor; carrying one inside the file would have
//! the copy claim to have already pulled work it has never seen.
//!
//! So the link is kept beside the settings, keyed by where the plan lives on
//! this machine, in a file a person can read and delete:
//!
//! ```text
//!   <project id> <cursor> <path to the plan>
//! ```
//!
//! Written in that order so the path can contain anything, spaces included,
//! and still be the rest of the line.
//!
//! The cursor is the server's `seq`, not the plan's own change id, and the two
//! are different numbers. `History::pushed_through` says how far this plan has
//! sent; this says how far the server had got when it was last spoken to.
//! Keeping them apart is what stops a client that has been handed a
//! renumbered log from asking for changes by the wrong name.

use std::path::{Path, PathBuf};

/// What is known about one plan's home on a server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Link {
    /// The project's id on the server.
    pub project: String,
    /// The last seq this copy has read. Zero means "nothing yet", which is
    /// what the protocol means by a cursor of zero as well.
    pub cursor: i64,
}

/// Where the links live: beside the settings, not beside the user's plans.
fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("alterion-open-project").join("collaborate.cfg"))
}

/// Turn the stored lines into pairs, skipping anything that no longer parses.
///
/// A line that cannot be read is dropped rather than failing the file. The
/// cost of being wrong is one plan that has to be linked again, against a
/// syncing feature that refuses to start at all.
fn parse(text: &str) -> Vec<(PathBuf, Link)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let (Some(project), Some(cursor), Some(plan)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Ok(cursor) = cursor.parse::<i64>() else {
            continue;
        };
        found.push((
            PathBuf::from(plan),
            Link {
                project: project.to_string(),
                cursor,
            },
        ));
    }
    found
}

fn render(links: &[(PathBuf, Link)]) -> String {
    let mut out = String::from(
        "# Which plan on a server each plan on this machine is.\n\
         # <project id> <cursor> <path>. Delete a line to unlink that plan.\n",
    );
    for (plan, link) in links {
        out.push_str(&format!(
            "{} {} {}\n",
            link.project,
            link.cursor,
            plan.display()
        ));
    }
    out
}

fn all() -> Vec<(PathBuf, Link)> {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// What is known about this plan, if anything.
pub fn load(plan: &Path) -> Option<Link> {
    all()
        .into_iter()
        .find(|(stored, _)| stored == plan)
        .map(|(_, link)| link)
}

/// Remember where this plan lives, replacing whatever was known before.
///
/// Quiet on failure, like the settings it sits beside: a plan that has to be
/// linked again is a smaller problem than a dialog in the middle of a sync.
pub fn save(plan: &Path, link: &Link) {
    let mut links = all();
    match links.iter_mut().find(|(stored, _)| stored == plan) {
        Some((_, held)) => *held = link.clone(),
        None => links.push((plan.to_path_buf(), link.clone())),
    }
    write(&links);
}

/// Forget a plan's link, for a plan that is no longer shared.
pub fn forget(plan: &Path) {
    let mut links = all();
    links.retain(|(stored, _)| stored != plan);
    write(&links);
}

fn write(links: &[(PathBuf, Link)]) {
    let Some(path) = path() else { return };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let _ = std::fs::write(path, render(links));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_survives_being_written_and_read_back() {
        let links = vec![(
            PathBuf::from("/home/ada/plans/bridge.aprj"),
            Link {
                project: "0198f0c2-1111-4222-8333-444455556666".into(),
                cursor: 45,
            },
        )];
        assert_eq!(parse(&render(&links)), links);
    }

    #[test]
    fn a_path_with_spaces_in_it_comes_back_whole() {
        // The path is the rest of the line for exactly this reason.
        let links = vec![(
            PathBuf::from("/home/ada/My Plans/the second bridge.aprj"),
            Link {
                project: "a-project".into(),
                cursor: 0,
            },
        )];
        assert_eq!(parse(&render(&links)), links);
    }

    #[test]
    fn a_line_that_no_longer_parses_is_skipped_rather_than_failing_the_file() {
        let text = "a-project not-a-number /home/ada/one.aprj\n\
                    b-project 7 /home/ada/two.aprj\n";
        let found = parse(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.cursor, 7);
    }

    #[test]
    fn a_cursor_of_zero_is_a_real_answer_and_not_an_absent_one() {
        // Zero means "I have nothing", which is what the protocol means by it
        // too, and is different from having no link at all.
        let found = parse("a-project 0 /home/ada/one.aprj\n");
        assert_eq!(found[0].1.cursor, 0);
    }
}
