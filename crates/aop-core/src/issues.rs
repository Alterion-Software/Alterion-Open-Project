//! What is worth flagging about a task, and what can be done about it.
//!
//! The table marks rows with a warning, and a warning that only says "this is
//! critical" is not worth the ink: being on the critical path is a fact about
//! the plan, not a fault in it. So each flag here carries what it means, and
//! where something can actually be done, the change that would do it.
//!
//! Some flags are informational and have no fix. That is deliberate. Offering a
//! "fix" for being critical would mean pretending there is a button that makes
//! a plan shorter.

use crate::model::{ConstraintType, IssueKind, Project, TaskMode};

/// A change that would clear an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFix {
    /// Put the task back to As Soon As Possible.
    ClearConstraint,
    /// Let the scheduler place it.
    MakeAutomatic,
    /// Drop the deadline it cannot meet.
    ClearDeadline,
    /// Switch the task back on.
    Activate,
}

impl TaskFix {
    /// What the menu entry says. Phrased as the change, not as "fix", so it is
    /// clear what will happen before it happens.
    pub fn label(self) -> &'static str {
        match self {
            TaskFix::ClearConstraint => "Remove the constraint",
            TaskFix::MakeAutomatic => "Switch to auto scheduled",
            TaskFix::ClearDeadline => "Remove the deadline",
            TaskFix::Activate => "Make the task active",
        }
    }
}

/// Something worth telling the planner about one task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskIssue {
    pub kind: IssueKind,
    /// One sentence, in terms of the plan rather than the software.
    pub message: String,
    /// The change that would clear it, where there is one.
    pub fix: Option<TaskFix>,
    /// Whether the planner has said they know and do not want it acted on.
    ///
    /// An ignored issue is still reported, quietly. Removing it from the list
    /// would leave no way to see what had been dismissed or to change your
    /// mind, and a row that silently stopped mentioning a constraint would be
    /// worse than one that mentions it in grey.
    pub ignored: bool,
}

/// Everything worth flagging about a task, ignored ones marked rather than
/// dropped.
///
/// Summary rows are skipped: their dates are rolled up from their children, so
/// anything worth saying belongs on the child that caused it.
pub fn task_issues(project: &Project, index: usize) -> Vec<TaskIssue> {
    let Some(task) = project.tasks.get(index) else {
        return Vec::new();
    };
    if project.is_summary(index) {
        return Vec::new();
    }

    let mut issues = Vec::new();

    if task.scheduled.critical
        && let Some(message) = crate::schedule::critical_reason(project, index)
    {
        issues.push(TaskIssue {
            kind: IssueKind::Critical,
            message,
            // Nothing here can shorten the plan. Saying so is the honest move;
            // offering a button would not be.
            fix: None,
            ignored: false,
        });
    }

    if task.constraint != ConstraintType::AsSoonAsPossible {
        let when = task
            .constraint_date
            .map(|date| format!(" {}", date.format("%d/%m/%Y")))
            .unwrap_or_default();
        issues.push(TaskIssue {
            kind: IssueKind::Constraint,
            message: format!(
                "{}{} holds this task in place, so its links cannot move it.",
                task.constraint.label(),
                when
            ),
            fix: Some(TaskFix::ClearConstraint),
            ignored: false,
        });
    }

    if let Some(deadline) = task.deadline
        && task.scheduled.finish > deadline
    {
        issues.push(TaskIssue {
            kind: IssueKind::MissedDeadline,
            message: format!(
                "Finishes {}, after its deadline of {}.",
                task.scheduled.finish.format("%d/%m/%Y"),
                deadline.format("%d/%m/%Y")
            ),
            fix: Some(TaskFix::ClearDeadline),
            ignored: false,
        });
    }

    if task.mode == TaskMode::Manual {
        issues.push(TaskIssue {
            kind: IssueKind::ManuallyScheduled,
            message: "Manually scheduled: its predecessors do not move it.".into(),
            fix: Some(TaskFix::MakeAutomatic),
            ignored: false,
        });
    }

    if !task.active {
        issues.push(TaskIssue {
            kind: IssueKind::Inactive,
            message: "Inactive: the scheduler leaves it out of the plan.".into(),
            fix: Some(TaskFix::Activate),
            ignored: false,
        });
    }

    for issue in &mut issues {
        issue.ignored = task.ignored_issues.contains(&issue.kind);
    }
    issues
}

/// Whether a task has been told to stop reporting one sort of issue.
pub fn is_ignored(project: &Project, index: usize, kind: IssueKind) -> bool {
    project
        .tasks
        .get(index)
        .is_some_and(|task| task.ignored_issues.contains(&kind))
}

/// Whether a task should be presented as critical.
///
/// The scheduler still knows it is: slack is a fact about the plan and nothing
/// here changes the arithmetic. This is only about what is shown, so a task
/// whose criticality has been acknowledged stops being coloured for it, on
/// screen and on paper alike.
pub fn shows_as_critical(project: &Project, index: usize) -> bool {
    project
        .tasks
        .get(index)
        .is_some_and(|task| task.scheduled.critical)
        && !is_ignored(project, index, IssueKind::Critical)
}

/// Make the change an issue asked for.
///
/// Returns whether anything actually changed, so a caller can decide whether a
/// reschedule and an undo step are warranted.
pub fn apply_fix(project: &mut Project, index: usize, fix: TaskFix) -> bool {
    let Some(task) = project.tasks.get_mut(index) else {
        return false;
    };
    match fix {
        TaskFix::ClearConstraint => {
            if task.constraint == ConstraintType::AsSoonAsPossible {
                return false;
            }
            task.constraint = ConstraintType::AsSoonAsPossible;
            task.constraint_date = None;
        }
        TaskFix::MakeAutomatic => {
            if task.mode == TaskMode::Auto {
                return false;
            }
            task.mode = TaskMode::Auto;
            task.manual_start = None;
        }
        TaskFix::ClearDeadline => {
            if task.deadline.is_none() {
                return false;
            }
            task.deadline = None;
        }
        TaskFix::Activate => {
            if task.active {
                return false;
            }
            task.active = true;
        }
    }
    true
}

/// Stop flagging one sort of issue on one task.
pub fn ignore(project: &mut Project, index: usize, kind: IssueKind) {
    if let Some(task) = project.tasks.get_mut(index)
        && !task.ignored_issues.contains(&kind)
    {
        task.ignored_issues.push(kind);
    }
}

/// Flag everything on this task again.
pub fn stop_ignoring(project: &mut Project, index: usize) {
    if let Some(task) = project.tasks.get_mut(index) {
        task.ignored_issues.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    fn plan() -> Project {
        let mut project = Project::blank(at(2026, 8, 17));
        project.tasks.clear();
        for name in ["One", "Two"] {
            let id = project.allocate_task_id();
            project.tasks.push(Task::new(id, name, 480));
        }
        project
    }

    #[test]
    fn a_plain_task_is_not_flagged() {
        // A warning on every row is a warning on none.
        let project = plan();
        assert!(task_issues(&project, 0).is_empty());
    }

    #[test]
    fn being_critical_is_explained_but_offers_no_fix() {
        // There is no button that makes a plan shorter, and pretending
        // otherwise would be worse than saying nothing.
        let mut project = plan();
        project.tasks[0].scheduled.critical = true;
        let issues = task_issues(&project, 0);
        let critical = issues.iter().find(|i| i.kind == IssueKind::Critical);
        assert!(critical.is_some(), "it should still be explained");
        assert!(critical.unwrap().fix.is_none(), "but not offered a fix");
    }

    #[test]
    fn a_constraint_is_flagged_and_can_be_removed() {
        let mut project = plan();
        project.tasks[0].constraint = ConstraintType::MustStartOn;
        project.tasks[0].constraint_date = Some(at(2026, 9, 1));

        let issues = task_issues(&project, 0);
        let found = issues
            .iter()
            .find(|i| i.kind == IssueKind::Constraint)
            .expect("flagged");
        assert_eq!(found.fix, Some(TaskFix::ClearConstraint));
        assert!(found.message.contains("01/09/2026"), "says which date");

        assert!(apply_fix(&mut project, 0, TaskFix::ClearConstraint));
        assert_eq!(project.tasks[0].constraint, ConstraintType::AsSoonAsPossible);
        assert!(project.tasks[0].constraint_date.is_none());
        assert!(task_issues(&project, 0).is_empty(), "and stops being flagged");
    }

    #[test]
    fn a_deadline_is_only_flagged_once_it_is_actually_missed() {
        let mut project = plan();
        project.tasks[0].deadline = Some(at(2026, 12, 1));
        project.tasks[0].scheduled.finish = at(2026, 9, 1);
        assert!(
            !task_issues(&project, 0)
                .iter()
                .any(|i| i.kind == IssueKind::MissedDeadline),
            "a deadline that is being met is not a problem"
        );

        project.tasks[0].scheduled.finish = at(2026, 12, 9);
        assert!(
            task_issues(&project, 0)
                .iter()
                .any(|i| i.kind == IssueKind::MissedDeadline)
        );
    }

    #[test]
    fn applying_a_fix_that_changes_nothing_reports_so() {
        // The caller uses this to decide whether an undo step is warranted.
        let mut project = plan();
        assert!(!apply_fix(&mut project, 0, TaskFix::ClearConstraint));
        assert!(!apply_fix(&mut project, 0, TaskFix::Activate));
    }

    #[test]
    fn an_ignored_flag_is_marked_rather_than_dropped() {
        // Dropping it would leave no way to see what had been dismissed, or to
        // change your mind about it.
        let mut project = plan();
        project.tasks[0].mode = TaskMode::Manual;
        project.tasks[0].active = false;
        assert!(task_issues(&project, 0).iter().all(|i| !i.ignored));

        ignore(&mut project, 0, IssueKind::ManuallyScheduled);
        let issues = task_issues(&project, 0);
        assert_eq!(issues.len(), 2, "both are still reported");
        let manual = issues
            .iter()
            .find(|i| i.kind == IssueKind::ManuallyScheduled)
            .unwrap();
        let inactive = issues.iter().find(|i| i.kind == IssueKind::Inactive).unwrap();
        assert!(manual.ignored, "the dismissed one is marked");
        assert!(!inactive.ignored, "and only that one");

        stop_ignoring(&mut project, 0);
        assert!(task_issues(&project, 0).iter().all(|i| !i.ignored));
    }

    #[test]
    fn ignoring_criticality_stops_it_being_shown_as_critical() {
        // The scheduler still knows: slack is a fact about the plan. This is
        // only about what gets coloured, on screen and on paper alike.
        let mut project = plan();
        project.tasks[0].scheduled.critical = true;
        assert!(shows_as_critical(&project, 0));

        ignore(&mut project, 0, IssueKind::Critical);
        assert!(!shows_as_critical(&project, 0), "it stops being coloured");
        assert!(
            project.tasks[0].scheduled.critical,
            "but the schedule is unchanged"
        );

        stop_ignoring(&mut project, 0);
        assert!(shows_as_critical(&project, 0));
    }

    #[test]
    fn ignoring_one_thing_does_not_quieten_another() {
        let mut project = plan();
        project.tasks[0].scheduled.critical = true;
        project.tasks[0].active = false;
        ignore(&mut project, 0, IssueKind::Critical);

        assert!(!shows_as_critical(&project, 0));
        assert!(
            task_issues(&project, 0)
                .iter()
                .any(|i| i.kind == IssueKind::Inactive && !i.ignored),
            "being switched off is still worth saying"
        );
    }

    #[test]
    fn a_summary_row_is_never_flagged_itself() {
        // Its dates are rolled up, so anything worth saying belongs on the
        // child that caused it.
        let mut project = plan();
        project.tasks[1].outline_level = 1;
        project.tasks[0].scheduled.critical = true;
        assert!(project.is_summary(0));
        assert!(task_issues(&project, 0).is_empty());
    }
}
