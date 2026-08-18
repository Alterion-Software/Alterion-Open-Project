//! What is different between two plans, and how to make one become the other.
//!
//! This answers two questions with one type. Compare Projects asks it of two
//! files a planner picked; a sync asks it of the copy it holds and the copy it
//! is about to accept, so somebody can see what is about to happen before it
//! does. Both want the same list, so there is one list.
//!
//! It is also the wire format for live collaboration, which is the reason
//! `apply` lives here. The change log holds commands, because a command is
//! what a person reads and what a macro replays. A command is the wrong thing
//! to send, because its result depends on the plan it ran against: levelling
//! two plans that differ by one task produces two different answers, and two
//! clients that each recompute `level()` diverge without either noticing. So
//! the log keeps commands and the wire carries effects. Two formats, two jobs.
//!
//! Three rules shape the whole module:
//!
//! - Tasks are matched by `TaskId`, never by row. A task that moved from row 4
//!   to row 9 moved; it was not deleted and replaced. Matching by row is the
//!   classic way a diff becomes useless, because a single insert at the top
//!   makes every row below it look changed.
//! - Only what a planner authored is reported. If a duration changed, the
//!   finish moving is a consequence, not a second change. See `AUTHORED`.
//! - The order is decided explicitly. Nothing here iterates a `HashMap`.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::duration::parse_duration;
use crate::fields::Field;
use crate::model::{
    Assignment, ConstraintType, Link, LinkType, Project, Resource, ResourceId, ResourceKind, Task,
    TaskId, TaskMode,
};

/// The date form a difference carries.
///
/// Deliberately not the planner's chosen display format. A difference is read
/// by a dialog, but it is also parsed back by `apply` on a machine that may
/// have picked a different format entirely, and `%d/%m/%y` throws away the
/// time of day on the way through. This one is unambiguous and lossless, so a
/// value survives the trip. Anything drawing these in the planner's own format
/// has the task id and the field, which is everything it needs to re-read the
/// value from the plan instead.
pub const DATE_FORMAT: &str = "%Y-%m-%d %H:%M";

/// The fields a difference is reported for: the ones somebody typed or chose.
///
/// Everything absent from this list is absent on purpose, and the reason is
/// always the same: it is worked out from something else, so reporting it says
/// the same edit twice. Somebody will eventually wonder where Start and Finish
/// went, so, in full:
///
/// - `Start`, `Finish`, `LateStart`, `LateFinish`, `TotalSlack`, `FreeSlack`
///   and `Critical` are the scheduler's answers. Lengthening a task moves its
///   own finish, its successors' starts, and the slack on half the plan. One
///   duration edit would read as forty changes, thirty-nine of which nobody
///   made. `Start` is the one exception and only for a manually scheduled
///   task, where it is typed rather than computed.
/// - `Work` and `Cost` fall out of duration, units and rates. `FixedCost` is
///   typed, so it is here.
/// - `Id`, `Wbs`, `Summary` and `Milestone` are read off position, outline and
///   duration. A move is already reported as a move.
/// - `Predecessors` and `Successors` are the link list rendered as text. Links
///   have their own differences, which say which link and how.
/// - `ResourceNames` and `ResourceInitials` are the assignment list rendered
///   as text, and assignments have their own differences for the same reason.
/// - The `Baseline*` fields and the variances against them are a snapshot of
///   what the schedule said at one moment. They change because Set Baseline
///   was run, which is one action on the whole plan, not two hundred edits.
/// - `ActualWork`, `ActualCost` and `RemainingWork` fall back to what percent
///   complete implies when nobody has typed a figure, so a progress edit would
///   be reported once as the progress and three more times as its shadow.
///   `ActualStart`, `ActualFinish` and `PhysicalPercentComplete` read what was
///   really entered and nothing else, so they are here.
/// - Earned value is arithmetic over the baseline and the actuals.
/// - Colours, fonts and the collapsed flag are how somebody is looking at the
///   plan, not what the plan says.
///
/// Listed in `Field::ALL` order, so a task's differences come out in the order
/// its columns do.
const AUTHORED: [Field; 15] = [
    Field::OutlineLevel,
    Field::TaskMode,
    Field::Name,
    Field::Duration,
    Field::Start,
    Field::ConstraintType,
    Field::ConstraintDate,
    Field::Deadline,
    Field::PercentComplete,
    Field::Active,
    Field::FixedCost,
    Field::ActualStart,
    Field::ActualFinish,
    Field::PhysicalPercentComplete,
    Field::Notes,
];

/// One field of a task, with its value as text.
///
/// Named rather than a `(Field, String)` pair because this goes over a network
/// to a client that may be a version behind, and a positional tuple is the
/// kind of thing that gets an extra element added to it one day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldText {
    pub field: Field,
    pub text: String,
}

/// A property of a resource. The task table has no columns for these, so they
/// are named here rather than borrowed from `Field`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceField {
    Name,
    Initials,
    Kind,
    Group,
    MaxUnits,
    StandardRate,
    OvertimeRate,
    CostPerUse,
    BaseCalendar,
    Notes,
    Email,
    Code,
}

impl ResourceField {
    pub const ALL: [ResourceField; 12] = [
        ResourceField::Name,
        ResourceField::Initials,
        ResourceField::Kind,
        ResourceField::Group,
        ResourceField::MaxUnits,
        ResourceField::StandardRate,
        ResourceField::OvertimeRate,
        ResourceField::CostPerUse,
        ResourceField::BaseCalendar,
        ResourceField::Notes,
        ResourceField::Email,
        ResourceField::Code,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ResourceField::Name => "Name",
            ResourceField::Initials => "Initials",
            ResourceField::Kind => "Type",
            ResourceField::Group => "Group",
            ResourceField::MaxUnits => "Max Units",
            ResourceField::StandardRate => "Standard Rate",
            ResourceField::OvertimeRate => "Overtime Rate",
            ResourceField::CostPerUse => "Cost Per Use",
            ResourceField::BaseCalendar => "Base Calendar",
            ResourceField::Notes => "Notes",
            ResourceField::Email => "Email",
            ResourceField::Code => "Code",
        }
    }

    pub fn read(self, resource: &Resource) -> String {
        match self {
            ResourceField::Name => resource.name.clone(),
            ResourceField::Initials => resource.initials.clone(),
            ResourceField::Kind => resource.kind.label().to_string(),
            ResourceField::Group => resource.group.clone(),
            ResourceField::MaxUnits => format!("{:.2}", resource.max_units),
            ResourceField::StandardRate => format!("{:.2}", resource.standard_rate),
            ResourceField::OvertimeRate => format!("{:.2}", resource.overtime_rate),
            ResourceField::CostPerUse => format!("{:.2}", resource.cost_per_use),
            ResourceField::BaseCalendar => resource.base_calendar.clone(),
            ResourceField::Notes => resource.notes.clone(),
            ResourceField::Email => resource.email.clone(),
            ResourceField::Code => resource.code.clone(),
        }
    }

    /// Put a value back. False means the text could not be read, which the
    /// caller reports rather than guessing at.
    fn write(self, resource: &mut Resource, text: &str) -> bool {
        let number = || text.trim().parse::<f64>().ok();
        match self {
            ResourceField::Name => resource.name = text.to_string(),
            ResourceField::Initials => resource.initials = text.to_string(),
            ResourceField::Kind => {
                match ResourceKind::ALL.iter().find(|kind| kind.label() == text) {
                    Some(&kind) => resource.kind = kind,
                    None => return false,
                }
            }
            ResourceField::Group => resource.group = text.to_string(),
            ResourceField::MaxUnits => match number() {
                Some(value) => resource.max_units = value,
                None => return false,
            },
            ResourceField::StandardRate => match number() {
                Some(value) => resource.standard_rate = value,
                None => return false,
            },
            ResourceField::OvertimeRate => match number() {
                Some(value) => resource.overtime_rate = value,
                None => return false,
            },
            ResourceField::CostPerUse => match number() {
                Some(value) => resource.cost_per_use = value,
                None => return false,
            },
            ResourceField::BaseCalendar => resource.base_calendar = text.to_string(),
            ResourceField::Notes => resource.notes = text.to_string(),
            ResourceField::Email => resource.email = text.to_string(),
            ResourceField::Code => resource.code = text.to_string(),
        }
        true
    }
}

/// One thing that is not the same between two plans.
///
/// Every variant names its subject by identifier and carries the name as well,
/// so a dialog can render a line without holding either plan, and a receiver
/// can act on it without guessing which row was meant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Difference {
    /// A task the second plan has and the first does not.
    ///
    /// `values` carries the authored fields of the new task, which is what
    /// makes this enough to recreate it. `at` is its row in the second plan.
    TaskAdded {
        id: TaskId,
        name: String,
        at: usize,
        values: Vec<FieldText>,
    },
    TaskRemoved {
        id: TaskId,
        name: String,
    },
    /// A task that sits in a different place relative to the tasks around it.
    ///
    /// `from` and `to` are its rows in the two plans, for a person to read.
    /// `apply` finds the task by id, because by the time a batch of moves is
    /// half done the old row numbers no longer mean anything.
    TaskMoved {
        id: TaskId,
        name: String,
        from: usize,
        to: usize,
    },
    FieldChanged {
        id: TaskId,
        name: String,
        field: Field,
        before: String,
        after: String,
    },
    LinkAdded {
        predecessor: TaskId,
        successor: TaskId,
        kind: LinkType,
        lag_minutes: i64,
    },
    LinkRemoved {
        predecessor: TaskId,
        successor: TaskId,
        kind: LinkType,
    },
    LinkChanged {
        predecessor: TaskId,
        successor: TaskId,
        before_kind: LinkType,
        after_kind: LinkType,
        before_lag_minutes: i64,
        after_lag_minutes: i64,
    },
    ResourceAdded {
        id: ResourceId,
        name: String,
        values: Vec<ResourceText>,
    },
    ResourceRemoved {
        id: ResourceId,
        name: String,
    },
    ResourceChanged {
        id: ResourceId,
        name: String,
        field: ResourceField,
        before: String,
        after: String,
    },
    AssignmentAdded {
        task: TaskId,
        task_name: String,
        resource: ResourceId,
        resource_name: String,
        units: f64,
    },
    AssignmentRemoved {
        task: TaskId,
        task_name: String,
        resource: ResourceId,
        resource_name: String,
    },
    AssignmentChanged {
        task: TaskId,
        task_name: String,
        resource: ResourceId,
        resource_name: String,
        before_units: f64,
        after_units: f64,
    },
}

/// One property of a resource, with its value as text. The resource twin of
/// `FieldText`, and named for the same reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceText {
    pub field: ResourceField,
    pub text: String,
}

impl Difference {
    /// The task this belongs to, where it belongs to one.
    ///
    /// A link is filed under its successor, because that is the row whose
    /// Predecessors cell it shows up in. Resource differences belong to the
    /// plan rather than to any task.
    pub fn task(&self) -> Option<TaskId> {
        match self {
            Difference::TaskAdded { id, .. }
            | Difference::TaskRemoved { id, .. }
            | Difference::TaskMoved { id, .. }
            | Difference::FieldChanged { id, .. } => Some(*id),
            Difference::LinkAdded { successor, .. }
            | Difference::LinkRemoved { successor, .. }
            | Difference::LinkChanged { successor, .. } => Some(*successor),
            Difference::AssignmentAdded { task, .. }
            | Difference::AssignmentRemoved { task, .. }
            | Difference::AssignmentChanged { task, .. } => Some(*task),
            Difference::ResourceAdded { .. }
            | Difference::ResourceRemoved { .. }
            | Difference::ResourceChanged { .. } => None,
        }
    }

    /// The name of whatever this is about, for a one line rendering.
    pub fn subject(&self) -> &str {
        match self {
            Difference::TaskAdded { name, .. }
            | Difference::TaskRemoved { name, .. }
            | Difference::TaskMoved { name, .. }
            | Difference::FieldChanged { name, .. }
            | Difference::ResourceAdded { name, .. }
            | Difference::ResourceRemoved { name, .. }
            | Difference::ResourceChanged { name, .. } => name,
            Difference::AssignmentAdded { task_name, .. }
            | Difference::AssignmentRemoved { task_name, .. }
            | Difference::AssignmentChanged { task_name, .. } => task_name,
            Difference::LinkAdded { .. }
            | Difference::LinkRemoved { .. }
            | Difference::LinkChanged { .. } => "",
        }
    }
}

// ---- comparing ----------------------------------------------------------

/// Everything that is different between two plans, in a fixed order.
///
/// Tasks first in the order the second plan lists them, then the tasks the
/// second plan no longer has, then links, then resources. Within one task, its
/// own differences come before its links and its assignments.
pub fn compare(before: &Project, after: &Project) -> Vec<Difference> {
    let mut out = Vec::new();
    task_differences(before, after, &mut out);
    link_differences(before, after, &mut out);
    resource_differences(before, after, &mut out);
    out
}

fn rows(project: &Project) -> HashMap<TaskId, usize> {
    project
        .tasks
        .iter()
        .enumerate()
        .map(|(index, task)| (task.id, index))
        .collect()
}

/// Which tasks present in both plans genuinely moved.
///
/// Comparing row numbers directly reports the whole plan as moved the moment
/// somebody inserts a row at the top, because every row below it shifts by
/// one. What matters is relative order: the longest run of tasks still in the
/// same order as each other did not move, and everything else did. That run is
/// the longest increasing subsequence of the first plan's positions read in
/// the second plan's order, which is also the smallest set of moves that
/// explains the reordering.
fn moved_tasks(before: &Project, after: &Project) -> BTreeSet<TaskId> {
    let in_after: HashSet<TaskId> = after.tasks.iter().map(|task| task.id).collect();

    // Position among the tasks the two plans share, in the first plan's order.
    let mut rank: HashMap<TaskId, usize> = HashMap::new();
    for task in before.tasks.iter().filter(|task| in_after.contains(&task.id)) {
        let next = rank.len();
        rank.insert(task.id, next);
    }

    let sequence: Vec<(TaskId, usize)> = after
        .tasks
        .iter()
        .filter_map(|task| rank.get(&task.id).map(|&at| (task.id, at)))
        .collect();

    // Patience sorting: `piles` holds, for each length, the index of the
    // smallest value ending an increasing run of that length, and `came_from`
    // chains a run back to its start.
    let mut piles: Vec<usize> = Vec::new();
    let mut came_from: Vec<Option<usize>> = vec![None; sequence.len()];
    for (at, &(_, value)) in sequence.iter().enumerate() {
        let pile = piles.partition_point(|&held| sequence[held].1 < value);
        came_from[at] = pile.checked_sub(1).map(|previous| piles[previous]);
        match piles.get_mut(pile) {
            Some(slot) => *slot = at,
            None => piles.push(at),
        }
    }

    let mut stayed: HashSet<TaskId> = HashSet::new();
    let mut cursor = piles.last().copied();
    while let Some(at) = cursor {
        stayed.insert(sequence[at].0);
        cursor = came_from[at];
    }

    sequence
        .iter()
        .filter(|(id, _)| !stayed.contains(id))
        .map(|(id, _)| *id)
        .collect()
}

/// The fields worth comparing for one task, given how it sits in the plans it
/// appears in.
fn authored_fields(sides: &[(&Project, usize)]) -> Vec<Field> {
    let summary = sides.iter().any(|(plan, index)| plan.is_summary(*index));
    let manual = sides.iter().any(|(plan, index)| {
        plan.tasks
            .get(*index)
            .is_some_and(|task| task.mode == TaskMode::Manual)
    });

    AUTHORED
        .iter()
        .copied()
        .filter(|field| match field {
            // A summary's duration is the span of its children, so a change to
            // it belongs to whichever child caused it, not here.
            Field::Duration => !summary,
            // An auto scheduled task's start is the scheduler's answer. A
            // manually scheduled one's is what somebody typed, and this is the
            // only place that typing shows.
            Field::Start => manual,
            _ => true,
        })
        .collect()
}

/// Read a field as the text a difference carries.
fn read(project: &Project, index: usize, field: Field) -> String {
    match field {
        // The grid flattens a note to one line so it fits in a cell. A
        // difference that did the same would drop the line breaks on the way
        // through an apply, so this reads what is actually stored and leaves
        // flattening to whoever draws it.
        Field::Notes => project
            .tasks
            .get(index)
            .map(|task| task.notes.clone())
            .unwrap_or_default(),
        other => other.value(project, index, DATE_FORMAT),
    }
}

fn task_differences(before: &Project, after: &Project, out: &mut Vec<Difference>) {
    let was = rows(before);
    let now = rows(after);
    let moved = moved_tasks(before, after);

    for (index, task) in after.tasks.iter().enumerate() {
        let Some(&previous) = was.get(&task.id) else {
            let values = authored_fields(&[(after, index)])
                .into_iter()
                .filter(|field| *field != Field::Name)
                .map(|field| FieldText {
                    field,
                    text: read(after, index, field),
                })
                .collect();
            out.push(Difference::TaskAdded {
                id: task.id,
                name: task.name.clone(),
                at: index,
                values,
            });
            continue;
        };

        if moved.contains(&task.id) {
            out.push(Difference::TaskMoved {
                id: task.id,
                name: task.name.clone(),
                from: previous,
                to: index,
            });
        }

        for field in authored_fields(&[(before, previous), (after, index)]) {
            let old = read(before, previous, field);
            let new = read(after, index, field);
            if old != new {
                out.push(Difference::FieldChanged {
                    id: task.id,
                    name: task.name.clone(),
                    field,
                    before: old,
                    after: new,
                });
            }
        }

        assignment_differences(before, previous, after, index, out);
    }

    // Removals last, as a block. They have no row in the second plan to sit
    // beside, and a dialog reads better for listing them together than for
    // slotting each one into a gap that is no longer there.
    for task in before.tasks.iter().filter(|task| !now.contains_key(&task.id)) {
        out.push(Difference::TaskRemoved {
            id: task.id,
            name: task.name.clone(),
        });
    }
}

fn assignment_differences(
    before: &Project,
    before_index: usize,
    after: &Project,
    after_index: usize,
    out: &mut Vec<Difference>,
) {
    let (Some(was), Some(now)) = (
        before.tasks.get(before_index),
        after.tasks.get(after_index),
    ) else {
        return;
    };

    let booked: BTreeSet<ResourceId> = was
        .assignments
        .iter()
        .chain(now.assignments.iter())
        .map(|assignment| assignment.resource)
        .collect();

    let find = |list: &[Assignment], id: ResourceId| {
        list.iter()
            .find(|assignment| assignment.resource == id)
            .copied()
    };

    for id in booked {
        let resource_name = after
            .resource(id)
            .or_else(|| before.resource(id))
            .map(|resource| resource.name.clone())
            .unwrap_or_default();
        let task_name = now.name.clone();

        match (find(&was.assignments, id), find(&now.assignments, id)) {
            (None, Some(added)) => out.push(Difference::AssignmentAdded {
                task: now.id,
                task_name,
                resource: id,
                resource_name,
                units: added.units,
            }),
            (Some(_), None) => out.push(Difference::AssignmentRemoved {
                task: now.id,
                task_name,
                resource: id,
                resource_name,
            }),
            (Some(old), Some(new)) if (old.units - new.units).abs() > f64::EPSILON => {
                out.push(Difference::AssignmentChanged {
                    task: now.id,
                    task_name,
                    resource: id,
                    resource_name,
                    before_units: old.units,
                    after_units: new.units,
                })
            }
            _ => {}
        }
    }
}

fn link_differences(before: &Project, after: &Project, out: &mut Vec<Difference>) {
    // A link is identified by the pair it joins, which is what the model
    // enforces: `add_link` refuses a second link between the same two tasks.
    let pairs: BTreeSet<(TaskId, TaskId)> = before
        .links
        .iter()
        .chain(after.links.iter())
        .map(|link| (link.predecessor, link.successor))
        .collect();

    let find = |links: &[Link], pair: (TaskId, TaskId)| {
        links
            .iter()
            .find(|link| (link.predecessor, link.successor) == pair)
            .copied()
    };

    for pair in pairs {
        match (find(&before.links, pair), find(&after.links, pair)) {
            (None, Some(added)) => out.push(Difference::LinkAdded {
                predecessor: added.predecessor,
                successor: added.successor,
                kind: added.kind,
                lag_minutes: added.lag_minutes,
            }),
            (Some(gone), None) => out.push(Difference::LinkRemoved {
                predecessor: gone.predecessor,
                successor: gone.successor,
                kind: gone.kind,
            }),
            (Some(old), Some(new))
                if old.kind != new.kind || old.lag_minutes != new.lag_minutes =>
            {
                out.push(Difference::LinkChanged {
                    predecessor: new.predecessor,
                    successor: new.successor,
                    before_kind: old.kind,
                    after_kind: new.kind,
                    before_lag_minutes: old.lag_minutes,
                    after_lag_minutes: new.lag_minutes,
                })
            }
            _ => {}
        }
    }
}

fn resource_differences(before: &Project, after: &Project, out: &mut Vec<Difference>) {
    let known: BTreeSet<ResourceId> = before
        .resources
        .iter()
        .chain(after.resources.iter())
        .map(|resource| resource.id)
        .collect();

    for id in known {
        match (before.resource(id), after.resource(id)) {
            (None, Some(added)) => out.push(Difference::ResourceAdded {
                id,
                name: added.name.clone(),
                values: ResourceField::ALL
                    .iter()
                    .copied()
                    .filter(|field| *field != ResourceField::Name)
                    .map(|field| ResourceText {
                        field,
                        text: field.read(added),
                    })
                    .collect(),
            }),
            (Some(gone), None) => out.push(Difference::ResourceRemoved {
                id,
                name: gone.name.clone(),
            }),
            (Some(old), Some(new)) => {
                for field in ResourceField::ALL {
                    let was = field.read(old);
                    let is = field.read(new);
                    if was != is {
                        out.push(Difference::ResourceChanged {
                            id,
                            name: new.name.clone(),
                            field,
                            before: was,
                            after: is,
                        });
                    }
                }
            }
            (None, None) => {}
        }
    }
}

// ---- grouping and counting ----------------------------------------------

/// Everything that happened to one task.
///
/// A dialog wants to say "Wing A: duration 3d to 5d, 20% to 60%" rather than
/// print two lines out of two hundred, which it cannot do from a flat list
/// without regrouping it first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskGroup {
    pub id: TaskId,
    pub name: String,
    pub differences: Vec<Difference>,
}

/// The same differences, filed under what they are about.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Grouped {
    /// Tasks in the order they first appear in the flat list.
    pub tasks: Vec<TaskGroup>,
    /// What belongs to no single task: the resource sheet.
    pub plan: Vec<Difference>,
}

/// File a flat list under the task each difference belongs to, keeping the
/// order it arrived in so the grouping inherits `compare`'s ordering.
pub fn group_by_task(differences: &[Difference]) -> Grouped {
    let mut grouped = Grouped::default();
    let mut at: HashMap<TaskId, usize> = HashMap::new();

    for difference in differences {
        let Some(id) = difference.task() else {
            grouped.plan.push(difference.clone());
            continue;
        };
        let index = *at.entry(id).or_insert_with(|| {
            grouped.tasks.push(TaskGroup {
                id,
                name: difference.subject().to_string(),
                differences: Vec::new(),
            });
            grouped.tasks.len() - 1
        });
        // A link is filed under its successor and carries no name, so a group
        // opened by one takes its name from the first difference that has one.
        if grouped.tasks[index].name.is_empty() {
            grouped.tasks[index].name = difference.subject().to_string();
        }
        grouped.tasks[index].differences.push(difference.clone());
    }

    grouped
}

/// How big the difference is, for a dialog that has to lead with one sentence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub tasks_added: usize,
    pub tasks_removed: usize,
    pub tasks_moved: usize,
    /// Tasks that are in both plans and carry at least one field difference.
    pub tasks_changed: usize,
    pub fields_changed: usize,
    pub links_changed: usize,
    pub resources_changed: usize,
    pub assignments_changed: usize,
}

impl Summary {
    pub fn is_empty(&self) -> bool {
        *self == Summary::default()
    }

    /// One honest sentence. Counts only what is actually there, so nothing
    /// reads "0 tasks removed" at somebody.
    pub fn sentence(&self) -> String {
        if self.is_empty() {
            return "The two plans are the same.".into();
        }

        let plural = |count: usize, one: &str, many: &str| {
            format!("{count} {}", if count == 1 { one } else { many })
        };
        let mut parts = Vec::new();
        if self.tasks_added > 0 {
            parts.push(format!("{} added", plural(self.tasks_added, "task", "tasks")));
        }
        if self.tasks_removed > 0 {
            parts.push(format!(
                "{} removed",
                plural(self.tasks_removed, "task", "tasks")
            ));
        }
        if self.tasks_moved > 0 {
            parts.push(format!("{} moved", plural(self.tasks_moved, "task", "tasks")));
        }
        if self.fields_changed > 0 {
            parts.push(format!(
                "{} changed on {}",
                plural(self.fields_changed, "field", "fields"),
                plural(self.tasks_changed, "task", "tasks")
            ));
        }
        if self.links_changed > 0 {
            parts.push(format!("{} altered", plural(self.links_changed, "link", "links")));
        }
        if self.assignments_changed > 0 {
            parts.push(format!(
                "{} altered",
                plural(self.assignments_changed, "assignment", "assignments")
            ));
        }
        if self.resources_changed > 0 {
            parts.push(format!(
                "{} altered",
                plural(self.resources_changed, "resource", "resources")
            ));
        }
        format!("{}.", parts.join(", "))
    }
}

/// Count a list of differences.
pub fn summarise(differences: &[Difference]) -> Summary {
    let mut summary = Summary::default();
    let mut touched: BTreeSet<TaskId> = BTreeSet::new();

    for difference in differences {
        match difference {
            Difference::TaskAdded { .. } => summary.tasks_added += 1,
            Difference::TaskRemoved { .. } => summary.tasks_removed += 1,
            Difference::TaskMoved { .. } => summary.tasks_moved += 1,
            Difference::FieldChanged { id, .. } => {
                summary.fields_changed += 1;
                touched.insert(*id);
            }
            Difference::LinkAdded { .. }
            | Difference::LinkRemoved { .. }
            | Difference::LinkChanged { .. } => summary.links_changed += 1,
            Difference::ResourceAdded { .. }
            | Difference::ResourceRemoved { .. }
            | Difference::ResourceChanged { .. } => summary.resources_changed += 1,
            Difference::AssignmentAdded { .. }
            | Difference::AssignmentRemoved { .. }
            | Difference::AssignmentChanged { .. } => summary.assignments_changed += 1,
        }
    }

    summary.tasks_changed = touched.len();
    summary
}

// ---- applying -----------------------------------------------------------

/// Why one difference did not take.
///
/// Every one of these means the two sides have drifted, which is why they are
/// reported rather than swallowed. A caller that sees any of them should ask
/// for a whole plan instead of carrying on from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reason {
    /// No task with that id in this plan.
    NoSuchTask,
    /// No resource with that id in this plan.
    NoSuchResource,
    /// The link named is not here to change or remove.
    NoSuchLink,
    /// Something with that id is already here.
    AlreadyHere,
    /// What is here is not what the difference expected to find.
    Stale { found: String },
    /// The value could not be read back out of its text.
    Unreadable { text: String },
    /// The row was placed, but not where the difference said it would land.
    OutOfPlace { landed: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rejected {
    pub difference: Difference,
    pub reason: Reason,
}

/// What an apply managed, and what it did not.
///
/// Not a bare `Result`, because a partly applied batch is the normal case
/// worth reporting rather than an error worth aborting on: the caller needs to
/// know both that most of it landed and exactly which parts did not.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Applied {
    pub applied: usize,
    pub rejected: Vec<Rejected>,
}

impl Applied {
    /// Whether everything took. False means the two sides no longer agree
    /// about what the plan was, and the honest next step is a whole snapshot.
    pub fn is_clean(&self) -> bool {
        self.rejected.is_empty()
    }

    fn reject(&mut self, difference: &Difference, reason: Reason) {
        self.rejected.push(Rejected {
            difference: difference.clone(),
            reason,
        });
    }
}

/// Take differences worked out somewhere else and make them true here.
///
/// A `FieldChanged` whose `before` is not what is actually in the plan is
/// refused rather than written, because a live session that writes over what
/// it did not expect converges on nothing. `apply_forcing` is the way to say
/// otherwise, deliberately and by name.
pub fn apply(project: &mut Project, differences: &[Difference]) -> Applied {
    apply_inner(project, differences, false)
}

/// The same, writing over values that are not what the difference expected.
///
/// For a caller that has already decided one side wins outright, such as a
/// person answering "keep theirs" to a conflict.
pub fn apply_forcing(project: &mut Project, differences: &[Difference]) -> Applied {
    apply_inner(project, differences, true)
}

fn apply_inner(project: &mut Project, differences: &[Difference], force: bool) -> Applied {
    let mut done = Applied::default();

    // The order below is not cosmetic. Links come out before the tasks they
    // hang off, so removing a task never has to silently take a link with it.
    // Rows are added and moved together in target order, because an insert and
    // a relocation both describe a place in the finished layout and doing
    // either alone leaves the other landing in the wrong gap. Assignments run
    // before resources are removed, so an assignment removal still has a
    // resource to name.

    for difference in differences {
        if let Difference::LinkRemoved {
            predecessor,
            successor,
            ..
        } = difference
        {
            if project.link_exists(*predecessor, *successor) {
                project.unlink(*predecessor, *successor);
                done.applied += 1;
            } else {
                done.reject(difference, Reason::NoSuchLink);
            }
        }
    }

    for difference in differences {
        if let Difference::TaskRemoved { id, .. } = difference {
            if remove_row(project, *id) {
                done.applied += 1;
            } else {
                done.reject(difference, Reason::NoSuchTask);
            }
        }
    }

    place_rows(project, differences, &mut done);

    for difference in differences {
        match difference {
            Difference::ResourceAdded { id, name, values } => {
                if project.resource(*id).is_some() {
                    done.reject(difference, Reason::AlreadyHere);
                    continue;
                }
                reserve_resource_id(project, *id);
                let mut resource = Resource::new(*id, name.clone());
                let mut unreadable = None;
                for value in values {
                    if !value.field.write(&mut resource, &value.text) {
                        unreadable.get_or_insert_with(|| value.text.clone());
                    }
                }
                project.resources.push(resource);
                match unreadable {
                    None => done.applied += 1,
                    Some(text) => done.reject(difference, Reason::Unreadable { text }),
                }
            }
            Difference::ResourceChanged {
                id,
                field,
                before,
                after,
                ..
            } => {
                let Some(resource) = project.resources.iter_mut().find(|held| held.id == *id)
                else {
                    done.reject(difference, Reason::NoSuchResource);
                    continue;
                };
                let found = field.read(resource);
                if found != *before && !force {
                    done.reject(difference, Reason::Stale { found });
                    continue;
                }
                if field.write(resource, after) {
                    done.applied += 1;
                } else {
                    done.reject(difference, Reason::Unreadable { text: after.clone() });
                }
            }
            _ => {}
        }
    }

    for difference in differences {
        if let Difference::FieldChanged {
            id,
            field,
            before,
            after,
            ..
        } = difference
        {
            let Some(index) = project.index_of(*id) else {
                done.reject(difference, Reason::NoSuchTask);
                continue;
            };
            let found = read(project, index, *field);
            if found != *before && !force {
                done.reject(difference, Reason::Stale { found });
                continue;
            }
            if write(project, index, *field, after) {
                done.applied += 1;
            } else {
                done.reject(difference, Reason::Unreadable { text: after.clone() });
            }
        }
    }

    for difference in differences {
        match difference {
            Difference::AssignmentAdded {
                task,
                resource,
                units,
                ..
            } => {
                if project.resource(*resource).is_none() {
                    done.reject(difference, Reason::NoSuchResource);
                    continue;
                }
                match project.task_mut(*task) {
                    Some(held) => {
                        held.assignments.retain(|held| held.resource != *resource);
                        held.assignments.push(Assignment {
                            resource: *resource,
                            units: *units,
                        });
                        done.applied += 1;
                    }
                    None => done.reject(difference, Reason::NoSuchTask),
                }
            }
            Difference::AssignmentRemoved { task, resource, .. } => match project.task_mut(*task) {
                Some(held) => {
                    let before = held.assignments.len();
                    held.assignments.retain(|held| held.resource != *resource);
                    if held.assignments.len() == before {
                        done.reject(difference, Reason::NoSuchResource);
                    } else {
                        done.applied += 1;
                    }
                }
                None => done.reject(difference, Reason::NoSuchTask),
            },
            Difference::AssignmentChanged {
                task,
                resource,
                before_units,
                after_units,
                ..
            } => {
                let Some(held) = project.task_mut(*task) else {
                    done.reject(difference, Reason::NoSuchTask);
                    continue;
                };
                let Some(assignment) = held
                    .assignments
                    .iter_mut()
                    .find(|held| held.resource == *resource)
                else {
                    done.reject(difference, Reason::NoSuchResource);
                    continue;
                };
                if (assignment.units - before_units).abs() > f64::EPSILON && !force {
                    done.reject(
                        difference,
                        Reason::Stale {
                            found: format!("{:.2}", assignment.units),
                        },
                    );
                    continue;
                }
                assignment.units = *after_units;
                done.applied += 1;
            }
            _ => {}
        }
    }

    for difference in differences {
        match difference {
            Difference::LinkAdded {
                predecessor,
                successor,
                kind,
                lag_minutes,
            } => {
                if project.index_of(*predecessor).is_none() || project.index_of(*successor).is_none()
                {
                    done.reject(difference, Reason::NoSuchTask);
                    continue;
                }
                if project.add_link(Link {
                    predecessor: *predecessor,
                    successor: *successor,
                    kind: *kind,
                    lag_minutes: *lag_minutes,
                }) {
                    done.applied += 1;
                } else {
                    done.reject(difference, Reason::AlreadyHere);
                }
            }
            Difference::LinkChanged {
                predecessor,
                successor,
                before_kind,
                before_lag_minutes,
                after_kind,
                after_lag_minutes,
                ..
            } => {
                let Some(link) = project
                    .links
                    .iter_mut()
                    .find(|link| link.predecessor == *predecessor && link.successor == *successor)
                else {
                    done.reject(difference, Reason::NoSuchLink);
                    continue;
                };
                if (link.kind != *before_kind || link.lag_minutes != *before_lag_minutes) && !force {
                    let found = format!("{}{:+}", link.kind.code(), link.lag_minutes);
                    done.reject(difference, Reason::Stale { found });
                    continue;
                }
                link.kind = *after_kind;
                link.lag_minutes = *after_lag_minutes;
                done.applied += 1;
            }
            _ => {}
        }
    }

    for difference in differences {
        if let Difference::ResourceRemoved { id, .. } = difference {
            if project.resource(*id).is_some() {
                project.delete_resource(*id);
                done.applied += 1;
            } else {
                done.reject(difference, Reason::NoSuchResource);
            }
        }
    }

    done
}


/// Put the added and moved rows where the differences say they go.
///
/// Both kinds name a row in the finished layout, so they are done together in
/// ascending target order. Doing all the inserts first and then the moves gets
/// this wrong: an insert placed against the finished layout lands in a list
/// that has not been reordered yet.
fn place_rows(project: &mut Project, differences: &[Difference], done: &mut Applied) {
    let mut placements: Vec<(usize, TaskId, &Difference)> = differences
        .iter()
        .filter_map(|difference| match difference {
            Difference::TaskAdded { at, id, .. } => Some((*at, *id, difference)),
            Difference::TaskMoved { to, id, .. } => Some((*to, *id, difference)),
            _ => None,
        })
        .collect();
    placements.sort_by_key(|(target, _, _)| *target);

    for (target, _, difference) in &placements {
        match difference {
            Difference::TaskAdded {
                id, name, values, ..
            } => {
                if project.index_of(*id).is_some() {
                    done.reject(difference, Reason::AlreadyHere);
                    continue;
                }
                reserve_task_id(project, *id);
                let at = (*target).min(project.tasks.len());
                project.tasks.insert(at, Task::new(*id, name.clone(), 0));
                for value in values {
                    if !write(project, at, value.field, &value.text) {
                        done.reject(
                            difference,
                            Reason::Unreadable {
                                text: value.text.clone(),
                            },
                        );
                    }
                }
            }
            Difference::TaskMoved { id, .. } => {
                // Found by id, not by `from`: after the first move of a batch
                // the old row numbers no longer describe this plan.
                let Some(current) = project.index_of(*id) else {
                    done.reject(difference, Reason::NoSuchTask);
                    continue;
                };
                let row = project.tasks.remove(current);
                let at = (*target).min(project.tasks.len());
                project.tasks.insert(at, row);
            }
            _ => {}
        }
    }

    // Counted here rather than above, because whether a row landed where it
    // was meant to is only knowable once the whole batch has run.
    for (target, id, difference) in &placements {
        match project.index_of(*id) {
            Some(landed) if landed == *target => done.applied += 1,
            Some(landed) => done.reject(difference, Reason::OutOfPlace { landed }),
            None => done.reject(difference, Reason::NoSuchTask),
        }
    }
}

/// Take one row out, along with the links and callouts hanging off it.
///
/// Deliberately not `Project::delete_task`, which takes the rows nested under
/// it as well. A comparison reports every removed row on its own, so removing
/// a summary here must not also swallow children that have their own
/// difference waiting to be applied.
fn remove_row(project: &mut Project, id: TaskId) -> bool {
    let Some(index) = project.index_of(id) else {
        return false;
    };
    project.tasks.remove(index);
    project
        .links
        .retain(|link| link.predecessor != id && link.successor != id);
    project
        .drawings
        .retain(|drawing| drawing.anchored_task().is_none_or(|task| task != id));
    true
}

/// Move the plan's id counter past one that arrived from elsewhere.
///
/// The counter is private and only moves by handing ids out, so the ids in
/// between are burnt. They are never many: both plans descend from the same
/// file. Not doing this would let the plan hand out an id it is already using,
/// which is the one thing the whole matching scheme rests on.
fn reserve_task_id(project: &mut Project, id: TaskId) {
    while project.allocate_task_id() <= id {}
}

fn reserve_resource_id(project: &mut Project, id: ResourceId) {
    while project.allocate_resource_id() <= id {}
}

/// Put a field back from the text a difference carries. False means the text
/// could not be read, which the caller reports rather than guessing at.
fn write(project: &mut Project, index: usize, field: Field, text: &str) -> bool {
    let currency = project.currency_symbol.clone();
    let Some(task) = project.tasks.get_mut(index) else {
        return false;
    };

    match field {
        Field::Name => task.name = text.to_string(),
        Field::OutlineLevel => match text.trim().parse::<u16>() {
            // The column counts from one, the model from zero.
            Ok(level) if level >= 1 => task.outline_level = level - 1,
            _ => return false,
        },
        Field::TaskMode => {
            if text == TaskMode::Manual.label() {
                task.mode = TaskMode::Manual;
            } else if text == TaskMode::Auto.label() {
                task.mode = TaskMode::Auto;
            } else {
                return false;
            }
        }
        Field::Duration => match parse_duration(text) {
            Some((minutes, estimated)) => {
                task.duration_minutes = minutes;
                task.estimated = estimated;
                // The scheduler owns the rolled up figure and will write this
                // again on its next pass. Setting it here is what lets a plan
                // read correctly in between, and what lets a comparison close
                // without a reschedule first.
                task.scheduled.duration_minutes = minutes;
            }
            None => return false,
        },
        Field::Start => match parse_moment(text) {
            Some(Some(at)) => {
                task.manual_start = Some(at);
                task.scheduled.start = at;
            }
            _ => return false,
        },
        Field::ConstraintType => {
            match ConstraintType::ALL.iter().find(|kind| kind.label() == text) {
                Some(&kind) => task.constraint = kind,
                None => return false,
            }
        }
        Field::ConstraintDate => match parse_moment(text) {
            Some(value) => task.constraint_date = value,
            None => return false,
        },
        Field::Deadline => match parse_moment(text) {
            Some(value) => task.deadline = value,
            None => return false,
        },
        Field::PercentComplete => match parse_percent(text) {
            Some(Some(percent)) => task.percent_complete = percent,
            _ => return false,
        },
        Field::Active => match text {
            "Yes" => task.active = true,
            "No" => task.active = false,
            _ => return false,
        },
        Field::FixedCost => match parse_money(&currency, text) {
            Some(amount) => task.fixed_cost = amount,
            None => return false,
        },
        Field::ActualStart => match parse_moment(text) {
            Some(value) => task.actual_start = value,
            None => return false,
        },
        Field::ActualFinish => match parse_moment(text) {
            Some(value) => task.actual_finish = value,
            None => return false,
        },
        Field::PhysicalPercentComplete => match parse_percent(text) {
            Some(value) => task.physical_percent_complete = value,
            None => return false,
        },
        Field::Notes => task.notes = text.to_string(),
        // Everything else is derived, so it is never reported and never ours
        // to write. A difference naming one has come from somewhere that does
        // not share these rules, and is not to be trusted with the plan.
        _ => return false,
    }
    true
}

/// The outer `None` means unreadable; the inner one means the cell is empty,
/// which is a value in its own right for a deadline or an actual finish.
fn parse_moment(text: &str) -> Option<Option<NaiveDateTime>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    NaiveDateTime::parse_from_str(text, DATE_FORMAT)
        .ok()
        .map(Some)
}

fn parse_percent(text: &str) -> Option<Option<u8>> {
    let text = text.trim().trim_end_matches('%').trim();
    if text.is_empty() {
        return Some(None);
    }
    text.parse::<u8>().ok().map(Some)
}

fn parse_money(currency: &str, text: &str) -> Option<f64> {
    let text = text.trim();
    let text = text.strip_prefix(currency).unwrap_or(text);
    text.trim().replace(',', "").parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Assignment, ConstraintType, Link, LinkType, Task};
    use crate::schedule::schedule;
    use crate::MINUTES_PER_DAY;

    fn start() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    /// Four linked tasks, one resource, scheduled. Small enough to reason
    /// about row by row.
    fn plan() -> Project {
        let mut project = Project::blank(start());
        for name in ["Survey", "Design", "Build", "Handover"] {
            project.push_task(name, MINUTES_PER_DAY * 2);
        }
        let ids: Vec<TaskId> = project.tasks.iter().map(|task| task.id).collect();
        for pair in ids.windows(2) {
            project.add_link(Link::finish_to_start(pair[0], pair[1]));
        }
        let ana = project.add_resource("Ana Reyes");
        project.tasks[1].assignments.push(Assignment {
            resource: ana,
            units: 1.0,
        });
        schedule(&mut project).expect("the sample plan schedules");
        project
    }

    fn names(project: &Project) -> Vec<&str> {
        project.tasks.iter().map(|task| task.name.as_str()).collect()
    }

    #[test]
    fn identical_plans_compare_to_nothing() {
        let project = plan();
        let differences = compare(&project, &project.clone());
        assert!(differences.is_empty(), "found {differences:?}");
        assert!(summarise(&differences).is_empty());
        assert_eq!(
            summarise(&differences).sentence(),
            "The two plans are the same."
        );
    }

    #[test]
    fn a_reordered_plan_reports_moves_rather_than_wholesale_replacement() {
        // Handover to the front. Everything else keeps its relative order, so
        // only one task actually moved however many row numbers changed.
        let before = plan();
        let mut after = before.clone();
        let row = after.tasks.remove(3);
        after.tasks.insert(0, row);

        let differences = compare(&before, &after);
        assert_eq!(differences.len(), 1, "found {differences:?}");
        match &differences[0] {
            Difference::TaskMoved { name, from, to, .. } => {
                assert_eq!(name, "Handover");
                assert_eq!((*from, *to), (3, 0));
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn an_insert_at_the_top_does_not_report_everything_below_as_moved() {
        // The whole point of matching by id. Every row below the new one is at
        // a different row number and none of them moved.
        let before = plan();
        let mut after = before.clone();
        let id = after.allocate_task_id();
        after.tasks.insert(0, Task::new(id, "Kickoff", MINUTES_PER_DAY));

        let differences = compare(&before, &after);
        assert!(
            !differences
                .iter()
                .any(|difference| matches!(difference, Difference::TaskMoved { .. })),
            "found {differences:?}"
        );
        assert_eq!(differences.len(), 1);
        assert!(matches!(
            differences[0],
            Difference::TaskAdded { at: 0, .. }
        ));
    }

    #[test]
    fn a_duration_edit_does_not_also_report_the_finish_moving() {
        // Lengthening Design pushes Build and Handover and changes the slack
        // on the plan. One person made one edit, so there is one difference.
        let before = plan();
        let mut after = before.clone();
        after.tasks[1].duration_minutes = MINUTES_PER_DAY * 5;
        schedule(&mut after).expect("the edited plan schedules");

        let differences = compare(&before, &after);
        assert_eq!(differences.len(), 1, "found {differences:?}");
        match &differences[0] {
            Difference::FieldChanged {
                field,
                before: was,
                after: is,
                ..
            } => {
                assert_eq!(*field, Field::Duration);
                assert_eq!(was, "2 days");
                assert_eq!(is, "1 wk");
            }
            other => panic!("expected a duration change, got {other:?}"),
        }
    }

    #[test]
    fn a_task_added_and_one_removed_are_told_apart() {
        let before = plan();
        let mut after = before.clone();
        let gone = after.tasks[2].id;
        after.tasks.remove(2);
        after.links.retain(|link| link.predecessor != gone && link.successor != gone);
        let fresh = after.allocate_task_id();
        after.tasks.push(Task::new(fresh, "Snagging", MINUTES_PER_DAY));

        let differences = compare(&before, &after);
        let added: Vec<&Difference> = differences
            .iter()
            .filter(|difference| matches!(difference, Difference::TaskAdded { .. }))
            .collect();
        let removed: Vec<&Difference> = differences
            .iter()
            .filter(|difference| matches!(difference, Difference::TaskRemoved { .. }))
            .collect();

        assert_eq!(added.len(), 1, "found {differences:?}");
        assert_eq!(removed.len(), 1, "found {differences:?}");
        assert_eq!(added[0].subject(), "Snagging");
        assert_eq!(removed[0].subject(), "Build");
        assert!(
            !differences
                .iter()
                .any(|difference| matches!(difference, Difference::TaskMoved { .. })),
            "a replacement is not a move"
        );
    }

    #[test]
    fn the_order_is_the_same_every_run() {
        // Nothing here may fall out of a hash map's iteration order, so the
        // same pair of plans has to produce byte for byte the same list.
        let before = plan();
        let mut after = before.clone();
        after.tasks[0].name = "Site survey".into();
        after.tasks[3].percent_complete = 40;
        after.resources[0].standard_rate = 95.0;
        let ids: Vec<TaskId> = after.tasks.iter().map(|task| task.id).collect();
        after.add_link(Link::finish_to_start(ids[0], ids[3]));
        let row = after.tasks.remove(2);
        after.tasks.insert(0, row);

        let first = format!("{:?}", compare(&before, &after));
        for _ in 0..20 {
            assert_eq!(format!("{:?}", compare(&before, &after)), first);
        }
    }

    #[test]
    fn differences_are_filed_under_the_task_they_belong_to() {
        let before = plan();
        let mut after = before.clone();
        after.tasks[1].name = "Detailed design".into();
        after.tasks[1].percent_complete = 60;
        after.resources[0].name = "Ana R".into();

        let grouped = group_by_task(&compare(&before, &after));
        assert_eq!(grouped.tasks.len(), 1, "one task changed, not two");
        assert_eq!(grouped.tasks[0].differences.len(), 2);
        assert_eq!(grouped.plan.len(), 1, "the resource rename is plan wide");

        let summary = summarise(&compare(&before, &after));
        assert_eq!(summary.tasks_changed, 1);
        assert_eq!(summary.fields_changed, 2);
        assert!(summary.sentence().contains("2 fields changed on 1 task"));
    }

    #[test]
    fn a_summary_rows_duration_is_left_to_the_child_that_moved() {
        let mut before = plan();
        // A phase heading cannot also be a predecessor of what sits under it,
        // so the sample's chain goes before the outline does.
        before.links.clear();
        before.tasks[1].outline_level = 1;
        before.tasks[2].outline_level = 1;
        schedule(&mut before).expect("the outlined plan schedules");

        let mut after = before.clone();
        after.tasks[1].duration_minutes = MINUTES_PER_DAY * 6;
        schedule(&mut after).expect("the edited plan schedules");

        let differences = compare(&before, &after);
        let on_summary: Vec<&Difference> = differences
            .iter()
            .filter(|difference| difference.task() == Some(before.tasks[0].id))
            .collect();
        assert!(
            on_summary.is_empty(),
            "the summary rolled up, it was not edited: {on_summary:?}"
        );
        assert_eq!(differences.len(), 1, "found {differences:?}");
    }

    // ---- applying -------------------------------------------------------

    #[test]
    fn a_round_trip_leaves_nothing_to_report() {
        // The test that matters: a reorder, a link change, a resource change,
        // an assignment and a field edit all at once, each of which is its own
        // code path in `apply`.
        let before = plan();
        let mut after = before.clone();

        let row = after.tasks.remove(3);
        after.tasks.insert(1, row);
        after.tasks[0].name = "Site survey".into();
        after.tasks[2].duration_minutes = MINUTES_PER_DAY * 4;
        after.tasks[2].deadline = Some(start() + chrono::Duration::days(30));
        let ids: Vec<TaskId> = after.tasks.iter().map(|task| task.id).collect();
        after.unlink(ids[2], ids[3]);
        after.add_link(Link {
            predecessor: ids[0],
            successor: ids[3],
            kind: LinkType::SS,
            lag_minutes: MINUTES_PER_DAY,
        });
        let rig = after.add_resource("Rig");
        after.resources[0].standard_rate = 95.0;
        after.tasks[2].assignments.push(Assignment {
            resource: rig,
            units: 0.5,
        });
        let fresh = after.allocate_task_id();
        let mut extra = Task::new(fresh, "Snagging", MINUTES_PER_DAY * 3);
        extra.constraint = ConstraintType::StartNoEarlierThan;
        extra.constraint_date = Some(start() + chrono::Duration::days(10));
        after.tasks.push(extra);
        let dropped = after.tasks[1].id;
        after.tasks.remove(1);
        after
            .links
            .retain(|link| link.predecessor != dropped && link.successor != dropped);
        schedule(&mut after).expect("the edited plan schedules");

        let differences = compare(&before, &after);
        assert!(!differences.is_empty());

        let mut receiver = before.clone();
        let done = apply(&mut receiver, &differences);
        assert!(done.is_clean(), "rejected {:?}", done.rejected);
        assert_eq!(done.applied, differences.len());

        assert_eq!(names(&receiver), names(&after), "the rows ended up in order");
        let left = compare(&receiver, &after);
        assert!(left.is_empty(), "still different: {left:?}");
    }

    #[test]
    fn a_wholesale_shuffle_still_lands_row_for_row() {
        // Placement is the part most likely to be subtly wrong, because an
        // insert and a move both name a row in the finished layout while the
        // list is only half way there. Ten rows reversed, with a row added and
        // a row taken out to shift everything else along as well.
        let mut before = Project::blank(start());
        for n in 0..10 {
            before.push_task(format!("Task {n}"), MINUTES_PER_DAY);
        }
        let mut after = before.clone();
        after.tasks.reverse();
        let gone = after.tasks[4].id;
        after.tasks.remove(4);
        let fresh = after.allocate_task_id();
        after.tasks.insert(2, Task::new(fresh, "Inserted", MINUTES_PER_DAY));
        assert_ne!(gone, fresh);

        let differences = compare(&before, &after);
        let mut receiver = before.clone();
        let done = apply(&mut receiver, &differences);

        assert!(done.is_clean(), "rejected {:?}", done.rejected);
        assert_eq!(names(&receiver), names(&after));
        assert!(compare(&receiver, &after).is_empty());
    }

    #[test]
    fn a_field_change_is_refused_when_the_plan_holds_something_else() {
        let before = plan();
        let mut after = before.clone();
        after.tasks[0].percent_complete = 50;
        let differences = compare(&before, &after);

        // Somebody else got there first and put it at 80.
        let mut receiver = before.clone();
        receiver.tasks[0].percent_complete = 80;

        let done = apply(&mut receiver, &differences);
        assert_eq!(done.applied, 0);
        assert_eq!(
            done.rejected[0].reason,
            Reason::Stale {
                found: "80%".into()
            }
        );
        assert_eq!(
            receiver.tasks[0].percent_complete, 80,
            "a refused change must not be half written"
        );

        let forced = apply_forcing(&mut receiver, &differences);
        assert!(forced.is_clean());
        assert_eq!(receiver.tasks[0].percent_complete, 50);
    }

    #[test]
    fn a_difference_about_a_task_this_plan_does_not_have_is_reported() {
        // The receiver has drifted far enough that it should ask for the whole
        // plan. Saying so is the entire job of `Applied`.
        let before = plan();
        let mut after = before.clone();
        after.tasks[2].name = "Construct".into();
        let differences = compare(&before, &after);

        let mut receiver = before.clone();
        receiver.tasks.remove(2);

        let done = apply(&mut receiver, &differences);
        assert!(!done.is_clean());
        assert_eq!(done.rejected[0].reason, Reason::NoSuchTask);
    }

    #[test]
    fn differences_survive_the_wire() {
        let before = plan();
        let mut after = before.clone();
        after.tasks[0].name = "Site survey".into();
        let fresh = after.allocate_task_id();
        after.tasks.push(Task::new(fresh, "Snagging", MINUTES_PER_DAY));
        after.add_resource("Rig");

        let differences = compare(&before, &after);
        let text = serde_json::to_string(&differences).expect("differences serialise");
        let back: Vec<Difference> = serde_json::from_str(&text).expect("and read back");
        assert_eq!(back, differences);
    }
}
