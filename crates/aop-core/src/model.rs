//! The project data model.
//!
//! Tasks live in a single flat, ordered `Vec` and express hierarchy through
//! `outline_level`, exactly the way Microsoft Project stores an outline. A task
//! is a summary when the task after it sits one level deeper, so indenting and
//! outdenting are pure integer edits and never restructure a tree.

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::calendar::{CalendarException, WorkCalendar};
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

/// Identifies an external dependency within the plan.
pub type ExternalId = u32;

/// Something outside the plan that work waits on.
///
/// A purchase order, a permit, a delivery, a sign-off held in another system.
/// It is a reference and a date, not a live link: the plan records what it was
/// told, and nothing here goes looking for the truth of it. That keeps a plan
/// openable by someone with no access to the system the reference came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalDependency {
    pub id: ExternalId,
    /// The identifier in the system it came from, such as a PO number.
    pub reference: String,
    /// What it is, in words.
    pub label: String,
    /// Which system it came from, for anyone trying to chase it up.
    #[serde(default)]
    pub source: String,
    /// When it is expected. Work that waits on it cannot start before this.
    pub available: NaiveDateTime,
    #[serde(default)]
    pub notes: String,
}

/// The sort of thing the table flags about a task.
///
/// Kept beside the task because a task remembers which of these it has been
/// told to stop showing, and that choice belongs in the saved plan: dismissing
/// a warning and having it return on reopening would make the dismissal
/// meaningless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueKind {
    /// No slack: any delay moves the project finish.
    Critical,
    /// A constraint is pinning the task rather than its links.
    Constraint,
    /// The task finishes after the deadline it was given.
    MissedDeadline,
    /// Manually scheduled, so its links do not move it.
    ManuallyScheduled,
    /// Switched off, so the scheduler ignores it.
    Inactive,
    /// The calendars this task has to satisfy leave no working time at all, so
    /// there is nowhere for it to be done.
    NoWorkingTime,
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
    /// Which calendar in the plan's library this person keeps to. Empty, or a
    /// name the library does not hold, means the project calendar, which is
    /// what every plan written before resource calendars existed says.
    pub base_calendar: String,
    /// This person's own non-working time on top of that base: leave, a
    /// sabbatical, the two days a week they are not here. A public holiday is
    /// not one of these; it belongs on the calendar everybody shares. Empty for
    /// most people, so it costs nothing in a saved plan.
    #[serde(default)]
    pub calendar_exceptions: Vec<CalendarException>,
    /// What the planner wrote about this person. Empty for most, so it costs
    /// nothing in a saved plan.
    #[serde(default)]
    pub notes: String,
    /// Where to reach them. Not used for anything yet, but it is the first
    /// thing anyone looks for on a resource.
    #[serde(default)]
    pub email: String,
    /// The organisation's own identifier: a payroll number, a cost centre.
    #[serde(default)]
    pub code: String,
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
            calendar_exceptions: Vec::new(),
            notes: String::new(),
            email: String::new(),
            code: String::new(),
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
    /// Set when the calendars this task has to satisfy leave no working time
    /// at all, so its dates were worked out against the project calendar as a
    /// stand-in. False for every task in a plan that has not composed itself
    /// into a corner, so it costs nothing in a saved plan.
    #[serde(default)]
    pub no_working_time: bool,
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
            no_working_time: false,
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
    /// Flags this task has been told to stop showing. Empty for almost every
    /// task, so it costs nothing in a saved plan.
    #[serde(default)]
    pub ignored_issues: Vec<IssueKind>,
    /// Things outside the plan this task waits on.
    #[serde(default)]
    pub external_predecessors: Vec<ExternalId>,
    /// What this task holds in the plan's spare fields. Empty for most tasks,
    /// so it costs nothing in a saved plan.
    #[serde(default)]
    pub custom: crate::custom::CustomValues,
    /// Row colours, when the planner has set them. Empty means the theme's.
    #[serde(default)]
    pub text_colour: String,
    #[serde(default)]
    pub fill_colour: String,
    /// Emphasis the planner put on this row by hand. False is the theme's.
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    /// Empty family and zero size both mean the theme decides, which is what
    /// nearly every row wants, so neither costs anything in a saved plan.
    #[serde(default)]
    pub font_family: String,
    #[serde(default)]
    pub font_size_pt: f32,
    pub collapsed: bool,
    /// Which calendar in the plan's library this task is worked to, when it is
    /// not the project's. Empty means the project's, which is what nearly
    /// every task wants and what every plan written before this existed says.
    #[serde(default)]
    pub calendar: String,
    /// Whether the people assigned to this task are consulted about when it can
    /// happen. Only meaningful when `calendar` names one, which is Project's
    /// own rule: without a task calendar there is nothing left to schedule
    /// against once the resources are dropped.
    #[serde(default)]
    pub ignore_resource_calendars: bool,
    /// Start typed by the user; authoritative only for manually scheduled tasks.
    pub manual_start: Option<NaiveDateTime>,
    pub baseline: Option<Baseline>,
    #[serde(default)]
    pub scheduled: Scheduled,

    // ---- what really happened -------------------------------------------
    //
    // Everything below is defaulted, so a plan saved before any of it existed
    // opens unchanged, and a plan that tracks nothing but percent complete
    // carries none of it in the file.
    /// When work really began. `None` until the task has started.
    #[serde(default)]
    pub actual_start: Option<NaiveDateTime>,
    /// When it really ended. `None` while it is still running.
    #[serde(default)]
    pub actual_finish: Option<NaiveDateTime>,
    /// Work really done, in minutes. Zero means nobody has said, not that
    /// nothing was done: `reported_actual_work_minutes` derives one instead.
    #[serde(default)]
    pub actual_work_minutes: i64,
    /// Money really spent. Zero means nobody has said, for the same reason.
    #[serde(default)]
    pub actual_cost: f64,
    /// Work still to do, in minutes. Zero means derive it from the rest.
    #[serde(default)]
    pub remaining_work_minutes: i64,
    /// How much of the job is judged done, as opposed to how much of the time
    /// has gone. Typed by hand, and only meaningful when somebody has.
    #[serde(default)]
    pub physical_percent_complete: Option<u8>,
    /// Which measure of progress earns value. Affects earned value only.
    #[serde(default)]
    pub earned_value_method: crate::earned_value::EarnedValueMethod,
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
            ignored_issues: Vec::new(),
            external_predecessors: Vec::new(),
            custom: Default::default(),
            text_colour: String::new(),
            fill_colour: String::new(),
            bold: false,
            italic: false,
            underline: false,
            font_family: String::new(),
            font_size_pt: 0.0,
            collapsed: false,
            calendar: String::new(),
            ignore_resource_calendars: false,
            manual_start: None,
            baseline: None,
            scheduled: Scheduled::default(),
            actual_start: None,
            actual_finish: None,
            actual_work_minutes: 0,
            actual_cost: 0.0,
            remaining_work_minutes: 0,
            physical_percent_complete: None,
            earned_value_method: crate::earned_value::EarnedValueMethod::default(),
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

    // ---- actuals --------------------------------------------------------

    /// Whether any work has been reported against this task at all.
    ///
    /// A plan that tracks nothing but percent complete never fills in an actual
    /// start, so progress on its own counts as having begun.
    pub fn has_started(&self) -> bool {
        self.actual_start.is_some() || self.percent_complete > 0
    }

    /// What has really been spent.
    ///
    /// Nothing typed in falls back to what the reported progress implies about
    /// the scheduled cost. Treating a zero as "not entered" rather than as
    /// "free" is the pragmatic reading: a task nobody has costed is far more
    /// common than one that truly cost nothing, and a plan whose only progress
    /// input is percent complete would otherwise report every actual as nil.
    pub fn reported_actual_cost(&self) -> f64 {
        if self.actual_cost != 0.0 {
            return self.actual_cost;
        }
        self.scheduled.cost * self.percent_complete as f64 / 100.0
    }

    /// Work really done, derived from progress when nobody has said.
    pub fn reported_actual_work_minutes(&self) -> i64 {
        if self.actual_work_minutes != 0 {
            return self.actual_work_minutes;
        }
        self.scheduled.work_minutes * self.percent_complete as i64 / 100
    }

    /// Work still to do. A typed figure wins, because a planner who has
    /// re-estimated the remainder knows something the subtraction does not.
    pub fn remaining_work(&self) -> i64 {
        if self.remaining_work_minutes != 0 {
            return self.remaining_work_minutes;
        }
        (self.scheduled.work_minutes - self.reported_actual_work_minutes()).max(0)
    }

    /// The progress figure earned value should read for this task.
    pub fn earned_percent(&self) -> u8 {
        match self.earned_value_method {
            crate::earned_value::EarnedValueMethod::PercentComplete => self.percent_complete,
            // Switching the method is one action and typing a figure is
            // another. Falling back stops a task earning nothing merely
            // because only the first of the two has happened yet.
            crate::earned_value::EarnedValueMethod::PhysicalPercentComplete => self
                .physical_percent_complete
                .unwrap_or(self.percent_complete),
        }
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

/// Which calendar an edit is aimed at.
///
/// The three are deliberately different things rather than three ways of
/// saying the same one. A day the organisation is closed belongs on the project
/// calendar or a base, where it moves everybody. One person being away belongs
/// on that person, where it moves only the tasks they are on. Naming the
/// distinction here rather than in the interface keeps the two from being
/// confused by anything that edits a calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarTarget {
    /// The calendar everything follows unless it says otherwise.
    Project,
    /// A named base in the library.
    Base(String),
    /// One person's own time, on top of whichever base they follow.
    Resource(ResourceId),
}

impl CalendarTarget {
    /// Whether this is one person rather than a calendar people share.
    ///
    /// Worth asking because it changes what an edit means: the same list of
    /// days is somebody's leave here and a public holiday there.
    pub fn is_person(&self) -> bool {
        matches!(self, CalendarTarget::Resource(_))
    }
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
    /// The calendar everything follows unless it says otherwise. It is one
    /// entry in the library, and the one every name resolves to when nothing
    /// else matches.
    pub calendar: WorkCalendar,
    /// The rest of the library: bases a task or a person can name. Empty in a
    /// plan that has only ever used the project calendar, which is every plan
    /// written before this existed.
    #[serde(default)]
    pub calendars: Vec<WorkCalendar>,
    pub tasks: Vec<Task>,
    pub links: Vec<Link>,
    pub resources: Vec<Resource>,
    /// Things outside the plan that work waits on.
    #[serde(default)]
    pub external: Vec<ExternalDependency>,
    /// The spare fields this plan has put to use.
    #[serde(default)]
    pub custom_fields: crate::custom::CustomFields,
    /// Annotation shapes marked on the chart, kept in ascending `z` so the
    /// chart can draw them in the order it finds them.
    #[serde(default)]
    pub drawings: Vec<crate::draw::Drawing>,
    pub currency_symbol: String,
    pub show_project_summary: bool,
    /// How little slack makes a task critical, in minutes. Zero is the usual
    /// answer and the default.
    ///
    /// Project offers this per plan because a chain running across two
    /// calendars, a five day week feeding a seven day one, ends up with a day
    /// or two of slack on every link and no critical path at all at zero. The
    /// setting is how a planner gets the chain back.
    #[serde(default)]
    pub critical_slack_minutes: i64,
    /// Who changed what, and when. Rides with the plan, so the trail survives
    /// being sent to somebody else and is what a sync exchanges.
    #[serde(default)]
    pub history: crate::history::History,
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
            calendars: Vec::new(),
            tasks: Vec::new(),
            links: Vec::new(),
            resources: Vec::new(),
            external: Vec::new(),
            custom_fields: Default::default(),
            drawings: Vec::new(),
            currency_symbol: "$".into(),
            show_project_summary: false,
            critical_slack_minutes: 0,
            history: crate::history::History::new(),
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

    /// The next free external dependency identifier.
    ///
    /// Taken from what is already there rather than from a counter on the
    /// project, so a plan that has been through an import or a merge cannot
    /// hand out an identifier that is already in use.
    pub fn allocate_external_id(&mut self) -> ExternalId {
        self.external.iter().map(|e| e.id).max().unwrap_or(0) + 1
    }

    /// The next free drawing identifier, taken from what is already there for
    /// the same reason `allocate_external_id` is: a plan that has been through
    /// an import or a merge must not hand out an identifier already in use.
    pub fn allocate_drawing_id(&mut self) -> crate::draw::DrawingId {
        self.drawings.iter().map(|d| d.id).max().unwrap_or(0) + 1
    }

    pub fn external(&self, id: ExternalId) -> Option<&ExternalDependency> {
        self.external.iter().find(|entry| entry.id == id)
    }

    /// What a task waits on outside the plan.
    pub fn externals_of(&self, index: usize) -> Vec<&ExternalDependency> {
        let Some(task) = self.tasks.get(index) else {
            return Vec::new();
        };
        task.external_predecessors
            .iter()
            .filter_map(|id| self.external(*id))
            .collect()
    }

    /// What a task shows for a custom field.
    ///
    /// A summary row shows the rollup of its children when the field has one,
    /// because that is the whole point of setting a rollup. Its own typed value
    /// is used otherwise, and when the rollup produces nothing.
    pub fn custom_value(&self, index: usize, slot: crate::custom::Slot) -> String {
        let own = self
            .tasks
            .get(index)
            .and_then(|task| task.custom.get(&slot))
            .cloned()
            .unwrap_or_default();

        let Some(field) = self.custom_fields.get(&slot) else {
            return own;
        };
        if field.rollup == crate::custom::Rollup::None || !self.is_summary(index) {
            return own;
        }

        let children: Vec<String> = self
            .descendants(index)
            .filter(|&child| !self.is_summary(child))
            .map(|child| {
                self.tasks[child]
                    .custom
                    .get(&slot)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();

        field.roll_up(&children).unwrap_or(own)
    }

    /// Put a value into a custom field, if the field will take it.
    pub fn set_custom_value(&mut self, index: usize, slot: crate::custom::Slot, value: &str) -> bool {
        if let Some(field) = self.custom_fields.get(&slot)
            && !field.accepts(value)
        {
            return false;
        }
        let Some(task) = self.tasks.get_mut(index) else {
            return false;
        };
        if value.trim().is_empty() {
            task.custom.remove(&slot);
        } else {
            task.custom.insert(slot, value.trim().to_string());
        }
        true
    }

    /// The earliest a task can start given what it waits on outside the plan.
    pub fn external_ready(&self, index: usize) -> Option<NaiveDateTime> {
        self.externals_of(index)
            .iter()
            .map(|entry| entry.available)
            .max()
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

    // ---- the calendar library -------------------------------------------

    /// Every calendar the plan knows by name, the project's own first.
    ///
    /// The project calendar leads because it is what an unresolved name falls
    /// back to, and a picker that offers it first is offering the answer that
    /// changes nothing.
    pub fn calendar_library(&self) -> impl Iterator<Item = &WorkCalendar> {
        std::iter::once(&self.calendar).chain(self.calendars.iter())
    }

    /// The calendar a name refers to, if the library holds one.
    pub fn calendar_named(&self, name: &str) -> Option<&WorkCalendar> {
        self.calendar_library().find(|cal| cal.name == name)
    }

    pub fn calendar_named_mut(&mut self, name: &str) -> Option<&mut WorkCalendar> {
        if self.calendar.name == name {
            return Some(&mut self.calendar);
        }
        self.calendars.iter_mut().find(|cal| cal.name == name)
    }

    /// The calendar a name refers to, falling back to the project's.
    ///
    /// Falling back rather than failing is deliberate. A name the library has
    /// lost, through an import, a merge, or a calendar someone deleted, must
    /// not stop a plan opening or leave a task with nowhere to be done; it has
    /// to mean what it meant before calendars were named at all.
    pub fn calendar_or_project(&self, name: &str) -> &WorkCalendar {
        if name.trim().is_empty() {
            return &self.calendar;
        }
        self.calendar_named(name).unwrap_or(&self.calendar)
    }

    /// The exceptions an edit aimed at `target` reads.
    ///
    /// A person's are their own rather than their base's: their working week is
    /// shared and is not theirs to change, and that is exactly why their time
    /// off has to live somewhere that is.
    pub fn exceptions_for(&self, target: &CalendarTarget) -> &[CalendarException] {
        match target {
            CalendarTarget::Project => &self.calendar.exceptions,
            CalendarTarget::Base(name) => &self.calendar_or_project(name).exceptions,
            CalendarTarget::Resource(id) => self
                .resource(*id)
                .map(|resource| resource.calendar_exceptions.as_slice())
                .unwrap_or_default(),
        }
    }

    /// The exception list an edit aimed at `target` writes into.
    ///
    /// `None` when the target names something the plan no longer has, so a
    /// stale picker cannot quietly write a holiday onto the wrong calendar.
    pub fn exceptions_for_mut(
        &mut self,
        target: &CalendarTarget,
    ) -> Option<&mut Vec<CalendarException>> {
        match target {
            CalendarTarget::Project => Some(&mut self.calendar.exceptions),
            CalendarTarget::Base(name) => {
                self.calendar_named_mut(name).map(|cal| &mut cal.exceptions)
            }
            CalendarTarget::Resource(id) => self
                .resources
                .iter_mut()
                .find(|resource| resource.id == *id)
                .map(|resource| &mut resource.calendar_exceptions),
        }
    }

    /// What to call the target in a sentence: a calendar's name, a person's.
    pub fn calendar_target_name(&self, target: &CalendarTarget) -> String {
        match target {
            CalendarTarget::Project => self.calendar.name.clone(),
            CalendarTarget::Base(name) => self.calendar_or_project(name).name.clone(),
            CalendarTarget::Resource(id) => self
                .resource(*id)
                .map(|resource| resource.name.clone())
                .unwrap_or_default(),
        }
    }

    /// Add a base calendar to the library under a name nothing else uses.
    ///
    /// Returns the name it ended up with, which is the one a task or a person
    /// then has to be pointed at.
    pub fn add_base_calendar(&mut self, mut calendar: WorkCalendar) -> String {
        if calendar.name.trim().is_empty() {
            calendar.name = "Calendar".into();
        }
        if self.calendar_named(&calendar.name).is_some() {
            let stem = calendar.name.clone();
            let mut suffix = 2u32;
            while self.calendar_named(&format!("{stem} {suffix}")).is_some() {
                suffix += 1;
            }
            calendar.name = format!("{stem} {suffix}");
        }
        let name = calendar.name.clone();
        self.calendars.push(calendar);
        name
    }

    /// Drop a base calendar and put anything that named it back on the project
    /// calendar, so nothing is left pointing at a name that no longer exists.
    pub fn remove_base_calendar(&mut self, name: &str) -> bool {
        let Some(at) = self.calendars.iter().position(|cal| cal.name == name) else {
            return false;
        };
        self.calendars.remove(at);
        for task in &mut self.tasks {
            if task.calendar == name {
                task.calendar.clear();
            }
        }
        for resource in &mut self.resources {
            if resource.base_calendar == name {
                resource.base_calendar.clear();
            }
        }
        true
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
        // Symmetric with the links: a callout pinned to a bar that no longer
        // exists has nothing to hang off, and left behind it would place
        // nowhere for the rest of the plan's life.
        self.drawings
            .retain(|d| d.anchored_task().is_none_or(|task| !removed.contains(&task)));
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

    fn day_off(name: &str, y: i32, m: u32, d: u32) -> CalendarException {
        let date = NaiveDate::from_ymd_opt(y, m, d).unwrap();
        CalendarException {
            name: name.into(),
            from: date,
            to: date,
            shifts: crate::calendar::DayShifts::nonworking(),
        }
    }

    fn planned() -> Project {
        Project::blank(
            NaiveDate::from_ymd_opt(2026, 8, 17)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        )
    }

    #[test]
    fn an_exception_goes_where_the_target_says_and_nowhere_else() {
        // The mistake this exists to stop is silent: a national holiday file
        // landing on one person leaves the plan working everybody else through
        // it, and nothing on screen says so afterwards.
        let mut project = planned();
        let ada = project.add_resource("Ada");

        project
            .exceptions_for_mut(&CalendarTarget::Resource(ada))
            .expect("she is in the plan")
            .push(day_off("Leave", 2026, 3, 3));

        assert_eq!(project.exceptions_for(&CalendarTarget::Resource(ada)).len(), 1);
        assert!(
            project.exceptions_for(&CalendarTarget::Project).is_empty(),
            "her leave is hers, not everybody's"
        );
    }

    #[test]
    fn a_base_calendar_takes_its_own_exceptions() {
        let mut project = planned();
        let name = project.add_base_calendar(WorkCalendar::standard());
        let target = CalendarTarget::Base(name.clone());

        project
            .exceptions_for_mut(&target)
            .expect("just added")
            .push(day_off("Shutdown", 2026, 8, 18));

        assert_eq!(project.exceptions_for(&target).len(), 1);
        assert!(project.calendar.exceptions.is_empty());
        assert!(
            !project
                .calendar_named(&name)
                .expect("in the library")
                .is_working_day(NaiveDate::from_ymd_opt(2026, 8, 18).unwrap())
        );
    }

    #[test]
    fn a_target_the_plan_no_longer_has_writes_nowhere() {
        // A picker can name a calendar that has since been deleted. Falling
        // back to the project calendar there would put a day off in front of
        // everybody, which is the one outcome worth refusing outright.
        let mut project = planned();
        assert!(
            project
                .exceptions_for_mut(&CalendarTarget::Base("Gone".into()))
                .is_none()
        );
        assert!(
            project
                .exceptions_for_mut(&CalendarTarget::Resource(999))
                .is_none()
        );
        assert!(project.exceptions_for(&CalendarTarget::Resource(999)).is_empty());
    }

    #[test]
    fn a_target_names_itself_the_way_a_sentence_needs_it() {
        let mut project = planned();
        let ada = project.add_resource("Ada Lovelace");
        assert_eq!(
            project.calendar_target_name(&CalendarTarget::Project),
            "Standard"
        );
        assert_eq!(
            project.calendar_target_name(&CalendarTarget::Resource(ada)),
            "Ada Lovelace"
        );
        assert!(CalendarTarget::Resource(ada).is_person());
        assert!(!CalendarTarget::Project.is_person());
        assert!(!CalendarTarget::Base("Night Shift".into()).is_person());
    }

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

#[cfg(test)]
mod custom_field_tests {
    use super::*;
    use crate::custom::{CustomField, CustomKind, Rollup, Slot};
    use chrono::NaiveDate;

    fn plan() -> Project {
        let start = NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut project = Project::blank(start);
        project.tasks.clear();
        // A summary with two children under it.
        for (level, name) in [(0u16, "Phase"), (1, "One"), (1, "Two")] {
            let id = project.allocate_task_id();
            let mut task = Task::new(id, name, 480);
            task.outline_level = level;
            project.tasks.push(task);
        }
        project
    }

    fn with_rollup(project: &mut Project, slot: Slot, rollup: Rollup) {
        let mut field = CustomField::new(slot);
        field.rollup = rollup;
        project.custom_fields.insert(slot, field);
    }

    #[test]
    fn a_value_set_on_a_task_reads_back() {
        let mut project = plan();
        let slot = Slot::new(CustomKind::Text, 3);
        assert!(project.set_custom_value(1, slot, "Delivery"));
        assert_eq!(project.custom_value(1, slot), "Delivery");
    }

    #[test]
    fn clearing_a_cell_removes_it_rather_than_storing_an_empty_string() {
        // Otherwise every task a user ever touched carries dead weight in the
        // saved plan.
        let mut project = plan();
        let slot = Slot::new(CustomKind::Text, 1);
        project.set_custom_value(1, slot, "Delivery");
        project.set_custom_value(1, slot, "   ");
        assert!(project.tasks[1].custom.is_empty());
    }

    #[test]
    fn a_summary_row_shows_the_rollup_of_its_children() {
        let mut project = plan();
        let slot = Slot::new(CustomKind::Number, 1);
        with_rollup(&mut project, slot, Rollup::Sum);
        project.set_custom_value(1, slot, "10");
        project.set_custom_value(2, slot, "32");

        assert!(project.is_summary(0));
        assert_eq!(project.custom_value(0, slot), "42");
    }

    #[test]
    fn a_field_with_no_rollup_leaves_the_summary_row_alone() {
        let mut project = plan();
        let slot = Slot::new(CustomKind::Text, 1);
        with_rollup(&mut project, slot, Rollup::None);
        project.set_custom_value(0, slot, "Typed here");
        project.set_custom_value(1, slot, "Child");
        assert_eq!(project.custom_value(0, slot), "Typed here");
    }

    #[test]
    fn a_rollup_that_produces_nothing_falls_back_to_what_was_typed() {
        let mut project = plan();
        let slot = Slot::new(CustomKind::Number, 1);
        with_rollup(&mut project, slot, Rollup::Sum);
        project.set_custom_value(0, slot, "manual");
        assert_eq!(
            project.custom_value(0, slot),
            "manual",
            "no children have values, so the summary keeps its own"
        );
    }

    #[test]
    fn a_restricted_field_refuses_a_value_off_its_list() {
        use crate::custom::LookupValue;
        let mut project = plan();
        let slot = Slot::new(CustomKind::Text, 1);
        let mut field = CustomField::new(slot);
        field.lookup = vec![LookupValue {
            value: "Delivery".into(),
            description: String::new(),
        }];
        field.lookup_only = true;
        project.custom_fields.insert(slot, field);

        assert!(!project.set_custom_value(1, slot, "Legal"));
        assert_eq!(project.custom_value(1, slot), "");
        assert!(project.set_custom_value(1, slot, "Delivery"));
    }
}

#[cfg(test)]
mod drawing_tests {
    use super::*;
    use crate::draw::{Anchor, BarPoint, Drawing, DrawingId, Extent, ShapeKind};

    fn plan() -> Project {
        let start = NaiveDate::from_ymd_opt(2026, 1, 5)
            .and_then(|d| d.and_hms_opt(8, 0, 0))
            .expect("a real date");
        let mut project = Project::blank(start);
        project.push_task("Phase", MINUTES_PER_DAY);
        project.push_task("Child", MINUTES_PER_DAY);
        project.push_task("Other", MINUTES_PER_DAY);
        project.tasks[1].outline_level = 1;
        project
    }

    fn on_task(id: DrawingId, task: TaskId) -> Drawing {
        Drawing::new(
            id,
            ShapeKind::Oval,
            Anchor::Task {
                task,
                point: BarPoint::Middle,
                dx: 0.0,
                dy: 0.0,
            },
            Extent::Fixed { w: 30.0, h: 12.0 },
        )
    }

    fn on_date(id: DrawingId, at: NaiveDateTime) -> Drawing {
        Drawing::new(
            id,
            ShapeKind::Line,
            Anchor::Timescale { at, row: 0.0 },
            Extent::Scaled {
                minutes: 0,
                rows: 4.0,
            },
        )
    }

    #[test]
    fn identifiers_come_from_what_is_already_there() {
        let mut project = plan();
        assert_eq!(project.allocate_drawing_id(), 1);

        // A plan that arrived by import may already hold high identifiers.
        project.drawings.push(on_date(9, project.start_date));
        assert_eq!(project.allocate_drawing_id(), 10);
    }

    #[test]
    fn deleting_a_task_takes_the_shapes_pinned_to_it() {
        let mut project = plan();
        let (summary, child, other) = (
            project.tasks[0].id,
            project.tasks[1].id,
            project.tasks[2].id,
        );
        project.drawings.push(on_task(1, summary));
        project.drawings.push(on_task(2, child));
        project.drawings.push(on_task(3, other));
        project.drawings.push(on_date(4, project.start_date));

        // Deleting the summary takes its child with it, so both shapes go.
        project.delete_task(0);

        let left: Vec<DrawingId> = project.drawings.iter().map(|d| d.id).collect();
        assert_eq!(left, vec![3, 4], "only the surviving task and the date keep theirs");
    }

    #[test]
    fn a_dated_shape_survives_every_task_being_deleted() {
        // It was never about a task, so there is nothing for it to lose.
        let mut project = plan();
        project.drawings.push(on_date(1, project.start_date));
        while !project.tasks.is_empty() {
            project.delete_task(0);
        }
        assert_eq!(project.drawings.len(), 1);
    }

    #[test]
    fn a_plan_saved_with_drawings_reads_back_the_same() {
        let mut project = plan();
        let task = project.tasks[2].id;
        let mut shape = on_task(1, task);
        shape.text = "Waiting on the permit".into();
        shape.behind_bars = true;
        shape.z = 3;
        project.drawings.push(shape);
        project.drawings.push(on_date(2, project.start_date));

        let json = serde_json::to_string(&project).expect("a plan serialises");
        let back: Project = serde_json::from_str(&json).expect("and reads back");

        assert_eq!(back.drawings, project.drawings);
    }

    #[test]
    fn a_plan_saved_before_drawings_existed_still_opens() {
        // The whole point of the serde default: a file written by an earlier
        // build has no `drawings` key at all, and must not fail to load.
        let project = plan();
        let mut json: serde_json::Value =
            serde_json::to_value(&project).expect("a plan serialises");
        json.as_object_mut()
            .expect("a plan is an object")
            .remove("drawings");

        let back: Project = serde_json::from_value(json).expect("an older plan still opens");
        assert!(back.drawings.is_empty());
        assert_eq!(back.tasks.len(), project.tasks.len());
    }
}
