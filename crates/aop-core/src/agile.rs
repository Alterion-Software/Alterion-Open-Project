//! Burndown, burnup and velocity.
//!
//! These come from the plan rather than from a separate agile tool: a task's
//! work and its scheduled finish are already here, so an iteration is just a
//! window over the same dates the Gantt chart draws.
//!
//! What is measured is **work**, in minutes, not story points. The plan already
//! knows how long things take, and asking a planner to maintain a second unit
//! that has to be kept in step with the first is how the two stop agreeing.
//! Anyone counting points can read the task count instead, which is also here.
//!
//! One honest limit, stated here because it changes how the charts should be
//! read: without a record of what was true on each past day, the "actual" line
//! is computed from where the plan currently says work lands, tempered by how
//! much of it is reported done. It is a projection from today's plan, not a
//! reconstruction of history. A baseline is what makes the comparison mean
//! something, which is why the ideal line follows it when one has been set.

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::model::Project;

/// How long an iteration runs. Two weeks is the common default, and the length
/// is settable because a team's cadence is not something to be guessed at.
pub const DEFAULT_ITERATION_DAYS: i64 = 14;

/// One iteration, and what happened in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Iteration {
    /// One-based, so it reads as "Sprint 1" rather than "Sprint 0".
    pub number: usize,
    pub start: NaiveDate,
    /// Inclusive: the last day of the iteration, not the first of the next.
    pub end: NaiveDate,
    /// Work on tasks finishing inside this window, in minutes.
    pub planned_minutes: i64,
    /// How much of that work is reported complete.
    pub completed_minutes: i64,
    /// Tasks finishing inside this window.
    pub planned_tasks: usize,
    pub completed_tasks: usize,
}

impl Iteration {
    /// Work completed per iteration, which is what velocity is.
    pub fn velocity_minutes(&self) -> i64 {
        self.completed_minutes
    }

    /// Whether the window has closed relative to a given day.
    pub fn is_finished(&self, today: NaiveDate) -> bool {
        self.end < today
    }
}

/// One day on a burn chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BurnPoint {
    pub date: NaiveDate,
    /// Work still to do, in minutes.
    pub remaining_minutes: i64,
    /// Work finished by this date, in minutes.
    pub completed_minutes: i64,
    /// Everything in the plan as at this date. Rises when scope is added,
    /// which is the whole reason a burnup is worth drawing beside a burndown.
    pub scope_minutes: i64,
    /// Where a plan running exactly to schedule would be.
    pub ideal_remaining_minutes: i64,
}

/// What the burn charts are counting.
///
/// Work is the better measure, but it only exists once resources are booked.
/// A plan with none would otherwise chart a flat zero, which looks like a bug
/// rather than a missing input, so duration stands in and the charts say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    Work,
    Duration,
}

impl Basis {
    /// How to describe the measure, for a chart's own label.
    pub fn label(self) -> &'static str {
        match self {
            Basis::Work => "work",
            Basis::Duration => "duration",
        }
    }
}

/// Everything the charts need, worked out in one pass.
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// What the figures are counting. Worth showing: the same chart means a
    /// different thing depending on this.
    pub basis: Basis,
    pub points: Vec<BurnPoint>,
    pub iterations: Vec<Iteration>,
    pub total_minutes: i64,
    pub completed_minutes: i64,
    /// Mean work completed per finished iteration.
    pub average_velocity_minutes: i64,
    /// What the average says about when the remaining work runs out, or `None`
    /// when nothing has been completed yet and the average says nothing.
    pub projected_finish: Option<NaiveDate>,
}

impl Metrics {
    pub fn remaining_minutes(&self) -> i64 {
        (self.total_minutes - self.completed_minutes).max(0)
    }

    pub fn percent_complete(&self) -> f64 {
        if self.total_minutes == 0 {
            return 0.0;
        }
        self.completed_minutes as f64 / self.total_minutes as f64 * 100.0
    }
}

/// Work a task contributes, and how much of it is done.
///
/// Summary rows are skipped throughout: their work is the sum of their
/// children, so counting both would double everything.
fn task_work(project: &Project, index: usize) -> Option<(i64, i64, NaiveDateTime)> {
    measured(project, index, basis_of(project))
}

/// Whether the plan records any work at all.
///
/// Work comes from booking resources onto tasks. A plan that has never had any
/// booked has none, which is a missing input rather than a plan of no size.
pub fn basis_of(project: &Project) -> Basis {
    let any_work = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i))
        .any(|i| project.tasks[i].scheduled.work_minutes > 0);
    if any_work { Basis::Work } else { Basis::Duration }
}

fn measured(project: &Project, index: usize, basis: Basis) -> Option<(i64, i64, NaiveDateTime)> {
    let task = project.tasks.get(index)?;
    if project.is_summary(index) || !task.active {
        return None;
    }
    let size = match basis {
        Basis::Work => task.scheduled.work_minutes,
        Basis::Duration => task.scheduled.duration_minutes,
    }
    .max(0);
    let done = size * task.percent_complete.min(100) as i64 / 100;
    Some((size, done, task.scheduled.finish))
}

/// Split the plan into iterations of `days` each, from its start.
pub fn iterations(project: &Project, days: i64) -> Vec<Iteration> {
    let days = days.max(1);
    let first = project.start_date.date();
    let last = project
        .tasks
        .iter()
        .map(|task| task.scheduled.finish.date())
        .max()
        .unwrap_or(first);

    let mut out = Vec::new();
    let mut start = first;
    let mut number = 1;

    while start <= last {
        let end = start + Duration::days(days - 1);
        let mut iteration = Iteration {
            number,
            start,
            end,
            planned_minutes: 0,
            completed_minutes: 0,
            planned_tasks: 0,
            completed_tasks: 0,
        };

        for index in 0..project.tasks.len() {
            let Some((work, done, finish)) = task_work(project, index) else {
                continue;
            };
            // A task belongs to the iteration it finishes in, which is the
            // convention that makes velocity add up to the total.
            let finish = finish.date();
            if finish < start || finish > end {
                continue;
            }
            iteration.planned_minutes += work;
            iteration.completed_minutes += done;
            iteration.planned_tasks += 1;
            if project.tasks[index].percent_complete >= 100 {
                iteration.completed_tasks += 1;
            }
        }

        out.push(iteration);
        start = end + Duration::days(1);
        number += 1;

        // A plan whose dates are nonsense must not spin here.
        if number > 500 {
            break;
        }
    }

    out
}

/// Work out the burn charts and the velocity figures.
pub fn metrics(project: &Project, iteration_days: i64) -> Metrics {
    let rows: Vec<(i64, i64, NaiveDate)> = (0..project.tasks.len())
        .filter_map(|index| {
            task_work(project, index).map(|(work, done, finish)| (work, done, finish.date()))
        })
        .collect();

    let total: i64 = rows.iter().map(|(work, _, _)| work).sum();
    let completed: i64 = rows.iter().map(|(_, done, _)| done).sum();

    let first = project.start_date.date();
    let last = rows.iter().map(|(_, _, finish)| *finish).max().unwrap_or(first);
    let span = (last - first).num_days().max(1);

    // The baseline is what a burndown is honestly measured against, so it sets
    // the ideal line when one has been taken.
    let baseline_total: i64 = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i))
        .filter_map(|i| project.tasks[i].baseline.map(|b| b.work_minutes.max(0)))
        .sum();
    let ideal_total = if baseline_total > 0 { baseline_total } else { total };

    let mut points = Vec::new();
    let mut day = first;
    while day <= last {
        let elapsed = (day - first).num_days();

        // Work whose task is scheduled to have finished by this day, weighted
        // by how much of it is actually reported done.
        let done_by: i64 = rows
            .iter()
            .filter(|(_, _, finish)| *finish <= day)
            .map(|(_, done, _)| done)
            .sum();
        let scope: i64 = rows.iter().map(|(work, _, _)| work).sum();

        points.push(BurnPoint {
            date: day,
            remaining_minutes: (total - done_by).max(0),
            completed_minutes: done_by,
            scope_minutes: scope,
            ideal_remaining_minutes: ideal_total
                - (ideal_total as f64 * elapsed as f64 / span as f64).round() as i64,
        });

        day += Duration::days(1);
    }

    let iterations = iterations(project, iteration_days);

    // Only closed iterations count towards an average. Including the one in
    // progress drags it down for no reason other than that it is not over.
    let today = project
        .status_date
        .map(|date| date.date())
        .unwrap_or_else(|| chrono::Local::now().naive_local().date());
    let finished: Vec<&Iteration> = iterations
        .iter()
        .filter(|iteration| iteration.is_finished(today))
        .collect();

    let average = if finished.is_empty() {
        0
    } else {
        finished
            .iter()
            .map(|iteration| iteration.velocity_minutes())
            .sum::<i64>()
            / finished.len() as i64
    };

    let remaining = (total - completed).max(0);
    let projected_finish = if average > 0 && remaining > 0 {
        // Rounded up: a part-full iteration is still an iteration of work.
        let iterations_left = (remaining + average - 1) / average;
        Some(today + Duration::days(iterations_left * iteration_days.max(1)))
    } else {
        None
    };

    Metrics {
        basis: basis_of(project),
        points,
        iterations,
        total_minutes: total,
        completed_minutes: completed,
        average_velocity_minutes: average,
        projected_finish,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Task;

    fn at(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    /// A plan of leaf tasks with the work and completion given.
    fn plan(rows: &[(i64, u8)]) -> Project {
        let mut project = Project::blank(at(2026, 1, 5));
        project.tasks.clear();
        for (index, (work, percent)) in rows.iter().enumerate() {
            let id = project.allocate_task_id();
            let mut task = Task::new(id, format!("Task {}", index + 1), *work);
            task.percent_complete = *percent;
            task.scheduled.work_minutes = *work;
            task.scheduled.start = at(2026, 1, 5) + Duration::days(index as i64 * 7);
            task.scheduled.finish = task.scheduled.start + Duration::days(4);
            project.tasks.push(task);
        }
        project
    }

    #[test]
    fn a_plan_with_no_resources_booked_falls_back_to_duration() {
        // Charting a flat zero would look like a bug rather than a missing
        // input, and plenty of plans never book a resource at all.
        let mut project = plan(&[(480, 50)]);
        project.tasks[0].scheduled.work_minutes = 0;
        project.tasks[0].duration_minutes = 960;
        project.tasks[0].scheduled.duration_minutes = 960;

        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.basis, Basis::Duration);
        assert_eq!(m.total_minutes, 960, "it measures the duration instead");
        assert_eq!(m.basis.label(), "duration", "and the chart can say so");
    }

    #[test]
    fn a_plan_with_work_booked_measures_work() {
        let project = plan(&[(480, 0)]);
        assert_eq!(metrics(&project, DEFAULT_ITERATION_DAYS).basis, Basis::Work);
    }

    #[test]
    fn work_and_completion_are_totalled_from_the_leaves() {
        let project = plan(&[(480, 100), (480, 50), (480, 0)]);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.total_minutes, 1440);
        assert_eq!(m.completed_minutes, 480 + 240);
        assert_eq!(m.remaining_minutes(), 720);
        assert!((m.percent_complete() - 50.0).abs() < 0.01);
    }

    #[test]
    fn a_summary_row_is_not_counted_twice() {
        // Its work is the sum of its children, so counting both doubles it.
        let mut project = plan(&[(480, 0), (480, 0)]);
        project.tasks[0].outline_level = 0;
        project.tasks[1].outline_level = 1;
        project.tasks[0].scheduled.work_minutes = 960;

        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.total_minutes, 480, "only the leaf counts");
    }

    #[test]
    fn an_inactive_task_is_left_out_of_the_totals() {
        let mut project = plan(&[(480, 0), (480, 0)]);
        project.tasks[1].active = false;
        assert_eq!(metrics(&project, DEFAULT_ITERATION_DAYS).total_minutes, 480);
    }

    #[test]
    fn a_burndown_starts_at_the_total_and_never_goes_below_zero() {
        let project = plan(&[(480, 100), (480, 100)]);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.points.first().unwrap().remaining_minutes, 960);
        assert!(m.points.iter().all(|p| p.remaining_minutes >= 0));
        assert_eq!(m.points.last().unwrap().remaining_minutes, 0);
    }

    #[test]
    fn a_burnup_rises_to_meet_the_scope_line() {
        let project = plan(&[(480, 100), (480, 100)]);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        let last = m.points.last().unwrap();
        assert_eq!(last.completed_minutes, last.scope_minutes);
        // Completion only ever goes up.
        for pair in m.points.windows(2) {
            assert!(pair[1].completed_minutes >= pair[0].completed_minutes);
        }
    }

    #[test]
    fn the_ideal_line_runs_from_everything_to_nothing() {
        let project = plan(&[(480, 0), (480, 0), (480, 0)]);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.points.first().unwrap().ideal_remaining_minutes, 1440);
        assert_eq!(m.points.last().unwrap().ideal_remaining_minutes, 0);
    }

    #[test]
    fn a_task_belongs_to_the_iteration_it_finishes_in() {
        // That convention is what makes the velocities add up to the total.
        let project = plan(&[(480, 100), (480, 100), (480, 100)]);
        let all = iterations(&project, 7);
        let counted: usize = all.iter().map(|i| i.planned_tasks).sum();
        assert_eq!(counted, 3, "every task lands in exactly one iteration");
        let work: i64 = all.iter().map(|i| i.planned_minutes).sum();
        assert_eq!(work, 1440);
    }

    #[test]
    fn iterations_are_numbered_from_one_and_do_not_overlap() {
        let project = plan(&[(480, 0), (480, 0), (480, 0)]);
        let all = iterations(&project, 7);
        assert_eq!(all[0].number, 1);
        for pair in all.windows(2) {
            assert!(pair[0].end < pair[1].start, "no day is in two iterations");
            assert_eq!(pair[1].start, pair[0].end + Duration::days(1), "and none is missed");
        }
    }

    #[test]
    fn velocity_only_counts_iterations_that_have_finished() {
        // Counting the one in progress drags the average down for no reason
        // other than that it is not over yet.
        let mut project = plan(&[(480, 100), (480, 100), (480, 0)]);
        project.status_date = Some(at(2026, 1, 14));
        let m = metrics(&project, 7);

        let closed = m
            .iterations
            .iter()
            .filter(|i| i.is_finished(at(2026, 1, 14).date()))
            .count();
        assert!(closed > 0 && closed < m.iterations.len());
        assert!(m.average_velocity_minutes > 0);
    }

    #[test]
    fn nothing_completed_means_no_projection_rather_than_a_wrong_one() {
        let project = plan(&[(480, 0), (480, 0)]);
        let m = metrics(&project, 7);
        assert_eq!(m.average_velocity_minutes, 0);
        assert!(
            m.projected_finish.is_none(),
            "an average of nothing forecasts nothing"
        );
    }

    #[test]
    fn a_finished_plan_needs_no_projection() {
        let mut project = plan(&[(480, 100), (480, 100)]);
        project.status_date = Some(at(2026, 3, 1));
        let m = metrics(&project, 7);
        assert_eq!(m.remaining_minutes(), 0);
        assert!(m.projected_finish.is_none(), "there is nothing left to forecast");
    }

    #[test]
    fn an_empty_plan_produces_empty_metrics_rather_than_dividing_by_zero() {
        let mut project = plan(&[]);
        project.tasks.clear();
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.total_minutes, 0);
        assert_eq!(m.percent_complete(), 0.0);
        assert!(m.projected_finish.is_none());
    }

    #[test]
    fn an_iteration_length_of_zero_is_treated_as_one_day() {
        // Rather than looping forever on a zero width window.
        let project = plan(&[(480, 0)]);
        let all = iterations(&project, 0);
        assert!(!all.is_empty());
        assert_eq!(all[0].start, all[0].end);
    }
}
