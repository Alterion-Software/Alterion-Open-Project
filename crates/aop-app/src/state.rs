//! Application state and every command the ribbon can fire.
//!
//! One `Signal<AppState>` is provided at the root and read by every component.
//! Mutations go through methods here rather than being written inline in the
//! views, so undo snapshots and rescheduling can never be forgotten.

use std::path::PathBuf;
use std::time::Duration;

use aop_core::draw::{
    snap_vertical, Anchor, BarPoint, Drawing, DrawingId, Extent, ShapeKind,
};
use aop_core::grouping::GroupRow;
use aop_core::{
    persist, schedule, templates, CalendarTarget, ConstraintType, Field, Link, LinkType, Project,
    ResourceId, ScheduleReport, Task, TaskId, TaskMode, WorkCalendar,
};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

use crate::cloud::collab::CollabError;
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

/// What this planner has typed into a cell and not committed yet.
///
/// The most valuable thing on the ephemeral channel and the most dangerous
/// place to put it: a keystroke is not a change to the plan, and holding it
/// on `AppState` would redraw the window on every letter. So it lives here,
/// beside [`Pointing`], for the same reason and read the same way. Nothing
/// renders from it. The timer that offers presence to the socket peeks at it.
///
/// It must never reach the change log. An abandoned edit would otherwise
/// become a permanent record of something that never happened, with an author
/// and a moment against it.
#[derive(Clone, Copy)]
pub struct Drafting(pub dioxus::prelude::Signal<Option<String>>);

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
    /// Saving would write over a file that is already there. Carrying the
    /// path rather than reading it back from the pane, because the pane can
    /// be typed into while the question is on screen.
    ConfirmOverwrite {
        path: PathBuf,
        /// The name that would be used instead, worked out once so the button
        /// can say it rather than promising something vague.
        beside: PathBuf,
    },
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
    /// The other end of the same links `Predecessors` edits. Nothing about a
    /// successor is stored separately, so this column reads and writes
    /// `Project::links` from the far side rather than a list of its own.
    Successors,
    Resources,
}

/// How often the interface looks at the live socket.
///
/// The socket runs on a thread of its own and the plan may only be written
/// where the interface does, so arrivals are collected rather than pushed.
///
/// This used to be one timer doing two jobs at three hundred and fifty
/// milliseconds, and it showed: a plan change arriving a third of a second
/// late is fine, and somebody's pointer arriving three times a second is a
/// slideshow. It is now fast enough for a pointer, which it can afford to be
/// because a tick that finds nothing queued costs nothing at all: the socket
/// says whether anything is waiting before a write handle to the plan is
/// taken, and taking one is what redraws the window. A quiet session is
/// silent, and a busy one redraws at the rate things are actually happening.
pub const LIVE_POLL_MILLIS: u64 = 100;

/// How often this planner's own position is offered to the socket.
///
/// Its own timer, because the two directions want different things: what
/// arrives should arrive as soon as it is there, and what is sent should be
/// capped however fast a mouse moves. The socket sends nothing unless the
/// position actually changed, so this is an upper bound rather than a rate.
pub const EPHEMERAL_POLL_MILLIS: u64 = 120;

/// How long an edit waits before it is offered to the live session.
///
/// Long enough that typing a task name is one message rather than one per
/// keystroke, short enough that somebody watching sees it as it happens
/// rather than afterwards. Every entry still goes in the log first; this is
/// only how long it sits there before being offered.
pub const STREAM_AFTER_MILLIS: u64 = 250;

/// The seq an entry that came from the server carries.
///
/// Every entry the server hands out is identified by its seq, because that is
/// the name the shared log gave it and the only name every copy agrees on.
/// Named here rather than written out at each comparison so that the one
/// assumption this rests on is stated once: an id on an incoming change is a
/// seq, and an id on a change this copy made is not.
fn seq_of(change: &aop_core::history::Change) -> i64 {
    // Saturating rather than wrapping. A seq that will not fit in an i64 is
    // not a thing this protocol can produce, and treating it as the furthest
    // possible point means an unreadable one is skipped rather than replayed.
    i64::try_from(change.id).unwrap_or(i64::MAX)
}

/// How often to look for work a save asked to be sent.
///
/// A read of one flag, so it costs nothing, and no write handle is taken
/// unless there is something to do. Quick enough that pressing Save and
/// watching the status is not a wait, slow enough that a plan nobody has
/// saved is not being asked about several times a second.
pub const SAVE_SYNC_POLL_MILLIS: u64 = 300;

/// How long the local copy of a server plan waits before being rewritten.
///
/// A local file, so this is cheap, but it is still a whole plan serialised
/// and it must not happen per keystroke.
pub const LOCAL_COPY_AFTER_MILLIS: u64 = 2_000;

/// How many entries one streamed batch carries.
///
/// The server refuses more than a page in a single push, and a copy that has
/// been offline for a day can easily have more than that waiting. What is left
/// over goes in the next batch, once this one has been answered and the cursor
/// has moved, which is also how a reconnect drains without a special case.
pub const STREAM_BATCH: usize = 200;

/// How long to wait before offering work the server has just refused.
///
/// Whatever refused it will refuse it again a millisecond later, and a socket
/// that is already open makes a tight retry loop very easy to write by
/// accident. The work is not lost by waiting: it is in the log.
pub const STREAM_RETRY_MILLIS: u64 = 5_000;

/// How long a streamed batch may go unanswered before it is given up on.
///
/// A refusal is an answer and is already handled. Silence is not an answer,
/// and silence is what an older or mismatched server gives: it takes the
/// message, understands none of it, and says nothing. Without a deadline the
/// in-flight marker stays set and this copy never offers anything again, for
/// the rest of the session, without a word.
///
/// Long enough that it is never reached by a server that is merely slow: the
/// REST calls beside it wait far longer than this before giving up, and a
/// batch is one round trip to a database and back. Short enough that somebody
/// finds out inside the same minute they started, rather than at the end of
/// an afternoon. Nothing is lost when it fires: the log still says the work is
/// unsent, because nothing was ever marked as sent.
pub const STREAM_ANSWER_SECONDS: u64 = 20;

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
    /// Entries in the log that were taken from the server rather than done
    /// here.
    ///
    /// `History::unsent` is everything past a watermark, and the watermark
    /// counts in this copy's own ids while a merged entry wears the seq the
    /// server gave it. Those are two numbering schemes sharing a type, so an
    /// entry of somebody else's can land on the unsent side of the line and be
    /// offered straight back, which appends their edit to the shared log a
    /// second time and sends the duplicate to everybody.
    ///
    /// A single watermark cannot express "these two interleaved entries, one
    /// sent and one not", so the ones that must never be offered are named.
    /// Kept for the session rather than in the plan file: the durable half is
    /// the watermark move below, which covers the case where nothing of this
    /// copy's own was waiting, and that is the case this happens in.
    taken_from_server: std::collections::HashSet<u64>,
    /// What the server calls this copy's socket, once it has said.
    ///
    /// Sent back on a REST push so the append does not broadcast the work to
    /// the connection that just offered it. `None` is either no socket or a
    /// server old enough not to say, and both mean the field is left out of
    /// the push, which is the body such a server already expects.
    live_connection: Option<u64>,
    /// Who else has this plan open.
    pub peers: Vec<crate::cloud::live::Peer>,
    /// Somebody else's work that arrived while a cell was open for editing.
    ///
    /// With streaming, being behind stops being rare and becomes constant, so
    /// a dialog every few seconds would be worse than no live editing at all
    /// and a rebase that applies cleanly simply happens. The one thing it must
    /// not do is happen underneath somebody's fingers: a change that touches
    /// the task they have a cell open on waits here until they are done, then
    /// goes in exactly as it would have. Nothing is lost by waiting, because
    /// the cursor does not move until it lands.
    held_live: Vec<aop_core::history::Change>,
    /// When the work waiting here should next be offered to the live
    /// session. `None` means there is nothing waiting to offer.
    ///
    /// A moment rather than a flag, because it is a debounce: typing a task
    /// name pushes it forward and the batch goes once the typing stops.
    stream_due: Option<std::time::Instant>,
    /// When the batch now with the server was offered, if one is.
    ///
    /// One at a time, because the cursor a batch was made against is only
    /// right until the answer moves it. Two in flight would have the second
    /// one built on a head the server has already left behind.
    ///
    /// A moment rather than a flag, because a marker that can only be cleared
    /// by an answer is a marker a server can leave set forever by saying
    /// nothing, and one that has been set too long is the only evidence there
    /// is that that has happened. See [`STREAM_ANSWER_SECONDS`].
    in_flight: Option<std::time::Instant>,
    /// Whether work may be offered over the socket at all.
    ///
    /// False against a server that does not understand the streaming message.
    /// Such a server still takes the connection and still relays everybody
    /// else's edits, so the session is worth having; what it will not do is
    /// take this copy's own work, and pretending otherwise is what makes a
    /// mismatched pair look like it is working.
    stream_out: bool,
    /// Whether this session has already said that nothing is being answered.
    ///
    /// Said once. The condition persists, the timer runs several times a
    /// second, and a message repeated at that rate is one somebody turns off
    /// rather than reads.
    stream_silence_told: bool,
    /// Whether a save has asked for this plan's unsent work to go over the
    /// ordinary sync, and it has not gone yet.
    ///
    /// A flag rather than a call, because a save may not wait on a network:
    /// the file is already written and the save point already marked by the
    /// time this is set, and the worker that acts on it is started from a
    /// timer where a slow server costs nobody a frozen window.
    sync_after_save: bool,
    /// The server asking for a fresh whole plan, once its log has run far
    /// enough past the newest stored one. Housekeeping, with no decision in
    /// it for a planner, so it is answered rather than shown.
    snapshot_wanted: bool,
    /// Where this plan is kept on this machine when it has no file of its
    /// own, because it came off a server.
    ///
    /// Not a backup and never described as one: one copy, overwritten as work
    /// happens, so that closing the window does not lose a plan that was
    /// never saved anywhere.
    pub local_copy: Option<std::path::PathBuf>,
    /// When that copy should next be written. `None` means it matches.
    local_due: Option<std::time::Instant>,
    /// Whether a local copy is being written right now. Shared with the
    /// thread doing it, so a second write cannot overtake the first and leave
    /// the older plan on disk.
    local_writing: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
            taken_from_server: std::collections::HashSet::new(),
            live_connection: None,
            held_live: Vec::new(),
            stream_due: None,
            in_flight: None,
            // Assumed until a server says otherwise, so nothing that never
            // opens a live session pays for a question about one.
            stream_out: true,
            stream_silence_told: false,
            sync_after_save: false,
            snapshot_wanted: false,
            local_copy: None,
            local_due: None,
            local_writing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            live_wanted: false,
            peers: Vec::new(),
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
        // Every entry, and only entries. The log is the record and it is also
        // what streams: there is no quicker path that skips it, because the
        // record is what makes a message somebody missed recoverable rather
        // than lost.
        self.work_to_offer();
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
        // The local copy belongs to whichever plan was on screen, not to this
        // one. Let go of it rather than delete it: that plan is still shared,
        // still on the server, and its copy is still its home.
        self.local_copy = None;
        self.local_due = None;
        self.held_live.clear();
        // Names of entries in a log that is no longer the log on screen. Left
        // behind, they would exclude whatever entries of this plan's own
        // happened to be called the same thing.
        self.taken_from_server.clear();
        self.snapshot_wanted = false;
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
        crate::applog::applog!("restore: signed in as {}", session.account().name);
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
    /// A plan that now lives somewhere new on this machine.
    ///
    /// **Save As on a shared plan keeps the link.** Somebody who wants a real
    /// file in their documents folder should get one, and it should still be
    /// the same plan: same project, same cursor, still syncing. Saving a copy
    /// is not the same as leaving a shared plan, and reading the link back off
    /// the new path, which is what used to happen, quietly conflated the two.
    ///
    /// The local copy goes at the same moment. Two copies of one plan, both
    /// being written to, is the state this whole protocol exists to prevent,
    /// and the file somebody chose is the one that should win.
    fn moved_to(&mut self, path: &std::path::Path) {
        match self.link.clone() {
            Some(link) => {
                crate::cloud::link::save(path, &link);
                self.drop_the_local_copy();
                self.sharing = None;
                self.sharing_for = None;
            }
            None => self.restore_link(),
        }
    }

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

    /// Everything a push carries, gathered in one place.
    ///
    /// One builder for the sync and the pull, because they differ only in what
    /// is offered and every other field has to be the same. `connection` in
    /// particular: it is the field nothing noticed was missing, and a second
    /// copy of this list is how it stays missing from one of them.
    pub(crate) fn offer_of(
        &self,
        changes: Vec<aop_core::history::Change>,
    ) -> Option<crate::cloud::work::Offer> {
        let link = self.link.clone()?;
        Some(crate::cloud::work::Offer {
            server: self.collaborate_server.trim().to_string(),
            project: link.project,
            after: link.cursor,
            changes,
            plan: self.project.clone(),
            // The socket the server must not send this work back down. Sent
            // even on a pull, which offers nothing and so can be echoed
            // nothing, because which connection this is does not depend on
            // what is in the body.
            connection: self.live_connection,
        })
    }

    /// Gather everything a sync needs, and hand the session over.
    pub fn start_sync(&mut self) -> Option<(crate::cloud::Session, crate::cloud::work::Offer)> {
        if self.sync_blocked().is_some() {
            return None;
        }
        let offer = self.offer_of(self.our_unsent())?;
        let session = self.hand_over(Working::Syncing)?;
        // Whatever a save asked for is being carried out now. Cleared once the
        // work is actually under way, and cleared then rather than when it
        // succeeds: a sync that fails is one attempt rather than a retry on
        // every tick at a server already having a bad time, and the work is
        // still in the log for the next save to ask about.
        self.sync_after_save = false;
        Some((session, offer))
    }

    /// Hand over what a fresh whole plan for the server needs, if it asked.
    ///
    /// Cleared as it is taken, so a request that fails is not retried on
    /// every tick at a server that is already having a bad time. The server
    /// asks again on the next batch that lands.
    pub fn start_snapshot(
        &mut self,
    ) -> Option<(crate::cloud::Session, String, String, i64, Project)> {
        if !self.snapshot_wanted() || self.sync_blocked().is_some() {
            return None;
        }
        let link = self.link.clone()?;
        let server = self.collaborate_server.trim().to_string();
        let plan = self.project.clone();
        let session = self.hand_over(Working::Syncing)?;
        self.snapshot_wanted = false;
        Some((session, server, link.project, link.cursor, plan))
    }

    /// Whether the server is waiting on a fresh whole plan, and this copy is
    /// in a position to give it one.
    ///
    /// The unsent check is the load bearing part, and it is what streaming
    /// made necessary. A snapshot is stored under a seq and read back as "the
    /// plan as of that seq". The plan on screen is only that while there is
    /// nothing waiting to go: with work still unsent it is the plan as of that
    /// seq *plus* edits the server has never seen, and storing it under the
    /// cursor would have the next person to open the plan receive those edits
    /// in the snapshot and then receive them again, as log entries, when they
    /// are finally pushed.
    ///
    /// Under the REST sync this could not happen, because the snapshot went in
    /// the same breath as a push that had just emptied the log. Streaming
    /// separated the two, so the condition has to be stated. Waiting costs
    /// nothing: the ask stays set and the next tick after the batch is
    /// acknowledged answers it.
    pub fn snapshot_wanted(&self) -> bool {
        self.snapshot_wanted
            && self.working.is_none()
            && !self.have_unsent()
    }

    /// Gather a pull: what the server has, without offering anything.
    ///
    /// The same call a sync makes with nothing in it, which is exactly what a
    /// pull is: an empty offer is how a client asks "am I still current?",
    /// and the answer to a client that is not carries what it missed. Reusing
    /// it means a pull is answered by the same decision, previewed by the
    /// same dialog and applied by the same rebase as everything else.
    pub fn start_pull(&mut self) -> Option<(crate::cloud::Session, crate::cloud::work::Offer)> {
        if self.sync_blocked().is_some() {
            return None;
        }
        // Deliberately nothing offered. Asking to see what is there is not
        // asking to hand over what is here, and a pull that quietly pushed
        // would be a sync under another name.
        let offer = self.offer_of(Vec::new())?;
        let session = self.hand_over(Working::Syncing)?;
        Some((session, offer))
    }

    /// What the server said about a push.
    pub fn sync_landed(&mut self, outcome: Result<crate::cloud::collab::Pushed, CollabError>) {
        use crate::cloud::collab::Pushed;

        self.working = None;
        let now = Local::now().naive_local();
        let pushed = match outcome {
            Ok(pushed) => pushed,
            Err(error) => {
                let why = error.to_string();
                if matches!(error, CollabError::NoSuchProject) {
                    self.no_longer_on_the_server();
                }
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
                self.touch_local_copy();
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
        let drifted = self.catch_up_to(&fetched.changes);
        // Every entry in this log came from the server, and the watermark
        // below says so exactly, so there is nothing left to name.
        self.taken_from_server.clear();
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
        self.cloud_message = Some(match drifted {
            Some(why) => format!(
                "This plan has been replaced with the server's copy. {why} The version you had \
                 is in History and Sync if you want it back."
            ),
            None => "This plan has been replaced with the server's copy. The version you had is \
                     in History and Sync if you want it back."
                .to_string(),
        });
    }

    /// Bring a fetched plan up to the end of the log that came with it.
    ///
    /// What the server sends is a snapshot as of some seq and every entry
    /// appended since. The snapshot is a plan; the tail is commands, and until
    /// they are run this copy is showing the plan as it was when the snapshot
    /// was stored rather than as it is now. Merging them into the history
    /// records that they happened without making them have happened here.
    ///
    /// How far behind the snapshot is depends entirely on the server's
    /// setting: the log is allowed to run a set number of entries past the
    /// newest stored snapshot before the server asks a client for a fresh one,
    /// so this is a run of at most that many commands and usually far fewer.
    ///
    /// Gives back what to say when some of them would not run, which is the
    /// two sides having drifted rather than a transient failure. Nothing is
    /// undone in that case: the commands that did run are the ones the server
    /// holds, and stopping at the first refusal keeps the plan a prefix of the
    /// log rather than a plan with holes in it.
    fn catch_up_to(&mut self, tail: &[aop_core::history::Change]) -> Option<String> {
        if tail.is_empty() {
            return None;
        }
        // Anything held from the plan that was on screen a moment ago belongs
        // to that plan and not to this one. Dropped rather than written,
        // because writing it would sign somebody's half finished edit of one
        // plan into the log of another.
        self.pending.clear();
        let (replayed, asked) = self.replay(tail);
        if replayed >= asked {
            return None;
        }
        Some(format!(
            "{} of the {asked} changes made since the server's stored copy could not be \
             replayed here, so this plan is as it stood after the first {replayed} of them. \
             That means this copy and the server understand those commands differently, which \
             is worth telling whoever runs the server about.",
            asked - replayed,
        ))
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
        // A first open is a snapshot plus whatever has been appended since,
        // and the second half has to be run for this copy to be showing the
        // plan as it is rather than as it was when the snapshot was stored.
        let drifted = self.catch_up_to(&fetched.changes);
        // As above: the whole log came from the server and the watermark says
        // so, so no entry has to be named separately.
        self.taken_from_server.clear();
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
            project: project.clone(),
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
        // Written straight away, because the gap between "it is on screen"
        // and "it exists somewhere" is exactly the gap this closes.
        self.keep_a_local_copy(&project);
        self.remember_cursor(head);
        let kept = "This plan came from the server. It is kept on this machine so that \
                    closing the window does not lose it, but that copy is not a backup and \
                    it is not yours to find: use Save As to put the plan where you want it, \
                    and it stays the same shared plan.";
        self.cloud_message = Some(match drifted {
            Some(why) => format!("{why} {kept}"),
            None => kept.to_string(),
        });
    }

    /// Open a plan from the local copy this machine already has.
    ///
    /// The point of the cursor. A plan opened for the second time is on screen
    /// as fast as a file opens, and what happened while it was closed comes
    /// down as a handful of log entries rather than as a whole plan. Anything
    /// that never reached the server is still in the log, and still unsent.
    pub fn open_local_copy(&mut self, server: String, project: String) -> bool {
        let Some((path, plan)) = crate::cloud::local::load(&project) else {
            return false;
        };
        // The cursor lives beside the copy rather than inside it, so that a
        // plan file handed to somebody else never claims to have read work it
        // has never seen.
        let cursor = crate::cloud::link::load(&path)
            .map(|link| link.cursor)
            .unwrap_or(0);

        // Whatever was open before was a different plan, and a socket into it
        // has nothing to do with this one.
        self.stop_live(None);
        self.snapshot_wanted = false;
        // A different plan, so anything named against the last one's log means
        // nothing here. What this copy has not sent is what its own watermark
        // says, which came off the disk with it.
        self.taken_from_server.clear();
        self.project = plan;
        self.file_path = None;
        self.local_copy = Some(path);
        self.local_due = None;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.pending.clear();
        self.collaborate_server = server;
        self.link = Some(crate::cloud::link::Link { project, cursor });
        self.versions = aop_core::versions::Versions::new();
        self.checked = None;
        self.dialog = None;
        self.selection = if self.project.tasks.is_empty() { Vec::new() } else { vec![0] };
        self.clamp_selection();
        self.reschedule();
        let waiting = self.project.history.unsent().len();
        self.status = "Opened the copy on this machine".into();
        self.cloud_message = Some(match waiting {
            0 => "Opened from the copy on this machine. Catching up with the server.".to_string(),
            1 => "Opened from the copy on this machine. 1 change was still waiting to go."
                .to_string(),
            many => format!(
                "Opened from the copy on this machine. {many} changes were still waiting to go."
            ),
        });
        true
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
        // work rather than being left to a later sync to discover, and on the
        // sent side of the line, because the server is where they came from.
        self.take_into_the_log(&changes);
        self.touch_local_copy();
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
        // What is on screen is not part of the question. Replaying their
        // commands runs the real ones, and those move a selection and close
        // whatever cell is open for editing, so a preview would quietly throw
        // away somebody's half typed word to answer a question about it.
        let selection = self.selection.clone();
        let editing = self.editing;
        let (replayed, asked) = self.replay(incoming);
        let theirs = std::mem::replace(&mut self.project, mine);
        self.selection = selection;
        self.editing = editing;
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

    /// The server no longer has this plan for this account.
    ///
    /// Deleted by its owner, or this account taken out of it. Either way there
    /// is nothing left to sync with, and a local copy left behind would be a
    /// plan that quietly came back the next time somebody opened the same
    /// link. So it goes, along with the cursor beside it, and the plan that is
    /// open stays open as an ordinary unsaved plan that somebody can put
    /// wherever they like.
    fn no_longer_on_the_server(&mut self) {
        self.stop_live(None);
        self.drop_the_local_copy();
        if let Some(path) = self.file_path.clone() {
            crate::cloud::link::forget(&path);
        }
        self.link = None;
        // Unsaved on purpose. The work is still here and still theirs, and
        // this copy is now the only one of it.
        self.dirty = true;
    }

    // ---- the local copy of a plan that lives on a server ------------------

    /// Note that the local copy no longer matches what is on screen.
    ///
    /// A moment rather than a flag, because it is a debounce: a run of edits
    /// pushes it forward and the write happens once the run stops.
    fn touch_local_copy(&mut self) {
        if self.local_copy.is_none() {
            return;
        }
        if self.local_due.is_none() {
            self.local_due =
                Some(std::time::Instant::now() + Duration::from_millis(LOCAL_COPY_AFTER_MILLIS));
        }
    }

    /// Whether the local copy is out of date and its moment has come.
    ///
    /// Asked before a write handle is taken, for the same reason everything
    /// else on these timers is: a plan nobody is editing must not redraw.
    pub fn local_copy_due(&self) -> bool {
        self.local_copy.is_some()
            && self
                .local_due
                .is_some_and(|due| due <= std::time::Instant::now())
    }

    /// Write the local copy, on a thread of its own.
    pub fn write_local_copy(&mut self) {
        let Some(path) = self.local_copy.clone() else {
            return;
        };
        self.local_due = None;
        crate::cloud::local::write_in_background(
            path,
            self.project.clone(),
            std::sync::Arc::clone(&self.local_writing),
        );
    }

    /// Start keeping a local copy of this plan, and write one now.
    ///
    /// Called the moment a plan arrives from a server, because the window
    /// between "it is on screen" and "it exists somewhere" is exactly the
    /// window this is meant to close.
    fn keep_a_local_copy(&mut self, project: &str) {
        self.local_copy = crate::cloud::local::path_for(project);
        self.local_due = Some(std::time::Instant::now());
    }

    /// Stop keeping one, and remove what is there.
    ///
    /// Two copies of one plan, both being written to, is the state this whole
    /// protocol exists to prevent, so the local copy goes the moment there is
    /// a real file: whichever way somebody opens the plan next, there is one
    /// home for it and one cursor beside it.
    fn drop_the_local_copy(&mut self) {
        if let Some(path) = self.local_copy.take() {
            crate::cloud::local::discard(&path);
        }
        self.local_due = None;
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

    /// Open the socket with a token a worker has just fetched, and what that
    /// worker found out about the server on the other end.
    ///
    /// The two answers a mismatched pair can give, and what each does:
    ///
    /// ```text
    ///   Speaks::Streaming     work goes over the socket, as it is done
    ///   Speaks::NotStreaming  the socket watches only, and it is said out
    ///                         loud; this copy's work goes on the REST sync
    ///   Speaks::Unknown       assumed to work, because a health endpoint
    ///                         that did not answer is not a refusal; a batch
    ///                         nobody answers times out and says so
    /// ```
    ///
    /// The old client against a new server needs nothing here: it asks no
    /// question, the extra field in the answer means nothing to it, and the
    /// server still speaks everything it spoke before.
    pub fn start_live(&mut self, token: String, speaks: crate::cloud::health::Speaks) {
        use crate::cloud::health::Speaks;

        self.working = None;
        let Some(link) = self.link.clone() else {
            return;
        };
        self.stream_out = !matches!(speaks, Speaks::NotStreaming);
        let name = match self.display_name().as_str() {
            "" => "Someone".to_string(),
            name => name.to_string(),
        };
        // The provider's address for this account's face, so the others can
        // draw it rather than a pair of letters. Absent is the ordinary case.
        let picture = self
            .account
            .as_ref()
            .and_then(|account| account.picture.clone());
        // The address itself is never written down: it carries the access
        // token in its query string, which is the one thing that must never
        // reach a file anybody might paste into a bug report.
        crate::applog::applog!(
            "live: opening a connection to {} for plan {} from cursor {}, streaming {}",
            self.collaborate_server.trim(),
            link.project,
            link.cursor,
            self.stream_out,
        );
        match crate::cloud::live::Live::connect(
            self.collaborate_server.trim(),
            &token,
            &link.project,
            link.cursor,
            &name,
            picture.as_deref(),
        ) {
            Ok(live) => {
                self.live = Some(live);
                self.live_wanted = true;
                // Whatever this copy has not sent goes as soon as the socket
                // is up. That is the other half of a reconnect: the catch-up
                // brings in what was missed, and this offers what was made
                // while there was nowhere to offer it.
                self.in_flight = None;
                self.stream_silence_told = false;
                // Not known until the welcome arrives. Anything pushed before
                // then leaves the field out, which is what it did before this
                // existed, and the cursor is what keeps that push's echo out.
                self.live_connection = None;
                self.stream_due = Some(std::time::Instant::now());
                self.status = "Live editing is on".into();
                if !self.stream_out {
                    // Said plainly and once, at the moment somebody turned it
                    // on, because everything they can see says it is working:
                    // the others appear, their edits arrive, and only this
                    // copy's own work quietly goes nowhere.
                    self.status = "Live editing is on, for watching only".into();
                    self.cloud_message = Some(
                        "This server does not understand edits sent over the live connection, \
                         so it is older than this copy. You will see other people's work as it \
                         happens, and your own goes to the server when you save this plan or \
                         press Sync rather than as you type. Updating the server turns the \
                         rest of it on."
                            .into(),
                    );
                    // The work still has somewhere to go, so it goes: the
                    // ordinary sync carries it instead of the socket.
                    self.sync_after_save = self.have_unsent();
                }
            }
            Err(error) => {
                crate::applog::applog!("live: the connection could not be started: {error}");
                self.live_wanted = false;
                self.cloud_message = Some(error.to_string());
            }
        }
    }

    /// Close the socket, and say why if there is anything to say.
    pub fn stop_live(&mut self, why: Option<String>) {
        // Only when there was one. This is also the tidy-up on the way into a
        // plan that has no live session at all, and a log that says a
        // connection closed when none was ever open is a log that sends the
        // next person reading it after the wrong thing.
        if self.live.is_some() {
            crate::applog::applog!(
                "live: the connection is being closed ({})",
                why.as_deref().unwrap_or("no reason given")
            );
        }
        self.live = None;
        self.live_wanted = false;
        // The handle belonged to the connection that has just gone. Sending it
        // on a later push would name a connection that is no longer there, and
        // at worst somebody else's.
        self.live_connection = None;
        self.peers.clear();
        // The next session has told nobody anything, and what this one said
        // went with it.
        // Nothing is in flight any more, and what was in flight was never
        // acknowledged, so it is still unsent work and stays in the log.
        self.in_flight = None;
        self.stream_silence_told = false;
        // What the last server understood says nothing about the next one.
        self.stream_out = true;
        self.stream_due = None;
        // Held work was never applied and the cursor never moved past it, so
        // dropping it loses nothing: the next sync asks for it again.
        self.held_live.clear();
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
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let batch = live.drain();
        if batch.is_empty() {
            return;
        }
        self.take_live_arrivals(batch);
    }

    /// Act on one batch of messages, whichever socket they came off.
    ///
    /// Split from the poll so that what the protocol does with each answer can
    /// be checked without a server, a socket or a thread. The protocol is the
    /// part that can be wrong in a way that loses somebody's work.
    fn take_live_arrivals(&mut self, batch: Vec<crate::cloud::live::Incoming>) {
        use crate::cloud::live::Incoming;

        let mut incoming = Vec::new();
        let mut cursor = None;
        let mut ended = None;
        let mut gap = false;
        let mut ahead = None;

        // A census rather than a line each: a catch-up can be hundreds of
        // changes and they are all the same news. The arms below say more
        // about the ones that decide anything.
        if crate::applog::on() {
            let changes = batch
                .iter()
                .filter(|one| matches!(one, Incoming::Change { .. }))
                .count();
            let presence = batch
                .iter()
                .filter(|one| matches!(one, Incoming::Presence(_)))
                .count();
            crate::applog::applog!(
                "live in: {} message(s), {changes} change(s), {presence} presence",
                batch.len()
            );
        }

        for message in batch {
            match message {
                Incoming::Welcome { head, peers, connection } => {
                    crate::applog::applog!(
                        "live in: welcome at head {head}, {} other(s) here, \
                         connection {connection:?}",
                        peers.len()
                    );
                    self.peers = peers;
                    // Kept for the REST push to hand back. An older server
                    // says nothing here, and then the push omits the field and
                    // the cursor below is what keeps the echo out.
                    self.live_connection = connection;
                    cursor = Some(head.max(cursor.unwrap_or(head)));
                }
                // A catch-up comes before any live change on purpose, so the
                // order things are applied in is the log's order.
                Incoming::Catchup { head, changes } => {
                    crate::applog::applog!(
                        "live in: catch-up to head {head}, {} change(s)",
                        changes.len()
                    );
                    cursor = Some(head.max(cursor.unwrap_or(head)));
                    incoming.extend(changes);
                }
                Incoming::Change { seq, change } => {
                    cursor = Some(seq.max(cursor.unwrap_or(seq)));
                    incoming.push(change);
                }
                Incoming::Applied { head, applied, snapshot_wanted } => {
                    crate::applog::applog!(
                        "live in: {} change(s) went in, head {head}, \
                         snapshot wanted {snapshot_wanted}",
                        applied.len()
                    );
                    // Marked by the local id the server acknowledged rather
                    // than by counting, because an answer that came back out
                    // of order must not mark work nobody has seen as sent.
                    self.batch_answered();
                    if let Some(highest) = applied.iter().map(|(local, _)| *local).max() {
                        self.project.history.mark_pushed(highest);
                    }
                    // Remembered here rather than gathered with the rest,
                    // because this head is one nothing else has to be applied
                    // to reach: an answer of "applied" means nobody else got
                    // in, so the log's end is this copy's own last change.
                    self.remember_cursor(head);
                    // Whatever did not fit in that batch goes in the next one.
                    if self.have_unsent() {
                        self.stream_due = Some(std::time::Instant::now());
                    }
                    self.snapshot_wanted |= snapshot_wanted;
                    self.touch_local_copy();
                }
                Incoming::Behind { head, changes, .. } => {
                    crate::applog::applog!(
                        "live in: refused as behind, head {head}, {} change(s) came back",
                        changes.len()
                    );
                    // Nothing was written, so the work offered is still
                    // unsent and still in the log. What came back is what was
                    // missed, and it goes in the same way a live change does:
                    // replayed onto this copy, or refused outright.
                    self.batch_answered();
                    cursor = Some(head.max(cursor.unwrap_or(head)));
                    incoming.extend(changes);
                    // Offered again once their work is in, which is what a
                    // rebase is. Leaving it for somebody to press Sync is how
                    // an afternoon of work quietly never goes anywhere.
                    self.stream_due = Some(std::time::Instant::now());
                }
                Incoming::Ahead { head, cursor: mine } => {
                    crate::applog::applog!(
                        "live in: this copy is ahead of the server, head {head} against {mine}"
                    );
                    self.batch_answered();
                    ahead = Some((head, mine));
                }
                Incoming::Refused(why) => {
                    crate::applog::applog!("live in: the server refused a batch: {why}");
                    // Nothing was written, so the batch is still unsent work
                    // and is offered again after a pause. A pause rather than
                    // at once, because whatever refused it will refuse it
                    // again a millisecond later and this is a socket, not a
                    // retry loop.
                    self.batch_answered();
                    self.stream_due = Some(
                        std::time::Instant::now() + Duration::from_millis(STREAM_RETRY_MILLIS),
                    );
                    if !why.trim().is_empty() {
                        self.cloud_message = Some(format!(
                            "The server would not take a change: {why}. It is still here and \
                             will be offered again."
                        ));
                    }
                }
                Incoming::Gap { .. } => {
                    crate::applog::applog!("live in: the server's log was trimmed past this copy");
                    gap = true;
                }
                Incoming::Presence(peer) => {
                    match self
                        .peers
                        .iter_mut()
                        .find(|held| held.subject == peer.subject)
                    {
                        Some(held) => {
                            held.row = peer.row;
                            held.name = peer.name;
                            held.picture = peer.picture;
                            // These two are stated whole every time, so what
                            // arrives is the answer and not a difference: a
                            // cell that has been closed and one that was
                            // simply not mentioned would otherwise look the
                            // same, and guessing wrong leaves somebody's
                            // abandoned half word on screen.
                            held.editing = peer.editing;
                            held.draft = peer.draft;
                            // The pointer is the exception, because there is
                            // no such thing as a pointer going away. Copying
                            // an absent one across would blank somebody's
                            // pointer every time they moved their selection.
                            if peer.at.is_some() {
                                held.at = peer.at;
                            }
                        }
                        None => self.peers.push(peer),
                    }
                }
                Incoming::Joined { name } => self.status = format!("{name} joined this plan"),
                Incoming::Left { subject } => self.peers.retain(|held| held.subject != subject),
                Incoming::Closed(why) => {
                    crate::applog::applog!("live in: the connection ended: {why}");
                    ended = Some(why);
                }
            }
        }

        if let Some((head, cursor)) = ahead {
            // Two logs that share numbers but not events. Appending would
            // interleave them, so the socket goes and the question is put in
            // front of somebody rather than guessed at.
            self.stop_live(None);
            self.dialog = Some(Dialog::SyncAhead { head, cursor });
            return;
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
            if self.push_in_flight() {
                // Parked rather than applied, and the cursor deliberately not
                // remembered. A push of this copy's own work is out with the
                // server, and until its answer lands and moves the cursor past
                // what it carried, an entry arriving here cannot be told from
                // somebody else's. Held work is applied the moment the answer
                // is in, and by then the cursor says which of it was this
                // copy's own all along.
                crate::applog::applog!(
                    "live: {} change(s) held until this copy's own push is answered",
                    incoming.len()
                );
                self.held_live.extend(incoming);
            } else {
                self.take_live_batch(&incoming, cursor);
            }
        }
        if let Some(why) = ended {
            self.stop_live(Some(why));
        }
    }

    /// Tell the others what this planner is doing, when any of it is news.
    ///
    /// Everything ephemeral in one call: the selected row, what the pointer is
    /// over, the cell that is open, and what has been typed into it and not
    /// committed. None of it goes in the log, none of it gets a seq, and none
    /// of it survives the connection, which is right: it is where somebody is
    /// now, not what they did.
    ///
    /// `&self` on purpose, and that is the whole reason this is arranged the
    /// way it is. It is called several times a second while a mouse is
    /// moving; taking a write handle to the plan would redraw the window for
    /// every pointer position. The socket keeps its own memory of what it last
    /// said and works out the difference itself, so nothing here has to.
    ///
    /// `at` and `draft` come from signals nothing renders from, peeked rather
    /// than read, for exactly the same reason.
    pub fn announce(&self, at: Option<crate::cloud::live::Pointer>, draft: Option<String>) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let editing = self.editing_cell();
        live.looking_at(&crate::cloud::live::Presence {
            row: self.primary().map(|row| row as i64),
            at,
            editing,
            // A draft belongs to the cell it is being typed into. Sending one
            // with no cell open would leave somebody's half typed word on
            // screen after they had abandoned it.
            draft: draft.filter(|_| editing.is_some()),
        });
    }

    /// The cell this planner has open, in the form the others can place.
    ///
    /// The stored form is a row and which *kind* of column is being edited;
    /// what travels is a row and a column number, because the receiving copy
    /// may not be showing the same columns in the same order and a number is
    /// something it can look up in its own layout.
    fn editing_cell(&self) -> Option<crate::cloud::live::Cell> {
        let (row, column) = self.editing?;
        let index = self.columns.iter().position(|held| {
            matches!(
                (held.field, column),
                (Field::Name, Column::Name)
                    | (Field::Duration, Column::Duration)
                    | (Field::Start, Column::Start)
                    | (Field::Finish, Column::Finish)
                    | (Field::Predecessors, Column::Predecessors)
                    | (Field::Successors, Column::Successors)
                    | (Field::ResourceNames, Column::Resources)
            )
        })?;
        Some(crate::cloud::live::Cell {
            row: row as i64,
            column: u16::try_from(index).unwrap_or(u16::MAX),
        })
    }

    // ---- streaming work out ----------------------------------------------

    /// Whether there is work waiting whose moment has come.
    ///
    /// Asked before a write handle is taken, so that a live session with
    /// nothing happening does not redraw the window on every tick.
    pub fn stream_due(&self) -> bool {
        self.streams_out()
            && self.in_flight.is_none()
            // Work of somebody else's is waiting to go in ahead of this. It
            // was made against a cursor this copy has not reached, so offering
            // anything now would only be told it is behind by that.
            && self.held_live.is_empty()
            && self
                .stream_due
                .is_some_and(|due| due <= std::time::Instant::now())
            && self.have_unsent()
    }

    /// The batch with the server has been answered, whatever the answer was.
    ///
    /// One place for it, because the four answers agree about exactly this:
    /// the round trip is over and the socket is free to carry the next one.
    /// Two of them mark work as sent and two do not, and that difference is
    /// made where the answer is read.
    fn batch_answered(&mut self) {
        self.in_flight = None;
        // A session that has started answering again is one worth complaining
        // about afresh if it stops.
        self.stream_silence_told = false;
    }

    /// Whether the batch with the server has gone unanswered too long.
    ///
    /// Asked from the same timer that offers work, and before a write handle
    /// is taken, so a session that is behaving normally costs nothing to
    /// check.
    pub fn stream_unanswered(&self) -> bool {
        self.in_flight.is_some_and(|sent| {
            sent.elapsed() >= Duration::from_secs(STREAM_ANSWER_SECONDS)
        })
    }

    /// Give up on a batch nobody answered, and carry on.
    ///
    /// This is the failure that cost an afternoon: a batch went to a server
    /// that did not understand the message, the server said nothing at all,
    /// and the in-flight marker stayed set. From that moment nothing else
    /// streamed, for the rest of the session, without a word. Sync went on
    /// working, because it is a different transport, which is exactly what
    /// made it look like something else.
    ///
    /// Giving up is safe and needs no undoing. Nothing was ever marked as
    /// sent, because only an answer marks anything, so the log still holds
    /// every entry as unsent and the next offer carries them again. If the
    /// server was merely slow and its answer arrives late, it acknowledges
    /// work this copy is offering again; `mark_pushed` only ever moves
    /// forward, and a change the server already holds is ignored by its own
    /// push decision rather than applied twice.
    pub fn gave_up_on_batch(&mut self) {
        if !self.stream_unanswered() {
            return;
        }
        self.in_flight = None;
        // A pause rather than at once. A server that ignored one batch will
        // ignore the next, and this is a socket rather than a retry loop.
        self.stream_due =
            Some(std::time::Instant::now() + Duration::from_millis(STREAM_RETRY_MILLIS));

        // Once. The condition persists and this is asked several times a
        // second; a message repeated at that rate is one somebody dismisses
        // without reading. Cleared again by the next answer that arrives, so
        // a session that recovers and then fails again says so again.
        if self.stream_silence_told {
            return;
        }
        self.stream_silence_told = true;
        crate::applog::applog!(
            "live out: a batch went unanswered for {STREAM_ANSWER_SECONDS}s \
             and has been given up on"
        );
        self.cloud_message = Some(format!(
            "The server has not answered work sent over the live connection for \
             {STREAM_ANSWER_SECONDS} seconds. That usually means it is older than this copy \
             and does not understand it. Nothing has been lost: your work is still here and \
             still waiting to go, and it goes when you save this plan or press Sync. Turning \
             live editing off and on again re-checks the server."
        ));
        self.status = "The live connection is not answering".into();
    }

    /// Offer this copy's unsent work over the live socket.
    ///
    /// The socket is already open, so there is no handshake to pay for and no
    /// reason to batch harder than the debounce already does. What goes is
    /// what the log says has not been sent, in the order it was done, against
    /// the cursor this copy has read to. The server answers with the same four
    /// decisions a REST push is answered with, because it is the same
    /// protocol reached a different way.
    pub fn stream_changes(&mut self) {
        // A REST sync moves the same work and marks the same entries. Two of
        // them running at once would offer the same commands twice, and the
        // second offer would be told it is behind by its own first one.
        if matches!(
            self.working,
            Some(Working::Syncing | Working::Publishing | Working::Fetching)
        ) {
            crate::applog::applog!("live out: not offering work, a sync is already running");
            return;
        }
        if !self.stream_due() {
            crate::applog::applog!("live out: not offering work, nothing is due");
            return;
        }
        let Some(after) = self.link.as_ref().map(|link| link.cursor) else {
            crate::applog::applog!("live out: not offering work, this plan is not linked");
            return;
        };
        // Capped, because the server refuses a batch bigger than one page and
        // a copy that has been offline all day can easily have more than that
        // waiting. What is left goes in the next batch, once this one is
        // answered and the cursor has moved.
        let batch: Vec<aop_core::history::Change> = self
            .our_unsent()
            .into_iter()
            .take(STREAM_BATCH)
            .collect();
        if let Some(live) = self.live.as_ref() {
            crate::applog::applog!(
                "live out: offering {} change(s) after cursor {after}",
                batch.len()
            );
            live.send_changes(after, &batch);
            // The moment it went, not merely that it did. A marker with no
            // moment on it can only be cleared by an answer, and a server that
            // never answers can then leave it set for the rest of the session.
            self.in_flight = Some(std::time::Instant::now());
            self.stream_due = None;
        }
    }

    /// Whether work held back can go in now.
    ///
    /// Two things hold work back, and both have to have cleared. An open cell
    /// editor, because a change to the task somebody is typing into would move
    /// the ground under them mid-word. And a push of this copy's own work that
    /// is still with the server, because until its answer lands the cursor has
    /// not moved past the work it carried, and anything the server echoed back
    /// in the meantime cannot yet be told from somebody else's.
    pub fn held_work_due(&self) -> bool {
        !self.held_live.is_empty() && self.editing.is_none() && !self.push_in_flight()
    }

    /// Whether a REST push of this copy's own work is out with the server.
    ///
    /// The window in which an echo of that push can arrive on the socket
    /// before the answer that would move the cursor past it. An up to date
    /// server never sends that echo, because the push names the connection to
    /// skip; an older one does, and this is what keeps it from being applied
    /// as though somebody else had made it.
    fn push_in_flight(&self) -> bool {
        matches!(self.working, Some(Working::Syncing))
    }

    /// Bring in the work that was waiting.
    pub fn apply_held_live(&mut self) {
        if !self.held_work_due() {
            return;
        }
        let held = std::mem::take(&mut self.held_live);
        // No head from an answer, because the answer that carried these went
        // by long ago. How far they read is worked out from the entries
        // themselves, which is the only honest source once they are applied
        // late: claiming the answer's head would skip whatever arrived between
        // then and now, and claiming nothing would have the server replay
        // these after the next reconnect.
        self.take_live_batch(&held, None);
    }

    /// Work that has been done here and not sent, from now on.
    ///
    /// Called wherever an entry lands in the log. The entry is what is offered
    /// and the entry is what is stored: there is no faster path that skips the
    /// record, because the record is the thing that makes a missed message
    /// recoverable rather than fatal.
    fn work_to_offer(&mut self) {
        if self.stream_due.is_none() {
            self.stream_due =
                Some(std::time::Instant::now() + Duration::from_millis(STREAM_AFTER_MILLIS));
        }
        self.touch_local_copy();
    }

    /// Bring one batch of live changes in.
    /// Bring one batch of live changes in.
    /// Which of these have not already been taken in.
    ///
    /// **What makes an incoming change recognisably one's own.** Not its id.
    /// The server renumbers every entry as it lands it: what a client sends as
    /// change 6 is stored as seq 43 and comes back to everybody, its author
    /// included, as change 43. So "have I seen this" cannot be answered by
    /// comparing an id the server chose against one this copy chose. Those are
    /// two different numbering schemes that happen to share a type, and asking
    /// the question that way is wrong in both directions at once: this copy's
    /// own work comes back wearing a number it has never used and is applied
    /// as though it were somebody else's, while somebody else's work whose seq
    /// happens to collide with an unsent local id is silently thrown away.
    ///
    /// The cursor is the answer, because the cursor is precisely the record of
    /// how far down the shared log this copy has read. Everything at or before
    /// it has been taken in, whoever wrote it and whatever this copy called it
    /// at the time; everything past it has not. Every entry that arrives from
    /// the server carries its seq as its id, so the comparison is exact and
    /// needs nothing remembered on the side.
    ///
    /// That covers all the ways one change can arrive twice: in a catch-up and
    /// again in the live stream, in a refusal and again as a broadcast, or
    /// pushed over REST and echoed back over a socket the server did not know
    /// to skip. The `connection` on the push stops that last one at the
    /// server; this stops it here as well, which is what keeps an older server
    /// merely older rather than corrupting.
    fn not_yet_taken(
        &self,
        incoming: &[aop_core::history::Change],
    ) -> Vec<aop_core::history::Change> {
        let taken_through = self.link.as_ref().map(|link| link.cursor).unwrap_or(0);
        let mut fresh: Vec<aop_core::history::Change> = Vec::new();
        for change in incoming {
            let seq = seq_of(change);
            if seq <= taken_through {
                continue;
            }
            // One batch can carry the same entry twice when a catch-up and the
            // live stream overlap, so the batch is deduplicated against itself
            // as well as against the cursor.
            if fresh.iter().any(|held| seq_of(held) == seq) {
                continue;
            }
            fresh.push(change.clone());
        }
        fresh
    }

    /// Put somebody else's entries in the log, on the sent side of the line.
    ///
    /// The log is the record, so their work belongs in it. What must not
    /// follow is this copy offering it back: it is already in the shared log,
    /// and appending it again puts a second copy of their edit in front of
    /// everybody, permanently.
    ///
    /// Two things keep it out of the offer, and both are needed. When nothing
    /// of this copy's own was waiting, the watermark can simply move past
    /// them, which is exact, needs no bookkeeping and survives being saved and
    /// reopened. When something of this copy's own was waiting, the watermark
    /// cannot move without marking that work as sent when it has not been, so
    /// the entries are named instead and left out of what is offered.
    fn take_into_the_log(&mut self, theirs: &[aop_core::history::Change]) {
        let nothing_of_ours_was_waiting = !self.have_unsent();
        self.project.history.merge(theirs.iter().cloned());

        let highest = theirs.iter().map(|change| change.id).max();
        match (nothing_of_ours_was_waiting, highest) {
            (true, Some(highest)) => self.project.history.mark_pushed(highest),
            _ => self
                .taken_from_server
                .extend(theirs.iter().map(|change| change.id)),
        }

        // The log drops its oldest entries once it is long enough, and a name
        // kept for an entry that no longer exists is a leak that grows for as
        // long as a session lasts.
        if !self.taken_from_server.is_empty() {
            let held: std::collections::HashSet<u64> = self
                .project
                .history
                .changes()
                .iter()
                .map(|change| change.id)
                .collect();
            self.taken_from_server.retain(|id| held.contains(id));
        }
    }

    /// The work this copy has done and not sent.
    ///
    /// What `History::unsent` says, less anything that came from the server.
    /// Everywhere that decides what to offer asks this rather than the log
    /// directly, because the log holds both copies' entries and only one of
    /// them is this copy's to offer.
    /// Whether there is any of it, without building the list.
    ///
    /// The predicate the timers ask several times a second. Cloning a run of
    /// entries to find out whether there are any would be a handful of string
    /// allocations per tick for a question with a one word answer.
    pub(crate) fn have_unsent(&self) -> bool {
        self.project
            .history
            .unsent()
            .iter()
            .any(|change| !self.taken_from_server.contains(&change.id))
    }

    pub(crate) fn our_unsent(&self) -> Vec<aop_core::history::Change> {
        self.project
            .history
            .unsent()
            .iter()
            .filter(|change| !self.taken_from_server.contains(&change.id))
            .cloned()
            .collect()
    }

    fn take_live_batch(&mut self, incoming: &[aop_core::history::Change], head: Option<i64>) {
        let fresh = self.not_yet_taken(incoming);
        // How far the log has actually been read once these are in. Worked out
        // from the entries themselves rather than taken on trust from the
        // answer that carried them, because work held back for an open cell
        // editor is applied later with no answer beside it, and a cursor that
        // never moved past it would have the server replay it after the next
        // reconnect.
        let reached = head
            .into_iter()
            .chain(fresh.iter().map(seq_of).max())
            .max();
        if fresh.is_empty() {
            if let Some(head) = head {
                self.remember_cursor(head);
            }
            return;
        }

        let (differences, replayed, asked) = self.preview_incoming(&fresh);

        // A clean rebase applies quietly, and that is the decision: with
        // streaming, "behind" is the ordinary state rather than an event, and
        // a modal every few seconds would make live editing worse than not
        // having it. The exception is a change to the very task somebody has a
        // cell open on, which would move the ground under them mid-word. That
        // waits until they are done. Anything that will not apply cleanly is
        // still refused outright, which is the rule that has not changed.
        if let Some(row) = self.editing.map(|(row, _)| row)
            && let Some(id) = self.project.tasks.get(row).map(|task| task.id)
            && differences.iter().any(|one| one.task() == Some(id))
        {
            self.held_live.extend(fresh);
            return;
        }

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

        self.take_into_the_log(&fresh);
        self.touch_local_copy();
        self.undo.push(before);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        if let Some(reached) = reached {
            self.remember_cursor(reached);
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
                self.file_path = Some(written.clone());
                self.backstage = None;

                // A plan saved somewhere new takes its versions with it: they
                // are keyed by where the plan lives, and Save As is the plan
                // moving rather than a different plan.
                if first_save {
                    self.moved_to(&written);
                }
                self.keep_version(aop_core::versions::Taken::Save);

                // The plan is on disk now, so the snapshot has nothing left to
                // give back and would only turn up as a false alarm later.
                crate::recovery::discard();

                // After the link has settled, because `moved_to` is what
                // decides which plan on the server this file now is.
                self.offer_after_save();

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

    /// Offer what this plan has not sent, because it has just been saved.
    ///
    /// Saving a shared plan and having nothing leave the machine is the
    /// surprise this removes: somebody with live editing off could press Save
    /// all afternoon and every one of those saves stayed on the disk in front
    /// of them. A save is a decision about the work, so it is the right moment
    /// to offer it.
    ///
    /// **Nothing here can make a save fail.** The file is written, the save
    /// point is marked and the recent list is updated before this is reached,
    /// and neither branch touches a network: one moves a debounce forward and
    /// the other sets a flag a timer reads. A server that is down, an account
    /// that is signed out, and a machine with no network at all all end the
    /// same way, which is the way they end today: the work stays in the log,
    /// unsent, waiting for the next opportunity. Saving is a local promise;
    /// syncing is a best effort laid on top of it, and neither may become
    /// conditional on the other.
    ///
    /// An unlinked plan has nowhere to send anything, so nothing happens.
    fn offer_after_save(&mut self) {
        if self.link.is_none() || !self.have_unsent() {
            return;
        }
        if self.streams_out() {
            // A socket is open and the server understands the message, so the
            // only thing between the work and the wire is the debounce.
            // Bringing it forward is what makes Save mean now rather than in a
            // quarter of a second.
            self.stream_due = Some(std::time::Instant::now());
            return;
        }
        // No socket, or one that cannot carry work. The ordinary sync goes
        // instead, started from a timer rather than from here.
        self.sync_after_save = true;
    }

    /// Whether the live socket is one this copy's own work can go out on.
    ///
    /// Two separate facts: whether there is a connection, and whether the
    /// server on the end of it understands work offered over it. An older
    /// server is happy to hold the socket and relay everybody else's edits,
    /// which is worth having and is why the session is not refused outright.
    fn streams_out(&self) -> bool {
        self.live.is_some() && self.stream_out
    }

    /// Whether a save has asked for a sync and the moment has come to start
    /// one.
    ///
    /// Asked before a write handle is taken, like everything else read from a
    /// timer: a plan nobody is editing must not redraw the window to be told
    /// there is nothing to do.
    pub fn sync_after_save_due(&self) -> bool {
        self.sync_after_save
            && self.sync_blocked().is_none()
            && self.have_unsent()
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
            Column::Successors => self.project.successor_text(task.id),
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
            Column::Successors => {
                let id = self.project.tasks[row].id;
                self.project.set_successor_text(id, &value);
            }
            Column::Resources => {
                self.project.set_resource_text(row, &value);
            }
        }

        self.editing = None;
        self.reschedule();

        // A link edit is the one cell that can create a loop; roll it back.
        // Either end of a link can close one, so both cells are checked.
        if matches!(column, Column::Predecessors | Column::Successors)
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

    /// Add or replace the link that runs from the task on `row` into
    /// `successor`.
    ///
    /// Deliberately no more than a turn of the same handle: adding a successor
    /// to A is creating the link that makes A a predecessor of B, so this
    /// resolves which row B is on and hands the work to `set_link`. One
    /// representation, one rollback on a loop, one entry in the log, whichever
    /// end the planner was looking at when they made the change.
    pub fn set_successor_link(
        &mut self,
        row: usize,
        successor: TaskId,
        kind: LinkType,
        lag_minutes: i64,
    ) {
        let Some(predecessor) = self.project.tasks.get(row).map(|t| t.id) else {
            return;
        };
        let Some(into) = self.row_of(successor) else {
            return;
        };
        self.set_link(into, predecessor, kind, lag_minutes);
    }

    /// Take off the link that runs from the task on `row` into `successor`.
    pub fn remove_successor_link(&mut self, row: usize, successor: TaskId) {
        let Some(predecessor) = self.project.tasks.get(row).map(|t| t.id) else {
            return;
        };
        let Some(into) = self.row_of(successor) else {
            return;
        };
        self.remove_link(into, predecessor);
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

    /// The visible width of the table pane.
    ///
    /// Whatever the splitter was dragged to, and not capped at the width the
    /// columns happen to need. Capping it there made the handle travel much
    /// further one way than the other: the table could be squeezed to almost
    /// nothing but never widened past its own content, so the two panes did
    /// not expand alike. Past the last column the pane simply shows the space,
    /// which is what dragging a divider that far is asking for.
    pub fn table_view_width(&self) -> f64 {
        self.table_pane_width
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
        static MOVES: crate::applog::Tally =
            crate::applog::Tally::new("bar drag: moving", crate::applog::HEARTBEAT_MILLIS);
        if let Some(drag) = &mut self.bar_drag {
            drag.delta_x = x - drag.origin_x;
            MOVES.note(format_args!("delta {:.0}px", drag.delta_x));
        } else {
            MOVES.note(format_args!("pointer moved with no drag running"));
        }
    }

    pub fn set_bar_hover(&mut self, row: usize) {
        if let Some(drag) = &mut self.bar_drag
            && drag.kind == BarDragKind::Link {
                drag.hover_row = Some(row);
            }
    }

    pub fn cancel_bar_drag(&mut self) {
        if self.bar_drag.is_some() {
            crate::applog::applog!("bar drag: cancelled");
        }
        self.bar_drag = None;
    }

    /// Apply whatever the drag was doing.
    pub fn finish_bar_drag(&mut self, px_per_day: f64) {
        let Some(drag) = self.bar_drag.take() else {
            crate::applog::applog!("bar drag: released with nothing running");
            return;
        };
        crate::applog::applog!(
            "bar drag: released after {:.0}px, {:?} on row {}",
            drag.delta_x,
            drag.kind,
            drag.row
        );
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
            Column::Successors => self.project.successor_text(task.id),
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
    } else {
        // The setting is off, so nothing is read and nobody is signed in. That
        // looks exactly like a session that would not open, which is why it
        // says so rather than leaving the log silent.
        crate::applog::applog!("restore: Collaborate is switched off, so no session is read");
    }
    // Worked out before anything else can write a preference, since the answer
    // depends on the version this copy last ran as and that is recorded here.
    state.begin_greetings();
    // One argument, and what it is decides what happens to it. Read the same
    // way the handoff reads it, so that opening a plan on a launch of its own
    // and handing one to a launch already running cannot disagree about what
    // the argument was.
    //
    // No question about unsaved work here, and none is needed: this runs
    // before there is anything on screen to lose.
    match std::env::args()
        .nth(1)
        .as_deref()
        .and_then(crate::handoff::Handed::from_argument)
    {
        Some(crate::handoff::Handed::Link(link)) => {
            state.splash = false;
            state.open_link_asked(&link);
        }
        Some(crate::handoff::Handed::Path(path)) => {
            state.splash = false;
            state.open_any(path);
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
        Column::Successors => Field::Successors,
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
    crate::settings::config_root().map(|dir| dir.join("recent.json"))
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

/// A name beside `path` that is not taken.
///
/// `plan.aprj` becomes `plan (1).aprj`, then `plan (2).aprj`, which is what
/// every file manager does and therefore what people expect. The number goes
/// before the extension, not after: `plan.aprj (1)` is not a plan file any
/// more, and the application would refuse to open it.
///
/// Bounded, because a folder containing every name up to the bound is a
/// stranger situation than one more overwrite prompt, and an unbounded loop
/// on a filesystem that answers slowly is a hang.
pub fn free_name_beside(path: &std::path::Path) -> Option<PathBuf> {
    if !path.exists() {
        return Some(path.to_path_buf());
    }
    let parent = path.parent().unwrap_or(std::path::Path::new("."));
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let extension = path.extension().map(|e| e.to_string_lossy().to_string());

    for n in 1..=999u32 {
        let mut name = format!("{stem} ({n})");
        if let Some(extension) = &extension {
            name.push('.');
            name.push_str(extension);
        }
        let candidate = parent.join(name);
        if !candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// The default folder the Save As and Open panes start in.
///
/// `HOME` is a Unix variable. Windows does not set it and uses `USERPROFILE`,
/// so asking for `HOME` alone fell through to `.`, the working directory,
/// which is wherever the application happened to be started from and holds
/// nobody's plans. Both are asked for, in the order that suits the platform.
///
/// The Documents folder is preferred but not insisted on: it can be renamed,
/// redirected to a network share, or simply absent, and starting in a folder
/// that does not exist shows an empty list with nothing to say why. Falling
/// back to the home folder is worse than Documents and much better than
/// nothing.
pub fn home_dir() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME", "USERPROFILE"]
    };
    for name in names {
        if let Some(value) = std::env::var_os(name) {
            let path = PathBuf::from(value);
            if !path.as_os_str().is_empty() {
                return Some(path);
            }
        }
    }
    // Windows splits it in two when it is not set as one.
    match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
        (Some(drive), Some(rest)) => {
            let mut path = drive;
            path.push(rest);
            Some(PathBuf::from(path))
        }
        _ => None,
    }
}

pub fn documents_dir() -> PathBuf {
    let Some(home) = home_dir() else {
        return PathBuf::from(".");
    };
    let documents = home.join("Documents");
    if documents.is_dir() { documents } else { home }
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
    fn a_plan_handed_over_by_another_launch_still_asks_about_unsaved_work() {
        // A plan double clicked in the file manager goes to the copy that is
        // already running, and it arrives by a different road from every other
        // way of opening one. It must not arrive by a road that goes around
        // the question: an afternoon's unsaved work is no less at stake for
        // having been interrupted from outside the window.
        let mut state = AppState::new();
        state.dirty = true;
        let handed = PathBuf::from("/home/ada/plans/bridge.aprj");
        state.guard(PendingAction::Open(handed.clone()));
        assert!(
            matches!(
                &state.dialog,
                Some(Dialog::UnsavedChanges(PendingAction::Open(path))) if *path == handed
            ),
            "the question is asked, and remembers which plan it was asked about"
        );
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

    // ---- saving a shared plan sends it ------------------------------------

    /// A linked plan with somebody signed in, which is what a sync needs
    /// before it can be more than a wish.
    fn signed_in() -> AppState {
        let mut state = linked();
        state.account = Some(crate::cloud::Account {
            subject: "ada".into(),
            name: "Ada".into(),
            email: "ada@example.test".into(),
            picture: None,
        });
        state
    }

    /// The offer a save makes, exercised on its own.
    ///
    /// `save_to` is not called here, and deliberately: a real save writes a
    /// recent list, a version store and a recovery marker into whichever
    /// configuration directory the machine running the tests happens to have,
    /// which is why the save tests above only ever exercise the failing path.
    /// What is checked here is the decision the save makes, which is the part
    /// that can be wrong.
    #[test]
    fn saving_a_linked_plan_offers_what_it_has_not_sent() {
        // The whole complaint: live editing off, a shared plan, and Save
        // pressed all afternoon while nothing left the machine.
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        assert_eq!(state.project.history.unsent().len(), 1);

        state.offer_after_save();

        assert!(state.sync_after_save, "the save asked for it to go");
        assert!(
            state.sync_after_save_due(),
            "and the timer that starts a worker can see that it should",
        );
    }

    #[test]
    fn saving_a_plan_that_is_on_no_server_asks_for_nothing() {
        let mut state = signed_in();
        state.link = None;
        state.append_task("Pour the foundations");

        state.offer_after_save();

        assert!(!state.sync_after_save, "there is nowhere to send it");
    }

    #[test]
    fn a_save_with_no_network_still_saves_and_leaves_the_work_unsent() {
        // The rule that matters most. A save is a local promise: the file, the
        // save point and the recent list do not depend on a server being
        // there. Syncing is a best effort laid on top, and when it cannot
        // happen the work stays in the log exactly as it does today.
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        let unsent_before = state.project.history.unsent().len();

        // Nothing is reachable: no account, which is what a machine with no
        // network looks like from here after a session has expired.
        state.account = None;
        state.offer_after_save();

        assert!(
            state.sync_after_save,
            "the wish is remembered, so the next opportunity takes it",
        );
        assert!(
            !state.sync_after_save_due(),
            "but nothing is started, because there is nothing to start it against",
        );
        assert_eq!(
            state.project.history.unsent().len(),
            unsent_before,
            "and the work is still unsent, which is what makes it recoverable",
        );
        assert!(state.dialog.is_none(), "and nothing was put in front of anybody");
    }

    #[test]
    fn a_sync_a_save_asked_for_is_not_lost_when_it_cannot_be_started() {
        // The wish is cleared when the work is taken, not when it is asked
        // for. A sync that never got off the ground has sent nothing, so the
        // note has to survive for the next opportunity to read it.
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        state.offer_after_save();

        assert!(state.start_sync().is_none(), "there is no session to hand over");
        assert!(state.sync_after_save, "so the save's wish is still standing");
    }

    #[test]
    fn a_save_does_not_start_a_second_conversation_with_the_server() {
        // One at a time. A sync running beside another would offer the same
        // commands twice, and the second offer would be told it is behind by
        // its own first one.
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        state.offer_after_save();
        state.working = Some(Working::Syncing);

        assert!(!state.sync_after_save_due(), "it waits for the one already going");
        assert!(state.sync_after_save, "and is not forgotten while it waits");
    }

    #[test]
    fn a_plan_fetched_from_the_server_is_brought_up_to_the_end_of_its_log() {
        // What the server sends is a snapshot as of some seq plus every entry
        // appended since, and the second half is commands. Recording that they
        // happened is not the same as having them happen: without this the
        // plan on screen is the plan as it stood when the snapshot was stored,
        // by as many entries as the server lets the log run past one.
        let mut state = linked();
        let tail = vec![theirs(70, "append_task(\"Theirs\");", "Added Theirs")];

        let drifted = state.catch_up_to(&tail);

        assert!(drifted.is_none(), "it replayed: {drifted:?}");
        assert!(
            state.project.tasks.iter().any(|task| task.name == "Theirs"),
            "the tail is part of the plan, not just part of its story",
        );
        assert!(
            state.project.history.unsent().is_empty(),
            "and replaying somebody else's work writes no entry of this copy's own",
        );
    }

    #[test]
    fn work_taken_from_the_server_is_never_offered_back_to_it() {
        // The third way one edit becomes two, and the worst of them, because
        // it writes the duplicate into the shared log where every copy gets
        // it. An entry taken from the server is merged into this copy's log
        // under the id the server gave it, and `unsent` is everything in the
        // log past a watermark that counts in this copy's own ids. Those are
        // two numbering schemes again, so somebody else's work lands on the
        // unsent side of the line and is offered straight back.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.append_task("Mine");
        let mine = state
            .project
            .history
            .unsent()
            .first()
            .map(|change| change.id)
            .expect("the edit was written down");
        state.sync_landed(Ok(Pushed::Applied {
            head: 5,
            applied: vec![(mine, 5)],
            snapshot_wanted: false,
        }));
        assert!(state.our_unsent().is_empty(), "everything of ours has gone");

        state.take_live_arrivals(vec![Incoming::Change {
            seq: 6,
            change: theirs(6, "append_task(\"Theirs\");", "Added Theirs"),
        }]);

        assert!(
            state.project.tasks.iter().any(|task| task.name == "Theirs"),
            "their work is in",
        );
        assert!(
            state.our_unsent().is_empty(),
            "and none of it is waiting to be sent back to them",
        );
    }

    #[test]
    fn our_own_work_still_goes_when_theirs_arrives_beside_it() {
        // The other side of the same line. Withholding somebody's own edit
        // because an entry of theirs happened to be merged next to it would
        // trade a duplicate for a loss, which is a worse bargain.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.append_task("Mine");
        let waiting = state.our_unsent().len();
        assert_eq!(waiting, 1);

        state.take_live_arrivals(vec![Incoming::Change {
            seq: 5,
            change: theirs(5, "append_task(\"Theirs\");", "Added Theirs"),
        }]);

        let ours = state.our_unsent();
        assert_eq!(ours.len(), 1, "still exactly one thing of ours to send");
        assert_eq!(ours[0].id, {
            state
                .project
                .history
                .changes()
                .iter()
                .find(|change| change.summary.contains("Mine"))
                .map(|change| change.id)
                .unwrap_or_default()
        });
    }

    // ---- not applying the same work twice --------------------------------

    #[test]
    fn a_push_names_the_socket_it_must_not_be_echoed_to() {
        // The field the client simply never filled in. The server reads it,
        // defaulted it to nothing, and so broadcast every synced change back
        // to the very copy that made it.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.take_live_arrivals(vec![Incoming::Welcome {
            head: 4,
            peers: Vec::new(),
            connection: Some(11),
        }]);

        let offer = state.offer_of(Vec::new()).expect("this plan is on a server");
        assert_eq!(offer.connection, Some(11));
    }

    #[test]
    fn a_push_with_no_socket_open_names_no_connection() {
        // And an older server that has never heard of the field sees the body
        // it already expects, because nothing is sent in place of it.
        let state = signed_in();
        let offer = state.offer_of(Vec::new()).expect("this plan is on a server");
        assert_eq!(offer.connection, None);
    }

    #[test]
    fn the_socket_handle_goes_when_the_socket_does() {
        // It named a connection that has ended. Sending it on a later push
        // would name one that is not there, and at worst somebody else's.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.take_live_arrivals(vec![Incoming::Welcome {
            head: 4,
            peers: Vec::new(),
            connection: Some(11),
        }]);
        state.stop_live(None);

        assert_eq!(state.offer_of(Vec::new()).and_then(|offer| offer.connection), None);
    }

    #[test]
    fn a_sync_during_a_live_session_does_not_change_the_plan() {
        // The corruption itself. Work is pushed over REST, the server echoes
        // it to this copy's own socket, and it arrives renumbered so it no
        // longer looks like this copy's own. Applying it again duplicates the
        // task, because `append_task` is not a no-op the second time.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        let mine = state
            .project
            .history
            .unsent()
            .first()
            .map(|change| change.id)
            .expect("the edit was written down");
        let tasks_after_editing = state.project.tasks.len();

        // The push lands: the entry this copy called `mine` is seq 5 now.
        state.sync_landed(Ok(Pushed::Applied {
            head: 5,
            applied: vec![(mine, 5)],
            snapshot_wanted: false,
        }));

        // And the server, not knowing to skip this connection, sends it back.
        state.take_live_arrivals(vec![Incoming::Change {
            seq: 5,
            change: Change {
                id: 5,
                at: Local::now().naive_local(),
                author: "Ada".into(),
                script: "append_task(\"Pour the foundations\");".into(),
                summary: "Added Pour the foundations".into(),
            },
        }]);

        assert_eq!(
            state.project.tasks.len(),
            tasks_after_editing,
            "the work went once, so it is in the plan once",
        );
    }

    #[test]
    fn work_already_read_is_not_applied_again_whatever_id_it_carries() {
        // A catch-up and the live stream can carry the same entry, and a
        // refusal and a broadcast can too. What settles it is the cursor:
        // everything at or before it has been read, whoever wrote it.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        let arrival = || Incoming::Catchup {
            head: 6,
            changes: vec![theirs(6, "append_task(\"Theirs\");", "Added Theirs")],
        };

        state.take_live_arrivals(vec![arrival()]);
        let after_first = state.project.tasks.len();
        assert_eq!(state.link.as_ref().map(|link| link.cursor), Some(6));

        state.take_live_arrivals(vec![arrival()]);

        assert_eq!(
            state.project.tasks.len(),
            after_first,
            "the second delivery is the same entry, not a second one",
        );
    }

    #[test]
    fn somebody_elses_work_is_not_thrown_away_for_sharing_a_number_with_ours() {
        // The other half of the same mistake, and the one that showed up as a
        // table that would not update. Ids from the server and ids this copy
        // chose are two numbering schemes sharing a type: a copy with five
        // unsent edits of its own has an entry called 5, and somebody else's
        // work landing at seq 5 was dropped as a duplicate of it.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        // Local ids run from wherever this copy's own counter has reached, and
        // the server's seqs run from the shared log. One of these copy's own
        // entries is bound to wear a number the server will hand out too.
        let mut mine = 0;
        for _ in 0..5 {
            mine = state.project.history.record(
                "Ada", "indent();", "Indented", Local::now().naive_local(),
            );
        }
        let collides = i64::try_from(mine).unwrap_or(0);
        state.link = Some(Link { project: "a-project".into(), cursor: collides - 1 });

        state.take_live_arrivals(vec![Incoming::Change {
            seq: collides,
            change: theirs(mine, "append_task(\"Theirs\");", "Added Theirs"),
        }]);

        assert!(
            state.project.tasks.iter().any(|task| task.name == "Theirs"),
            "their work is past the cursor, so it is theirs and it is new",
        );
    }

    #[test]
    fn an_arrival_during_a_push_waits_for_that_push_to_be_answered() {
        // The window an older server can slip an echo through: it broadcasts
        // when it commits, and the answer that moves the cursor past that work
        // arrives afterwards. Until it does, an entry cannot be told from
        // somebody else's, so it waits rather than being guessed at.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        state.working = Some(Working::Syncing);

        state.take_live_arrivals(vec![Incoming::Change {
            seq: 5,
            change: theirs(5, "append_task(\"Theirs\");", "Added Theirs"),
        }]);

        assert!(!state.held_live.is_empty(), "parked rather than applied");
        assert!(!state.held_work_due(), "and it stays parked while the push is out");
        assert_eq!(
            state.link.as_ref().map(|link| link.cursor),
            Some(4),
            "and nothing claims to have been read that has not been",
        );
    }

    #[test]
    fn work_held_for_an_open_editor_moves_the_cursor_when_it_finally_lands() {
        // Applied late, with no answer beside it to take a head from. A cursor
        // left where it was would have the server replay this after the next
        // reconnect, which is the duplicate arriving by another road.
        use crate::cloud::live::Incoming;
        let mut state = signed_in();
        state.append_task("Pour the foundations");
        state.editing = Some((0, Column::Name));
        state.take_live_arrivals(vec![Incoming::Change {
            seq: 6,
            change: theirs(6, "set_field(1, Name, \"Pour the piles\");", "Renamed"),
        }]);
        assert!(!state.held_live.is_empty(), "held while the cell is open");

        state.editing = None;
        state.apply_held_live();

        assert_eq!(
            state.link.as_ref().map(|link| link.cursor),
            Some(6),
            "read this far, and says so, so it is never sent again",
        );
    }

    // ---- streaming, both ways -------------------------------------------

    #[test]
    fn an_edit_arms_the_stream_and_lands_in_the_log_first() {
        // The log is the record and the log is what streams. There is no
        // quicker path that skips it: an entry nobody wrote down is one a
        // client that missed the message can never be told about.
        let mut state = linked();
        assert!(state.project.history.unsent().is_empty());

        state.append_task("Pour the foundations");

        assert_eq!(state.project.history.unsent().len(), 1, "written down first");
        assert!(state.stream_due.is_some(), "and waiting to go");
        // Not yet, because there is no socket. The work simply waits, which is
        // what a dropped socket looks like too.
        assert!(!state.stream_due(), "nothing streams without a session");
    }

    #[test]
    fn work_the_socket_took_is_marked_as_sent_by_the_id_that_came_back() {
        // By the local id the server acknowledged rather than by counting: an
        // answer that came back out of order must not mark work nobody has
        // seen as sent.
        use crate::cloud::live::Incoming;
        let mut state = linked();
        let first = state.project.history.record(
            "Ada", "append_task(\"A\");", "Added A", Local::now().naive_local(),
        );
        state.in_flight = Some(std::time::Instant::now());

        state.take_live_arrivals(vec![Incoming::Applied {
            head: 8,
            applied: vec![(first, 8)],
            snapshot_wanted: false,
        }]);

        assert!(state.project.history.unsent().is_empty(), "it went");
        assert_eq!(state.link.as_ref().map(|link| link.cursor), Some(8));
        assert!(state.in_flight.is_none(), "and the socket is free to carry the next batch");
    }

    #[test]
    fn a_streamed_push_that_was_beaten_to_it_keeps_its_work_and_offers_it_again() {
        // Nothing is written on a refusal, so the work is still unsent and
        // still in the log. What came back is what was missed, and it goes in
        // the same way a live change does.
        use crate::cloud::live::Incoming;
        let mut state = linked();
        let mine = state.project.history.record(
            "Ada", "append_task(\"Mine\");", "Added Mine", Local::now().naive_local(),
        );
        state.in_flight = Some(std::time::Instant::now());

        state.take_live_arrivals(vec![Incoming::Behind {
            head: 6,
            changes: vec![theirs(90, "append_task(\"Theirs\");", "Added Theirs")],
            more: false,
        }]);

        assert!(
            state.project.history.unsent().iter().any(|change| change.id == mine),
            "a refusal writes nothing, so this is still waiting to go",
        );
        assert!(state.stream_due.is_some(), "and is offered again once theirs is in");
        assert!(state.in_flight.is_none());
        assert!(
            state.project.tasks.iter().any(|task| task.name == "Theirs"),
            "their work applied quietly, because it applied cleanly",
        );
        assert!(state.dialog.is_none(), "and without a modal in front of somebody typing");
    }

    #[test]
    fn a_server_that_does_not_speak_streaming_is_said_so_and_not_streamed_to() {
        // The other half of the same problem, caught before anything is sent
        // rather than after nothing comes back. Such a server is happy to hold
        // the socket and relay everybody else's edits, so the session is worth
        // having; what it will not do is take this copy's work, and that is
        // the part that has to be said out loud.
        //
        // Loopback on a port nothing listens on, so the worker thread this
        // starts fails at once and no name is ever looked up.
        let mut state = signed_in();
        state.collaborate_server = "http://127.0.0.1:1".into();
        state.append_task("Pour the foundations");

        state.start_live("a-token".into(), crate::cloud::health::Speaks::NotStreaming);

        assert!(state.live.is_some(), "the socket is still worth having");
        assert!(!state.streams_out(), "but nothing of this copy's own goes down it");
        assert!(!state.stream_due(), "so no batch is ever offered, and none can hang");
        assert!(
            state.cloud_message.as_deref().is_some_and(|said| {
                said.contains("does not understand") && said.contains("press Sync")
            }),
            "said plainly, and with what does work: got {:?}",
            state.cloud_message,
        );
        assert!(
            state.sync_after_save_due(),
            "and the work is routed the way that does reach this server",
        );

        // Once. It is said when the session starts, which happens when
        // somebody asks for one, rather than by anything on a timer.
        state.cloud_message = None;
        state.gave_up_on_batch();
        assert!(state.cloud_message.is_none(), "nothing repeats it");
    }

    #[test]
    fn a_batch_nobody_answers_is_given_up_on_and_the_next_one_is_offered() {
        // This is the failure that cost an afternoon. A batch went to a server
        // that did not understand the message, the server said nothing, and
        // the in-flight marker stayed set. From that moment nothing else
        // streamed, for the rest of the session, silently.
        let mut state = signed_in();
        let mine = state.project.history.record(
            "Ada", "append_task(\"Mine\");", "Added Mine", Local::now().naive_local(),
        );
        state.in_flight = Some(
            std::time::Instant::now() - Duration::from_secs(STREAM_ANSWER_SECONDS + 1),
        );
        assert!(state.stream_unanswered(), "long enough to be a fault rather than a wait");

        state.gave_up_on_batch();

        assert!(state.in_flight.is_none(), "the socket is free to carry the next batch");
        assert!(
            state.project.history.unsent().iter().any(|change| change.id == mine),
            "nothing was ever marked as sent, so the work is still here",
        );
        assert!(state.stream_due.is_some(), "and is offered again after a pause");
        assert!(
            state.cloud_message.is_some_and(|said| said.contains("not answered")),
            "and somebody is told, rather than left to discover it",
        );
    }

    #[test]
    fn a_connection_that_keeps_saying_nothing_is_complained_about_once() {
        // The condition persists and the timer that notices it runs several
        // times a second. A message repeated at that rate is one somebody
        // turns off rather than reads.
        let mut state = signed_in();
        state.project.history.record(
            "Ada", "append_task(\"Mine\");", "Added Mine", Local::now().naive_local(),
        );
        let long_ago =
            || std::time::Instant::now() - Duration::from_secs(STREAM_ANSWER_SECONDS + 1);

        state.in_flight = Some(long_ago());
        state.gave_up_on_batch();
        assert!(state.cloud_message.is_some(), "said the first time");

        state.cloud_message = None;
        state.in_flight = Some(long_ago());
        state.gave_up_on_batch();
        assert!(state.cloud_message.is_none(), "and not again");

        // Until the server starts answering, which makes the next silence
        // news again rather than more of the same.
        state.batch_answered();
        state.in_flight = Some(long_ago());
        state.gave_up_on_batch();
        assert!(state.cloud_message.is_some(), "a session that recovers and fails again says so");
    }

    #[test]
    fn a_streamed_push_onto_a_log_this_copy_is_past_is_refused_out_loud() {
        // Two logs that share numbers but not events. There is no quiet
        // answer to this one, so live editing stops and the question is put.
        use crate::cloud::live::Incoming;
        let mut state = linked();
        state.take_live_arrivals(vec![Incoming::Ahead { head: 3, cursor: 7 }]);
        assert!(
            matches!(state.dialog, Some(Dialog::SyncAhead { head: 3, cursor: 7 })),
            "got {:?}", state.dialog,
        );
    }

    #[test]
    fn a_rebase_waits_rather_than_moving_the_ground_under_somebody_typing() {
        // With streaming, being behind is the ordinary state rather than an
        // event, so a clean rebase applies quietly. The exception is the task
        // somebody has a cell open on: that one waits until they are done,
        // and nothing is lost by waiting because the cursor does not move
        // until it lands.
        use crate::cloud::live::Incoming;
        let mut state = linked();
        state.append_task("Pour the foundations");
        state.append_task("Cure");
        state.editing = Some((0, Column::Name));
        let cursor_was = state.link.as_ref().map(|link| link.cursor);

        state.take_live_arrivals(vec![Incoming::Change {
            seq: 6,
            change: theirs(
                91,
                "set_field(1, Name, \"Pour the piles\");",
                "Changed a duration",
            ),
        }]);


        assert!(!state.held_live.is_empty(), "held while the cell is open");
        assert_eq!(state.link.as_ref().map(|link| link.cursor), cursor_was,
                   "and the cursor stays put, so nothing is claimed as read");
        assert!(!state.stream_due(), "nor is anything of this copy's offered past it");

        // The editor closes, and it goes in exactly as it would have.
        state.editing = None;
        assert!(state.held_work_due());
        state.apply_held_live();
        assert!(state.held_live.is_empty(), "in it goes");
    }

    #[test]
    fn a_dropped_socket_keeps_the_work_it_never_managed_to_send() {
        // The other half of the recovery story. The catch-up brings in what
        // was missed; this is what makes sure nothing of this copy's own is
        // lost while there was nowhere to send it.
        let mut state = linked();
        state.append_task("Pour the foundations");
        state.in_flight = Some(std::time::Instant::now());

        state.stop_live(Some("Live editing stopped.".into()));

        assert_eq!(state.project.history.unsent().len(), 1, "still waiting");
        assert!(state.in_flight.is_none(), "and no longer thought to be in flight");
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

    #[test]
    fn a_start_folder_is_found_from_whichever_variable_this_platform_sets() {
        // The bug this pins: `documents_dir` asked only for HOME, which is a
        // Unix variable. Windows sets USERPROFILE, so every Windows copy fell
        // through to ".", the working directory, and the Open page listed the
        // folder the application happened to be started from.
        let found = home_dir();
        assert!(
            found.is_some(),
            "no home could be found from HOME, USERPROFILE or HOMEDRIVE/HOMEPATH"
        );
        let start = documents_dir();
        assert_ne!(
            start,
            std::path::Path::new("."),
            "the working directory is never somebody's plans"
        );
        // Documents when it is there, the home folder when it is not, but
        // always somewhere that exists rather than a guess.
        assert!(start.is_dir(), "the start folder must exist: {start:?}");
    }

    // ---- successors -----------------------------------------------------
    //
    // A successor is not stored. Every test here checks that editing one end
    // moves the very link the other end is showing, because the moment there
    // are two representations a plan can contradict itself.

    /// Three unnested rows, so nothing is excluded for being somebody's child.
    fn three() -> AppState {
        let mut state = AppState::new();
        state.project.tasks.clear();
        state.project.links.clear();
        for name in ["A", "B", "C"] {
            state.project.push_task(name, MINUTES_PER_DAY);
        }
        state.reschedule();
        state
    }

    #[test]
    fn adding_a_successor_is_adding_the_predecessor_from_the_far_end() {
        let mut state = three();
        let (a, b) = (state.project.tasks[0].id, state.project.tasks[1].id);

        state.set_successor_link(0, b, LinkType::FS, 0);

        assert_eq!(state.project.links.len(), 1, "one link, not one per end");
        assert!(state.project.link_exists(a, b));
        // What B's own Predecessors view reads, unchanged code path.
        assert_eq!(state.cell_text(1, Column::Predecessors), "1");
        assert_eq!(state.cell_text(0, Column::Successors), "2");
    }

    #[test]
    fn the_same_relationship_cannot_be_added_twice_from_opposite_ends() {
        let mut state = three();
        let (a, b) = (state.project.tasks[0].id, state.project.tasks[1].id);

        state.set_successor_link(0, b, LinkType::FS, 0);
        state.set_link(1, a, LinkType::FS, 0);

        assert_eq!(state.project.links.len(), 1);
    }

    #[test]
    fn editing_the_type_or_lag_from_either_end_moves_one_link() {
        let mut state = three();
        let (a, b) = (state.project.tasks[0].id, state.project.tasks[1].id);
        state.set_link(1, a, LinkType::FS, 0);

        // From the successor end.
        state.set_successor_link(0, b, LinkType::SS, MINUTES_PER_DAY * 2);
        assert_eq!(state.project.links.len(), 1);
        assert_eq!(state.cell_text(1, Column::Predecessors), "1SS+2 days");

        // And from the predecessor end, over the top of it.
        state.set_link(1, a, LinkType::FF, -MINUTES_PER_DAY);
        assert_eq!(state.project.links.len(), 1);
        assert_eq!(state.cell_text(0, Column::Successors), "2FF-1 day");
    }

    #[test]
    fn removing_a_successor_removes_the_link_rather_than_orphaning_it() {
        let mut state = three();
        let (a, b) = (state.project.tasks[0].id, state.project.tasks[1].id);

        state.set_link(1, a, LinkType::FS, 0);
        state.remove_successor_link(0, b);
        assert!(state.project.links.is_empty(), "gone from both views at once");

        // And the other way round: made from the successor end, taken off from
        // the predecessor end.
        state.set_successor_link(0, b, LinkType::FS, 0);
        state.remove_link(1, a);
        assert!(state.project.links.is_empty());
    }

    #[test]
    fn a_successor_that_would_close_a_loop_is_refused() {
        // Adding a successor closes a loop exactly as readily as adding a
        // predecessor does, and a plan whose links form one cannot be
        // scheduled at all. It has to be rolled back and said out loud.
        let mut state = three();
        let (a, b) = (state.project.tasks[0].id, state.project.tasks[1].id);
        state.set_link(1, a, LinkType::FS, 0);
        assert!(state.dialog.is_none());

        // B before A, on top of A before B.
        state.set_successor_link(1, a, LinkType::FS, 0);

        assert!(!state.project.link_exists(b, a), "the loop was rolled back");
        assert!(state.project.link_exists(a, b), "and the good link survives");
        assert_eq!(state.project.links.len(), 1);
        assert!(
            matches!(&state.dialog, Some(Dialog::Message { title, .. }) if title.contains("link")),
            "the refusal is reported the same way either end reports it"
        );
    }

    #[test]
    fn a_typed_successor_cell_reads_the_same_language_as_a_predecessor_cell() {
        let mut state = three();
        state.commit_cell(0, Column::Successors, "2FS+2d,3SS");

        assert_eq!(state.project.links.len(), 2);
        assert_eq!(state.cell_text(1, Column::Predecessors), "1FS+2 days");
        assert_eq!(state.cell_text(2, Column::Predecessors), "1SS");
    }

    #[test]
    fn a_typed_successor_cell_that_would_close_a_loop_is_refused() {
        let mut state = three();
        state.commit_cell(0, Column::Successors, "2");
        assert!(state.dialog.is_none());

        state.commit_cell(1, Column::Successors, "1");

        assert_eq!(state.project.links.len(), 1, "rolled back to what worked");
        assert!(state.project.link_exists(
            state.project.tasks[0].id,
            state.project.tasks[1].id
        ));
        assert!(matches!(state.dialog, Some(Dialog::Message { .. })));
    }
}
