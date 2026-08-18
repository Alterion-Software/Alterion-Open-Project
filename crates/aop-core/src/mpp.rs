//! Mapping a Microsoft Project `.mpp` onto a plan.
//!
//! The file itself is read by `alterion-mpp-parser`, a separate crate that
//! knows the on-disk format and nothing about scheduling. This module is only
//! the translation: outline levels, link kinds, constraints and bookings into
//! the shapes the scheduler works with.

use std::collections::HashMap;
use std::path::Path;

use alterion_mpp_parser as mpp;

use crate::calendar::WorkCalendar;
use crate::model::{
    Assignment, ConstraintType, Link, LinkType, Project, Resource, Task, TaskId,
};
use crate::mspdi::ImportError;

/// Read an `.mpp` and map it onto a plan.
pub fn open(path: &Path) -> Result<Project, ImportError> {
    // Touch the file first, so a missing or unreadable path reports as the
    // plain filesystem problem it is rather than as a parsing failure.
    std::fs::metadata(path)?;

    let plan = mpp::read(path).map_err(|e| ImportError::Mpp(e.to_string()))?;
    Ok(assemble(plan, path))
}

fn link_from(kind: mpp::LinkKind) -> LinkType {
    match kind {
        mpp::LinkKind::FinishToStart => LinkType::FS,
        mpp::LinkKind::StartToStart => LinkType::SS,
        mpp::LinkKind::FinishToFinish => LinkType::FF,
        mpp::LinkKind::StartToFinish => LinkType::SF,
    }
}

fn constraint_from(kind: mpp::Constraint) -> ConstraintType {
    match kind {
        mpp::Constraint::AsSoonAsPossible => ConstraintType::AsSoonAsPossible,
        mpp::Constraint::AsLateAsPossible => ConstraintType::AsLateAsPossible,
        mpp::Constraint::MustStartOn => ConstraintType::MustStartOn,
        mpp::Constraint::MustFinishOn => ConstraintType::MustFinishOn,
        mpp::Constraint::StartNoEarlierThan => ConstraintType::StartNoEarlierThan,
        mpp::Constraint::StartNoLaterThan => ConstraintType::StartNoLaterThan,
        mpp::Constraint::FinishNoEarlierThan => ConstraintType::FinishNoEarlierThan,
        mpp::Constraint::FinishNoLaterThan => ConstraintType::FinishNoLaterThan,
    }
}

fn assemble(plan: mpp::Plan, path: &Path) -> Project {
    let start = plan.properties.start.unwrap_or_else(|| {
        chrono::NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    });

    let mut project = Project::blank(start);
    project.name = plan
        .properties
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Imported Project".into())
        });
    project.author = plan.properties.author.clone().unwrap_or_default();
    project.company = plan.properties.company.clone().unwrap_or_default();
    project.calendar = WorkCalendar::standard();

    let rows = plan.ordered_tasks();
    let mut uid_to_id: HashMap<u32, TaskId> = HashMap::new();

    for row in &rows {
        let id = project.allocate_task_id();
        uid_to_id.insert(row.uid, id);

        let duration = if row.is_milestone {
            0
        } else {
            row.duration_minutes.unwrap_or(0).max(0)
        };

        let mut task = Task::new(id, row.name.clone().unwrap_or_default(), duration);
        // The file counts the outline from one; this model counts from zero.
        task.outline_level = row
            .outline_level
            .unwrap_or(1)
            .saturating_sub(1)
            .min(u16::MAX as u32) as u16;
        task.percent_complete = row.percent_complete.unwrap_or(0).min(100);
        task.notes = row.notes.clone().unwrap_or_default();
        task.constraint = row
            .constraint
            .map(constraint_from)
            .unwrap_or(ConstraintType::AsSoonAsPossible);
        task.constraint_date = if task.constraint.needs_date() {
            row.constraint_date
        } else {
            None
        };
        task.deadline = row.deadline;
        project.tasks.push(task);
    }

    // Links second, so every identifier already has a task to point at.
    for row in &rows {
        let Some(&successor) = uid_to_id.get(&row.uid) else {
            continue;
        };
        for relation in &row.predecessors {
            let Some(&predecessor) = uid_to_id.get(&relation.uid) else {
                continue;
            };
            project.add_link(Link {
                predecessor,
                successor,
                kind: link_from(relation.kind),
                lag_minutes: relation.lag_minutes,
            });
        }
    }

    let mut resource_ids: HashMap<u32, u32> = HashMap::new();
    for row in &plan.resources {
        let Some(name) = row.name.as_ref().filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        let id = project.allocate_resource_id();
        resource_ids.insert(row.uid, id);
        let mut resource = Resource::new(id, name.clone());
        if let Some(initials) = row.initials.as_ref().filter(|i| !i.trim().is_empty()) {
            resource.initials = initials.clone();
        }
        resource.group = row.group.clone().unwrap_or_default();
        resource.standard_rate = row.standard_rate.unwrap_or(0.0);
        project.resources.push(resource);
    }

    for booking in &plan.bookings {
        let (Some(&task_id), Some(&resource_id)) = (
            uid_to_id.get(&booking.task_uid),
            resource_ids.get(&booking.resource_uid),
        ) else {
            continue;
        };
        let units = booking.units.filter(|u| *u > 0.0).unwrap_or(1.0);
        if let Some(task) = project.task_mut(task_id)
            && task.assignments.iter().all(|a| a.resource != resource_id) {
                task.assignments.push(Assignment {
                    resource: resource_id,
                    units,
                });
            }
    }

    project.start_date = project.calendar.next_working_instant(project.start_date);
    project
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_a_project_is_refused() {
        let dir = std::env::temp_dir().join("aop-mpp-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("not-a-plan.mpp");
        std::fs::write(&path, b"this is plainly not a compound document").unwrap();

        let error = open(&path).unwrap_err();
        assert!(
            error.to_string().contains("Microsoft Project"),
            "the message should say what is wrong: {error}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_reports_as_a_file_problem() {
        let error = open(Path::new("/nonexistent/plan.mpp")).unwrap_err();
        assert!(matches!(error, ImportError::Io(_)));
    }

    #[test]
    fn every_link_kind_and_constraint_has_a_home() {
        assert_eq!(link_from(mpp::LinkKind::FinishToStart), LinkType::FS);
        assert_eq!(link_from(mpp::LinkKind::StartToStart), LinkType::SS);
        assert_eq!(link_from(mpp::LinkKind::FinishToFinish), LinkType::FF);
        assert_eq!(link_from(mpp::LinkKind::StartToFinish), LinkType::SF);

        assert_eq!(
            constraint_from(mpp::Constraint::StartNoEarlierThan),
            ConstraintType::StartNoEarlierThan
        );
        assert_eq!(
            constraint_from(mpp::Constraint::MustFinishOn),
            ConstraintType::MustFinishOn
        );
    }

    #[test]
    fn an_empty_plan_still_produces_a_usable_project() {
        let plan = mpp::Plan::default();
        let project = assemble(plan, Path::new("/tmp/Refit.mpp"));
        // With no title in the file, the file's own name is used.
        assert_eq!(project.name, "Refit");
        assert!(project.tasks.is_empty());
    }

    #[test]
    fn the_outline_is_rebased_from_one_to_zero() {
        let plan = mpp::Plan {
            tasks: vec![
                mpp::Task {
                    uid: 1,
                    id: Some(1),
                    name: Some("Phase".into()),
                    outline_level: Some(1),
                    ..Default::default()
                },
                mpp::Task {
                    uid: 2,
                    id: Some(2),
                    name: Some("Child".into()),
                    outline_level: Some(2),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let project = assemble(plan, Path::new("/tmp/x.mpp"));
        assert_eq!(project.tasks[0].outline_level, 0);
        assert_eq!(project.tasks[1].outline_level, 1);
    }
}
