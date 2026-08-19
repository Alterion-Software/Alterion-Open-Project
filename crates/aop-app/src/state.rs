//! Application state and every command the ribbon can fire.
//!
//! One `Signal<AppState>` is provided at the root and read by every component.
//! Mutations go through methods here rather than being written inline in the
//! views, so undo snapshots and rescheduling can never be forgotten.

use std::path::PathBuf;

use aop_core::draw::{
    snap_vertical, Anchor, BarPoint, Drawing, DrawingId, Extent, ShapeKind,
};
use aop_core::grouping::GroupRow;
use aop_core::{
    persist, schedule, templates, CalendarTarget, ConstraintType, Field, Link, LinkType, Project,
    ResourceId, ScheduleReport, Task, TaskId, TaskMode, WorkCalendar,
};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

use crate::gantt::{bar_edges, chart_range, Scale, ROW_H};
use crate::macros::cmd::{Cmd, ResourceField, Row, ViewOption};

const UNDO_LIMIT: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonTab {
    Task,
    Resource,
    Report,
    Project,
    View,
    Format,
    Help,
}

impl RibbonTab {
    pub const ORDER: [RibbonTab; 6] = [
        RibbonTab::Task,
        RibbonTab::Resource,
        RibbonTab::Report,
        RibbonTab::Project,
        RibbonTab::View,
        RibbonTab::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RibbonTab::Task => "Task",
            RibbonTab::Resource => "Resource",
            RibbonTab::Report => "Report",
            RibbonTab::Project => "Project",
            RibbonTab::View => "View",
            RibbonTab::Format => "Format",
            RibbonTab::Help => "Help",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    GanttChart,
    TrackingGantt,
    TaskSheet,
    TaskUsage,
    NetworkDiagram,
    CalendarView,
    ResourceSheet,
    ResourceUsage,
    TeamPlanner,
    /// Each report stands on its own rather than sharing one page: a report is
    /// figures, a chart and the rows behind it, and four of those crammed
    /// together is a dashboard, which is a different thing.
    Burndown,
    Burnup,
    Velocity,
    CriticalPath,
}

impl ViewKind {
    pub fn label(self) -> &'static str {
        match self {
            ViewKind::GanttChart => "Gantt Chart",
            ViewKind::TrackingGantt => "Tracking Gantt",
            ViewKind::TaskSheet => "Task Sheet",
            ViewKind::TaskUsage => "Task Usage",
            ViewKind::NetworkDiagram => "Network Diagram",
            ViewKind::CalendarView => "Calendar",
            ViewKind::ResourceSheet => "Resource Sheet",
            ViewKind::ResourceUsage => "Resource Usage",
            ViewKind::TeamPlanner => "Team Planner",
            ViewKind::Burndown => "Burndown",
            ViewKind::Burnup => "Burnup",
            ViewKind::Velocity => "Velocity",
            ViewKind::CriticalPath => "Critical Path",
        }
    }

    /// The contextual ribbon tab that appears above Format for this view.
    pub fn tools_label(self) -> &'static str {
        match self {
            ViewKind::GanttChart => "Gantt Chart Tools",
            ViewKind::TrackingGantt => "Tracking Gantt Tools",
            ViewKind::TaskSheet => "Task Sheet Tools",
            ViewKind::TaskUsage => "Task Usage Tools",
            ViewKind::NetworkDiagram => "Network Diagram Tools",
            ViewKind::CalendarView => "Calendar Tools",
            ViewKind::ResourceSheet => "Resource Sheet Tools",
            ViewKind::ResourceUsage => "Resource Usage Tools",
            ViewKind::TeamPlanner => "Team Planner Tools",
            ViewKind::Burndown
            | ViewKind::Burnup
            | ViewKind::Velocity
            | ViewKind::CriticalPath => "Report Tools",
        }
    }

    /// Whether the view draws a timescale down the right-hand side.
    pub fn has_chart(self) -> bool {
        matches!(self, ViewKind::GanttChart | ViewKind::TrackingGantt)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackstagePage {
    Home,
    Info,
    New,
    Open,
    Save,
    SaveAs,
    Print,
    Export,
    Import,
    About,
    Options,
}

impl BackstagePage {
    pub fn label(self) -> &'static str {
        match self {
            BackstagePage::Home => "Home",
            BackstagePage::Info => "Info",
            BackstagePage::New => "New",
            BackstagePage::Open => "Open",
            BackstagePage::Save => "Save",
            BackstagePage::SaveAs => "Save As",
            BackstagePage::Print => "Print",
            BackstagePage::Export => "Export",
            BackstagePage::Import => "Import",
            BackstagePage::About => "About",
            BackstagePage::Options => "Options",
        }
    }

    /// The glyph shown beside the entry in the File menu.
    pub fn glyph(self) -> &'static str {
        match self {
            BackstagePage::Home => "home",
            BackstagePage::Info => "info-circle",
            BackstagePage::New => "file-new",
            BackstagePage::Open => "folder-open",
            BackstagePage::Save => "save-mono",
            BackstagePage::SaveAs => "save-as",
            BackstagePage::Print => "printer",
            BackstagePage::Export => "file-output",
            BackstagePage::Import => "file-input",
            BackstagePage::About => "badge-info",
            BackstagePage::Options => "settings",
        }
    }
}

/// The window's inner size, so a floating panel can tell how much room it has
/// before an edge. Nothing in a click event says where the screen edges are.
///
/// Held in a signal of its own rather than on the plan: it changes for reasons
/// that have nothing to do with the document, and sharing a signal would mean
/// every window resize re-rendered the whole application.
pub type Viewport = (f64, f64);

/// Where a dragged row will land relative to the row under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropWhere {
    Above,
    Below,
    /// Nest the dragged block under the target as a child.
    Into,
}

/// Categories down the left of the Options page, as Project groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsPage {
    General,
    Display,
    Schedule,
    Save,
    Advanced,
    Collaborate,
    Keyboard,
    CustomizeRibbon,
    QuickAccess,
}

impl OptionsPage {
    pub const ORDER: [OptionsPage; 9] = [
        OptionsPage::General,
        OptionsPage::Display,
        OptionsPage::Schedule,
        OptionsPage::Save,
        OptionsPage::Advanced,
        OptionsPage::Collaborate,
        OptionsPage::Keyboard,
        OptionsPage::CustomizeRibbon,
        OptionsPage::QuickAccess,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OptionsPage::General => "General",
            OptionsPage::Display => "Display",
            OptionsPage::Schedule => "Schedule",
            OptionsPage::Save => "Save",
            OptionsPage::Advanced => "Advanced",
            OptionsPage::Collaborate => "Alterion Collaborate",
            OptionsPage::Keyboard => "Keyboard",
            OptionsPage::CustomizeRibbon => "Customize Ribbon",
            OptionsPage::QuickAccess => "Quick Access Toolbar",
        }
    }
}

/// A column in the task table: which field it shows and how wide it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSpec {
    pub field: Field,
    pub width: f64,
}

/// The task the pointer is over, shared by every pane that draws it.
///
/// Deliberately its own signal rather than a field on `AppState`. Pointing at
/// something is not a change to the plan, and putting it in the plan's state
/// invalidated the chart's layout memo on every bar the pointer crossed, which
/// meant rebuilding a tick per day of the plan just to move a highlight.
///
/// Shared rather than per pane, because that is the point: a report and the
/// chart beside it are two views of one row, and pointing at either should
/// light up both.
#[derive(Clone, Copy)]
pub struct Hovered(pub dioxus::prelude::Signal<Option<usize>>);

/// Where this planner's own pointer is, in plan coordinates, for the others.
///
/// Its own signal for the same reason `Hovered` is, and more so. A mouse
/// produces events faster than anything should redraw for, and this is written
/// on every one of them: putting it on `AppState` would redraw the window
/// whenever the pointer moved a pixel. Nothing renders from it. The timer that
/// already reads the live socket picks it up, which is also what throttles it
/// to a few messages a second rather than one per movement.
#[derive(Clone, Copy)]
pub struct Pointing(pub dioxus::prelude::Signal<Option<crate::cloud::live::Pointer>>);

impl ColumnSpec {
    pub fn new(field: Field) -> Self {
        Self {
            width: field.default_width(),
            field,
        }
    }
}

/// The columns a plan opens with, matching Project's Entry table.
pub fn default_columns() -> Vec<ColumnSpec> {
    Field::ENTRY_TABLE.iter().copied().map(ColumnSpec::new).collect()
}

/// Width of the default table, which is where the splitter starts.
pub const DEFAULT_COLUMNS_WIDTH: f64 = 660.0;

/// Which internal pane is showing, for the split views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Both,
    TableOnly,
    ChartOnly,
}

/// A command that can sit on the Quick Access Toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QatCommand {
    New,
    Open,
    Save,
    Print,
    Export,
    Undo,
    Redo,
    Link,
    Unlink,
    TaskInformation,
    AssignResources,
    ProjectInformation,
    SetBaseline,
    ScrollToTask,
    ZoomIn,
    ZoomOut,
    /// Open or close History and Sync, which is where everything to do with
    /// the server is answered.
    Cloud,
    /// Start live editing and copy the link to this plan, in one press.
    Collaborate,
}

impl QatCommand {
    pub const ALL: [QatCommand; 18] = [
        QatCommand::New,
        QatCommand::Open,
        QatCommand::Save,
        QatCommand::Print,
        QatCommand::Export,
        QatCommand::Undo,
        QatCommand::Redo,
        QatCommand::Link,
        QatCommand::Unlink,
        QatCommand::TaskInformation,
        QatCommand::AssignResources,
        QatCommand::ProjectInformation,
        QatCommand::SetBaseline,
        QatCommand::ScrollToTask,
        QatCommand::ZoomIn,
        QatCommand::ZoomOut,
        QatCommand::Cloud,
        QatCommand::Collaborate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            QatCommand::New => "New",
            QatCommand::Open => "Open",
            QatCommand::Save => "Save",
            QatCommand::Print => "Print",
            QatCommand::Export => "Export",
            QatCommand::Undo => "Undo",
            QatCommand::Redo => "Redo",
            QatCommand::Link => "Link the Selected Tasks",
            QatCommand::Unlink => "Unlink Tasks",
            QatCommand::TaskInformation => "Task Information",
            QatCommand::AssignResources => "Assign Resources",
            QatCommand::ProjectInformation => "Project Information",
            QatCommand::SetBaseline => "Set Baseline",
            QatCommand::ScrollToTask => "Scroll to Task",
            QatCommand::ZoomIn => "Zoom In",
            QatCommand::ZoomOut => "Zoom Out",
            QatCommand::Cloud => "History and Sync",
            // What it does rather than what it is about, because it does two
            // things at once and a planner should not have to press it to find
            // out which. The toolbar says which of the two it will do next.
            QatCommand::Collaborate => "Collaborate: Start Live Editing and Copy the Link",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            QatCommand::New => "new",
            QatCommand::Open => "open",
            QatCommand::Save => "save",
            QatCommand::Print => "print",
            QatCommand::Export => "export",
            QatCommand::Undo => "undo",
            QatCommand::Redo => "redo",
            QatCommand::Link => "link",
            QatCommand::Unlink => "unlink",
            QatCommand::TaskInformation => "information",
            QatCommand::AssignResources => "assign-resources",
            QatCommand::ProjectInformation => "project-info",
            QatCommand::SetBaseline => "baseline",
            QatCommand::ScrollToTask => "scroll-to-task",
            QatCommand::ZoomIn => "zoom-in",
            QatCommand::ZoomOut => "zoom-out",
            QatCommand::Cloud => "cloud",
            QatCommand::Collaborate => "share",
        }
    }

    fn key(self) -> &'static str {
        match self {
            QatCommand::New => "new",
            QatCommand::Open => "open",
            QatCommand::Save => "save",
            QatCommand::Print => "print",
            QatCommand::Export => "export",
            QatCommand::Undo => "undo",
            QatCommand::Redo => "redo",
            QatCommand::Link => "link",
            QatCommand::Unlink => "unlink",
            QatCommand::TaskInformation => "task-info",
            QatCommand::AssignResources => "assign",
            QatCommand::ProjectInformation => "project-info",
            QatCommand::SetBaseline => "baseline",
            QatCommand::ScrollToTask => "scroll",
            QatCommand::ZoomIn => "zoom-in",
            QatCommand::ZoomOut => "zoom-out",
            QatCommand::Cloud => "cloud",
            QatCommand::Collaborate => "collaborate",
        }
    }

    fn from_key(key: &str) -> Option<QatCommand> {
        QatCommand::ALL.into_iter().find(|c| c.key() == key)
    }
}

/// The buttons a fresh install starts with.
const DEFAULT_QAT: [QatCommand; 7] = [
    QatCommand::Save,
    QatCommand::Undo,
    QatCommand::Redo,
    QatCommand::Link,
    QatCommand::Unlink,
    QatCommand::TaskInformation,
    QatCommand::AssignResources,
];

/// Which rows the views show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    All,
    Critical,
    Milestones,
    Incomplete,
}

impl TaskFilter {
    pub fn label(self) -> &'static str {
        match self {
            TaskFilter::All => "All Tasks",
            TaskFilter::Critical => "Critical",
            TaskFilter::Milestones => "Milestones",
            TaskFilter::Incomplete => "Incomplete",
        }
    }
}

/// What dragging a Gantt bar is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarDragKind {
    /// Slide the whole bar, which pins the task with a start constraint.
    Move,
    /// Pull the right edge to change the duration.
    Resize,
    /// Pull from the left edge to set percent complete.
    Progress,
    /// Shift-drag onto another bar to link them.
    Link,
}

/// An in-progress Gantt bar drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarDrag {
    pub row: usize,
    pub kind: BarDragKind,
    pub origin_x: f64,
    pub delta_x: f64,
    pub base_start: NaiveDateTime,
    pub base_duration: i64,
    pub base_percent: u8,
    pub bar_width: f64,
    /// The bar the pointer is currently over, used when linking.
    pub hover_row: Option<usize>,
}

impl BarDrag {
    /// Whole days the pointer has travelled at this zoom level.
    pub fn days(&self, px_per_day: f64) -> i64 {
        if px_per_day <= 0.0 {
            return 0;
        }
        (self.delta_x / px_per_day).round() as i64
    }

    pub fn preview_percent(&self) -> u8 {
        if self.bar_width <= 0.0 {
            return self.base_percent;
        }
        let shift = self.delta_x / self.bar_width * 100.0;
        (self.base_percent as f64 + shift).clamp(0.0, 100.0) as u8
    }
}

/// What a drawing drag is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawDragKind {
    /// Rubber-banding a new shape of this kind onto the chart.
    New(ShapeKind),
    /// Sliding a shape that is already there.
    Move(DrawingId),
}

/// An in-progress drawing drag, in the chart body's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawDrag {
    pub kind: DrawDragKind,
    /// Where the pointer went down.
    ///
    /// Filled in by the first sample the chart's overlay reports rather than at
    /// mousedown, because a move begins on the shape itself and an event on a
    /// shape is measured against that shape, not against the canvas. Waiting
    /// for the overlay costs nothing visible: the shape simply does not stir
    /// until the pointer has actually moved.
    pub origin: Option<(f64, f64)>,
    pub at: (f64, f64),
}

impl DrawDrag {
    /// How far the pointer has travelled since it went down.
    pub fn delta(&self) -> (f64, f64) {
        match self.origin {
            Some((x, y)) => (self.at.0 - x, self.at.1 - y),
            None => (0.0, 0.0),
        }
    }

    /// The rubber band being pulled out, once there is one to draw.
    pub fn band(&self) -> Option<(f64, f64, f64, f64)> {
        let (x, y) = self.origin?;
        Some((x, y, self.at.0 - x, self.at.1 - y))
    }
}

/// Shortest drag that counts as one, in pixels. Below this the planner clicked.
const MIN_DRAW: f64 = 4.0;

/// What a shape gets when it was clicked rather than dragged out.
///
/// A zero-sized shape is invisible and has nothing to click on, so a click
/// meant to place something would look like it had done nothing at all. Lines
/// default to upright, because the vertical gate marker is what they are for.
fn drawn_size(kind: ShapeKind, dx: f64, dy: f64, px_per_day: f64) -> (f64, f64) {
    if dx.abs() >= MIN_DRAW || dy.abs() >= MIN_DRAW {
        return (dx, dy);
    }
    match kind {
        ShapeKind::Line | ShapeKind::Arrow => (0.0, 3.0 * ROW_H),
        ShapeKind::TextBox => (110.0, ROW_H),
        ShapeKind::Rectangle | ShapeKind::Oval => (3.0 * px_per_day.max(1.0), 2.0 * ROW_H),
    }
}

/// An open right-click menu and the screen position it was opened at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContextMenu {
    Task { row: usize, x: f64, y: f64 },
    Chart { x: f64, y: f64 },
    Resource { index: usize, x: f64, y: f64 },
    Column { index: usize, x: f64, y: f64 },
}

impl ContextMenu {
    pub fn position(self) -> (f64, f64) {
        match self {
            ContextMenu::Task { x, y, .. } => (x, y),
            ContextMenu::Chart { x, y } => (x, y),
            ContextMenu::Resource { x, y, .. } => (x, y),
            ContextMenu::Column { x, y, .. } => (x, y),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    TaskInformation(usize),
    /// How levelling should behave before it runs.
    LevelingOptions,
    /// Pick another plan to bring in under a summary row.
    InsertSubproject,
    /// Everything in this plan that reaches outside it.
    LinksBetweenProjects,
    /// Move progress or remaining work relative to a status date.
    UpdateProject,
    /// The look each category of row gets.
    TextStyles,
    /// How bars and links are drawn in the chart.
    Layout,
    /// One annotation shape: its words, its colours, its line.
    FormatDrawing(aop_core::draw::DrawingId),
    /// Everything about one person, in the same shape Project puts it.
    /// The tab says which page opens first, so Notes can go straight there.
    ResourceInformation { row: usize, tab: usize },
    /// Preview of a starter template before creating it.
    TemplatePreview(String),
    ProjectInformation,
    AssignResources,
    ChangeWorkingTime,
    CustomizeQat,
    BarStyles,
    /// Offers a repair for a plan that will not schedule.
    FixIssue,
    /// Set up the plan's spare fields: rename, lookup list, rollup, indicators.
    CustomFields,
    /// Things outside the plan that work waits on.
    ExternalDependencies,
    /// Asks what to do about unsaved work before something discards it.
    UnsavedChanges(PendingAction),
    /// Offers back work left behind by a session that never finished.
    Recover(crate::recovery::Recovered),
    /// Pick a field to insert as a column, at the given position.
    InsertColumn(usize),
    /// Who changed what, and when, newest first.
    History,
    /// Somebody pushed first. What they did, and the choice about it.
    SyncBehind {
        /// The server's head, which is what this copy syncs to if it takes
        /// the changes.
        head: i64,
        /// What their work did, in one sentence.
        sentence: String,
        /// The differences themselves, worked out before the question was
        /// asked so the answer is against something real.
        differences: Vec<aop_core::compare::Difference>,
        /// The entries, for the plan's own log.
        changes: Vec<aop_core::history::Change>,
        /// How many of their commands could be replayed here, and how many
        /// there were. Fewer replayed than came means the two sides have
        /// already drifted, which is decided before anything is applied.
        replayed: usize,
        asked: usize,
        /// Whether more is waiting beyond this page.
        more: bool,
    },
    /// This copy's cursor is past the server's, so the two are not the same
    /// log and pushing would interleave two histories.
    SyncAhead { head: i64, cursor: i64 },
    /// Replaying is no longer possible, so only a whole plan will do.
    FreshCopy { why: String },
    /// Put the plan back to one of its versions.
    RestoreVersion(usize),
    /// What the collaborate health check found.
    HealthCheck,
    /// A newer release, what this copy may do about it, and how to get it
    /// where it may not.
    UpdateAvailable,
    /// A link somebody opened, and the server it says to go and ask.
    ///
    /// Always asked before anything is fetched. A link is an instruction from
    /// a stranger to talk to a host of their choosing, and which host that is
    /// belongs in front of the person before the request goes, not in a log
    /// afterwards.
    OpenLink(crate::cloud::share::Share),
    Message { title: String, body: String },
}

/// What the user was doing when unsaved work got in the way.
///
/// Held so the answer to "save first?" can be acted on and then the original
/// action carried out, rather than the user having to ask for it twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// Quit the application.
    Quit,
    /// Close the plan and start an empty one.
    CloseProject,
    /// Start a new plan from a template.
    NewFromTemplate(String),
    /// Open a file from disk.
    Open(PathBuf),
    /// Take the plan the Import page has built and made ready.
    ///
    /// The plan itself is not carried here: it is held on the state, because
    /// this has to be comparable and a plan is not. It also has to survive the
    /// unsaved changes dialog, which is exactly what the staging slot is for.
    AdoptImport,
}

impl PendingAction {
    /// What the buttons should say this will discard.
    pub fn describe(&self) -> &'static str {
        match self {
            PendingAction::Quit => "closing",
            PendingAction::CloseProject => "closing this plan",
            PendingAction::NewFromTemplate(_) => "starting a new plan",
            PendingAction::Open(_) => "opening another plan",
            PendingAction::AdoptImport => "importing a spreadsheet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    Days,
    Weeks,
    Months,
    Quarters,
}

impl Zoom {
    pub const ORDER: [Zoom; 4] = [Zoom::Days, Zoom::Weeks, Zoom::Months, Zoom::Quarters];

    pub fn label(self) -> &'static str {
        match self {
            Zoom::Days => "Days",
            Zoom::Weeks => "Weeks",
            Zoom::Months => "Months",
            Zoom::Quarters => "Quarters",
        }
    }

    /// Horizontal pixels per calendar day at this zoom level.
    pub fn px_per_day(self) -> f64 {
        match self {
            Zoom::Days => 26.0,
            Zoom::Weeks => 9.0,
            Zoom::Months => 3.4,
            Zoom::Quarters => 1.3,
        }
    }

    pub fn zoom_in(self) -> Zoom {
        match self {
            Zoom::Quarters => Zoom::Months,
            Zoom::Months => Zoom::Weeks,
            _ => Zoom::Days,
        }
    }

    pub fn zoom_out(self) -> Zoom {
        match self {
            Zoom::Days => Zoom::Weeks,
            Zoom::Weeks => Zoom::Months,
            _ => Zoom::Quarters,
        }
    }
}

/// The grid column a cell edit is targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Name,
    Duration,
    Start,
    Finish,
    Predecessors,
    Resources,
}

/// How often the interface looks at the live socket.
///
/// The socket runs on a thread of its own and the plan may only be written
/// where the interface does, so arrivals are collected rather than pushed.
/// Fast enough that somebody else's typing appears while they are still doing
/// it, slow enough that it is not a redraw loop.
pub const LIVE_POLL_MILLIS: u64 = 350;

/// What the network is doing, so a button can say so rather than looking
/// broken.
///
/// A sign in waits on a person in a browser, which can be minutes. Anything
/// that long has to say what it is waiting for, or it gets pressed again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Working {
    SigningIn,
    SigningOut,
    Syncing,
    Fetching,
    Publishing,
    Checking,
    RunningHealthCheck,
    ReadingAccount,
    ReadingSharing,
    ChangingSharing,
}

impl Working {
    /// What to show while it runs.
    pub fn waiting(self) -> &'static str {
        match self {
            Working::SigningIn => {
                "Your browser has opened. This window is waiting for you to finish signing in there."
            }
            Working::SigningOut => "Signing out...",
            Working::Syncing => "Sending your changes and asking for anything new...",
            Working::Fetching => "Fetching a fresh copy of the plan from the server...",
            Working::Publishing => "Putting this plan on the server...",
            Working::Checking => "Asking the server where this plan has got to...",
            Working::RunningHealthCheck => "Checking the server and the sign in...",
            Working::ReadingAccount => "Reading your account details again...",
            Working::ReadingSharing => "Asking the server who this plan is shared with...",
            Working::ChangingSharing => "Telling the server who this plan is shared with...",
        }
    }
}

/// What the server said when it was last asked, and when that was.
///
/// Kept rather than inferred, because "nothing has changed here since I last
/// pushed" and "the server agrees this is the latest version" are different
/// claims, and only the second one is worth a tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub at: NaiveDateTime,
    pub outcome: CheckOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Asked, and the server agreed this copy has everything.
    Current,
    /// The server holds work this copy has not read.
    Behind { by: i64 },
    /// The question could not be put.
    Failed(String),
}

/// What happened when somebody else's work was brought in.
///
/// Two counts and a verdict, because a batch can go wrong in two ways: a
/// command that will not replay here, and a difference that will not apply.
/// Either means the two sides no longer agree about what the plan was.
#[derive(Debug, Clone, PartialEq)]
pub struct BroughtIn {
    pub applied: aop_core::compare::Applied,
    /// How many of the incoming commands could be replayed here.
    pub replayed: usize,
    pub sent: usize,
}

impl BroughtIn {
    /// Whether everything took. False means the honest next step is a whole
    /// plan, not carrying on.
    pub fn is_clean(&self) -> bool {
        self.applied.is_clean() && self.replayed == self.sent
    }

    /// Why it is not clean, for a dialog that has to say.
    pub fn why(&self) -> String {
        let mut parts = Vec::new();
        if self.replayed < self.sent {
            parts.push(format!(
                "{} of the {} commands that came in could not be replayed here",
                self.sent - self.replayed,
                self.sent
            ));
        }
        if !self.applied.is_clean() {
            parts.push(format!(
                "{} of the changes they made do not fit this copy of the plan",
                self.applied.rejected.len()
            ));
        }
        parts.join(", and ")
    }
}

/// Which machine a session is bound to, in a form worth showing.
///
/// Enough of the fingerprint to tell one of your machines from another, and no
/// more: the whole value identifies a machine and has no business being copied
/// about. A session is sealed to it, so a stolen file is no use elsewhere.
fn describe_device() -> Option<String> {
    let components = crate::cloud::device::components().ok()?;
    let fingerprint = components.fingerprint_hex();
    let short: String = fingerprint.chars().take(8).collect();
    Some(format!("{} ({short})", components.platform))
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecentEntry {
    pub name: String,
    pub path: PathBuf,
}

/// How stale the account details may get before something worth a request has
/// happened. Long enough that alt-tabbing or reopening a page costs nothing,
/// short enough that a picture uploaded a moment ago appears on the way back.
/// Whether a file browser should offer this file.
///
/// One answer, because there were three and they disagreed: the Open page
/// showed no workbook at all, the browser showed `.xlsx` but not `.xls`,
/// `.xlsm` or `.ods`, and `open_any` opens all of them. A file the
/// application can read but will not show is indistinguishable from one it
/// cannot read.
///
/// `saving` narrows it to what Save As can write, which is the plan format
/// alone: saving a plan as a workbook would lose most of it.
pub fn offered_in_browser(path: &std::path::Path, saving: bool) -> bool {
    let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if extension.eq_ignore_ascii_case(aop_core::persist::FILE_EXTENSION) {
        return true;
    }
    !saving
        && aop_core::persist::IMPORTED_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
}

/// The exceptions an import aimed at `target` is measured against, in the shape
/// `aop_core::holidays` works in.
///
/// That module reads and writes a `WorkCalendar`, and a person in this model
/// has an exception list rather than a calendar of their own: their working
/// week is their base's and is shared. Lending them one carrying their own
/// exceptions is what lets the preview and the import ask the same question of
/// the same list, so the count shown can never be a count of something else.
///
/// The week is the Standard one and is never read. Only the exceptions matter
/// here, and an empty week would be a calendar that panics if anything ever
/// looked at it.
pub fn target_calendar(project: &Project, target: &CalendarTarget) -> WorkCalendar {
    let mut carrier = WorkCalendar::standard();
    carrier.name = project.calendar_target_name(target);
    carrier.exceptions = project.exceptions_for(target).to_vec();
    carrier
}

/// The places a browser can start from when there is nowhere above the
/// current folder.
///
/// On Windows the parent of `C:\` is nothing, so walking up from a folder
/// strands you on whichever drive you began on: a workbook on `D:` or a
/// network share simply cannot be reached. Every other platform has one root
/// that contains everything, so this is empty there and the Up button alone
/// is enough.
pub fn browser_roots() -> Vec<(String, std::path::PathBuf)> {
    #[cfg(windows)]
    {
        // Probing letters is how this is done without pulling in a Windows
        // API crate for one list; a drive that is not there simply fails.
        (b'A'..=b'Z')
            .map(|letter| format!("{}:\\", letter as char))
            .filter(|root| std::path::Path::new(root).exists())
            .map(|root| (root.clone(), std::path::PathBuf::from(root)))
            .collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub const ACCOUNT_RECHECK: std::time::Duration = std::time::Duration::from_secs(20);

pub struct AppState {
    pub project: Project,
    /// The last scheduling result, or the error explaining why it failed.
    pub report: Result<ScheduleReport, String>,
    pub file_path: Option<PathBuf>,
    pub dirty: bool,

    pub selection: Vec<usize>,
    pub selected_resource: Option<usize>,
    /// How levelling should behave, kept between runs like Project keeps it.
    pub leveling: aop_core::leveling::LevelingOptions,
    pub editing: Option<(usize, Column)>,
    /// The text in the cell being edited, when that cell is one the picker
    /// also writes to.
    ///
    /// Held here rather than inside the editor because two things edit a
    /// predecessor cell at once: the planner typing, and the picker ticking
    /// boxes. A draft owned by the input would go stale the moment a box was
    /// ticked, and blur would then write the stale text back over the change.
    pub cell_draft: String,
    /// Bumped only when the picker changes the plan, so the cell's text box
    /// knows to pull the new text in. Watching `cell_draft` itself would not
    /// do: the box would then be reset by every unrelated change in the
    /// application, wiping out whatever was being typed.
    pub picker_edits: u64,
    /// The column the cursor last sat in. Fill Down works down a column, so it
    /// has to know which one without the grid holding a full cell cursor.
    pub fill_field: Option<Field>,
    /// How deep inside a running macro we are.
    ///
    /// Every mutating method snapshots the plan and reschedules. That is right
    /// for a button press and ruinous for a script: a macro touching five
    /// thousand fields would clone the whole project five thousand times, run
    /// the critical path pass as often, and push the planner's real history
    /// off the end of the undo stack. While this is non zero the snapshots are
    /// suppressed and the reschedule is deferred, so a macro is one step to
    /// undo and one pass to schedule, however much it does.
    macro_depth: u32,
    /// A reschedule was asked for while deferred and is still owed.
    reschedule_owed: bool,
    /// Commands seen but not yet written into the change log.
    ///
    /// Held so a run of them can become one entry: the rows selected before an
    /// edit belong with it, and everything inside one grouped step is one
    /// thing the planner did. Each command is kept beside the sentence that
    /// describes it, worked out while the plan still looked as it did when the
    /// command was given.
    pending: Vec<(Cmd, String)>,
    /// True while a script is being replayed.
    ///
    /// The commands a replay carries out were recorded when they were first
    /// done, and the run writes one entry of its own, so recording them again
    /// would count the same work twice.
    replaying: bool,
    /// How the rows are banded, when the planner has asked for that.
    pub group_by: Option<aop_core::grouping::GroupBy>,
    /// The look each category of row gets, before any row's own formatting.
    pub text_styles: aop_core::textstyle::TextStyles,
    /// A look lifted off one row, waiting to be brushed onto others.
    pub painter: Option<aop_core::textstyle::Painter>,
    /// Which rules are drawn behind the rows, and how bars are laid out.
    pub grid_rows: bool,
    pub grid_columns: bool,
    pub grid_status_date: bool,
    pub round_bars: bool,
    pub show_links: bool,
    pub bar_text: bool,
    /// What to carry out once a Save As has finished, when the save was asked
    /// for in order to get past unsaved work.
    pub after_save: Option<PendingAction>,
    /// Set when the plan is safe to abandon and the window should now close.
    pub quit_requested: bool,
    /// Screen position to anchor a cell popup at.
    pub popup_at: (f64, f64),
    /// Which cell of the selected resource row is being edited, if any.
    pub editing_resource_field: Option<String>,

    pub tab: RibbonTab,
    pub view: ViewKind,
    pub backstage: Option<BackstagePage>,
    pub dialog: Option<Dialog>,
    pub context_menu: Option<ContextMenu>,
    /// A Gantt bar being dragged.
    pub bar_drag: Option<BarDrag>,
    /// The drawing tool the ribbon has armed, if any. While one is armed the
    /// chart takes a drag as a shape rather than as a bar move.
    pub draw_tool: Option<ShapeKind>,
    pub selected_drawing: Option<DrawingId>,
    pub draw_drag: Option<DrawDrag>,
    pub show_drawings: bool,
    /// Where to sign in, and as what. Not secret: a desktop application is a
    /// public client and proves itself with PKCE rather than with anything it
    /// would have to keep hidden on disk.
    pub idp_issuer: String,
    pub idp_client_id: String,
    /// Where Manage account opens, when a deployment has moved that page off
    /// its issuer. Empty is the ordinary case and means "under the issuer".
    pub idp_account_url: String,
    /// Where the sync server lives. A different machine from the provider, so
    /// a different address.
    pub collaborate_server: String,
    pub collaborate: bool,

    // ---- the licence, what changed, and updates -------------------------
    /// The start up pages this launch still owes, front first.
    pub greetings: Vec<crate::welcome::Greeting>,
    /// Which version's licence was acknowledged, and when. Empty until it has
    /// been, and that emptiness is the only thing that shows it.
    pub licence_acknowledged: String,
    pub licence_acknowledged_at: String,
    /// The version this copy last started as, written down at start up so
    /// "once per update" survives being closed part way through.
    pub last_version: String,
    pub patch_notes: bool,
    pub support_page: bool,
    pub update_check: bool,
    /// The one version that has been skipped, if any. Held here as well as in
    /// the file so the pages that must show it can, and so the offer can be
    /// suppressed without reading the disk on every check.
    pub skip_version: String,
    /// A newer release, once one has been found.
    pub update_found: Option<crate::updates::Found>,
    /// The last thing the updater had to say, for the page that asked. Never
    /// raised on its own: failing to reach a release host is not news.
    pub update_message: Option<String>,
    /// Whether the updater is doing something at this moment.
    pub updating: bool,
    /// An installer that has been fetched and checked, waiting to be run.
    pub update_ready: Option<crate::updates::Installed>,
    /// Whoever is signed in.
    ///
    /// Taken out of here while a worker is using it and put back when it comes
    /// home. There is deliberately no way to copy one: the server spends a
    /// refresh token the moment it is used and treats a second use of the same
    /// one as theft, so two sessions renewing from one stored record would
    /// revoke the account they share.
    pub session: Option<crate::cloud::Session>,
    /// Who that is, kept beside the session so the page can still say who is
    /// signed in while a worker has it.
    pub account: Option<crate::cloud::Account>,
    /// The machine this session is bound to, shown so a planner can tell one
    /// of their machines from another.
    pub device: Option<String>,
    /// What the network is doing, if anything.
    pub working: Option<Working>,
    /// The last thing the collaborate machinery had to say, shown on the
    /// Options page and in the sync view.
    pub cloud_message: Option<String>,
    /// Where this plan lives on a server, once it has been put on one.
    pub link: Option<crate::cloud::link::Link>,
    /// The live socket, while there is one.
    pub live: Option<crate::cloud::live::Live>,
    /// Whether live editing has been asked for. Separate from the socket,
    /// because a socket that drops should not look like a choice being undone.
    pub live_wanted: bool,
    /// Who else has this plan open.
    pub peers: Vec<crate::cloud::live::Peer>,
    /// The row the others were last told this planner is on. Remembered so
    /// only a move is sent: the socket is polled several times a second, and
    /// saying "still on row 12" that often is a lot of frames to say nothing.
    told_row: Option<i64>,
    /// Where the others were last told this planner's pointer is. Remembered
    /// for the same reason the row is: the timer asks several times a second,
    /// and a pointer that has not moved anywhere new is not worth a frame.
    told_at: Option<crate::cloud::live::Pointer>,
    /// What the server said when it was last asked.
    pub checked: Option<Checked>,
    /// Who this plan is shared with, once somebody has asked. `None` means
    /// nobody has asked, which is not the same as "nobody": this is read on
    /// demand rather than kept up to date, because it changes when somebody
    /// else changes it and there is nothing here that would hear about that.
    pub sharing: Option<crate::cloud::collab::Sharing>,
    /// Which plan the list above was asked about. Kept so that opening a
    /// second plan does not leave the first one's members on screen, and so
    /// that a read which failed is not retried on every render.
    pub sharing_for: Option<String>,
    /// The address being typed into the invite box, and the role beside it.
    /// Kept here rather than in the component so that a failed invitation does
    /// not also lose what was typed.
    pub invite_email: String,
    pub invite_role: String,
    /// The last thing the sharing machinery had to say. Its own field rather
    /// than `cloud_message`, because a sync failure and a refused invitation
    /// are answers to different questions and one must not overwrite the
    /// other under somebody's eyes.
    pub sharing_message: Option<String>,
    /// The versions this plan can be put back to.
    pub versions: aop_core::versions::Versions,
    /// Which version the History and Sync view is showing the difference for.
    pub version_selected: Option<usize>,
    /// What the health check found, once it has been run.
    pub health: Vec<crate::cloud::health::Check>,
    /// The row currently being dragged, and where it would land.
    pub drag_row: Option<usize>,
    pub drop_target: Option<(usize, DropWhere)>,
    /// A confirmation shown on the current Backstage page.
    pub backstage_message: Option<String>,
    /// A plan the Import page has read out of a spreadsheet, waiting for the
    /// word. Nothing is imported until somebody says so, and unsaved work in
    /// the open plan gets its question asked first, so the built plan has to
    /// wait somewhere that outlives the dialog.
    pub pending_import: Option<(Project, PathBuf, String)>,
    pub zoom: Zoom,
    pub filter: TaskFilter,
    pub status: String,

    /// The start-up splash, dismissed on a timer or by any click.
    pub splash: bool,
    pub ribbon_collapsed: bool,
    pub show_timeline: bool,
    pub show_outline_number: bool,
    pub show_critical: bool,
    /// Words told to be left alone for this plan.
    pub ignored_words: std::collections::HashSet<String>,
    /// Whether somebody has been sent to the provider's account page.
    ///
    /// Set when Manage account opens the browser and cleared when this window
    /// gets the focus back, which is the round trip a change is made in. It is
    /// the only signal there is: the page belongs to the provider, nothing
    /// tells this application what happened on it, and the alternative is
    /// asking on a timer for something that happens a handful of times in an
    /// account's life.
    pub account_page_opened: bool,
    /// When the account details were last read back from the provider. The
    /// flag above catches the trip through Manage account; this catches
    /// everything else, including a second upload in a browser tab that was
    /// already open, without polling for a thing that changes twice a year.
    pub account_checked_at: Option<std::time::Instant>,
    /// Whether History and Sync is open beside the plan.
    ///
    /// A panel rather than a view, and for the same reason the spelling one
    /// is. Whether this copy is current, what came in and which version to go
    /// back to are all things somebody asks *about the plan in front of them*,
    /// and a view would take the plan off the screen to answer them.
    pub sync_open: bool,
    /// Whether the spelling panel is open beside the plan.
    ///
    /// A panel rather than a view: correcting a word means seeing the row it is
    /// in, and a full-screen list of mistakes takes away the thing being
    /// corrected.
    pub spelling_open: bool,
    /// How long an iteration runs, for the burn charts and velocity.
    ///
    /// Only used as a fallback: a plan that names its sprints has its
    /// iterations read from those instead, which is the case that matters,
    /// because guessing a cadence from a calendar slices straight through
    /// declared sprints. Nothing writes this yet and it is not persisted, so
    /// it is the default until a plan without named sprints needs otherwise.
    pub iteration_days: i64,
    /// Which palette to paint, or to follow the desktop.
    pub theme: crate::theme::ThemeChoice,
    /// Which key press runs which command.
    pub keys: crate::keymap::Keymap,
    pub show_slack: bool,
    pub show_baseline: bool,
    pub gantt_style: usize,
    /// The table's columns, in the order they are drawn.
    pub columns: Vec<ColumnSpec>,
    /// How much of the table is on show. Narrower than the columns need simply
    /// scrolls; it never squashes them.
    pub table_pane_width: f64,
    pub pane_focus: PaneFocus,

    pub recent: Vec<RecentEntry>,
    /// Quick Access Toolbar buttons, in order.
    pub qat: Vec<QatCommand>,

    // ---- options -------------------------------------------------------
    pub user_name: String,
    pub user_initials: String,
    pub default_view: ViewKind,
    pub date_format: usize,
    /// Mode given to newly inserted tasks.
    pub new_tasks_mode: TaskMode,
    pub default_folder: String,
    pub options_page: OptionsPage,
    clipboard: Vec<Task>,
    undo: Vec<Project>,
    redo: Vec<Project>,
}

impl AppState {
    pub fn new() -> Self {
        let start = default_start();

        let mut state = Self {
            project: Project::blank(start),
            report: Ok(empty_report(start)),
            file_path: None,
            dirty: false,
            selection: Vec::new(),
            selected_resource: None,
            cell_draft: String::new(),
            picker_edits: 0,
            fill_field: None,
            macro_depth: 0,
            reschedule_owed: false,
            pending: Vec::new(),
            replaying: false,
            group_by: None,
            text_styles: aop_core::textstyle::TextStyles::new(),
            painter: None,
            grid_rows: true,
            grid_columns: true,
            grid_status_date: true,
            round_bars: false,
            show_links: true,
            bar_text: true,
            leveling: aop_core::leveling::LevelingOptions::default(),
            editing: None,
            after_save: None,
            quit_requested: false,
            popup_at: (0.0, 0.0),
            editing_resource_field: None,
            account_page_opened: false,
            account_checked_at: None,
            sync_open: false,
            tab: RibbonTab::Task,
            view: ViewKind::GanttChart,
            backstage: Some(BackstagePage::Home),
            dialog: None,
            context_menu: None,
            bar_drag: None,
            draw_tool: None,
            selected_drawing: None,
            draw_drag: None,
            show_drawings: true,
            idp_issuer: String::new(),
            idp_client_id: String::new(),
            idp_account_url: String::new(),
            collaborate_server: String::new(),
            collaborate: false,
            greetings: Vec::new(),
            licence_acknowledged: String::new(),
            licence_acknowledged_at: String::new(),
            last_version: String::new(),
            patch_notes: true,
            support_page: true,
            update_check: true,
            skip_version: String::new(),
            update_found: None,
            update_message: None,
            updating: false,
            update_ready: None,
            session: None,
            account: None,
            device: None,
            working: None,
            cloud_message: None,
            link: None,
            live: None,
            live_wanted: false,
            peers: Vec::new(),
            told_row: None,
            told_at: None,
            checked: None,
            sharing: None,
            sharing_for: None,
            invite_email: String::new(),
            invite_role: "editor".into(),
            sharing_message: None,
            versions: aop_core::versions::Versions::new(),
            version_selected: None,
            health: Vec::new(),
            drag_row: None,
            drop_target: None,
            backstage_message: None,
            pending_import: None,
            zoom: Zoom::Days,
            filter: TaskFilter::All,
            status: "Ready".into(),
            splash: true,
            ribbon_collapsed: false,
            show_timeline: true,
            show_outline_number: false,
            show_critical: false,
            ignored_words: std::collections::HashSet::new(),
            spelling_open: false,
            iteration_days: aop_core::agile::DEFAULT_ITERATION_DAYS,
            theme: crate::theme::ThemeChoice::default(),
            keys: crate::keymap::Keymap::default(),
            show_slack: false,
            show_baseline: false,
            gantt_style: 0,
            columns: default_columns(),
            table_pane_width: DEFAULT_COLUMNS_WIDTH,
            pane_focus: PaneFocus::Both,
            recent: load_recent(),
            qat: load_qat(),
            user_name: String::new(),
            user_initials: String::new(),
            default_view: ViewKind::GanttChart,
            date_format: 0,
            new_tasks_mode: TaskMode::Auto,
            default_folder: documents_dir().display().to_string(),
            options_page: OptionsPage::General,
            clipboard: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        };
        state.reschedule();
        state
    }

    // ---- scheduling -----------------------------------------------------

    /// Recalculate the plan. Called after every mutation.
    pub fn reschedule(&mut self) {
        // Deferred until the macro finishes, then run once. Scheduling after
        // every command would be the same answer computed thousands of times.
        if self.macro_depth > 0 {
            self.reschedule_owed = true;
            return;
        }
        self.report = schedule(&mut self.project).map_err(|e| e.to_string());
        if let Err(message) = &self.report {
            self.status = message.clone();
        }
    }

    pub fn schedule_error(&self) -> Option<String> {
        self.report.as_ref().err().cloned()
    }

    /// Work out how to repair a plan that will not schedule.
    pub fn remedy(&self) -> Option<aop_core::Remedy> {
        if self.report.is_ok() {
            return None;
        }
        aop_core::diagnose(&self.project)
    }

    /// Apply a repair the user has agreed to.
    pub fn apply_remedy(&mut self, remedy: &aop_core::Remedy) {
        self.checkpoint();
        let removed = aop_core::apply_remedy(&mut self.project, remedy);
        self.reschedule();
        self.dialog = None;
        self.status = match self.report {
            Ok(_) => format!("Fixed: {removed} link(s) removed, the plan schedules again"),
            Err(_) => format!("{removed} link(s) removed, but the plan still will not schedule"),
        };
    }

    // ---- undo -----------------------------------------------------------

    /// Run something as a single undoable step.
    ///
    /// Mirrors what Microsoft Project calls an undo transaction, and like that
    /// one it does not nest: an inner call adds to the step already open
    /// rather than starting another.
    pub fn as_one_step<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        self.begin_macro();
        let out = work(self);
        self.end_macro();
        out
    }

    fn begin_macro(&mut self) {
        if self.macro_depth == 0 {
            self.checkpoint();
        }
        self.macro_depth += 1;
    }

    fn end_macro(&mut self) {
        self.macro_depth = self.macro_depth.saturating_sub(1);
        if self.macro_depth > 0 {
            return;
        }
        // The step has closed, so what it did becomes one entry in the log.
        self.write_pending();
        if std::mem::take(&mut self.reschedule_owed) {
            self.reschedule();
        }
    }

    /// Snapshot the plan before changing it. Every mutating command calls this.
    pub fn checkpoint(&mut self) {
        // Inside a macro the step is already open, so this would only add an
        // identical snapshot and evict a real one.
        if self.macro_depth > 0 {
            self.dirty = true;
            return;
        }
        self.undo.push(self.project.clone());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Step back one change, and say in the log that the step was taken back.
    ///
    /// An undo is written down rather than quietly deleting the entry it puts
    /// back. Dropping that entry would leave a log saying the work never
    /// happened, and a trail that hides removals is not a trail. What the
    /// entry does not do is describe itself: it names the step being taken
    /// back, so the panel reads as what became of the plan rather than as a
    /// list of keys pressed.
    pub fn undo(&mut self) {
        if !self.can_undo() {
            return;
        }
        let taken_back = match self.project.history.recent(1).next() {
            Some(change) if !is_undo_entry(change) => format!("Undid: {}", change.summary),
            // An undo of an undo moves to an older step, so naming the step the
            // last entry named would be a lie about what has just happened.
            _ => "Undid the last change".to_string(),
        };
        self.record_as(Cmd::Undo {}, taken_back);
        self.roll_back();
        self.status = "Undo".into();
    }

    pub fn redo(&mut self) {
        let Some(next) = self.redo.pop() else {
            return;
        };
        let put_back = match self
            .project
            .history
            .recent(1)
            .next()
            .and_then(|change| change.summary.strip_prefix("Undid: "))
        {
            // The undo said which step it took back, so the redo can say which
            // step it is putting on again.
            Some(work) => format!("Redid: {work}"),
            None => "Redid the last change".to_string(),
        };
        self.record_as(Cmd::Redo {}, put_back);
        let log = std::mem::take(&mut self.project.history);
        self.undo.push(std::mem::replace(&mut self.project, next));
        self.project.history = log;
        self.dirty = true;
        self.clamp_selection();
        self.reschedule();
        self.status = "Redo".into();
    }

    /// Put the plan back to the last checkpoint without saying anything about
    /// it.
    ///
    /// The rollback a command does when the scheduler refuses what it has just
    /// been asked for. Nobody asked for an undo, so this is neither written in
    /// the log nor announced in the status bar; the command that called it
    /// says what happened instead.
    fn roll_back(&mut self) {
        let Some(previous) = self.undo.pop() else {
            return;
        };
        // The log rides inside the plan so that it travels with the file, but
        // it is not part of what an undo puts back. Rolling it back with
        // everything else would erase the record of the work being taken out,
        // which is the one record an audit trail exists to keep.
        let log = std::mem::take(&mut self.project.history);
        self.redo.push(std::mem::replace(&mut self.project, previous));
        self.project.history = log;
        self.dirty = true;
        self.clamp_selection();
        self.reschedule();
    }

    // ---- the change log -------------------------------------------------

    /// Take note of a command, for the change log and for the macro recorder
    /// when it is running. One hook, two consumers.
    ///
    /// Called at the top of the method that carries the command out, while the
    /// plan still looks as it did when the command was given: how many rows
    /// were selected, who was already booked, whether a task was active. Half
    /// the sentences in the panel are read off the plan, and after the edit
    /// they would describe the answer rather than the question.
    fn record(&mut self, cmd: Cmd) {
        // Checked here as well as in `record_as` so a replay does not pay for
        // a sentence nobody will read.
        if self.replaying {
            return;
        }
        let summary = describe(&cmd, self);
        self.record_as(cmd, summary);
    }

    /// Take note of a command under a sentence the caller has already worked
    /// out, which an undo needs because only it knows what it took back.
    fn record_as(&mut self, cmd: Cmd, summary: String) {
        if self.replaying {
            return;
        }
        // Selecting row 3, then 4, then 5 is one act of selecting, so the last
        // of a run replaces the ones before it rather than each arrow key
        // earning its own entry.
        if is_selecting(&cmd)
            && self
                .pending
                .last()
                .is_some_and(|(held, _)| is_selecting(held))
        {
            self.pending.pop();
        }
        self.pending.push((cmd, summary));

        // Inside a grouped step the whole run becomes one entry, written when
        // the step closes: a fill down over twenty rows is one thing the
        // planner did, not twenty.
        if self.macro_depth > 0 {
            return;
        }
        // A selection on its own is held back. Nearly every command acts on
        // the selection, so the rows are the context that makes the entry
        // replayable, and they belong in the entry for the edit that follows.
        if self.pending.iter().all(|(held, _)| is_selecting(held)) {
            return;
        }
        self.write_pending();
    }

    /// Write everything held back as one entry.
    fn write_pending(&mut self) {
        let held = std::mem::take(&mut self.pending);
        if held.is_empty() {
            return;
        }
        let script = held
            .iter()
            .map(|(cmd, _)| cmd.to_script())
            .collect::<Vec<String>>()
            .join("\n");
        let summary = describe_run(&held);
        self.write_change(script, summary);
    }

    /// Put one entry in the plan's log, under whoever is at the keyboard.
    pub(crate) fn write_change(&mut self, script: String, summary: String) {
        // A blank name is what a fresh install has. Saying so is better than
        // signing somebody else's work with a guess.
        let author = match self.display_name().as_str() {
            "" => "Unknown".to_string(),
            name => name.to_string(),
        };
        self.project
            .history
            .record(author, script, summary, Local::now().naive_local());
    }

    /// Carry something out without recording the commands inside it.
    ///
    /// Two callers want this. A method built out of other recorded methods, so
    /// that a cut reads as one act rather than as a copy and then a delete;
    /// and a macro replay, where every command was recorded when it was first
    /// done and the run writes a single entry of its own.
    pub(crate) fn unrecorded<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        // Whatever is held belongs to what the planner did by hand, so it is
        // written first rather than being swept into the run.
        self.write_pending();
        let was = std::mem::replace(&mut self.replaying, true);
        let out = work(self);
        self.replaying = was;
        out
    }

    // ---- selection ------------------------------------------------------

    pub fn select(&mut self, index: usize) {
        self.record(Cmd::SelectRow {
            row: Row(index as u32 + 1),
        });
        self.selected_drawing = None;
        // Selecting a different row abandons any edit in progress; staying on
        // the same row does not, so clicking about inside a cell is safe.
        if self.editing.map(|(row, _)| row) != Some(index) {
            self.editing = None;
        }
        self.selection = vec![index];
    }

    pub fn extend_selection(&mut self, index: usize) {
        if let Some(&anchor) = self.selection.first() {
            self.record(Cmd::SelectRows {
                from: Row(anchor as u32 + 1),
                to: Row(index as u32 + 1),
            });
            let (lo, hi) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            let mut range: Vec<usize> = (lo..=hi).collect();
            if anchor > index {
                range.reverse();
            }
            self.selection = range;
        } else {
            self.select(index);
        }
    }

    pub fn toggle_selection(&mut self, index: usize) {
        self.record(Cmd::ToggleRow {
            row: Row(index as u32 + 1),
        });
        if let Some(position) = self.selection.iter().position(|&i| i == index) {
            self.selection.remove(position);
        } else {
            self.selection.push(index);
        }
    }

    pub fn primary(&self) -> Option<usize> {
        self.selection.first().copied()
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selection.contains(&index)
    }

    fn clamp_selection(&mut self) {
        let limit = self.project.tasks.len();
        self.selection.retain(|&i| i < limit);
        self.editing = None;
    }

    /// Selected rows in ascending order, which is what structural edits want.
    fn ordered_selection(&self) -> Vec<usize> {
        let mut rows = self.selection.clone();
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    // ---- file -----------------------------------------------------------

    pub fn document_title(&self) -> String {
        let name = self
            .file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.project.name.clone());
        format!("{name}{}", if self.dirty { "*" } else { "" })
    }

    pub fn new_from_template(&mut self, template_id: &str) {
        let start = default_start();
        self.project = match templates::by_id(template_id) {
            Some(spec) => templates::build(spec, start),
            None => Project::blank(start),
        };
        self.file_path = None;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.pending.clear();
        self.selection = if self.project.tasks.is_empty() {
            Vec::new()
        } else {
            vec![0]
        };
        self.backstage = None;
        self.view = ViewKind::GanttChart;
        self.tab = RibbonTab::Task;
        self.zoom = if self.project.tasks.len() > 20 {
            Zoom::Weeks
        } else {
            Zoom::Days
        };
        self.plan_changed();
        self.reschedule();
        self.status = format!("Created {}", self.project.name);
    }

    /// Everything that belongs to the plan rather than to the application,
    /// pointed at whichever plan is now on screen.
    ///
    /// One place for it, because a link left over from the last plan would
    /// have this one syncing into somebody else's project, and versions left
    /// over would offer the wrong plan back.
    fn plan_changed(&mut self) {
        self.restore_link();
        self.versions = crate::versions::read(self.file_path.as_deref());
        self.version_selected = None;
        self.checked = None;
        self.cloud_message = None;
        self.stop_live(None);
    }

    pub fn open_path(&mut self, path: PathBuf) {
        match persist::open(&path) {
            Ok(project) => {
                self.project = project;
                self.dirty = false;
                self.undo.clear();
                self.redo.clear();
                self.pending.clear();
                self.selection = if self.project.tasks.is_empty() {
                    Vec::new()
                } else {
                    vec![0]
                };
                self.status = format!("Opened {}", path.display());
                self.push_recent(&path);
                self.file_path = Some(path);
                self.backstage = None;
                self.plan_changed();
                self.reschedule();
            }
            Err(error) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not open file".into(),
                    body: error.to_string(),
                });
                self.backstage = None;
            }
        }
    }

    /// Read a plan out of a workbook.
    pub fn import_excel(&mut self, path: PathBuf) {
        match aop_core::excel::open(&path) {
            Ok(project) => {
                self.project = project;
                // A workbook is not a plan file, so it has nowhere to save back
                // to: Save As has to be chosen deliberately.
                self.file_path = None;
                self.dirty = true;
                self.undo.clear();
                self.redo.clear();
                self.pending.clear();
                self.selection = if self.project.tasks.is_empty() {
                    Vec::new()
                } else {
                    vec![0]
                };
                self.push_recent(&path);
                self.backstage = None;
                self.reschedule();
                let tasks = self.project.tasks.len();
                self.status = format!(
                    "Imported {tasks} tasks from {}. Save As to keep it as a .{} file.",
                    path.display(),
                    persist::FILE_EXTENSION
                );
            }
            Err(error) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not import".into(),
                    body: error.to_string(),
                })
            }
        }
    }

    /// Hold a plan the Import page has built, and ask about unsaved work.
    ///
    /// Nothing is imported here. The plan waits in the slot until the question
    /// about the open plan has been answered, and if the answer is no it is
    /// dropped and the open plan never knew about it.
    pub fn stage_import(&mut self, project: Project, source: PathBuf, note: String) {
        self.pending_import = Some((project, source, note));
        self.guard(PendingAction::AdoptImport);
    }

    /// Take the plan the Import page built. Called once the way is clear.
    fn adopt_import(&mut self) {
        let Some((project, source, note)) = self.pending_import.take() else {
            return;
        };
        self.project = project;
        // A spreadsheet is not a plan file, so there is nowhere to save back
        // to: Save As has to be chosen deliberately.
        self.file_path = None;
        self.dirty = true;
        self.undo.clear();
        self.redo.clear();
        self.pending.clear();
        self.selection = if self.project.tasks.is_empty() {
            Vec::new()
        } else {
            vec![0]
        };
        self.push_recent(&source);
        self.backstage = None;
        self.plan_changed();
        self.reschedule();
        self.status = note;
    }

    /// Book days off into whichever calendar the import was aimed at.
    ///
    /// A checkpoint first, because this moves dates: a planner who imports the
    /// wrong country's holidays, or drops a national holiday file onto one
    /// person, has to be able to undo it in one step.
    pub fn import_holidays(
        &mut self,
        target: &CalendarTarget,
        holidays: &[aop_core::holidays::Holiday],
    ) -> usize {
        let where_to = self.project.calendar_target_name(target);
        self.checkpoint();

        let Some(existing) = self.project.exceptions_for_mut(target) else {
            // The picker is naming something the plan no longer has. Writing
            // the days onto the project calendar instead would be the one
            // mistake this whole control is arranged to prevent.
            self.undo.pop();
            self.note("That calendar is no longer in the plan.");
            return 0;
        };

        // `holidays::add` works on a calendar, and a person in this model has
        // an exception list rather than a calendar of their own: their working
        // week is their base's and is shared. Lending them one carrying their
        // own exceptions keeps a single answer to "is this day already booked
        // off" rather than growing a second one here that could disagree with
        // the one doing the adding.
        let mut carrier = aop_core::WorkCalendar::standard();
        carrier.exceptions = std::mem::take(existing);
        let added = aop_core::holidays::add(&mut carrier, holidays);
        *existing = std::mem::take(&mut carrier.exceptions);

        if added == 0 {
            // Nothing changed, so the checkpoint would be an empty step in the
            // undo stack, which is worse than no step at all.
            self.undo.pop();
            self.note(format!("Every one of those days is already on {where_to}."));
            return 0;
        }
        self.reschedule();
        self.plan_changed();
        self.note(format!("Added {added} day(s) off to {where_to}"));
        added
    }

    /// Import a Microsoft Project XML (MSPDI) plan.
    pub fn import_path(&mut self, path: PathBuf) {
        match aop_core::mspdi::open(&path) {
            Ok(project) => {
                self.project = project;
                // An import is a new document: it has no .aprj file behind it.
                self.file_path = None;
                self.dirty = true;
                self.undo.clear();
                self.redo.clear();
                self.pending.clear();
                self.selection = if self.project.tasks.is_empty() {
                    Vec::new()
                } else {
                    vec![0]
                };
                // An import is a file the user opened, so it belongs in Recent
                // like any other. It has no .aprj behind it yet, which is why
                // `file_path` stays empty while the recent entry does not.
                self.push_recent(&path);
                self.backstage = None;
                self.reschedule();
                let tasks = self.project.tasks.len();
                self.status = format!(
                    "Imported {tasks} tasks from {}. Save As to keep it as a .{} file.",
                    path.display(),
                    persist::FILE_EXTENSION
                );
            }
            Err(error) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not import".into(),
                    body: error.to_string(),
                });
            }
        }
    }

    /// Take the remembered preferences on, at start up.
    fn apply_settings(&mut self, mut settings: crate::settings::Settings) {
        // Done here rather than anywhere later, because this is the one moment
        // the running version is news: a skip that this copy has already gone
        // past means somebody updated by another route, and the record is
        // spent. Clearing it here also gets it written back, since the copy
        // start up compares against was read from the file before this ran.
        settings.forget_a_spent_skip(crate::welcome::RUNNING);
        self.user_name = settings.user_name.clone();
        self.user_initials = settings.user_initials.clone();
        if !settings.company.is_empty() {
            self.project.company = settings.company.clone();
        }
        if !settings.user_name.is_empty() {
            self.project.author = settings.user_name.clone();
        }
        self.theme = settings.theme;
        self.date_format = settings.date_format;
        set_date_format(settings.date_format);
        self.show_timeline = settings.show_timeline;
        self.show_outline_number = settings.show_outline_number;
        self.show_critical = settings.show_critical;
        self.grid_rows = settings.grid_rows;
        self.grid_columns = settings.grid_columns;
        self.grid_status_date = settings.grid_status_date;
        self.round_bars = settings.round_bars;
        self.show_links = settings.show_links;
        self.bar_text = settings.bar_text;
        self.show_drawings = settings.show_drawings;
        self.idp_issuer = settings.idp_issuer.clone();
        self.idp_client_id = settings.idp_client_id.clone();
        self.idp_account_url = settings.idp_account_url.clone();
        self.collaborate_server = settings.collaborate_server.clone();
        self.collaborate = settings.collaborate;
        self.licence_acknowledged = settings.licence_acknowledged.clone();
        self.licence_acknowledged_at = settings.licence_acknowledged_at.clone();
        self.last_version = settings.last_version.clone();
        self.patch_notes = settings.patch_notes;
        self.support_page = settings.support_page;
        self.update_check = settings.update_check;
        self.skip_version = settings.skip_version.clone();
        self.keys = settings.keys;
    }

    /// What to remember for next time.
    pub fn settings(&self) -> crate::settings::Settings {
        crate::settings::Settings {
            user_name: self.user_name.clone(),
            user_initials: self.user_initials.clone(),
            company: self.project.company.clone(),
            theme: self.theme,
            date_format: self.date_format,
            show_timeline: self.show_timeline,
            show_outline_number: self.show_outline_number,
            show_critical: self.show_critical,
            grid_rows: self.grid_rows,
            grid_columns: self.grid_columns,
            grid_status_date: self.grid_status_date,
            round_bars: self.round_bars,
            show_links: self.show_links,
            bar_text: self.bar_text,
            show_drawings: self.show_drawings,
            idp_issuer: self.idp_issuer.clone(),
            idp_client_id: self.idp_client_id.clone(),
            idp_account_url: self.idp_account_url.clone(),
            collaborate_server: self.collaborate_server.clone(),
            collaborate: self.collaborate,
            licence_acknowledged: self.licence_acknowledged.clone(),
            licence_acknowledged_at: self.licence_acknowledged_at.clone(),
            last_version: self.last_version.clone(),
            patch_notes: self.patch_notes,
            support_page: self.support_page,
            update_check: self.update_check,
            skip_version: self.skip_version.clone(),
            keys: self.keys.clone(),
        }
    }

    // ---- the licence, what changed, and the ask -------------------------

    /// Work out which start up pages this launch owes, and record the version
    /// it is running as.
    ///
    /// The version is written down here rather than when the last page is
    /// dismissed, which is what makes it once per update rather than once per
    /// start: somebody who closes the window part way through the notes has
    /// still been shown them, and reopening does not begin again.
    pub fn begin_greetings(&mut self) {
        let settings = self.settings();
        self.greetings = crate::welcome::on_start(&settings, crate::welcome::RUNNING);
        self.last_version = crate::welcome::RUNNING.to_string();
    }

    /// The licence has been read. Recorded with the version it was shown for
    /// and the moment, since a record with only one of those answers half the
    /// question.
    pub fn acknowledge_licence(&mut self) {
        self.licence_acknowledged = crate::welcome::RUNNING.to_string();
        self.licence_acknowledged_at = chrono::Utc::now().to_rfc3339();
        self.greeting_answered();
    }

    /// Done with whichever page was in front.
    pub fn greeting_answered(&mut self) {
        if !self.greetings.is_empty() {
            self.greetings.remove(0);
        }
    }

    /// Open the support page because somebody went looking for it, rather than
    /// because the version moved. Nothing showed it to them, so there is
    /// nothing to silence and no checkbox is offered.
    pub fn show_support(&mut self) {
        self.backstage = None;
        self.greetings.insert(
            0,
            crate::welcome::Greeting::Support {
                after_update: false,
            },
        );
    }

    // ---- updates --------------------------------------------------------

    /// What a check came back with.
    ///
    /// Not reaching a release host is kept for whichever page asked and is
    /// never raised on its own. Somebody typing into a plan does not need to
    /// hear that a server they were not thinking about did not answer.
    pub fn update_landed(&mut self, outcome: Result<Option<crate::updates::Found>, String>) {
        self.updating = false;
        match outcome {
            // The skipped release, found again. Nothing is offered and nothing
            // is said in the status bar, since being told about it is the
            // thing that was refused. The page that asked is told plainly,
            // though, and told where the refusal can be withdrawn: a skip
            // nobody can find again is a trap rather than a preference.
            Ok(Some(found)) if crate::settings::is_skipped(&self.skip_version, &found.version) => {
                self.update_found = None;
                self.update_message = Some(format!(
                    "Version {} is available. You chose to skip it, so it is not offered. \
                     Options, under General, offers it again.",
                    found.version
                ));
            }
            Ok(Some(found)) => {
                self.note(format!("Version {} is available", found.version));
                self.update_message = None;
                self.update_found = Some(found);
            }
            Ok(None) => {
                self.update_found = None;
                self.update_message = Some(format!(
                    "Version {} is the newest there is.",
                    crate::welcome::RUNNING
                ));
            }
            Err(why) => {
                self.update_found = None;
                self.update_message = Some(why);
            }
        }
    }

    /// Why an update cannot be installed at this moment, if it cannot.
    ///
    /// Unsaved work comes first: installing replaces the running program, and
    /// a plan that only exists in this window would go with it.
    pub fn update_blocked(&self) -> Option<String> {
        if self.dirty {
            return Some(
                "This plan has unsaved changes. Save it first, because installing an update                  replaces the running program."
                    .into(),
            );
        }
        if self.updating {
            return Some("An update is already being fetched.".into());
        }
        self.update_found.as_ref().and_then(|found| found.why_not())
    }

    /// Never offer this particular release again.
    ///
    /// The version is written down rather than a flag being set, so the answer
    /// stays about this release alone. The next one is a different version and
    /// is offered as though nothing had been skipped.
    pub fn skip_the_found_version(&mut self) {
        let Some(found) = self.update_found.take() else {
            return;
        };
        self.note(format!(
            "Version {} will not be offered again. Later versions still will.",
            found.version
        ));
        self.skip_version = found.version;
        // The offer is gone, so anything the updater had to say about it is
        // stale. Left behind it would sit under the next check's answer.
        self.update_message = None;
    }

    /// Withdraw a skip, so the version it named is offered again.
    ///
    /// The record is the only thing suppressing the offer, so clearing it is
    /// the whole undo. The version comes back at the next check rather than
    /// this instant, since finding it means asking a server and nothing here
    /// waits on one.
    pub fn offer_the_skipped_version_again(&mut self) {
        if self.skip_version.is_empty() {
            return;
        }
        self.note(format!(
            "Version {} will be offered again at the next check.",
            self.skip_version
        ));
        self.skip_version.clear();
    }

    /// What an install came back with.
    pub fn install_landed(&mut self, outcome: Result<crate::updates::Installed, String>) {
        self.updating = false;
        match outcome {
            Ok(installed) => {
                self.update_message = Some(match &installed {
                    crate::updates::Installed::Replaced { kept } => format!(
                        "The new version is in place and starts the next time you open this                          application. The previous one has been kept at {}.",
                        kept.display()
                    ),
                    crate::updates::Installed::Downloaded { .. } => {
                        "The installer has been downloaded and checked against its published \
                         checksum. Installing closes this application, which is what frees its \
                         files to be replaced, and starts the new version when it is done."
                            .into()
                    }
                });
                self.update_ready = Some(installed);
                self.update_found = None;
            }
            Err(why) => self.update_message = Some(why),
        }
    }


    // ---- Alterion Collaborate -------------------------------------------

    /// Pick up the last run's sign in.
    ///
    /// Reads the store and no more, so start up is never held up by a server
    /// that is slow or absent. A session that has since been ended shows up on
    /// the first thing that uses it, which is the right moment to hear about
    /// it rather than during a splash screen.
    pub fn restore_session(&mut self) {
        let Some(session) = crate::cloud::restore() else {
            return;
        };
        self.account = Some(session.account().clone());
        self.device = describe_device();
        self.session = Some(session);
    }

    /// The name other people see against this planner's work.
    ///
    /// The account's name while there is one, and what was typed in Options
    /// otherwise. The shared log is written under whoever the server says this
    /// is, and a local name that disagrees with it is a name only this machine
    /// ever sees, which is how somebody ends up wondering who made a change
    /// they made themselves.
    ///
    /// The typed name is read, never overwritten. That is what makes signing
    /// out put it straight back rather than leaving somebody's colleagues'
    /// idea of them behind on their own machine.
    pub fn display_name(&self) -> String {
        match self.account.as_ref() {
            Some(account) if !account.name.trim().is_empty() => account.name.trim().to_string(),
            _ => self.user_name.trim().to_string(),
        }
    }

    /// Where Manage account opens, if there is anywhere to open.
    ///
    /// Off the live session's issuer first: the page belongs to the provider
    /// this copy actually signed in against, which is not necessarily the
    /// address in the box today. The settings key overrides both, for a
    /// deployment that keeps its account page somewhere else.
    pub fn account_page_url(&self) -> Option<String> {
        let issuer = match self.session.as_ref() {
            Some(session) => session.issuer().to_string(),
            None => self.idp_issuer.clone(),
        };
        crate::cloud::account_page(&issuer, &self.idp_account_url)
    }

    /// Which provider this sign in came from, and how long the current pass
    /// has left, for showing on the Options page.
    ///
    /// Both read off the live session rather than off the settings: what is in
    /// the boxes is what the next sign in will use, which is not necessarily
    /// what this one did.
    pub fn session_summary(&self) -> Option<(String, String)> {
        let session = self.session.as_ref()?;
        Some((
            session.issuer().to_string(),
            session
                .expires_at()
                .naive_local()
                .format("%Y-%m-%d %H:%M")
                .to_string(),
        ))
    }

    /// Forget which plan on the server this one is.
    ///
    /// Nothing is removed from either side. It is the answer to a plan that
    /// turns out to be linked to the wrong project, which is what a cursor
    /// past the server's head means, and to a copy that should stop syncing.
    pub fn unlink_plan(&mut self) {
        if let Some(path) = self.file_path.clone() {
            crate::cloud::link::forget(&path);
        }
        self.link = None;
        self.checked = None;
        self.sharing = None;
        self.sharing_for = None;
        self.stop_live(None);
        self.cloud_message = Some(
            "This plan is no longer linked to a server. Nothing was removed from either side, \
             and it can be put on a server again."
                .into(),
        );
        self.status = "Unlinked from the server".into();
    }

    /// Read this plan's link to a server, if it has one.
    ///
    /// Keyed by where the plan lives on this machine, so a copy of a plan is a
    /// separate client of the same project rather than a second copy of one
    /// cursor.
    pub fn restore_link(&mut self) {
        self.link = self
            .file_path
            .as_deref()
            .and_then(crate::cloud::link::load);
        // Whoever the last plan was shared with has nothing to do with this
        // one. Dropped rather than left to be replaced, so there is no render
        // in which one plan's members are shown under another plan's name.
        self.sharing = None;
        self.sharing_for = None;
    }

    /// Take the session for a worker, and say what it is doing.
    ///
    /// The session is moved rather than shared. Two of them renewing from one
    /// stored record would each spend the other's refresh token, and the
    /// server treats a second use of a spent one as theft against the whole
    /// account.
    pub fn hand_over(&mut self, working: Working) -> Option<crate::cloud::Session> {
        if self.working.is_some() {
            return None;
        }
        let session = self.session.take()?;
        self.working = Some(working);
        self.cloud_message = None;
        Some(session)
    }

    /// Take the session back when the work is done.
    pub fn hand_back(&mut self, session: crate::cloud::Session) {
        self.account = Some(session.account().clone());
        self.session = Some(session);
        self.working = None;
    }

    /// A worker that never came back.
    ///
    /// The session went with it, so what is left is whatever the store holds.
    /// Reading that back is what stops a worker falling over from looking like
    /// somebody being signed out.
    pub fn worker_lost(&mut self, what: &str) {
        self.working = None;
        self.cloud_message = Some(format!("{what} stopped unexpectedly. Try it again."));
        if self.session.is_none() {
            self.restore_session();
        }
    }

    /// What is missing before a sign in can be started, if anything.
    ///
    /// Named rather than counted, so a disabled button can say which field is
    /// still needed instead of leaving somebody to guess.
    pub fn sign_in_blocked(&self) -> Option<&'static str> {
        if self.idp_issuer.trim().is_empty() {
            return Some("Fill in the identity provider address first.");
        }
        if self.idp_client_id.trim().is_empty() {
            return Some("Fill in the client ID first.");
        }
        if self.working.is_some() {
            return Some("Something else is talking to the server. Wait for it to finish.");
        }
        None
    }

    /// Hand over what a sign in needs, and mark this copy as waiting.
    pub fn start_sign_in(&mut self) -> Option<(String, String)> {
        if self.sign_in_blocked().is_some() {
            return None;
        }
        self.working = Some(Working::SigningIn);
        self.cloud_message = None;
        Some((
            self.idp_issuer.trim().to_string(),
            self.idp_client_id.trim().to_string(),
        ))
    }

    pub fn sign_in_landed(
        &mut self,
        outcome: Result<crate::cloud::Session, crate::cloud::SignInError>,
    ) {
        self.working = None;
        match outcome {
            Ok(session) => {
                let account = session.account().clone();
                let who = if account.email.is_empty() {
                    account.name.clone()
                } else {
                    account.email.clone()
                };
                self.device = describe_device();
                self.account = Some(account);
                self.session = Some(session);
                self.cloud_message = Some(format!("Signed in as {who}."));
                self.status = format!("Signed in as {who}");
            }
            // Every one of these already carries text a person can act on, so
            // it is shown as it is rather than replaced by something general.
            Err(error) => self.cloud_message = Some(error.to_string()),
        }
    }

    pub fn sign_out_landed(&mut self, outcome: Result<(), String>) {
        self.working = None;
        self.session = None;
        self.account = None;
        self.device = None;
        self.stop_live(None);
        self.cloud_message = Some(match outcome {
            Ok(()) => "Signed out. Nothing has been removed from this machine.".to_string(),
            // Signed out here either way, because a person who pressed Sign
            // out is signed out. What the message says is what did not happen.
            Err(why) => why,
        });
        self.status = "Signed out".into();
    }

    /// Why the sync commands cannot be used, if they cannot.
    ///
    /// A reason rather than a bare disabled button. A greyed control with
    /// nothing to read gets pressed again; one that says what is missing says
    /// what to do next.
    pub fn sync_blocked(&self) -> Option<String> {
        if !self.collaborate {
            return Some(
                "Alterion Collaborate is turned off. Turn it on in Options to sync this plan."
                    .into(),
            );
        }
        if self.collaborate_server.trim().is_empty() {
            return Some(
                "No Collaborate server address is set. Fill it in in Options, under Alterion \
                 Collaborate."
                    .into(),
            );
        }
        if self.account.is_none() {
            return Some(
                "Not signed in. Sign in from Options, under Alterion Collaborate.".into(),
            );
        }
        if self.link.is_none() {
            return Some(
                "This plan is on this machine only. Put it on the server from Options, under \
                 Alterion Collaborate."
                    .into(),
            );
        }
        if self.working.is_some() {
            return Some("Something else is talking to the server. Wait for it to finish.".into());
        }
        None
    }

    /// Why this plan cannot be put on a server, if it cannot.
    pub fn publish_blocked(&self) -> Option<String> {
        if !self.collaborate {
            return Some("Alterion Collaborate is turned off. Turn it on in Options.".into());
        }
        if self.collaborate_server.trim().is_empty() {
            return Some("No Collaborate server address is set. Fill it in in Options.".into());
        }
        if self.account.is_none() {
            return Some("Not signed in. Sign in first.".into());
        }
        if self.link.is_some() {
            return Some("This plan is already on the server.".into());
        }
        // The link is remembered against the file, so a plan that has never
        // been saved has nothing to remember it against.
        if self.file_path.is_none() {
            return Some("Save this plan to a file first, so the link to the server can be kept.".into());
        }
        if self.working.is_some() {
            return Some("Something else is talking to the server. Wait for it to finish.".into());
        }
        None
    }

    /// Gather everything a sync needs, and hand the session over.
    pub fn start_sync(&mut self) -> Option<(crate::cloud::Session, crate::cloud::work::Offer)> {
        if self.sync_blocked().is_some() {
            return None;
        }
        let link = self.link.clone()?;
        let offer = crate::cloud::work::Offer {
            server: self.collaborate_server.trim().to_string(),
            project: link.project,
            after: link.cursor,
            changes: self.project.history.unsent().to_vec(),
            plan: self.project.clone(),
        };
        let session = self.hand_over(Working::Syncing)?;
        Some((session, offer))
    }

    /// What the server said about a push.
    pub fn sync_landed(&mut self, outcome: Result<crate::cloud::collab::Pushed, String>) {
        use crate::cloud::collab::Pushed;

        self.working = None;
        let now = Local::now().naive_local();
        let pushed = match outcome {
            Ok(pushed) => pushed,
            Err(why) => {
                self.checked = Some(Checked {
                    at: now,
                    outcome: CheckOutcome::Failed(why.clone()),
                });
                self.cloud_message = Some(why);
                self.status = "Could not sync".into();
                return;
            }
        };

        match pushed {
            Pushed::Applied { head, applied, .. } => {
                // Marked by the local id the server acknowledged rather than
                // by counting, because an answer that came back out of order
                // must not mark work nobody has seen as sent.
                if let Some(highest) = applied.iter().map(|(local, _)| *local).max() {
                    self.project.history.mark_pushed(highest);
                }
                self.remember_cursor(head);
                self.checked = Some(Checked {
                    at: now,
                    outcome: CheckOutcome::Current,
                });
                let said = match applied.len() {
                    0 => "Already up to date. Nothing was waiting to go.".to_string(),
                    1 => "1 change sent.".to_string(),
                    many => format!("{many} changes sent."),
                };
                self.cloud_message = Some(said.clone());
                self.status = said;
            }

            Pushed::Behind {
                head,
                changes,
                more,
                ..
            } => {
                let (differences, replayed, asked) = self.preview_incoming(&changes);
                let sentence = aop_core::compare::summarise(&differences).sentence();
                self.checked = Some(Checked {
                    at: now,
                    outcome: CheckOutcome::Behind {
                        by: changes.len() as i64,
                    },
                });
                self.status = "Somebody else changed this plan first".into();
                self.cloud_message = Some(sentence.clone());
                // A real question, not a message: taking somebody else's work
                // is the one decision this whole design exists to offer, and
                // it is not one to make on a planner's behalf.
                self.dialog = Some(Dialog::SyncBehind {
                    head,
                    sentence,
                    differences,
                    changes,
                    replayed,
                    asked,
                    more,
                });
            }

            Pushed::Gap { head, oldest } => {
                self.checked = Some(Checked {
                    at: now,
                    outcome: CheckOutcome::Failed("the log this copy needs has been trimmed".into()),
                });
                self.status = "This copy is too far behind to catch up".into();
                let held = match oldest {
                    Some(oldest) => format!("the oldest change it still keeps is {oldest}"),
                    None => "it no longer keeps any of them".to_string(),
                };
                self.dialog = Some(Dialog::FreshCopy {
                    why: format!(
                        "The server's log has been trimmed past the point this copy had reached, \
                         so there is nothing left to replay onto: {held}, and the head is now \
                         {head}. Replaying what survives would look like a sync that worked and \
                         would have lost the rest, so this copy needs a fresh whole plan instead."
                    ),
                });
            }

            Pushed::Ahead { head, cursor } => {
                self.checked = Some(Checked {
                    at: now,
                    outcome: CheckOutcome::Failed("this copy is not on the same log".into()),
                });
                self.status = "This plan is not the one the server holds".into();
                self.dialog = Some(Dialog::SyncAhead { head, cursor });
            }
        }
    }

    // ---- Who a plan is shared with ---------------------------------------

    /// Why the sharing commands cannot be used, if they cannot.
    ///
    /// The same shape as [`Self::sync_blocked`] and for the same reason: a
    /// greyed control with nothing to read gets pressed again, and one that
    /// says what is missing says what to do next.
    pub fn sharing_blocked(&self) -> Option<String> {
        if !self.collaborate {
            return Some("Alterion Collaborate is turned off. Turn it on in Options.".into());
        }
        if self.collaborate_server.trim().is_empty() {
            return Some("No Collaborate server address is set. Fill it in above.".into());
        }
        if self.account.is_none() {
            return Some("Not signed in. Sign in first.".into());
        }
        if self.link.is_none() {
            return Some(
                "This plan is on this machine only, so there is nobody to share it with yet. \
                 Put it on the server first."
                    .into(),
            );
        }
        if self.working.is_some() {
            return Some("Something else is talking to the server. Wait for it to finish.".into());
        }
        None
    }

    /// Hand over what reading the sharing needs.
    ///
    /// Read on demand rather than kept fresh. Membership changes when somebody
    /// else changes it, and nothing here would hear about that, so a list that
    /// looked live would be a list that was quietly wrong.
    pub fn start_sharing(&mut self, working: Working) -> Option<(crate::cloud::Session, String, String)> {
        if self.sharing_blocked().is_some() {
            return None;
        }
        let project = self.link.as_ref()?.project.clone();
        let server = self.collaborate_server.trim().to_string();
        let session = self.hand_over(working)?;
        // Marked as asked before the answer, not after. A read that fails
        // otherwise leaves the page asking again on every render, which is a
        // request per frame at whichever server is already having a bad time.
        self.sharing_for = Some(project.clone());
        self.sharing_message = None;
        Some((session, server, project))
    }

    /// What the invite box currently amounts to, or why it does not.
    ///
    /// The address is checked here so that an obvious mistake is answered
    /// without a round trip. It is checked on the server too, which is the
    /// check that counts: this one is a courtesy and is written as one.
    pub fn invite_ready(&self) -> Result<(String, String), String> {
        let typed = self.invite_email.trim();
        if typed.is_empty() {
            return Err("Type the email address of whoever you want to invite.".into());
        }
        let (local, domain) = typed
            .split_once('@')
            .ok_or("That is not an email address. It needs an @ in it.")?;
        if local.is_empty() || domain.is_empty() || typed.contains(char::is_whitespace) {
            return Err("That is not an email address. One address, with no spaces.".into());
        }
        Ok((typed.to_string(), self.invite_role.clone()))
    }

    /// The list the server holds, or why it could not be had.
    pub fn sharing_landed(&mut self, outcome: Result<crate::cloud::collab::Sharing, String>) {
        self.working = None;
        match outcome {
            Ok(sharing) => {
                self.sharing = Some(sharing);
                self.sharing_message = None;
            }
            // The old list is left on screen. It is what the server last said,
            // which is more use than an empty panel, and the message beside it
            // says it could not be brought up to date.
            Err(why) => self.sharing_message = Some(why),
        }
    }

    /// The same, after a change that was asked for.
    ///
    /// The change and the read back are one job, so a list that arrives is a
    /// list the server produced after the change rather than one this copy
    /// assumed it would produce.
    pub fn sharing_changed(
        &mut self,
        said: String,
        outcome: Result<crate::cloud::collab::Sharing, String>,
    ) {
        let worked = outcome.is_ok();
        self.sharing_landed(outcome);
        if worked {
            self.invite_email.clear();
            self.sharing_message = Some(said.clone());
            self.status = said;
        }
    }

    /// What the server holds, asked rather than assumed.
    pub fn standing_landed(&mut self, outcome: Result<crate::cloud::collab::Standing, String>) {
        self.working = None;
        let at = Local::now().naive_local();
        match outcome {
            Ok(standing) => {
                let here = self.link.as_ref().map(|link| link.cursor).unwrap_or(0);
                self.checked = Some(Checked {
                    at,
                    outcome: if standing.head > here {
                        CheckOutcome::Behind {
                            by: standing.head - here,
                        }
                    } else {
                        CheckOutcome::Current
                    },
                });
                // The server counts this copy's own socket among the
                // connected, so the number shown is who else is there.
                self.cloud_message = Some(format!(
                    "{} is at change {} on the server, with {} connection(s) open.",
                    standing.name, standing.head, standing.connected
                ));
            }
            Err(why) => {
                self.checked = Some(Checked {
                    at,
                    outcome: CheckOutcome::Failed(why.clone()),
                });
                self.cloud_message = Some(why);
            }
        }
    }

    /// This plan is now on the server.
    pub fn publish_landed(&mut self, outcome: Result<crate::cloud::collab::Created, String>) {
        self.working = None;
        match outcome {
            Ok(created) => {
                let link = crate::cloud::link::Link {
                    project: created.id,
                    cursor: created.head,
                };
                if let Some(path) = self.file_path.clone() {
                    crate::cloud::link::save(&path, &link);
                }
                // The plan went up whole, so everything in its log is already
                // there and none of it is waiting to be sent again.
                if let Some(newest) = self.project.history.changes().last().map(|c| c.id) {
                    self.project.history.mark_pushed(newest);
                }
                self.link = Some(link);
                self.checked = Some(Checked {
                    at: Local::now().naive_local(),
                    outcome: CheckOutcome::Current,
                });
                self.cloud_message = Some(
                    "This plan is on the server. Other people you share it with will see it \
                     the next time they sync."
                        .into(),
                );
                self.status = "Plan put on the server".into();
            }
            Err(why) => self.cloud_message = Some(why),
        }
    }

    /// Take a whole plan from the server, in place of this one.
    pub fn fresh_copy_landed(&mut self, outcome: Result<crate::cloud::collab::Fetched, String>) {
        self.working = None;
        let fetched = match outcome {
            Ok(fetched) => fetched,
            Err(why) => {
                self.cloud_message = Some(why);
                return;
            }
        };

        // Whatever is on screen is about to be replaced wholesale, so it is
        // kept first. This is the same reason a rebase keeps one: it is the
        // moment a planner will want to go back to if the answer is wrong.
        self.keep_version(aop_core::versions::Taken::BeforeRebase);
        self.checkpoint();

        let mut plan = fetched.plan;
        // The log comes down with the plan and then has the tail appended, so
        // the two are the same story rather than a plan and a separate list.
        plan.history.merge(fetched.changes.iter().cloned());
        let head = fetched.head.max(fetched.seq);
        self.project = plan;
        if let Some(newest) = self.project.history.changes().last().map(|c| c.id) {
            self.project.history.mark_pushed(newest);
        }
        self.remember_cursor(head);
        self.checked = Some(Checked {
            at: Local::now().naive_local(),
            outcome: CheckOutcome::Current,
        });
        self.dialog = None;
        self.clamp_selection();
        self.reschedule();
        self.status = "Took a fresh copy from the server".into();
        self.cloud_message = Some(
            "This plan has been replaced with the server's copy. The version you had is in \
             History and Sync if you want it back."
                .into(),
        );
    }

    /// Somebody has opened a link. Ask about it before anything is fetched.
    ///
    /// Nothing is contacted here. The link arrived from a browser, a chat
    /// message or a paste box, and the server it names is the sender's choice
    /// rather than this planner's, so the choice is put in front of them.
    pub fn open_link_asked(&mut self, link: &str) {
        match crate::cloud::share::read(link) {
            Some(share) => self.dialog = Some(Dialog::OpenLink(share)),
            None => {
                self.cloud_message = Some(format!(
                    "That link could not be read: {link}. A plan link looks like \
                     aop://your-server/plan/ and then the plan's id."
                ))
            }
        }
    }

    /// A plan fetched by opening a link.
    ///
    /// Opened as a plan with no file behind it, which is what it is: it came
    /// off a server and has never been saved here. Saving it is what gives the
    /// cursor somewhere to live, since the link store is keyed by where a plan
    /// sits on this machine.
    pub fn open_link_landed(
        &mut self,
        server: String,
        project: String,
        outcome: Result<crate::cloud::collab::Fetched, String>,
    ) {
        self.working = None;
        let fetched = match outcome {
            Ok(fetched) => fetched,
            Err(why) => {
                self.cloud_message = Some(why);
                return;
            }
        };

        let mut plan = fetched.plan;
        plan.history.merge(fetched.changes.iter().cloned());
        let head = fetched.head.max(fetched.seq);
        self.project = plan;
        if let Some(newest) = self.project.history.changes().last().map(|c| c.id) {
            self.project.history.mark_pushed(newest);
        }
        // A plan opened from a link has no file yet, so nothing here is a
        // change to something on disk and there is nothing to recover.
        self.file_path = None;
        self.dirty = true;
        self.undo.clear();
        self.redo.clear();
        // Adopted only now, and only once the server has actually answered
        // with the plan: a link that turned out to name nothing never changes
        // where this copy syncs.
        self.collaborate_server = server;
        self.link = Some(crate::cloud::link::Link {
            project,
            cursor: head,
        });
        self.checked = Some(Checked {
            at: Local::now().naive_local(),
            outcome: CheckOutcome::Current,
        });
        self.versions = aop_core::versions::Versions::new();
        self.dialog = None;
        self.clamp_selection();
        self.reschedule();
        self.status = "Opened from a link".into();
        self.cloud_message = Some(
            "This plan came from the server and has not been saved on this machine yet. \
             Save it somewhere to keep it, and to remember how far down the server's log \
             this copy has read."
                .into(),
        );
    }

    /// Take somebody else's work, keeping this planner's on top of it.
    ///
    /// A version is kept first, and that is the point of the feature: a rebase
    /// is the only thing in the application that replays a planner's own work
    /// against somebody else's, so it is the one they will want back if it
    /// turns out wrong.
    pub fn accept_incoming(
        &mut self,
        head: i64,
        differences: &[aop_core::compare::Difference],
        changes: Vec<aop_core::history::Change>,
        replayed: usize,
        asked: usize,
    ) -> BroughtIn {
        self.keep_version(aop_core::versions::Taken::BeforeRebase);

        let before = self.project.clone();
        let applied = aop_core::compare::apply(&mut self.project, differences);
        let brought = BroughtIn {
            applied,
            replayed,
            sent: asked,
        };

        if !brought.is_clean() {
            // Nothing is left half changed. A batch that did not fit means the
            // two sides no longer agree about what the plan was, and applying
            // the part that happened to fit would make that permanent.
            self.project = before;
            self.dialog = Some(Dialog::FreshCopy {
                why: format!(
                    "Their changes could not be brought into this copy: {}. Nothing has been \
                     changed here. That means the two copies have drifted apart, and the only \
                     honest way back is a fresh whole plan from the server.",
                    brought.why()
                ),
            });
            return brought;
        }

        // The entries are the record of what was done, so they go in with the
        // work rather than being left to a later sync to discover.
        self.project.history.merge(changes);
        self.undo.push(before);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        self.remember_cursor(head);
        self.dialog = None;
        self.clamp_selection();
        self.reschedule();
        self.status = "Took the other changes; yours are still here".into();
        brought
    }

    /// What a run of somebody else's commands would do here, without doing it.
    ///
    /// Their commands are replayed onto a copy of the plan rather than onto
    /// the plan itself, because the question is not "run these" but "what do
    /// these do to what I have". A command means something different against a
    /// different plan, and the difference is the part that can be put in front
    /// of a person and checked before anything lands.
    ///
    /// Gives back the differences, how many commands could be replayed, and
    /// how many there were. The last two are how drift shows up: a command
    /// that will not run here is the two sides having already parted.
    fn preview_incoming(
        &mut self,
        incoming: &[aop_core::history::Change],
    ) -> (Vec<aop_core::compare::Difference>, usize, usize) {
        let mine = self.project.clone();
        let (replayed, asked) = self.replay(incoming);
        let theirs = std::mem::replace(&mut self.project, mine);
        (
            aop_core::compare::compare(&self.project, &theirs),
            replayed,
            asked,
        )
    }

    /// Run recorded commands without writing any of them down.
    ///
    /// They were written down when they were first done, by whoever did them,
    /// and the entries come across on the wire. Recording them again would say
    /// the work happened twice.
    fn replay(&mut self, incoming: &[aop_core::history::Change]) -> (usize, usize) {
        let mut commands: Vec<Cmd> = Vec::new();
        // A script this build cannot read counts as one command that did not
        // run, rather than as nothing: it is work that is not here.
        let mut unreadable = 0usize;
        for change in incoming {
            match crate::macros::script::parse(&change.script) {
                Ok(mut parsed) => commands.append(&mut parsed),
                Err(_) => unreadable += 1,
            }
        }
        let asked = commands.len() + unreadable;

        let replayed = self.unrecorded(|state| {
            state.as_one_step(|state| {
                let mut done = 0usize;
                for command in &commands {
                    if command.apply(state).is_err() {
                        break;
                    }
                    done += 1;
                }
                done
            })
        });
        (replayed, asked)
    }

    /// Remember how far down the server's log this copy has read.
    ///
    /// Only ever forward. A cursor that went backwards would ask for changes
    /// this copy already holds, and `merge` would drop them, which looks
    /// exactly like a sync that worked.
    fn remember_cursor(&mut self, head: i64) {
        let Some(link) = self.link.as_mut() else {
            return;
        };
        if head <= link.cursor {
            return;
        }
        link.cursor = head;
        let link = link.clone();
        if let Some(path) = self.file_path.clone() {
            crate::cloud::link::save(&path, &link);
        }
    }

    // ---- live editing ----------------------------------------------------

    /// Open the socket with a token a worker has just fetched.
    pub fn start_live(&mut self, token: String) {
        self.working = None;
        let Some(link) = self.link.clone() else {
            return;
        };
        let name = match self.display_name().as_str() {
            "" => "Someone".to_string(),
            name => name.to_string(),
        };
        match crate::cloud::live::Live::connect(
            self.collaborate_server.trim(),
            &token,
            &link.project,
            link.cursor,
            &name,
        ) {
            Ok(live) => {
                self.live = Some(live);
                self.live_wanted = true;
                self.status = "Live editing is on".into();
            }
            Err(error) => {
                self.live_wanted = false;
                self.cloud_message = Some(error.to_string());
            }
        }
    }

    /// Close the socket, and say why if there is anything to say.
    pub fn stop_live(&mut self, why: Option<String>) {
        self.live = None;
        self.live_wanted = false;
        self.peers.clear();
        // Forgotten as well as cleared, so the next session says where this
        // planner is rather than assuming the others already know.
        self.told_row = None;
        self.told_at = None;
        if let Some(why) = why {
            self.status = why.clone();
            self.cloud_message = Some(why);
        }
    }

    /// Take whatever has arrived on the live socket.
    ///
    /// Driven from a timer rather than by the socket, because the plan may
    /// only be written where the interface runs and the socket is on a thread
    /// of its own.
    pub fn poll_live(&mut self) {
        use crate::cloud::live::Incoming;

        let Some(live) = self.live.as_mut() else {
            return;
        };
        let batch = live.drain();
        if batch.is_empty() {
            return;
        }

        let mut incoming = Vec::new();
        let mut cursor = None;
        let mut ended = None;
        let mut gap = false;

        for message in batch {
            match message {
                Incoming::Welcome { head, peers } => {
                    self.peers = peers;
                    cursor = Some(head.max(cursor.unwrap_or(head)));
                }
                // A catch-up comes before any live change on purpose, so the
                // order things are applied in is the log's order.
                Incoming::Catchup { head, changes } => {
                    cursor = Some(head.max(cursor.unwrap_or(head)));
                    incoming.extend(changes);
                }
                Incoming::Change { seq, change } => {
                    cursor = Some(seq.max(cursor.unwrap_or(seq)));
                    incoming.push(change);
                }
                Incoming::Gap { .. } => gap = true,
                Incoming::Presence(peer) => {
                    match self
                        .peers
                        .iter_mut()
                        .find(|held| held.subject == peer.subject)
                    {
                        Some(held) => {
                            held.row = peer.row;
                            // Absent means unchanged. Copying it across
                            // regardless would blank somebody's pointer every
                            // time they moved their selection, which is the
                            // one thing the protocol says not to do.
                            if peer.at.is_some() {
                                held.at = peer.at;
                            }
                        }
                        None => self.peers.push(peer),
                    }
                }
                Incoming::Joined { name } => self.status = format!("{name} joined this plan"),
                Incoming::Left { subject } => self.peers.retain(|held| held.subject != subject),
                Incoming::Closed(why) => ended = Some(why),
            }
        }

        if gap {
            self.stop_live(None);
            self.dialog = Some(Dialog::FreshCopy {
                why: "The server's log has been trimmed past the point this copy had reached, \
                      so live editing has nothing to replay onto. Live editing has been turned \
                      off, and this copy needs a fresh whole plan."
                    .into(),
            });
            return;
        }

        if !incoming.is_empty() {
            self.take_live_batch(&incoming, cursor);
        }
        if let Some(why) = ended {
            self.stop_live(Some(why));
        }
    }

    /// Tell the others where this planner is, when that has changed.
    ///
    /// Sent from the same timer that reads the socket rather than from
    /// wherever the selection moves or the pointer goes, because those happen
    /// in a dozen places and every one of them would have to remember to say
    /// so. The timer is also the throttle: a mouse produces events far faster
    /// than a socket should carry them, and one message per movement would
    /// flood it to say almost nothing.
    ///
    /// `at` is where the pointer is now, which the interface keeps out of the
    /// plan's state so that moving a mouse does not redraw a window. Nothing
    /// is sent unless the row or the pointer has actually moved somewhere new.
    pub fn announce(&mut self, at: Option<crate::cloud::live::Pointer>) {
        let row = self.primary().map(|row| row as i64);
        let moved = at.filter(|at| Some(*at) != self.told_at);
        if row == self.told_row && moved.is_none() {
            return;
        }
        if let Some(live) = self.live.as_ref() {
            live.looking_at(row, moved);
            self.told_row = row;
            if let Some(at) = moved {
                self.told_at = Some(at);
            }
        }
    }

    /// Bring one batch of live changes in.
    fn take_live_batch(&mut self, incoming: &[aop_core::history::Change], head: Option<i64>) {
        // A change already in the log is one that arrived twice, which the
        // protocol allows: it can be in a catch-up and in the live stream
        // both. Applying it again would count the work twice.
        let fresh: Vec<aop_core::history::Change> = incoming
            .iter()
            .filter(|change| {
                !self
                    .project
                    .history
                    .changes()
                    .iter()
                    .any(|held| held.id == change.id)
            })
            .cloned()
            .collect();
        if fresh.is_empty() {
            if let Some(head) = head {
                self.remember_cursor(head);
            }
            return;
        }

        let (differences, replayed, asked) = self.preview_incoming(&fresh);
        let before = self.project.clone();
        let applied = aop_core::compare::apply(&mut self.project, &differences);
        let brought = BroughtIn {
            applied,
            replayed,
            sent: asked,
        };

        if !brought.is_clean() {
            // Carrying on from here is exactly the failure this check exists
            // to catch. The plan goes back as it was, the socket is closed,
            // and a whole plan is offered instead of a quietly wrong one.
            self.project = before;
            self.stop_live(None);
            self.dialog = Some(Dialog::FreshCopy {
                why: format!(
                    "A live change could not be brought into this copy: {}. Nothing has been \
                     changed here and live editing has been turned off. The two copies have \
                     drifted apart, and a fresh whole plan from the server is the way back.",
                    brought.why()
                ),
            });
            return;
        }

        self.project.history.merge(fresh.clone());
        self.undo.push(before);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        if let Some(head) = head {
            self.remember_cursor(head);
        }
        self.clamp_selection();
        self.reschedule();
        let who = fresh
            .last()
            .map(|change| change.author.clone())
            .unwrap_or_else(|| "Somebody".into());
        self.status = match fresh.len() {
            1 => format!("{who} changed this plan"),
            many => format!("{who} and others made {many} changes"),
        };
    }

    // ---- versions --------------------------------------------------------

    /// Keep the plan as it stands, so it can be returned to.
    ///
    /// Returns whether anything was kept: a plan identical to the newest
    /// version is not kept twice, for the same reason a second Ctrl+S with no
    /// edit between does not leave a second save marker.
    pub fn keep_version(&mut self, taken: aop_core::versions::Taken) -> bool {
        let author = match self.display_name().as_str() {
            "" => "Unknown".to_string(),
            name => name.to_string(),
        };
        let kept = self
            .versions
            .take(&self.project, author, Local::now().naive_local(), taken);
        if kept {
            crate::versions::write(self.file_path.as_deref(), &self.versions);
        }
        kept
    }

    /// Put the plan back to one of its versions.
    ///
    /// The log stays where it is. Going back to an older plan is one more
    /// thing that was done, not a reason to forget the record of the rest, and
    /// a trail that hides removals is not a trail.
    pub fn restore_version(&mut self, index: usize) {
        let Some(snapshot) = self.versions.get(index).cloned() else {
            return;
        };
        self.checkpoint();
        let log = std::mem::take(&mut self.project.history);
        let when = snapshot.at.format("%Y-%m-%d %H:%M");
        self.project = snapshot.plan;
        self.project.history = log;
        self.dialog = None;
        self.clamp_selection();
        self.reschedule();
        self.status = format!("Went back to the version from {when}");
        // A restore cannot be written as a command, so a shared plan cannot be
        // told about it by replaying anything. Saying the check is no longer
        // good is better than leaving a tick that now means nothing.
        if self.link.is_some() {
            self.checked = None;
            self.cloud_message = Some(
                "This copy has gone back to an older version. The server has not been told: \
                 sync to see what it makes of it."
                    .into(),
            );
        }
    }

    /// Put a correction into the plan.
    pub fn correct_spelling(&mut self, place: aop_core::spelling::Place, from: &str, to: &str) {
        use aop_core::spelling::{replace_word, Place};
        self.checkpoint();
        match place {
            Place::ProjectName => {
                self.project.name = replace_word(&self.project.name, from, to);
            }
            Place::TaskName(row) => {
                if let Some(task) = self.project.tasks.get_mut(row) {
                    task.name = replace_word(&task.name, from, to);
                }
            }
            Place::TaskNotes(row) => {
                if let Some(task) = self.project.tasks.get_mut(row) {
                    task.notes = replace_word(&task.notes, from, to);
                }
            }
            Place::ResourceName(index) => {
                if let Some(resource) = self.project.resources.get_mut(index) {
                    resource.name = replace_word(&resource.name, from, to);
                }
            }
        }
        self.dirty = true;
        self.status = format!("Changed \"{from}\" to \"{to}\"");
    }

    /// Leave a word alone for this plan.
    pub fn ignore_word(&mut self, word: &str) {
        self.ignored_words.insert(word.trim().to_lowercase());
        self.status = format!("Ignoring \"{word}\"");
    }

    /// Colour the selected rows, or clear the colour when given nothing.
    ///
    /// Held on the task rather than in a view setting, so a recoloured plan
    /// still looks the same when it is sent to somebody else.
    pub fn set_row_colour(&mut self, text: Option<&str>, fill: Option<&str>) {
        self.checkpoint();
        for row in self.selection.clone() {
            if let Some(task) = self.project.tasks.get_mut(row) {
                if let Some(colour) = text {
                    task.text_colour = colour.trim().to_string();
                }
                if let Some(colour) = fill {
                    task.fill_colour = colour.trim().to_string();
                }
            }
        }
        self.dirty = true;
        self.status = "Row colour changed".into();
    }

    /// What the last selected row is coloured, for showing on the button.
    pub fn current_row_colours(&self) -> (String, String) {
        self.primary()
            .and_then(|row| self.project.tasks.get(row))
            .map(|task| (task.text_colour.clone(), task.fill_colour.clone()))
            .unwrap_or_default()
    }

    /// Record something outside the plan that work can wait on.
    pub fn add_external(&mut self, reference: &str, label: &str, available: NaiveDateTime) -> u32 {
        self.checkpoint();
        let id = self.project.allocate_external_id();
        self.project.external.push(aop_core::model::ExternalDependency {
            id,
            reference: reference.trim().to_string(),
            label: label.trim().to_string(),
            source: String::new(),
            available,
            notes: String::new(),
        });
        self.dirty = true;
        self.reschedule();
        id
    }

    /// Change one, and reschedule, since its date moves work.
    pub fn update_external(&mut self, id: u32, apply: impl FnOnce(&mut aop_core::model::ExternalDependency)) {
        self.checkpoint();
        if let Some(entry) = self.project.external.iter_mut().find(|e| e.id == id) {
            apply(entry);
        }
        self.dirty = true;
        self.reschedule();
    }

    /// Remove one, and unhook every task that was waiting on it.
    ///
    /// Leaving the references behind would be harmless to the scheduler, which
    /// ignores unknown ids, but it would mean a task quietly waiting on nothing.
    pub fn remove_external(&mut self, id: u32) {
        self.checkpoint();
        self.project.external.retain(|entry| entry.id != id);
        for task in &mut self.project.tasks {
            task.external_predecessors.retain(|held| *held != id);
        }
        self.dirty = true;
        self.reschedule();
    }

    /// Make a task wait on something, or stop it waiting.
    pub fn toggle_external_on(&mut self, row: usize, id: u32) {
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            if task.external_predecessors.contains(&id) {
                task.external_predecessors.retain(|held| *held != id);
            } else {
                task.external_predecessors.push(id);
            }
        }
        self.dirty = true;
        self.reschedule();
    }

    /// Make the change a flagged issue asked for.
    pub fn fix_issue(&mut self, row: usize, fix: aop_core::issues::TaskFix) {
        self.checkpoint();
        if aop_core::issues::apply_fix(&mut self.project, row, fix) {
            self.dirty = true;
            self.reschedule();
            self.status = format!("{} on row {}", fix.label(), row + 1);
        }
    }

    /// Stop flagging one sort of issue on one task.
    ///
    /// Dismissing lives on the task and is saved with the plan: a warning that
    /// came back on reopening would make dismissing it pointless.
    pub fn ignore_issue(&mut self, row: usize, kind: aop_core::model::IssueKind) {
        self.checkpoint();
        aop_core::issues::ignore(&mut self.project, row, kind);
        self.dirty = true;
        self.status = "Warning dismissed for this task".into();
    }

    /// Show one dismissed warning again.
    pub fn restore_issue(&mut self, row: usize, kind: aop_core::model::IssueKind) {
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            task.ignored_issues.retain(|held| *held != kind);
        }
        self.dirty = true;
        self.status = "Warning shown again".into();
    }

    /// Show every warning on a task again.
    pub fn restore_issues(&mut self, row: usize) {
        self.checkpoint();
        aop_core::issues::stop_ignoring(&mut self.project, row);
        self.dirty = true;
        self.status = "Warnings restored for this task".into();
    }

    /// Write a snapshot of the plan, so a crash cannot lose it.
    ///
    /// Only unsaved work is worth snapshotting: once the plan matches its file
    /// there is nothing a snapshot could give back that the file does not.
    pub fn snapshot(&self) {
        if self.dirty {
            crate::recovery::write(&self.project, self.file_path.as_deref());
        }
    }

    /// Take back work from a session that never finished.
    ///
    /// The plan comes back unsaved and pointed at wherever it came from, so the
    /// user still decides where it lands. Writing it out for them would be
    /// deciding on their behalf, using a file they may not have chosen.
    pub fn recover(&mut self, found: crate::recovery::Recovered) {
        match persist::open(&found.snapshot) {
            Ok(project) => {
                self.project = project;
                self.file_path = found.origin.clone();
                self.dirty = true;
                self.undo.clear();
                self.redo.clear();
                self.pending.clear();
                self.selection = if self.project.tasks.is_empty() {
                    Vec::new()
                } else {
                    vec![0]
                };
                self.dialog = None;
                self.backstage = None;
                self.reschedule();
                self.status = match &found.origin {
                    Some(path) => format!(
                        "Recovered unsaved changes to {}. Save to keep them.",
                        path.display()
                    ),
                    None => "Recovered an unsaved plan. Save to keep it.".into(),
                };
            }
            Err(error) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not recover".into(),
                    body: error.to_string(),
                });
            }
        }
        crate::recovery::clear(&found.snapshot);
    }

    /// Do something that discards the plan, asking about unsaved work first.
    ///
    /// Everything that throws away the open plan goes through here, so there is
    /// one place that decides whether the question needs asking rather than
    /// each command remembering to ask.
    pub fn guard(&mut self, action: PendingAction) {
        if self.dirty {
            self.dialog = Some(Dialog::UnsavedChanges(action));
        } else {
            self.carry_out(action);
        }
    }

    /// Carry out a guarded action, once unsaved work is no longer in the way.
    ///
    /// Quitting only raises a flag: closing the window needs the window, which
    /// belongs to the title bar, so that one is left for it to notice.
    pub fn carry_out(&mut self, action: PendingAction) {
        self.dialog = None;
        match action {
            PendingAction::Quit => {
                // Leaving on purpose, having been asked about the work: there
                // is nothing here that was lost, so nothing to offer back.
                crate::recovery::discard();
                self.quit_requested = true;
            }
            PendingAction::CloseProject => {
                self.new_from_template("blank");
                self.note("Closed the project");
            }
            PendingAction::NewFromTemplate(id) => self.new_from_template(&id),
            PendingAction::Open(path) => self.open_any(path),
            PendingAction::AdoptImport => self.adopt_import(),
        }
    }

    /// Save, so a guarded action can go ahead.
    ///
    /// A plan that has never been saved has nowhere to go yet, so this opens
    /// Save As and remembers what to do once a name has been chosen.
    pub fn save_then(&mut self, action: PendingAction) {
        if self.save() {
            self.carry_out(action);
            return;
        }
        self.after_save = Some(action);
        self.dialog = None;
        self.backstage = Some(BackstagePage::SaveAs);
    }

    /// Open whatever the path points at, picking the reader from its extension.
    pub fn open_any(&mut self, path: PathBuf) {
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(extension.as_str(), "xlsx" | "xlsm" | "xls" | "ods") {
            self.import_excel(path);
        } else if persist::IMPORTED_EXTENSIONS.contains(&extension.as_str()) {
            self.import_path(path);
        } else {
            self.open_path(path);
        }
    }

    /// Put a save marker in the log, and say which change it landed on.
    ///
    /// A save is the unit a sync offers somebody a decision about: without one
    /// the log is a wall of single commands, and nobody decides about
    /// `indent()`. A second save with nothing edited in between adds no such
    /// unit, so it leaves one marker rather than two with nothing between
    /// them, and says so by giving back nothing.
    pub fn mark_save_point(&mut self) -> Option<u64> {
        // A blank name is what a fresh install has. Saying so beats signing
        // somebody else's work with a guess.
        let author = match self.display_name().as_str() {
            "" => "Unknown".to_string(),
            name => name.to_string(),
        };
        self.project
            .history
            .mark_saved(author, Local::now().naive_local(), None)
    }

    pub fn save_to(&mut self, path: PathBuf) {
        // Marked before the write, so what lands on disk carries the marker.
        self.mark_save_point();

        match persist::save(&path, &self.project) {
            Ok(written) => {
                self.status = format!("Saved to {}", written.display());
                self.backstage_message = Some(format!("Saved {}", written.display()));
                self.dirty = false;
                self.push_recent(&written);
                let first_save = self.file_path.as_ref() != Some(&written);
                self.file_path = Some(written);
                self.backstage = None;

                // A plan saved somewhere new takes its versions with it: they
                // are keyed by where the plan lives, and Save As is the plan
                // moving rather than a different plan.
                if first_save {
                    self.restore_link();
                }
                self.keep_version(aop_core::versions::Taken::Save);

                // The plan is on disk now, so the snapshot has nothing left to
                // give back and would only turn up as a false alarm later.
                crate::recovery::discard();

                // If this save was only asked for so that something else could
                // go ahead, that something else happens now.
                if let Some(action) = self.after_save.take() {
                    self.carry_out(action);
                }
            }
            Err(error) => {
                // The plan is still unsaved, so whatever was waiting on the
                // save must not go ahead.
                self.after_save = None;
                self.dialog = Some(Dialog::Message {
                    title: "Could not save file".into(),
                    body: error.to_string(),
                });
            }
        }
    }

    /// Save over the current file, or fall through to Save As when there is none.
    pub fn save(&mut self) -> bool {
        match self.file_path.clone() {
            Some(path) => {
                self.save_to(path);
                true
            }
            None => false,
        }
    }

    fn push_recent(&mut self, path: &PathBuf) {
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Project".into());
        self.recent.retain(|entry| &entry.path != path);
        self.recent.insert(
            0,
            RecentEntry {
                name,
                path: path.clone(),
            },
        );
        self.recent.truncate(12);
        save_recent(&self.recent);
    }

    /// Write the plan as a workbook.
    pub fn export_excel_to(&mut self, path: PathBuf) {
        match aop_core::excel::save(&path, &self.project) {
            Ok(()) => {
                self.status = format!("Exported {}", path.display());
                self.backstage_message = Some(format!("Exported {}", path.display()));
            }
            Err(error) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not export".into(),
                    body: error.to_string(),
                })
            }
        }
    }

    pub fn export_csv_to(&mut self, path: PathBuf) {
        let path = path.with_extension("csv");
        self.write_export(path, persist::to_csv(&self.project));
    }

    /// Write the print-ready page, used by both Print and Export.
    pub fn export_html_to(&mut self, path: PathBuf) {
        let path = path.with_extension("html");
        self.write_export(path, persist::to_print_html(&self.project));
    }

    /// Shared file write that reports back on the page rather than closing it.
    fn write_export(&mut self, path: PathBuf, contents: String) {
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent) {
                self.backstage_message = None;
                self.dialog = Some(Dialog::Message {
                    title: "Could not export".into(),
                    body: format!("{}: {error}", parent.display()),
                });
                return;
            }
        match std::fs::write(&path, contents) {
            Ok(()) => {
                let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                self.status = format!("Exported {}", path.display());
                self.backstage_message =
                    Some(format!("Saved {} ({bytes} bytes)", path.display()));
            }
            Err(error) => {
                self.backstage_message = None;
                self.dialog = Some(Dialog::Message {
                    title: "Could not export".into(),
                    body: format!("{}: {error}", path.display()),
                });
            }
        }
    }

    // ---- task commands --------------------------------------------------

    /// Insert a blank task above the selection, the behaviour of the Task button.
    pub fn insert_task(&mut self) {
        self.record(Cmd::InsertTask {});
        self.checkpoint();
        let at = self.primary().unwrap_or(self.project.tasks.len());
        let id = self.project.insert_task(at, "");
        let mode = self.new_tasks_mode;
        if let Some(task) = self.project.task_mut(id) {
            task.mode = mode;
        }
        self.select(at);
        self.editing = Some((at, Column::Name));
        self.reschedule();
        self.status = "New task inserted".into();
    }

    /// Append a row at the bottom, used when typing into the blank last row.
    pub fn append_task(&mut self, name: &str) -> usize {
        self.record(Cmd::AppendTask {
            name: name.to_string(),
        });
        self.checkpoint();
        let at = self.project.tasks.len();
        let id = self.project.insert_task(at, name);
        if let Some(task) = self.project.task_mut(id) {
            task.estimated = false;
        }
        self.reschedule();
        at
    }

    pub fn insert_milestone(&mut self) {
        self.record(Cmd::InsertMilestone {});
        self.checkpoint();
        let at = self.primary().unwrap_or(self.project.tasks.len());
        let id = self.project.insert_task(at, "New milestone");
        if let Some(task) = self.project.task_mut(id) {
            task.duration_minutes = 0;
            task.estimated = false;
        }
        self.select(at);
        self.editing = Some((at, Column::Name));
        self.reschedule();
        self.status = "Milestone inserted".into();
    }

    /// Insert a summary row and nest the selection underneath it.
    pub fn insert_summary(&mut self) {
        self.record(Cmd::InsertSummary {});
        self.checkpoint();
        let at = self.primary().unwrap_or(self.project.tasks.len());
        let rows = self.ordered_selection();
        let id = self.project.insert_task(at, "New summary task");
        if let Some(task) = self.project.task_mut(id) {
            task.estimated = false;
        }
        // Everything that was selected shifts down one and indents under the new row.
        let shifted: Vec<usize> = if rows.is_empty() {
            vec![at + 1]
        } else {
            rows.iter().map(|&r| r + 1).collect()
        };
        if shifted.iter().all(|&r| r < self.project.tasks.len()) {
            for &row in &shifted {
                self.project.indent(row);
            }
        }
        self.select(at);
        self.editing = Some((at, Column::Name));
        self.reschedule();
        self.status = "Summary task inserted".into();
    }

    pub fn delete_selected(&mut self) {
        // Delete acts on whatever is selected, and a shape on the chart is as
        // selectable as a row is.
        if self.delete_selected_drawing() {
            return;
        }
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::DeleteTasks {});
        self.checkpoint();
        let mut rows = self.ordered_selection();
        rows.reverse();
        for row in rows {
            self.project.delete_task(row);
        }
        self.clamp_selection();
        self.reschedule();
        self.status = "Task deleted".into();
    }

    pub fn indent_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::Indent {});
        self.checkpoint();
        for row in self.ordered_selection() {
            self.project.indent(row);
        }
        self.reschedule();
    }

    pub fn outdent_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::Outdent {});
        self.checkpoint();
        for row in self.ordered_selection() {
            self.project.outdent(row);
        }
        self.reschedule();
    }

    pub fn move_selected(&mut self, delta: isize) {
        let Some(row) = self.primary() else { return };
        let span = self.project.descendants(row).end - row;
        let target = if delta < 0 {
            row.checked_sub(1)
        } else {
            let after = row + span;
            if after < self.project.tasks.len() {
                Some(self.project.descendants(after).end)
            } else {
                None
            }
        };
        let Some(target) = target else { return };
        self.record(if delta < 0 {
            Cmd::MoveUp {}
        } else {
            Cmd::MoveDown {}
        });
        self.checkpoint();
        self.project.move_task(row, target);
        let landed = if delta < 0 { target } else { target - span };
        self.select(landed);
        self.reschedule();
    }

    /// Link the selection into a finish-to-start chain, in selection order.
    pub fn link_selected(&mut self) {
        if self.selection.len() < 2 {
            self.status = "Select two or more tasks to link them".into();
            return;
        }
        self.record(Cmd::Link {});
        self.checkpoint();
        let ids: Vec<_> = self
            .selection
            .iter()
            .filter_map(|&i| self.project.tasks.get(i).map(|t| t.id))
            .collect();
        for pair in ids.windows(2) {
            self.project.add_link(Link::finish_to_start(pair[0], pair[1]));
        }
        self.reschedule();
        if let Some(error) = self.schedule_error() {
            self.roll_back();
            self.dialog = Some(Dialog::Message {
                title: "Cannot create this link".into(),
                body: error,
            });
        } else {
            self.status = format!("Linked {} tasks", ids.len());
        }
    }

    pub fn unlink_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::Unlink {});
        self.checkpoint();
        let ids: Vec<_> = self
            .ordered_selection()
            .iter()
            .filter_map(|&i| self.project.tasks.get(i).map(|t| t.id))
            .collect();
        for id in ids {
            self.project.unlink_all(id);
        }
        self.reschedule();
        self.status = "Links removed".into();
    }

    pub fn set_percent_complete(&mut self, percent: u8) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::SetPercentComplete {
            percent: percent.min(100),
        });
        self.checkpoint();
        for row in self.ordered_selection() {
            if self.project.is_summary(row) {
                continue;
            }
            if let Some(task) = self.project.tasks.get_mut(row) {
                task.percent_complete = percent.min(100);
            }
        }
        self.reschedule();
        self.status = format!("Marked {percent}% complete");
    }

    pub fn set_task_mode(&mut self, mode: TaskMode) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::SetTaskMode { mode });
        self.checkpoint();
        for row in self.ordered_selection() {
            let start = self.project.tasks.get(row).map(|t| t.scheduled.start);
            if let Some(task) = self.project.tasks.get_mut(row) {
                task.mode = mode;
                match mode {
                    // Pin a manual task where it currently sits.
                    TaskMode::Manual => task.manual_start = start,
                    // Going back to auto must drop the pin, or the task would
                    // still be anchored to a stale date the next time round.
                    TaskMode::Auto => task.manual_start = None,
                }
            }
        }
        self.reschedule();
        self.status = mode.label().into();
    }

    /// Recolour the chart from one of the named palettes.
    pub fn apply_bar_preset(&mut self, index: usize) {
        self.checkpoint();
        self.project.bar_styles = aop_core::BarStyles::preset(index);
        self.gantt_style = index;
        self.dirty = true;
        let name = aop_core::BarStyles::PRESETS[index.min(aop_core::BarStyles::PRESETS.len() - 1)].0;
        self.status = format!("Gantt chart style: {name}");
    }

    /// Change one bar colour.
    pub fn set_bar_colour(&mut self, key: &str, value: &str) {
        self.checkpoint();
        self.project.bar_styles.set(key, value);
        self.dirty = true;
    }

    /// Put the selection back under the scheduler's control: auto scheduled,
    /// As Soon As Possible, with any pinned date removed.
    pub fn respect_links(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::RespectLinks {});
        self.checkpoint();
        let mut released = 0;
        for row in self.ordered_selection() {
            if let Some(task) = self.project.tasks.get_mut(row) {
                if task.constraint != ConstraintType::AsSoonAsPossible
                    || task.constraint_date.is_some()
                    || task.manual_start.is_some()
                    || task.mode == TaskMode::Manual
                {
                    released += 1;
                }
                task.mode = TaskMode::Auto;
                task.constraint = ConstraintType::AsSoonAsPossible;
                task.constraint_date = None;
                task.manual_start = None;
            }
        }
        self.reschedule();
        self.status = if released == 0 {
            "Selection was already following its links".into()
        } else {
            format!("{released} task(s) released back to auto scheduling")
        };
    }

    /// Whether any selected task is pinned by a constraint or a manual date.
    pub fn selection_is_pinned(&self) -> bool {
        self.selection.iter().any(|&row| {
            self.project.tasks.get(row).is_some_and(|t| {
                t.mode == TaskMode::Manual
                    || t.constraint != ConstraintType::AsSoonAsPossible
                    || t.manual_start.is_some()
            })
        })
    }

    pub fn toggle_active(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.record(Cmd::ToggleActive {});
        self.checkpoint();
        for row in self.ordered_selection() {
            if let Some(task) = self.project.tasks.get_mut(row) {
                task.active = !task.active;
            }
        }
        self.reschedule();
    }

    pub fn toggle_collapse(&mut self, row: usize) {
        if let Some(task) = self.project.tasks.get_mut(row) {
            task.collapsed = !task.collapsed;
        }
    }

    pub fn expand_all(&mut self, collapsed: bool) {
        self.record(if collapsed {
            Cmd::CollapseAll {}
        } else {
            Cmd::ExpandAll {}
        });
        for task in &mut self.project.tasks {
            task.collapsed = collapsed;
        }
    }

    pub fn copy_selected(&mut self) {
        // Nothing selected copies nothing, and a log that said otherwise would
        // have a paste in it with no source.
        if !self.selection.is_empty() {
            self.record(Cmd::CopyTasks {});
        }
        self.clipboard = self
            .ordered_selection()
            .iter()
            .filter_map(|&i| self.project.tasks.get(i).cloned())
            .collect();
        self.status = format!("{} task(s) copied", self.clipboard.len());
    }

    pub fn cut_selected(&mut self) {
        self.record(Cmd::CutTasks {});
        // A cut is one act. Left to themselves the two calls below would put a
        // copy and then a delete in the log, which is how it was done and not
        // what was asked for.
        self.unrecorded(|state| {
            state.copy_selected();
            state.delete_selected();
        });
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.record(Cmd::PasteTasks {});
        self.checkpoint();
        let at = self.primary().unwrap_or(self.project.tasks.len());
        let rows: Vec<Task> = self.clipboard.clone();
        for (offset, source) in rows.into_iter().enumerate() {
            let mut copy = source;
            copy.id = self.project.allocate_task_id();
            copy.baseline = None;
            self.project.tasks.insert(at + offset, copy);
        }
        self.reschedule();
        self.status = "Pasted".into();
    }

    // ---- cell edits -----------------------------------------------------

    /// Exactly the text a cell editor is seeded with, so a commit can tell
    /// whether the user actually changed anything.
    pub fn current_cell_text(&self, row: usize, column: Column) -> String {
        let Some(task) = self.project.tasks.get(row) else {
            return String::new();
        };
        match column {
            Column::Name => task.name.clone(),
            Column::Duration => aop_core::format_duration_flagged(
                task.scheduled.duration_minutes,
                task.estimated,
            ),
            Column::Start => task.scheduled.start.format("%Y-%m-%d").to_string(),
            Column::Finish => task.scheduled.finish.format("%Y-%m-%d").to_string(),
            Column::Predecessors => self.project.predecessor_text(task.id),
            Column::Resources => self.project.resource_text(task),
        }
    }

    pub fn commit_cell(&mut self, row: usize, column: Column, value: &str) {
        if row >= self.project.tasks.len() {
            return;
        }
        let value = value.trim().to_string();

        // Opening a cell and clicking away must not change anything. Without
        // this, blurring a Start cell would pin the task with a constraint and
        // quietly take it out of auto scheduling.
        if value == self.current_cell_text(row, column).trim() {
            self.editing = None;
            return;
        }

        self.record(Cmd::SetField {
            row: Row(row as u32 + 1),
            field: field_of(column),
            value: value.clone(),
        });
        self.checkpoint();

        match column {
            Column::Name => {
                if let Some(task) = self.project.tasks.get_mut(row) {
                    task.name = value;
                }
            }
            Column::Duration => {
                if let Some((minutes, estimated)) = aop_core::parse_duration(&value)
                    && let Some(task) = self.project.tasks.get_mut(row) {
                        task.duration_minutes = minutes;
                        task.estimated = estimated;
                    }
            }
            Column::Start => {
                if let Some(date) = parse_date(&value) {
                    let task_mode = self.project.tasks[row].mode;
                    if let Some(task) = self.project.tasks.get_mut(row) {
                        // Typing a start date pins the task, exactly as Project does.
                        task.constraint = ConstraintType::StartNoEarlierThan;
                        task.constraint_date = Some(date);
                        if task_mode == TaskMode::Manual {
                            task.manual_start = Some(date);
                        }
                    }
                }
            }
            Column::Finish => {
                if let Some(date) = parse_date(&value) {
                    let milestone = self
                        .project
                        .tasks
                        .get(row)
                        .is_some_and(|t| t.is_milestone());
                    // A task finishes at the end of its last day, but a
                    // milestone simply happens on its date, so pin it to the
                    // start of that day rather than to knocking-off time.
                    let pinned = if milestone {
                        self.project
                            .calendar
                            .next_working_instant(date.date().and_hms_opt(0, 0, 0).unwrap_or(date))
                    } else {
                        date.date().and_hms_opt(17, 0, 0).unwrap_or(date)
                    };
                    if let Some(task) = self.project.tasks.get_mut(row) {
                        task.constraint = ConstraintType::FinishNoEarlierThan;
                        task.constraint_date = Some(pinned);
                    }
                }
            }
            Column::Predecessors => {
                let id = self.project.tasks[row].id;
                self.project.set_predecessor_text(id, &value);
            }
            Column::Resources => {
                self.project.set_resource_text(row, &value);
            }
        }

        self.editing = None;
        self.reschedule();

        // A link edit is the one cell that can create a loop; roll it back.
        if column == Column::Predecessors
            && let Some(error) = self.schedule_error() {
                self.roll_back();
                self.dialog = Some(Dialog::Message {
                    title: "Cannot create this link".into(),
                    body: error,
                });
            }
    }

    /// Add or replace the link from `predecessor` into the task on `row`.
    pub fn set_link(&mut self, row: usize, predecessor: TaskId, kind: LinkType, lag_minutes: i64) {
        let Some(successor) = self.project.tasks.get(row).map(|t| t.id) else {
            return;
        };
        if predecessor == successor {
            return;
        }
        if let Some(from) = self.row_of(predecessor) {
            self.record(Cmd::SetLink {
                row: Row(row as u32 + 1),
                predecessor: Row(from as u32 + 1),
                kind,
                lag_minutes,
            });
        }
        self.checkpoint();
        self.project.unlink(predecessor, successor);
        self.project.links.push(Link {
            predecessor,
            successor,
            kind,
            lag_minutes,
        });
        self.reschedule();
        if let Some(error) = self.schedule_error() {
            self.roll_back();
            self.dialog = Some(Dialog::Message {
                title: "Cannot create this link".into(),
                body: error,
            });
        }
    }

    pub fn remove_link(&mut self, row: usize, predecessor: TaskId) {
        let Some(successor) = self.project.tasks.get(row).map(|t| t.id) else {
            return;
        };
        if let Some(from) = self.row_of(predecessor) {
            self.record(Cmd::RemoveLink {
                row: Row(row as u32 + 1),
                predecessor: Row(from as u32 + 1),
            });
        }
        self.checkpoint();
        self.project.unlink(predecessor, successor);
        self.reschedule();
    }

    /// Book or unbook a resource against an explicit row, with units.
    pub fn set_assignment(&mut self, row: usize, resource: ResourceId, units: Option<f64>) {
        if let Some(name) = self
            .project
            .resources
            .iter()
            .find(|held| held.id == resource)
            .map(|held| held.name.clone())
        {
            let at = Row(row as u32 + 1);
            self.record(match units {
                Some(units) => Cmd::AssignResource {
                    row: at,
                    name,
                    units_percent: units * 100.0,
                },
                None => Cmd::UnassignResource { row: at, name },
            });
        }
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            match units {
                Some(units) => {
                    if let Some(existing) = task.assignments.iter_mut().find(|a| a.resource == resource) {
                        existing.units = units;
                    } else {
                        task.assignments.push(aop_core::Assignment { resource, units });
                    }
                }
                None => task.assignments.retain(|a| a.resource != resource),
            }
        }
        self.reschedule();
    }

    // ---- resources ------------------------------------------------------

    pub fn add_resource(&mut self, name: &str) {
        self.record(Cmd::AddResource {
            name: name.to_string(),
        });
        self.checkpoint();
        self.project.add_resource(name);
        self.reschedule();
    }

    pub fn delete_resource(&mut self, index: usize) {
        let Some(id) = self.project.resources.get(index).map(|r| r.id) else {
            return;
        };
        self.record(Cmd::DeleteResource {
            resource_row: Row(index as u32 + 1),
        });
        self.checkpoint();
        self.project.delete_resource(id);
        self.selected_resource = None;
        self.reschedule();
    }

    pub fn commit_resource_cell(&mut self, index: usize, field: &str, value: &str) {
        if index >= self.project.resources.len() {
            return;
        }
        // A key the sheet does not know is ignored below, so it is not written
        // down as a change either.
        if let Some(field) = resource_field_of(field) {
            self.record(Cmd::SetResourceField {
                resource_row: Row(index as u32 + 1),
                field,
                value: value.trim().to_string(),
            });
        }
        self.checkpoint();
        let value = value.trim().to_string();
        let resource = &mut self.project.resources[index];
        match field {
            "name" => resource.name = value,
            "initials" => resource.initials = value,
            "group" => resource.group = value,
            "max" => {
                let cleaned = value.trim_end_matches('%').trim().to_string();
                if let Ok(units) = cleaned.parse::<f64>() {
                    resource.max_units = (units / 100.0).max(0.0);
                }
            }
            "rate" => {
                let cleaned: String = value
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if let Ok(rate) = cleaned.parse::<f64>() {
                    resource.standard_rate = rate;
                }
            }
            "kind" => {
                resource.kind = match value.as_str() {
                    "Material" => aop_core::ResourceKind::Material,
                    "Cost" => aop_core::ResourceKind::Cost,
                    _ => aop_core::ResourceKind::Work,
                };
            }
            _ => {}
        }
        self.reschedule();
    }

    /// Book or unbook a resource against the selected task.
    pub fn toggle_assignment(&mut self, resource_index: usize) {
        let Some(row) = self.primary() else { return };
        let Some(resource_id) = self.project.resources.get(resource_index).map(|r| r.id) else {
            return;
        };
        // Which way the booking went, because "toggled" is not something a
        // script can replay or a person can read afterwards.
        let booked = self
            .project
            .tasks
            .get(row)
            .is_some_and(|task| task.assignments.iter().any(|a| a.resource == resource_id));
        if let Some(name) = self
            .project
            .resources
            .get(resource_index)
            .map(|resource| resource.name.clone())
        {
            let row = Row(row as u32 + 1);
            self.record(if booked {
                Cmd::UnassignResource { row, name }
            } else {
                Cmd::AssignResource {
                    row,
                    name,
                    units_percent: 100.0,
                }
            });
        }
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            if let Some(position) = task.assignments.iter().position(|a| a.resource == resource_id)
            {
                task.assignments.remove(position);
            } else {
                task.assignments.push(aop_core::Assignment {
                    resource: resource_id,
                    units: 1.0,
                });
            }
        }
        self.reschedule();
    }

    /// The row a task sits on, which is how the change log names it.
    ///
    /// The log counts rows the way the ID column does, so the one place a task
    /// id turns back into a position is here.
    fn row_of(&self, task: TaskId) -> Option<usize> {
        self.project.tasks.iter().position(|held| held.id == task)
    }

    /// Find a resource by the name shown on screen.
    ///
    /// Kept alongside `set_assignment` because naming a person is how a script
    /// or an import refers to them; the pickers work by id.
    #[allow(dead_code)]
    ///
    /// Names are matched trimmed and without regard to case, the same rule
    /// `set_resource_text` uses, so the two ways of naming a person agree.
    fn resource_id_by_name(&self, name: &str) -> Option<aop_core::ResourceId> {
        let wanted = name.trim().to_lowercase();
        self.project
            .resources
            .iter()
            .find(|r| r.name.trim().to_lowercase() == wanted)
            .map(|r| r.id)
    }

    /// Book a named person onto a task, or change how much of them is booked.
    #[allow(dead_code)]
    pub fn assign_resource_by_name(&mut self, row: usize, name: &str, units: f64) {
        let Some(resource) = self.resource_id_by_name(name) else {
            self.note(format!("There is no resource called {name}."));
            return;
        };
        self.record(Cmd::AssignResource {
            row: Row(row as u32 + 1),
            name: name.to_string(),
            units_percent: units * 100.0,
        });
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            match task.assignments.iter_mut().find(|a| a.resource == resource) {
                Some(existing) => existing.units = units,
                None => task
                    .assignments
                    .push(aop_core::Assignment { resource, units }),
            }
        }
        self.reschedule();
        self.dirty = true;
    }

    /// Change how much of a person a task books, leaving the booking in place.
    #[allow(dead_code)]
    pub fn set_assignment_units(&mut self, row: usize, name: &str, units: f64) {
        let Some(resource) = self.resource_id_by_name(name) else {
            return;
        };
        self.record(Cmd::SetAssignmentUnits {
            row: Row(row as u32 + 1),
            name: name.to_string(),
            units_percent: units * 100.0,
        });
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row)
            && let Some(existing) = task.assignments.iter_mut().find(|a| a.resource == resource)
        {
            existing.units = units;
        }
        self.reschedule();
        self.dirty = true;
    }

    /// Take a person off a task.
    #[allow(dead_code)]
    pub fn unassign_resource(&mut self, row: usize, name: &str) {
        let Some(resource) = self.resource_id_by_name(name) else {
            return;
        };
        self.record(Cmd::UnassignResource {
            row: Row(row as u32 + 1),
            name: name.to_string(),
        });
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            task.assignments.retain(|a| a.resource != resource);
        }
        self.reschedule();
        self.dirty = true;
    }

    // ---- project commands -----------------------------------------------

    pub fn set_baseline(&mut self) {
        self.record(Cmd::SetBaseline {});
        self.checkpoint();
        self.project.set_baseline();
        self.show_baseline = true;
        self.status = "Baseline saved".into();
    }

    pub fn clear_baseline(&mut self) {
        self.record(Cmd::ClearBaseline {});
        self.checkpoint();
        self.project.clear_baseline();
        self.show_baseline = false;
        self.status = "Baseline cleared".into();
    }

    pub fn set_project_start(&mut self, date: NaiveDateTime) {
        self.record(Cmd::SetProjectStart { date });
        self.checkpoint();
        self.project.start_date = date;
        self.reschedule();
    }

    /// Jump the timescale so the given row is in view. The chart scrolls itself,
    /// so this only needs to move the selection.
    pub fn scroll_to_task(&mut self) {
        if let Some(row) = self.primary() {
            self.select(row);
            self.status = format!("Scrolled to row {}", row + 1);
        }
    }

    // ---- drag reorder ---------------------------------------------------

    pub fn begin_drag(&mut self, row: usize) {
        self.drag_row = Some(row);
        self.drop_target = None;
        self.editing = None;
    }

    pub fn hover_drop(&mut self, row: usize, mode: DropWhere) {
        // dragover fires continuously, so only write when the target moves.
        if self.drag_row.is_some() && self.drop_target != Some((row, mode)) {
            self.drop_target = Some((row, mode));
        }
    }

    pub fn cancel_drag(&mut self) {
        self.drag_row = None;
        self.drop_target = None;
    }

    /// Finish a drag: move the dragged row and everything nested under it to
    /// the drop position, re-levelling the block to match where it landed.
    pub fn finish_drag(&mut self) {
        let (Some(from), Some((target, mode))) = (self.drag_row, self.drop_target) else {
            self.cancel_drag();
            return;
        };
        self.cancel_drag();
        self.drop_row(from, target, mode);
    }

    pub fn drop_row(&mut self, from: usize, target: usize, mode: DropWhere) {
        let count = self.project.tasks.len();
        if from >= count || target >= count || from == target {
            return;
        }
        let span = self.project.descendants(from).end - from;
        // A block can never be dropped inside itself.
        if target >= from && target < from + span {
            return;
        }

        let target_level = self.project.tasks[target].outline_level;
        let desired = match mode {
            DropWhere::Into => target_level + 1,
            _ => target_level,
        };
        let delta = desired as i32 - self.project.tasks[from].outline_level as i32;

        let insert_at = match mode {
            DropWhere::Above => target,
            DropWhere::Below | DropWhere::Into => self.project.descendants(target).end,
        };
        // Landing on the same index still counts when the nesting level changes,
        // which is what dropping onto the last child of a summary does.
        if insert_at == from && delta == 0 {
            return;
        }

        // Deliberately not recorded. The vocabulary can move a block past its
        // sibling, which is not what a drop does: a row dragged anywhere in the
        // outline and re-levelled where it lands has no command that says so,
        // and inventing one that says something else would be worse than the
        // gap.
        self.checkpoint();
        self.project.move_task(from, insert_at);

        let landed = if insert_at > from { insert_at - span } else { insert_at };
        for index in landed..(landed + span).min(self.project.tasks.len()) {
            let level = self.project.tasks[index].outline_level as i32 + delta;
            self.project.tasks[index].outline_level = level.max(0) as u16;
        }

        self.select(landed);
        self.reschedule();
        self.status = "Task moved".into();
    }

    // ---- grid columns ----------------------------------------------------

    /// Total width of the table, which is also the width of its pane.
    pub fn grid_width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }

    pub fn set_column_width(&mut self, column: usize, width: f64) {
        if let Some(slot) = self.columns.get_mut(column) {
            slot.width = width.clamp(24.0, 640.0);
        }
    }

    /// Dragging the splitter changes how much of the table is visible.
    ///
    /// It deliberately does not resize any column: narrowing the pane scrolls
    /// the table instead, so the columns stay the width they were set to.
    pub fn set_table_width(&mut self, width: f64) {
        self.table_pane_width = width.clamp(120.0, 2000.0);
    }

    /// The visible width of the table pane, never wider than the columns need.
    pub fn table_view_width(&self) -> f64 {
        self.table_pane_width.min(self.grid_width())
    }

    /// Add a column before `at`, the way Insert Column works.
    pub fn insert_column(&mut self, at: usize, field: Field) {
        if self.columns.iter().any(|c| c.field == field) {
            self.status = format!("{} is already shown", field.label());
            return;
        }
        let at = at.min(self.columns.len());
        self.record(Cmd::ShowColumn {
            field,
            at: Row(at as u32 + 1),
        });
        self.columns.insert(at, ColumnSpec::new(field));
        self.status = format!("Inserted the {} column", field.label());
    }

    pub fn remove_column(&mut self, index: usize) {
        if self.columns.len() <= 1 || index >= self.columns.len() {
            return;
        }
        if let Some(column) = self.columns.get(index) {
            self.record(Cmd::HideColumn {
                field: column.field,
            });
        }
        let removed = self.columns.remove(index);
        self.status = format!("Hid the {} column", removed.field.label());
    }

    pub fn move_column(&mut self, index: usize, delta: isize) {
        let target = index as isize + delta;
        if target < 0 || target as usize >= self.columns.len() {
            return;
        }
        self.columns.swap(index, target as usize);
    }

    pub fn reset_columns(&mut self) {
        self.record(Cmd::ResetColumns {});
        self.columns = default_columns();
        self.table_pane_width = self.grid_width();
        self.status = "Columns reset to the Entry table".into();
    }

    /// The chrono pattern the Display options picked.
    pub fn date_pattern(&self) -> &'static str {
        DATE_FORMATS
            .get(self.date_format)
            .map(|f| f.1)
            .unwrap_or(DATE_FORMATS[0].1)
    }

    // ---- internal panes -------------------------------------------------

    /// Maximise one pane of a split view, or restore both.
    pub fn toggle_pane(&mut self, pane: PaneFocus) {
        self.pane_focus = if self.pane_focus == pane {
            PaneFocus::Both
        } else {
            pane
        };

        // Format is the chart's contextual tab and goes away with the chart, so
        // leaving it selected would show its commands under no tab at all.
        if self.pane_focus == PaneFocus::TableOnly && self.tab == RibbonTab::Format {
            self.tab = RibbonTab::Task;
        }
    }

    // ---- quick access toolbar -------------------------------------------

    pub fn toggle_qat(&mut self, command: QatCommand) {
        match self.qat.iter().position(|c| *c == command) {
            Some(index) => {
                self.qat.remove(index);
            }
            None => self.qat.push(command),
        }
        save_qat(&self.qat);
    }

    pub fn move_qat(&mut self, command: QatCommand, delta: isize) {
        let Some(index) = self.qat.iter().position(|c| *c == command) else {
            return;
        };
        let target = index as isize + delta;
        if target < 0 || target as usize >= self.qat.len() {
            return;
        }
        self.qat.swap(index, target as usize);
        save_qat(&self.qat);
    }

    pub fn reset_qat(&mut self) {
        self.qat = DEFAULT_QAT.to_vec();
        save_qat(&self.qat);
        self.status = "Quick Access Toolbar reset".into();
    }

    // ---- what the views show --------------------------------------------

    /// Visible rows, honouring collapsed summaries and the active filter.
    /// A summary stays visible when any of its children pass the filter.
    pub fn visible_rows(&self) -> Vec<usize> {
        let outlined = self.project.visible_indices();
        if self.filter == TaskFilter::All {
            return outlined;
        }

        let passes = |index: usize| -> bool {
            let task = &self.project.tasks[index];
            match self.filter {
                TaskFilter::All => true,
                TaskFilter::Critical => task.scheduled.critical,
                TaskFilter::Milestones => task.is_milestone(),
                TaskFilter::Incomplete => task.percent_complete < 100,
            }
        };

        outlined
            .into_iter()
            .filter(|&index| {
                if self.project.is_summary(index) {
                    self.project
                        .descendants(index)
                        .any(|child| !self.project.is_summary(child) && passes(child))
                } else {
                    passes(index)
                }
            })
            .collect()
    }

    /// Every row the two panes should draw, bands included.
    ///
    /// The grid and the Gantt both read this one list, so a band can never
    /// push one pane out of step with the other.
    pub fn layout_rows(&self) -> Vec<GroupRow> {
        let Some(spec) = &self.group_by else {
            return self.visible_rows().into_iter().map(GroupRow::Task).collect();
        };

        let grouped = aop_core::grouping::group(&self.project, spec);
        if self.filter == TaskFilter::All {
            return grouped;
        }

        // Grouping runs over the whole plan, so a filter has to be applied
        // afterwards, and the band totals rebuilt from what actually survived.
        let kept: std::collections::HashSet<usize> = self.visible_rows().into_iter().collect();
        let mut out: Vec<GroupRow> = Vec::with_capacity(grouped.len());
        for row in grouped {
            match row {
                GroupRow::Task(index) if kept.contains(&index) => out.push(GroupRow::Task(index)),
                GroupRow::Task(_) => {}
                band @ GroupRow::Band { .. } => {
                    // Drop the band above it if nothing came through.
                    while matches!(out.last(), Some(GroupRow::Band { .. })) {
                        out.pop();
                    }
                    out.push(band);
                }
            }
        }
        while matches!(out.last(), Some(GroupRow::Band { .. })) {
            out.pop();
        }
        restate_bands(&self.project, &mut out);
        out
    }

    pub fn set_group_by(&mut self, key: &str) {
        self.group_by = match key {
            "duration" => Some(Field::Duration),
            "critical" => Some(Field::Critical),
            "milestone" => Some(Field::Milestone),
            "resources" => Some(Field::ResourceNames),
            "start" => Some(Field::Start),
            "finish" => Some(Field::Finish),
            "complete" => Some(Field::PercentComplete),
            _ => None,
        }
        .map(aop_core::grouping::GroupBy::new);
        self.record(Cmd::GroupBy {
            field: self.group_by.as_ref().map(|spec| spec.field),
        });
        self.selection.clear();
        self.status = match &self.group_by {
            Some(spec) => format!("Grouped by {}", spec.field.label()),
            None => "No group".to_string(),
        };
    }

    pub fn set_filter(&mut self, key: &str) {
        self.filter = match key {
            "critical" => TaskFilter::Critical,
            "milestones" => TaskFilter::Milestones,
            "incomplete" => TaskFilter::Incomplete,
            _ => TaskFilter::All,
        };
        self.record(Cmd::SetFilter {
            filter: self.filter,
        });
        self.selection.clear();
        self.status = format!("Filter: {}", self.filter.label());
    }

    /// Fit the whole plan on screen by picking a timescale for its span.
    pub fn zoom_to_fit(&mut self) {
        self.record(Cmd::ZoomToFit {});
        let span = (self.project.finish_date - self.project.start_date).num_days();
        self.zoom = if span > 720 {
            Zoom::Quarters
        } else if span > 200 {
            Zoom::Months
        } else if span > 60 {
            Zoom::Weeks
        } else {
            Zoom::Days
        };
        self.status = format!("Zoomed to {}", self.zoom.label());
    }

    /// Sort sibling blocks by a field, keeping the outline intact: children
    /// move with their summary and never change parent.
    pub fn sort_tasks(&mut self, key: &str) {
        if self.project.tasks.len() < 2 {
            return;
        }
        // A key nothing sorts by leaves the plan alone, so it is not a change.
        if let Some(field) = sort_field_of(key) {
            self.record(Cmd::SortBy { field });
        }
        self.checkpoint();
        let sorted = sort_range(&self.project, 0, self.project.tasks.len(), 0, key);
        self.project.tasks = sorted;
        self.clamp_selection();
        self.reschedule();
        self.status = format!("Sorted by {key}");
    }

    // ---- gantt bar dragging ---------------------------------------------

    pub fn begin_bar_drag(
        &mut self,
        row: usize,
        kind: BarDragKind,
        origin_x: f64,
        bar_width: f64,
    ) {
        // A drag with a tool armed is a shape being drawn, not a bar being
        // moved. Without this the two contend and the plan is edited by
        // accident while the planner is marking it up.
        if self.draw_tool.is_some() {
            return;
        }
        let Some(task) = self.project.tasks.get(row) else {
            return;
        };
        // Summary rows are derived from their children, so they do not drag.
        if self.project.is_summary(row) {
            self.status = "Summary bars follow their subtasks".into();
            return;
        }
        self.bar_drag = Some(BarDrag {
            row,
            kind,
            origin_x,
            delta_x: 0.0,
            base_start: task.scheduled.start,
            base_duration: task.duration_minutes,
            base_percent: task.percent_complete,
            bar_width,
            hover_row: None,
        });
        self.select(row);
    }

    pub fn update_bar_drag(&mut self, x: f64) {
        if let Some(drag) = &mut self.bar_drag {
            drag.delta_x = x - drag.origin_x;
        }
    }

    pub fn set_bar_hover(&mut self, row: usize) {
        if let Some(drag) = &mut self.bar_drag
            && drag.kind == BarDragKind::Link {
                drag.hover_row = Some(row);
            }
    }

    pub fn cancel_bar_drag(&mut self) {
        self.bar_drag = None;
    }

    /// Apply whatever the drag was doing.
    pub fn finish_bar_drag(&mut self, px_per_day: f64) {
        let Some(drag) = self.bar_drag.take() else {
            return;
        };
        if drag.row >= self.project.tasks.len() {
            return;
        }

        match drag.kind {
            BarDragKind::Move => {
                let days = drag.days(px_per_day);
                if days == 0 {
                    return;
                }
                let moved = drag.base_start + chrono::Duration::days(days);
                let snapped = self.project.calendar.next_working_instant(moved);
                // What the drag came to, not the frames it took to get there.
                // The pointer moving is not a change to the plan; where it was
                // let go is.
                self.record(Cmd::SetField {
                    row: Row(drag.row as u32 + 1),
                    field: Field::Start,
                    value: snapped.format("%Y-%m-%d %H:%M").to_string(),
                });
                self.checkpoint();
                if let Some(task) = self.project.tasks.get_mut(drag.row) {
                    // Project pins a dragged bar with a start constraint.
                    task.constraint = ConstraintType::StartNoEarlierThan;
                    task.constraint_date = Some(snapped);
                    if task.mode == TaskMode::Manual {
                        task.manual_start = Some(snapped);
                    }
                }
                self.reschedule();
                self.status = format!("Moved to {}", format_date(snapped));
            }
            BarDragKind::Resize => {
                let days = drag.days(px_per_day);
                if days == 0 {
                    return;
                }
                let minutes = (drag.base_duration + days * aop_core::MINUTES_PER_DAY).max(0);
                self.record(Cmd::SetField {
                    row: Row(drag.row as u32 + 1),
                    field: Field::Duration,
                    value: aop_core::format_duration(minutes),
                });
                self.checkpoint();
                if let Some(task) = self.project.tasks.get_mut(drag.row) {
                    task.duration_minutes = minutes;
                    task.estimated = false;
                }
                self.reschedule();
                self.status = format!("Duration {}", aop_core::format_duration(minutes));
            }
            BarDragKind::Progress => {
                let percent = drag.preview_percent();
                if percent == drag.base_percent {
                    return;
                }
                // Which row it lands on is already selected: a drag starts by
                // selecting the bar it takes hold of.
                self.record(Cmd::SetPercentComplete { percent });
                self.checkpoint();
                if let Some(task) = self.project.tasks.get_mut(drag.row) {
                    task.percent_complete = percent;
                }
                self.reschedule();
                self.status = format!("{percent}% complete");
            }
            BarDragKind::Link => {
                let Some(target) = drag.hover_row else { return };
                if target == drag.row {
                    return;
                }
                let (Some(from), Some(to)) = (
                    self.project.tasks.get(drag.row).map(|t| t.id),
                    self.project.tasks.get(target).map(|t| t.id),
                ) else {
                    return;
                };
                self.record(Cmd::SetLink {
                    row: Row(target as u32 + 1),
                    predecessor: Row(drag.row as u32 + 1),
                    kind: LinkType::FS,
                    lag_minutes: 0,
                });
                self.checkpoint();
                self.project.add_link(Link::finish_to_start(from, to));
                self.reschedule();
                if let Some(error) = self.schedule_error() {
                    self.roll_back();
                    self.dialog = Some(Dialog::Message {
                        title: "Cannot create this link".into(),
                        body: error,
                    });
                } else {
                    self.status = "Tasks linked".into();
                }
            }
        }
    }

    // ---- drawings --------------------------------------------------------

    /// Arm a drawing tool, or put it away when it was already armed.
    pub fn arm_draw_tool(&mut self, kind: ShapeKind) {
        self.draw_tool = (self.draw_tool != Some(kind)).then_some(kind);
        // Drawing with the shapes hidden would be drawing into the dark.
        self.show_drawings = true;
        self.status = match self.draw_tool {
            Some(kind) => format!("{}: drag on the chart to draw", kind.label()),
            None => "Ready".into(),
        };
    }

    /// Put down whatever shape tool is armed.
    ///
    /// Separate from `arm_draw_tool` because that one toggles: calling it to
    /// disarm would mean naming the tool you are trying to forget.
    pub fn arm_draw_tool_off(&mut self) {
        self.draw_tool = None;
        self.cancel_draw_drag();
        self.status = "Ready".into();
    }

    /// The scale the chart is drawn at.
    ///
    /// The drawing tools have to hit-test and place against exactly what the
    /// chart drew, so both take the scale from here rather than each working
    /// one out.
    pub fn chart_scale(&self) -> Scale {
        Scale {
            origin: chart_range(&self.project).0,
            px_per_day: self.zoom.px_per_day(),
        }
    }

    /// What a point in the table is over, said the way the wire says it.
    ///
    /// The row is a task index rather than a line number, and that is the
    /// whole reason this is worth converting at all: two copies can have
    /// different rows collapsed, different filters and different grouping, so
    /// line seven is a different task on each of them. The column is an index
    /// for the same reason, since column widths are a local matter.
    pub fn table_pointer(&self, row: usize, x: f64) -> crate::cloud::live::Pointer {
        let mut edge = 0.0;
        let mut column = self.columns.len().saturating_sub(1);
        for (index, spec) in self.columns.iter().enumerate() {
            edge += spec.width;
            if x < edge {
                column = index;
                break;
            }
        }
        crate::cloud::live::Pointer::Table {
            row: row as i64,
            column: column as u16,
        }
    }

    /// What a point on the chart is over, said the way the wire says it.
    ///
    /// Nothing over a band or above the first row: there is no task there to
    /// be pointing at, and naming one anyway would put somebody's pointer on a
    /// row they are not on.
    pub fn chart_pointer(&self, x: f64, y: f64) -> Option<crate::cloud::live::Pointer> {
        if y < 0.0 {
            return None;
        }
        let line = (y / ROW_H).floor() as usize;
        let Some(&GroupRow::Task(index)) = self.layout_rows().get(line) else {
            return None;
        };
        // Minutes rather than pixels, so it means the same thing to somebody
        // reading it at a different zoom.
        let minutes = (x / self.chart_scale().px_per_day * 1440.0).round() as i64;
        Some(crate::cloud::live::Pointer::Chart {
            row: index as i64,
            minutes,
        })
    }

    /// What a point on the chart is about: the bar under it, or the date it
    /// sits at when it is over bare canvas.
    ///
    /// This is the whole difference between an annotation that follows its task
    /// through a reschedule and one that stays where the plan used to be.
    pub fn chart_anchor(&self, x: f64, y: f64) -> Anchor {
        let scale = self.chart_scale();
        let row = y / ROW_H;
        if row >= 0.0
            && let Some(&GroupRow::Task(index)) = self.layout_rows().get(row.floor() as usize)
            && let Some((left, right)) = bar_edges(&self.project, &scale, self.round_bars, index)
            && x >= left
            && x <= right
            && let Some(task) = self.project.tasks.get(index).map(|t| t.id)
        {
            // Which end of the bar the shape rides with. The nearest one wins,
            // so a note dropped by a finish date stays by the finish date when
            // the bar later grows out from its start.
            let (mut point, mut px) = (BarPoint::Start, left);
            for (candidate, at) in [
                (BarPoint::Middle, (left + right) / 2.0),
                (BarPoint::Finish, right),
            ] {
                if (x - at).abs() < (x - px).abs() {
                    (point, px) = (candidate, at);
                }
            }
            return Anchor::Task {
                task,
                point,
                dx: x - px,
                dy: y - row.floor() * ROW_H,
            };
        }

        Anchor::Timescale {
            at: scale.at_x(x),
            row,
        }
    }

    /// Start pulling a new shape out, from a point in chart coordinates.
    pub fn begin_draw(&mut self, x: f64, y: f64) {
        let Some(kind) = self.draw_tool else { return };
        self.selected_drawing = None;
        self.draw_drag = Some(DrawDrag {
            kind: DrawDragKind::New(kind),
            origin: Some((x, y)),
            at: (x, y),
        });
    }

    /// Select a shape, and be ready to slide it if the pointer moves.
    pub fn begin_drawing_move(&mut self, id: DrawingId) {
        self.selected_drawing = Some(id);
        self.selection.clear();
        // A locked shape is still selectable, so it can be unlocked again; it
        // simply does not follow the pointer.
        if self.project.drawings.iter().any(|d| d.id == id && d.locked) {
            return;
        }
        self.draw_drag = Some(DrawDrag {
            kind: DrawDragKind::Move(id),
            origin: None,
            at: (0.0, 0.0),
        });
    }

    pub fn update_draw_drag(&mut self, x: f64, y: f64) {
        if let Some(drag) = &mut self.draw_drag {
            drag.origin.get_or_insert((x, y));
            drag.at = (x, y);
        }
    }

    pub fn cancel_draw_drag(&mut self) {
        self.draw_drag = None;
    }

    /// Commit whatever the drag was doing, in one undo step.
    ///
    /// Nothing here is checkpointed while the pointer moves: a drag across the
    /// chart would otherwise fill the undo stack with a hundred snapshots of a
    /// shape on its way somewhere.
    pub fn finish_draw_drag(&mut self) {
        let Some(drag) = self.draw_drag.take() else {
            return;
        };
        let px_per_day = self.zoom.px_per_day();
        let (dx, dy) = drag.delta();

        match drag.kind {
            DrawDragKind::New(kind) => {
                let Some((x, y)) = drag.origin else { return };
                let (dx, dy) = if kind == ShapeKind::Line {
                    snap_vertical(dx, dy)
                } else {
                    (dx, dy)
                };
                let (dx, dy) = drawn_size(kind, dx, dy, px_per_day);
                let anchor = self.chart_anchor(x, y);
                // A caption keeps its size on screen; everything else keeps the
                // stretch of plan it was drawn over.
                let extent = if kind == ShapeKind::TextBox {
                    Extent::Fixed { w: dx, h: dy }
                } else {
                    Extent::Scaled {
                        minutes: (dx / px_per_day.max(0.001) * 1440.0).round() as i64,
                        rows: dy / ROW_H,
                    }
                };

                self.checkpoint();
                let id = self.project.allocate_drawing_id();
                let mut shape = Drawing::new(id, kind, anchor, extent);
                shape.z = self.project.drawings.last().map_or(0, |last| last.z + 1);
                if kind == ShapeKind::TextBox {
                    shape.text = "Text".into();
                }
                self.project.drawings.push(shape);
                self.selected_drawing = Some(id);
                // One shape per arming, the way Project's drawing tools behave.
                self.draw_tool = None;
                self.status = format!("{} drawn", kind.label());
            }
            DrawDragKind::Move(id) => {
                if dx == 0.0 && dy == 0.0 {
                    return;
                }
                self.checkpoint();
                if let Some(shape) = self.project.drawings.iter_mut().find(|d| d.id == id) {
                    shape.nudge(dx, dy, px_per_day, ROW_H);
                }
                self.status = "Drawing moved".into();
            }
        }
    }

    /// Remove the selected shape.
    ///
    /// Says whether it did, so Delete can go to the shape when one is selected
    /// and to the selected rows otherwise.
    /// Change one shape, taking a checkpoint so the edit can be undone.
    ///
    /// The whole edit is one step: a planner who has retyped a caption and
    /// picked two colours means that as one change, not four.
    pub fn amend_drawing(&mut self, id: DrawingId, edit: impl FnOnce(&mut aop_core::draw::Drawing)) {
        let Some(at) = self.project.drawings.iter().position(|d| d.id == id) else {
            return;
        };
        self.checkpoint();
        edit(&mut self.project.drawings[at]);
        self.dirty = true;
    }

    pub fn delete_selected_drawing(&mut self) -> bool {
        let Some(id) = self.selected_drawing.take() else {
            return false;
        };
        if !self.project.drawings.iter().any(|d| d.id == id) {
            return false;
        }
        self.checkpoint();
        self.project.drawings.retain(|d| d.id != id);
        self.status = "Drawing deleted".into();
        true
    }

    // ---- menus ----------------------------------------------------------

    /// Start editing a cell, remembering where to anchor any popup.
    pub fn edit_cell_at(&mut self, row: usize, column: Column, x: f64, y: f64) {
        self.editing = Some((row, column));
        self.popup_at = (x, y);
        self.context_menu = None;
        self.cell_draft = self.cell_text(row, column);
    }

    /// What a cell currently reads, as the planner would type it.
    pub fn cell_text(&self, row: usize, column: Column) -> String {
        let Some(task) = self.project.tasks.get(row) else {
            return String::new();
        };
        match column {
            Column::Predecessors => self.project.predecessor_text(task.id),
            Column::Resources => self.project.resource_text(task),
            _ => String::new(),
        }
    }

    /// Put the typed text back in step with the plan after the picker has
    /// changed something.
    pub fn refresh_cell_draft(&mut self) {
        if let Some((row, column)) = self.editing {
            self.cell_draft = self.cell_text(row, column);
            self.picker_edits = self.picker_edits.wrapping_add(1);
        }
    }

    pub fn open_task_menu(&mut self, row: usize, x: f64, y: f64) {
        if !self.is_selected(row) {
            self.select(row);
        }
        self.context_menu = Some(ContextMenu::Task { row, x, y });
    }

    pub fn open_chart_menu(&mut self, x: f64, y: f64) {
        self.context_menu = Some(ContextMenu::Chart { x, y });
    }

    pub fn open_column_menu(&mut self, index: usize, x: f64, y: f64) {
        self.context_menu = Some(ContextMenu::Column { index, x, y });
    }

    pub fn open_resource_menu(&mut self, index: usize, x: f64, y: f64) {
        self.selected_resource = Some(index);
        self.context_menu = Some(ContextMenu::Resource { index, x, y });
    }

    pub fn close_menu(&mut self) {
        self.context_menu = None;
    }

    pub fn note(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    /// Push overbooked work later until nobody is asked for more hours than
    /// they have. The scope decides how much of the plan is fair game.
    pub fn level(&mut self, scope: aop_core::leveling::LevelScope) {
        // The vocabulary has one levelling command and it levels the whole
        // plan. Writing it down for a narrower run would put a line in the log
        // that replays as something the planner did not ask for, which is
        // worse than an entry that is not there.
        if scope == aop_core::leveling::LevelScope::EntireProject {
            self.record(Cmd::Level {});
        }
        self.checkpoint();
        let options = aop_core::leveling::LevelingOptions {
            scope,
            ..self.leveling.clone()
        };
        let result = aop_core::leveling::level(&mut self.project, &options);
        self.reschedule();
        self.dirty = true;

        self.status = if result.delayed.is_empty() {
            if result.remaining == 0 {
                "Nothing to level. Nobody is overbooked.".to_string()
            } else {
                // Worth saying plainly: silence here reads as a broken button.
                format!(
                    "Could not level {} overallocation{} without breaking the schedule.",
                    result.remaining,
                    if result.remaining == 1 { "" } else { "s" }
                )
            }
        } else {
            let moved = result.delayed.len();
            let mut message = format!(
                "Levelled {moved} task{}, clearing {} overallocation{}.",
                if moved == 1 { "" } else { "s" },
                result.resolved,
                if result.resolved == 1 { "" } else { "s" }
            );
            if result.remaining > 0 {
                message.push_str(&format!(" {} still overbooked.", result.remaining));
            }
            message
        };
    }

    /// Copy the top selected row's value down the rest of the selection.
    pub fn fill_down(&mut self) {
        let Some(field) = self.fill_field else {
            self.note("Click a cell first so Fill Down knows which column to fill.");
            return;
        };
        if !aop_core::grouping::is_fillable(field) {
            self.note(format!("{} cannot be filled down.", field.label()));
            return;
        }
        let mut rows = self.selection.clone();
        if rows.len() < 2 {
            self.note("Select the cell to copy and the rows to copy it into.");
            return;
        }
        rows.sort_unstable();

        self.checkpoint();
        let filled = aop_core::grouping::fill_down(&mut self.project, field, &rows);
        if filled == 0 {
            self.roll_back();
            self.note("Nothing to fill. Those rows already match.");
            return;
        }
        // Written once something has actually moved. An entry saying a column
        // was filled when every row already matched would be a line the log
        // can never take back.
        self.record(Cmd::FillDown { field });
        self.reschedule();
        self.dirty = true;
        self.note(format!(
            "Filled {} down {filled} row{}.",
            field.label(),
            if filled == 1 { "" } else { "s" }
        ));
    }

    /// Bring another plan in as a summary row and its children.
    ///
    /// A snapshot, not a live link: the rows become part of this plan, so
    /// saving here never writes to somebody else's file.
    pub fn insert_subproject(&mut self, path: PathBuf) {
        let at = self.selection.first().copied().unwrap_or(self.project.tasks.len());
        self.checkpoint();
        match aop_core::subproject::insert(&mut self.project, &path, at) {
            Ok(inserted) => {
                self.reschedule();
                self.dirty = true;
                self.selection = vec![inserted.summary_row];
                self.dialog = None;
                self.status = format!(
                    "Inserted {} task{} and {} link{} from {}.",
                    inserted.task_count,
                    if inserted.task_count == 1 { "" } else { "s" },
                    inserted.link_count,
                    if inserted.link_count == 1 { "" } else { "s" },
                    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                );
            }
            Err(error) => {
                self.roll_back();
                self.dialog = Some(Dialog::Message {
                    title: "Insert Subproject".to_string(),
                    body: error.to_string(),
                });
            }
        }
    }

    /// Turn one of the gridline rules on or off, and remember it.
    pub fn toggle_gridline(&mut self, which: &str) {
        let on = match which {
            "rows" => {
                self.grid_rows = !self.grid_rows;
                self.grid_rows
            }
            "columns" => {
                self.grid_columns = !self.grid_columns;
                self.grid_columns
            }
            "status" => {
                self.grid_status_date = !self.grid_status_date;
                self.grid_status_date
            }
            _ => return,
        };
        self.settings().save();
        self.note(format!(
            "{} gridlines {}.",
            match which {
                "rows" => "Row",
                "columns" => "Column",
                _ => "Status date",
            },
            if on { "shown" } else { "hidden" }
        ));
    }

    /// Roll progress forward, or push what has not happened yet past a date.
    pub fn update_project(&mut self, options: aop_core::update::UpdateOptions) {
        self.checkpoint();
        match aop_core::update::update_project(&mut self.project, &options) {
            Ok(summary) => {
                if summary.changed() == 0 {
                    // Nothing moved, so the checkpoint would be an empty step
                    // in the undo history.
                    self.roll_back();
                }
                self.dirty = summary.changed() > 0;
                self.dialog = None;
                self.status = summary.describe();
                self.reschedule();
            }
            Err(error) => {
                self.roll_back();
                self.dialog = Some(Dialog::Message {
                    title: "Update Project".to_string(),
                    body: error.to_string(),
                });
            }
        }
    }

    /// Turn bold, italic or underline on or off across the selection.
    pub fn toggle_emphasis(&mut self, mark: aop_core::textstyle::Emphasis) {
        if self.selection.is_empty() {
            self.note("Select the rows to format first.");
            return;
        }
        let rows = self.selection.clone();
        self.checkpoint();
        let on = aop_core::textstyle::toggle_emphasis(&mut self.project, &rows, mark);
        self.dirty = true;
        let name = match mark {
            aop_core::textstyle::Emphasis::Bold => "Bold",
            aop_core::textstyle::Emphasis::Italic => "Italic",
            aop_core::textstyle::Emphasis::Underline => "Underline",
        };
        self.note(format!(
            "{name} {} for {} row{}.",
            if on { "on" } else { "off" },
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ));
    }

    /// Put a font family or a size on the selected rows.
    ///
    /// An empty family or a zero size hands the row back to the theme, which
    /// is how a row says it has no opinion.
    pub fn set_row_font(&mut self, family: Option<String>, size_pt: Option<f32>) {
        if self.selection.is_empty() {
            self.note("Select the rows to format first.");
            return;
        }
        let rows = self.selection.clone();
        self.checkpoint();
        for index in &rows {
            if let Some(task) = self.project.tasks.get_mut(*index) {
                if let Some(family) = &family {
                    task.font_family = family.clone();
                }
                if let Some(size) = size_pt {
                    task.font_size_pt = size;
                }
            }
        }
        self.dirty = true;
        self.note(match (&family, size_pt) {
            (Some(family), _) => format!("Font set to {family}."),
            (None, Some(size)) => format!("Font size set to {size:.0}."),
            _ => "Font unchanged.".to_string(),
        });
    }

    /// Copy the look of the first selected row, ready to brush onto others.
    pub fn pick_up_format(&mut self) {
        let Some(row) = self.primary() else {
            self.note("Select a row to copy formatting from.");
            return;
        };
        let painter = aop_core::textstyle::pick_up(&self.text_styles, &self.project, row);
        self.painter = Some(painter);
        self.note("Format picked up. Select the rows to paint, then click Format Painter again.");
    }

    /// Brush the picked-up look onto the selection.
    pub fn brush_format(&mut self) {
        let Some(painter) = self.painter.clone() else {
            self.pick_up_format();
            return;
        };
        if self.selection.is_empty() {
            self.note("Select the rows to paint first.");
            return;
        }
        let rows = self.selection.clone();
        self.checkpoint();
        for index in &rows {
            painter.brush(&mut self.project, *index);
        }
        self.painter = None;
        self.dirty = true;
        self.note(format!(
            "Painted {} row{}.",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ));
    }

    /// Take back the delays levelling put in.
    pub fn clear_leveling(&mut self) {
        self.record(Cmd::ClearLeveling {});
        self.checkpoint();
        let cleared = aop_core::leveling::clear_leveling(&mut self.project);
        self.reschedule();
        self.dirty = true;
        self.status = if cleared == 0 {
            "No levelling delays to clear.".to_string()
        } else {
            format!(
                "Cleared levelling on {cleared} task{}.",
                if cleared == 1 { "" } else { "s" }
            )
        };
    }

    /// Placeholder for ribbon commands that are present but not yet wired up.
    pub fn not_implemented(&mut self, command: &str) {
        self.status = format!("{command} is not available in this build");
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Start with whatever file the command line names.
///
/// This is how a desktop file association arrives: the system runs the binary
/// with the document as its argument. Without it, double-clicking a plan opens
/// an empty window, and the association looks broken even though it fired.
pub fn from_command_line() -> AppState {
    let mut state = AppState::new();
    state.apply_settings(crate::settings::Settings::load());
    // Reads the token store and nothing else, so start up is never held up by
    // a server that is slow or absent. A session that has since been ended
    // shows up on the first thing that uses it, which is a better moment to
    // hear about it than during a splash screen.
    if state.collaborate {
        state.restore_session();
    }
    // Worked out before anything else can write a preference, since the answer
    // depends on the version this copy last ran as and that is recorded here.
    state.begin_greetings();
    // One argument, and what it is decides what happens to it. A link is told
    // from a path by its scheme rather than by guessing, because a guess about
    // this is a guess about whether a network request is made.
    match std::env::args().nth(1) {
        Some(argument) if crate::cloud::share::looks_like_a_link(&argument) => {
            state.splash = false;
            state.open_link_asked(&argument);
        }
        Some(argument) => {
            let path = PathBuf::from(argument);
            if path.is_file() {
                state.splash = false;
                state.open_any(path);
            }
        }
        None => {}
    }
    state
}

/// Sort the sibling blocks that start at `level` within `start..end`, sorting
/// each block's own children the same way.
fn sort_range(project: &Project, start: usize, end: usize, level: u16, key: &str) -> Vec<Task> {
    // Split the range into blocks, each a sibling plus its descendants.
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let mut next = cursor + 1;
        while next < end && project.tasks[next].outline_level > level {
            next += 1;
        }
        blocks.push((cursor, next));
        cursor = next;
    }

    let sort_key = |index: usize| -> (i64, String) {
        let task = &project.tasks[index];
        let value = match key {
            "start" => task.scheduled.start.and_utc().timestamp(),
            "finish" => task.scheduled.finish.and_utc().timestamp(),
            "duration" => task.scheduled.duration_minutes,
            "cost" => (task.scheduled.cost * 100.0) as i64,
            _ => 0,
        };
        (value, task.name.to_lowercase())
    };

    blocks.sort_by(|a, b| sort_key(a.0).cmp(&sort_key(b.0)));

    let mut out = Vec::with_capacity(end - start);
    for (head, tail) in blocks {
        out.push(project.tasks[head].clone());
        if tail > head + 1 {
            out.extend(sort_range(project, head + 1, tail, level + 1, key));
        }
    }
    out
}

// ---- what the change log says -------------------------------------------

/// Whether a command only moves the selection about.
fn is_selecting(cmd: &Cmd) -> bool {
    matches!(
        cmd,
        Cmd::SelectRow { .. }
            | Cmd::SelectRows { .. }
            | Cmd::ToggleRow { .. }
            | Cmd::SelectAll {}
            | Cmd::ClearSelection {}
    )
}

/// Whether an entry is itself an undo or a redo, which is what stops the two
/// of them naming each other's work in turn.
fn is_undo_entry(change: &aop_core::history::Change) -> bool {
    let last = change.script.lines().next_back().unwrap_or("").trim();
    last == "undo();" || last == "redo();"
}

/// "1 task" or "4 tasks", so a sentence can be built without a stray plural.
fn tasks_phrase(count: usize) -> String {
    match count {
        1 => "1 task".to_string(),
        other => format!("{other} tasks"),
    }
}

fn rows_phrase(count: usize) -> String {
    match count {
        1 => "1 row".to_string(),
        other => format!("{other} rows"),
    }
}

/// The field a grid column types into. The inverse of the mapping the macro
/// vocabulary keeps, and the reason a typed cell can be written as a command.
fn field_of(column: Column) -> Field {
    match column {
        Column::Name => Field::Name,
        Column::Duration => Field::Duration,
        Column::Start => Field::Start,
        Column::Finish => Field::Finish,
        Column::Predecessors => Field::Predecessors,
        Column::Resources => Field::ResourceNames,
    }
}

/// The resource sheet column a key names, for the commands that carry one.
fn resource_field_of(key: &str) -> Option<ResourceField> {
    Some(match key {
        "name" => ResourceField::Name,
        "initials" => ResourceField::Initials,
        "group" => ResourceField::Group,
        "max" => ResourceField::MaxUnits,
        "rate" => ResourceField::Rate,
        "kind" => ResourceField::Kind,
        _ => return None,
    })
}

fn resource_field_label(field: ResourceField) -> &'static str {
    match field {
        ResourceField::Name => "Name",
        ResourceField::Initials => "Initials",
        ResourceField::Group => "Group",
        ResourceField::MaxUnits => "Max Units",
        ResourceField::Rate => "Rate",
        ResourceField::Kind => "Type",
    }
}

fn view_option_label(option: ViewOption) -> &'static str {
    match option {
        ViewOption::CriticalPath => "the critical path",
        ViewOption::Timeline => "the timeline",
        ViewOption::OutlineNumber => "outline numbers",
        ViewOption::Slack => "slack",
        ViewOption::Baseline => "the baseline",
        ViewOption::Links => "link lines",
        ViewOption::BarText => "bar text",
        ViewOption::RoundBars => "rounded bars",
    }
}

/// The field a sort key names, so a sort can be written as a command.
fn sort_field_of(key: &str) -> Option<Field> {
    Some(match key {
        "start" => Field::Start,
        "finish" => Field::Finish,
        "duration" => Field::Duration,
        "cost" => Field::Cost,
        "name" => Field::Name,
        _ => return None,
    })
}

/// One short sentence for a command, in the words a planner would use.
///
/// Worked out before the command runs, because half of these read the plan as
/// it was: how many rows were selected, whether a task was active, who was
/// already booked. The match is exhaustive so a new command cannot arrive
/// without somebody deciding what the panel should say about it.
fn describe(cmd: &Cmd, state: &AppState) -> String {
    let rows = state.selection.len();
    match cmd {
        Cmd::SelectRow { row } => format!("Selected row {}", row.0),
        Cmd::SelectRows { from, to } => format!("Selected rows {} to {}", from.0, to.0),
        Cmd::ToggleRow { row } => {
            let already = (row.0 as usize)
                .checked_sub(1)
                .is_some_and(|index| state.is_selected(index));
            if already {
                format!("Took row {} out of the selection", row.0)
            } else {
                format!("Added row {} to the selection", row.0)
            }
        }
        Cmd::SelectAll {} => "Selected every row".to_string(),
        Cmd::ClearSelection {} => "Selected nothing".to_string(),
        Cmd::InsertTask {} => "Inserted a task".to_string(),
        Cmd::InsertMilestone {} => "Inserted a milestone".to_string(),
        Cmd::InsertSummary {} => "Inserted a summary task".to_string(),
        Cmd::AppendTask { name } => match name.trim() {
            "" => "Added a task at the end".to_string(),
            named => format!("Added the task {named}"),
        },
        Cmd::DeleteTasks {} => format!("Deleted {}", tasks_phrase(rows)),
        Cmd::Indent {} => format!("Indented {}", tasks_phrase(rows)),
        Cmd::Outdent {} => format!("Outdented {}", tasks_phrase(rows)),
        Cmd::MoveUp {} => "Moved a task up".to_string(),
        Cmd::MoveDown {} => "Moved a task down".to_string(),
        Cmd::CopyTasks {} => format!("Copied {}", tasks_phrase(rows)),
        Cmd::CutTasks {} => format!("Cut {}", tasks_phrase(rows)),
        Cmd::PasteTasks {} => "Pasted from the clipboard".to_string(),
        Cmd::ExpandAll {} => "Opened every summary row".to_string(),
        Cmd::CollapseAll {} => "Closed every summary row".to_string(),
        Cmd::Link {} => format!("Linked {}", tasks_phrase(rows)),
        Cmd::Unlink {} => format!("Unlinked {}", tasks_phrase(rows)),
        Cmd::SetLink {
            row,
            predecessor,
            kind,
            ..
        } => format!(
            "Linked row {} to row {} ({})",
            predecessor.0,
            row.0,
            kind.code()
        ),
        Cmd::RemoveLink { row, predecessor } => format!(
            "Took the link from row {} off row {}",
            predecessor.0, row.0
        ),
        Cmd::SetField { row, field, value } => match value.trim() {
            "" => format!("Cleared {} on row {}", field.label(), row.0),
            typed => format!("Set {} on row {} to {typed}", field.label(), row.0),
        },
        Cmd::SetPercentComplete { percent } => {
            format!("Marked {} {percent}% complete", tasks_phrase(rows))
        }
        Cmd::SetTaskMode { mode } => format!(
            "Set {} to {} scheduling",
            tasks_phrase(rows),
            match mode {
                TaskMode::Manual => "manual",
                TaskMode::Auto => "automatic",
            }
        ),
        Cmd::ToggleActive {} => {
            let was_active = state
                .primary()
                .and_then(|row| state.project.tasks.get(row))
                .is_some_and(|task| task.active);
            format!(
                "Made {} {}",
                tasks_phrase(rows),
                if was_active { "inactive" } else { "active" }
            )
        }
        Cmd::RespectLinks {} => format!(
            "Released {} back to auto scheduling",
            tasks_phrase(rows)
        ),
        Cmd::FillDown { field } => {
            format!("Filled {} down {}", field.label(), rows_phrase(rows))
        }
        Cmd::AddResource { name } => format!("Added {name} to the resource sheet"),
        Cmd::DeleteResource { resource_row } => {
            let named = (resource_row.0 as usize)
                .checked_sub(1)
                .and_then(|index| state.project.resources.get(index))
                .map(|resource| resource.name.clone())
                .unwrap_or_else(|| format!("row {}", resource_row.0));
            format!("Took {named} off the resource sheet")
        }
        Cmd::AssignResource {
            row,
            name,
            units_percent,
        } => format!("Booked {name} onto row {} at {units_percent:.0}%", row.0),
        Cmd::SetAssignmentUnits {
            row,
            name,
            units_percent,
        } => format!("Changed {name} on row {} to {units_percent:.0}%", row.0),
        Cmd::UnassignResource { row, name } => format!("Took {name} off row {}", row.0),
        Cmd::SetResourceField {
            resource_row,
            field,
            value,
        } => format!(
            "Set {} on resource row {} to {value}",
            resource_field_label(*field),
            resource_row.0
        ),
        Cmd::SetView { view } => format!("Switched to the {} view", view.label()),
        Cmd::SetZoom { zoom } => format!("Zoomed to {}", zoom.label()),
        Cmd::ZoomToFit {} => "Zoomed to fit the whole plan".to_string(),
        Cmd::SetFilter { filter } => format!("Filtered to {}", filter.label()),
        Cmd::GroupBy { field } => match field {
            Some(field) => format!("Grouped by {}", field.label()),
            None => "Took the grouping off".to_string(),
        },
        Cmd::SortBy { field } => format!("Sorted by {}", field.label()),
        Cmd::ShowColumn { field, at } => {
            format!("Showed the {} column at position {}", field.label(), at.0)
        }
        Cmd::HideColumn { field } => format!("Hid the {} column", field.label()),
        Cmd::ResetColumns {} => "Put the Entry columns back".to_string(),
        Cmd::SetViewOption { option, on } => format!(
            "Turned {} {}",
            view_option_label(*option),
            if *on { "on" } else { "off" }
        ),
        Cmd::SetBaseline {} => "Saved the baseline".to_string(),
        Cmd::ClearBaseline {} => "Cleared the baseline".to_string(),
        Cmd::SetProjectStart { date } => {
            format!("Moved the plan to start on {}", format_date(*date))
        }
        Cmd::Level {} => "Levelled overbooked work".to_string(),
        Cmd::ClearLeveling {} => "Cleared the levelling delays".to_string(),
        // Both of these are recorded through `record_as`, which knows which
        // step was moved and says so. This is only the fallback.
        Cmd::Undo {} => "Undid the last change".to_string(),
        Cmd::Redo {} => "Redid the last change".to_string(),
        Cmd::Note { message } => format!("Said: {message}"),
    }
}

/// One sentence for a run of commands written as a single entry.
fn describe_run(held: &[(Cmd, String)]) -> String {
    let edits: Vec<&(Cmd, String)> = held
        .iter()
        .filter(|(cmd, _)| !is_selecting(cmd))
        .collect();
    let Some((last, summary)) = edits.last() else {
        // Nothing but selecting, so the selecting is what it says.
        return held
            .last()
            .map(|(_, summary)| summary.clone())
            .unwrap_or_default();
    };
    if edits.len() == 1 {
        return summary.clone();
    }
    // The same command over a run of rows is one thing the planner did, so it
    // is named once with a count rather than listed line by line.
    if edits.iter().all(|(cmd, _)| cmd.fn_name() == last.fn_name())
        && let Cmd::SetField { field, .. } = last
    {
        return format!("Set {} on {}", field.label(), rows_phrase(edits.len()));
    }
    format!("{} changes in one step", edits.len())
}

fn empty_report(start: NaiveDateTime) -> ScheduleReport {
    ScheduleReport {
        start,
        finish: start,
        duration_minutes: 0,
        critical_task_count: 0,
        total_cost: 0.0,
        total_work_minutes: 0,
        overallocations: Vec::new(),
    }
}

/// A new plan starts today, at the start of the working day. A plan created on
/// a weekend or a holiday rolls on to the next working morning, because the
/// scheduler has nowhere to put work otherwise.
pub fn default_start() -> NaiveDateTime {
    let today = Local::now().naive_local().date();
    let morning = today.and_hms_opt(8, 0, 0).expect("valid time");
    WorkCalendar::standard().next_working_instant(morning)
}

/// The next Monday on or after a date, used by the template previews so they
/// all line up on a tidy week boundary.
pub fn next_monday(from: NaiveDate) -> NaiveDate {
    let mut date = from;
    while date.weekday() != chrono::Weekday::Mon {
        date += chrono::Duration::days(1);
    }
    date
}

/// Accepts `2026-08-17`, `17/08/2026` and `08/17/2026`, with optional time.
pub fn parse_date(input: &str) -> Option<NaiveDateTime> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    for format in ["%Y-%m-%d %H:%M", "%d/%m/%Y %H:%M", "%m/%d/%Y %H:%M"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(text, format) {
            return Some(value);
        }
    }
    for format in ["%Y-%m-%d", "%d/%m/%Y", "%m/%d/%Y", "%d %b %y", "%d %B %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(text, format) {
            return date.and_hms_opt(8, 0, 0);
        }
    }
    None
}

/// The date styles offered on the Display options page.
pub const DATE_FORMATS: [(&str, &str); 5] = [
    ("Mon 17/08/26", "%a %d/%m/%y"),
    ("17/08/2026", "%d/%m/%Y"),
    ("Mon 17 Aug '26", "%a %d %b '%y"),
    ("17 August 2026", "%d %B %Y"),
    ("2026-08-17", "%Y-%m-%d"),
];

/// The chosen format, held globally so the many `format_date` call sites do not
/// each have to be handed the application state.
static DATE_FORMAT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_date_format(index: usize) {
    DATE_FORMAT.store(
        index.min(DATE_FORMATS.len() - 1),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub fn format_date(value: NaiveDateTime) -> String {
    let index = DATE_FORMAT.load(std::sync::atomic::Ordering::Relaxed);
    let pattern = DATE_FORMATS
        .get(index)
        .map(|f| f.1)
        .unwrap_or(DATE_FORMATS[0].1);
    value.format(pattern).to_string()
}

pub fn format_date_long(value: NaiveDateTime) -> String {
    value.format("%d %B %Y").to_string()
}

// ---- recent file list ---------------------------------------------------

fn recent_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("alterion-open-project").join("recent.json"))
}

fn load_recent() -> Vec<RecentEntry> {
    let Some(path) = recent_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(paths) = serde_json::from_str::<Vec<String>>(&text) else {
        return Vec::new();
    };
    paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .map(|path| RecentEntry {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Project".into()),
            path,
        })
        .collect()
}

fn save_recent(entries: &[RecentEntry]) {
    let Some(path) = recent_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let paths: Vec<String> = entries
        .iter()
        .map(|e| e.path.to_string_lossy().to_string())
        .collect();
    if let Ok(text) = serde_json::to_string_pretty(&paths) {
        let _ = std::fs::write(path, text);
    }
}

fn qat_path() -> Option<PathBuf> {
    recent_path().map(|p| p.with_file_name("quick-access.json"))
}

fn load_qat() -> Vec<QatCommand> {
    let Some(path) = qat_path() else {
        return DEFAULT_QAT.to_vec();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return DEFAULT_QAT.to_vec();
    };
    let Ok(keys) = serde_json::from_str::<Vec<String>>(&text) else {
        return DEFAULT_QAT.to_vec();
    };
    let restored: Vec<QatCommand> = keys
        .iter()
        .filter_map(|k| QatCommand::from_key(k))
        .collect();
    if restored.is_empty() {
        DEFAULT_QAT.to_vec()
    } else {
        restored
    }
}

fn save_qat(commands: &[QatCommand]) {
    let Some(path) = qat_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let keys: Vec<&str> = commands.iter().map(|c| c.key()).collect();
    if let Ok(text) = serde_json::to_string_pretty(&keys) {
        let _ = std::fs::write(path, text);
    }
}

/// The default folder the Save As and Open panes start in.
pub fn documents_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Documents"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Rebuild each band's count and totals from the rows still under it.
///
/// Grouping runs before filtering, so once rows are dropped the numbers the
/// bands were built with no longer describe what is on screen.
fn restate_bands(project: &aop_core::Project, rows: &mut [GroupRow]) {
    let depth_of = |row: &GroupRow| match row {
        GroupRow::Band { depth, .. } => *depth,
        GroupRow::Task(_) => usize::MAX,
    };

    for at in 0..rows.len() {
        let here = depth_of(&rows[at]);
        if here == usize::MAX {
            continue;
        }
        let mut count = 0usize;
        let mut work = 0i64;
        let mut cost = 0.0f64;
        for row in rows.iter().skip(at + 1) {
            match row {
                GroupRow::Task(index) => {
                    count += 1;
                    if let Some(task) = project.tasks.get(*index) {
                        work += task.scheduled.work_minutes;
                        cost += task.scheduled.cost;
                    }
                }
                // A deeper band still sits inside this one, so keep counting.
                GroupRow::Band { depth, .. } if *depth > here => {}
                GroupRow::Band { .. } => break,
            }
        }
        if let GroupRow::Band {
            count: c,
            work_minutes: w,
            cost: k,
            ..
        } = &mut rows[at]
        {
            *c = count;
            *w = work;
            *k = cost;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use aop_core::MINUTES_PER_DAY;

    /// A four row outline: Phase / Child A / Child B / Standalone.
    fn outlined() -> AppState {
        let mut state = AppState::new();
        state.project.tasks.clear();
        state.project.links.clear();
        for name in ["Phase", "Child A", "Child B", "Standalone"] {
            state.project.push_task(name, MINUTES_PER_DAY);
        }
        state.project.tasks[1].outline_level = 1;
        state.project.tasks[2].outline_level = 1;
        state.reschedule();
        state
    }

    /// A plan built somewhere else, of the shape an import produces.
    fn brought_in() -> Project {
        let mut project = Project::blank(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .and_then(|date| date.and_hms_opt(8, 0, 0))
                .expect("a valid morning"),
        );
        project.tasks.clear();
        project.name = "From a spreadsheet".into();
        project.push_task("Survey the site", MINUTES_PER_DAY);
        project
    }

    #[test]
    fn an_import_waits_while_unsaved_work_is_asked_about() {
        // Nothing is imported until the person says so, and a plan with
        // unsaved work in it must not be replaced behind their back.
        let mut state = outlined();
        state.dirty = true;
        state.stage_import(brought_in(), std::path::PathBuf::from("plan.xlsx"), "note".into());

        assert_eq!(names(&state).len(), 4, "the open plan is untouched");
        assert!(matches!(state.dialog, Some(Dialog::UnsavedChanges(_))));
        assert!(state.pending_import.is_some(), "the built plan is waiting");

        state.carry_out(PendingAction::AdoptImport);
        assert_eq!(names(&state), vec!["Survey the site"]);
        assert!(state.pending_import.is_none());
        assert!(state.file_path.is_none(), "a workbook is not somewhere to save to");
    }

    #[test]
    fn an_import_into_a_saved_plan_goes_straight_in() {
        let mut state = outlined();
        state.dirty = false;
        state.stage_import(brought_in(), std::path::PathBuf::from("plan.xlsx"), "note".into());
        assert_eq!(names(&state), vec!["Survey the site"]);
        assert_eq!(state.status, "note");
    }

    #[test]
    fn holidays_go_in_once_and_can_be_undone() {
        let mut state = outlined();
        let holidays = vec![aop_core::holidays::Holiday {
            name: "Christmas Day".into(),
            from: chrono::NaiveDate::from_ymd_opt(2026, 12, 25).expect("a date"),
            to: chrono::NaiveDate::from_ymd_opt(2026, 12, 25).expect("a date"),
            repeating: false,
        }];
        assert_eq!(state.import_holidays(&CalendarTarget::Project, &holidays), 1);
        // A second pass adds nothing, and must not leave an empty step in the
        // undo stack for somebody to press through.
        let steps = state.undo.len();
        assert_eq!(state.import_holidays(&CalendarTarget::Project, &holidays), 0);
        assert_eq!(state.undo.len(), steps);
        assert_eq!(state.project.calendar.exceptions.len(), 1);
    }

    #[test]
    fn importing_into_a_person_lands_on_them_and_not_on_anybody_else() {
        // The mistake this guards against is silent: a national holiday file
        // dropped onto one person leaves the plan scheduling everybody else
        // straight through Christmas.
        let mut state = outlined();
        let ada = state.project.add_resource("Ada Lovelace");
        let leave = vec![aop_core::holidays::Holiday {
            name: "Away".into(),
            from: chrono::NaiveDate::from_ymd_opt(2026, 3, 3).expect("a date"),
            to: chrono::NaiveDate::from_ymd_opt(2026, 3, 6).expect("a date"),
            repeating: false,
        }];

        assert_eq!(
            state.import_holidays(&CalendarTarget::Resource(ada), &leave),
            1
        );
        let resource = state.project.resource(ada).expect("still there");
        assert_eq!(resource.calendar_exceptions.len(), 1);
        assert!(
            state.project.calendar.exceptions.is_empty(),
            "their leave is theirs, not everybody's"
        );

        // And a second run over the same person adds nothing, so an import
        // repeated by accident cannot double the days up.
        let steps = state.undo.len();
        assert_eq!(
            state.import_holidays(&CalendarTarget::Resource(ada), &leave),
            0
        );
        assert_eq!(state.undo.len(), steps);
    }

    #[test]
    fn a_holiday_file_is_not_offered_where_a_plan_is_opened() {
        // The Open page opens plans. An .ics is a list of days off, and
        // offering it there would be offering to open something that is not a
        // plan; it belongs to the import inside Change Working Time.
        assert!(!offered_in_browser(
            &std::path::PathBuf::from("holidays.ics"),
            false
        ));
        assert!(!offered_in_browser(
            &std::path::PathBuf::from("HOLIDAYS.ICS"),
            false
        ));
    }

    fn names(state: &AppState) -> Vec<&str> {
        state.project.tasks.iter().map(|t| t.name.as_str()).collect()
    }

    fn levels(state: &AppState) -> Vec<u16> {
        state.project.tasks.iter().map(|t| t.outline_level).collect()
    }

    // ---- drawings -------------------------------------------------------

    /// Where the first task's bar sits on the chart, in body coordinates.
    fn first_bar(state: &AppState) -> (f64, f64) {
        let scale = state.chart_scale();
        let (left, right) =
            bar_edges(&state.project, &scale, state.round_bars, 0).expect("row zero has a bar");
        ((left + right) / 2.0, ROW_H / 2.0)
    }

    #[test]
    fn a_pickers_change_is_not_written_back_over_by_the_cells_text() {
        // The cell and the picker edit the same thing from two places. Ticking
        // a box changes the plan; committing the cell's text afterwards used to
        // put the old value back, so the tick appeared to do nothing.
        let mut state = AppState::new();
        state.append_task("A");
        state.append_task("B");
        state.add_resource("Ada");

        let resource = state.project.resources[0].id;
        state.edit_cell_at(1, Column::Resources, 0.0, 0.0);
        let seeded = state.cell_draft.clone();
        assert!(seeded.is_empty(), "nothing is booked yet");

        state.set_assignment(1, resource, Some(1.0));
        state.refresh_cell_draft();
        assert!(
            state.cell_draft.contains("Ada"),
            "the cell has to catch up with the picker, got {:?}",
            state.cell_draft
        );

        // What blur then compares against: identical, so it must not commit.
        assert_eq!(
            state.cell_draft,
            state.cell_text(1, Column::Resources),
            "the text now matches the plan, so there is nothing to write back"
        );
        assert_eq!(
            state.project.tasks[1].assignments.len(),
            1,
            "and the booking survives"
        );
    }

    #[test]
    fn hover_is_not_part_of_the_plans_state() {
        // It lives in its own signal on purpose. Held on AppState, moving the
        // pointer across the chart invalidated the layout memo per bar, which
        // rebuilds a tick for every day of the plan to move a highlight.
        let state = AppState::new();
        let _ = state;
        // Nothing to assert on AppState itself, which is the point: the field
        // is gone. The behaviour is covered where it is used.
    }


    // ---- the change log -------------------------------------------------

    /// A plan with a name behind the keyboard, so the entries have an author.
    fn worked_on_by(who: &str) -> AppState {
        let mut state = AppState::new();
        state.user_name = who.to_string();
        for name in ["Phase", "Design", "Build", "Ship"] {
            state.append_task(name);
        }
        state
    }

    fn log_of(state: &AppState) -> Vec<(String, String)> {
        state
            .project
            .history
            .changes()
            .iter()
            .map(|change| (change.summary.clone(), change.script.clone()))
            .collect()
    }

    #[test]
    fn one_edit_is_one_entry_signed_by_whoever_made_it() {
        let mut state = worked_on_by("Ada Lovelace");
        let before = state.project.history.len();

        state.select(1);
        state.indent_selected();

        let log = state.project.history.changes();
        assert_eq!(log.len(), before + 1, "one edit, one entry");
        let entry = log.last().expect("the edit just recorded one");
        assert_eq!(entry.author, "Ada Lovelace");
        assert_eq!(entry.summary, "Indented 1 task");
        assert!(entry.script.contains("indent();"), "{}", entry.script);
    }

    /// Somebody signed in, without a session: the account is kept beside the
    /// session precisely so the interface can still say who this is while a
    /// worker has the session, and that is the state to test against.
    fn signed_in_as(name: &str) -> crate::cloud::Account {
        crate::cloud::Account {
            subject: "0198f0c2-0000-7000-8000-000000000000".into(),
            name: name.into(),
            email: "ada@example.org".into(),
            picture: None,
        }
    }

    #[test]
    fn the_name_other_people_see_is_the_one_the_server_knows() {
        // The shared log is the point. A local name that disagrees with the
        // account is a name only this machine ever sees.
        let mut state = worked_on_by("ada-laptop");
        state.account = Some(signed_in_as("Ada Lovelace"));
        assert_eq!(state.display_name(), "Ada Lovelace");

        state.select(1);
        state.indent_selected();
        let entry = state
            .project
            .history
            .changes()
            .last()
            .expect("the edit just recorded one");
        assert_eq!(entry.author, "Ada Lovelace");
    }

    #[test]
    fn signing_out_leaves_no_trace_of_the_account_name() {
        // What was typed here is read, never overwritten, which is the whole
        // reason signing out can put it straight back.
        let mut state = worked_on_by("ada-laptop");
        state.account = Some(signed_in_as("Ada Lovelace"));
        assert_eq!(state.display_name(), "Ada Lovelace");

        state.sign_out_landed(Ok(()));
        assert_eq!(state.user_name, "ada-laptop");
        assert_eq!(state.display_name(), "ada-laptop");

        state.select(1);
        state.indent_selected();
        let entry = state
            .project
            .history
            .changes()
            .last()
            .expect("the edit just recorded one");
        assert_eq!(entry.author, "ada-laptop");
    }

    #[test]
    fn an_account_with_nothing_to_show_falls_back_to_what_was_typed() {
        // A name is never empty coming from the server, but nothing about the
        // interface should depend on that being true.
        let mut state = worked_on_by("ada-laptop");
        state.account = Some(signed_in_as("   "));
        assert_eq!(state.display_name(), "ada-laptop");
    }

    #[test]
    fn work_done_before_anybody_says_who_they_are_is_signed_honestly() {
        // A fresh install has no name in it. "Unknown" is true; putting the
        // machine's account name in would be a guess written into a record.
        let mut state = AppState::new();
        state.append_task("Phase");
        let entry = state
            .project
            .history
            .changes()
            .last()
            .expect("appending a task is a change");
        assert_eq!(entry.author, "Unknown");
    }

    #[test]
    fn a_grouped_run_is_one_entry_and_not_one_for_every_command() {
        // A fill down over four rows is one thing the planner did. An entry per
        // command would bury the day's real work under its own mechanics.
        let mut state = worked_on_by("Ada");
        let before = state.project.history.len();

        state.as_one_step(|s| {
            for row in 0..s.project.tasks.len() {
                s.select(row);
                s.set_percent_complete(50);
            }
        });

        let log = state.project.history.changes();
        assert_eq!(log.len(), before + 1, "the run is one entry");
        let entry = log.last().expect("the run recorded one");
        assert_eq!(
            entry.command_count(),
            8,
            "and it keeps every command it stands for, so it can be replayed"
        );
        assert_eq!(entry.summary, "4 changes in one step");
    }

    #[test]
    fn a_run_of_selecting_is_one_act_of_selecting() {
        let mut state = worked_on_by("Ada");
        let before = state.project.history.len();

        state.select(1);
        state.select(2);
        state.select(3);
        state.indent_selected();

        let log = state.project.history.changes();
        assert_eq!(log.len(), before + 1);
        let entry = log.last().expect("the indent recorded one");
        assert_eq!(
            entry.command_count(),
            2,
            "row 2, then 3, then 4 is one act of selecting: {}",
            entry.script
        );
        assert!(
            entry.script.starts_with("select_row(4);"),
            "the last of the run is the one that stands: {}",
            entry.script
        );
    }

    #[test]
    fn a_summary_reads_as_a_sentence_rather_than_as_the_command_again() {
        let mut state = worked_on_by("Ada");
        state.select(0);
        state.set_percent_complete(50);
        state.add_resource("Ada");
        state.select(2);
        state.indent_selected();

        for (summary, script) in log_of(&state) {
            assert!(!summary.is_empty(), "{script} has nothing said about it");
            assert!(
                summary.starts_with(|first: char| first.is_uppercase()),
                "{summary} does not start a sentence"
            );
            assert!(!summary.ends_with('.'), "{summary} carries a full stop");
            assert!(
                !summary.contains('('),
                "{summary} is the command written out again"
            );
        }

        let said: Vec<String> = log_of(&state).into_iter().map(|(what, _)| what).collect();
        assert!(said.contains(&"Marked 1 task 50% complete".to_string()), "{said:?}");
        assert!(said.contains(&"Added Ada to the resource sheet".to_string()), "{said:?}");
    }

    #[test]
    fn an_undo_is_written_down_rather_than_rubbing_out_what_it_took_back() {
        // The log rides inside the plan, and the plan is what an undo puts
        // back, so without carrying the log across the swap an undo would
        // delete the record of the work it undid. A trail that quietly loses
        // entries is worse than no trail, because it still looks complete.
        let mut state = worked_on_by("Ada");
        state.select(1);
        state.indent_selected();
        let after_edit = state.project.history.len();

        state.undo();

        let log = state.project.history.changes();
        assert_eq!(log.len(), after_edit + 1, "the undo is a change of its own");
        assert_eq!(
            log.last().map(|change| change.summary.as_str()),
            Some("Undid: Indented 1 task"),
            "and it names the step it took back rather than itself"
        );
        assert!(
            log.iter().any(|change| change.summary == "Indented 1 task"),
            "the work that was undone is still on the record"
        );

        state.redo();
        assert_eq!(
            state
                .project
                .history
                .changes()
                .last()
                .map(|change| change.summary.as_str()),
            Some("Redid: Indented 1 task")
        );
    }

    #[test]
    fn a_command_that_rolls_itself_back_does_not_record_an_undo() {
        // Linking a loop puts the plan back by itself and says so in a dialog.
        // Nobody pressed Undo, so nothing in the log should say they did.
        let mut state = worked_on_by("Ada");
        state.set_link(1, state.project.tasks[0].id, LinkType::FS, 0);
        state.set_link(0, state.project.tasks[1].id, LinkType::FS, 0);

        assert!(
            !log_of(&state)
                .iter()
                .any(|(summary, _)| summary.starts_with("Undid")),
            "a rollback is not an undo"
        );
    }

    #[test]
    fn a_drag_records_where_it_was_let_go_and_not_the_frames_on_the_way() {
        // The one place this beats Project, whose recorder produces nothing at
        // all for a drag.
        let mut state = outlined();
        let before = state.project.history.len();

        state.begin_bar_drag(3, BarDragKind::Move, 0.0, 40.0);
        for step in 1..=20 {
            state.update_bar_drag(step as f64 * 10.0);
        }
        state.finish_bar_drag(Zoom::Days.px_per_day());

        let log = state.project.history.changes();
        assert_eq!(log.len(), before + 1, "one drag, one entry");
        let entry = log.last().expect("the drag recorded one");
        assert!(
            entry.script.contains("set_field(4, Start,"),
            "the committed result, in the vocabulary: {}",
            entry.script
        );
    }

    #[test]
    fn a_grouped_run_is_one_undo_step_and_one_schedule() {
        // Without this, a macro over a large plan clones the whole project
        // once per command and pushes the planner's real history off the end
        // of the undo stack, then runs the critical path pass just as often.
        let mut state = AppState::new();
        for name in ["One", "Two", "Three", "Four"] {
            state.append_task(name);
        }
        let before = state.project.clone();
        let depth = state.undo.len();

        state.as_one_step(|s| {
            for row in 0..s.project.tasks.len() {
                s.select(row);
                s.set_percent_complete(50);
            }
        });

        assert_eq!(
            state.undo.len(),
            depth + 1,
            "a grouped run has to cost exactly one undo step"
        );
        assert!(
            state.project.tasks.iter().all(|t| t.percent_complete == 50),
            "the work still has to happen"
        );

        state.undo();
        assert_eq!(
            state.project.tasks.iter().map(|t| t.percent_complete).collect::<Vec<_>>(),
            before.tasks.iter().map(|t| t.percent_complete).collect::<Vec<_>>(),
            "one undo has to put everything back"
        );
    }

    #[test]
    fn a_drag_that_starts_on_a_bar_anchors_to_that_task() {
        // Which is what makes an annotation follow its task through a
        // reschedule instead of staying where the plan used to be.
        let state = outlined();
        let (x, y) = first_bar(&state);
        let task = state.project.tasks[0].id;

        assert!(matches!(
            state.chart_anchor(x, y),
            Anchor::Task { task: on, .. } if on == task
        ));
    }

    #[test]
    fn a_drag_that_starts_on_bare_canvas_anchors_to_the_date() {
        let state = outlined();
        let scale = state.chart_scale();
        // Well past the end of every bar.
        let x = scale.px_per_day * 400.0;

        let Anchor::Timescale { at, row } = state.chart_anchor(x, ROW_H * 1.5) else {
            unreachable!("nothing is drawn out there to anchor to");
        };
        assert_eq!(at, scale.at_x(x), "the date under the pointer");
        assert_eq!(row, 1.5);
    }

    #[test]
    fn drawing_a_shape_costs_one_undo_step() {
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Rectangle);
        let before = state.project.clone();

        state.begin_draw(200.0, ROW_H * 1.5);
        // Several samples on the way, as a real drag delivers them.
        for step in 1..=8 {
            state.update_draw_drag(200.0 + step as f64 * 12.0, ROW_H * 1.5 + step as f64 * 2.0);
        }
        state.finish_draw_drag();

        assert_eq!(state.project.drawings.len(), 1);
        assert!(state.draw_tool.is_none(), "the tool disarms once it has drawn");
        state.undo();
        assert_eq!(state.project.drawings, before.drawings, "one step undoes it");
    }

    #[test]
    fn a_line_dragged_near_upright_is_stored_upright() {
        // The vertical gate marker, which is what a drawn line is mostly for.
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Line);
        state.begin_draw(300.0, 0.0);
        state.update_draw_drag(306.0, 200.0);
        state.finish_draw_drag();

        let shape = state.project.drawings.first().expect("a line was drawn");
        assert_eq!(
            shape.extent,
            Extent::Scaled {
                minutes: 0,
                rows: 200.0 / ROW_H
            },
            "the lean is given away, the drop is kept"
        );
    }

    // ---- the licence, what changed, and the ask -------------------------

    #[test]
    fn acknowledging_the_licence_records_the_version_and_the_moment() {
        // Both, because a record holding only one of them answers half the
        // question, and the record is the only thing that suppresses it.
        let mut state = AppState::new();
        state.greetings = vec![crate::welcome::Greeting::Licence];
        assert!(state.licence_acknowledged.is_empty());

        state.acknowledge_licence();

        assert_eq!(state.licence_acknowledged, crate::welcome::RUNNING);
        assert!(state.licence_acknowledged_at.contains('T'), "an RFC 3339 moment");
        assert!(state.greetings.is_empty(), "and the page is done with");
        // And with the record in place, a later start owes nothing.
        assert!(crate::welcome::on_start(&state.settings(), crate::welcome::RUNNING).is_empty());
    }

    #[test]
    fn the_version_is_recorded_before_the_pages_are_answered() {
        // This is what makes it once per update rather than once per start.
        // Somebody who closes the window during the notes has been shown them.
        let mut state = AppState::new();
        state.licence_acknowledged = crate::welcome::RUNNING.into();
        state.last_version = "0.0.1-nonesuch".into();

        state.begin_greetings();

        assert!(!state.greetings.is_empty(), "an update owes both pages");
        assert_eq!(state.last_version, crate::welcome::RUNNING);
        // The very next start, with nothing dismissed, owes nothing.
        let mut again = AppState::new();
        again.apply_settings(state.settings());
        again.begin_greetings();
        assert!(again.greetings.is_empty());
    }

    #[test]
    fn the_support_page_asked_for_offers_nothing_to_silence() {
        // Nothing showed it to them, so there is nothing to turn off.
        let mut state = AppState::new();
        state.show_support();
        assert_eq!(
            state.greetings.first(),
            Some(&crate::welcome::Greeting::Support { after_update: false })
        );
        assert!(state.backstage.is_none(), "the File menu gets out of the way");
    }

    #[test]
    fn an_update_will_not_install_over_unsaved_work() {
        // Installing replaces the running program, and a plan that exists only
        // in this window would go with it.
        let mut state = AppState::new();
        state.update_found = Some(crate::updates::Found {
            version: "9.9.9".into(),
            install: crate::updates::Install::SelfManaged,
            artefact: Some(crate::updates::Artefact {
                name: "alterion-open-project-9.9.9-x86_64-linux.tar.gz".into(),
                digest: "0".repeat(64),
            }),
            page: "https://example.test/releases/v9.9.9".into(),
        });
        state.dirty = true;

        let why = state.update_blocked().expect("unsaved work stops it");
        assert!(why.contains("unsaved changes"), "got {why}");

        state.dirty = false;
        assert!(state.update_blocked().is_none());
    }

    /// A release found by a check, at whatever version.
    fn found(version: &str) -> crate::updates::Found {
        crate::updates::Found {
            version: version.into(),
            install: crate::updates::Install::SelfManaged,
            artefact: Some(crate::updates::Artefact {
                name: format!("alterion-open-project-{version}-x86_64-linux.tar.gz"),
                digest: "0".repeat(64),
            }),
            page: format!("https://example.test/releases/v{version}"),
        }
    }

    #[test]
    fn skipping_a_version_takes_the_offer_away_without_stopping_the_next_one() {
        let mut state = AppState::new();
        state.update_landed(Ok(Some(found("9.9.9"))));
        assert!(state.update_found.is_some(), "the offer stands before it is refused");

        state.skip_the_found_version();
        assert_eq!(state.skip_version, "9.9.9");
        assert!(state.update_found.is_none(), "and the offer goes with it");

        // Found again, and this time not offered at all.
        state.update_landed(Ok(Some(found("9.9.9"))));
        assert!(state.update_found.is_none());
        let message = state.update_message.clone().expect("the page that asked is told");
        assert!(message.contains("9.9.9") && message.contains("Options"), "got {message}");

        // The next release is a different version, and nothing was said about
        // it. This is the whole point of storing the version rather than a flag.
        state.update_landed(Ok(Some(found("9.9.10"))));
        assert_eq!(
            state.update_found.as_ref().map(|f| f.version.as_str()),
            Some("9.9.10")
        );
    }

    #[test]
    fn skipping_one_version_does_not_bring_an_older_one_back() {
        // A skip is not a floor either: it says nothing at all about versions
        // other than the one it names.
        let mut state = AppState::new();
        state.skip_version = "9.9.9".into();
        state.update_landed(Ok(Some(found("9.9.8"))));
        assert_eq!(
            state.update_found.as_ref().map(|f| f.version.as_str()),
            Some("9.9.8"),
            "an older release is judged on its own, not by the skip"
        );
    }

    #[test]
    fn withdrawing_a_skip_puts_the_offer_back() {
        let mut state = AppState::new();
        state.skip_version = "9.9.9".into();
        state.update_landed(Ok(Some(found("9.9.9"))));
        assert!(state.update_found.is_none(), "suppressed while the record stands");

        state.offer_the_skipped_version_again();
        assert!(state.skip_version.is_empty());

        state.update_landed(Ok(Some(found("9.9.9"))));
        assert!(state.update_found.is_some(), "and offered once it is gone");
    }

    #[test]
    fn a_skip_this_copy_has_already_passed_is_dropped_at_start_up() {
        // Updated by some other route, so the skipped release is behind us and
        // the record is dead weight. Cleared where the preferences are taken
        // on, which is also where it gets written back to the file.
        let mut state = AppState::new();
        state.apply_settings(crate::settings::Settings {
            skip_version: "0.0.1-nonesuch".into(),
            ..Default::default()
        });
        assert!(state.skip_version.is_empty());

        // One that is still ahead of this copy survives, since it is still a
        // release somebody could be offered.
        let mut ahead = AppState::new();
        ahead.apply_settings(crate::settings::Settings {
            skip_version: "9999.0.0".into(),
            ..Default::default()
        });
        assert_eq!(ahead.skip_version, "9999.0.0");
    }

    #[test]
    fn a_skipped_version_says_nothing_in_the_status_bar() {
        // Being told about it is the thing that was refused.
        let mut state = AppState::new();
        state.skip_version = "9.9.9".into();
        let before = state.status.clone();
        state.update_landed(Ok(Some(found("9.9.9"))));
        assert_eq!(state.status, before);
        assert!(state.dialog.is_none());
    }

    #[test]
    fn failing_to_reach_a_release_host_is_kept_quiet() {
        // Nothing was waiting on the answer, so nothing is interrupted by it
        // not arriving.
        let mut state = AppState::new();
        let before = state.status.clone();
        state.update_landed(Err("nothing is answering at that address".into()));

        assert!(state.update_found.is_none());
        assert_eq!(state.status, before, "the status bar says nothing new");
        assert!(state.dialog.is_none(), "and nothing is put in front of anybody");
    }

    #[test]
    fn a_bar_will_not_drag_while_a_drawing_tool_is_armed() {
        // Otherwise marking a plan up quietly reschedules it.
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Oval);
        state.begin_bar_drag(3, BarDragKind::Move, 100.0, 26.0);
        assert!(state.bar_drag.is_none());
    }

    #[test]
    fn delete_takes_the_selected_shape_rather_than_the_selected_rows() {
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Oval);
        state.begin_draw(300.0, ROW_H * 1.5);
        state.finish_draw_drag();
        let rows = state.project.tasks.len();

        state.delete_selected();

        assert!(state.project.drawings.is_empty());
        assert_eq!(state.project.tasks.len(), rows, "no row was touched");
    }

    #[test]
    fn a_click_that_did_not_drag_still_leaves_something_visible() {
        // A zero-sized shape has nothing to see and nothing to click, so a
        // click that meant to place one would look like it had done nothing.
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Rectangle);
        state.begin_draw(300.0, ROW_H * 1.5);
        state.finish_draw_drag();

        let shape = state.project.drawings.first().expect("a box was placed");
        let Extent::Scaled { minutes, rows } = shape.extent else {
            unreachable!("a box keeps the stretch of plan it covers");
        };
        assert!(minutes > 0 && rows > 0.0);
    }

    #[test]
    fn moving_a_shape_is_one_step_however_far_the_pointer_travels() {
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Rectangle);
        state.begin_draw(300.0, ROW_H * 1.5);
        state.update_draw_drag(400.0, ROW_H * 2.5);
        state.finish_draw_drag();
        let id = state.selected_drawing.expect("the new shape is selected");
        let before = state.project.drawings.clone();

        state.begin_drawing_move(id);
        for step in 1..=10 {
            state.update_draw_drag(400.0 + step as f64 * 10.0, ROW_H * 2.5);
        }
        state.finish_draw_drag();

        assert_ne!(state.project.drawings, before, "it moved");
        state.undo();
        assert_eq!(state.project.drawings, before, "and one undo puts it back");
    }

    #[test]
    fn a_locked_shape_is_selectable_but_does_not_follow_the_pointer() {
        let mut state = outlined();
        state.arm_draw_tool(ShapeKind::Rectangle);
        state.begin_draw(300.0, ROW_H * 1.5);
        state.finish_draw_drag();
        let id = state.selected_drawing.expect("the new shape is selected");
        if let Some(shape) = state.project.drawings.iter_mut().find(|d| d.id == id) {
            shape.locked = true;
        }
        let before = state.project.drawings.clone();

        state.begin_drawing_move(id);
        assert_eq!(state.selected_drawing, Some(id));
        assert!(state.draw_drag.is_none(), "nothing is dragging");

        state.update_draw_drag(900.0, 900.0);
        state.finish_draw_drag();
        assert_eq!(state.project.drawings, before);
    }

    #[test]
    fn dropping_below_a_row_reorders_it() {
        let mut state = outlined();
        // Move "Standalone" above "Phase".
        state.drop_row(3, 0, DropWhere::Above);
        assert_eq!(names(&state), ["Standalone", "Phase", "Child A", "Child B"]);
        assert_eq!(levels(&state), [0, 0, 1, 1]);
    }

    #[test]
    fn dropping_into_a_row_nests_underneath_it() {
        let mut state = outlined();
        // Drop "Standalone" onto "Phase" to make it a third child.
        state.drop_row(3, 0, DropWhere::Into);
        assert_eq!(names(&state), ["Phase", "Child A", "Child B", "Standalone"]);
        assert_eq!(levels(&state), [0, 1, 1, 1], "should land one level deeper");
    }

    #[test]
    fn dragging_a_summary_carries_its_children() {
        let mut state = outlined();
        // Move the whole "Phase" block below "Standalone".
        state.drop_row(0, 3, DropWhere::Below);
        assert_eq!(names(&state), ["Standalone", "Phase", "Child A", "Child B"]);
        assert_eq!(levels(&state), [0, 0, 1, 1]);
    }

    #[test]
    fn a_summary_cannot_be_dropped_inside_itself() {
        let mut state = outlined();
        let before = names(&state).join(",");
        state.drop_row(0, 1, DropWhere::Into);
        assert_eq!(names(&state).join(","), before, "the outline must not change");
    }

    #[test]
    fn outdenting_a_child_by_dropping_it_at_the_top_level() {
        let mut state = outlined();
        state.drop_row(1, 3, DropWhere::Below);
        assert_eq!(names(&state), ["Phase", "Child B", "Standalone", "Child A"]);
        assert_eq!(levels(&state), [0, 1, 0, 0], "should adopt the target's level");
    }

    #[test]
    fn a_bar_drag_of_zero_days_changes_nothing() {
        let mut state = outlined();
        let before = state.project.tasks[3].constraint;
        state.begin_bar_drag(3, BarDragKind::Move, 100.0, 26.0);
        state.update_bar_drag(103.0);
        state.finish_bar_drag(26.0);
        assert_eq!(state.project.tasks[3].constraint, before);
    }

    #[test]
    fn dragging_a_bar_right_pins_the_task_later() {
        let mut state = outlined();
        let start = state.project.tasks[3].scheduled.start;
        state.begin_bar_drag(3, BarDragKind::Move, 100.0, 26.0);
        // Two days at 26 pixels per day.
        state.update_bar_drag(152.0);
        state.finish_bar_drag(26.0);

        let task = &state.project.tasks[3];
        assert_eq!(task.constraint, ConstraintType::StartNoEarlierThan);
        assert!(task.scheduled.start > start, "the task should have moved later");
    }

    #[test]
    fn dragging_the_right_edge_changes_the_duration() {
        let mut state = outlined();
        state.begin_bar_drag(3, BarDragKind::Resize, 100.0, 26.0);
        state.update_bar_drag(178.0); // three days wider
        state.finish_bar_drag(26.0);
        assert_eq!(state.project.tasks[3].duration_minutes, MINUTES_PER_DAY * 4);
    }

    #[test]
    fn a_resize_can_never_produce_a_negative_duration() {
        let mut state = outlined();
        state.begin_bar_drag(3, BarDragKind::Resize, 400.0, 26.0);
        state.update_bar_drag(0.0);
        state.finish_bar_drag(26.0);
        assert_eq!(state.project.tasks[3].duration_minutes, 0);
    }

    #[test]
    fn dragging_from_the_left_edge_sets_progress() {
        let mut state = outlined();
        state.begin_bar_drag(3, BarDragKind::Progress, 100.0, 40.0);
        state.update_bar_drag(120.0); // half the bar width
        state.finish_bar_drag(26.0);
        assert_eq!(state.project.tasks[3].percent_complete, 50);
    }

    #[test]
    fn shift_dragging_one_bar_onto_another_links_them() {
        let mut state = outlined();
        state.begin_bar_drag(3, BarDragKind::Link, 100.0, 26.0);
        state.set_bar_hover(1);
        state.finish_bar_drag(26.0);

        let from = state.project.tasks[3].id;
        let to = state.project.tasks[1].id;
        assert!(state.project.link_exists(from, to));
    }

    #[test]
    fn a_summary_bar_refuses_to_drag() {
        let mut state = outlined();
        state.begin_bar_drag(0, BarDragKind::Move, 100.0, 26.0);
        assert!(state.bar_drag.is_none(), "summary bars are derived");
    }

    #[test]
    fn undo_restores_the_outline_after_a_drop() {
        let mut state = outlined();
        state.drop_row(3, 0, DropWhere::Into);
        assert_eq!(levels(&state), [0, 1, 1, 1]);
        state.undo();
        assert_eq!(names(&state), ["Phase", "Child A", "Child B", "Standalone"]);
        assert_eq!(levels(&state), [0, 1, 1, 0]);
    }

    // ---- unsaved work --------------------------------------------------

    #[test]
    fn a_clean_plan_is_discarded_without_asking() {
        let mut state = AppState::new();
        state.dirty = false;
        state.guard(PendingAction::CloseProject);
        assert!(state.dialog.is_none(), "nothing was at stake");
    }

    #[test]
    fn unsaved_work_is_asked_about_before_it_is_thrown_away() {
        let mut state = AppState::new();
        state.dirty = true;
        state.guard(PendingAction::Quit);
        assert!(
            matches!(state.dialog, Some(Dialog::UnsavedChanges(PendingAction::Quit))),
            "the question has to be asked, and remember what it was asked about"
        );
        assert!(!state.quit_requested, "and nothing happens until it is answered");
    }

    #[test]
    fn declining_to_save_goes_ahead_with_what_was_asked_for() {
        let mut state = AppState::new();
        state.dirty = true;
        state.guard(PendingAction::Quit);
        state.carry_out(PendingAction::Quit);
        assert!(state.quit_requested);
        assert!(state.dialog.is_none());
    }

    #[test]
    fn saving_a_plan_with_no_file_defers_until_a_name_is_chosen() {
        // Save on an unnamed plan cannot finish on its own, so the action it
        // was standing in the way of must wait rather than run regardless.
        let mut state = AppState::new();
        state.dirty = true;
        state.file_path = None;
        state.save_then(PendingAction::Quit);

        assert!(!state.quit_requested, "the plan is still unsaved");
        assert_eq!(state.after_save, Some(PendingAction::Quit));
        assert_eq!(state.backstage, Some(BackstagePage::SaveAs));
    }

    #[test]
    fn a_save_that_fails_abandons_what_was_waiting_on_it() {
        // Otherwise a failed save would still let the plan be thrown away.
        let mut state = AppState::new();
        state.dirty = true;
        state.after_save = Some(PendingAction::Quit);
        state.save_to(PathBuf::from("/proc/nonexistent-directory/plan.aprj"));

        assert!(!state.quit_requested, "the work was never written");
        assert!(state.after_save.is_none());
        assert!(state.dirty, "and it is still unsaved");
    }

    // ---- collaborating --------------------------------------------------

    use crate::cloud::collab::Pushed;
    use crate::cloud::link::Link;
    use aop_core::compare::{compare, Difference};
    use aop_core::history::Change;
    use aop_core::versions::Taken;

    /// A plan that knows where it lives on a server, which is the state every
    /// answer to a push is judged against.
    fn linked() -> AppState {
        let mut state = AppState::new();
        state.project.tasks.clear();
        state.project.links.clear();
        state.user_name = "Ada".into();
        state.collaborate = true;
        state.collaborate_server = "https://sync.example.test".into();
        state.link = Some(Link {
            project: "a-project".into(),
            cursor: 4,
        });
        state
    }

    /// One entry of somebody else's work, as it arrives on the wire.
    fn theirs(id: u64, script: &str, summary: &str) -> Change {
        Change {
            id,
            at: Local::now().naive_local(),
            author: "Grace".into(),
            script: script.into(),
            summary: summary.into(),
        }
    }

    #[test]
    fn work_the_server_took_is_marked_as_sent_and_counted() {
        let mut state = linked();
        let first = state.project.history.record(
            "Ada",
            "append_task(\"A\");",
            "Added A",
            Local::now().naive_local(),
        );
        let second = state.project.history.record(
            "Ada",
            "append_task(\"B\");",
            "Added B",
            Local::now().naive_local(),
        );
        assert_eq!(state.project.history.unsent().len(), 2);

        state.sync_landed(Ok(Pushed::Applied {
            head: 9,
            applied: vec![(first, 8), (second, 9)],
            snapshot_wanted: false,
        }));

        assert!(state.project.history.unsent().is_empty(), "both went");
        assert_eq!(state.link.as_ref().map(|link| link.cursor), Some(9));
        assert_eq!(
            state.checked.as_ref().map(|checked| &checked.outcome),
            Some(&CheckOutcome::Current),
            "the server was asked and agreed, so the tick is earned"
        );
        assert!(
            state.status.contains("2 changes sent"),
            "it says how many, got {:?}",
            state.status
        );
        assert!(state.dialog.is_none(), "nothing to decide");
    }

    #[test]
    fn being_behind_asks_the_planner_rather_than_deciding_for_them() {
        // The one decision this whole design exists to offer, so it is a
        // dialog with the difference in it and not a message.
        let mut state = linked();
        state.project.history.record(
            "Ada",
            "append_task(\"Mine\");",
            "Added Mine",
            Local::now().naive_local(),
        );
        let rows = state.project.tasks.len();

        state.sync_landed(Ok(Pushed::Behind {
            head: 12,
            after: 4,
            changes: vec![theirs(90, "append_task(\"Theirs\");", "Added Theirs")],
            more: false,
        }));

        match &state.dialog {
            Some(Dialog::SyncBehind {
                head, differences, ..
            }) => {
                assert_eq!(*head, 12);
                assert!(!differences.is_empty(), "there is something to show");
            }
            other => panic!("expected the question, got {other:?}"),
        }
        assert_eq!(
            state.project.tasks.len(),
            rows,
            "nothing lands before the answer"
        );
        assert_eq!(
            state.project.history.unsent().len(),
            1,
            "and nothing was marked as sent"
        );
        assert_eq!(
            state.link.as_ref().map(|link| link.cursor),
            Some(4),
            "the cursor stays where it was"
        );
    }

    #[test]
    fn a_trimmed_log_offers_a_whole_plan_and_says_why() {
        let mut state = linked();
        state.sync_landed(Ok(Pushed::Gap {
            head: 40,
            oldest: Some(30),
        }));

        match &state.dialog {
            Some(Dialog::FreshCopy { why }) => {
                assert!(why.contains("30"), "it says what is left, got {why:?}");
            }
            other => panic!("expected the offer of a fresh copy, got {other:?}"),
        }
        assert!(matches!(
            state.checked.as_ref().map(|checked| &checked.outcome),
            Some(CheckOutcome::Failed(_))
        ));
    }

    #[test]
    fn a_cursor_past_the_servers_head_is_refused_rather_than_reconciled() {
        // A copy cannot have read further than there is to read, so this is
        // not the plan it thinks it is and pushing would interleave two logs.
        let mut state = linked();
        state.project.history.record(
            "Ada",
            "append_task(\"Mine\");",
            "Added Mine",
            Local::now().naive_local(),
        );

        state.sync_landed(Ok(Pushed::Ahead { head: 3, cursor: 7 }));

        assert!(matches!(
            state.dialog,
            Some(Dialog::SyncAhead { head: 3, cursor: 7 })
        ));
        assert_eq!(
            state.project.history.unsent().len(),
            1,
            "nothing was sent, so nothing is marked as sent"
        );
        assert_eq!(state.link.as_ref().map(|link| link.cursor), Some(4));
    }

    #[test]
    fn a_rejected_change_leaves_the_plan_as_it_was() {
        // Half of a batch landing is the failure this check exists for: the
        // two copies no longer agree about what the plan was, and writing the
        // part that happened to fit would make that permanent.
        let mut state = linked();
        state.append_task("Bridge");
        let before: Vec<String> = state
            .project
            .tasks
            .iter()
            .map(|task| task.name.clone())
            .collect();

        let mut moved = state.project.clone();
        moved.push_task("Theirs", MINUTES_PER_DAY);
        let mut differences = compare(&state.project, &moved);
        // A row this copy has never had. Their side taking away something that
        // was never here is what drift looks like on the wire.
        differences.push(Difference::TaskRemoved {
            id: 9999,
            name: "A row this copy never had".into(),
        });

        let brought = state.accept_incoming(20, &differences, Vec::new(), 0, 0);

        assert!(!brought.is_clean(), "one of them did not fit");
        let after: Vec<String> = state
            .project
            .tasks
            .iter()
            .map(|task| task.name.clone())
            .collect();
        assert_eq!(after, before, "so none of them landed");
        assert!(
            matches!(state.dialog, Some(Dialog::FreshCopy { .. })),
            "and a whole plan is offered instead of a half changed one"
        );
    }

    #[test]
    fn a_version_is_kept_before_a_rebase_and_can_be_returned_to() {
        // The reason the store exists: a rebase is the only thing that replays
        // a planner's own work on top of somebody else's.
        let mut state = linked();
        state.append_task("Mine");
        assert!(state.versions.is_empty(), "nothing kept yet");

        let mut moved = state.project.clone();
        moved.push_task("Theirs", MINUTES_PER_DAY);
        let differences = compare(&state.project, &moved);

        let brought = state.accept_incoming(
            20,
            &differences,
            vec![theirs(90, "append_task(\"Theirs\");", "Added Theirs")],
            0,
            0,
        );
        assert!(brought.is_clean());
        assert_eq!(state.versions.len(), 1, "the moment before it is kept");
        assert_eq!(
            state.versions.newest().map(|snapshot| snapshot.taken),
            Some(Taken::BeforeRebase)
        );
        assert!(
            state.project.tasks.iter().any(|task| task.name == "Theirs"),
            "their work is here"
        );

        state.restore_version(0);
        assert!(
            !state.project.tasks.iter().any(|task| task.name == "Theirs"),
            "and the version before it can be returned to"
        );
        assert!(
            state.project.tasks.iter().any(|task| task.name == "Mine"),
            "with this planner's own work still in it"
        );
        assert!(
            !state.project.history.changes().is_empty(),
            "the log is kept: going back is one more thing that was done"
        );
    }

    #[test]
    fn saving_twice_with_nothing_edited_between_leaves_one_marker() {
        // A save is the unit a sync offers a decision about. Two of them with
        // nothing in between offer a decision about nothing.
        let mut state = AppState::new();
        state.user_name = "Ada".into();
        state.append_task("Bridge");

        assert!(state.mark_save_point().is_some(), "the first one counts");
        assert!(
            state.mark_save_point().is_none(),
            "the second has nothing under it"
        );
        assert_eq!(state.project.history.saves().len(), 1);

        state.append_task("Tunnel");
        assert!(
            state.mark_save_point().is_some(),
            "an edit in between earns another"
        );
        assert_eq!(state.project.history.saves().len(), 2);
    }

    #[test]
    fn a_browser_offers_everything_the_opener_can_read() {
        // The bug this pins: three copies of "which files can we open" that
        // disagreed. The Open page listed no workbook at all, so an .xlsx was
        // invisible on every platform while `open_any` would happily import
        // it. A file the application can read but will not show is
        // indistinguishable, to the person looking for it, from one it cannot.
        for extension in aop_core::persist::IMPORTED_EXTENSIONS {
            let path = std::path::PathBuf::from(format!("plan.{extension}"));
            assert!(
                offered_in_browser(&path, false),
                ".{extension} is importable but would not be listed"
            );
            // Upper case too: Windows names are commonly shouted.
            let shouted = std::path::PathBuf::from(format!("PLAN.{}", extension.to_uppercase()));
            assert!(offered_in_browser(&shouted, false), ".{extension} uppercase");
        }

        let plan = std::path::PathBuf::from(format!("a.{}", aop_core::persist::FILE_EXTENSION));
        assert!(offered_in_browser(&plan, false));
        // Saving narrows to the plan format: writing a schedule out as a
        // workbook would lose most of what is in it.
        assert!(offered_in_browser(&plan, true));
        assert!(!offered_in_browser(&std::path::PathBuf::from("book.xlsx"), true));

        assert!(!offered_in_browser(&std::path::PathBuf::from("notes.txt"), false));
        assert!(!offered_in_browser(&std::path::PathBuf::from("noextension"), false));
    }
}
