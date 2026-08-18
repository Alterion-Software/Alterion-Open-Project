//! Inserting one plan inside another, the way Project's Subproject command does.
//!
//! A programme is rarely one file. Each workstream is planned by whoever runs
//! it, and the programme plan is those files brought together under a single
//! outline. Inserting a plan puts its whole outline under a new summary row
//! named after the file, one level deeper than that row, so the host reads as
//! phases whose contents happen to have been planned somewhere else.
//!
//! Two numbering schemes exist in every plan and both are re-issued on the way
//! in. Task identifiers are handed out from 1 by whichever plan created the
//! task, so two plans that were never meant to meet almost always both hold
//! tasks 1, 2 and 3. Carrying those numbers across would not fail loudly:
//! links, assignments and external references are all stored as numbers, so a
//! link meant for the incoming task 3 would quietly attach itself to the host's
//! task 3 and the result would schedule perfectly while describing work nobody
//! agreed to. Every incoming task therefore takes a fresh identifier from the
//! host, and everything that pointed at the old one is rewritten to match.
//!
//! Resources go the other way. A person is known by their name rather than by a
//! number, so an Ana Reyes arriving from another file is the Ana Reyes the host
//! already has, booked onto the host's record of her at the host's rates.
//! Duplicating her would split her workload across two rows and hide every
//! overallocation she is in.
//!
//! # What this does not do
//!
//! The inserted plan is a copy, not a live link. Project offers both and this
//! is the unlinked half of that choice: later edits to the source file do not
//! appear here, and edits made here are never written back to it.
//!
//! That is the safer default because the alternative writes to a file the
//! planner never opened. A live subproject means saving the programme plan
//! silently rewrites somebody else's file, and opening the programme plan
//! anywhere that file cannot be reached leaves rows that cannot be read or
//! scheduled. A copy is openable everywhere, is consistent on its own, and
//! surprises nobody whose file it came from. Re-inserting is how a copy is
//! refreshed, which is a visible act rather than a silent one.

use std::collections::HashMap;
use std::path::Path;

use crate::model::{ExternalDependency, ExternalId, Project, Resource, ResourceId, Task, TaskId};

/// What an insertion added, so the caller can select the new rows and say what
/// happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inserted {
    /// The row the new summary landed on.
    pub summary_row: usize,
    /// Rows brought in beneath it, not counting the summary itself.
    pub task_count: usize,
    /// Links that came with them.
    pub link_count: usize,
}

#[derive(Debug)]
pub enum SubprojectError {
    /// The file could not be read as a plan at all.
    CannotOpen { name: String, detail: String },
    /// It read as a plan, but there is nothing in it to insert.
    NothingToInsert { name: String },
}

impl std::fmt::Display for SubprojectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubprojectError::CannotOpen { name, detail } => write!(
                f,
                "{name} could not be opened as a plan, so nothing was inserted: {detail}"
            ),
            SubprojectError::NothingToInsert { name } => write!(
                f,
                "{name} has no tasks in it, so there is nothing to insert."
            ),
        }
    }
}

impl std::error::Error for SubprojectError {}

/// Insert the plan held in `source` at row `at`.
///
/// Reads whatever the application can open, so a workstream planned in Project
/// or handed over as a spreadsheet inserts exactly like one of ours.
pub fn insert(project: &mut Project, source: &Path, at: usize) -> Result<Inserted, SubprojectError> {
    let incoming =
        crate::persist::open_any(source).map_err(|detail| SubprojectError::CannotOpen {
            name: file_label(source),
            detail,
        })?;

    // An empty plan would leave behind a bare row named after a file, which
    // reads exactly like an insertion that worked.
    if incoming.tasks.is_empty() {
        return Err(SubprojectError::NothingToInsert {
            name: file_label(source),
        });
    }

    let inserted = insert_plan(project, incoming, &plan_name(source), at);
    if let Some(summary) = project.tasks.get_mut(inserted.summary_row) {
        summary.notes = source_note(source);
    }
    Ok(inserted)
}

/// Insert a plan already in hand, under a summary row called `name`.
///
/// This is where the renumbering happens, and it is kept separate from the
/// reading of files so it can be exercised against two plans that deliberately
/// share their identifiers.
pub fn insert_plan(project: &mut Project, incoming: Project, name: &str, at: usize) -> Inserted {
    let at = at.min(project.tasks.len());

    // The summary takes the level of the row it displaces, matching what
    // inserting a plain task does. Appending takes the level of the last row.
    let level = project
        .tasks
        .get(at)
        .or_else(|| project.tasks.last())
        .map_or(0, |task| task.outline_level);

    let resources = merge_resources(project, &incoming.resources);
    let externals = merge_externals(project, &incoming.external);

    // The host's meaning for a slot wins. It is the plan being added to, and
    // one column cannot carry two names. Slots it has never used are free, so
    // the incoming plan's name and lookup list are worth keeping there.
    for (slot, field) in incoming.custom_fields {
        project.custom_fields.entry(slot).or_insert(field);
    }

    let mut ids: HashMap<TaskId, TaskId> = HashMap::with_capacity(incoming.tasks.len());
    let mut rows: Vec<Task> = Vec::with_capacity(incoming.tasks.len() + 1);

    let summary_id = project.allocate_task_id();
    let mut summary = Task::new(summary_id, name, 0);
    summary.outline_level = level;
    rows.push(summary);

    for mut task in incoming.tasks {
        let fresh = project.allocate_task_id();
        ids.insert(task.id, fresh);
        task.id = fresh;
        task.outline_level = task.outline_level.saturating_add(level).saturating_add(1);

        let assignments = std::mem::take(&mut task.assignments);
        task.assignments = assignments
            .into_iter()
            .filter_map(|mut booking| {
                // A booking naming a resource that came with no record names
                // nobody. Leaving the number as it stands would book whoever
                // the host happens to hold under that number.
                resources.get(&booking.resource).map(|&id| {
                    booking.resource = id;
                    booking
                })
            })
            .collect();

        let waiting_on = std::mem::take(&mut task.external_predecessors);
        task.external_predecessors = waiting_on
            .into_iter()
            .filter_map(|old| externals.get(&old).copied())
            .collect();

        rows.push(task);
    }

    let mut link_count = 0;
    for link in incoming.links {
        // Both ends have to be rows that actually arrived. An end that is not
        // in the map is dropped rather than kept as it stands, because the
        // number it holds belongs to a host task now.
        let (Some(&predecessor), Some(&successor)) =
            (ids.get(&link.predecessor), ids.get(&link.successor))
        else {
            continue;
        };
        let mut moved = link;
        moved.predecessor = predecessor;
        moved.successor = successor;
        if project.add_link(moved) {
            link_count += 1;
        }
    }

    let task_count = rows.len() - 1;
    project.tasks.splice(at..at, rows);

    Inserted {
        summary_row: at,
        task_count,
        link_count,
    }
}

/// Where an inserted plan came from, so the summary row can say so.
pub fn source_note(path: &Path) -> String {
    format!(
        "Inserted from {}.\n\
         This is a copy taken at the time it was inserted, so later changes to \
         that file do not appear here.",
        path.display()
    )
}

/// What to call the file when telling someone about it: its own name, or the
/// whole path when there is no name to show.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The name the summary row takes: the file's name without its extension,
/// since that is what the planner filed the workstream under.
fn plan_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "Subproject".to_string())
}

/// Give every incoming resource a host resource to stand for it, adding only
/// the ones the host has never heard of.
///
/// Matching is on the name because that is the only thing about a person two
/// plans can be expected to agree on: the numbers were handed out separately.
/// Two different people with one name do merge, which is the same trade the
/// Resource Names cell already makes, and is far cheaper to notice and undo
/// than a workload silently split in half.
fn merge_resources(
    project: &mut Project,
    incoming: &[Resource],
) -> HashMap<ResourceId, ResourceId> {
    let mut map = HashMap::with_capacity(incoming.len());
    for resource in incoming {
        let name = resource.name.trim();
        let held = project
            .resources
            .iter()
            .find(|existing| existing.name.trim().eq_ignore_ascii_case(name))
            .map(|existing| existing.id);

        let id = match held {
            // The host's record wins: its rates and calendar are the ones the
            // plan being added to has been costed against.
            Some(id) => id,
            None => {
                let mut copy = resource.clone();
                copy.id = project.allocate_resource_id();
                let id = copy.id;
                project.resources.push(copy);
                id
            }
        };
        map.insert(resource.id, id);
    }
    map
}

/// Bring across what the incoming plan waits on outside itself.
///
/// One purchase order referenced by two plans is still one purchase order, so a
/// reference the host already holds is reused rather than listed twice. A blank
/// reference identifies nothing and so never counts as a match.
fn merge_externals(
    project: &mut Project,
    incoming: &[ExternalDependency],
) -> HashMap<ExternalId, ExternalId> {
    let mut map = HashMap::with_capacity(incoming.len());
    for entry in incoming {
        let reference = entry.reference.trim();
        let held = if reference.is_empty() {
            None
        } else {
            project
                .external
                .iter()
                .find(|existing| {
                    existing.reference.trim().eq_ignore_ascii_case(reference)
                        && existing.source.trim().eq_ignore_ascii_case(entry.source.trim())
                })
                .map(|existing| existing.id)
        };

        let id = match held {
            Some(id) => id,
            None => {
                let mut copy = entry.clone();
                copy.id = project.allocate_external_id();
                let id = copy.id;
                project.external.push(copy);
                id
            }
        };
        map.insert(entry.id, id);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::{CustomField, CustomKind, Slot};
    use crate::model::{Assignment, Link, LinkType};
    use chrono::NaiveDate;
    use std::collections::HashSet;

    fn at_eight(y: i32, m: u32, d: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    /// A plan whose task identifiers start at 1, which is what every plan does
    /// and is exactly why they collide.
    fn plan(rows: &[(u16, &str)]) -> Project {
        let mut project = Project::blank(at_eight(2026, 8, 17));
        project.tasks.clear();
        for &(level, name) in rows {
            let id = project.allocate_task_id();
            let mut task = Task::new(id, name, 480);
            task.outline_level = level;
            project.tasks.push(task);
        }
        project
    }

    fn host() -> Project {
        plan(&[(0, "Host A"), (0, "Host B"), (0, "Host C")])
    }

    fn wing() -> Project {
        plan(&[(0, "Wing A"), (0, "Wing B"), (0, "Wing C")])
    }

    fn id_of(project: &Project, name: &str) -> TaskId {
        project
            .tasks
            .iter()
            .find(|task| task.name == name)
            .map(|task| task.id)
            .unwrap_or_else(|| panic!("no row called {name}"))
    }

    fn names(project: &Project) -> Vec<&str> {
        project.tasks.iter().map(|task| task.name.as_str()).collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("aop-subproject-tests");
        std::fs::create_dir_all(&dir).expect("a writable temporary directory");
        dir.join(name)
    }

    #[test]
    fn the_inserted_plan_becomes_a_summary_row_named_after_the_plan() {
        let mut project = host();
        let report = insert_plan(&mut project, wing(), "East Wing", 3);

        assert_eq!(report.summary_row, 3);
        assert_eq!(report.task_count, 3, "the summary is not one of its own rows");
        assert_eq!(project.tasks[3].name, "East Wing");
        assert!(
            project.is_summary(3),
            "it carries the plan's rows, so it reads as a summary"
        );
        assert_eq!(project.descendants(3).len(), 3);
    }

    #[test]
    fn identifiers_are_reallocated_so_two_plans_using_the_same_ids_do_not_merge() {
        // Both plans hand out identifiers from 1, so both hold tasks 1, 2 and
        // 3 before anything happens. Keeping those numbers would make three
        // pairs of unrelated tasks indistinguishable to every link, booking
        // and reference in the file.
        let mut project = host();
        let incoming = wing();
        assert_eq!(
            project.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            incoming.tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "the collision is the normal case, not a contrived one"
        );

        insert_plan(&mut project, incoming, "East Wing", 3);

        let all: Vec<TaskId> = project.tasks.iter().map(|task| task.id).collect();
        let distinct: HashSet<TaskId> = all.iter().copied().collect();
        assert_eq!(all.len(), 7, "three host rows, a summary and three more");
        assert_eq!(distinct.len(), all.len(), "and no two rows share a number");

        for (row, name) in [(0, "Host A"), (1, "Host B"), (2, "Host C")] {
            assert_eq!(
                project.tasks[row].id,
                row as TaskId + 1,
                "{name} keeps the number it always had"
            );
        }
        for name in ["Wing A", "Wing B", "Wing C"] {
            assert!(
                id_of(&project, name) > 3,
                "{name} was given a number the host was not already using"
            );
        }
        assert_eq!(
            project.task(1).map(|t| t.name.as_str()),
            Some("Host A"),
            "task 1 is still the host's task 1"
        );
    }

    #[test]
    fn links_inside_the_inserted_plan_are_remapped_and_still_join_the_same_two_rows() {
        let mut project = host();
        project.add_link(Link::finish_to_start(1, 2));

        let mut incoming = wing();
        // Wing A to Wing C, under numbers the host is also using.
        incoming.links.push(Link {
            predecessor: 1,
            successor: 3,
            kind: LinkType::SS,
            lag_minutes: 480,
        });

        let report = insert_plan(&mut project, incoming, "East Wing", 3);
        assert_eq!(report.link_count, 1);
        assert_eq!(project.links.len(), 2, "the host's own link is untouched");

        let carried = project
            .links
            .iter()
            .find(|link| link.kind == LinkType::SS)
            .expect("the inserted link survived");
        assert_eq!(carried.lag_minutes, 480, "with its lag intact");

        // Resolved through the whole plan rather than compared to a number:
        // a number the host is also using would find the host's row first, and
        // that is precisely the mistake being tested for.
        let row_named = |id: TaskId| {
            project
                .index_of(id)
                .map(|row| project.tasks[row].name.clone())
                .unwrap_or_default()
        };
        assert_eq!(row_named(carried.predecessor), "Wing A");
        assert_eq!(row_named(carried.successor), "Wing C");

        assert!(
            project.links.contains(&Link::finish_to_start(1, 2)),
            "Host A still runs into Host B"
        );
    }

    #[test]
    fn no_link_from_the_inserted_plan_lands_on_a_task_of_the_host() {
        // This is the failure the renumbering exists to prevent, and it is a
        // silent one: the plan would schedule without complaint while joining
        // rows nobody joined.
        let mut project = host();
        let mut incoming = wing();
        incoming.links.push(Link::finish_to_start(1, 2));
        incoming.links.push(Link::finish_to_start(2, 3));

        insert_plan(&mut project, incoming, "East Wing", 0);

        let host_ids: HashSet<TaskId> = HashSet::from([1, 2, 3]);
        for link in &project.links {
            assert!(
                !host_ids.contains(&link.predecessor) && !host_ids.contains(&link.successor),
                "a link from the inserted plan reached a host task: {link:?}"
            );
        }
        assert!(
            project.successors_of(1).is_empty(),
            "Host A gained nothing from a plan that never mentioned it"
        );
        assert_eq!(project.links.len(), 2, "and both incoming links survived");
    }

    #[test]
    fn everything_from_the_inserted_plan_sits_one_level_deeper_than_its_summary() {
        let mut project = host();
        let incoming = plan(&[(0, "Phase"), (1, "Cut"), (1, "Fit")]);

        let report = insert_plan(&mut project, incoming, "East Wing", 3);
        let levels: Vec<u16> = project.tasks[report.summary_row..]
            .iter()
            .map(|task| task.outline_level)
            .collect();

        assert_eq!(levels, vec![0, 1, 2, 2], "the whole shape moved down one");
        assert!(project.is_summary(report.summary_row));
        assert!(project.is_summary(report.summary_row + 1), "and Phase still is");
    }

    #[test]
    fn the_summary_takes_the_outline_level_of_the_row_it_displaces() {
        let mut project = plan(&[(0, "Programme"), (1, "Host A"), (1, "Host B")]);

        let report = insert_plan(&mut project, wing(), "East Wing", 2);

        assert_eq!(
            project.tasks[report.summary_row].outline_level, 1,
            "it goes in beside the row it pushed down, not above it"
        );
        assert_eq!(project.tasks[report.summary_row + 1].outline_level, 2);
        assert_eq!(
            project.parent_index(report.summary_row),
            Some(0),
            "so it belongs to the programme it was dropped into"
        );
    }

    #[test]
    fn existing_rows_are_pushed_down_rather_than_overwritten() {
        let mut project = host();
        insert_plan(&mut project, wing(), "East Wing", 1);

        assert_eq!(
            names(&project),
            vec![
                "Host A",
                "East Wing",
                "Wing A",
                "Wing B",
                "Wing C",
                "Host B",
                "Host C"
            ]
        );
    }

    #[test]
    fn an_insertion_point_past_the_end_is_clamped_rather_than_panicking() {
        let mut project = host();
        let report = insert_plan(&mut project, wing(), "East Wing", 999);
        assert_eq!(report.summary_row, 3, "it lands at the end");
        assert_eq!(names(&project).last(), Some(&"Wing C"));

        let mut empty = Project::blank(at_eight(2026, 8, 17));
        empty.tasks.clear();
        let report = insert_plan(&mut empty, wing(), "East Wing", 7);
        assert_eq!(report.summary_row, 0);
        assert_eq!(empty.tasks[0].outline_level, 0, "there is nothing to sit under");
    }

    #[test]
    fn a_resource_of_the_same_name_is_the_same_person_and_is_not_duplicated() {
        let mut project = host();
        let ana = project.add_resource("Ana Reyes");
        if let Some(resource) = project.resources.iter_mut().find(|r| r.id == ana) {
            resource.standard_rate = 95.0;
        }

        let mut incoming = wing();
        // Numbered from 1 in its own plan, spelled differently, and priced
        // differently as well.
        let their_ana = incoming.add_resource("ana reyes");
        if let Some(resource) = incoming.resources.iter_mut().find(|r| r.id == their_ana) {
            resource.standard_rate = 40.0;
        }
        let rig = incoming.add_resource("Rig");
        incoming.tasks[0].assignments = vec![
            Assignment {
                resource: their_ana,
                units: 0.5,
            },
            Assignment {
                resource: rig,
                units: 1.0,
            },
        ];

        insert_plan(&mut project, incoming, "East Wing", 3);

        assert_eq!(project.resources.len(), 2, "Ana was not listed twice");
        assert_eq!(
            project.resource(ana).map(|r| r.standard_rate),
            Some(95.0),
            "and the host's rate for her stands"
        );

        let row = project
            .tasks
            .iter()
            .find(|task| task.name == "Wing A")
            .expect("the booked row came across");
        assert_eq!(row.assignments[0].resource, ana, "booked onto the host's Ana");
        assert_eq!(row.assignments[0].units, 0.5, "at the units she was given");
        assert_ne!(row.assignments[1].resource, ana);
        assert_eq!(
            project.resource(row.assignments[1].resource).map(|r| r.name.as_str()),
            Some("Rig"),
            "and the rig the host had never heard of came with her"
        );
    }

    #[test]
    fn the_host_keeps_its_own_name_for_a_slot_but_the_values_still_come_across() {
        let text1 = Slot::new(CustomKind::Text, 1);
        let text2 = Slot::new(CustomKind::Text, 2);

        let mut project = host();
        let mut theirs = CustomField::new(text1);
        theirs.title = "Department".into();
        project.custom_fields.insert(text1, theirs);

        let mut incoming = wing();
        let mut clashing = CustomField::new(text1);
        clashing.title = "Risk".into();
        incoming.custom_fields.insert(text1, clashing);
        let mut spare = CustomField::new(text2);
        spare.title = "Owner".into();
        incoming.custom_fields.insert(text2, spare);
        incoming.set_custom_value(0, text1, "High");
        incoming.set_custom_value(0, text2, "Sam");

        let report = insert_plan(&mut project, incoming, "East Wing", 3);

        assert_eq!(
            project.custom_fields.get(&text1).map(|f| f.title()),
            Some("Department".to_string()),
            "one column cannot carry two names, and the host's is the one in use"
        );
        assert_eq!(
            project.custom_fields.get(&text2).map(|f| f.title()),
            Some("Owner".to_string()),
            "a slot the host never used is free to take the incoming name"
        );

        let row = report.summary_row + 1;
        assert_eq!(project.custom_value(row, text1), "High");
        assert_eq!(project.custom_value(row, text2), "Sam");
    }

    #[test]
    fn external_dependencies_come_across_without_taking_over_the_hosts_own() {
        use crate::model::ExternalDependency;

        let mut project = host();
        project.external.push(ExternalDependency {
            id: 1,
            reference: "PO-12".into(),
            label: "Steel".into(),
            source: "Purchasing".into(),
            available: at_eight(2026, 9, 1),
            notes: String::new(),
        });
        project.tasks[0].external_predecessors = vec![1];

        let mut incoming = wing();
        // Numbered 1 in its own plan, exactly like the host's.
        incoming.external.push(ExternalDependency {
            id: 1,
            reference: "PO-88".into(),
            label: "Glazing".into(),
            source: "Purchasing".into(),
            available: at_eight(2026, 10, 1),
            notes: String::new(),
        });
        incoming.tasks[0].external_predecessors = vec![1];

        let report = insert_plan(&mut project, incoming, "East Wing", 3);

        assert_eq!(project.external.len(), 2, "two different orders, two entries");
        let ids: HashSet<ExternalId> = project.external.iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), 2, "and they do not share a number");

        assert_eq!(
            project.externals_of(0).iter().map(|e| e.reference.as_str()).collect::<Vec<_>>(),
            vec!["PO-12"],
            "Host A still waits on the order it always waited on"
        );
        assert_eq!(
            project
                .externals_of(report.summary_row + 1)
                .iter()
                .map(|e| e.reference.as_str())
                .collect::<Vec<_>>(),
            vec!["PO-88"],
            "and Wing A waits on its own"
        );
    }

    #[test]
    fn a_plan_on_disk_is_inserted_whole_and_says_where_it_came_from() {
        let path = scratch("East Wing.aprj");
        let mut saved = wing();
        saved.links.push(Link::finish_to_start(1, 2));
        crate::persist::save(&path, &saved).expect("a plan can be written to a scratch directory");

        let mut project = host();
        let report = insert(&mut project, &path, 3).expect("and read back in");

        assert_eq!(report.task_count, 3);
        assert_eq!(report.link_count, 1);
        assert_eq!(
            project.tasks[report.summary_row].name, "East Wing",
            "the row is named after the file, without the extension"
        );
        let note = &project.tasks[report.summary_row].notes;
        assert!(note.contains("East Wing.aprj"), "the note says which file");
        assert!(
            note.contains("copy"),
            "and that it is a copy, since nothing will refresh it"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_cannot_be_opened_is_reported_in_words_rather_than_panicking() {
        let mut project = host();
        let missing = scratch("no-such-plan.aprj");
        let _ = std::fs::remove_file(&missing);

        let error = insert(&mut project, &missing, 1).expect_err("there is no such file");
        let said = error.to_string();
        assert!(said.contains("no-such-plan.aprj"), "it names the file: {said}");
        assert!(said.contains("nothing was inserted"), "and says so: {said}");
        assert_eq!(names(&project), vec!["Host A", "Host B", "Host C"]);
    }

    #[test]
    fn a_plan_with_no_tasks_is_refused_rather_than_leaving_a_bare_row() {
        let path = scratch("Nothing Yet.aprj");
        let mut blank = Project::blank(at_eight(2026, 8, 17));
        blank.tasks.clear();
        crate::persist::save(&path, &blank).expect("a plan can be written to a scratch directory");

        let mut project = host();
        let error = insert(&mut project, &path, 1).expect_err("there was nothing in it");
        assert!(error.to_string().contains("no tasks"), "{error}");
        assert_eq!(
            project.tasks.len(),
            3,
            "an empty summary would read like an insertion that worked"
        );

        let _ = std::fs::remove_file(&path);
    }
}
