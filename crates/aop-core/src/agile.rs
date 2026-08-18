//! Burndown, burnup and velocity.
//!
//! These come from the plan rather than from a separate agile tool: a task's
//! work and its scheduled finish are already here, so an iteration is either a
//! sprint the plan already declares or, failing that, a window over the same
//! dates the Gantt chart draws.
//!
//! What is measured is **work**, in minutes, not story points. The plan already
//! knows how long things take, and asking a planner to maintain a second unit
//! that has to be kept in step with the first is how the two stop agreeing. A
//! plan with no resources booked records no work, and then what is counted is
//! tasks: a real quantity, and the one Project's own burndown plots on its
//! second chart. Duration is deliberately not an option, because durations
//! overlap in time and summing them counts the same week twice.
//!
//! Two limits are stated here because they change how the charts must be read.
//! Actual start and finish dates are used wherever a plan records them, but
//! nothing records what was true on a past *day*, so the actual line is
//! today's reported progress spread back across the elapsed part of each
//! task's span; it is a reconstruction from today's plan rather than history,
//! and it stops at the status date rather than running on through a future in
//! which nothing has happened yet. Nothing records when scope was added either, so the
//! burnup's scope line is the plan's size as it stands today and cannot rise
//! part way along. The baseline is what makes any of it a comparison, which is
//! why the ideal line is the baseline's own remaining curve when one is set.

use chrono::{Duration, NaiveDate};

use crate::model::Project;

/// How long an iteration runs when the plan does not declare its own. Two
/// weeks is the usual cadence, and a plan that names its sprints is believed
/// ahead of anything guessed here.
pub const DEFAULT_ITERATION_DAYS: i64 = 14;

/// A plan whose dates are nonsense must not draw a point a day for a thousand
/// years and take the window down with it.
const MAX_POINTS: usize = 3660;

/// One iteration, and what happened in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iteration {
    /// One-based, so it reads as "Sprint 1" rather than "Sprint 0".
    pub number: usize,
    /// What to call it: the summary row's own name when the plan declares its
    /// sprints, otherwise the window's number.
    pub name: String,
    /// Whether the plan declared this iteration or it was inferred from a
    /// fixed window. Worth showing: the two mean different things.
    pub declared: bool,
    pub start: NaiveDate,
    /// Inclusive: the last day of the iteration, not the first of the next.
    pub end: NaiveDate,
    /// Size of the tasks in this iteration, in the units of the basis.
    pub planned: i64,
    /// Size of the tasks in it that are reported finished. Only tasks at 100%
    /// count, because a task half done was not delivered, and each is credited
    /// to the iteration its actual finish falls in when the plan records one.
    pub completed: i64,
    pub planned_tasks: usize,
    pub completed_tasks: usize,
}

impl Iteration {
    /// Work delivered in the iteration, which is what velocity is.
    pub fn velocity(&self) -> i64 {
        self.completed
    }

    /// Whether the window has closed relative to a given day.
    pub fn is_finished(&self, today: NaiveDate) -> bool {
        self.end < today
    }

    pub fn is_running(&self, today: NaiveDate) -> bool {
        self.start <= today && self.end >= today
    }

    /// How many days it runs, counting both ends.
    pub fn length_days(&self) -> i64 {
        (self.end - self.start).num_days() + 1
    }
}

/// One day on a burn chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BurnPoint {
    pub date: NaiveDate,
    /// Still to do as at this day, or `None` past the status date. Nothing
    /// after today has happened yet, and a line carried on to the right edge
    /// reads as a forecast of no progress at all.
    pub remaining: Option<i64>,
    /// Finished by this day, on the same terms.
    pub completed: Option<i64>,
    /// Everything in the plan. Flat, because no history of scope changes is
    /// kept: this is the plan's size as it stands today, not as it stood then.
    pub scope: i64,
    /// Where the baseline said the plan would be, or a straight run from the
    /// total to zero when no baseline has been taken.
    pub ideal_remaining: i64,
}

/// What the burn charts are counting.
///
/// Work is the better measure, but it only exists once resources are booked.
/// A plan with none would otherwise chart a flat zero, so tasks are counted
/// instead and the charts say which they are showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    Work,
    Count,
}

impl Basis {
    /// How to describe the measure, for a chart's own label.
    pub fn label(self) -> &'static str {
        match self {
            Basis::Work => "work",
            Basis::Count => "tasks",
        }
    }

    /// The unit the axis is scaled in, and how many of the measure go into one
    /// of them. Work is minutes and needs converting; a count does not.
    pub fn axis_unit(self, peak: i64) -> (&'static str, f64) {
        const HOUR: f64 = 60.0;
        const DAY: f64 = 480.0;
        match self {
            Basis::Count => ("tasks", 1.0),
            Basis::Work if peak as f64 >= DAY * 5.0 => ("days", DAY),
            Basis::Work if peak as f64 >= HOUR * 3.0 => ("hours", HOUR),
            Basis::Work => ("minutes", 1.0),
        }
    }

    /// A figure in this measure. Work is an amount of effort and is written in
    /// hours: rendering it the way a duration is written turns three days of
    /// one person's time into "3 days" of calendar, which is the very
    /// distinction these pages exist to draw.
    pub fn format(self, value: i64) -> String {
        match self {
            Basis::Work => crate::duration::format_work(value),
            Basis::Count if value == 1 => "1 task".into(),
            Basis::Count => format!("{value} tasks"),
        }
    }
}

/// Everything the charts need, worked out in one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metrics {
    /// What the figures are counting. Worth showing: the same chart means a
    /// different thing depending on this.
    pub basis: Basis,
    /// The day every figure on the page is read at, so that setting a status
    /// date in the past moves the whole page together rather than half of it.
    pub status_date: NaiveDate,
    pub points: Vec<BurnPoint>,
    pub iterations: Vec<Iteration>,
    /// Whether the iterations came from sprints the plan declares.
    pub iterations_declared: bool,
    pub total: i64,
    /// Reported progress with partial credit, which is what a burndown draws.
    pub completed: i64,
    /// Size of the tasks not yet reported finished. Velocity counts whole
    /// tasks, so this is what its forecast has left to get through.
    pub incomplete: i64,
    /// What the baseline recorded, in the same measure, when one was taken.
    pub baseline_total: Option<i64>,
    /// Whether the ideal line is the baseline's own remaining curve rather
    /// than a straight run to zero.
    pub ideal_from_baseline: bool,
    /// Mean delivered per closed iteration that had anything planned in it.
    pub average_velocity: i64,
    /// What the average says about when the outstanding tasks run out, or
    /// `None` when nothing has been finished yet and the average says nothing.
    pub projected_finish: Option<NaiveDate>,
}

impl Metrics {
    pub fn remaining(&self) -> i64 {
        (self.total - self.completed).max(0)
    }

    pub fn percent_complete(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.completed as f64 / self.total as f64 * 100.0
    }

    /// Where the plan stands against where it meant to be, positive being
    /// behind.
    ///
    /// Read at the status date, falling back to the last point drawn, because
    /// a plan whose dates all sit in the past must not read as on plan for
    /// want of a point to read.
    pub fn against_plan(&self) -> i64 {
        let point = self
            .points
            .iter()
            .find(|p| p.date >= self.status_date)
            .or_else(|| self.points.last());
        point
            .map(|p| p.remaining.unwrap_or_else(|| self.remaining()) - p.ideal_remaining)
            .unwrap_or(0)
    }

    /// How many iterations have closed as at the status date.
    pub fn closed_iterations(&self) -> usize {
        self.iterations
            .iter()
            .filter(|i| i.is_finished(self.status_date))
            .count()
    }

    /// How much the plan has grown since the baseline was taken, when one was.
    /// The one scope movement that can honestly be shown without a history.
    pub fn scope_change(&self) -> Option<i64> {
        self.baseline_total.map(|baseline| self.total - baseline)
    }

    /// The last day the actual line is drawn for.
    pub fn actual_end(&self) -> Option<NaiveDate> {
        self.points
            .iter()
            .rev()
            .find(|p| p.remaining.is_some())
            .map(|p| p.date)
    }
}

/// A task's contribution to the charts.
///
/// Summary rows are skipped throughout: their work is the sum of their
/// children, so counting both would double everything.
#[derive(Debug, Clone, Copy)]
struct Row {
    size: i64,
    /// Partial credit from the reported percentage, which is what a burndown
    /// draws.
    done: i64,
    /// Whether the task is reported finished, which is what velocity counts.
    finished: bool,
    start: NaiveDate,
    finish: NaiveDate,
}

fn row(project: &Project, index: usize, basis: Basis) -> Option<Row> {
    let task = project.tasks.get(index)?;
    if project.is_summary(index) || !task.active {
        return None;
    }
    let percent = task.percent_complete.min(100) as i64;
    let size = match basis {
        Basis::Work => task.scheduled.work_minutes.max(0),
        Basis::Count => 1,
    };
    let done = match basis {
        Basis::Work => size * percent / 100,
        // There is no half a task, so a count only moves at the finish.
        Basis::Count => i64::from(percent >= 100),
    };
    // A task that records when work really began and ended is read on those
    // dates. Without them the only dates the plan has are the scheduled ones,
    // and then an iteration credits what was meant to finish in it rather than
    // what did.
    Some(Row {
        size,
        done,
        finished: percent >= 100,
        start: task.actual_start.unwrap_or(task.scheduled.start).date(),
        finish: task.actual_finish.unwrap_or(task.scheduled.finish).date(),
    })
}

/// The day the reports are written against: the plan's status date when one is
/// set, otherwise today. Every figure on every report page reads this, so a
/// status date in the past moves the whole page rather than half of it.
pub fn status_date(project: &Project) -> NaiveDate {
    project
        .status_date
        .map(|date| date.date())
        .unwrap_or_else(|| chrono::Local::now().naive_local().date())
}

/// Whether the plan records any work at all.
///
/// Work comes from booking resources onto tasks. A plan that has never had any
/// booked has none, which is a missing input rather than a plan of no size.
pub fn basis_of(project: &Project) -> Basis {
    let any_work = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i))
        .any(|i| project.tasks[i].scheduled.work_minutes > 0);
    if any_work { Basis::Work } else { Basis::Count }
}

/// Whether a row's name declares it a sprint. Our own Agile template names its
/// sprints this way, and so does most of what a planner writes by hand.
fn names_an_iteration(name: &str) -> bool {
    let first = name.split_whitespace().next().unwrap_or("");
    first.eq_ignore_ascii_case("sprint") || first.eq_ignore_ascii_case("iteration")
}

/// Whether a row already sits inside a sprint, in which case taking it as an
/// iteration of its own would count the rows under it twice.
fn nested_in_an_iteration(project: &Project, index: usize) -> bool {
    let mut above = project.parent_index(index);
    while let Some(parent) = above {
        if names_an_iteration(&project.tasks[parent].name) {
            return true;
        }
        above = project.parent_index(parent);
    }
    false
}

/// Iterations the plan declares for itself: summary rows named for a sprint.
///
/// Membership is the plan's own statement, the rows nested under the sprint,
/// rather than whatever happens to finish between its dates. A plan that
/// carries its cadence should not have a fixed window sliced through it.
fn declared_iterations(project: &Project, basis: Basis) -> Vec<Iteration> {
    let mut out: Vec<Iteration> = Vec::new();
    for index in 0..project.tasks.len() {
        if !project.is_summary(index) {
            continue;
        }
        let task = &project.tasks[index];
        if !task.active || !names_an_iteration(&task.name) {
            continue;
        }
        // A sprint nested inside another sprint would have its rows counted
        // twice, so only the outermost one is taken.
        if nested_in_an_iteration(project, index) {
            continue;
        }

        let mut iteration = Iteration {
            number: out.len() + 1,
            name: task.name.clone(),
            declared: true,
            start: task.scheduled.start.date(),
            end: task.scheduled.finish.date(),
            planned: 0,
            completed: 0,
            planned_tasks: 0,
            completed_tasks: 0,
        };
        for child in project.descendants(index) {
            let Some(row) = row(project, child, basis) else {
                continue;
            };
            iteration.planned += row.size;
            iteration.planned_tasks += 1;
            if row.finished {
                iteration.completed += row.size;
                iteration.completed_tasks += 1;
            }
        }
        out.push(iteration);
    }
    out
}

/// Fixed windows of `days` from the plan's start, for a plan that names no
/// sprints of its own.
fn windowed_iterations(project: &Project, days: i64, basis: Basis) -> Vec<Iteration> {
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
            name: format!("Iteration {number}"),
            declared: false,
            start,
            end,
            planned: 0,
            completed: 0,
            planned_tasks: 0,
            completed_tasks: 0,
        };

        for index in 0..project.tasks.len() {
            let Some(row) = row(project, index, basis) else {
                continue;
            };
            // The date it really finished on when the plan records one, and
            // the date it is scheduled to finish on when it does not.
            if row.finish < start || row.finish > end {
                continue;
            }
            iteration.planned += row.size;
            iteration.planned_tasks += 1;
            if row.finished {
                iteration.completed += row.size;
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

/// The plan's iterations: the sprints it declares, or fixed windows of `days`
/// from its start when it declares none.
pub fn iterations(project: &Project, days: i64) -> Vec<Iteration> {
    let basis = basis_of(project);
    let declared = declared_iterations(project, basis);
    if declared.is_empty() {
        windowed_iterations(project, days, basis)
    } else {
        declared
    }
}

/// Work out the burn charts and the velocity figures.
pub fn metrics(project: &Project, iteration_days: i64) -> Metrics {
    let basis = basis_of(project);
    let rows: Vec<Row> = (0..project.tasks.len())
        .filter_map(|index| row(project, index, basis))
        .collect();

    let total: i64 = rows.iter().map(|r| r.size).sum();
    let completed: i64 = rows.iter().map(|r| r.done).sum();
    let incomplete: i64 = rows.iter().filter(|r| !r.finished).map(|r| r.size).sum();

    let today = status_date(project);
    let first = project.start_date.date();

    // The baseline is read in the measure the chart is drawn in: baseline work
    // against a count of tasks would be two lines with nothing in common.
    let baseline: Vec<(NaiveDate, i64)> = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i) && project.tasks[i].active)
        .filter_map(|i| {
            project.tasks[i].baseline.map(|b| {
                let size = match basis {
                    Basis::Work => b.work_minutes.max(0),
                    Basis::Count => 1,
                };
                (b.finish.date(), size)
            })
        })
        .collect();
    let baseline_total: i64 = baseline.iter().map(|(_, size)| size).sum();
    let ideal_from_baseline = baseline_total > 0;

    let plan_last = rows.iter().map(|r| r.finish).max().unwrap_or(first);
    let baseline_last = baseline.iter().map(|(finish, _)| *finish).max();
    // The chart runs as far as today even when the plan says it should already
    // be over: that gap is exactly what an overdue plan has to show.
    let last = plan_last
        .max(today)
        .max(baseline_last.unwrap_or(plan_last))
        .max(first);

    // Without a baseline there is no shape to follow, only a total, so the
    // ideal line is the straight run to zero over the plan's own span. With
    // one it is the baseline's remaining curve, which ends where the baseline
    // ended rather than where the plan now does, so a slip shows as a slip.
    let straight_span = (plan_last - first).num_days().max(1);
    let ideal_at = |day: NaiveDate| -> i64 {
        if ideal_from_baseline {
            let met: i64 = baseline
                .iter()
                .filter(|(finish, _)| *finish <= day)
                .map(|(_, size)| size)
                .sum();
            (baseline_total - met).max(0)
        } else {
            let elapsed = (day - first).num_days().clamp(0, straight_span);
            total - (total as f64 * elapsed as f64 / straight_span as f64).round() as i64
        }
    };

    // Reported progress carries no date of its own, so it is spread across the
    // elapsed part of each task's span. Dropping it all on the finish date
    // instead would make the line a staircase and hide every task in flight,
    // and would leave the chart and the figures disagreeing about the same
    // word: by the status date every task has its full reported credit, so
    // the last drawn point is the Remaining figure exactly.
    let actual_end = today.max(first);
    let completed_at = |day: NaiveDate| -> i64 {
        rows.iter()
            .map(|r| {
                let from = r.start.min(actual_end);
                let to = r.finish.min(actual_end).max(from);
                if day >= to {
                    r.done
                } else if day <= from {
                    0
                } else {
                    let elapsed = (day - from).num_days() as f64;
                    let span = (to - from).num_days() as f64;
                    (r.done as f64 * elapsed / span).round() as i64
                }
            })
            .sum()
    };

    let mut points = Vec::new();
    let mut day = first;
    while day <= last && points.len() < MAX_POINTS {
        let (remaining, done) = if day <= actual_end {
            let done = completed_at(day);
            (Some((total - done).max(0)), Some(done))
        } else {
            (None, None)
        };
        points.push(BurnPoint {
            date: day,
            remaining,
            completed: done,
            scope: total,
            ideal_remaining: ideal_at(day),
        });
        day += Duration::days(1);
    }

    let iterations = iterations(project, iteration_days);
    let iterations_declared = iterations.first().is_some_and(|i| i.declared);

    // Only closed iterations count towards an average, and only ones that had
    // something planned in them: an empty window says nothing about how fast
    // the team goes and averaging it in drags the forecast out for no reason.
    let closed: Vec<&Iteration> = iterations
        .iter()
        .filter(|iteration| iteration.is_finished(today) && iteration.planned > 0)
        .collect();

    let average = if closed.is_empty() {
        0
    } else {
        closed.iter().map(|iteration| iteration.velocity()).sum::<i64>() / closed.len() as i64
    };

    // Forecasting in iterations means knowing how long one runs, which a plan
    // that declares its own sprints has already answered.
    let cadence = if iterations.is_empty() {
        iteration_days.max(1)
    } else {
        let days: i64 = iterations.iter().map(|i| i.length_days()).sum();
        (days / iterations.len() as i64).max(1)
    };

    // Measured against what velocity itself counts: whole tasks not yet done.
    let projected_finish = if average > 0 && incomplete > 0 {
        // Rounded up: a part-full iteration is still an iteration of work.
        let iterations_left = (incomplete + average - 1) / average;
        Some(today + Duration::days(iterations_left * cadence))
    } else {
        None
    };

    Metrics {
        basis,
        status_date: today,
        points,
        iterations,
        iterations_declared,
        total,
        completed,
        incomplete,
        baseline_total: if baseline.is_empty() { None } else { Some(baseline_total) },
        ideal_from_baseline,
        average_velocity: average,
        projected_finish,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Baseline, Task};
    use chrono::NaiveDateTime;

    fn at(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    /// A plan of leaf tasks with the work and completion given, one a week.
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

    /// The Agile template's shape: sprints as summary rows with their work
    /// nested underneath.
    fn sprints() -> Project {
        let mut project = Project::blank(at(2026, 1, 5));
        project.tasks.clear();
        for sprint in 0..2 {
            let id = project.allocate_task_id();
            let mut head = Task::new(id, format!("Sprint {}", sprint + 1), 0);
            head.outline_level = 0;
            head.scheduled.start = at(2026, 1, 5) + Duration::days(sprint * 14);
            head.scheduled.finish = head.scheduled.start + Duration::days(13);
            project.tasks.push(head);

            for row in 0..2 {
                let id = project.allocate_task_id();
                let mut task = Task::new(id, format!("Sprint {} task {}", sprint + 1, row + 1), 480);
                task.outline_level = 1;
                task.scheduled.work_minutes = 480;
                task.scheduled.start = at(2026, 1, 5) + Duration::days(sprint * 14 + row * 3);
                task.scheduled.finish = task.scheduled.start + Duration::days(2);
                project.tasks.push(task);
            }
        }
        project
    }

    #[test]
    fn a_plan_with_no_resources_booked_counts_tasks() {
        // Summing durations would add two parallel five day tasks into ten
        // "days" where five elapse, which is neither effort nor elapsed time.
        // A count of tasks is a real quantity and is what Project falls to.
        let mut project = plan(&[(480, 50), (480, 100)]);
        for task in &mut project.tasks {
            task.scheduled.work_minutes = 0;
        }

        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.basis, Basis::Count);
        assert_eq!(m.total, 2, "two tasks, not their durations added together");
        assert_eq!(m.completed, 1, "and half a task is not half delivered");
        assert_eq!(m.basis.label(), "tasks", "and the chart can say so");
        assert_eq!(m.basis.format(1), "1 task");
    }

    #[test]
    fn a_plan_with_work_booked_measures_work() {
        let project = plan(&[(480, 0)]);
        assert_eq!(metrics(&project, DEFAULT_ITERATION_DAYS).basis, Basis::Work);
    }

    #[test]
    fn work_is_written_in_hours_rather_than_as_a_duration() {
        // 24 person-hours is three days of one person's time, not three days
        // of calendar, and these pages exist to draw that distinction.
        assert_eq!(Basis::Work.format(1440), "24 hrs");
    }

    #[test]
    fn work_and_completion_are_totalled_from_the_leaves() {
        let project = plan(&[(480, 100), (480, 50), (480, 0)]);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.total, 1440);
        assert_eq!(m.completed, 480 + 240);
        assert_eq!(m.remaining(), 720);
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
        assert_eq!(m.total, 480, "only the leaf counts");
    }

    #[test]
    fn an_inactive_task_is_left_out_of_the_totals() {
        let mut project = plan(&[(480, 0), (480, 0)]);
        project.tasks[1].active = false;
        assert_eq!(metrics(&project, DEFAULT_ITERATION_DAYS).total, 480);
    }

    #[test]
    fn the_chart_and_the_remaining_figure_agree_at_the_status_date() {
        // The same word twice on one page has to be the same number. A task
        // half done today but finishing next month counts in both or neither.
        let mut project = plan(&[(480, 50), (480, 0), (480, 100)]);
        project.tasks[0].scheduled.finish = at(2026, 6, 1);
        project.status_date = Some(at(2026, 1, 20));

        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        let today = m
            .points
            .iter()
            .find(|p| p.date == m.status_date)
            .expect("the chart is drawn as far as the status date");
        assert_eq!(today.remaining, Some(m.remaining()));
        assert_eq!(today.completed, Some(m.completed));
    }

    #[test]
    fn the_actual_line_stops_at_the_status_date() {
        // Past today nothing has progress, so a line carried to the right edge
        // reads as a forecast of no progress at all.
        let mut project = plan(&[(480, 0), (480, 0)]);
        project.status_date = Some(at(2026, 1, 8));
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);

        assert_eq!(m.actual_end(), Some(at(2026, 1, 8).date()));
        assert!(
            m.points.iter().any(|p| p.date > m.status_date),
            "the ideal line still runs the length of the plan"
        );
        assert!(m
            .points
            .iter()
            .filter(|p| p.date > m.status_date)
            .all(|p| p.remaining.is_none()));
    }

    #[test]
    fn progress_shows_before_the_finish_date_rather_than_all_at_once() {
        // A staircase hides every task in flight until its finish date.
        let mut project = plan(&[(480, 50)]);
        project.tasks[0].scheduled.start = at(2026, 1, 5);
        project.tasks[0].scheduled.finish = at(2026, 1, 15);
        project.status_date = Some(at(2026, 1, 20));

        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        let midway = m
            .points
            .iter()
            .find(|p| p.date == at(2026, 1, 10).date())
            .and_then(|p| p.completed)
            .unwrap_or(0);
        assert!(midway > 0 && midway < 240, "part way through, part credited");
    }

    #[test]
    fn a_burndown_starts_at_the_total_and_never_goes_below_zero() {
        let mut project = plan(&[(480, 100), (480, 100)]);
        project.status_date = Some(at(2026, 3, 1));
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.points.first().and_then(|p| p.remaining), Some(960));
        assert!(m.points.iter().all(|p| p.remaining.unwrap_or(0) >= 0));
        assert_eq!(m.points.last().and_then(|p| p.remaining), Some(0));
    }

    #[test]
    fn the_scope_line_is_the_plan_as_it_stands_and_does_not_pretend_to_rise() {
        // No history of scope changes is kept, so it cannot rise part way
        // along and the page must not claim it does.
        let project = plan(&[(480, 100), (480, 100)]);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert!(m.points.iter().all(|p| p.scope == m.total));
    }

    #[test]
    fn scope_against_the_baseline_is_the_one_movement_that_can_be_shown() {
        let mut project = plan(&[(480, 0), (480, 0)]);
        project.tasks[0].baseline = Some(Baseline {
            start: at(2026, 1, 5),
            finish: at(2026, 1, 9),
            duration_minutes: 2400,
            work_minutes: 480,
            cost: 0.0,
        });
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.scope_change(), Some(480), "a task's worth added since");
    }

    #[test]
    fn the_ideal_line_follows_the_baseline_rather_than_a_straight_run() {
        // Project's comparison line is the baseline remaining curve, stepped
        // on the baseline's own finish dates.
        let mut project = plan(&[(480, 0), (480, 0)]);
        for (index, finish) in [at(2026, 1, 9), at(2026, 1, 16)].into_iter().enumerate() {
            project.tasks[index].baseline = Some(Baseline {
                start: at(2026, 1, 5),
                finish,
                duration_minutes: 2400,
                work_minutes: 480,
                cost: 0.0,
            });
        }
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert!(m.ideal_from_baseline);

        let ideal_on = |date| {
            m.points
                .iter()
                .find(|p| p.date == date)
                .map(|p| p.ideal_remaining)
                .unwrap_or(-1)
        };
        assert_eq!(ideal_on(at(2026, 1, 8).date()), 960, "nothing due yet");
        assert_eq!(ideal_on(at(2026, 1, 9).date()), 480, "one baselined finish");
        assert_eq!(ideal_on(at(2026, 1, 16).date()), 0, "and then the other");
    }

    #[test]
    fn the_ideal_line_ends_where_the_baseline_did_not_where_the_plan_now_does() {
        // Stretching it to the slipped finish understates the slip.
        let mut project = plan(&[(480, 0)]);
        project.tasks[0].scheduled.finish = at(2026, 3, 1);
        project.tasks[0].baseline = Some(Baseline {
            start: at(2026, 1, 5),
            finish: at(2026, 1, 9),
            duration_minutes: 2400,
            work_minutes: 480,
            cost: 0.0,
        });
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        let after = m
            .points
            .iter()
            .find(|p| p.date == at(2026, 1, 10).date())
            .map(|p| p.ideal_remaining);
        assert_eq!(after, Some(0), "the baseline said it would all be done");
    }

    #[test]
    fn an_overdue_plan_reads_as_behind_rather_than_on_plan() {
        // Every point sits in the past, so looking for the first one at or
        // after today finds nothing and used to report no drift at all.
        let mut project = plan(&[(480, 0), (480, 0)]);
        project.status_date = Some(at(2026, 6, 1));
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(
            m.against_plan(),
            960,
            "three months late, with all the work still to do"
        );
    }

    #[test]
    fn a_plan_running_to_schedule_reads_as_on_plan() {
        let mut project = plan(&[(480, 0)]);
        project.status_date = Some(at(2026, 1, 5));
        assert_eq!(metrics(&project, DEFAULT_ITERATION_DAYS).against_plan(), 0);
    }

    #[test]
    fn iterations_come_from_the_sprints_the_plan_declares() {
        // The plan already carries the cadence; slicing fourteen day windows
        // through it throws that away and cuts across the sprints.
        let project = sprints();
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert!(m.iterations_declared);
        assert_eq!(m.iterations.len(), 2);
        assert_eq!(m.iterations[0].name, "Sprint 1");
        assert_eq!(m.iterations[0].start, at(2026, 1, 5).date());
        assert_eq!(m.iterations[0].end, at(2026, 1, 18).date());
        assert_eq!(m.iterations[0].planned_tasks, 2, "what sits under it");
        assert_eq!(m.iterations[1].planned, 960);
    }

    #[test]
    fn our_own_agile_template_reports_its_three_sprints() {
        // The template declares them as summary rows, and fourteen day windows
        // sliced from the plan's start cut straight through them.
        let spec = crate::templates::by_id("agile").expect("the template ships with the app");
        let mut project = crate::templates::build(spec, at(2026, 1, 5));
        crate::schedule::schedule(&mut project).ok();

        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert!(m.iterations_declared);
        let names: Vec<&str> = m.iterations.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Sprint 1", "Sprint 2", "Sprint 3"]);
        assert!(m.iterations.iter().all(|i| i.planned_tasks == 4));
    }

    #[test]
    fn a_plan_that_declares_no_sprints_falls_back_to_fixed_windows() {
        let project = plan(&[(480, 0), (480, 0), (480, 0)]);
        let m = metrics(&project, 7);
        assert!(!m.iterations_declared);
        assert_eq!(m.iterations[0].name, "Iteration 1");
        assert_eq!(m.iterations[0].length_days(), 7);
    }

    #[test]
    fn a_task_belongs_to_the_iteration_it_finishes_in() {
        // With no record of when anything was actually finished, the only date
        // to hand is the scheduled one, and that convention is what makes the
        // velocities add up to the total.
        let project = plan(&[(480, 100), (480, 100), (480, 100)]);
        let all = iterations(&project, 7);
        let counted: usize = all.iter().map(|i| i.planned_tasks).sum();
        assert_eq!(counted, 3, "every task lands in exactly one iteration");
        let work: i64 = all.iter().map(|i| i.planned).sum();
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
    fn a_task_is_credited_to_the_iteration_it_really_finished_in() {
        // Scheduled to finish in the second window, actually finished in the
        // first, and velocity is a record of what was delivered when.
        let mut project = plan(&[(480, 100)]);
        project.tasks[0].scheduled.start = at(2026, 1, 19);
        project.tasks[0].scheduled.finish = at(2026, 1, 23);
        project.tasks[0].actual_finish = Some(at(2026, 1, 8));
        project.status_date = Some(at(2026, 3, 1));

        let all = iterations(&project, 7);
        assert_eq!(all[0].completed, 480, "the week it was finished in");
        assert!(all.iter().skip(1).all(|i| i.completed == 0));
    }

    #[test]
    fn velocity_counts_finished_tasks_only() {
        // Scrum counts what was delivered. Partial credit is "the fraction of
        // the work reported done" wearing velocity's name.
        let mut project = plan(&[(480, 50), (480, 100)]);
        project.status_date = Some(at(2026, 3, 1));
        let m = metrics(&project, 7);
        let delivered: i64 = m.iterations.iter().map(|i| i.velocity()).sum();
        assert_eq!(delivered, 480, "the half done task delivered nothing");
        assert_eq!(m.incomplete, 480, "and is still outstanding in full");
    }

    #[test]
    fn an_empty_iteration_does_not_drag_the_average_down() {
        // Nothing was scheduled to finish in it, so it says nothing about how
        // fast the team goes.
        let mut project = plan(&[(480, 100), (480, 0)]);
        project.tasks[0].scheduled.finish = at(2026, 1, 9);
        // Weeks of nothing sit between the two, and then the plan runs on.
        project.tasks[1].scheduled.start = at(2026, 3, 2);
        project.tasks[1].scheduled.finish = at(2026, 3, 6);
        project.status_date = Some(at(2026, 3, 2));

        let m = metrics(&project, 7);
        let empty = m.iterations.iter().filter(|i| i.planned == 0).count();
        assert!(empty > 0, "there are empty windows to ignore");
        assert_eq!(m.average_velocity, 480, "the one that had work in it");
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
        assert!(m.average_velocity > 0);
    }

    #[test]
    fn nothing_completed_means_no_projection_rather_than_a_wrong_one() {
        let project = plan(&[(480, 0), (480, 0)]);
        let m = metrics(&project, 7);
        assert_eq!(m.average_velocity, 0);
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
        assert_eq!(m.remaining(), 0);
        assert!(m.projected_finish.is_none(), "there is nothing left to forecast");
    }

    #[test]
    fn an_empty_plan_produces_empty_metrics_rather_than_dividing_by_zero() {
        let mut project = plan(&[]);
        project.tasks.clear();
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert_eq!(m.total, 0);
        assert_eq!(m.percent_complete(), 0.0);
        assert!(m.projected_finish.is_none());
        assert_eq!(m.against_plan(), 0);
    }

    #[test]
    fn an_iteration_length_of_zero_is_treated_as_one_day() {
        // Rather than looping forever on a zero width window.
        let project = plan(&[(480, 0)]);
        let all = iterations(&project, 0);
        assert!(!all.is_empty());
        assert_eq!(all[0].start, all[0].end);
    }

    #[test]
    fn a_plan_with_nonsense_dates_does_not_draw_a_point_a_day_forever() {
        let mut project = plan(&[(480, 0)]);
        project.tasks[0].scheduled.finish = at(3000, 1, 1);
        let m = metrics(&project, DEFAULT_ITERATION_DAYS);
        assert!(m.points.len() <= MAX_POINTS);
    }
}
