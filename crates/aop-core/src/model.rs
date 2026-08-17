//! The project data model.
//!
//! Tasks live in a single flat, ordered `Vec` and express hierarchy through
//! `outline_level`, exactly the way Microsoft Project stores an outline. A task
//! is a summary when the task after it sits one level deeper, so indenting and
//! outdenting are pure integer edits and never restructure a tree.

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::calendar::WorkCalendar;
use crate::MINUTES_PER_DAY;

pub type TaskId = u32;
pub type ResourceId = u32;

/// Auto-scheduled tasks are driven by links and constraints. Manually scheduled
/// tasks keep whatever dates the user typed and act as fixed points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskMode {
    Manual,
    Auto,
}

impl TaskMode {
    pub fn label(self) -> &'static str {
        match self {
            TaskMode::Manual => "Manually Scheduled",
            TaskMode::Auto => "Auto Scheduled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintType {
    AsSoonAsPossible,
    AsLateAsPossible,
    StartNoEarlierThan,
    StartNoLaterThan,
    FinishNoEarlierThan,
    FinishNoLaterThan,
    MustStartOn,
    MustFinishOn,
}

impl ConstraintType {
    pub const ALL: [ConstraintType; 8] = [
        ConstraintType::AsSoonAsPossible,
        ConstraintType::AsLateAsPossible,
        ConstraintType::StartNoEarlierThan,
        ConstraintType::StartNoLaterThan,
        ConstraintType::FinishNoEarlierThan,
        ConstraintType::FinishNoLaterThan,
        ConstraintType::MustStartOn,
        ConstraintType::MustFinishOn,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConstraintType::AsSoonAsPossible => "As Soon As Possible",
            ConstraintType::AsLateAsPossible => "As Late As Possible",
            ConstraintType::StartNoEarlierThan => "Start No Earlier Than",
            ConstraintType::StartNoLaterThan => "Start No Later Than",
            ConstraintType::FinishNoEarlierThan => "Finish No Earlier Than",
            ConstraintType::FinishNoLaterThan => "Finish No Later Than",
            ConstraintType::MustStartOn => "Must Start On",
            ConstraintType::MustFinishOn => "Must Finish On",
        }
    }

    /// Whether this constraint needs a date to go with it.
    pub fn needs_date(self) -> bool {
        !matches!(
            self,
            ConstraintType::AsSoonAsPossible | ConstraintType::AsLateAsPossible
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkType {
    /// Finish-to-Start: the successor starts after the predecessor finishes.
    FS,
    /// Start-to-Start.
    SS,
    /// Finish-to-Finish.
    FF,
    /// Start-to-Finish.
    SF,
}

impl LinkType {
    pub const ALL: [LinkType; 4] = [LinkType::FS, LinkType::SS, LinkType::FF, LinkType::SF];

    pub fn code(self) -> &'static str {
        match self {
            LinkType::FS => "FS",
            LinkType::SS => "SS",
            LinkType::FF => "FF",
            LinkType::SF => "SF",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LinkType::FS => "Finish-to-Start (FS)",
            LinkType::SS => "Start-to-Start (SS)",
            LinkType::FF => "Finish-to-Finish (FF)",
            LinkType::SF => "Start-to-Finish (SF)",
        }
    }

    pub fn parse(code: &str) -> Option<LinkType> {
        match code.trim().to_ascii_uppercase().as_str() {
            "FS" => Some(LinkType::FS),
            "SS" => Some(LinkType::SS),
            "FF" => Some(LinkType::FF),
            "SF" => Some(LinkType::SF),
            _ => None,
        }
    }
}

/// A dependency between two tasks, with optional lag (positive) or lead
/// (negative) expressed in working minutes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub predecessor: TaskId,
    pub successor: TaskId,
    pub kind: LinkType,
    pub lag_minutes: i64,
}

impl Link {
    pub fn finish_to_start(predecessor: TaskId, successor: TaskId) -> Self {
        Self {
            predecessor,
            successor,
            kind: LinkType::FS,
            lag_minutes: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    Work,
    Material,
    Cost,
}

impl ResourceKind {
    pub const ALL: [ResourceKind; 3] = [ResourceKind::Work, ResourceKind::Material, ResourceKind::Cost];

    pub fn label(self) -> &'static str {
        match self {
            ResourceKind::Work => "Work",
            ResourceKind::Material => "Material",
            ResourceKind::Cost => "Cost",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub name: String,
    pub initials: String,
    pub kind: ResourceKind,
    pub group: String,
    /// 1.0 means one full-time unit, 0.5 means half time.
    pub max_units: f64,
    pub standard_rate: f64,
    pub overtime_rate: f64,
    pub cost_per_use: f64,
    pub base_calendar: String,
}

impl Resource {
    pub fn new(id: ResourceId, name: impl Into<String>) -> Self {
        let name = name.into();
        let initials = name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_uppercase();
        Self {
            id,
            initials: if initials.is_empty() { "R".into() } else { initials },
            name,
            kind: ResourceKind::Work,
            group: String::new(),
            max_units: 1.0,
            standard_rate: 0.0,
            overtime_rate: 0.0,
            cost_per_use: 0.0,
            base_calendar: "Standard".into(),
        }
    }

    pub fn with_rate(mut self, rate: f64) -> Self {
        self.standard_rate = rate;
        self.overtime_rate = rate * 1.5;
        self
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }
}

/// A resource booked onto a task at some percentage of its capacity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub resource: ResourceId,
    pub units: f64,
}

/// The saved snapshot a task is compared against once work is under way.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub start: NaiveDateTime,
    pub finish: NaiveDateTime,
    pub duration_minutes: i64,
    pub work_minutes: i64,
    pub cost: f64,
}

/// Fields the scheduler writes. Kept separate from the user's input so a
/// reschedule can never silently corrupt what was typed in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Scheduled {
    pub start: NaiveDateTime,
    pub finish: NaiveDateTime,
    pub late_start: NaiveDateTime,
    pub late_finish: NaiveDateTime,
    pub total_slack_minutes: i64,
    pub free_slack_minutes: i64,
    pub critical: bool,
    /// Rolled-up duration for summary rows, own duration for leaves.
    pub duration_minutes: i64,
    pub work_minutes: i64,
    pub cost: f64,
}

impl Default for Scheduled {
    fn default() -> Self {
        let epoch = NaiveDate::from_ymd_opt(2000, 1, 1)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        Self {
            start: epoch,
            finish: epoch,
            late_start: epoch,
            late_finish: epoch,
            total_slack_minutes: 0,
            free_slack_minutes: 0,
            critical: false,
            duration_minutes: 0,
            work_minutes: 0,
            cost: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub outline_level: u16,
    pub duration_minutes: i64,
    pub estimated: bool,
    pub mode: TaskMode,
    pub constraint: ConstraintType,
    pub constraint_date: Option<NaiveDateTime>,
    pub deadline: Option<NaiveDateTime>,
    pub percent_complete: u8,
    pub notes: String,
    pub assignments: Vec<Assignment>,
    pub fixed_cost: f64,
    pub active: bool,
    pub collapsed: bool,
    /// Start typed by the user; authoritative only for manually scheduled tasks.
    pub manual_start: Option<NaiveDateTime>,
    pub baseline: Option<Baseline>,
    #[serde(default)]
    pub scheduled: Scheduled,
}

impl Task {
    pub fn new(id: TaskId, name: impl Into<String>, duration_minutes: i64) -> Self {
        Self {
            id,
            name: name.into(),
            outline_level: 0,
            duration_minutes,
            estimated: false,
            mode: TaskMode::Auto,
            constraint: ConstraintType::AsSoonAsPossible,
            constraint_date: None,
            deadline: None,
            percent_complete: 0,
            notes: String::new(),
            assignments: Vec::new(),
            fixed_cost: 0.0,
            active: true,
            collapsed: false,
            manual_start: None,
            baseline: None,
            scheduled: Scheduled::default(),
        }
    }

    pub fn milestone(id: TaskId, name: impl Into<String>) -> Self {
        Self::new(id, name, 0)
    }

    /// A zero-duration task draws as a diamond rather than a bar.
    /// Whether the task was entered as a milestone: a marker with no duration
    /// of its own.
    ///
    /// A summary row also has no duration of its own, since its span is rolled
    /// up from its children, so this alone does not decide how a row is drawn.
    /// Use `Project::is_marker` for that.
    pub fn is_milestone(&self) -> bool {
        self.duration_minutes == 0
    }

    pub fn is_complete(&self) -> bool {
        self.percent_complete >= 100
    }

    /// How far along the bar the progress fill reaches, in working minutes.
    pub fn completed_minutes(&self) -> i64 {
        self.scheduled.duration_minutes * self.percent_complete as i64 / 100
    }

    pub fn start_variance_minutes(&self, calendar: &WorkCalendar) -> Option<i64> {
        self.baseline
            .map(|b| calendar.work_minutes_between(b.start, self.scheduled.start))
    }

    pub fn finish_variance_minutes(&self, calendar: &WorkCalendar) -> Option<i64> {
        self.baseline
            .map(|b| calendar.work_minutes_between(b.finish, self.scheduled.finish))
    }
}

/// Colours the Gantt chart draws with. Stored on the plan, so a recoloured
/// chart travels with the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarStyles {
    pub task: String,
    pub critical: String,
    pub summary: String,
    pub milestone: String,
    pub progress: String,
    pub baseline: String,
}

impl Default for BarStyles {
    fn default() -> Self {
        Self::preset(0)
    }
}

impl BarStyles {
    /// Named palettes offered by the Gantt Chart Style gallery.
    pub const PRESETS: [(&'static str, [&'static str; 6]); 6] = [
        ("Alterion", ["#3f7d7d", "#9d474d", "#cfe3e3", "#a5d3d3", "#a5d3d3", "#6b7f7f"]),
        ("Ocean", ["#3d6f9e", "#9d5c47", "#c8dbe8", "#9ec6e8", "#9ec6e8", "#6b7a85"]),
        ("Violet", ["#6a5f9e", "#9d4a72", "#d6d1e8", "#b3a8e0", "#b3a8e0", "#79738c"]),
        ("Amber", ["#95762f", "#9d4a3f", "#e8dcc0", "#e0c882", "#e0c882", "#877c63"]),
        ("Crimson", ["#9d474d", "#c0392b", "#e8cfd1", "#e0a0a4", "#e0a0a4", "#8c7376"]),
        ("Slate", ["#5a6a6a", "#8f5a5a", "#d3dada", "#a8b8b8", "#a8b8b8", "#77807f"]),
    ];

    pub fn preset(index: usize) -> Self {
        let (_, colours) = Self::PRESETS[index.min(Self::PRESETS.len() - 1)];
        Self {
            task: colours[0].into(),
            critical: colours[1].into(),
            summary: colours[2].into(),
            milestone: colours[3].into(),
            progress: colours[4].into(),
            baseline: colours[5].into(),
        }
    }

    /// Editable fields, as (label, current value) pairs.
    pub fn fields(&self) -> [(&'static str, &str); 6] {
        [
            ("Task", &self.task),
            ("Critical task", &self.critical),
            ("Summary", &self.summary),
            ("Milestone", &self.milestone),
            ("Progress", &self.progress),
            ("Baseline", &self.baseline),
        ]
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let value = value.to_string();
        match key {
            "Task" => self.task = value,
            "Critical task" => self.critical = value,
            "Summary" => self.summary = value,
            "Milestone" => self.milestone = value,
            "Progress" => self.progress = value,
            "Baseline" => self.baseline = value,
            _ => {}
        }
    }
}

/// Whether the plan is driven forward from a start date or backward from a
/// required finish date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleFrom {
    ProjectStartDate,
    ProjectFinishDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub author: String,
    pub company: String,
    pub start_date: NaiveDateTime,
    pub finish_date: NaiveDateTime,
    pub schedule_from: ScheduleFrom,
    pub status_date: Option<NaiveDateTime>,
    pub current_date: NaiveDateTime,
    pub calendar: WorkCalendar,
    pub tasks: Vec<Task>,
    pub links: Vec<Link>,
    pub resources: Vec<Resource>,
    pub currency_symbol: String,
    pub show_project_summary: bool,
    #[serde(default)]
    pub bar_styles: BarStyles,
    next_task_id: TaskId,
    next_resource_id: ResourceId,
}

impl Default for Project {
    fn default() -> Self {
        Self::blank(
            NaiveDate::from_ymd_opt(2026, 1, 5)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        )
    }
}

impl Project {
    pub fn blank(start: NaiveDateTime) -> Self {
        Self {
            name: "Project1".into(),
            author: String::new(),
            company: String::new(),
            start_date: start,
            finish_date: start,
            schedule_from: ScheduleFrom::ProjectStartDate,
            status_date: None,
            current_date: start,
            calendar: WorkCalendar::standard(),
            tasks: Vec::new(),
            links: Vec::new(),
            resources: Vec::new(),
            currency_symbol: "$".into(),
            show_project_summary: false,
            bar_styles: BarStyles::default(),
            next_task_id: 1,
            next_resource_id: 1,
        }
    }

    // ---- identity -------------------------------------------------------

    pub fn allocate_task_id(&mut self) -> TaskId {
        let id = self.next_task_id;
        self.next_task_id += 1;
        id
    }

    pub fn allocate_resource_id(&mut self) -> ResourceId {
        let id = self.next_resource_id;
        self.next_resource_id += 1;
        id
    }

    pub fn index_of(&self, id: TaskId) -> Option<usize> {
        self.tasks.iter().position(|t| t.id == id)
    }

    pub fn task(&self, id: TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn resource(&self, id: ResourceId) -> Option<&Resource> {
        self.resources.iter().find(|r| r.id == id)
    }

    // ---- outline --------------------------------------------------------

    /// A task is a summary when the row below it is indented deeper.
    pub fn is_summary(&self, index: usize) -> bool {
        match (self.tasks.get(index), self.tasks.get(index + 1)) {
            (Some(this), Some(next)) => next.outline_level > this.outline_level,
            _ => false,
        }
    }

    /// Whether a row should be drawn as a milestone marker rather than a bar.
    ///
    /// A summary row carries no duration of its own, so testing the task alone
    /// reports every phase heading as a milestone and the chart loses all its
    /// blocks. A summary always spans its children, so it is never a marker
    /// however its own duration reads.
    pub fn is_marker(&self, index: usize) -> bool {
        !self.is_summary(index)
            && self.tasks.get(index).is_some_and(|task| task.is_milestone())
    }

    /// The contiguous run of rows nested under `index`.
    pub fn descendants(&self, index: usize) -> std::ops::Range<usize> {
        let Some(level) = self.tasks.get(index).map(|t| t.outline_level) else {
            return index..index;
        };
        let mut end = index + 1;
        while end < self.tasks.len() && self.tasks[end].outline_level > level {
            end += 1;
        }
        (index + 1)..end
    }

    /// Leaf rows nested under `index`, or `index` itself when it is a leaf.
    pub fn leaf_indices(&self, index: usize) -> Vec<usize> {
        let range = self.descendants(index);
        if range.is_empty() {
            return vec![index];
        }
        range.filter(|&i| !self.is_summary(i)).collect()
    }

    pub fn parent_index(&self, index: usize) -> Option<usize> {
        let level = self.tasks.get(index)?.outline_level;
        if level == 0 {
            return None;
        }
        (0..index).rev().find(|&i| self.tasks[i].outline_level < level)
    }

    /// The dotted outline number shown in the WBS column: `2.1.3`.
    pub fn wbs(&self, index: usize) -> String {
        let Some(task) = self.tasks.get(index) else {
            return String::new();
        };
        let mut counters: Vec<u32> = Vec::new();
        for row in &self.tasks[..=index] {
            let level = row.outline_level as usize;
            if level + 1 > counters.len() {
                counters.resize(level + 1, 0);
            } else {
                counters.truncate(level + 1);
            }
            counters[level] += 1;
        }
        counters
            .iter()
            .take(task.outline_level as usize + 1)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Rows the grid should draw, honouring collapsed summaries.
    pub fn visible_indices(&self) -> Vec<usize> {
        let mut visible = Vec::with_capacity(self.tasks.len());
        let mut skip_until = 0usize;
        for index in 0..self.tasks.len() {
            if index < skip_until {
                continue;
            }
            visible.push(index);
            if self.tasks[index].collapsed && self.is_summary(index) {
                skip_until = self.descendants(index).end;
            }
        }
        visible
    }

    // ---- editing --------------------------------------------------------

    /// Insert a new task above `index`, inheriting that row's outline level.
    pub fn insert_task(&mut self, index: usize, name: impl Into<String>) -> TaskId {
        let id = self.allocate_task_id();
        let level = self
            .tasks
            .get(index)
            .map(|t| t.outline_level)
            .or_else(|| self.tasks.last().map(|t| t.outline_level))
            .unwrap_or(0);
        let mut task = Task::new(id, name, MINUTES_PER_DAY);
        task.outline_level = level;
        task.estimated = true;
        let at = index.min(self.tasks.len());
        self.tasks.insert(at, task);
        id
    }

    pub fn push_task(&mut self, name: impl Into<String>, duration_minutes: i64) -> TaskId {
        let id = self.allocate_task_id();
        self.tasks.push(Task::new(id, name, duration_minutes));
        id
    }

    /// Delete a row along with everything nested under it.
    pub fn delete_task(&mut self, index: usize) {
        if index >= self.tasks.len() {
            return;
        }
        let end = self.descendants(index).end;
        let removed: Vec<TaskId> = self.tasks[index..end].iter().map(|t| t.id).collect();
        self.tasks.drain(index..end);
        self.links
            .retain(|l| !removed.contains(&l.predecessor) && !removed.contains(&l.successor));
    }

    /// Indent a row one level, carrying its children with it. A row cannot
    /// indent past one level deeper than the row above it.
    pub fn indent(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.tasks.len() {
            return false;
        }
        if self.tasks[index].outline_level > self.tasks[index - 1].outline_level {
            return false;
        }
        let range = self.descendants(index);
        self.tasks[index].outline_level += 1;
        for i in range {
            self.tasks[i].outline_level += 1;
        }
        true
    }

    pub fn outdent(&mut self, index: usize) -> bool {
        if index >= self.tasks.len() || self.tasks[index].outline_level == 0 {
            return false;
        }
        let range = self.descendants(index);
        self.tasks[index].outline_level -= 1;
        for i in range {
            self.tasks[i].outline_level -= 1;
        }
        true
    }

    /// Move a row and its children so they sit before `target`.
    pub fn move_task(&mut self, from: usize, target: usize) {
        if from >= self.tasks.len() {
            return;
        }
        let end = self.descendants(from).end;
        if target >= from && target <= end {
            return;
        }
        let block: Vec<Task> = self.tasks.drain(from..end).collect();
        let insert_at = if target > from { target - block.len() } else { target };
        let insert_at = insert_at.min(self.tasks.len());
        for (offset, task) in block.into_iter().enumerate() {
            self.tasks.insert(insert_at + offset, task);
        }
    }

    // ---- links ----------------------------------------------------------

    pub fn link_exists(&self, predecessor: TaskId, successor: TaskId) -> bool {
        self.links
            .iter()
            .any(|l| l.predecessor == predecessor && l.successor == successor)
    }

    pub fn add_link(&mut self, link: Link) -> bool {
        if link.predecessor == link.successor || self.link_exists(link.predecessor, link.successor) {
            return false;
        }
        self.links.push(link);
        true
    }

    pub fn unlink(&mut self, predecessor: TaskId, successor: TaskId) {
        self.links
            .retain(|l| !(l.predecessor == predecessor && l.successor == successor));
    }

    /// Drop every link touching `id`, used by the Unlink Tasks command.
    pub fn unlink_all(&mut self, id: TaskId) {
        self.links
            .retain(|l| l.predecessor != id && l.successor != id);
    }

    pub fn predecessors_of(&self, id: TaskId) -> Vec<Link> {
        self.links.iter().copied().filter(|l| l.successor == id).collect()
    }

    pub fn successors_of(&self, id: TaskId) -> Vec<Link> {
        self.links.iter().copied().filter(|l| l.predecessor == id).collect()
    }

    /// The Predecessors cell text, for example `3FS+2 days,7SS`.
    pub fn predecessor_text(&self, id: TaskId) -> String {
        let mut parts = Vec::new();
        for link in self.predecessors_of(id) {
            let Some(index) = self.index_of(link.predecessor) else {
                continue;
            };
            let row = index + 1;
            let mut text = row.to_string();
            if link.kind != LinkType::FS || link.lag_minutes != 0 {
                text.push_str(link.kind.code());
            }
            if link.lag_minutes != 0 {
                let sign = if link.lag_minutes > 0 { "+" } else { "-" };
                text.push_str(sign);
                text.push_str(&crate::duration::format_duration(link.lag_minutes.abs()));
            }
            parts.push(text);
        }
        parts.join(",")
    }

    /// Parse a Predecessors cell back into links. Row numbers are 1-based
    /// positions in the current outline, matching what the grid displays.
    pub fn parse_predecessor_text(&self, id: TaskId, text: &str) -> Vec<Link> {
        let mut links = Vec::new();
        for token in text.split(&[',', ';'][..]) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let digits: String = token.chars().take_while(|c| c.is_ascii_digit()).collect();
            let Ok(row) = digits.parse::<usize>() else {
                continue;
            };
            let Some(predecessor) = self.tasks.get(row.saturating_sub(1)).map(|t| t.id) else {
                continue;
            };
            if predecessor == id {
                continue;
            }

            let rest = &token[digits.len()..];
            let (kind_part, lag_part) = match rest.find(['+', '-']) {
                Some(pos) => (&rest[..pos], &rest[pos..]),
                None => (rest, ""),
            };
            let kind = LinkType::parse(kind_part).unwrap_or(LinkType::FS);
            let lag_minutes = if lag_part.is_empty() {
                0
            } else {
                let negative = lag_part.starts_with('-');
                let magnitude = crate::duration::parse_duration(&lag_part[1..])
                    .map(|(m, _)| m)
                    .unwrap_or(0);
                if negative {
                    -magnitude
                } else {
                    magnitude
                }
            };

            links.push(Link {
                predecessor,
                successor: id,
                kind,
                lag_minutes,
            });
        }
        links
    }

    /// Replace every incoming link of `id` with the ones described by `text`.
    pub fn set_predecessor_text(&mut self, id: TaskId, text: &str) {
        let parsed = self.parse_predecessor_text(id, text);
        self.links.retain(|l| l.successor != id);
        for link in parsed {
            if !self.link_exists(link.predecessor, link.successor) {
                self.links.push(link);
            }
        }
    }

    // ---- resources ------------------------------------------------------

    pub fn add_resource(&mut self, name: impl Into<String>) -> ResourceId {
        let id = self.allocate_resource_id();
        self.resources.push(Resource::new(id, name));
        id
    }

    pub fn delete_resource(&mut self, id: ResourceId) {
        self.resources.retain(|r| r.id != id);
        for task in &mut self.tasks {
            task.assignments.retain(|a| a.resource != id);
        }
    }

    /// The Resource Names cell text, for example `Ana Reyes[50%],Rig`.
    pub fn resource_text(&self, task: &Task) -> String {
        task.assignments
            .iter()
            .filter_map(|a| {
                self.resource(a.resource).map(|r| {
                    if (a.units - 1.0).abs() < f64::EPSILON {
                        r.name.clone()
                    } else {
                        format!("{}[{:.0}%]", r.name, a.units * 100.0)
                    }
                })
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse a Resource Names cell, creating any resource that does not exist.
    pub fn set_resource_text(&mut self, task_index: usize, text: &str) {
        let mut assignments = Vec::new();
        for token in text.split(&[',', ';'][..]) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let (name, units) = match (token.find('['), token.find(']')) {
                (Some(open), Some(close)) if close > open => {
                    let raw = token[open + 1..close].trim().trim_end_matches('%');
                    let value: f64 = raw.parse().unwrap_or(100.0);
                    (token[..open].trim(), value / 100.0)
                }
                _ => (token, 1.0),
            };
            if name.is_empty() {
                continue;
            }
            let id = match self.resources.iter().find(|r| r.name.eq_ignore_ascii_case(name)) {
                Some(existing) => existing.id,
                None => self.add_resource(name),
            };
            assignments.push(Assignment { resource: id, units });
        }
        if let Some(task) = self.tasks.get_mut(task_index) {
            task.assignments = assignments;
        }
    }

    // ---- baselines ------------------------------------------------------

    pub fn set_baseline(&mut self) {
        for task in &mut self.tasks {
            task.baseline = Some(Baseline {
                start: task.scheduled.start,
                finish: task.scheduled.finish,
                duration_minutes: task.scheduled.duration_minutes,
                work_minutes: task.scheduled.work_minutes,
                cost: task.scheduled.cost,
            });
        }
    }

    pub fn clear_baseline(&mut self) {
        for task in &mut self.tasks {
            task.baseline = None;
        }
    }

    pub fn has_baseline(&self) -> bool {
        self.tasks.iter().any(|t| t.baseline.is_some())
    }

    // ---- rolled-up totals ----------------------------------------------

    pub fn total_cost(&self) -> f64 {
        (0..self.tasks.len())
            .filter(|&i| !self.is_summary(i))
            .map(|i| self.tasks[i].scheduled.cost)
            .sum()
    }

    pub fn total_work_minutes(&self) -> i64 {
        (0..self.tasks.len())
            .filter(|&i| !self.is_summary(i))
            .map(|i| self.tasks[i].scheduled.work_minutes)
            .sum()
    }

    /// Duration-weighted completion across every leaf task.
    pub fn percent_complete(&self) -> u8 {
        let mut planned = 0i64;
        let mut done = 0i64;
        for index in 0..self.tasks.len() {
            if self.is_summary(index) {
                continue;
            }
            let task = &self.tasks[index];
            let minutes = task.duration_minutes.max(1);
            planned += minutes;
            done += minutes * task.percent_complete as i64 / 100;
        }
        if planned == 0 {
            0
        } else {
            ((done * 100) / planned).clamp(0, 100) as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_never_a_marker_however_its_own_duration_reads() {
        // Phase headings are entered with no duration of their own, since the
        // scheduler rolls their span up from their children. Reading that as
        // "milestone" is what turns every block on a chart into a diamond.
        let start = NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut project = Project::blank(start);
        project.tasks.clear();
        for (level, name, minutes) in [
            (0u16, "Initiation", 0i64),
            (1, "Kickoff", 480),
            (1, "Scope approved", 0),
        ] {
            let id = project.allocate_task_id();
            let mut task = Task::new(id, name, minutes);
            task.outline_level = level;
            project.tasks.push(task);
        }

        assert!(project.is_summary(0));
        assert!(
            project.tasks[0].is_milestone(),
            "it has no duration of its own"
        );
        assert!(!project.is_marker(0), "but it is a block, not a diamond");
        assert!(!project.is_marker(1), "a task with duration is a bar");
        assert!(project.is_marker(2), "a leaf with no duration is a diamond");
    }
}
