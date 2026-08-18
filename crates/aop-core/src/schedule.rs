//! Critical path scheduling.
//!
//! The engine schedules leaf tasks only; summary rows are derived afterwards by
//! rolling their children up. Links are allowed to name a summary at either end,
//! and are expanded onto that summary's leaves for ordering purposes. Because
//! the topological order therefore places every leaf of a predecessor before its
//! successor, a summary's rolled-up dates are always known by the time a
//! successor reads them, and a single forward pass is enough.

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::calendar::WorkCalendar;
use crate::model::{
    ConstraintType, Link, LinkType, Project, ResourceId, ResourceKind, ScheduleFrom, TaskId,
    TaskMode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// The names of the tasks forming a dependency loop, in order.
    CircularDependency(Vec<String>),
}

impl std::fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleError::CircularDependency(path) => {
                // A loop of three is readable in full; one of thirty is a wall
                // of text that hides the two tasks the planner has to change.
                const SHOWN: usize = 6;
                let arrow = " \u{2192} ";
                if path.len() <= SHOWN {
                    write!(
                        f,
                        "These tasks depend on each other in a loop:\n\n{}\n\n\
                         A task cannot be linked so that it ends up waiting for itself. \
                         Remove one of the links above to break it.",
                        path.join(arrow)
                    )
                } else {
                    let head = path[..SHOWN - 1].join(arrow);
                    let last = path.last().map(String::as_str).unwrap_or_default();
                    write!(
                        f,
                        "These tasks depend on each other in a loop of {}:\n\n{}{}\u{2026}{}{}\n\n\
                         A task cannot be linked so that it ends up waiting for itself. \
                         Remove one of the links in this loop to break it.",
                        path.len() - 1,
                        head,
                        arrow,
                        arrow,
                        last
                    )
                }
            }
        }
    }
}

impl std::error::Error for ScheduleError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Overallocation {
    pub resource: ResourceId,
    pub resource_name: String,
    pub first_date: NaiveDate,
    pub days: u32,
    pub peak_units: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScheduleReport {
    pub start: NaiveDateTime,
    pub finish: NaiveDateTime,
    pub duration_minutes: i64,
    pub critical_task_count: usize,
    pub total_cost: f64,
    pub total_work_minutes: i64,
    pub overallocations: Vec<Overallocation>,
}

/// Where a link places its successor, given the predecessor's dates.
fn driven_start(
    calendar: &WorkCalendar,
    link: &Link,
    pred_start: NaiveDateTime,
    pred_finish: NaiveDateTime,
    duration: i64,
) -> NaiveDateTime {
    let anchor = match link.kind {
        LinkType::FS | LinkType::FF => pred_finish,
        LinkType::SS | LinkType::SF => pred_start,
    };
    let lagged = apply_lag(calendar, anchor, link.lag_minutes);
    match link.kind {
        // Start-driven links place the start directly.
        LinkType::FS | LinkType::SS => calendar.next_working_instant(lagged),
        // Finish-driven links place the finish, so back off by the duration.
        LinkType::FF | LinkType::SF => calendar.sub_minutes(lagged, duration),
    }
}

/// Where a link places its predecessor's late finish, given the successor's.
fn driven_late_finish(
    calendar: &WorkCalendar,
    link: &Link,
    succ_late_start: NaiveDateTime,
    succ_late_finish: NaiveDateTime,
    duration: i64,
) -> NaiveDateTime {
    let anchor = match link.kind {
        LinkType::FS | LinkType::SF => succ_late_start,
        LinkType::SS | LinkType::FF => succ_late_finish,
    };
    let lagged = apply_lag(calendar, anchor, -link.lag_minutes);
    match link.kind {
        // The predecessor's finish is what the link constrains.
        LinkType::FS | LinkType::FF => calendar.prev_working_instant(lagged),
        // The link constrains the predecessor's start, so add the duration back.
        LinkType::SS | LinkType::SF => calendar.add_minutes(lagged, duration),
    }
}

fn apply_lag(calendar: &WorkCalendar, at: NaiveDateTime, lag_minutes: i64) -> NaiveDateTime {
    match lag_minutes.cmp(&0) {
        std::cmp::Ordering::Greater => calendar.add_minutes(at, lag_minutes),
        std::cmp::Ordering::Less => calendar.sub_minutes(at, -lag_minutes),
        std::cmp::Ordering::Equal => at,
    }
}

struct Graph {
    /// Leaf row indices in dependency order.
    order: Vec<usize>,
    /// Leaf row index -> links whose successor resolves onto it.
    incoming: HashMap<usize, Vec<Link>>,
    /// Leaf row index -> links whose predecessor resolves onto it.
    outgoing: HashMap<usize, Vec<Link>>,
    /// Task id -> the leaf rows it covers.
    leaves_of: HashMap<TaskId, Vec<usize>>,
}

fn build_graph(project: &Project) -> Result<Graph, ScheduleError> {
    let count = project.tasks.len();
    let summary: Vec<bool> = (0..count).map(|i| project.is_summary(i)).collect();
    let leaves: Vec<usize> = (0..count).filter(|&i| !summary[i]).collect();
    let leaf_set: HashSet<usize> = leaves.iter().copied().collect();

    let mut leaves_of: HashMap<TaskId, Vec<usize>> = HashMap::new();
    for (index, &is_summary) in summary.iter().enumerate().take(count) {
        let id = project.tasks[index].id;
        let covered = if is_summary {
            project
                .descendants(index)
                .filter(|i| leaf_set.contains(i))
                .collect()
        } else {
            vec![index]
        };
        leaves_of.insert(id, covered);
    }

    let mut incoming: HashMap<usize, Vec<Link>> = HashMap::new();
    let mut outgoing: HashMap<usize, Vec<Link>> = HashMap::new();
    let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut indegree: HashMap<usize, usize> = leaves.iter().map(|&i| (i, 0)).collect();

    for link in &project.links {
        let (Some(pred_leaves), Some(succ_leaves)) = (
            leaves_of.get(&link.predecessor),
            leaves_of.get(&link.successor),
        ) else {
            continue;
        };
        if pred_leaves.is_empty() || succ_leaves.is_empty() {
            continue;
        }
        // A link onto a summary applies to each of its leaves.
        for &succ in succ_leaves {
            if pred_leaves.contains(&succ) {
                continue;
            }
            incoming.entry(succ).or_default().push(*link);
        }
        for &pred in pred_leaves {
            if succ_leaves.contains(&pred) {
                continue;
            }
            outgoing.entry(pred).or_default().push(*link);
            for &succ in succ_leaves {
                if pred == succ {
                    continue;
                }
                adjacency.entry(pred).or_default().push(succ);
                *indegree.entry(succ).or_insert(0) += 1;
            }
        }
    }

    // Kahn's algorithm, seeded in row order so an unlinked plan keeps its
    // natural top-to-bottom reading order.
    let mut queue: VecDeque<usize> = leaves
        .iter()
        .copied()
        .filter(|i| indegree.get(i).copied().unwrap_or(0) == 0)
        .collect();
    let mut order = Vec::with_capacity(leaves.len());

    while let Some(node) = queue.pop_front() {
        order.push(node);
        if let Some(next) = adjacency.get(&node) {
            for &succ in next {
                let entry = indegree.entry(succ).or_insert(0);
                *entry = entry.saturating_sub(1);
                if *entry == 0 {
                    queue.push_back(succ);
                }
            }
        }
    }

    if order.len() != leaves.len() {
        let stuck: Vec<usize> = leaves
            .iter()
            .copied()
            .filter(|i| indegree.get(i).copied().unwrap_or(0) > 0)
            .collect();
        let names = trace_cycle(project, &adjacency, &stuck);
        return Err(ScheduleError::CircularDependency(names));
    }

    Ok(Graph {
        order,
        incoming,
        outgoing,
        leaves_of,
    })
}

/// Walk the leftover nodes to recover a readable loop for the error message.
fn trace_cycle(
    project: &Project,
    adjacency: &HashMap<usize, Vec<usize>>,
    stuck: &[usize],
) -> Vec<String> {
    let Some(&start) = stuck.first() else {
        return vec!["unknown".into()];
    };
    let stuck_set: HashSet<usize> = stuck.iter().copied().collect();
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    let mut node = start;

    while seen.insert(node) {
        path.push(node);
        let Some(next) = adjacency
            .get(&node)
            .and_then(|list| list.iter().copied().find(|n| stuck_set.contains(n)))
        else {
            break;
        };
        node = next;
    }
    path.push(node);

    // Walking from an arbitrary stuck task usually arrives at the loop through
    // a long run of tasks that are merely upstream of it. Those are not part of
    // the loop and naming them buries the handful of tasks that are, so the
    // run-in is trimmed: the loop is what sits between the repeated task and
    // its second appearance.
    if let Some(at) = path.iter().position(|&n| n == node) {
        path.drain(..at);
    }

    path.iter()
        .filter_map(|&i| project.tasks.get(i).map(|t| t.name.clone()))
        .collect()
}

fn forward_pass(project: &mut Project, graph: &Graph, pinned: &HashMap<usize, NaiveDateTime>) {
    let calendar = project.calendar.clone();
    let project_start = calendar.next_working_instant(project.start_date);

    for &index in &graph.order {
        let duration = project.tasks[index].duration_minutes;
        let mut start = project_start;

        // Manually scheduled tasks sit where the user put them.
        if project.tasks[index].mode == TaskMode::Manual
            && let Some(manual) = project.tasks[index].manual_start {
                start = manual;
            }

        for link in graph.incoming.get(&index).into_iter().flatten() {
            let Some(pred_leaves) = graph.leaves_of.get(&link.predecessor) else {
                continue;
            };
            // A summary predecessor contributes its rolled-up span.
            let Some(pred_start) = pred_leaves
                .iter()
                .map(|&i| project.tasks[i].scheduled.start)
                .min()
            else {
                continue;
            };
            let pred_finish = pred_leaves
                .iter()
                .map(|&i| project.tasks[i].scheduled.finish)
                .max()
                .unwrap_or(pred_start);

            if project.tasks[index].mode == TaskMode::Manual {
                continue;
            }
            let candidate = driven_start(&calendar, link, pred_start, pred_finish, duration);
            if candidate > start {
                start = candidate;
            }
        }

        // Something outside the plan cannot be moved by rescheduling, so it is
        // a floor on the start rather than something to negotiate with. A
        // manually placed task is left where the planner put it, as with links.
        if project.tasks[index].mode != TaskMode::Manual
            && let Some(ready) = project.external_ready(index)
            && ready > start
        {
            start = ready;
        }

        // Date constraints can only push a task later during the forward pass.
        let task = &project.tasks[index];
        if let Some(date) = task.constraint_date {
            match task.constraint {
                ConstraintType::StartNoEarlierThan => start = start.max(date),
                ConstraintType::MustStartOn => start = date,
                ConstraintType::FinishNoEarlierThan => {
                    start = start.max(calendar.sub_minutes(date, duration));
                }
                ConstraintType::MustFinishOn => start = calendar.sub_minutes(date, duration),
                _ => {}
            }
        }

        if let Some(&pin) = pinned.get(&index) {
            start = pin;
        }

        // A milestone is an instant, not a span. Snapping it the way a task
        // start is snapped would push a marker sitting at the end of a working
        // day into the following morning, landing it on the wrong date.
        let (start, finish) = if duration == 0 {
            let at = calendar.snap_marker(start);
            (at, at)
        } else {
            let start = calendar.next_working_instant(start);
            (start, calendar.add_minutes(start, duration))
        };
        let slot = &mut project.tasks[index].scheduled;
        slot.start = start;
        slot.finish = finish;
        slot.duration_minutes = duration;
    }
}

fn backward_pass(project: &mut Project, graph: &Graph, project_finish: NaiveDateTime) {
    let calendar = project.calendar.clone();

    for &index in graph.order.iter().rev() {
        let duration = project.tasks[index].duration_minutes;
        let mut late_finish = project_finish;
        let mut constrained = false;

        for link in graph.outgoing.get(&index).into_iter().flatten() {
            let Some(succ_leaves) = graph.leaves_of.get(&link.successor) else {
                continue;
            };
            let Some(succ_late_start) = succ_leaves
                .iter()
                .map(|&i| project.tasks[i].scheduled.late_start)
                .min()
            else {
                continue;
            };
            let succ_late_finish = succ_leaves
                .iter()
                .map(|&i| project.tasks[i].scheduled.late_finish)
                .max()
                .unwrap_or(succ_late_start);

            let candidate = driven_late_finish(
                &calendar,
                link,
                succ_late_start,
                succ_late_finish,
                duration,
            );
            if !constrained || candidate < late_finish {
                late_finish = candidate;
                constrained = true;
            }
        }

        // Late constraints and deadlines pull the late dates in, which is what
        // produces negative slack when a plan cannot meet its dates.
        let task = &project.tasks[index];
        if let Some(date) = task.constraint_date {
            match task.constraint {
                ConstraintType::StartNoLaterThan => {
                    late_finish = late_finish.min(calendar.add_minutes(date, duration));
                }
                ConstraintType::FinishNoLaterThan => late_finish = late_finish.min(date),
                ConstraintType::MustStartOn => {
                    late_finish = calendar.add_minutes(date, duration);
                }
                ConstraintType::MustFinishOn => late_finish = date,
                _ => {}
            }
        }
        if let Some(deadline) = task.deadline {
            late_finish = late_finish.min(deadline);
        }

        let (late_finish, late_start) = if duration == 0 {
            let at = calendar.snap_marker(late_finish);
            (at, at)
        } else {
            let late_finish = calendar.prev_working_instant(late_finish);
            (late_finish, calendar.sub_minutes(late_finish, duration))
        };
        let slot = &mut project.tasks[index].scheduled;
        slot.late_finish = late_finish;
        slot.late_start = late_start;
    }
}

fn compute_slack(project: &mut Project, graph: &Graph) {
    let calendar = project.calendar.clone();
    let project_finish = project
        .tasks
        .iter()
        .map(|t| t.scheduled.finish)
        .max()
        .unwrap_or(project.start_date);

    for &index in &graph.order {
        let scheduled = project.tasks[index].scheduled;
        let total = calendar.work_minutes_between(scheduled.finish, scheduled.late_finish);

        // Free slack is how far this task can slip before it moves anything.
        let mut free = None;
        for link in graph.outgoing.get(&index).into_iter().flatten() {
            let Some(succ_leaves) = graph.leaves_of.get(&link.successor) else {
                continue;
            };
            for &succ in succ_leaves {
                let succ_start = project.tasks[succ].scheduled.start;
                let gap = calendar.work_minutes_between(scheduled.finish, succ_start);
                free = Some(free.map_or(gap, |current: i64| current.min(gap)));
            }
        }
        let free = free
            .unwrap_or_else(|| calendar.work_minutes_between(scheduled.finish, project_finish))
            .min(total)
            .max(0);

        let active = project.tasks[index].active;
        let slot = &mut project.tasks[index].scheduled;
        slot.total_slack_minutes = total;
        slot.free_slack_minutes = free;
        slot.critical = total <= 0 && active;
    }
}

fn compute_cost_and_work(project: &mut Project) {
    let rates: HashMap<ResourceId, (ResourceKind, f64, f64)> = project
        .resources
        .iter()
        .map(|r| (r.id, (r.kind, r.standard_rate, r.cost_per_use)))
        .collect();

    for index in 0..project.tasks.len() {
        if project.is_summary(index) {
            continue;
        }
        let task = &project.tasks[index];
        let duration = task.duration_minutes;
        let mut work = 0i64;
        let mut cost = task.fixed_cost;

        for assignment in &task.assignments {
            let Some(&(kind, standard_rate, cost_per_use)) = rates.get(&assignment.resource) else {
                continue;
            };
            match kind {
                ResourceKind::Work => {
                    let minutes = (duration as f64 * assignment.units).round() as i64;
                    work += minutes;
                    cost += (minutes as f64 / 60.0) * standard_rate + cost_per_use;
                }
                ResourceKind::Material => {
                    cost += assignment.units * standard_rate + cost_per_use;
                }
                ResourceKind::Cost => {
                    cost += assignment.units * standard_rate;
                }
            }
        }

        let slot = &mut project.tasks[index].scheduled;
        slot.work_minutes = work;
        slot.cost = cost;
    }
}

/// Summary rows span their children and sum their work and cost.
fn roll_up_summaries(project: &mut Project) {
    let count = project.tasks.len();
    // Deepest first, so a nested summary is finished before its parent reads it.
    let mut order: Vec<usize> = (0..count).filter(|&i| project.is_summary(i)).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(project.tasks[i].outline_level));

    for index in order {
        let children: Vec<usize> = project.descendants(index).collect();
        if children.is_empty() {
            continue;
        }
        let direct: Vec<usize> = {
            let level = project.tasks[index].outline_level;
            children
                .iter()
                .copied()
                .filter(|&i| project.tasks[i].outline_level == level + 1)
                .collect()
        };
        let leaves: Vec<usize> = children
            .iter()
            .copied()
            .filter(|&i| !project.is_summary(i))
            .collect();
        if leaves.is_empty() {
            continue;
        }

        let start = leaves
            .iter()
            .map(|&i| project.tasks[i].scheduled.start)
            .min()
            .unwrap();
        let finish = leaves
            .iter()
            .map(|&i| project.tasks[i].scheduled.finish)
            .max()
            .unwrap();
        let late_start = leaves
            .iter()
            .map(|&i| project.tasks[i].scheduled.late_start)
            .min()
            .unwrap();
        let late_finish = leaves
            .iter()
            .map(|&i| project.tasks[i].scheduled.late_finish)
            .max()
            .unwrap();

        let work: i64 = leaves.iter().map(|&i| project.tasks[i].scheduled.work_minutes).sum();
        let cost: f64 = leaves.iter().map(|&i| project.tasks[i].scheduled.cost).sum();
        let critical = leaves.iter().any(|&i| project.tasks[i].scheduled.critical);

        // Completion rolls up weighted by each child's duration.
        let mut planned = 0i64;
        let mut done = 0i64;
        for &leaf in &leaves {
            let minutes = project.tasks[leaf].duration_minutes.max(1);
            planned += minutes;
            done += minutes * project.tasks[leaf].percent_complete as i64 / 100;
        }
        let percent = if planned == 0 {
            0
        } else {
            ((done * 100) / planned).clamp(0, 100) as u8
        };

        let span = project.calendar.work_minutes_between(start, finish);
        let total_slack = project
            .calendar
            .work_minutes_between(finish, late_finish);

        let _ = direct;
        let task = &mut project.tasks[index];
        task.percent_complete = percent;
        task.scheduled.start = start;
        task.scheduled.finish = finish;
        task.scheduled.late_start = late_start;
        task.scheduled.late_finish = late_finish;
        task.scheduled.duration_minutes = span;
        task.scheduled.work_minutes = work;
        task.scheduled.cost = cost;
        task.scheduled.critical = critical;
        task.scheduled.total_slack_minutes = total_slack;
        task.scheduled.free_slack_minutes = 0;
    }
}

fn find_overallocations(project: &Project) -> Vec<Overallocation> {
    let Some(start) = project.tasks.iter().map(|t| t.scheduled.start).min() else {
        return Vec::new();
    };
    let Some(finish) = project.tasks.iter().map(|t| t.scheduled.finish).max() else {
        return Vec::new();
    };

    let mut per_resource: HashMap<ResourceId, HashMap<NaiveDate, f64>> = HashMap::new();
    for index in 0..project.tasks.len() {
        if project.is_summary(index) {
            continue;
        }
        let task = &project.tasks[index];
        if !task.active || task.assignments.is_empty() {
            continue;
        }
        let mut date = task.scheduled.start.date();
        let last = task.scheduled.finish.date();
        let mut guard = 0;
        while date <= last && guard < 4000 {
            if project.calendar.is_working_day(date) {
                for assignment in &task.assignments {
                    let is_work = project
                        .resource(assignment.resource)
                        .is_some_and(|r| r.kind == ResourceKind::Work);
                    if is_work {
                        *per_resource
                            .entry(assignment.resource)
                            .or_default()
                            .entry(date)
                            .or_insert(0.0) += assignment.units;
                    }
                }
            }
            date += Duration::days(1);
            guard += 1;
        }
    }

    let _ = (start, finish);
    let mut result = Vec::new();
    for resource in &project.resources {
        let Some(days) = per_resource.get(&resource.id) else {
            continue;
        };
        // Both halves are Copy, so the days worth reporting are taken by value
        // rather than carried around as a pair of references and dereferenced
        // twice at every use.
        let over: Vec<(NaiveDate, f64)> = days
            .iter()
            .filter(|(_, units)| **units > resource.max_units + 1e-9)
            .map(|(date, units)| (*date, *units))
            .collect();
        if over.is_empty() {
            continue;
        }
        let first_date = over.iter().map(|(date, _)| *date).min().unwrap();
        let peak = over.iter().map(|(_, units)| *units).fold(0.0f64, f64::max);
        result.push(Overallocation {
            resource: resource.id,
            resource_name: resource.name.clone(),
            first_date,
            days: over.len() as u32,
            peak_units: peak,
        });
    }
    result.sort_by(|a, b| a.first_date.cmp(&b.first_date));
    result
}

/// A proposed repair for a plan that will not schedule.
///
/// Nothing is applied until the caller asks for it, so the user can be shown
/// exactly what would change first.
#[derive(Debug, Clone, PartialEq)]
pub struct Remedy {
    /// What is wrong, in one sentence.
    pub problem: String,
    /// What the fix will do, in one sentence.
    pub action: String,
    /// The individual changes, one line each.
    pub changes: Vec<String>,
    /// The links that would be removed.
    pub links_to_remove: Vec<Link>,
}

/// Work out how to make a broken plan schedule again.
///
/// A dependency loop is broken by dropping one link. Links are tried newest
/// first, because the newest is nearly always the one just added by mistake,
/// and the older links are the ones the plan was already built around.
pub fn diagnose(project: &Project) -> Option<Remedy> {
    let mut probe = project.clone();
    let error = schedule(&mut probe).err()?;

    match error {
        ScheduleError::CircularDependency(path) => {
            let loop_text = if path.is_empty() {
                String::from("a loop")
            } else {
                path.join(" \u{2192} ")
            };

            // Try removing each link, newest first, and keep the first one that
            // lets the whole plan schedule.
            for index in (0..project.links.len()).rev() {
                let mut candidate = project.clone();
                let removed = candidate.links.remove(index);
                if schedule(&mut candidate).is_ok() {
                    let from = project
                        .task(removed.predecessor)
                        .map(|t| task_label(project, t))
                        .unwrap_or_else(|| "a task".into());
                    let to = project
                        .task(removed.successor)
                        .map(|t| task_label(project, t))
                        .unwrap_or_else(|| "a task".into());

                    return Some(Remedy {
                        problem: format!(
                            "These tasks depend on each other in a loop: {loop_text}. A plan cannot be scheduled while a task has to wait for itself."
                        ),
                        action: format!(
                            "Remove the {} link from {from} to {to}.",
                            removed.kind.code()
                        ),
                        changes: vec![format!(
                            "Delete link: {from} \u{2192} {to} ({})",
                            removed.kind.label()
                        )],
                        links_to_remove: vec![removed],
                    });
                }
            }

            // More than one loop, so one removal is not enough. Fall back to
            // clearing every link that touches the reported cycle.
            let involved: Vec<Link> = project
                .links
                .iter()
                .copied()
                .filter(|link| {
                    let named = |id| {
                        project
                            .task(id)
                            .map(|t| path.contains(&t.name))
                            .unwrap_or(false)
                    };
                    named(link.predecessor) && named(link.successor)
                })
                .collect();

            let changes = involved
                .iter()
                .map(|link| {
                    let from = project
                        .task(link.predecessor)
                        .map(|t| task_label(project, t))
                        .unwrap_or_else(|| "a task".into());
                    let to = project
                        .task(link.successor)
                        .map(|t| task_label(project, t))
                        .unwrap_or_else(|| "a task".into());
                    format!("Delete link: {from} \u{2192} {to} ({})", link.kind.label())
                })
                .collect();

            Some(Remedy {
                problem: format!(
                    "These tasks depend on each other in a loop: {loop_text}. There is more than one loop, so a single link will not clear it."
                ),
                action: format!(
                    "Remove the {} links between the tasks in the loop.",
                    involved.len()
                ),
                changes,
                links_to_remove: involved,
            })
        }
    }
}

/// "3 Strip deck", the way a task is referred to in the interface.
fn task_label(project: &Project, task: &crate::model::Task) -> String {
    let number = project
        .index_of(task.id)
        .map(|i| (i + 1).to_string())
        .unwrap_or_default();
    if task.name.trim().is_empty() {
        format!("{number} (unnamed task)")
    } else {
        format!("{number} {}", task.name)
    }
}

/// Apply a remedy. Returns how many links were removed.
pub fn apply_remedy(project: &mut Project, remedy: &Remedy) -> usize {
    let before = project.links.len();
    project
        .links
        .retain(|link| !remedy.links_to_remove.contains(link));
    before - project.links.len()
}

/// Explain, in one sentence, why a task sits on the critical path. Returns
/// `None` for tasks that are not critical.
///
/// This is the text behind the warning marker in the table, so it has to say
/// something a planner can act on rather than just "slack is zero".
pub fn critical_reason(project: &Project, index: usize) -> Option<String> {
    let task = project.tasks.get(index)?;
    if !task.scheduled.critical || project.is_summary(index) {
        return None;
    }

    let finish = project
        .tasks
        .iter()
        .map(|t| t.scheduled.finish)
        .max()
        .unwrap_or(task.scheduled.finish);
    let finish_text = finish.format("%d/%m/%y");
    let slack = task.scheduled.total_slack_minutes;

    // Negative slack means something is already unachievable, and the deadline
    // or constraint responsible is the useful thing to name.
    if slack < 0 {
        let late_by = crate::duration::format_duration(-slack);
        if let Some(deadline) = task.deadline {
            return Some(format!(
                "Behind by {late_by}. It cannot meet its deadline of {}.",
                deadline.format("%d/%m/%y")
            ));
        }
        if task.constraint.needs_date()
            && let Some(date) = task.constraint_date {
                return Some(format!(
                    "Behind by {late_by}. It cannot meet \"{}\" on {}.",
                    task.constraint.label(),
                    date.format("%d/%m/%y")
                ));
            }
        return Some(format!(
            "Behind by {late_by}. The plan cannot finish by the date it is held to."
        ));
    }

    // Otherwise name what this task is holding up.
    let successors: Vec<String> = project
        .successors_of(task.id)
        .into_iter()
        .filter_map(|link| project.task(link.successor))
        .map(|t| {
            if t.name.trim().is_empty() {
                "an unnamed task".to_string()
            } else {
                t.name.clone()
            }
        })
        .collect();

    Some(match successors.len() {
        0 => format!(
            "Zero slack: nothing follows it, so its finish sets the project finish of {finish_text}. Any delay moves that date."
        ),
        1 => format!(
            "Zero slack: it holds up \"{}\", and through it the project finish of {finish_text}. Any delay moves that date.",
            successors[0]
        ),
        _ => format!(
            "Zero slack: it holds up {} tasks, starting with \"{}\", and through them the project finish of {finish_text}. Any delay moves that date.",
            successors.len(),
            successors[0]
        ),
    })
}

/// Recalculate the whole plan. This is the only function that writes to
/// `Task::scheduled`, so calling it is always safe and always idempotent.
pub fn schedule(project: &mut Project) -> Result<ScheduleReport, ScheduleError> {
    if project.tasks.is_empty() {
        project.finish_date = project.start_date;
        return Ok(ScheduleReport {
            start: project.start_date,
            finish: project.start_date,
            duration_minutes: 0,
            critical_task_count: 0,
            total_cost: 0.0,
            total_work_minutes: 0,
            overallocations: Vec::new(),
        });
    }

    let graph = build_graph(project)?;
    let no_pins = HashMap::new();

    forward_pass(project, &graph, &no_pins);

    let mut project_finish = project
        .tasks
        .iter()
        .map(|t| t.scheduled.finish)
        .max()
        .unwrap_or(project.start_date);
    // Scheduling backwards from a required finish date targets that date even
    // when the plan is shorter than the window available.
    if project.schedule_from == ScheduleFrom::ProjectFinishDate {
        project_finish = project_finish.max(project.finish_date);
    }
    backward_pass(project, &graph, project_finish);

    // As Late As Possible tasks, and every task when the whole plan runs
    // backwards, are re-pinned onto their late dates and the passes repeated so
    // successors still see consistent predecessor dates.
    let mut pinned: HashMap<usize, NaiveDateTime> = HashMap::new();
    for &index in &graph.order {
        let alap = project.tasks[index].constraint == ConstraintType::AsLateAsPossible
            || project.schedule_from == ScheduleFrom::ProjectFinishDate;
        if alap {
            pinned.insert(index, project.tasks[index].scheduled.late_start);
        }
    }
    if !pinned.is_empty() {
        forward_pass(project, &graph, &pinned);
        let refreshed = project
            .tasks
            .iter()
            .map(|t| t.scheduled.finish)
            .max()
            .unwrap_or(project.start_date)
            .max(project_finish);
        backward_pass(project, &graph, refreshed);
    }

    compute_slack(project, &graph);
    compute_cost_and_work(project);
    roll_up_summaries(project);

    let start = project
        .tasks
        .iter()
        .map(|t| t.scheduled.start)
        .min()
        .unwrap_or(project.start_date);
    let finish = project
        .tasks
        .iter()
        .map(|t| t.scheduled.finish)
        .max()
        .unwrap_or(project.start_date);
    project.finish_date = finish;

    let critical_task_count = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i) && project.tasks[i].scheduled.critical)
        .count();

    Ok(ScheduleReport {
        start,
        finish,
        duration_minutes: project.calendar.work_minutes_between(start, finish),
        critical_task_count,
        total_cost: project.total_cost(),
        total_work_minutes: project.total_work_minutes(),
        overallocations: find_overallocations(project),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Task, TaskMode};
    use crate::MINUTES_PER_DAY;
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    /// Monday 2026-08-17 is the reference start for every test below.
    fn project_with(durations: &[i64]) -> Project {
        let mut project = Project::blank(day(2026, 8, 17));
        for (n, &duration) in durations.iter().enumerate() {
            project.push_task(format!("Task {}", n + 1), duration);
        }
        project
    }

    #[test]
    fn ignoring_a_warning_does_not_take_a_task_off_the_critical_path() {
        // The path was selected with `shows_as_critical`, which is a
        // presentation rule: it hides tasks whose critical warning the planner
        // has dismissed. Dismissing a warning is a statement about the warning
        // list, not about the schedule, and letting it reach the algorithm
        // broke the chain in half wherever it was used.
        let mut project = Project::blank(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        );
        let mut ids = Vec::new();
        for name in ["First", "Second", "Third"] {
            let id = project.allocate_task_id();
            project.tasks.push(crate::model::Task::new(id, name, 480));
            ids.push(id);
        }
        for pair in ids.windows(2) {
            project.links.push(Link {
                predecessor: pair[0],
                successor: pair[1],
                kind: LinkType::FS,
                lag_minutes: 0,
            });
        }
        schedule(&mut project).expect("a straight chain schedules");

        let whole = critical_path(&project);
        assert_eq!(whole.len(), 3, "a straight chain of three is the path");

        // The planner dismisses the middle task's critical warning.
        project.tasks[1]
            .ignored_issues
            .push(crate::model::IssueKind::Critical);

        let after = critical_path(&project);
        assert_eq!(
            after.len(),
            3,
            "dismissing a warning must not remove the task from the chain, got {:?}",
            after.iter().map(|s| project.tasks[s.index].name.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_loop_is_reported_as_the_loop_and_not_everything_leading_into_it() {
        // Walking to a cycle from an arbitrary task passes through everything
        // upstream of it. Naming all of that buries the two or three tasks the
        // planner actually has to change: a real plan produced a message
        // hundreds of tasks long for a loop of two.
        let mut project = Project::blank(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        );
        let mut ids = Vec::new();
        for name in ["A", "B", "C", "D", "E", "F"] {
            let id = project.allocate_task_id();
            project.tasks.push(crate::model::Task::new(id, name, 480));
            ids.push(id);
        }
        // A long tail, then a loop between the last two.
        let link = |from: TaskId, to: TaskId| Link {
            predecessor: from,
            successor: to,
            kind: LinkType::FS,
            lag_minutes: 0,
        };
        for pair in ids.windows(2) {
            project.links.push(link(pair[0], pair[1]));
        }
        project.links.push(link(ids[5], ids[4]));

        let error = schedule(&mut project).expect_err("this plan cannot schedule");
        let ScheduleError::CircularDependency(path) = error;

        assert!(
            path.len() <= 3,
            "only the loop should be named, got {} entries: {path:?}",
            path.len()
        );
        assert_eq!(
            path.first(),
            path.last(),
            "a loop has to come back to where it started: {path:?}"
        );
        for name in ["A", "B", "C"] {
            assert!(
                !path.iter().any(|n| n == name),
                "{name} only leads into the loop, it is not in it: {path:?}"
            );
        }
    }

    #[test]
    fn a_chain_of_finish_to_start_links_runs_end_to_end() {
        let mut project = project_with(&[MINUTES_PER_DAY * 2, MINUTES_PER_DAY * 3]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link::finish_to_start(a, b));

        let report = schedule(&mut project).unwrap();

        // Task 1: Mon 08:00 -> Tue 17:00. Task 2: Wed 08:00 -> Fri 17:00.
        assert_eq!(project.tasks[0].scheduled.start, day(2026, 8, 17));
        assert_eq!(
            project.tasks[0].scheduled.finish,
            day(2026, 8, 18).with_hour_17()
        );
        assert_eq!(project.tasks[1].scheduled.start, day(2026, 8, 19));
        assert_eq!(report.finish, day(2026, 8, 21).with_hour_17());
    }

    #[test]
    fn everything_on_a_single_chain_is_critical() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link::finish_to_start(a, b));

        let report = schedule(&mut project).unwrap();

        assert_eq!(report.critical_task_count, 2);
        assert!(project.tasks.iter().all(|t| t.scheduled.critical));
    }

    #[test]
    fn the_shorter_parallel_branch_gets_slack_and_is_not_critical() {
        // Start -> (long 5d | short 2d) -> End
        let mut project = project_with(&[
            MINUTES_PER_DAY,
            MINUTES_PER_DAY * 5,
            MINUTES_PER_DAY * 2,
            MINUTES_PER_DAY,
        ]);
        let ids: Vec<TaskId> = project.tasks.iter().map(|t| t.id).collect();
        project.add_link(Link::finish_to_start(ids[0], ids[1]));
        project.add_link(Link::finish_to_start(ids[0], ids[2]));
        project.add_link(Link::finish_to_start(ids[1], ids[3]));
        project.add_link(Link::finish_to_start(ids[2], ids[3]));

        schedule(&mut project).unwrap();

        assert!(project.tasks[1].scheduled.critical, "5d branch drives the finish");
        assert!(!project.tasks[2].scheduled.critical, "2d branch has float");
        // The short branch can slip three days before it delays the finish.
        assert_eq!(
            project.tasks[2].scheduled.total_slack_minutes,
            MINUTES_PER_DAY * 3
        );
    }

    #[test]
    fn lag_pushes_the_successor_out() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link {
            predecessor: a,
            successor: b,
            kind: LinkType::FS,
            lag_minutes: MINUTES_PER_DAY * 2,
        });

        schedule(&mut project).unwrap();

        // Task 1 ends Mon 17:00, +2 days of lag, so task 2 starts Thursday.
        assert_eq!(project.tasks[1].scheduled.start, day(2026, 8, 20));
    }

    #[test]
    fn negative_lag_overlaps_the_two_tasks() {
        let mut project = project_with(&[MINUTES_PER_DAY * 5, MINUTES_PER_DAY * 5]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link {
            predecessor: a,
            successor: b,
            kind: LinkType::FS,
            lag_minutes: -MINUTES_PER_DAY * 2,
        });

        schedule(&mut project).unwrap();

        // Task 1 ends Fri 17:00; two days of lead pulls task 2 back to Thursday.
        assert_eq!(project.tasks[1].scheduled.start, day(2026, 8, 20));
    }

    #[test]
    fn start_to_start_aligns_the_two_starts() {
        let mut project = project_with(&[MINUTES_PER_DAY * 4, MINUTES_PER_DAY * 2]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link {
            predecessor: a,
            successor: b,
            kind: LinkType::SS,
            lag_minutes: 0,
        });

        schedule(&mut project).unwrap();

        assert_eq!(
            project.tasks[1].scheduled.start,
            project.tasks[0].scheduled.start
        );
    }

    #[test]
    fn finish_to_finish_aligns_the_two_finishes() {
        let mut project = project_with(&[MINUTES_PER_DAY * 4, MINUTES_PER_DAY * 2]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link {
            predecessor: a,
            successor: b,
            kind: LinkType::FF,
            lag_minutes: 0,
        });

        schedule(&mut project).unwrap();

        assert_eq!(
            project.tasks[1].scheduled.finish,
            project.tasks[0].scheduled.finish
        );
    }

    #[test]
    fn a_summary_spans_its_children() {
        let mut project = Project::blank(day(2026, 8, 17));
        project.push_task("Phase", MINUTES_PER_DAY);
        project.push_task("Child A", MINUTES_PER_DAY * 2);
        project.push_task("Child B", MINUTES_PER_DAY * 3);
        project.tasks[1].outline_level = 1;
        project.tasks[2].outline_level = 1;
        let (a, b) = (project.tasks[1].id, project.tasks[2].id);
        project.add_link(Link::finish_to_start(a, b));

        schedule(&mut project).unwrap();

        assert!(project.is_summary(0));
        assert_eq!(project.tasks[0].scheduled.start, project.tasks[1].scheduled.start);
        assert_eq!(project.tasks[0].scheduled.finish, project.tasks[2].scheduled.finish);
        // Two days then three days of work, rolled up.
        assert_eq!(project.tasks[0].scheduled.duration_minutes, MINUTES_PER_DAY * 5);
    }

    #[test]
    fn a_milestone_has_no_span() {
        let mut project = Project::blank(day(2026, 8, 17));
        project.push_task("Work", MINUTES_PER_DAY * 2);
        let gate = project.allocate_task_id();
        project.tasks.push(Task::milestone(gate, "Gate"));
        let work = project.tasks[0].id;
        project.add_link(Link::finish_to_start(work, gate));

        schedule(&mut project).unwrap();

        let milestone = &project.tasks[1];
        assert!(milestone.is_milestone());
        assert_eq!(milestone.scheduled.start, milestone.scheduled.finish);
        // Work ends Tuesday 17:00, so the gate lands Wednesday 08:00.
        assert_eq!(milestone.scheduled.start, day(2026, 8, 19));
    }

    #[test]
    fn a_milestone_costs_the_chain_nothing() {
        // A -> B, then the same chain with a milestone wedged between them.
        // Both must finish on exactly the same day: a marker takes no time.
        let mut plain = project_with(&[MINUTES_PER_DAY * 2, MINUTES_PER_DAY * 2]);
        let (a, b) = (plain.tasks[0].id, plain.tasks[1].id);
        plain.add_link(Link::finish_to_start(a, b));
        let plain_report = schedule(&mut plain).unwrap();

        let mut gated = project_with(&[MINUTES_PER_DAY * 2]);
        let gate = gated.allocate_task_id();
        gated.tasks.push(Task::milestone(gate, "Gate"));
        let last = gated.push_task("B", MINUTES_PER_DAY * 2);
        let first = gated.tasks[0].id;
        gated.add_link(Link::finish_to_start(first, gate));
        gated.add_link(Link::finish_to_start(gate, last));
        let gated_report = schedule(&mut gated).unwrap();

        assert_eq!(
            plain_report.finish, gated_report.finish,
            "putting a milestone in the chain must not add a day"
        );
        // And the task after the marker starts the same morning the marker is on.
        assert_eq!(
            gated.tasks[1].scheduled.start.date(),
            gated.tasks[2].scheduled.start.date(),
            "work should start on the milestone's own day"
        );
    }

    #[test]
    fn a_milestone_pinned_to_the_end_of_a_day_stays_on_that_day() {
        let mut project = project_with(&[MINUTES_PER_DAY]);
        let gate = project.allocate_task_id();
        let mut marker = Task::milestone(gate, "Gate");
        // This is what typing a date into the Finish column produces.
        marker.constraint = ConstraintType::FinishNoEarlierThan;
        marker.constraint_date = Some(day(2026, 9, 2).with_hour_17());
        project.tasks.push(marker);

        schedule(&mut project).unwrap();

        let marker = &project.tasks[1];
        assert_eq!(
            marker.scheduled.start, marker.scheduled.finish,
            "a milestone occupies a single instant"
        );
        assert_eq!(
            marker.scheduled.start.date(),
            NaiveDate::from_ymd_opt(2026, 9, 2).unwrap(),
            "it must stay on the day it was pinned to, not roll to the next morning"
        );
    }

    #[test]
    fn work_after_an_end_of_day_milestone_starts_the_next_morning() {
        let mut project = project_with(&[MINUTES_PER_DAY]);
        let gate = project.allocate_task_id();
        let mut marker = Task::milestone(gate, "Gate");
        marker.constraint = ConstraintType::FinishNoEarlierThan;
        marker.constraint_date = Some(day(2026, 9, 2).with_hour_17());
        project.tasks.push(marker);
        let after = project.push_task("After", MINUTES_PER_DAY * 2);
        project.add_link(Link::finish_to_start(gate, after));

        schedule(&mut project).unwrap();

        assert_eq!(project.tasks[2].scheduled.start, day(2026, 9, 3));
    }

    #[test]
    fn a_milestone_always_starts_and_finishes_together() {
        // Whichever way it is pinned, the two dates must agree.
        for (constraint, at) in [
            (ConstraintType::StartNoEarlierThan, day(2026, 9, 2)),
            (ConstraintType::FinishNoEarlierThan, day(2026, 9, 2).with_hour_17()),
            (ConstraintType::MustStartOn, day(2026, 9, 2)),
            (ConstraintType::MustFinishOn, day(2026, 9, 2).with_hour_17()),
        ] {
            let mut project = project_with(&[MINUTES_PER_DAY]);
            let gate = project.allocate_task_id();
            let mut marker = Task::milestone(gate, "Gate");
            marker.constraint = constraint;
            marker.constraint_date = Some(at);
            project.tasks.push(marker);

            schedule(&mut project).unwrap();
            let marker = &project.tasks[1];
            assert_eq!(
                marker.scheduled.start, marker.scheduled.finish,
                "{constraint:?} left a milestone with two different dates"
            );
        }
    }

    #[test]
    fn a_start_no_earlier_than_constraint_delays_the_task() {
        let mut project = project_with(&[MINUTES_PER_DAY]);
        project.tasks[0].constraint = ConstraintType::StartNoEarlierThan;
        project.tasks[0].constraint_date = Some(day(2026, 8, 20));

        schedule(&mut project).unwrap();

        assert_eq!(project.tasks[0].scheduled.start, day(2026, 8, 20));
    }

    #[test]
    fn a_missed_deadline_produces_negative_slack() {
        let mut project = project_with(&[MINUTES_PER_DAY * 5]);
        // Five days of work cannot finish by Wednesday.
        project.tasks[0].deadline = Some(day(2026, 8, 19).with_hour_17());

        schedule(&mut project).unwrap();

        assert!(
            project.tasks[0].scheduled.total_slack_minutes < 0,
            "expected negative slack, got {}",
            project.tasks[0].scheduled.total_slack_minutes
        );
    }

    #[test]
    fn a_critical_task_explains_what_it_is_holding_up() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.tasks[1].name = "Handover".into();
        project.add_link(Link::finish_to_start(a, b));
        schedule(&mut project).unwrap();

        let reason = critical_reason(&project, 0).expect("task 1 is critical");
        assert!(reason.contains("Handover"), "should name the successor: {reason}");
        assert!(reason.contains("project finish"), "should mention the finish: {reason}");

        // The last task has nothing after it, so it is described differently.
        let last = critical_reason(&project, 1).expect("task 2 is critical");
        assert!(last.contains("nothing follows"), "{last}");
    }

    #[test]
    fn a_task_with_slack_has_no_reason_to_give() {
        let mut project = project_with(&[
            MINUTES_PER_DAY,
            MINUTES_PER_DAY * 5,
            MINUTES_PER_DAY * 2,
            MINUTES_PER_DAY,
        ]);
        let ids: Vec<TaskId> = project.tasks.iter().map(|t| t.id).collect();
        project.add_link(Link::finish_to_start(ids[0], ids[1]));
        project.add_link(Link::finish_to_start(ids[0], ids[2]));
        project.add_link(Link::finish_to_start(ids[1], ids[3]));
        project.add_link(Link::finish_to_start(ids[2], ids[3]));
        schedule(&mut project).unwrap();

        assert!(critical_reason(&project, 2).is_none(), "the slack branch is not critical");
    }

    #[test]
    fn a_missed_deadline_is_named_as_the_cause() {
        let mut project = project_with(&[MINUTES_PER_DAY * 5]);
        project.tasks[0].deadline = Some(day(2026, 8, 19).with_hour_17());
        schedule(&mut project).unwrap();

        let reason = critical_reason(&project, 0).expect("a missed deadline is critical");
        assert!(reason.contains("Behind by"), "{reason}");
        assert!(reason.contains("deadline"), "{reason}");
    }

    #[test]
    fn a_dependency_loop_is_reported_rather_than_hanging() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let ids: Vec<TaskId> = project.tasks.iter().map(|t| t.id).collect();
        project.add_link(Link::finish_to_start(ids[0], ids[1]));
        project.add_link(Link::finish_to_start(ids[1], ids[2]));
        project.add_link(Link::finish_to_start(ids[2], ids[0]));

        let error = schedule(&mut project).unwrap_err();

        assert!(matches!(error, ScheduleError::CircularDependency(_)));
    }

    #[test]
    fn a_loop_is_diagnosed_with_a_fix_that_works() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let ids: Vec<TaskId> = project.tasks.iter().map(|t| t.id).collect();
        project.add_link(Link::finish_to_start(ids[0], ids[1]));
        project.add_link(Link::finish_to_start(ids[1], ids[2]));
        project.add_link(Link::finish_to_start(ids[2], ids[0]));

        let remedy = diagnose(&project).expect("a loop should be diagnosed");
        assert_eq!(remedy.links_to_remove.len(), 1, "one link should clear it");
        assert!(remedy.problem.contains("loop"), "{}", remedy.problem);
        assert!(!remedy.changes.is_empty());

        // The newest link is the one just added, so that is the one to drop.
        assert_eq!(remedy.links_to_remove[0].predecessor, ids[2]);
        assert_eq!(remedy.links_to_remove[0].successor, ids[0]);

        let removed = apply_remedy(&mut project, &remedy);
        assert_eq!(removed, 1);
        assert!(schedule(&mut project).is_ok(), "the fix must actually work");
    }

    #[test]
    fn a_healthy_plan_has_nothing_to_diagnose() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link::finish_to_start(a, b));
        schedule(&mut project).unwrap();

        assert!(diagnose(&project).is_none());
    }

    #[test]
    fn the_fix_keeps_the_links_that_were_not_at_fault() {
        let mut project = project_with(&[MINUTES_PER_DAY; 4]);
        let ids: Vec<TaskId> = project.tasks.iter().map(|t| t.id).collect();
        // A healthy chain, plus one link that closes a loop.
        project.add_link(Link::finish_to_start(ids[0], ids[1]));
        project.add_link(Link::finish_to_start(ids[1], ids[2]));
        project.add_link(Link::finish_to_start(ids[2], ids[3]));
        project.add_link(Link::finish_to_start(ids[3], ids[1]));

        let remedy = diagnose(&project).unwrap();
        apply_remedy(&mut project, &remedy);

        assert!(schedule(&mut project).is_ok());
        assert_eq!(project.links.len(), 3, "the good chain must survive");
        assert!(project.link_exists(ids[0], ids[1]));
        assert!(project.link_exists(ids[1], ids[2]));
        assert!(project.link_exists(ids[2], ids[3]));
    }

    #[test]
    fn a_manually_scheduled_task_ignores_its_link() {
        let mut project = project_with(&[MINUTES_PER_DAY, MINUTES_PER_DAY]);
        let (a, b) = (project.tasks[0].id, project.tasks[1].id);
        project.add_link(Link::finish_to_start(a, b));
        project.tasks[1].mode = TaskMode::Manual;
        project.tasks[1].manual_start = Some(day(2026, 8, 17));

        schedule(&mut project).unwrap();

        assert_eq!(project.tasks[1].scheduled.start, day(2026, 8, 17));
    }

    #[test]
    fn work_and_cost_follow_the_assignment_units() {
        let mut project = project_with(&[MINUTES_PER_DAY * 2]);
        let resource = project.allocate_resource_id();
        project
            .resources
            .push(crate::model::Resource::new(resource, "Ana Reyes").with_rate(100.0));
        project.tasks[0].assignments.push(crate::model::Assignment {
            resource,
            units: 0.5,
        });

        let report = schedule(&mut project).unwrap();

        // Half of two 8-hour days is 8 hours of work at $100/hr.
        assert_eq!(report.total_work_minutes, 480);
        assert!((report.total_cost - 800.0).abs() < 1e-6);
    }

    #[test]
    fn booking_one_person_twice_over_is_flagged() {
        let mut project = project_with(&[MINUTES_PER_DAY * 3, MINUTES_PER_DAY * 3]);
        let resource = project.allocate_resource_id();
        project
            .resources
            .push(crate::model::Resource::new(resource, "Ana Reyes"));
        for task in &mut project.tasks {
            task.assignments.push(crate::model::Assignment {
                resource,
                units: 1.0,
            });
        }

        let report = schedule(&mut project).unwrap();

        assert_eq!(report.overallocations.len(), 1);
        assert!((report.overallocations[0].peak_units - 2.0).abs() < 1e-9);
    }

    /// Small readability helper: the 17:00 end of a working day.
    trait AtFive {
        fn with_hour_17(self) -> NaiveDateTime;
    }
    impl AtFive for NaiveDateTime {
        fn with_hour_17(self) -> NaiveDateTime {
            self.date().and_hms_opt(17, 0, 0).unwrap()
        }
    }
}

#[cfg(test)]
mod external_tests {
    use crate::model::{ExternalDependency, Project, Task, TaskMode};
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    fn plan() -> Project {
        let mut project = Project::blank(at(2026, 9, 7, 8));
        project.tasks.clear();
        let id = project.allocate_task_id();
        project.tasks.push(Task::new(id, "Fit the frame", 480));
        project
    }

    fn waiting_on(project: &mut Project, available: chrono::NaiveDateTime) {
        let id = project.allocate_external_id();
        project.external.push(ExternalDependency {
            id,
            reference: "PO-4471".into(),
            label: "Steel delivery".into(),
            source: "ERP".into(),
            available,
            notes: String::new(),
        });
        project.tasks[0].external_predecessors.push(id);
    }

    #[test]
    fn work_cannot_start_before_what_it_waits_on_arrives() {
        let mut project = plan();
        waiting_on(&mut project, at(2026, 9, 21, 8));
        crate::schedule(&mut project).unwrap();
        assert!(project.tasks[0].scheduled.start >= at(2026, 9, 21, 8));
    }

    #[test]
    fn an_arrival_already_past_holds_nothing_up() {
        // A delivery that landed last month is not a reason to move work.
        let mut project = plan();
        waiting_on(&mut project, at(2026, 8, 1, 8));
        crate::schedule(&mut project).unwrap();
        assert_eq!(project.tasks[0].scheduled.start, at(2026, 9, 7, 8));
    }

    #[test]
    fn the_latest_of_several_arrivals_is_what_counts() {
        let mut project = plan();
        waiting_on(&mut project, at(2026, 9, 14, 8));
        waiting_on(&mut project, at(2026, 10, 5, 8));
        crate::schedule(&mut project).unwrap();
        assert!(project.tasks[0].scheduled.start >= at(2026, 10, 5, 8));
    }

    #[test]
    fn a_manually_placed_task_is_left_where_the_planner_put_it() {
        // Consistent with how links behave: manual means manual.
        let mut project = plan();
        project.tasks[0].mode = TaskMode::Manual;
        project.tasks[0].manual_start = Some(at(2026, 9, 7, 8));
        waiting_on(&mut project, at(2026, 12, 1, 8));
        crate::schedule(&mut project).unwrap();
        assert_eq!(project.tasks[0].scheduled.start, at(2026, 9, 7, 8));
    }

    #[test]
    fn a_reference_no_longer_in_the_plan_is_ignored_rather_than_fatal() {
        // Deleting the dependency must not stop the plan from scheduling.
        let mut project = plan();
        waiting_on(&mut project, at(2026, 12, 1, 8));
        project.external.clear();
        assert!(crate::schedule(&mut project).is_ok());
        assert_eq!(project.tasks[0].scheduled.start, at(2026, 9, 7, 8));
    }

    #[test]
    fn identifiers_are_never_reused_after_a_deletion() {
        let mut project = plan();
        waiting_on(&mut project, at(2026, 9, 21, 8));
        let first = project.external[0].id;
        let second = project.allocate_external_id();
        assert_ne!(first, second, "a reused id would rewire a task silently");
    }
}

/// One step along the critical path.
#[derive(Debug, Clone, PartialEq)]
pub struct CriticalStep {
    /// Row index in the plan.
    pub index: usize,
    /// How this task is tied to the one before it, if there is one.
    pub link_from_previous: Option<LinkType>,
    pub lag_minutes: i64,
}

/// The critical path: the longest chain of dependent tasks that sets the
/// project's finish date.
///
/// This is not the same as "every task with zero slack". Those tasks are the
/// candidates, but the path is the sequence through them, joined by their
/// links. A plan can hold several zero-slack chains running in parallel, and
/// only the longest one determines the finish, so that is the one returned.
///
/// Summary rows are left out: their span is rolled up from their children, so
/// including them would put the same time on the path twice.
pub fn critical_path(project: &Project) -> Vec<CriticalStep> {
    // What the scheduler worked out, not what the warning list chooses to
    // show. `shows_as_critical` also hides tasks whose critical warning the
    // planner has dismissed, and dismissing a warning says nothing about the
    // schedule: letting it reach the algorithm broke the chain wherever one
    // had been dismissed, and the report then drew a fragment of the path.
    let candidates: Vec<usize> = (0..project.tasks.len())
        .filter(|&index| {
            !project.is_summary(index)
                && project.tasks[index].active
                && project.tasks[index].scheduled.critical
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    let position: HashMap<usize, usize> = candidates
        .iter()
        .enumerate()
        .map(|(slot, &index)| (index, slot))
        .collect();

    // Links that run between two candidates, which is what makes a chain
    // rather than a list.
    let mut incoming: HashMap<usize, Vec<(usize, Link)>> = HashMap::new();
    for link in &project.links {
        let (Some(from), Some(to)) = (
            project.index_of(link.predecessor),
            project.index_of(link.successor),
        ) else {
            continue;
        };
        if position.contains_key(&from) && position.contains_key(&to) {
            incoming.entry(to).or_default().push((from, *link));
        }
    }

    // Longest chain ending at each task, measured in duration so that the
    // path returned is the one that actually sets the finish date. The plan is
    // already known to be acyclic here, since it scheduled.
    let mut order = candidates.clone();
    order.sort_by_key(|&index| project.tasks[index].scheduled.start);

    let mut best: HashMap<usize, i64> = HashMap::new();
    let mut came_from: HashMap<usize, (usize, Link)> = HashMap::new();

    for &index in &order {
        let own = project.tasks[index].scheduled.duration_minutes.max(0);
        let mut longest = own;
        for (from, link) in incoming.get(&index).into_iter().flatten() {
            let through = best.get(from).copied().unwrap_or(0) + own;
            if through > longest {
                longest = through;
                came_from.insert(index, (*from, *link));
            }
        }
        best.insert(index, longest);
    }

    // Walk back from wherever the longest chain ended.
    let Some((&tail, _)) = best.iter().max_by_key(|(index, length)| {
        // Ties broken by the later finish, so the path ends where the project
        // does rather than at whichever row happened to hash first.
        (**length, project.tasks[**index].scheduled.finish)
    }) else {
        return Vec::new();
    };

    let mut chain = Vec::new();
    let mut cursor = tail;
    loop {
        let step = came_from.get(&cursor).copied();
        chain.push(CriticalStep {
            index: cursor,
            link_from_previous: step.map(|(_, link)| link.kind),
            lag_minutes: step.map(|(_, link)| link.lag_minutes).unwrap_or(0),
        });
        match step {
            Some((previous, _)) => cursor = previous,
            None => break,
        }
        // A malformed graph must not spin here.
        if chain.len() > project.tasks.len() {
            break;
        }
    }

    chain.reverse();
    chain
}

/// How long the critical path runs, in working minutes.
pub fn critical_path_minutes(project: &Project, path: &[CriticalStep]) -> i64 {
    path.iter()
        .map(|step| project.tasks[step.index].scheduled.duration_minutes.max(0))
        .sum()
}

#[cfg(test)]
mod critical_path_tests {
    use super::*;
    use crate::model::{LinkType, Task};
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    /// A chain of `durations`, each linked to the one before it, plus any
    /// extra standalone tasks.
    fn chain(durations: &[i64]) -> Project {
        let mut project = Project::blank(at(2026, 1, 5));
        project.tasks.clear();
        let mut previous: Option<crate::model::TaskId> = None;
        for (index, minutes) in durations.iter().enumerate() {
            let id = project.allocate_task_id();
            project
                .tasks
                .push(Task::new(id, format!("Task {}", index + 1), *minutes));
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
        project
    }

    #[test]
    fn the_path_is_the_chain_in_order_not_a_bag_of_tasks() {
        let mut project = chain(&[480, 480, 480]);
        crate::schedule(&mut project).unwrap();

        let path = critical_path(&project);
        assert_eq!(path.len(), 3);
        assert_eq!(
            path.iter().map(|s| s.index).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "it has to read start to finish"
        );
        assert!(path[0].link_from_previous.is_none(), "the first has nothing before it");
        assert_eq!(path[1].link_from_previous, Some(LinkType::FS));
    }

    #[test]
    fn the_longest_chain_wins_when_two_run_in_parallel() {
        // Both are zero slack in their own right; only one sets the finish.
        let mut project = Project::blank(at(2026, 1, 5));
        project.tasks.clear();
        let mut ids = Vec::new();
        for (name, minutes) in [("A1", 480), ("A2", 4800), ("B1", 480)] {
            let id = project.allocate_task_id();
            project.tasks.push(Task::new(id, name, minutes));
            ids.push(id);
        }
        project.links.push(Link {
            predecessor: ids[0],
            successor: ids[1],
            kind: LinkType::FS,
            lag_minutes: 0,
        });
        crate::schedule(&mut project).unwrap();

        let path = critical_path(&project);
        let names: Vec<&str> = path
            .iter()
            .map(|s| project.tasks[s.index].name.as_str())
            .collect();
        assert!(names.contains(&"A2"), "the long chain is the path");
        assert!(!names.contains(&"B1"), "the short standalone one is not");
    }

    #[test]
    fn a_plan_with_no_critical_tasks_has_no_path() {
        let mut project = chain(&[480]);
        crate::schedule(&mut project).unwrap();
        project.tasks[0].scheduled.critical = false;
        assert!(critical_path(&project).is_empty());
    }

    #[test]
    fn an_acknowledged_task_stays_on_the_path() {
        // Dismissing a critical warning is about the warning list and the
        // colour a bar is drawn in. It is not a statement about the schedule,
        // so the path itself must not change: a chain with a piece missing is
        // not a shorter critical path, it is a wrong one.
        let mut project = chain(&[480, 480]);
        crate::schedule(&mut project).unwrap();
        assert_eq!(critical_path(&project).len(), 2);

        crate::issues::ignore(&mut project, 1, crate::model::IssueKind::Critical);
        let path = critical_path(&project);
        assert_eq!(path.len(), 2, "the chain is unchanged");
        assert!(
            path.iter().any(|s| s.index == 1),
            "the dismissed task is still on the path it is on"
        );

        // The colouring is where it does take effect, and still does.
        assert!(
            !crate::issues::shows_as_critical(&project, 1),
            "the bar stops being drawn as critical, which is the whole effect"
        );
    }

    #[test]
    fn the_path_length_is_the_sum_of_its_steps() {
        let mut project = chain(&[480, 960]);
        crate::schedule(&mut project).unwrap();
        let path = critical_path(&project);
        assert_eq!(critical_path_minutes(&project, &path), 1440);
    }

    #[test]
    fn a_summary_row_is_never_a_step_on_the_path() {
        // Its span is its children's, so it would put the same time on twice.
        let mut project = chain(&[480, 480]);
        project.tasks[0].outline_level = 0;
        project.tasks[1].outline_level = 1;
        crate::schedule(&mut project).unwrap();

        let path = critical_path(&project);
        assert!(path.iter().all(|step| !project.is_summary(step.index)));
    }
}

// ------------------------------------------------------------ updating

/// What an update run should do to the plan.
///
/// The two halves of Project's Update Project dialog. They answer different
/// questions: the first says "assume everything went to plan up to here", the
/// second says "nothing more happened before here, move it".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    /// Mark work complete as though the plan had been followed exactly up to
    /// this date. A task finished before it becomes fully complete; one
    /// straddling it becomes part complete in proportion to the working time
    /// that has passed.
    CompleteThrough(NaiveDateTime),
    /// Move work that has not started to begin after this date, for when a
    /// plan has been left alone and its early tasks are now in the past.
    RescheduleAfter(NaiveDateTime),
}

/// Which tasks an update applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateScope {
    EntireProject,
    /// Only these rows, for updating one branch at a time.
    Selected,
}

/// Apply an update, returning how many tasks it changed.
///
/// Summary rows are skipped throughout: their progress is their children's, so
/// setting it directly would be overwritten by the next reschedule anyway.
pub fn update_project(
    project: &mut Project,
    update: Update,
    scope: UpdateScope,
    selected: &[usize],
) -> usize {
    let calendar = project.calendar.clone();
    let mut changed = 0;

    let rows: Vec<usize> = (0..project.tasks.len())
        .filter(|index| !project.is_summary(*index) && project.tasks[*index].active)
        .filter(|index| match scope {
            UpdateScope::EntireProject => true,
            UpdateScope::Selected => selected.contains(index),
        })
        .collect();

    for index in rows {
        match update {
            Update::CompleteThrough(through) => {
                let task = &project.tasks[index];
                let percent = if task.scheduled.finish <= through {
                    100
                } else if task.scheduled.start >= through {
                    // Not started by this date, so nothing to report.
                    continue;
                } else {
                    // Part done, in proportion to the working time elapsed
                    // rather than the wall clock, so a weekend does not count
                    // as progress.
                    let done = calendar.work_minutes_between(task.scheduled.start, through);
                    let total = calendar
                        .work_minutes_between(task.scheduled.start, task.scheduled.finish)
                        .max(1);
                    ((done * 100 / total).clamp(0, 100)) as u8
                };

                if project.tasks[index].percent_complete != percent {
                    project.tasks[index].percent_complete = percent;
                    changed += 1;
                }
            }
            Update::RescheduleAfter(after) => {
                let task = &project.tasks[index];
                // Only work that has not started. Something already under way
                // is not moved, because its start actually happened.
                if task.percent_complete > 0 || task.scheduled.start >= after {
                    continue;
                }
                let task = &mut project.tasks[index];
                task.constraint = ConstraintType::StartNoEarlierThan;
                task.constraint_date = Some(after);
                changed += 1;
            }
        }
    }

    changed
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use crate::model::Task;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    /// Three one-day tasks in a row from Monday.
    fn plan() -> Project {
        let mut project = Project::blank(at(2026, 1, 5, 8));
        project.tasks.clear();
        let mut previous = None;
        for name in ["One", "Two", "Three"] {
            let id = project.allocate_task_id();
            project.tasks.push(Task::new(id, name, 480));
            if let Some(from) = previous {
                project.links.push(Link {
                    predecessor: from,
                    successor: id,
                    kind: crate::model::LinkType::FS,
                    lag_minutes: 0,
                });
            }
            previous = Some(id);
        }
        crate::schedule(&mut project).unwrap();
        project
    }

    #[test]
    fn work_finished_before_the_date_is_marked_complete() {
        let mut project = plan();
        let changed = update_project(
            &mut project,
            Update::CompleteThrough(at(2026, 1, 6, 17)),
            UpdateScope::EntireProject,
            &[],
        );
        assert!(changed >= 2);
        assert_eq!(project.tasks[0].percent_complete, 100);
        assert_eq!(project.tasks[1].percent_complete, 100);
    }

    #[test]
    fn work_not_yet_started_is_left_alone() {
        let mut project = plan();
        update_project(
            &mut project,
            Update::CompleteThrough(at(2026, 1, 5, 17)),
            UpdateScope::EntireProject,
            &[],
        );
        assert_eq!(project.tasks[2].percent_complete, 0, "it has not begun");
    }

    #[test]
    fn a_task_straddling_the_date_is_part_complete() {
        let mut project = plan();
        project.tasks[0].duration_minutes = 960;
        crate::schedule(&mut project).unwrap();

        // Half a two day task.
        update_project(
            &mut project,
            Update::CompleteThrough(at(2026, 1, 5, 17)),
            UpdateScope::EntireProject,
            &[],
        );
        let percent = project.tasks[0].percent_complete;
        assert!((40..=60).contains(&percent), "got {percent}");
    }

    #[test]
    fn progress_counts_working_time_not_the_wall_clock() {
        // A weekend passing is not work getting done.
        let mut project = Project::blank(at(2026, 1, 9, 8));
        project.tasks.clear();
        let id = project.allocate_task_id();
        project.tasks.push(Task::new(id, "Over the weekend", 960));
        crate::schedule(&mut project).unwrap();

        // Friday start, measured to Monday morning: one working day has passed.
        update_project(
            &mut project,
            Update::CompleteThrough(at(2026, 1, 12, 8)),
            UpdateScope::EntireProject,
            &[],
        );
        let percent = project.tasks[0].percent_complete;
        assert!((40..=60).contains(&percent), "two days of weekend counted, got {percent}");
    }

    #[test]
    fn rescheduling_moves_only_work_that_never_started() {
        let mut project = plan();
        project.tasks[0].percent_complete = 50;

        let after = at(2026, 2, 2, 8);
        update_project(
            &mut project,
            Update::RescheduleAfter(after),
            UpdateScope::EntireProject,
            &[],
        );
        crate::schedule(&mut project).unwrap();

        assert!(
            project.tasks[0].scheduled.start < after,
            "work already under way is not moved"
        );
        assert!(project.tasks[1].scheduled.start >= after);
    }

    #[test]
    fn an_update_can_be_limited_to_the_rows_chosen() {
        let mut project = plan();
        update_project(
            &mut project,
            Update::CompleteThrough(at(2026, 1, 8, 17)),
            UpdateScope::Selected,
            &[1],
        );
        assert_eq!(project.tasks[0].percent_complete, 0, "not selected");
        assert_eq!(project.tasks[1].percent_complete, 100);
    }

    #[test]
    fn summary_rows_are_left_to_their_children() {
        let mut project = plan();
        project.tasks[0].outline_level = 0;
        project.tasks[1].outline_level = 1;
        crate::schedule(&mut project).unwrap();

        update_project(
            &mut project,
            Update::CompleteThrough(at(2026, 2, 1, 17)),
            UpdateScope::EntireProject,
            &[],
        );
        assert!(project.is_summary(0));
        assert_eq!(
            project.tasks[0].percent_complete, 0,
            "setting it directly would be overwritten by the next reschedule"
        );
    }
}
