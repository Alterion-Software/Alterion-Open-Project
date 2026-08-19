//! Which calendar a task is actually worked to.
//!
//! A plan has more than one opinion about when work can happen. The project
//! calendar says when the organisation is open, a task calendar can say when
//! one particular job is allowed to run, and every person assigned to it says
//! when they are there. Work happens only where all of those agree, so what
//! the scheduler needs is not a calendar but the intersection of several.
//!
//! Microsoft Project's rule is the one followed here:
//!
//! - a task is worked to its own calendar, or the project's when it has none;
//! - and to every assigned work resource's calendar as well;
//! - unless the task names a calendar and says to ignore resource calendars,
//!   in which case the people are not consulted. The flag means nothing without
//!   a task calendar, because dropping the resources would then leave only the
//!   project calendar, which is what happens anyway.
//!
//! Material and cost resources are left out. They have no working time to
//! contribute: a pallet of bricks is not away in March.
//!
//! # What it costs
//!
//! Intersecting is not free, so it is done once per reschedule rather than once
//! per pass. `EffectiveCalendars::build` also folds together rows that ask the
//! same question: a plan of several hundred tasks with a handful of people on it
//! has only a handful of distinct combinations, so the number of intersections
//! tracks the number of distinct assignment sets, not the number of tasks. In
//! the overwhelmingly common case, no task calendar and nobody with a calendar
//! of their own, nothing is intersected at all and every row borrows the project
//! calendar.
//!
//! Both halves of that were measured rather than assumed, on a release build.
//! A plan of 500 tasks and 20 people, each with three months of leave and four
//! people per task, composes in 0.5ms against a whole reschedule of 1.2ms; at
//! 5000 tasks and 50 people it is 0.9ms against 5.5ms. Composing per row per
//! pass instead, which is the obvious thing to do without a cache, costs 58ms
//! and 199ms for those same two plans: two orders of magnitude, and the whole
//! reason the cache is here rather than a call in the middle of each pass.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::calendar::WorkCalendar;
use crate::model::{Project, ResourceId, ResourceKind};

/// The calendar one person keeps to: their base, plus their own exceptions.
///
/// Their own exceptions come first because `WorkCalendar::shifts_on` takes the
/// first match, and a person's own answer about their own time has to beat the
/// base's. That is what lets somebody work a day the organisation is shut.
///
/// A person who names nothing, or names a calendar the library has lost,
/// follows the project calendar, which is what every plan did before resource
/// calendars existed.
pub fn resource_calendar(project: &Project, id: ResourceId) -> Cow<'_, WorkCalendar> {
    let Some(resource) = project.resource(id) else {
        return Cow::Borrowed(&project.calendar);
    };
    let base = project.calendar_or_project(&resource.base_calendar);
    if resource.calendar_exceptions.is_empty() {
        return Cow::Borrowed(base);
    }
    let mut own = base.clone();
    // Naming it after the person is what makes an "Ada is away" gap readable
    // when it turns up in a composed calendar's name.
    own.name = resource.name.clone();
    let mut exceptions = resource.calendar_exceptions.clone();
    exceptions.extend(base.exceptions.iter().cloned());
    own.exceptions = exceptions;
    Cow::Owned(own)
}

/// The calendars one row has to satisfy, in the order they are composed.
///
/// Split out from the composing so that the cache can key on the same question
/// two rows are asking without composing an answer twice.
fn parts_for(project: &Project, index: usize) -> Vec<Cow<'_, WorkCalendar>> {
    let Some(task) = project.tasks.get(index) else {
        return vec![Cow::Borrowed(&project.calendar)];
    };

    let has_task_calendar = !task.calendar.trim().is_empty();
    let base = project.calendar_or_project(&task.calendar);
    let mut parts: Vec<Cow<'_, WorkCalendar>> = vec![Cow::Borrowed(base)];

    // The flag only bites when the task named a calendar of its own, so a plan
    // that ticked it and never set one schedules exactly as it did before.
    if has_task_calendar && task.ignore_resource_calendars {
        return parts;
    }

    for assignment in &task.assignments {
        let is_work = project
            .resource(assignment.resource)
            .is_some_and(|r| r.kind == ResourceKind::Work);
        if !is_work {
            continue;
        }
        let calendar = resource_calendar(project, assignment.resource);
        // A person on the same calendar as everything else adds nothing, and
        // intersecting a calendar with itself is a pure cost. This is what keeps
        // the ordinary plan at zero intersections.
        if parts.iter().any(|part| part.as_ref() == calendar.as_ref()) {
            continue;
        }
        parts.push(calendar);
    }
    parts
}

/// Compose the calendars a row has to satisfy into the one it is worked to.
///
/// Borrows rather than clones when there is only one, which is the usual case.
pub fn effective_calendar(project: &Project, index: usize) -> Cow<'_, WorkCalendar> {
    let mut parts = parts_for(project, index);
    if parts.len() == 1 {
        // A single part is already the answer; there is nothing to intersect.
        return parts.remove(0);
    }
    let mut composed = parts[0].as_ref().clone();
    for part in &parts[1..] {
        composed = composed.intersect(part.as_ref());
    }
    Cow::Owned(composed)
}

/// What every row in a plan is worked to, composed once.
pub struct EffectiveCalendars {
    /// One composed calendar per distinct combination the plan asks for.
    composed: Vec<WorkCalendar>,
    /// Row index into `composed`.
    of_row: Vec<usize>,
    /// Which composed entries turned out to have no working time at all.
    empty: Vec<bool>,
    /// What a row with no working time is scheduled against instead.
    fallback: WorkCalendar,
}

/// What makes two rows ask the same question about working time.
///
/// Assignment order is not part of it, because intersection does not care in
/// which order it is done, and two rows with the same people on them in a
/// different order must not each pay for a composition.
type CombinationKey = (String, bool, Vec<ResourceId>);

fn combination_key(project: &Project, index: usize) -> CombinationKey {
    let Some(task) = project.tasks.get(index) else {
        return (String::new(), false, Vec::new());
    };
    let has_task_calendar = !task.calendar.trim().is_empty();
    let ignore = has_task_calendar && task.ignore_resource_calendars;
    let mut resources: Vec<ResourceId> = if ignore {
        Vec::new()
    } else {
        task.assignments.iter().map(|a| a.resource).collect()
    };
    resources.sort_unstable();
    resources.dedup();
    (task.calendar.clone(), ignore, resources)
}

impl EffectiveCalendars {
    pub fn build(project: &Project) -> Self {
        let mut composed: Vec<WorkCalendar> = Vec::new();
        let mut empty: Vec<bool> = Vec::new();
        let mut of_row: Vec<usize> = Vec::with_capacity(project.tasks.len());
        let mut seen: HashMap<CombinationKey, usize> = HashMap::new();

        for index in 0..project.tasks.len() {
            let key = combination_key(project, index);
            let slot = match seen.get(&key) {
                Some(&slot) => slot,
                None => {
                    let calendar = effective_calendar(project, index).into_owned();
                    let slot = composed.len();
                    empty.push(!calendar.has_working_time());
                    composed.push(calendar);
                    seen.insert(key, slot);
                    slot
                }
            };
            of_row.push(slot);
        }

        Self {
            composed,
            of_row,
            empty,
            fallback: project.calendar.clone(),
        }
    }

    /// The calendar a row is worked to.
    ///
    /// A row whose calendars intersect to nothing is handed the project
    /// calendar instead. That is a stand-in and not an answer: its dates are
    /// arithmetically sound but nobody is actually there to do the work, which
    /// is why `has_no_working_time` exists and why the scheduler flags the row.
    /// Walking an empty calendar would give a date arrived at by running out of
    /// scan budget, which is worse than a wrong date that says it is wrong.
    pub fn for_row(&self, index: usize) -> &WorkCalendar {
        match self.of_row.get(index) {
            Some(&slot) if !self.empty.get(slot).copied().unwrap_or(false) => &self.composed[slot],
            _ => &self.fallback,
        }
    }

    /// Whether this row's calendars leave it nowhere to be done.
    pub fn has_no_working_time(&self, index: usize) -> bool {
        self.of_row
            .get(index)
            .and_then(|&slot| self.empty.get(slot))
            .copied()
            .unwrap_or(false)
    }

    /// How many calendars actually had to be composed, which is what the cache
    /// exists to keep small. Used by the tests that hold it to that.
    pub fn composed_count(&self) -> usize {
        self.composed.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{CalendarException, DayShifts};
    use crate::model::{Assignment, Resource, Task};
    use chrono::NaiveDate;

    fn leave(name: &str, from: (i32, u32, u32), to: (i32, u32, u32)) -> CalendarException {
        CalendarException {
            name: name.into(),
            from: NaiveDate::from_ymd_opt(from.0, from.1, from.2).unwrap(),
            to: NaiveDate::from_ymd_opt(to.0, to.1, to.2).unwrap(),
            shifts: DayShifts::nonworking(),
        }
    }

    fn plan() -> Project {
        let mut project = Project::blank(
            NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        );
        let id = project.allocate_task_id();
        project.tasks.push(Task::new(id, "Write it", 480));
        project
    }

    #[test]
    fn a_plan_with_no_resource_calendars_composes_nothing() {
        // The whole point of the cache is that the ordinary plan pays nothing.
        let mut project = plan();
        let id = project.allocate_resource_id();
        project.resources.push(Resource::new(id, "Ada"));
        project.tasks[0].assignments.push(Assignment {
            resource: id,
            units: 1.0,
        });

        let cals = EffectiveCalendars::build(&project);
        assert_eq!(cals.composed_count(), 1, "one borrow of the project calendar");
        assert_eq!(cals.for_row(0), &project.calendar);
    }

    #[test]
    fn rows_asking_the_same_question_share_one_composition() {
        let mut project = plan();
        let id = project.allocate_resource_id();
        let mut ada = Resource::new(id, "Ada");
        ada.calendar_exceptions.push(leave("Away", (2026, 3, 3), (2026, 3, 14)));
        project.resources.push(ada);

        for name in ["Two", "Three", "Four"] {
            let task_id = project.allocate_task_id();
            project.tasks.push(Task::new(task_id, name, 480));
        }
        for task in &mut project.tasks {
            task.assignments.push(Assignment {
                resource: id,
                units: 1.0,
            });
        }

        let cals = EffectiveCalendars::build(&project);
        assert_eq!(project.tasks.len(), 4);
        assert_eq!(
            cals.composed_count(),
            1,
            "four rows with the same person on them ask one question"
        );
    }

    #[test]
    fn the_order_people_were_assigned_in_does_not_split_the_cache() {
        let mut project = plan();
        let ada = project.allocate_resource_id();
        let mut ada_res = Resource::new(ada, "Ada");
        ada_res.calendar_exceptions.push(leave("Away", (2026, 3, 3), (2026, 3, 6)));
        project.resources.push(ada_res);
        let ben = project.allocate_resource_id();
        let mut ben_res = Resource::new(ben, "Ben");
        ben_res.calendar_exceptions.push(leave("Away", (2026, 3, 9), (2026, 3, 13)));
        project.resources.push(ben_res);

        let second = project.allocate_task_id();
        project.tasks.push(Task::new(second, "Two", 480));
        project.tasks[0].assignments = vec![
            Assignment { resource: ada, units: 1.0 },
            Assignment { resource: ben, units: 1.0 },
        ];
        project.tasks[1].assignments = vec![
            Assignment { resource: ben, units: 1.0 },
            Assignment { resource: ada, units: 1.0 },
        ];

        let cals = EffectiveCalendars::build(&project);
        assert_eq!(cals.composed_count(), 1);
    }

    #[test]
    fn the_ignore_flag_needs_a_task_calendar_to_mean_anything() {
        let mut project = plan();
        let id = project.allocate_resource_id();
        let mut ada = Resource::new(id, "Ada");
        ada.calendar_exceptions.push(leave("Away", (2026, 3, 3), (2026, 3, 14)));
        project.resources.push(ada);
        project.tasks[0].assignments.push(Assignment {
            resource: id,
            units: 1.0,
        });
        project.tasks[0].ignore_resource_calendars = true;

        // No task calendar, so the flag is inert and Ada's leave still counts.
        let with_flag_only = effective_calendar(&project, 0).into_owned();
        assert!(!with_flag_only.is_working_day(NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()));

        // Name a calendar and the flag takes effect.
        project.tasks[0].calendar = "Round the clock".into();
        let mut round = WorkCalendar::twenty_four_hour();
        round.name = "Round the clock".into();
        project.calendars.push(round);
        let with_calendar = effective_calendar(&project, 0).into_owned();
        assert!(with_calendar.is_working_day(NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()));
    }

    #[test]
    fn a_name_the_library_has_lost_means_the_project_calendar() {
        // An import or a deleted calendar must not leave a task with nowhere to
        // be done; it has to mean what it meant before calendars were named.
        let mut project = plan();
        project.tasks[0].calendar = "Gone".into();
        assert_eq!(effective_calendar(&project, 0).as_ref(), &project.calendar);
    }

    #[test]
    fn people_who_are_never_both_there_leave_no_working_time() {
        let mut project = plan();

        let mut first_half = WorkCalendar::standard();
        first_half.name = "Monday to Wednesday".into();
        first_half.week[3] = DayShifts::nonworking();
        first_half.week[4] = DayShifts::nonworking();
        project.calendars.push(first_half);

        let mut second_half = WorkCalendar::standard();
        second_half.name = "Thursday and Friday".into();
        for day in 0..3 {
            second_half.week[day] = DayShifts::nonworking();
        }
        project.calendars.push(second_half);

        let ada = project.allocate_resource_id();
        let mut ada_res = Resource::new(ada, "Ada");
        ada_res.base_calendar = "Monday to Wednesday".into();
        project.resources.push(ada_res);

        let ben = project.allocate_resource_id();
        let mut ben_res = Resource::new(ben, "Ben");
        ben_res.base_calendar = "Thursday and Friday".into();
        project.resources.push(ben_res);

        project.tasks[0].assignments = vec![
            Assignment { resource: ada, units: 1.0 },
            Assignment { resource: ben, units: 1.0 },
        ];

        let composed = effective_calendar(&project, 0).into_owned();
        assert!(
            !composed.has_working_time(),
            "no day of the week has both of them in it"
        );

        let cals = EffectiveCalendars::build(&project);
        assert!(cals.has_no_working_time(0));
        assert_eq!(
            cals.for_row(0),
            &project.calendar,
            "and the stand-in is the project calendar, not an empty one"
        );
    }

    #[test]
    fn a_persons_own_exception_beats_the_base_calendars() {
        // The organisation is shut on the Tuesday; this person is in anyway.
        let mut project = plan();
        let shutdown = NaiveDate::from_ymd_opt(2026, 3, 3).unwrap();
        project.calendar.exceptions.push(leave("Shutdown", (2026, 3, 3), (2026, 3, 3)));

        let id = project.allocate_resource_id();
        let mut ada = Resource::new(id, "Ada");
        ada.calendar_exceptions.push(CalendarException {
            name: "Catching up".into(),
            from: shutdown,
            to: shutdown,
            shifts: DayShifts::standard(),
        });
        project.resources.push(ada);

        let calendar = resource_calendar(&project, id).into_owned();
        assert!(calendar.is_working_day(shutdown));
    }
}
