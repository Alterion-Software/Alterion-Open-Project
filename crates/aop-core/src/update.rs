//! The Update Project command.
//!
//! Microsoft Project puts two quite different operations behind one dialog, and
//! the difference is worth stating plainly because picking the wrong one
//! rewrites a plan's history. "Update work as complete through" asserts that
//! the plan was followed: everything scheduled before the status date is
//! declared done. "Reschedule uncompleted work to start after" asserts the
//! opposite: nothing happened, so work that should have started has to move.
//! They are mutually exclusive for that reason, which is why [`UpdateMode`] is
//! an enum rather than a pair of flags.
//!
//! Neither half moves a date itself. Both write to the fields the planner owns
//! (`percent_complete`, `constraint`, `manual_start`) and then hand the plan
//! back to [`crate::schedule::schedule`]. Links, calendars and summary rollups
//! are the scheduler's business, and a second implementation of them here would
//! drift from the first within a release.
//!
//! ## Working time
//!
//! [`Project`] carries one [`WorkCalendar`], so every proportion below is
//! measured in that calendar's working minutes rather than in calendar days.
//! That is the part that matters: a weekend passing is not progress. There are
//! no per-task or per-resource calendars in the model yet, so a task booked to
//! a night-shift resource is still measured against the plan's calendar.
//!
//! ## Where a single span cannot match Microsoft Project
//!
//! Rescheduling a part-finished task should leave the finished part where it
//! happened and move only the remainder, drawing the task as two bars with a
//! gap between them. A [`crate::model::Task`] here is one span with nowhere to
//! store split parts, so the whole bar moves and lands with its finished part
//! butted up against the status date. That puts the remaining work in exactly
//! the right place and the finished work in the wrong one. [`Rescheduled`]
//! records the start the task actually had before the move, so a caller can
//! report the difference rather than quietly lose it.

use chrono::NaiveDateTime;

use crate::calendar::WorkCalendar;
use crate::model::{ConstraintType, Project, Task, TaskId, TaskMode};
use crate::schedule::{ScheduleError, ScheduleReport};

// ------------------------------------------------------------ what to do

/// How much detail "update work as complete" is allowed to invent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionRule {
    /// "Set 0% - 100% complete". A task straddling the date takes the share of
    /// its working duration that has elapsed.
    Proportional,
    /// "Set 0% or 100% complete only". For planners who will not report a
    /// number they did not measure: a task is finished or it is untouched.
    WholeTasksOnly,
}

/// The two halves of the Update Project dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// Assume the plan was followed up to the date and mark work done.
    Complete(CompletionRule),
    /// Assume nothing happened before the date and move the work that has not
    /// finished so it starts after it.
    RescheduleUncompleted,
}

/// Which rows an update run touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateScope {
    EntireProject,
    /// Row positions, as the grid numbers them. Naming a summary row means
    /// naming the work under it, since a summary has no progress of its own.
    Rows(Vec<usize>),
}

/// Everything the dialog collects, in one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    pub mode: UpdateMode,
    /// The status date. Read as "through" when completing and as "after" when
    /// rescheduling, exactly as the dialog labels its one date field.
    pub through: NaiveDateTime,
    pub scope: UpdateScope,
    /// Rescheduling only. A manually scheduled task sits where the planner put
    /// it, so moving one is an override rather than a routine update and has
    /// to be asked for. Completion ignores this flag: reporting progress does
    /// not move a date, so there is nothing to override.
    pub move_manually_scheduled: bool,
}

impl UpdateOptions {
    /// "Update work as complete through this date", whole project, in
    /// proportion. The dialog's own defaults.
    pub fn complete_through(through: NaiveDateTime) -> Self {
        Self {
            mode: UpdateMode::Complete(CompletionRule::Proportional),
            through,
            scope: UpdateScope::EntireProject,
            move_manually_scheduled: false,
        }
    }

    /// "Reschedule uncompleted work to start after this date", whole project.
    pub fn reschedule_after(after: NaiveDateTime) -> Self {
        Self {
            mode: UpdateMode::RescheduleUncompleted,
            through: after,
            scope: UpdateScope::EntireProject,
            move_manually_scheduled: false,
        }
    }

    /// Switch a completion run to "0% or 100% only". Does nothing to a
    /// rescheduling run, which has no such choice to make.
    pub fn whole_tasks_only(mut self) -> Self {
        if let UpdateMode::Complete(rule) = &mut self.mode {
            *rule = CompletionRule::WholeTasksOnly;
        }
        self
    }

    pub fn for_rows(mut self, rows: impl IntoIterator<Item = usize>) -> Self {
        self.scope = UpdateScope::Rows(rows.into_iter().collect());
        self
    }

    pub fn moving_manually_scheduled(mut self) -> Self {
        self.move_manually_scheduled = true;
        self
    }
}

// ------------------------------------------------------------ what happened

/// Why a row in scope was left alone.
///
/// The dialog gives no feedback at all, which is how planners end up believing
/// an update did nothing when it did something they did not expect. Every
/// candidate that goes untouched says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Switched off, so the scheduler ignores it and so does this.
    Inactive,
    /// Manually scheduled, and the run was not told to move those.
    ManuallyScheduled,
    /// Scheduled to begin after the date, so no work is claimed for it.
    NotStarted,
    /// Not finished by the date, and the run was told to report nothing in
    /// between.
    NotFinished,
    /// Already fully complete, so there is no remaining work to move.
    AlreadyComplete,
    /// Already starting after the date.
    AlreadyAfterDate,
    /// The update worked out the value the task already held.
    NoChange,
}

impl SkipReason {
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::Inactive => "Inactive",
            SkipReason::ManuallyScheduled => "Manually scheduled",
            SkipReason::NotStarted => "Not started by that date",
            SkipReason::NotFinished => "Not finished by that date",
            SkipReason::AlreadyComplete => "Already complete",
            SkipReason::AlreadyAfterDate => "Already starts after that date",
            SkipReason::NoChange => "Already up to date",
        }
    }
}

/// Progress written onto one task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completed {
    pub index: usize,
    pub id: TaskId,
    pub from: u8,
    pub to: u8,
}

/// Work moved out from behind the status date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rescheduled {
    pub index: usize,
    pub id: TaskId,
    /// Where the task began before the move. Kept because the finished part of
    /// a part-done task really did happen there, and the moved bar no longer
    /// says so.
    pub was_start: NaiveDateTime,
    /// Where it begins now, read back after the scheduler settled the plan, so
    /// this is the date the task actually got rather than the one asked for.
    pub new_start: NaiveDateTime,
    /// When the unfinished part picks up. Equal to `new_start` for a task that
    /// had not begun.
    pub resumes: NaiveDateTime,
    /// Working minutes already reported done, which is the part that would be
    /// left behind if a task could be split.
    pub completed_minutes: i64,
}

impl Rescheduled {
    /// Whether Microsoft Project would have drawn this as a split bar.
    pub fn is_split(&self) -> bool {
        self.completed_minutes > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skipped {
    pub index: usize,
    pub id: TaskId,
    pub reason: SkipReason,
}

/// What an update run did, in enough detail for the status bar to say so.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateSummary {
    pub completed: Vec<Completed>,
    pub rescheduled: Vec<Rescheduled>,
    pub skipped: Vec<Skipped>,
    /// The plan as the scheduler left it. An update always ends in a
    /// reschedule, if only so summary rows roll their children's progress up.
    pub schedule: ScheduleReport,
}

impl UpdateSummary {
    /// Rows the run actually wrote to.
    pub fn changed(&self) -> usize {
        self.completed.len() + self.rescheduled.len()
    }

    /// A one-line report, for the status bar.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.completed.is_empty() {
            parts.push(format!("{} updated", self.completed.len()));
        }
        if !self.rescheduled.is_empty() {
            parts.push(format!("{} rescheduled", self.rescheduled.len()));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("{} left alone", self.skipped.len()));
        }
        if parts.is_empty() {
            return "Nothing to update".into();
        }
        format!("{} tasks", parts.join(", "))
    }
}

// ------------------------------------------------------------ the work

/// Rows an update should consider, summary rows already resolved to their work.
///
/// Summary rows never appear in the result. Their progress and their span are
/// both rolled up from their children, so writing to one would be undone by the
/// reschedule at the end of the run. Naming one in a selection therefore means
/// naming the leaves beneath it, which is what a planner picking a phase
/// heading means anyway.
fn rows_in_scope(project: &Project, scope: &UpdateScope) -> Vec<usize> {
    match scope {
        UpdateScope::EntireProject => (0..project.tasks.len())
            .filter(|&index| !project.is_summary(index))
            .collect(),
        UpdateScope::Rows(rows) => {
            let mut out: Vec<usize> = Vec::with_capacity(rows.len());
            for &row in rows {
                if row >= project.tasks.len() {
                    continue;
                }
                if project.is_summary(row) {
                    out.extend(project.leaf_indices(row));
                } else {
                    out.push(row);
                }
            }
            out.sort_unstable();
            out.dedup();
            out
        }
    }
}

/// The percentage a task should report, or why it should be left alone.
fn completion_for(
    calendar: &WorkCalendar,
    task: &Task,
    through: NaiveDateTime,
    rule: CompletionRule,
) -> Result<u8, SkipReason> {
    // Finished by the date under either rule, and a milestone reached by the
    // date lands here too, which is the only sensible reading of a marker.
    if task.scheduled.finish <= through {
        return Ok(100);
    }

    match rule {
        // Nothing between 0 and 100 exists in this mode. An unfinished task is
        // left exactly as it was rather than forced to zero: dropping progress
        // a planner measured and typed in would be the update destroying data
        // rather than adding it.
        CompletionRule::WholeTasksOnly => Err(SkipReason::NotFinished),
        CompletionRule::Proportional => {
            if task.scheduled.start >= through {
                return Err(SkipReason::NotStarted);
            }
            let span = calendar.work_minutes_between(task.scheduled.start, task.scheduled.finish);
            if span <= 0 {
                // A marker has no span to take a share of, and its finish is
                // after the date, so it has not been reached.
                return Err(SkipReason::NotStarted);
            }
            let elapsed = calendar
                .work_minutes_between(task.scheduled.start, through)
                .max(0);
            // Truncated rather than rounded, and capped below 100, so work
            // still in progress can never read as finished. A planner chasing
            // the last task on a plan should not be told it is done.
            Ok((elapsed * 100 / span).clamp(0, 99) as u8)
        }
    }
}

/// Apply the Update Project command and settle the plan.
///
/// The reschedule at the end is not optional. Completion changes what summary
/// rows must roll up, rescheduling changes what successors are driven by, and
/// both leave the plan half-stated until the scheduler has run. The error is
/// the scheduler's own, which means the plan already had a dependency loop in
/// it before this was called.
pub fn update_project(
    project: &mut Project,
    options: &UpdateOptions,
) -> Result<UpdateSummary, ScheduleError> {
    // Completion is a share of working time, so it has to be measured in the
    // time each task is actually worked in rather than the project's.
    let calendars = crate::effective::EffectiveCalendars::build(project);
    let mut completed = Vec::new();
    let mut rescheduled = Vec::new();
    let mut skipped = Vec::new();

    for index in rows_in_scope(project, &options.scope) {
        let id = project.tasks[index].id;
        if !project.tasks[index].active {
            skipped.push(Skipped {
                index,
                id,
                reason: SkipReason::Inactive,
            });
            continue;
        }

        match options.mode {
            UpdateMode::Complete(rule) => {
                let was = project.tasks[index].percent_complete;
                match completion_for(
                    calendars.for_row(index),
                    &project.tasks[index],
                    options.through,
                    rule,
                ) {
                    Err(reason) => skipped.push(Skipped { index, id, reason }),
                    Ok(percent) if percent == was => skipped.push(Skipped {
                        index,
                        id,
                        reason: SkipReason::NoChange,
                    }),
                    Ok(percent) => {
                        project.tasks[index].percent_complete = percent;
                        completed.push(Completed {
                            index,
                            id,
                            from: was,
                            to: percent,
                        });
                    }
                }
            }

            UpdateMode::RescheduleUncompleted => {
                let task = &project.tasks[index];
                let reason = if task.is_complete() {
                    Some(SkipReason::AlreadyComplete)
                } else if task.mode == TaskMode::Manual && !options.move_manually_scheduled {
                    Some(SkipReason::ManuallyScheduled)
                } else if task.scheduled.start >= options.through {
                    Some(SkipReason::AlreadyAfterDate)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    skipped.push(Skipped { index, id, reason });
                    continue;
                }

                let was_start = task.scheduled.start;
                let calendar = calendars.for_row(index);
                // Where the unfinished part has to pick up. Snapping forward
                // matters: a status date of Friday evening means Monday
                // morning, not a slot in the middle of the weekend, and on a
                // task somebody is away for it means the day they are back.
                let resume = calendar.next_working_instant(options.through);
                let span = calendar.work_minutes_between(was_start, task.scheduled.finish);
                let done = span * task.percent_complete.min(100) as i64 / 100;

                // With nowhere to store a split, placing the bar so its
                // finished part ends exactly at the status date is the closest
                // a single span gets: the remaining work then starts where it
                // should, which is what the command was asked for.
                let new_start = if done > 0 {
                    calendar.sub_minutes(resume, done)
                } else {
                    resume
                };

                let mode = task.mode;
                let task = &mut project.tasks[index];
                if mode == TaskMode::Manual {
                    // A manual task takes its dates from what was typed, so
                    // that is what has to change for it to move at all.
                    task.manual_start = Some(new_start);
                }
                // Any date constraint already on the task is replaced. The
                // planner has just said this work did not happen, and a
                // constraint asserting that it did cannot both be honoured.
                task.constraint = ConstraintType::StartNoEarlierThan;
                task.constraint_date = Some(new_start);

                rescheduled.push(Rescheduled {
                    index,
                    id,
                    was_start,
                    // Filled in properly once the scheduler has spoken.
                    new_start,
                    resumes: resume,
                    completed_minutes: done,
                });
            }
        }
    }

    let report = crate::schedule::schedule(project)?;

    // A constraint is a floor, not an instruction: a predecessor can push a
    // task later still. Reporting the date asked for rather than the date given
    // would have the dialog claim something the plan does not say.
    // Recomposed after the reschedule rather than reused, because the borrow
    // above ended when the scheduler took the plan mutably. Nothing the
    // scheduler does changes what a task is worked to, so this is the same
    // answer, only fetched again.
    let calendars = crate::effective::EffectiveCalendars::build(project);
    for moved in &mut rescheduled {
        moved.new_start = project.tasks[moved.index].scheduled.start;
        moved.resumes = if moved.completed_minutes > 0 {
            calendars
                .for_row(moved.index)
                .add_minutes(moved.new_start, moved.completed_minutes)
        } else {
            moved.new_start
        };
    }

    Ok(UpdateSummary {
        completed,
        rescheduled,
        skipped,
        schedule: report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Link, LinkType};
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    /// Finish-to-start chain from Monday 5 January 2026, 08:00.
    fn chain(durations: &[i64]) -> Project {
        let mut project = Project::blank(at(2026, 1, 5, 8));
        project.tasks.clear();
        let mut previous = None;
        for (position, minutes) in durations.iter().enumerate() {
            let id = project.allocate_task_id();
            project
                .tasks
                .push(Task::new(id, format!("Task {}", position + 1), *minutes));
            if let Some(from) = previous {
                project.links.push(Link {
                    predecessor: from,
                    successor: id,
                    kind: LinkType::FS,
                    lag_minutes: 0,
                });
            }
            previous = Some(id);
        }
        crate::schedule::schedule(&mut project).unwrap();
        project
    }

    /// A phase heading with two one day tasks running back to back beneath it.
    ///
    /// Built separately from `chain` because a link naming a summary is
    /// expanded onto that summary's own leaves, which would have the phase
    /// depending on its own children.
    fn phase() -> Project {
        let mut project = Project::blank(at(2026, 1, 5, 8));
        project.tasks.clear();
        for (level, name, minutes) in [(0u16, "Phase", 0i64), (1, "One", 480), (1, "Two", 480)] {
            let id = project.allocate_task_id();
            let mut task = Task::new(id, name, minutes);
            task.outline_level = level;
            project.tasks.push(task);
        }
        project.links.push(Link::finish_to_start(
            project.tasks[1].id,
            project.tasks[2].id,
        ));
        crate::schedule::schedule(&mut project).unwrap();
        project
    }

    fn reason(summary: &UpdateSummary, index: usize) -> Option<SkipReason> {
        summary
            .skipped
            .iter()
            .find(|row| row.index == index)
            .map(|row| row.reason)
    }

    // ---- completing work ------------------------------------------------

    #[test]
    fn a_task_finished_before_the_date_is_fully_complete() {
        // One day each from Monday: task one ends Monday 17:00.
        let mut project = chain(&[480, 480, 480]);
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 6, 17)))
                .unwrap();

        assert_eq!(project.tasks[0].percent_complete, 100);
        assert_eq!(project.tasks[1].percent_complete, 100);
        assert_eq!(summary.completed.len(), 2);
    }

    #[test]
    fn a_task_straddling_the_date_takes_the_share_that_has_elapsed() {
        // A two day task from Monday, measured at the end of Monday.
        let mut project = chain(&[960]);
        update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 5, 17))).unwrap();
        assert_eq!(project.tasks[0].percent_complete, 50);
    }

    #[test]
    fn a_straddling_task_never_quite_reads_as_finished() {
        // An hour short of the end of a two day task. Rounding to the nearest
        // percent would report 100 on work still going on, which is the one
        // answer a planner cannot act on.
        let mut project = chain(&[960]);
        update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 6, 16))).unwrap();
        assert_eq!(project.tasks[0].percent_complete, 93);
    }

    #[test]
    fn the_share_counts_working_time_and_not_the_wall_clock() {
        // Two days of work from Friday morning, measured on Monday morning.
        // A weekend passing is not work getting done.
        let mut project = Project::blank(at(2026, 1, 9, 8));
        project.tasks.clear();
        let id = project.allocate_task_id();
        project.tasks.push(Task::new(id, "Over the weekend", 960));
        crate::schedule::schedule(&mut project).unwrap();

        update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 12, 8))).unwrap();
        assert_eq!(project.tasks[0].percent_complete, 50);
    }

    #[test]
    fn a_task_starting_after_the_date_is_left_at_nothing() {
        let mut project = chain(&[480, 480, 480]);
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 5, 17)))
                .unwrap();

        assert_eq!(project.tasks[2].percent_complete, 0);
        assert_eq!(reason(&summary, 2), Some(SkipReason::NotStarted));
    }

    #[test]
    fn whole_tasks_only_reports_nothing_in_between() {
        let mut project = chain(&[480, 960]);
        let summary = update_project(
            &mut project,
            &UpdateOptions::complete_through(at(2026, 1, 6, 17)).whole_tasks_only(),
        )
        .unwrap();

        assert_eq!(project.tasks[0].percent_complete, 100, "it finished Monday");
        assert_eq!(
            project.tasks[1].percent_complete, 0,
            "it runs to Wednesday, so nothing is claimed for it"
        );
        assert_eq!(reason(&summary, 1), Some(SkipReason::NotFinished));
    }

    #[test]
    fn whole_tasks_only_does_not_wipe_progress_a_planner_typed_in() {
        // Forcing an unfinished task to zero would have the update destroying
        // a measurement rather than adding one.
        let mut project = chain(&[960]);
        project.tasks[0].percent_complete = 30;
        update_project(
            &mut project,
            &UpdateOptions::complete_through(at(2026, 1, 5, 17)).whole_tasks_only(),
        )
        .unwrap();
        assert_eq!(project.tasks[0].percent_complete, 30);
    }

    #[test]
    fn a_milestone_is_reached_or_it_is_not() {
        // The marker follows a task ending Monday evening, so it sits at
        // Tuesday morning: a finish-to-start successor cannot begin inside a
        // day that has already ended.
        let mut project = chain(&[480, 0]);
        assert!(project.is_marker(1));
        assert_eq!(project.tasks[1].scheduled.finish, at(2026, 1, 6, 8));

        update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 6, 8))).unwrap();
        assert_eq!(
            project.tasks[1].percent_complete, 100,
            "the date reaches the marker, and a marker has no half way"
        );

        let mut project = chain(&[480, 0]);
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 5, 12)))
                .unwrap();
        assert_eq!(project.tasks[1].percent_complete, 0);
        assert_eq!(reason(&summary, 1), Some(SkipReason::NotStarted));
    }

    #[test]
    fn an_update_can_be_limited_to_the_rows_picked() {
        let mut project = chain(&[480, 480, 480]);
        update_project(
            &mut project,
            &UpdateOptions::complete_through(at(2026, 1, 9, 17)).for_rows([1]),
        )
        .unwrap();

        assert_eq!(project.tasks[0].percent_complete, 0, "not picked");
        assert_eq!(project.tasks[1].percent_complete, 100);
        assert_eq!(project.tasks[2].percent_complete, 0, "not picked");
    }

    #[test]
    fn picking_a_phase_heading_picks_the_work_under_it() {
        let mut project = phase();
        let summary = update_project(
            &mut project,
            &UpdateOptions::complete_through(at(2026, 1, 9, 17)).for_rows([0]),
        )
        .unwrap();

        assert!(project.is_summary(0));
        assert_eq!(summary.completed.len(), 2, "both children, not the heading");
        assert!(summary.completed.iter().all(|row| row.index != 0));
    }

    #[test]
    fn an_inactive_task_is_left_alone_and_says_so() {
        let mut project = chain(&[480, 480]);
        project.tasks[1].active = false;
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 9, 17)))
                .unwrap();
        assert_eq!(reason(&summary, 1), Some(SkipReason::Inactive));
        assert_eq!(project.tasks[1].percent_complete, 0);
    }

    // ---- summary rows ---------------------------------------------------

    #[test]
    fn a_summary_row_keeps_the_rollup_of_its_children() {
        // Its progress is its children's. Writing to it directly would be
        // undone by the reschedule at the end of the run anyway, so the run
        // must not claim to have set it either.
        let mut project = phase();
        // A stale figure sitting on the heading, to prove it is recomputed.
        project.tasks[0].percent_complete = 90;

        // The end of Monday: the first child is done, the second has not begun.
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 5, 17)))
                .unwrap();

        assert!(project.is_summary(0));
        assert!(
            summary.completed.iter().all(|row| row.index != 0),
            "the heading is never written to directly"
        );
        assert!(summary.skipped.iter().all(|row| row.index != 0));
        assert_eq!(project.tasks[1].percent_complete, 100);
        assert_eq!(project.tasks[2].percent_complete, 0);
        assert_eq!(
            project.tasks[0].percent_complete, 50,
            "one of two equal children done, rolled up rather than the stale 90"
        );
    }

    // ---- rescheduling ---------------------------------------------------

    #[test]
    fn work_that_never_started_moves_wholesale() {
        let mut project = chain(&[480, 480, 480]);
        let after = at(2026, 2, 2, 8);
        let summary = update_project(&mut project, &UpdateOptions::reschedule_after(after)).unwrap();

        assert!(project.tasks.iter().all(|task| task.scheduled.start >= after));
        assert_eq!(summary.rescheduled.len(), 3);
        assert!(
            summary.rescheduled.iter().all(|moved| !moved.is_split()),
            "nothing had started, so nothing is split"
        );
    }

    #[test]
    fn a_part_finished_task_leaves_only_the_finished_part_behind_the_date() {
        let mut project = chain(&[480, 480, 480]);
        project.tasks[0].percent_complete = 50;
        let after = at(2026, 2, 2, 8);
        let summary = update_project(&mut project, &UpdateOptions::reschedule_after(after)).unwrap();

        let moved = summary
            .rescheduled
            .iter()
            .find(|row| row.index == 0)
            .expect("the part finished task moved");
        assert!(moved.is_split());
        assert_eq!(moved.completed_minutes, 240);
        assert_eq!(moved.was_start, at(2026, 1, 5, 8), "where it really began");

        // Exactly the half day already reported sits before the status date,
        // and the rest of the work starts after it.
        assert_eq!(
            project
                .calendar
                .work_minutes_between(project.tasks[0].scheduled.start, after),
            240
        );
        assert_eq!(project.tasks[0].percent_complete, 50, "progress is untouched");
        assert!(project.tasks[1].scheduled.start >= after, "and successors follow");
    }

    #[test]
    fn a_finished_task_is_not_moved() {
        let mut project = chain(&[480, 480]);
        project.tasks[0].percent_complete = 100;
        let was = project.tasks[0].scheduled.start;

        let summary =
            update_project(&mut project, &UpdateOptions::reschedule_after(at(2026, 2, 2, 8)))
                .unwrap();
        assert_eq!(project.tasks[0].scheduled.start, was);
        assert_eq!(reason(&summary, 0), Some(SkipReason::AlreadyComplete));
    }

    #[test]
    fn a_task_already_starting_after_the_date_is_not_touched() {
        let mut project = chain(&[480, 480]);
        let summary =
            update_project(&mut project, &UpdateOptions::reschedule_after(at(2026, 1, 5, 8)))
                .unwrap();
        assert_eq!(reason(&summary, 0), Some(SkipReason::AlreadyAfterDate));
        assert_eq!(reason(&summary, 1), Some(SkipReason::AlreadyAfterDate));
        assert!(summary.rescheduled.is_empty());
    }

    #[test]
    fn rescheduling_snaps_a_date_in_non_working_time_forward() {
        // Friday evening means Monday morning, not a slot in the weekend.
        let mut project = chain(&[480]);
        update_project(&mut project, &UpdateOptions::reschedule_after(at(2026, 1, 9, 18))).unwrap();
        assert_eq!(project.tasks[0].scheduled.start, at(2026, 1, 12, 8));
    }

    // ---- manually scheduled work ----------------------------------------

    #[test]
    fn a_manually_scheduled_task_is_not_moved_unless_asked() {
        let mut project = Project::blank(at(2026, 1, 5, 8));
        project.tasks.clear();
        let id = project.allocate_task_id();
        let mut task = Task::new(id, "Pinned by hand", 480);
        task.mode = TaskMode::Manual;
        task.manual_start = Some(at(2026, 1, 6, 8));
        project.tasks.push(task);
        crate::schedule::schedule(&mut project).unwrap();

        let summary =
            update_project(&mut project, &UpdateOptions::reschedule_after(at(2026, 2, 2, 8)))
                .unwrap();
        assert_eq!(reason(&summary, 0), Some(SkipReason::ManuallyScheduled));
        assert_eq!(project.tasks[0].scheduled.start, at(2026, 1, 6, 8));
        assert_eq!(project.tasks[0].constraint_date, None, "nothing was written");

        let summary = update_project(
            &mut project,
            &UpdateOptions::reschedule_after(at(2026, 2, 2, 8)).moving_manually_scheduled(),
        )
        .unwrap();
        assert_eq!(summary.rescheduled.len(), 1);
        assert_eq!(project.tasks[0].manual_start, Some(at(2026, 2, 2, 8)));
        assert_eq!(project.tasks[0].scheduled.start, at(2026, 2, 2, 8));
    }

    #[test]
    fn a_manually_scheduled_task_still_reports_progress() {
        // Saying how far along something is does not move a date, so there is
        // nothing here for a manual task to be protected from.
        let mut project = Project::blank(at(2026, 1, 5, 8));
        project.tasks.clear();
        let id = project.allocate_task_id();
        let mut task = Task::new(id, "Pinned by hand", 480);
        task.mode = TaskMode::Manual;
        task.manual_start = Some(at(2026, 1, 5, 8));
        project.tasks.push(task);
        crate::schedule::schedule(&mut project).unwrap();

        update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 5, 17))).unwrap();
        assert_eq!(project.tasks[0].percent_complete, 100);
        assert_eq!(project.tasks[0].manual_start, Some(at(2026, 1, 5, 8)));
    }

    // ---- reporting ------------------------------------------------------

    #[test]
    fn a_run_that_changes_nothing_says_so_rather_than_looking_broken() {
        let mut project = chain(&[480]);
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 1, 8)))
                .unwrap();
        assert_eq!(summary.changed(), 0);
        assert_eq!(summary.skipped.len(), 1);
        assert!(summary.describe().contains("left alone"));
    }

    #[test]
    fn an_empty_plan_updates_to_nothing() {
        let mut project = Project::blank(at(2026, 1, 5, 8));
        project.tasks.clear();
        let summary =
            update_project(&mut project, &UpdateOptions::complete_through(at(2026, 1, 5, 17)))
                .unwrap();
        assert_eq!(summary.changed(), 0);
        assert_eq!(summary.describe(), "Nothing to update");
    }
}
