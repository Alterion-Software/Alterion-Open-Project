//! Resource levelling.
//!
//! A plan can be sound as a network of dependencies and still be impossible:
//! two tasks on the same Tuesday, each wanting all of one person. Levelling is
//! the answer to that, and it is deliberately the dullest answer available. It
//! never shortens work, never reassigns it and never splits it. It only pushes
//! tasks later until nobody is booked past their capacity, and lets the
//! critical path engine recalculate around the delays.
//!
//! Overallocation is measured a whole day at a time, which is what the
//! scheduler's own report does, so a delay is always to a later working day
//! rather than to later the same afternoon.
//!
//! The honest limitation: Microsoft Project keeps a Levelling Delay field on
//! every task, so its Clear Levelling knows exactly what it wrote. `Task` here
//! has no such field and cannot grow one, so a delay is written as a Start No
//! Earlier Than constraint, which is indistinguishable from one a planner typed
//! in. `clear_leveling` therefore narrows the field to the rows levelling could
//! possibly have written to (leaf rows sharing a work resource with another
//! task) and clears the Start No Earlier Than constraints on those, which will
//! also clear a planner's own constraint on such a row. A caller that needs
//! precision should keep `LevelingResult::delayed` and undo that instead.

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};

use crate::calendar::WorkCalendar;
use crate::effective::EffectiveCalendars;
use crate::model::{ConstraintType, Project, ResourceId, ResourceKind, TaskId, TaskMode};

/// How many delays one run will apply before giving up.
///
/// Every accepted move puts a task on a strictly later day, so the loop cannot
/// cycle, and a plan of any sane size settles in a handful of moves. The cap is
/// for the pathological plan where each move opens a fresh conflict further
/// out, which would otherwise walk the dates forwards indefinitely.
const MAX_MOVES: usize = 500;

/// Ceiling on the day by day walk across one task's span, mirroring the guard
/// the scheduler uses, so a plan with runaway dates cannot hang the caller.
const MAX_TASK_DAYS: u32 = 4000;

/// Units are a fraction of a person, so capacity is compared with a tolerance
/// rather than exactly, matching the scheduler's own overallocation test.
const UNIT_TOLERANCE: f64 = 1e-9;

/// Which part of the plan a run of levelling is allowed to touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelScope {
    EntireProject,
    /// Only these rows may be delayed. Everything else still counts towards the
    /// load, because a resource being double booked is a fact about the plan
    /// and not about what happens to be selected.
    Selected(Vec<usize>),
    /// Only this resource's conflicts are worked on; anyone else stays as they
    /// are, overallocated or not.
    Resource(ResourceId),
}

/// Which task yields when two of them want the same person on the same day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelOrder {
    /// Least slack first, then row order. The tasks holding up the finish keep
    /// their dates and the ones with room to spare are the ones that move.
    Standard,
    /// Ascending task id and nothing else, so the outcome is predictable even
    /// where it is not the cheapest.
    IdOnly,
    /// Tasks the planner has already said something about, by giving them a
    /// deadline or a constraint, keep their dates first; ties then fall back to
    /// the Standard criteria.
    ///
    /// Project has a Priority field from 0 to 1000 for this. There is no such
    /// field in this model, so what the planner has written about a task's
    /// timing stands in for it.
    PriorityFirst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelingOptions {
    pub scope: LevelScope,
    /// Never push the project finish date, whatever it costs in unresolved
    /// conflicts.
    pub only_within_slack: bool,
    /// Whether manually scheduled tasks may be delayed. Off by default, since
    /// the point of scheduling a task by hand is that it stays where it is put.
    pub level_manual: bool,
    pub order: LevelOrder,
}

impl Default for LevelingOptions {
    fn default() -> Self {
        Self {
            scope: LevelScope::EntireProject,
            only_within_slack: false,
            level_manual: false,
            order: LevelOrder::Standard,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LevelingResult {
    /// Row index and the working minutes it was pushed back by, so a caller can
    /// undo exactly this run rather than guessing at it later.
    pub delayed: Vec<(usize, i64)>,
    /// Resources that were overallocated before the run and are not after it.
    pub resolved: usize,
    /// Resources still overallocated when the run finished.
    pub remaining: usize,
}

/// Level the plan, reschedule it, and report what moved.
pub fn level(project: &mut Project, options: &LevelingOptions) -> LevelingResult {
    let Ok(before) = crate::schedule(project) else {
        // Nothing can be levelled against dates that do not exist, and the
        // circular link that stopped the scheduler is the better thing to put
        // in front of the planner anyway.
        return LevelingResult::default();
    };
    let baseline_finish = before.finish;
    let was_over: HashSet<ResourceId> = before.overallocations.iter().map(|o| o.resource).collect();

    let candidates = candidate_rows(project, options);
    let run = Run {
        options,
        candidates: &candidates,
        baseline_finish,
        calendars: EffectiveCalendars::build(project),
    };
    let mut delayed: Vec<(usize, i64)> = Vec::new();
    // Conflicts nothing can be done about, so the search moves past them
    // instead of offering them again.
    let mut stuck: HashSet<(ResourceId, NaiveDate)> = HashSet::new();
    let mut moves = 0usize;

    while moves < MAX_MOVES {
        let Some((resource, date)) = next_conflict(project, options, &stuck, &run.calendars) else {
            break;
        };
        if resolve_one(project, resource, date, &run, &mut delayed) {
            moves += 1;
            // A move changes every date after it, so a conflict written off
            // earlier may now involve different tasks and deserves another
            // look. The move counter still bounds the whole run.
            stuck.clear();
        } else {
            stuck.insert((resource, date));
        }
    }

    delayed.sort_by_key(|(index, _)| *index);
    let still_over: HashSet<ResourceId> = crate::schedule(project)
        .map(|report| report.overallocations.iter().map(|o| o.resource).collect())
        .unwrap_or_default();

    LevelingResult {
        delayed,
        resolved: was_over.difference(&still_over).count(),
        remaining: still_over.len(),
    }
}

/// Take the levelling delays back out and reschedule, returning how many rows
/// were changed.
///
/// See the module note: a levelling delay is a Start No Earlier Than constraint
/// and cannot be told from one the planner wrote, so this clears that
/// constraint on every row levelling could have reached and no others.
pub fn clear_leveling(project: &mut Project) -> usize {
    let reachable: Vec<usize> = (0..project.tasks.len())
        .filter(|&index| shares_a_work_resource(project, index))
        .collect();

    let mut cleared = 0;
    for index in reachable {
        let Some(task) = project.tasks.get_mut(index) else {
            continue;
        };
        if task.constraint != ConstraintType::StartNoEarlierThan {
            continue;
        }
        task.constraint = ConstraintType::AsSoonAsPossible;
        task.constraint_date = None;
        cleared += 1;
    }

    if cleared > 0 {
        let _ = crate::schedule(project);
    }
    cleared
}

// ---- picking the conflict -----------------------------------------------

/// Rows levelling is allowed to delay, before any per-task objection.
fn candidate_rows(project: &Project, options: &LevelingOptions) -> HashSet<usize> {
    let all = (0..project.tasks.len()).filter(|&index| is_levellable_row(project, index));
    match &options.scope {
        LevelScope::EntireProject | LevelScope::Resource(_) => all.collect(),
        LevelScope::Selected(rows) => {
            // A summary in the selection means its children: a summary has no
            // dates of its own to delay.
            let mut wanted: HashSet<usize> = HashSet::new();
            for &row in rows {
                if row < project.tasks.len() {
                    wanted.extend(project.leaf_indices(row));
                }
            }
            all.filter(|index| wanted.contains(index)).collect()
        }
    }
}

/// Whether a row has dates of its own that a delay could act on.
fn is_levellable_row(project: &Project, index: usize) -> bool {
    !project.is_summary(index) && project.tasks.get(index).is_some_and(|task| task.active)
}

/// The earliest working day on which some resource in scope is booked past its
/// capacity, skipping the conflicts already written off.
fn next_conflict(
    project: &Project,
    options: &LevelingOptions,
    stuck: &HashSet<(ResourceId, NaiveDate)>,
    calendars: &EffectiveCalendars,
) -> Option<(ResourceId, NaiveDate)> {
    let mut best: Option<(ResourceId, NaiveDate)> = None;
    for resource in &project.resources {
        if let LevelScope::Resource(only) = &options.scope
            && resource.id != *only
        {
            continue;
        }
        for date in overloaded_days(project, resource.id, calendars) {
            if stuck.contains(&(resource.id, date)) {
                continue;
            }
            // Ties break on resource id so two runs over the same plan always
            // produce the same delays.
            let better = match best {
                None => true,
                Some((best_resource, best_date)) => {
                    (date, resource.id) < (best_date, best_resource)
                }
            };
            if better {
                best = Some((resource.id, date));
            }
            break;
        }
    }
    best
}

/// Working days on which `resource` is booked past capacity, earliest first.
///
/// The scheduler's overallocation report names only the first such day per
/// resource, and levelling has to keep going once that one is cleared, so the
/// daily load is rebuilt here on exactly the same terms: whole working days,
/// active leaf rows, work resources only.
fn overloaded_days(
    project: &Project,
    resource: ResourceId,
    calendars: &EffectiveCalendars,
) -> Vec<NaiveDate> {
    let Some(limit) = project
        .resource(resource)
        .filter(|entry| entry.kind == ResourceKind::Work)
        .map(|entry| entry.max_units)
    else {
        return Vec::new();
    };

    let mut load: HashMap<NaiveDate, f64> = HashMap::new();
    for index in 0..project.tasks.len() {
        if !is_levellable_row(project, index) {
            continue;
        }
        let units = booked_units(project, index, resource);
        if units <= 0.0 {
            continue;
        }
        let Some(task) = project.tasks.get(index) else {
            continue;
        };
        // The days this task is really worked on, which is the intersection of
        // the calendars it has to satisfy. Reading the project calendar instead
        // would book somebody on a day they are away and then move work onto
        // the days they are not there to clear it.
        let worked = calendars.for_row(index);
        let mut date = task.scheduled.start.date();
        let last = task.scheduled.finish.date();
        let mut seen = 0u32;
        while date <= last && seen < MAX_TASK_DAYS {
            if worked.is_working_day(date) {
                *load.entry(date).or_insert(0.0) += units;
            }
            date += Duration::days(1);
            seen += 1;
        }
    }

    let mut days: Vec<NaiveDate> = load
        .into_iter()
        .filter(|(_, booked)| *booked > limit + UNIT_TOLERANCE)
        .map(|(date, _)| date)
        .collect();
    days.sort_unstable();
    days
}

/// How much of one resource a row books. Summed, because nothing stops a task
/// carrying the same resource on two lines.
fn booked_units(project: &Project, index: usize, resource: ResourceId) -> f64 {
    project
        .tasks
        .get(index)
        .map(|task| {
            task.assignments
                .iter()
                .filter(|assignment| assignment.resource == resource)
                .map(|assignment| assignment.units)
                .sum()
        })
        .unwrap_or(0.0)
}

/// Rows that book `resource` and are running on `date`.
fn runners(project: &Project, resource: ResourceId, date: NaiveDate) -> Vec<usize> {
    (0..project.tasks.len())
        .filter(|&index| is_levellable_row(project, index))
        .filter(|&index| booked_units(project, index, resource) > 0.0)
        .filter(|&index| {
            project.tasks.get(index).is_some_and(|task| {
                task.scheduled.start.date() <= date && date <= task.scheduled.finish.date()
            })
        })
        .collect()
}

/// Where a row sits in the queue for keeping its dates. Lowest keeps.
fn order_key(
    project: &Project,
    index: usize,
    order: LevelOrder,
) -> (u8, i64, NaiveDateTime, TaskId) {
    let Some(task) = project.tasks.get(index) else {
        // A row that is not there sorts last rather than panicking the run.
        return (u8::MAX, i64::MAX, project.start_date, TaskId::MAX);
    };
    match order {
        LevelOrder::IdOnly => (0, 0, project.start_date, task.id),
        LevelOrder::Standard => (
            0,
            task.scheduled.total_slack_minutes,
            task.scheduled.start,
            task.id,
        ),
        LevelOrder::PriorityFirst => {
            let spoken_for =
                task.deadline.is_some() || task.constraint != ConstraintType::AsSoonAsPossible;
            (
                u8::from(!spoken_for),
                task.scheduled.total_slack_minutes,
                task.scheduled.start,
                task.id,
            )
        }
    }
}

// ---- making the move ----------------------------------------------------

/// What one levelling run knows before it starts and does not change.
///
/// Gathered into one place because every step needs all of it and passing the
/// four separately made the signatures longer than the work they described.
struct Run<'a> {
    options: &'a LevelingOptions,
    /// Rows this run is allowed to touch at all.
    candidates: &'a HashSet<usize>,
    /// The finish the plan had before anything moved, which is what
    /// `only_within_slack` measures a proposed delay against.
    baseline_finish: NaiveDateTime,
    /// What each row is worked to. Levelling changes assignments and calendars
    /// not at all, so this is composed once for the whole run.
    calendars: EffectiveCalendars,
}

/// Try to clear one resource's conflict on one day. Reports whether a task was
/// actually delayed, so the caller can write the conflict off when not.
fn resolve_one(
    project: &mut Project,
    resource: ResourceId,
    date: NaiveDate,
    run: &Run<'_>,
    delayed: &mut Vec<(usize, i64)>,
) -> bool {
    let Some(limit) = project.resource(resource).map(|entry| entry.max_units) else {
        return false;
    };

    let mut running = runners(project, resource, date);
    if running.len() < 2 {
        // One task on its own can book more of somebody than they have. Moving
        // it takes the problem with it, so it stays put and gets reported.
        return false;
    }
    running.sort_by_key(|&index| order_key(project, index, run.options.order));

    let mut booked = 0.0f64;
    let mut kept_finish: Option<NaiveDateTime> = None;
    let mut overflow: Vec<usize> = Vec::new();
    for &index in &running {
        let units = booked_units(project, index, resource);
        let finish = project
            .tasks
            .get(index)
            .map(|task| task.scheduled.finish)
            .unwrap_or(run.baseline_finish);
        // The first task always keeps its dates, even where it alone is over
        // capacity, so that there is something for the rest to wait behind.
        if kept_finish.is_none() || booked + units <= limit + UNIT_TOLERANCE {
            booked += units;
            kept_finish = Some(kept_finish.map_or(finish, |kept| kept.max(finish)));
        } else {
            overflow.push(index);
        }
    }
    let Some(kept_finish) = kept_finish else {
        return false;
    };

    // Capacity frees up the day after the work that kept its place finishes,
    // and never earlier than the day after the conflict itself, since a load is
    // counted whole days at a time. Which instant that lands on is a question
    // for each delayed task's own calendar, below: two people freed on the same
    // day do not necessarily start again at the same hour.
    let free_from = kept_finish.date().max(date);

    // Least important first: the queue is in keeping order, so the tail is what
    // the chosen ordering says should yield.
    for &index in overflow.iter().rev() {
        if !can_be_delayed(project, index, run.options, run.candidates) {
            continue;
        }
        let Some(task) = project.tasks.get(index) else {
            continue;
        };
        let previous = (task.constraint, task.constraint_date);
        let was_start = task.scheduled.start;
        let calendar = run.calendars.for_row(index).clone();
        let target = first_instant_after(&calendar, free_from);
        if target <= was_start {
            continue;
        }

        if let Some(task) = project.tasks.get_mut(index) {
            task.constraint = ConstraintType::StartNoEarlierThan;
            task.constraint_date = Some(target);
        }

        let Ok(report) = crate::schedule(project) else {
            // Dating a task cannot create a dependency loop, so a plan that
            // stops scheduling here was already broken. Put it back untouched.
            restore(project, index, previous);
            return false;
        };

        let moved = project
            .tasks
            .get(index)
            .map(|task| calendar.work_minutes_between(was_start, task.scheduled.start))
            .unwrap_or(0);
        let pushed_the_finish =
            run.options.only_within_slack && report.finish > run.baseline_finish;
        if pushed_the_finish || moved <= 0 {
            // Either the delay would cost the project its finish date, or the
            // constraint bought nothing and would only clutter the plan.
            restore(project, index, previous);
            continue;
        }

        record_delay(delayed, index, moved);
        return true;
    }

    false
}

/// Whether this particular row will accept a delay.
fn can_be_delayed(
    project: &Project,
    index: usize,
    options: &LevelingOptions,
    candidates: &HashSet<usize>,
) -> bool {
    if !candidates.contains(&index) {
        return false;
    }
    let Some(task) = project.tasks.get(index) else {
        return false;
    };
    if task.mode == TaskMode::Manual && !options.level_manual {
        return false;
    }
    // Work already under way has a start that actually happened, and no amount
    // of levelling makes it happen later.
    if task.percent_complete > 0 {
        return false;
    }
    // Anything but these two is the planner pinning the task deliberately, and
    // overwriting it would throw away what they said.
    matches!(
        task.constraint,
        ConstraintType::AsSoonAsPossible | ConstraintType::StartNoEarlierThan
    )
}

fn restore(
    project: &mut Project,
    index: usize,
    previous: (ConstraintType, Option<NaiveDateTime>),
) {
    if let Some(task) = project.tasks.get_mut(index) {
        task.constraint = previous.0;
        task.constraint_date = previous.1;
    }
    let _ = crate::schedule(project);
}

/// Add to a row's running total, so a task delayed twice reports one figure.
fn record_delay(delayed: &mut Vec<(usize, i64)>, index: usize, minutes: i64) {
    if let Some(entry) = delayed.iter_mut().find(|(row, _)| *row == index) {
        entry.1 += minutes;
    } else {
        delayed.push((index, minutes));
    }
}

/// The first working instant on a day later than `date`.
fn first_instant_after(calendar: &WorkCalendar, date: NaiveDate) -> NaiveDateTime {
    calendar.next_working_instant((date + Duration::days(1)).and_time(NaiveTime::MIN))
}

/// Whether levelling could ever have written to this row, used to keep
/// `clear_leveling` off rows it plainly did not touch.
fn shares_a_work_resource(project: &Project, index: usize) -> bool {
    if !is_levellable_row(project, index) {
        return false;
    }
    let Some(task) = project.tasks.get(index) else {
        return false;
    };
    task.assignments.iter().any(|assignment| {
        let is_work = project
            .resource(assignment.resource)
            .is_some_and(|entry| entry.kind == ResourceKind::Work);
        is_work
            && (0..project.tasks.len()).any(|other| {
                other != index
                    && is_levellable_row(project, other)
                    && booked_units(project, other, assignment.resource) > 0.0
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Assignment, Link};
    use crate::MINUTES_PER_DAY;

    fn at(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    fn assign(project: &mut Project, index: usize, resource: ResourceId, units: f64) {
        if let Some(task) = project.tasks.get_mut(index) {
            task.assignments.push(Assignment { resource, units });
        }
    }

    /// Two two-day tasks that both want all of Ana from Monday 17 August 2026.
    fn clash() -> (Project, ResourceId) {
        let mut project = Project::blank(at(2026, 8, 17));
        let ana = project.add_resource("Ana");
        project.push_task("One", 2 * MINUTES_PER_DAY);
        project.push_task("Two", 2 * MINUTES_PER_DAY);
        assign(&mut project, 0, ana, 1.0);
        assign(&mut project, 1, ana, 1.0);
        let _ = crate::schedule(&mut project);
        (project, ana)
    }

    #[test]
    fn two_tasks_sharing_one_resource_are_pushed_apart() {
        let (mut project, _) = clash();
        assert_eq!(project.tasks[0].scheduled.start, project.tasks[1].scheduled.start);

        let result = level(&mut project, &LevelingOptions::default());
        assert_eq!(result.delayed.len(), 1, "only one of the pair needs to move");
        assert_eq!(result.resolved, 1);
        assert_eq!(result.remaining, 0);
        assert!(
            project.tasks[1].scheduled.start > project.tasks[0].scheduled.finish,
            "the delayed task waits for the other to finish"
        );
    }

    #[test]
    fn levelling_does_not_move_work_onto_a_day_nobody_is_there() {
        // Ana is booked twice over on the Monday and is then away all week.
        // The delayed task has to land the following Monday: pushing it to a
        // day she is not there would clear the report and not the problem.
        let (mut project, ana) = clash();
        if let Some(resource) = project.resources.iter_mut().find(|r| r.id == ana) {
            resource.calendar_exceptions.push(crate::CalendarException {
                name: "Leave".into(),
                from: NaiveDate::from_ymd_opt(2026, 8, 19).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 8, 28).unwrap(),
                shifts: crate::DayShifts::nonworking(),
            });
        }
        let _ = crate::schedule(&mut project);

        let result = level(&mut project, &LevelingOptions::default());
        assert_eq!(result.delayed.len(), 1);

        let moved = project.tasks[1].scheduled.start;
        assert!(
            moved.date() >= NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(),
            "it waits until she is back, landing on {moved} instead"
        );
        assert!(
            project
                .resource(ana)
                .is_some_and(|r| r.calendar_exceptions.iter().all(|ex| moved.date() > ex.to)),
            "and not in the middle of her leave"
        );
    }

    #[test]
    fn levelling_within_slack_never_moves_the_project_finish() {
        // The whole promise of the option: an unresolved conflict is preferred
        // to a plan that quietly finishes later than it said it would.
        let (mut project, _) = clash();
        let finish_before = project.finish_date;

        let options = LevelingOptions {
            only_within_slack: true,
            ..LevelingOptions::default()
        };
        let result = level(&mut project, &options);

        assert_eq!(project.finish_date, finish_before, "the date is untouched");
        assert!(result.delayed.is_empty(), "there was no slack to spend");
        assert_eq!(result.remaining, 1, "and it says so rather than pretending");
    }

    #[test]
    fn levelling_within_slack_still_spends_the_slack_a_task_has() {
        // The option limits levelling, it does not switch it off, so a conflict
        // that fits inside the float must still be cleared.
        let (mut project, _) = clash();
        project.push_task("Long haul", 10 * MINUTES_PER_DAY);
        let _ = crate::schedule(&mut project);
        let finish_before = project.finish_date;

        let options = LevelingOptions {
            only_within_slack: true,
            ..LevelingOptions::default()
        };
        let result = level(&mut project, &options);

        assert_eq!(result.resolved, 1);
        assert_eq!(result.remaining, 0);
        assert_eq!(project.finish_date, finish_before);
    }

    #[test]
    fn a_summary_row_is_never_delayed_itself() {
        // Its span is rolled up from its children, so a constraint on it would
        // be overwritten by the next reschedule anyway.
        let mut project = Project::blank(at(2026, 8, 17));
        let ana = project.add_resource("Ana");
        project.push_task("Phase", MINUTES_PER_DAY);
        project.push_task("One", 2 * MINUTES_PER_DAY);
        project.push_task("Two", 2 * MINUTES_PER_DAY);
        project.tasks[1].outline_level = 1;
        project.tasks[2].outline_level = 1;
        assign(&mut project, 1, ana, 1.0);
        assign(&mut project, 2, ana, 1.0);
        let _ = crate::schedule(&mut project);
        assert!(project.is_summary(0));

        let result = level(&mut project, &LevelingOptions::default());
        assert_eq!(result.remaining, 0);
        assert_eq!(project.tasks[0].constraint, ConstraintType::AsSoonAsPossible);
        assert!(
            result.delayed.iter().all(|(row, _)| *row != 0),
            "the child moved, not the heading"
        );
    }

    #[test]
    fn an_inactive_task_is_left_out_of_the_reckoning() {
        // The scheduler leaves it out of the plan, so it cannot be booking
        // anybody and there is nothing to level.
        let (mut project, _) = clash();
        project.tasks[1].active = false;
        let _ = crate::schedule(&mut project);

        let result = level(&mut project, &LevelingOptions::default());
        assert!(result.delayed.is_empty());
        assert_eq!(result.remaining, 0);
        assert_eq!(project.tasks[1].constraint, ConstraintType::AsSoonAsPossible);
    }

    #[test]
    fn a_manually_scheduled_task_is_only_moved_when_asked_for() {
        // The point of scheduling a task by hand is that it stays where it was
        // put, so levelling has to be told before it overrides that.
        let (mut project, _) = clash();
        project.tasks[1].mode = TaskMode::Manual;
        project.tasks[1].manual_start = Some(at(2026, 8, 17));
        let _ = crate::schedule(&mut project);

        let left_alone = level(&mut project, &LevelingOptions::default());
        assert!(left_alone.delayed.is_empty());
        assert_eq!(left_alone.remaining, 1);

        let options = LevelingOptions {
            level_manual: true,
            ..LevelingOptions::default()
        };
        let asked_for = level(&mut project, &options);
        assert_eq!(asked_for.delayed.len(), 1);
        assert_eq!(asked_for.remaining, 0);
    }

    /// One and Two clash on Ana; Two feeds a long task, so only One has slack.
    fn clash_with_uneven_slack() -> Project {
        let mut project = Project::blank(at(2026, 8, 17));
        let ana = project.add_resource("Ana");
        let one = project.push_task("One", 2 * MINUTES_PER_DAY);
        let two = project.push_task("Two", 2 * MINUTES_PER_DAY);
        let tail = project.push_task("Tail", 10 * MINUTES_PER_DAY);
        assign(&mut project, 0, ana, 1.0);
        assign(&mut project, 1, ana, 1.0);
        project.links.push(Link::finish_to_start(two, tail));
        let _ = one;
        let _ = crate::schedule(&mut project);
        project
    }

    #[test]
    fn the_levelling_order_decides_which_task_yields() {
        // All three in one place because the whole point of the setting is the
        // contrast: the same clash resolves differently on each.
        let mut standard = clash_with_uneven_slack();
        let result = level(&mut standard, &LevelingOptions::default());
        assert!(standard.tasks[0].scheduled.total_slack_minutes >= 0);
        assert_eq!(result.delayed.len(), 1);
        assert_eq!(result.delayed[0].0, 0, "Standard moves the task with float");

        let mut by_id = clash_with_uneven_slack();
        let result = level(
            &mut by_id,
            &LevelingOptions {
                order: LevelOrder::IdOnly,
                ..LevelingOptions::default()
            },
        );
        assert_eq!(result.delayed[0].0, 1, "IdOnly moves the higher id");

        // A deadline stands in for Project's Priority field, so the task
        // carrying one keeps its dates even though it has the float to spare.
        let mut by_priority = clash_with_uneven_slack();
        by_priority.tasks[0].deadline = Some(at(2026, 8, 21));
        let _ = crate::schedule(&mut by_priority);
        let result = level(
            &mut by_priority,
            &LevelingOptions {
                order: LevelOrder::PriorityFirst,
                ..LevelingOptions::default()
            },
        );
        assert_eq!(result.delayed[0].0, 1, "PriorityFirst spares the spoken-for task");
    }

    #[test]
    fn a_task_pinned_by_an_inflexible_constraint_is_not_moved() {
        // Overwriting a Must Start On would throw away something the planner
        // said outright, which is worse than leaving the conflict standing.
        let (mut project, _) = clash();
        project.tasks[1].constraint = ConstraintType::MustStartOn;
        project.tasks[1].constraint_date = Some(at(2026, 8, 17));
        let _ = crate::schedule(&mut project);

        let options = LevelingOptions {
            order: LevelOrder::IdOnly,
            ..LevelingOptions::default()
        };
        let result = level(&mut project, &options);

        assert!(result.delayed.is_empty());
        assert_eq!(result.remaining, 1);
        assert_eq!(project.tasks[1].constraint, ConstraintType::MustStartOn);
    }

    #[test]
    fn levelling_one_resource_leaves_the_others_alone() {
        let mut project = Project::blank(at(2026, 8, 17));
        let ana = project.add_resource("Ana");
        let bob = project.add_resource("Bob");
        for name in ["A1", "A2", "B1", "B2"] {
            project.push_task(name, 2 * MINUTES_PER_DAY);
        }
        assign(&mut project, 0, ana, 1.0);
        assign(&mut project, 1, ana, 1.0);
        assign(&mut project, 2, bob, 1.0);
        assign(&mut project, 3, bob, 1.0);
        let _ = crate::schedule(&mut project);

        let options = LevelingOptions {
            scope: LevelScope::Resource(ana),
            ..LevelingOptions::default()
        };
        let result = level(&mut project, &options);

        assert_eq!(result.resolved, 1, "Ana is sorted out");
        assert_eq!(result.remaining, 1, "Bob is honestly still double booked");
        assert_eq!(project.tasks[3].constraint, ConstraintType::AsSoonAsPossible);
    }

    #[test]
    fn one_task_that_alone_exceeds_its_resource_is_reported_rather_than_chased() {
        // Delaying it would take the overallocation with it for ever, so the
        // run has to recognise the case and stop instead of walking the dates.
        let mut project = Project::blank(at(2026, 8, 17));
        let ana = project.add_resource("Ana");
        project.push_task("Too much", 2 * MINUTES_PER_DAY);
        assign(&mut project, 0, ana, 2.0);
        let _ = crate::schedule(&mut project);

        let start_before = project.tasks[0].scheduled.start;
        let result = level(&mut project, &LevelingOptions::default());

        assert!(result.delayed.is_empty());
        assert_eq!(result.remaining, 1);
        assert_eq!(project.tasks[0].scheduled.start, start_before);
    }

    #[test]
    fn clearing_levelling_puts_the_delayed_task_back() {
        let (mut project, _) = clash();
        let starts_before: Vec<NaiveDateTime> =
            project.tasks.iter().map(|t| t.scheduled.start).collect();

        let result = level(&mut project, &LevelingOptions::default());
        assert_eq!(result.delayed.len(), 1);

        let cleared = clear_leveling(&mut project);
        assert_eq!(cleared, 1);
        let starts_after: Vec<NaiveDateTime> =
            project.tasks.iter().map(|t| t.scheduled.start).collect();
        assert_eq!(starts_after, starts_before);
        assert!(
            project
                .tasks
                .iter()
                .all(|t| t.constraint == ConstraintType::AsSoonAsPossible)
        );
    }
}
