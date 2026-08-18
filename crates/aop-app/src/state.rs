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
    persist, schedule, templates, ConstraintType, Field, Link, LinkType, Project, ResourceId,
    ScheduleReport, Task, TaskId, TaskMode, WorkCalendar,
};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

use crate::gantt::{bar_edges, chart_range, Scale, ROW_H};

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
    Keyboard,
    CustomizeRibbon,
    QuickAccess,
}

impl OptionsPage {
    pub const ORDER: [OptionsPage; 8] = [
        OptionsPage::General,
        OptionsPage::Display,
        OptionsPage::Schedule,
        OptionsPage::Save,
        OptionsPage::Advanced,
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
}

impl QatCommand {
    pub const ALL: [QatCommand; 16] = [
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
}

impl PendingAction {
    /// What the buttons should say this will discard.
    pub fn describe(&self) -> &'static str {
        match self {
            PendingAction::Quit => "closing",
            PendingAction::CloseProject => "closing this plan",
            PendingAction::NewFromTemplate(_) => "starting a new plan",
            PendingAction::Open(_) => "opening another plan",
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

#[derive(Debug, Clone, PartialEq)]
pub struct RecentEntry {
    pub name: String,
    pub path: PathBuf,
}

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
    /// The row currently being dragged, and where it would land.
    pub drag_row: Option<usize>,
    pub drop_target: Option<(usize, DropWhere)>,
    /// A confirmation shown on the current Backstage page.
    pub backstage_message: Option<String>,
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
    /// Whether the spelling panel is open beside the plan.
    ///
    /// A panel rather than a view: correcting a word means seeing the row it is
    /// in, and a full-screen list of mistakes takes away the thing being
    /// corrected.
    pub spelling_open: bool,
    /// How long an iteration runs, for the burn charts and velocity. A team's
    /// cadence is not something to guess at, so it is settable.
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
            drag_row: None,
            drop_target: None,
            backstage_message: None,
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
        if self.macro_depth == 0 && std::mem::take(&mut self.reschedule_owed) {
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

    pub fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, previous));
            self.dirty = true;
            self.clamp_selection();
            self.reschedule();
            self.status = "Undo".into();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, next));
            self.dirty = true;
            self.clamp_selection();
            self.reschedule();
            self.status = "Redo".into();
        }
    }

    // ---- selection ------------------------------------------------------

    pub fn select(&mut self, index: usize) {
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
        self.reschedule();
        self.status = format!("Created {}", self.project.name);
    }

    pub fn open_path(&mut self, path: PathBuf) {
        match persist::open(&path) {
            Ok(project) => {
                self.project = project;
                self.dirty = false;
                self.undo.clear();
                self.redo.clear();
                self.selection = if self.project.tasks.is_empty() {
                    Vec::new()
                } else {
                    vec![0]
                };
                self.status = format!("Opened {}", path.display());
                self.push_recent(&path);
                self.file_path = Some(path);
                self.backstage = None;
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
    fn apply_settings(&mut self, settings: crate::settings::Settings) {
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
            keys: self.keys.clone(),
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

    pub fn save_to(&mut self, path: PathBuf) {
        match persist::save(&path, &self.project) {
            Ok(written) => {
                self.status = format!("Saved to {}", written.display());
                self.backstage_message = Some(format!("Saved {}", written.display()));
                self.dirty = false;
                self.push_recent(&written);
                self.file_path = Some(written);
                self.backstage = None;

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
            self.undo();
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
        for task in &mut self.project.tasks {
            task.collapsed = collapsed;
        }
    }

    pub fn copy_selected(&mut self) {
        self.clipboard = self
            .ordered_selection()
            .iter()
            .filter_map(|&i| self.project.tasks.get(i).cloned())
            .collect();
        self.status = format!("{} task(s) copied", self.clipboard.len());
    }

    pub fn cut_selected(&mut self) {
        self.copy_selected();
        self.delete_selected();
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
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
                self.undo();
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
            self.undo();
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
        self.checkpoint();
        self.project.unlink(predecessor, successor);
        self.reschedule();
    }

    /// Book or unbook a resource against an explicit row, with units.
    pub fn set_assignment(&mut self, row: usize, resource: ResourceId, units: Option<f64>) {
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
        self.checkpoint();
        self.project.add_resource(name);
        self.reschedule();
    }

    pub fn delete_resource(&mut self, index: usize) {
        let Some(id) = self.project.resources.get(index).map(|r| r.id) else {
            return;
        };
        self.checkpoint();
        self.project.delete_resource(id);
        self.selected_resource = None;
        self.reschedule();
    }

    pub fn commit_resource_cell(&mut self, index: usize, field: &str, value: &str) {
        if index >= self.project.resources.len() {
            return;
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
        self.checkpoint();
        if let Some(task) = self.project.tasks.get_mut(row) {
            task.assignments.retain(|a| a.resource != resource);
        }
        self.reschedule();
        self.dirty = true;
    }

    // ---- project commands -----------------------------------------------

    pub fn set_baseline(&mut self) {
        self.checkpoint();
        self.project.set_baseline();
        self.show_baseline = true;
        self.status = "Baseline saved".into();
    }

    pub fn clear_baseline(&mut self) {
        self.checkpoint();
        self.project.clear_baseline();
        self.show_baseline = false;
        self.status = "Baseline cleared".into();
    }

    pub fn set_project_start(&mut self, date: NaiveDateTime) {
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
        self.columns.insert(at, ColumnSpec::new(field));
        self.status = format!("Inserted the {} column", field.label());
    }

    pub fn remove_column(&mut self, index: usize) {
        if self.columns.len() <= 1 || index >= self.columns.len() {
            return;
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
        self.selection.clear();
        self.status = format!("Filter: {}", self.filter.label());
    }

    /// Fit the whole plan on screen by picking a timescale for its span.
    pub fn zoom_to_fit(&mut self) {
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
                self.checkpoint();
                self.project.add_link(Link::finish_to_start(from, to));
                self.reschedule();
                if let Some(error) = self.schedule_error() {
                    self.undo();
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
            self.undo();
            self.note("Nothing to fill. Those rows already match.");
            return;
        }
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
                self.undo();
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
                    self.undo();
                }
                self.dirty = summary.changed() > 0;
                self.dialog = None;
                self.status = summary.describe();
                self.reschedule();
            }
            Err(error) => {
                self.undo();
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
    if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from)
        && path.is_file() {
            state.splash = false;
            state.open_any(path);
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
    fn hover_is_not_part_of_the_plans_state() {
        // It lives in its own signal on purpose. Held on AppState, moving the
        // pointer across the chart invalidated the layout memo per bar, which
        // rebuilds a tick for every day of the plan to move a highlight.
        let state = AppState::new();
        let _ = state;
        // Nothing to assert on AppState itself, which is the point: the field
        // is gone. The behaviour is covered where it is used.
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
}
