//! Actuals, timephasing and earned value.
//!
//! Everything else in this crate answers "what is the plan?". This answers
//! "what has it cost so far, and what is that worth?", which is the question a
//! plan is actually kept for once work has started.
//!
//! The keystone is timephasing: spreading a single number, a cost or an amount
//! of work, across the working calendar between two instants. Nothing else here
//! spreads anything, so every curve in the product, cash flow, usage views and
//! the earned value primitives below, comes out of `spread`.
//!
//! Two things are worth stating plainly, because they change how the figures
//! should be read.
//!
//! The first is that a quantity is spread **evenly across working time**. A
//! task's cost accrues at a constant rate per working minute, so a short
//! Friday takes a smaller share than a full Monday and a weekend takes none.
//! Microsoft can do better than this because it timephases every assignment
//! separately and lets a resource's contour vary; this model books a resource
//! at flat units for the whole task, so an even spread is not an approximation
//! of what the model says, it is exactly what the model says.
//!
//! The second is about whose calendar the spreading is done against. The
//! scheduler now works each task to the intersection of its own calendar and
//! its resources' (see `effective`), but everything here is still spread across
//! the **project** calendar, for everyone. That is a known gap rather than a
//! decision: a cost curve for a task somebody is away in the middle of will put
//! money on days they were not there, even though the task's *dates* now step
//! over those days correctly. Closing it means threading `EffectiveCalendars`
//! through `spread`, and it is the one place left that still holds the old
//! assumption.

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::calendar::WorkCalendar;
use crate::model::{Project, ResourceId, ResourceKind};

/// Ceiling on period-by-period iteration, mirroring the calendar's own guard so
/// a plan with runaway dates cannot hang the caller.
const MAX_PERIODS: usize = 4000;

/// What to tell a planner who asks for earned value on a plan that has none.
///
/// Kept here rather than in the interface because the reason is a property of
/// the calculation: without a baseline there is no budget, so PV, EV and every
/// ratio built on them are undefined rather than zero.
pub const NO_BASELINE_MESSAGE: &str =
    "Earned value needs a baseline. Set one from the Project tab, then the cost figures have something to be measured against.";

// ---- periods ------------------------------------------------------------

/// The width of one bucket on a timephased curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    Day,
    Week,
    Month,
    Quarter,
}

impl Period {
    pub const ALL: [Period; 4] = [Period::Day, Period::Week, Period::Month, Period::Quarter];

    pub fn label(self) -> &'static str {
        match self {
            Period::Day => "Days",
            Period::Week => "Weeks",
            Period::Month => "Months",
            Period::Quarter => "Quarters",
        }
    }

    /// The first day of the period `date` falls in.
    ///
    /// Weeks start on Monday, matching the calendar's own week, so a curve
    /// bucketed by week lines up with the working pattern rather than cutting
    /// it in half.
    pub fn start_of(self, date: NaiveDate) -> NaiveDate {
        match self {
            Period::Day => date,
            Period::Week => date - Duration::days(date.weekday().num_days_from_monday() as i64),
            Period::Month => first_of_month(date.year(), date.month()),
            Period::Quarter => first_of_month(date.year(), (date.month() - 1) / 3 * 3 + 1),
        }
    }

    /// The first day of the period after the one starting at `start`.
    pub fn next(self, start: NaiveDate) -> NaiveDate {
        match self {
            Period::Day => start + Duration::days(1),
            Period::Week => start + Duration::days(7),
            Period::Month => add_months(start, 1),
            Period::Quarter => add_months(start, 3),
        }
    }
}

/// Falls back to the date it was given rather than panicking, which only
/// happens at the far end of chrono's range. The period loops are bounded
/// separately, so a `next` that fails to advance cannot spin.
fn first_of_month(year: i32, month: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(NaiveDate::MIN)
}

fn add_months(start: NaiveDate, months: u32) -> NaiveDate {
    start
        .checked_add_months(Months::new(months))
        .unwrap_or(start)
}

// ---- timephasing --------------------------------------------------------

/// One bucket of a timephased curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimephasedEntry {
    /// The first day of the period, whatever width the period is.
    pub start: NaiveDate,
    /// What falls in this period alone.
    pub value: f64,
    /// Everything up to and including this period. Earned value reads this far
    /// more often than it reads `value`, so both are carried.
    pub cumulative: f64,
}

/// A quantity spread across the calendar, one entry per period.
///
/// Entries are contiguous from the first period to the last, so a period with
/// no working time in it appears with a value of zero rather than being missed
/// out. A chart can therefore draw the entries straight across without having
/// to work out where the gaps are.
#[derive(Debug, Clone, PartialEq)]
pub struct Timephased {
    pub period: Period,
    pub entries: Vec<TimephasedEntry>,
}

impl Timephased {
    pub fn empty(period: Period) -> Self {
        Self {
            period,
            entries: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Everything on the curve.
    pub fn total(&self) -> f64 {
        self.entries.last().map(|e| e.cumulative).unwrap_or(0.0)
    }

    /// The first instant the curve covers, at midnight on its first period.
    pub fn first_instant(&self) -> Option<NaiveDateTime> {
        self.entries.first().map(|e| midnight(e.start))
    }

    /// The instant the curve stops covering, at midnight on the day after its
    /// last period ends. Half-open, matching `work_minutes_between`.
    pub fn last_instant(&self) -> Option<NaiveDateTime> {
        self.entries
            .last()
            .map(|e| midnight(self.period.next(e.start)))
    }

    /// Working minutes the curve spans.
    pub fn span_minutes(&self, calendar: &WorkCalendar) -> i64 {
        match (self.first_instant(), self.last_instant()) {
            (Some(from), Some(to)) => calendar.work_minutes_between(from, to),
            _ => 0,
        }
    }

    /// How much of the curve has accrued by `at`.
    ///
    /// A period the instant falls inside is prorated by working time, not by
    /// wall time, so a status date at Wednesday lunchtime reads half of
    /// Wednesday rather than a little over half of the week.
    pub fn cumulative_at(&self, calendar: &WorkCalendar, at: NaiveDateTime) -> f64 {
        let mut total = 0.0;
        for entry in &self.entries {
            let from = midnight(entry.start);
            if at <= from {
                break;
            }
            let to = midnight(self.period.next(entry.start));
            if at >= to {
                total += entry.value;
                continue;
            }
            let span = calendar.work_minutes_between(from, to);
            total += if span > 0 {
                entry.value * calendar.work_minutes_between(from, at) as f64 / span as f64
            } else {
                // No working time to prorate against, so whatever sits in this
                // period sits at its start. A milestone lands here.
                entry.value
            };
            break;
        }
        total
    }
}

fn midnight(date: NaiveDate) -> NaiveDateTime {
    date.and_time(NaiveTime::MIN)
}

/// Spread `quantity` evenly across the working time in `[from, to)`.
///
/// A span with no working time in it, which is what a milestone is, puts the
/// whole quantity in the single period it sits in. That matters: a milestone
/// can carry a fixed cost, and dropping it would quietly lose money from every
/// cash flow the product draws.
pub fn spread(
    calendar: &WorkCalendar,
    from: NaiveDateTime,
    to: NaiveDateTime,
    quantity: f64,
    period: Period,
) -> Timephased {
    let mut curve = Timephased::empty(period);
    if to < from {
        return curve;
    }

    let total = calendar.work_minutes_between(from, to);
    let first = period.start_of(from.date());
    // The last period holding working time, not the period `to` happens to land
    // in: a task finishing at 08:00 on Wednesday did no work on Wednesday, and
    // a trailing empty period would stretch the duration axis earned value
    // reads along.
    let last = period
        .start_of(calendar.prev_working_instant(to).date())
        .max(first);

    let mut start = first;
    let mut cumulative = 0.0;
    for _ in 0..MAX_PERIODS {
        if start > last {
            break;
        }
        let next = period.next(start);
        let lo = midnight(start).max(from);
        let hi = midnight(next).min(to);
        let value = if total > 0 {
            if hi > lo {
                quantity * calendar.work_minutes_between(lo, hi) as f64 / total as f64
            } else {
                0.0
            }
        } else if start == first {
            quantity
        } else {
            0.0
        };

        cumulative += value;
        curve.entries.push(TimephasedEntry {
            start,
            value,
            cumulative,
        });
        if next <= start {
            break;
        }
        start = next;
    }
    curve
}

/// Add curves of the same period together, period by period.
///
/// The result is contiguous across the whole range the inputs cover, so a
/// month in which nothing happens still appears with a zero.
pub fn combine(period: Period, curves: &[Timephased]) -> Timephased {
    let mut buckets: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for curve in curves {
        for entry in &curve.entries {
            *buckets.entry(period.start_of(entry.start)).or_insert(0.0) += entry.value;
        }
    }

    let mut combined = Timephased::empty(period);
    let (Some(&first), Some(&last)) = (
        buckets.keys().next(),
        buckets.keys().next_back(),
    ) else {
        return combined;
    };

    let mut start = first;
    let mut cumulative = 0.0;
    for _ in 0..MAX_PERIODS {
        if start > last {
            break;
        }
        let value = buckets.get(&start).copied().unwrap_or(0.0);
        cumulative += value;
        combined.entries.push(TimephasedEntry {
            start,
            value,
            cumulative,
        });
        let next = period.next(start);
        if next <= start {
            break;
        }
        start = next;
    }
    combined
}

// ---- the curves a plan can draw ----------------------------------------

/// Baseline cost spread across the baseline's own dates.
///
/// Empty when the task has no baseline, which is the honest answer: there is no
/// budget to spread, and a flat zero would read as one that happened to be nil.
pub fn baseline_cost_curve(project: &Project, index: usize, period: Period) -> Timephased {
    let Some(baseline) = project.tasks.get(index).and_then(|task| task.baseline) else {
        return Timephased::empty(period);
    };
    spread(
        &project.calendar,
        baseline.start,
        baseline.finish,
        baseline.cost,
        period,
    )
}

/// Actual cost incurred, spread across the actual dates so far.
///
/// Empty until the task has started, since nothing has been spent on it yet.
pub fn actual_cost_curve(project: &Project, index: usize, period: Period) -> Timephased {
    let Some((from, to)) = actual_window(project, index) else {
        return Timephased::empty(period);
    };
    let cost = project
        .tasks
        .get(index)
        .map(|task| task.reported_actual_cost())
        .unwrap_or(0.0);
    spread(&project.calendar, from, to, cost, period)
}

/// Cost as the plan currently stands, spread across the scheduled dates. This
/// is the cash flow line, the one a finance team asks for.
pub fn scheduled_cost_curve(project: &Project, index: usize, period: Period) -> Timephased {
    let Some(task) = project.tasks.get(index) else {
        return Timephased::empty(period);
    };
    spread(
        &project.calendar,
        task.scheduled.start,
        task.scheduled.finish,
        task.scheduled.cost,
        period,
    )
}

/// Scheduled work spread across the scheduled dates, in minutes. This is what
/// a Task Usage view draws.
pub fn work_curve(project: &Project, index: usize, period: Period) -> Timephased {
    let Some(task) = project.tasks.get(index) else {
        return Timephased::empty(period);
    };
    spread(
        &project.calendar,
        task.scheduled.start,
        task.scheduled.finish,
        task.scheduled.work_minutes as f64,
        period,
    )
}

/// One resource's booked work across the plan, in minutes, which is what a
/// Resource Usage view draws.
///
/// Only work resources contribute: material and cost resources are bought, not
/// staffed, so they carry money rather than hours.
pub fn resource_work_curve(project: &Project, resource: ResourceId, period: Period) -> Timephased {
    let is_work_resource = project
        .resource(resource)
        .is_some_and(|r| r.kind == ResourceKind::Work);
    if !is_work_resource {
        return Timephased::empty(period);
    }

    let curves: Vec<Timephased> = leaves(project)
        .into_iter()
        .filter_map(|index| {
            let task = &project.tasks[index];
            let units: f64 = task
                .assignments
                .iter()
                .filter(|a| a.resource == resource)
                .map(|a| a.units)
                .sum();
            if units <= 0.0 {
                return None;
            }
            Some(spread(
                &project.calendar,
                task.scheduled.start,
                task.scheduled.finish,
                task.duration_minutes as f64 * units,
                period,
            ))
        })
        .collect();
    combine(period, &curves)
}

pub fn project_baseline_cost_curve(project: &Project, period: Period) -> Timephased {
    combine(period, &leaf_curves(project, period, baseline_cost_curve))
}

pub fn project_actual_cost_curve(project: &Project, period: Period) -> Timephased {
    combine(period, &leaf_curves(project, period, actual_cost_curve))
}

pub fn project_scheduled_cost_curve(project: &Project, period: Period) -> Timephased {
    combine(period, &leaf_curves(project, period, scheduled_cost_curve))
}

fn leaf_curves(
    project: &Project,
    period: Period,
    of: fn(&Project, usize, Period) -> Timephased,
) -> Vec<Timephased> {
    leaves(project)
        .into_iter()
        .map(|index| of(project, index, period))
        .collect()
}

/// Summary rows are skipped everywhere in this module: they carry a rolled-up
/// copy of their children's cost, and counting both would double every figure.
fn leaves(project: &Project) -> Vec<usize> {
    (0..project.tasks.len())
        .filter(|&index| !project.is_summary(index))
        .collect()
}

/// Where actual cost is spread: from when work really started to where it has
/// got to.
///
/// A plan that tracks nothing but percent complete has no actual start typed
/// in, so the scheduled start stands in for one, and progress against the
/// scheduled duration says where the work has reached.
fn actual_window(project: &Project, index: usize) -> Option<(NaiveDateTime, NaiveDateTime)> {
    let task = project.tasks.get(index)?;
    if !task.has_started() {
        return None;
    }
    let from = task.actual_start.unwrap_or(task.scheduled.start);
    let to = task
        .actual_finish
        .unwrap_or_else(|| project.calendar.add_minutes(from, task.completed_minutes()));
    Some((from, to.max(from)))
}

// ---- earned value -------------------------------------------------------

/// Which measure of progress feeds BCWP.
///
/// This affects the earned value only. PV comes from the baseline cost and the
/// status date, so nothing a planner does here can move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EarnedValueMethod {
    /// Progress as the schedule reports it, which is elapsed duration.
    #[default]
    PercentComplete,
    /// Progress as someone judged it, independent of how long it has taken.
    /// The honest choice for work whose elapsed time says nothing about how
    /// much of it is done.
    PhysicalPercentComplete,
}

impl EarnedValueMethod {
    pub const ALL: [EarnedValueMethod; 2] = [
        EarnedValueMethod::PercentComplete,
        EarnedValueMethod::PhysicalPercentComplete,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EarnedValueMethod::PercentComplete => "% Complete",
            EarnedValueMethod::PhysicalPercentComplete => "Physical % Complete",
        }
    }
}

/// The three earned value primitives and the budget they are measured against,
/// all currency, all as at `status_date`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EarnedValue {
    /// The moment everything here was measured at.
    pub status_date: NaiveDateTime,
    /// Budget at completion: the whole baseline cost.
    pub bac: f64,
    /// BCWS: baseline cost accumulated up to the status date.
    pub planned_value: f64,
    /// BCWP: the budgeted cost of the work actually done.
    pub earned_value: f64,
    /// ACWP: actual cost incurred up to the status date.
    pub actual_cost: f64,
}

impl EarnedValue {
    /// Cost variance. Positive is under budget.
    pub fn cost_variance(&self) -> f64 {
        self.earned_value - self.actual_cost
    }

    /// Schedule variance, in money rather than in time. Positive is ahead.
    pub fn schedule_variance(&self) -> f64 {
        self.earned_value - self.planned_value
    }

    pub fn cost_variance_percent(&self) -> f64 {
        ratio(self.cost_variance(), self.earned_value) * 100.0
    }

    pub fn schedule_variance_percent(&self) -> f64 {
        ratio(self.schedule_variance(), self.planned_value) * 100.0
    }

    /// Cost performance index. Above 1.0 is value for money.
    pub fn cpi(&self) -> f64 {
        ratio(self.earned_value, self.actual_cost)
    }

    /// Schedule performance index. Above 1.0 is ahead of the baseline.
    pub fn spi(&self) -> f64 {
        ratio(self.earned_value, self.planned_value)
    }

    /// Estimate at completion.
    ///
    /// Microsoft uses the CPI-based form, `AC + (BAC - EV) / CPI`, which
    /// reduces to `BAC / CPI`, and not PMBOK's more common `AC + (BAC - EV)`.
    /// This is a Project clone, so it matches Project.
    ///
    /// Before anything has been spent there is no performance to extrapolate
    /// from, and dividing by a zero CPI would report an estimate of nothing for
    /// a project that plainly has a budget. The budget itself is the only
    /// defensible answer at that point, so that is what comes back.
    pub fn eac(&self) -> f64 {
        if self.cpi() <= 0.0 {
            return self.bac;
        }
        self.bac / self.cpi()
    }

    /// Variance at completion. Positive means it is forecast to come in under.
    pub fn vac(&self) -> f64 {
        self.bac - self.eac()
    }

    /// To complete performance index: the efficiency the remaining work has to
    /// run at for the budget to hold.
    pub fn tcpi(&self) -> f64 {
        ratio(self.bac - self.earned_value, self.bac - self.actual_cost)
    }
}

/// Every ratio here divides by something that is legitimately zero at the start
/// of a project: PV is nil before the baseline has begun, AC is nil before
/// anyone has spent anything, and EV is nil before any work is done.
///
/// Microsoft does not document what it does about that. What it is reported to
/// show is a plain zero, and that is what this returns. The alternative,
/// leaving the cell blank or handing back an infinity, is worse in the same
/// place it matters most: a planner opening a report on day one would see an
/// error where the answer is simply "nothing has happened yet".
fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator.abs() < f64::EPSILON {
        0.0
    } else {
        numerator / denominator
    }
}

/// The moment earned value is measured at.
///
/// Microsoft falls back to the current date when no status date has been set,
/// so this does too. It means an unset status date gives a figure that changes
/// by itself overnight, which is exactly why Project nags for one.
pub fn status_date(project: &Project) -> NaiveDateTime {
    project
        .status_date
        .unwrap_or_else(|| chrono::Local::now().naive_local())
}

/// Whether the plan can be measured at all.
pub fn is_available(project: &Project) -> bool {
    project.has_baseline()
}

/// Earned value for the whole plan, or `None` when no baseline has been saved.
///
/// `None` rather than a struct full of zeroes: without a budget every figure
/// here is undefined, and zeroes look like real answers that happen to be nil.
pub fn project_earned_value(project: &Project) -> Option<EarnedValue> {
    if !is_available(project) {
        return None;
    }
    Some(accumulate(project, &leaves(project), status_date(project)))
}

/// Earned value for one row.
///
/// A summary row is the rolled-up total of its leaves and then its ratios are
/// re-derived from those totals. Averaging the children's ratios would be
/// wrong in the ordinary case rather than the awkward one: a cheap task running
/// at CPI 0.5 and an expensive one at 1.0 do not average to 0.75 of anything a
/// budget holder cares about. The same reasoning is why a summary's own
/// baseline, which the scheduler wrote as a copy of the rollup, is ignored
/// here in favour of reading the leaves directly.
pub fn task_earned_value(project: &Project, index: usize) -> Option<EarnedValue> {
    project.tasks.get(index)?;
    let rows = project.leaf_indices(index);
    if !rows
        .iter()
        .any(|&row| project.tasks.get(row).is_some_and(|t| t.baseline.is_some()))
    {
        return None;
    }
    Some(accumulate(project, &rows, status_date(project)))
}

fn accumulate(project: &Project, rows: &[usize], status: NaiveDateTime) -> EarnedValue {
    let mut totals = EarnedValue {
        status_date: status,
        bac: 0.0,
        planned_value: 0.0,
        earned_value: 0.0,
        actual_cost: 0.0,
    };

    for &index in rows {
        let Some(task) = project.tasks.get(index) else {
            continue;
        };

        // Money spent is money spent whether or not anyone saved a baseline for
        // the row it was spent on, so ACWP is collected before the baseline
        // check rather than after it.
        totals.actual_cost += actual_cost_curve(project, index, Period::Day)
            .cumulative_at(&project.calendar, status);

        let Some(baseline) = task.baseline else {
            continue;
        };
        totals.bac += baseline.cost;
        totals.planned_value += baseline.cost
            * elapsed_fraction(&project.calendar, baseline.start, baseline.finish, status);

        let curve = baseline_cost_curve(project, index, Period::Day);
        totals.earned_value += earned_at_percent(
            &curve,
            &project.calendar,
            task.earned_percent() as f64,
        );
    }

    totals
}

/// How much of the working time in `[from, to)` has passed by `at`, from 0.0
/// to 1.0. A span with no working time in it is all or nothing.
fn elapsed_fraction(
    calendar: &WorkCalendar,
    from: NaiveDateTime,
    to: NaiveDateTime,
    at: NaiveDateTime,
) -> f64 {
    if at <= from {
        return 0.0;
    }
    if at >= to {
        return 1.0;
    }
    let total = calendar.work_minutes_between(from, to);
    if total <= 0 {
        return 1.0;
    }
    (calendar.work_minutes_between(from, at) as f64 / total as f64).clamp(0.0, 1.0)
}

/// Microsoft's BCWP, and the one place a clone usually gets this wrong.
///
/// BCWP is **not** `percent complete * BAC`. Percent complete is mapped onto
/// the baseline **duration** axis, and the cumulative baseline cost curve is
/// read at that point. Microsoft's own worked example: a baseline duration of
/// 4 days and a baseline cost of 60 timephased as 10, 10, 20, 20. At 50 percent
/// complete BCWP is 20, "because 50 percent of the baseline duration consists
/// of the first 2 days, which have a baseline cost of 10 each". At 75 percent
/// it is 40.
///
/// Only where the baseline is spread evenly does this collapse to
/// `percent * BAC`, which is why the mistake survives so long: on a single task
/// booked at flat units the two agree exactly, and they part company the moment
/// a summary row, a resource contour or an uneven calendar is involved.
///
/// The lookup walks `percent` of the curve's working minutes forward from its
/// start and reads the cumulative value there, prorating inside whichever
/// period it lands in, so the answer is continuous rather than stepping a whole
/// period at a time.
pub fn earned_at_percent(curve: &Timephased, calendar: &WorkCalendar, percent: f64) -> f64 {
    let Some(from) = curve.first_instant() else {
        return 0.0;
    };
    let percent = percent.clamp(0.0, 100.0);
    let span = curve.span_minutes(calendar);
    if span <= 0 {
        // A milestone has no duration axis to walk along, so it is worth its
        // budget once it is done and nothing before that.
        return if percent >= 100.0 { curve.total() } else { 0.0 };
    }
    let along = (span as f64 * percent / 100.0).round() as i64;
    curve.cumulative_at(calendar, calendar.add_minutes(from, along))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MINUTES_PER_DAY;
    use crate::model::{Assignment, Baseline, Task};
    use crate::schedule::schedule;

    fn at(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|date| date.and_hms_opt(8, 0, 0))
            .expect("a real date")
    }

    /// The end of a working day, which is where a task's finish sits.
    fn close(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|date| date.and_hms_opt(17, 0, 0))
            .expect("a real date")
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("a real date")
    }

    /// Monday 5 January 2026 is the standard start for these plans.
    fn plan(rows: &[(u16, &str, i64, u8)]) -> Project {
        let mut project = Project::blank(at(2026, 1, 5));
        project.status_date = Some(at(2026, 1, 5));
        for (level, name, minutes, percent) in rows {
            let id = project.allocate_task_id();
            let mut task = Task::new(id, *name, *minutes);
            task.outline_level = *level;
            task.percent_complete = *percent;
            project.tasks.push(task);
        }
        project
    }

    /// The curve from Microsoft's own worked example: 4 baseline days costing
    /// 60 in total, timephased 10, 10, 20, 20 across Monday to Thursday.
    fn microsofts_curve() -> Timephased {
        let mut curve = Timephased::empty(Period::Day);
        let mut cumulative = 0.0;
        for (offset, value) in [10.0, 10.0, 20.0, 20.0].into_iter().enumerate() {
            cumulative += value;
            curve.entries.push(TimephasedEntry {
                start: day(2026, 1, 5) + Duration::days(offset as i64),
                value,
                cumulative,
            });
        }
        curve
    }

    // ---- timephasing ----------------------------------------------------

    #[test]
    fn a_cost_spreads_across_working_days_and_skips_the_weekend() {
        let calendar = WorkCalendar::standard();
        // Thursday to the following Tuesday: four working days, one weekend.
        let curve = spread(
            &calendar,
            at(2026, 1, 8),
            close(2026, 1, 13),
            400.0,
            Period::Day,
        );

        let values: Vec<f64> = curve.entries.iter().map(|e| e.value).collect();
        assert_eq!(curve.entries.len(), 6, "the weekend still gets a row");
        assert!((values[0] - 100.0).abs() < 1e-6, "Thursday");
        assert!((values[1] - 100.0).abs() < 1e-6, "Friday");
        assert!(values[2].abs() < 1e-9, "Saturday earns nothing");
        assert!(values[3].abs() < 1e-9, "Sunday earns nothing");
        assert!((values[4] - 100.0).abs() < 1e-6, "Monday");
        assert!((values[5] - 100.0).abs() < 1e-6, "Tuesday");
        assert!((curve.total() - 400.0).abs() < 1e-6);
    }

    #[test]
    fn a_milestone_puts_its_whole_cost_in_the_one_period_it_sits_in() {
        // Otherwise a fixed cost on a milestone vanishes from every cash flow.
        let calendar = WorkCalendar::standard();
        let curve = spread(&calendar, at(2026, 1, 7), at(2026, 1, 7), 500.0, Period::Day);
        assert_eq!(curve.entries.len(), 1);
        assert!((curve.total() - 500.0).abs() < 1e-6);
        assert_eq!(curve.entries[0].start, day(2026, 1, 7));
    }

    #[test]
    fn wider_periods_bucket_the_same_total() {
        let calendar = WorkCalendar::standard();
        let from = at(2026, 1, 5);
        let to = at(2026, 3, 31);
        let total = 1000.0;
        for period in Period::ALL {
            let curve = spread(&calendar, from, to, total, period);
            assert!(
                (curve.total() - total).abs() < 1e-6,
                "{} lost money",
                period.label()
            );
            assert!(!curve.is_empty());
        }
        assert_eq!(
            spread(&calendar, from, to, total, Period::Quarter).entries.len(),
            1,
            "January to March is one quarter"
        );
    }

    #[test]
    fn weeks_start_on_monday_and_months_on_the_first() {
        assert_eq!(Period::Week.start_of(day(2026, 1, 8)), day(2026, 1, 5));
        assert_eq!(Period::Week.start_of(day(2026, 1, 5)), day(2026, 1, 5));
        assert_eq!(Period::Month.start_of(day(2026, 2, 17)), day(2026, 2, 1));
        assert_eq!(Period::Quarter.start_of(day(2026, 5, 9)), day(2026, 4, 1));
        assert_eq!(Period::Quarter.start_of(day(2026, 12, 31)), day(2026, 10, 1));
        assert_eq!(Period::Month.next(day(2026, 12, 1)), day(2027, 1, 1));
    }

    #[test]
    fn a_curve_reads_back_partway_through_a_period() {
        let calendar = WorkCalendar::standard();
        let curve = spread(&calendar, at(2026, 1, 5), at(2026, 1, 7), 480.0, Period::Day);
        // Midday on the second day: one full day plus half of the next.
        let midday = day(2026, 1, 6)
            .and_hms_opt(12, 0, 0)
            .expect("a real time");
        assert!((curve.cumulative_at(&calendar, midday) - 360.0).abs() < 1e-6);
    }

    #[test]
    fn curves_of_the_same_period_add_up() {
        let calendar = WorkCalendar::standard();
        let a = spread(&calendar, at(2026, 1, 5), at(2026, 1, 7), 100.0, Period::Day);
        let b = spread(&calendar, at(2026, 1, 6), at(2026, 1, 8), 100.0, Period::Day);
        let both = combine(Period::Day, &[a, b]);
        assert_eq!(both.entries.len(), 3, "Monday through Wednesday");
        assert!((both.total() - 200.0).abs() < 1e-6);
        assert!((both.entries[1].value - 100.0).abs() < 1e-6, "Tuesday holds both");
    }

    // ---- the inverse lookup, which is the whole point -------------------

    #[test]
    fn microsofts_worked_example() {
        // Verbatim from Microsoft's own documentation. A baseline duration of
        // 4 days and a baseline cost of 60 timephased as 10, 10, 20, 20. At 50
        // percent complete BCWP is 20, "because 50 percent of the baseline
        // duration consists of the first 2 days, which have a baseline cost of
        // 10 each". At 75 percent it is 40.
        let calendar = WorkCalendar::standard();
        let curve = microsofts_curve();
        assert!((curve.total() - 60.0).abs() < 1e-6, "the budget is 60");

        assert!(
            (earned_at_percent(&curve, &calendar, 50.0) - 20.0).abs() < 1e-6,
            "50 percent of a 4 day baseline is the first 2 days, worth 10 each"
        );
        assert!(
            (earned_at_percent(&curve, &calendar, 75.0) - 40.0).abs() < 1e-6,
            "75 percent is the first 3 days"
        );
        assert!(earned_at_percent(&curve, &calendar, 0.0).abs() < 1e-9);
        assert!((earned_at_percent(&curve, &calendar, 100.0) - 60.0).abs() < 1e-6);
    }

    #[test]
    fn the_naive_formula_would_have_got_that_wrong() {
        // The trap, stated as a test so nobody quietly "simplifies" the lookup
        // back into a multiplication.
        let calendar = WorkCalendar::standard();
        let curve = microsofts_curve();
        let naive = 0.5 * curve.total();
        assert!((naive - 30.0).abs() < 1e-6);
        assert!(
            (earned_at_percent(&curve, &calendar, 50.0) - naive).abs() > 1.0,
            "percent times BAC gives 30, the baseline curve gives 20"
        );
    }

    #[test]
    fn an_evenly_spread_baseline_does_collapse_to_percent_times_bac() {
        // Which is why the mistake survives: on a flat curve the two agree.
        let calendar = WorkCalendar::standard();
        let curve = spread(
            &calendar,
            at(2026, 1, 5),
            close(2026, 1, 9),
            1000.0,
            Period::Day,
        );
        for percent in [0.0, 20.0, 50.0, 60.0, 100.0] {
            let expected = 1000.0 * percent / 100.0;
            assert!(
                (earned_at_percent(&curve, &calendar, percent) - expected).abs() < 1e-6,
                "flat curve at {percent} percent"
            );
        }
    }

    // ---- earned value on a plan -----------------------------------------

    /// A summary with two children of equal duration but very unequal cost,
    /// which is the front-loaded baseline Microsoft's example describes,
    /// arrived at the way this model can actually arrive at one.
    fn front_loaded_plan() -> Project {
        let mut project = plan(&[
            (0, "Phase", 0, 0),
            (1, "Expensive", 2 * MINUTES_PER_DAY, 100),
            (1, "Cheap", 2 * MINUTES_PER_DAY, 0),
        ]);
        let dear = project.add_resource("Dear");
        let cheap = project.add_resource("Cheap");
        if let Some(r) = project.resources.iter_mut().find(|r| r.id == dear) {
            r.standard_rate = 2.5;
        }
        if let Some(r) = project.resources.iter_mut().find(|r| r.id == cheap) {
            r.standard_rate = 1.25;
        }
        project.tasks[1].assignments = vec![Assignment {
            resource: dear,
            units: 1.0,
        }];
        project.tasks[2].assignments = vec![Assignment {
            resource: cheap,
            units: 1.0,
        }];
        schedule(&mut project).expect("a plan with no cycles schedules");
        project.set_baseline();
        project
    }

    #[test]
    fn a_front_loaded_baseline_earns_more_than_percent_times_bac() {
        let mut project = front_loaded_plan();
        // Status date past the end, so the schedule side is settled and only
        // the earned value maths is under test.
        project.status_date = Some(at(2026, 1, 30));

        let ev = task_earned_value(&project, 0).expect("the phase has a baseline");
        // Two days at 2.5 an hour is 40; two days at 1.25 is 20.
        assert!((ev.bac - 60.0).abs() < 1e-6, "budget at completion");
        assert_eq!(
            project.tasks[0].percent_complete, 50,
            "the summary is half done by duration"
        );
        assert!(
            (ev.earned_value - 40.0).abs() < 1e-6,
            "the expensive half is the half that is done"
        );
        assert!(
            (ev.earned_value - 0.5 * ev.bac).abs() > 1.0,
            "percent times BAC would have said 30"
        );
    }

    #[test]
    fn a_summary_re_derives_its_ratios_rather_than_averaging_its_children() {
        let mut project = front_loaded_plan();
        project.status_date = Some(at(2026, 1, 30));
        // The finished half came in at half its budget.
        project.tasks[1].actual_cost = 20.0;

        let phase = task_earned_value(&project, 0).expect("the phase has a baseline");
        let expensive = task_earned_value(&project, 1).expect("and so does the child");
        let cheap = task_earned_value(&project, 2).expect("and so does the other");

        assert!((phase.earned_value - (expensive.earned_value + cheap.earned_value)).abs() < 1e-6);
        assert!((phase.actual_cost - (expensive.actual_cost + cheap.actual_cost)).abs() < 1e-6);
        assert!((expensive.cpi() - 2.0).abs() < 1e-6, "40 earned for 20 spent");
        assert!(
            (phase.cpi() - 2.0).abs() < 1e-6,
            "the rollup is EV over AC, not the mean of 2.0 and the untouched child"
        );
        let averaged = (expensive.cpi() + cheap.cpi()) / 2.0;
        assert!(
            (phase.cpi() - averaged).abs() > 0.5,
            "averaging the children would have said {averaged}"
        );
    }

    #[test]
    fn planned_value_follows_the_status_date() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 0)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();

        // Monday to Thursday, 400 over four working days.
        project.status_date = Some(at(2026, 1, 5));
        let start = project_earned_value(&project).expect("baselined");
        assert!(start.planned_value.abs() < 1e-9, "nothing planned on day one");

        project.status_date = Some(at(2026, 1, 7));
        let midway = project_earned_value(&project).expect("baselined");
        assert!((midway.planned_value - 200.0).abs() < 1e-6, "two days in");

        project.status_date = Some(at(2026, 2, 1));
        let after = project_earned_value(&project).expect("baselined");
        assert!((after.planned_value - 400.0).abs() < 1e-6, "all of it");
        assert!((after.bac - 400.0).abs() < 1e-6);
    }

    #[test]
    fn a_plan_with_no_baseline_says_so_rather_than_returning_zeroes() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 50)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");

        assert!(!is_available(&project));
        assert!(project_earned_value(&project).is_none());
        assert!(task_earned_value(&project, 0).is_none());
        assert!(!NO_BASELINE_MESSAGE.is_empty());
    }

    #[test]
    fn ratios_show_zero_rather_than_dividing_by_zero() {
        // Day one of a plan: PV is nil because the baseline has not started,
        // EV is nil because nothing is done, AC is nil because nothing has been
        // spent. Every ratio below has a zero denominator.
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 0)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();
        project.status_date = Some(at(2026, 1, 5));

        let ev = project_earned_value(&project).expect("baselined");
        assert!(ev.planned_value.abs() < 1e-9);
        assert!(ev.earned_value.abs() < 1e-9);
        assert!(ev.actual_cost.abs() < 1e-9);

        assert_eq!(ev.cpi(), 0.0);
        assert_eq!(ev.spi(), 0.0);
        assert_eq!(ev.cost_variance_percent(), 0.0);
        assert_eq!(ev.schedule_variance_percent(), 0.0);
    }

    #[test]
    fn the_estimate_at_completion_falls_back_to_the_budget_before_anything_is_spent() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 0)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();
        project.status_date = Some(at(2026, 1, 5));

        let ev = project_earned_value(&project).expect("baselined");
        assert!((ev.eac() - 400.0).abs() < 1e-6, "the budget is the only estimate there is");
        assert!(ev.vac().abs() < 1e-9);
        assert!((ev.tcpi() - 1.0).abs() < 1e-6, "the whole budget for the whole job");
    }

    #[test]
    fn overspending_shows_up_in_cpi_and_the_forecast() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 50)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();
        project.status_date = Some(at(2026, 1, 30));
        project.tasks[0].actual_cost = 400.0;

        let ev = project_earned_value(&project).expect("baselined");
        assert!((ev.earned_value - 200.0).abs() < 1e-6, "half of an even 400");
        assert!((ev.actual_cost - 400.0).abs() < 1e-6);
        assert!((ev.cost_variance() + 200.0).abs() < 1e-6, "200 over");
        assert!((ev.cpi() - 0.5).abs() < 1e-6);
        assert!((ev.eac() - 800.0).abs() < 1e-6, "BAC over CPI");
        assert!((ev.vac() + 400.0).abs() < 1e-6);
        // The status date is past the baseline finish, so all 400 was planned
        // to have been earned by now and only half of it has been.
        assert!((ev.planned_value - 400.0).abs() < 1e-6);
        assert!((ev.schedule_variance() + 200.0).abs() < 1e-6);
        assert!((ev.spi() - 0.5).abs() < 1e-6);
        assert!(
            (ev.tcpi() - 0.0).abs() < 1e-6,
            "the budget is spent, so no efficiency finishes the job inside it"
        );
    }

    #[test]
    fn physical_percent_complete_moves_the_earned_value_and_leaves_the_plan_alone() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 90)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();
        project.status_date = Some(at(2026, 1, 7));

        let optimistic = project_earned_value(&project).expect("baselined");

        project.tasks[0].earned_value_method = EarnedValueMethod::PhysicalPercentComplete;
        project.tasks[0].physical_percent_complete = Some(30);
        let honest = project_earned_value(&project).expect("baselined");

        assert!((optimistic.earned_value - 360.0).abs() < 1e-6);
        assert!((honest.earned_value - 120.0).abs() < 1e-6);
        assert!(
            (optimistic.planned_value - honest.planned_value).abs() < 1e-9,
            "the method must not touch PV, which comes from the baseline and the status date"
        );
        assert_eq!(
            project.tasks[0].percent_complete, 90,
            "and it must not touch the schedule's own progress"
        );
    }

    #[test]
    fn physical_percent_falls_back_when_nobody_has_typed_one_in() {
        let mut task = Task::new(1, "Build", MINUTES_PER_DAY);
        task.percent_complete = 40;
        task.earned_value_method = EarnedValueMethod::PhysicalPercentComplete;
        assert_eq!(task.earned_percent(), 40);
        task.physical_percent_complete = Some(10);
        assert_eq!(task.earned_percent(), 10);
    }

    #[test]
    fn actual_cost_is_read_at_the_status_date_and_not_beyond_it() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 100)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();
        project.tasks[0].actual_cost = 400.0;

        project.status_date = Some(at(2026, 1, 7));
        let midway = project_earned_value(&project).expect("baselined");
        assert!(
            (midway.actual_cost - 200.0).abs() < 1e-6,
            "two of the four days have been paid for by Wednesday morning"
        );

        project.status_date = Some(at(2026, 2, 1));
        let after = project_earned_value(&project).expect("baselined");
        assert!((after.actual_cost - 400.0).abs() < 1e-6);
    }

    #[test]
    fn a_plan_that_tracks_only_percent_complete_still_reports_actuals() {
        // Nobody typed an actual cost, so the reported progress against the
        // scheduled cost has to stand in for one.
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 50)]);
        project.tasks[0].fixed_cost = 400.0;
        schedule(&mut project).expect("it schedules");
        project.set_baseline();
        project.status_date = Some(at(2026, 1, 30));

        let ev = project_earned_value(&project).expect("baselined");
        assert!((ev.actual_cost - 200.0).abs() < 1e-6);
        assert!((ev.cpi() - 1.0).abs() < 1e-6, "no actuals means no variance");
    }

    #[test]
    fn remaining_work_is_derived_until_somebody_says_otherwise() {
        let mut project = plan(&[(0, "Build", 4 * MINUTES_PER_DAY, 25)]);
        let resource = project.add_resource("Ana");
        project.tasks[0].assignments = vec![Assignment {
            resource,
            units: 1.0,
        }];
        schedule(&mut project).expect("it schedules");

        let task = &project.tasks[0];
        assert_eq!(task.scheduled.work_minutes, 4 * MINUTES_PER_DAY);
        assert_eq!(task.reported_actual_work_minutes(), MINUTES_PER_DAY);
        assert_eq!(task.remaining_work(), 3 * MINUTES_PER_DAY);

        project.tasks[0].remaining_work_minutes = 5 * MINUTES_PER_DAY;
        assert_eq!(
            project.tasks[0].remaining_work(),
            5 * MINUTES_PER_DAY,
            "a typed figure wins over the derivation"
        );
    }

    #[test]
    fn a_milestone_earns_its_budget_only_once_it_is_done() {
        let calendar = WorkCalendar::standard();
        let curve = spread(&calendar, at(2026, 1, 7), at(2026, 1, 7), 500.0, Period::Day);
        // The curve occupies a whole working day, so it does have an axis; the
        // all-or-nothing branch is for a marker sitting on a non-working day.
        let barren = spread(
            &calendar,
            at(2026, 1, 10),
            at(2026, 1, 10),
            500.0,
            Period::Day,
        );
        assert!(barren.span_minutes(&calendar) == 0, "Saturday has no working time");
        assert!(earned_at_percent(&barren, &calendar, 50.0).abs() < 1e-9);
        assert!((earned_at_percent(&barren, &calendar, 100.0) - 500.0).abs() < 1e-6);
        assert!((curve.total() - 500.0).abs() < 1e-6);
    }

    // ---- usage curves ---------------------------------------------------

    #[test]
    fn a_resource_curve_holds_the_hours_it_is_booked_for() {
        let mut project = plan(&[
            (0, "One", 2 * MINUTES_PER_DAY, 0),
            (0, "Two", 2 * MINUTES_PER_DAY, 0),
        ]);
        let ana = project.add_resource("Ana");
        for index in 0..2 {
            project.tasks[index].assignments = vec![Assignment {
                resource: ana,
                units: 0.5,
            }];
        }
        schedule(&mut project).expect("it schedules");

        let curve = resource_work_curve(&project, ana, Period::Day);
        assert!(
            (curve.total() - 2.0 * MINUTES_PER_DAY as f64).abs() < 1e-6,
            "half of four days is two"
        );
        assert!(resource_work_curve(&project, 999, Period::Day).is_empty());
    }

    #[test]
    fn the_project_cash_flow_is_the_sum_of_its_leaves_and_not_of_its_summaries() {
        let mut project = front_loaded_plan();
        project.status_date = Some(at(2026, 1, 30));
        let curve = project_scheduled_cost_curve(&project, Period::Day);
        assert!(
            (curve.total() - 60.0).abs() < 1e-6,
            "counting the summary too would have doubled it"
        );
        let baseline = project_baseline_cost_curve(&project, Period::Week);
        assert!((baseline.total() - 60.0).abs() < 1e-6);
    }

    // ---- persistence ----------------------------------------------------

    #[test]
    fn a_plan_saved_before_actuals_existed_still_opens() {
        // The whole point of the serde defaults: a file written by an earlier
        // build has none of these keys and must not fail to load.
        let project = plan(&[(0, "Build", MINUTES_PER_DAY, 0)]);
        let mut json: serde_json::Value =
            serde_json::to_value(&project).expect("a plan serialises");
        for task in json["tasks"]
            .as_array_mut()
            .expect("tasks are an array")
            .iter_mut()
        {
            let task = task.as_object_mut().expect("a task is an object");
            for key in [
                "actual_start",
                "actual_finish",
                "actual_work_minutes",
                "actual_cost",
                "remaining_work_minutes",
                "physical_percent_complete",
                "earned_value_method",
            ] {
                task.remove(key);
            }
        }

        let back: Project = serde_json::from_value(json).expect("an older plan still opens");
        let task = &back.tasks[0];
        assert!(task.actual_start.is_none());
        assert!(task.actual_finish.is_none());
        assert_eq!(task.actual_work_minutes, 0);
        assert_eq!(task.actual_cost, 0.0);
        assert_eq!(task.remaining_work_minutes, 0);
        assert!(task.physical_percent_complete.is_none());
        assert_eq!(task.earned_value_method, EarnedValueMethod::PercentComplete);
    }

    #[test]
    fn actuals_survive_a_round_trip() {
        let mut project = plan(&[(0, "Build", MINUTES_PER_DAY, 60)]);
        let task = &mut project.tasks[0];
        task.actual_start = Some(at(2026, 1, 5));
        task.actual_finish = Some(at(2026, 1, 6));
        task.actual_work_minutes = 300;
        task.actual_cost = 123.45;
        task.remaining_work_minutes = 180;
        task.physical_percent_complete = Some(40);
        task.earned_value_method = EarnedValueMethod::PhysicalPercentComplete;
        task.baseline = Some(Baseline {
            start: at(2026, 1, 5),
            finish: at(2026, 1, 6),
            duration_minutes: MINUTES_PER_DAY,
            work_minutes: MINUTES_PER_DAY,
            cost: 500.0,
        });

        let json = serde_json::to_string(&project).expect("a plan serialises");
        let back: Project = serde_json::from_str(&json).expect("and reads back");
        let task = &back.tasks[0];
        assert_eq!(task.actual_start, Some(at(2026, 1, 5)));
        assert_eq!(task.actual_finish, Some(at(2026, 1, 6)));
        assert_eq!(task.actual_work_minutes, 300);
        assert!((task.actual_cost - 123.45).abs() < 1e-9);
        assert_eq!(task.remaining_work_minutes, 180);
        assert_eq!(task.physical_percent_complete, Some(40));
        assert_eq!(
            task.earned_value_method,
            EarnedValueMethod::PhysicalPercentComplete
        );
        assert_eq!(task.earned_percent(), 40);
    }
}
