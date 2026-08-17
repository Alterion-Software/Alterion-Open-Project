//! Title bar, Quick Access Toolbar, tab strip and the ribbon itself.
//!
//! Group and command names follow Microsoft Project 2021 so the muscle memory
//! carries over, minus the commands that only make sense against Microsoft's
//! own services. Every button drawn with a caret opens a real menu.

#[cfg(feature = "desktop")]
use dioxus::desktop::{use_window, WindowCloseBehaviour};
use dioxus::prelude::*;

use aop_core::{TaskMode, APP_NAME};

use crate::controls::{Choice, ComboBox, Dropdown, MenuBtn, MenuOption};
use crate::icons::icon;
use crate::state::{
    AppState, BackstagePage, Dialog, PaneFocus, PendingAction, QatCommand, RibbonTab, ViewKind, Zoom,
};

// ---------------------------------------------------------------- buttons

#[component]
fn BigBtn(glyph: String, caption: String, enabled: bool, on: EventHandler<()>) -> Element {
    let class = if enabled { "rbtn-lg" } else { "rbtn-lg disabled" };
    rsx! {
        button {
            class: "{class}",
            title: "{caption}",
            onclick: move |_| { if enabled { on.call(()) } },
            span { class: "glyph", {icon(&glyph, 28)} }
            span { class: "caption", "{caption}" }
        }
    }
}

#[component]
fn SmallBtn(glyph: String, caption: String, enabled: bool, on: EventHandler<()>) -> Element {
    let class = if enabled { "rbtn-sm" } else { "rbtn-sm disabled" };
    rsx! {
        button {
            class: "{class}",
            title: "{caption}",
            onclick: move |_| { if enabled { on.call(()) } },
            span { class: "glyph", {icon(&glyph, 16)} }
            span { class: "caption", "{caption}" }
        }
    }
}

#[component]
fn CheckItem(label: String, on_state: bool, on: EventHandler<()>) -> Element {
    let box_class = if on_state { "box on" } else { "box" };
    rsx! {
        div { class: "rcheck", onclick: move |_| on.call(()),
            span { class: "{box_class}", if on_state { "\u{2713}" } }
            span { "{label}" }
        }
    }
}

#[component]
fn Group(title: String, launcher: bool, children: Element) -> Element {
    rsx! {
        div { class: "rgroup",
            div { class: "rgroup-body", {children} }
            div { class: "rgroup-title",
                "{title}"
                if launcher { span { class: "launcher", "\u{2935}" } }
            }
        }
    }
}

// ------------------------------------------------------------ view menus

fn task_view_options() -> Vec<MenuOption> {
    vec![
        MenuOption::new("gantt", "Gantt Chart", "GanttChart"),
        MenuOption::new("tracking-gantt", "Tracking Gantt", "TrackingGantt"),
        MenuOption::new("task-usage", "Task Usage", "TaskUsage"),
        MenuOption::new("network", "Network Diagram", "NetworkDiagram"),
        MenuOption::new("calendar", "Calendar", "CalendarView"),
        MenuOption::new("task-sheet", "Task Sheet", "TaskSheet"),
        MenuOption::separator(),
        MenuOption::new("team-planner", "Team Planner", "TeamPlanner"),
        MenuOption::new("resource-sheet", "Resource Sheet", "ResourceSheet"),
        MenuOption::new("resource-usage", "Resource Usage", "ResourceUsage"),
    ]
}

fn resource_view_options() -> Vec<MenuOption> {
    vec![
        MenuOption::new("team-planner", "Team Planner", "TeamPlanner"),
        MenuOption::new("resource-sheet", "Resource Sheet", "ResourceSheet"),
        MenuOption::new("resource-usage", "Resource Usage", "ResourceUsage"),
        MenuOption::separator(),
        MenuOption::new("gantt", "Gantt Chart", "GanttChart"),
    ]
}

fn view_from(value: &str) -> Option<ViewKind> {
    Some(match value {
        "GanttChart" => ViewKind::GanttChart,
        "TrackingGantt" => ViewKind::TrackingGantt,
        "TaskSheet" => ViewKind::TaskSheet,
        "TaskUsage" => ViewKind::TaskUsage,
        "NetworkDiagram" => ViewKind::NetworkDiagram,
        "CalendarView" => ViewKind::CalendarView,
        "ResourceSheet" => ViewKind::ResourceSheet,
        "ResourceUsage" => ViewKind::ResourceUsage,
        "TeamPlanner" => ViewKind::TeamPlanner,
        _ => return None,
    })
}

// ---------------------------------------------------------------- chrome

#[component]
pub fn TitleBar() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (title, can_undo, can_redo, has_selection, qat) = {
        let s = state.read();
        (
            s.document_title(),
            s.can_undo(),
            s.can_redo(),
            !s.selection.is_empty(),
            s.qat.clone(),
        )
    };

    rsx! {
        div { class: "titlebar",
            div { class: "qat",
                for command in qat.iter().copied() {
                    {
                        let enabled = match command {
                            QatCommand::Undo => can_undo,
                            QatCommand::Redo => can_redo,
                            QatCommand::Link | QatCommand::Unlink | QatCommand::TaskInformation => has_selection,
                            _ => true,
                        };
                        rsx! {
                            button {
                                key: "{command:?}",
                                class: "qat-btn",
                                title: "{command.label()}",
                                disabled: !enabled,
                                onclick: move |_| run_qat(&mut state, command),
                                {icon(command.glyph(), 15)}
                            }
                        }
                    }
                }
                div { class: "qat-sep" }
                button { class: "qat-btn", title: "Customize Quick Access Toolbar",
                    onclick: move |_| state.write().dialog = Some(Dialog::CustomizeQat),
                    span { class: "caret", "\u{25be}" }
                }
            }

            WindowChrome { title: title.clone() }
        }
    }
}

/// The webview build draws its own title bar, drag region and window buttons,
/// because the operating system decorations are switched off.
#[cfg(feature = "desktop")]
#[component]
fn WindowChrome(title: String) -> Element {
    let window = use_window();
    let mut state = use_context::<Signal<AppState>>();

    // Refuse a close asked for from outside the application: a window manager
    // keybinding, Alt+F4, the compositor.
    //
    // This has to happen at the toolkit layer. The portable event loop runs
    // every handler and then carries the close out regardless, so the only
    // portable option is to hide the window and put it back, and a tiling
    // window manager treats that as the window closing and reopening: it
    // unmaps, the layout closes the gap, and the window comes back somewhere
    // else. GTK's own close signal can simply be refused, so the window is
    // never unmapped and never moves.
    #[cfg(target_os = "linux")]
    {
        use dioxus::core::Runtime;
        use dioxus::desktop::tao::platform::unix::WindowExtUnix;
        use gtk::glib;
        use gtk::prelude::*;

        let runtime = Runtime::current();
        let scope = dioxus::core::current_scope_id();
        let gtk_window = window.gtk_window().clone();

        use_hook(move || {
            // The toolkit's close signal already has a handler on it, put there
            // by the windowing layer, and it reports the close as handled. That
            // stops the signal, so a handler added afterwards is never reached.
            // Taking that one off is what makes refusing the close possible at
            // all; the application closes its own window by another route, so
            // nothing is lost by removing it.
            unsafe {
                use glib::translate::{IntoGlib, ToGlibPtr};
                let raw: *mut gtk::ffi::GtkApplicationWindow = gtk_window.to_glib_none().0;
                let object = raw as *mut glib::gobject_ffi::GObject;
                let signal = glib::gobject_ffi::g_signal_lookup(
                    c"delete-event".as_ptr() as *const _,
                    glib::prelude::ObjectExt::type_(&gtk_window).into_glib(),
                );
                if signal != 0 {
                    glib::gobject_ffi::g_signal_handlers_disconnect_matched(
                        object,
                        glib::gobject_ffi::G_SIGNAL_MATCH_ID,
                        signal,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                }
            }

            gtk_window.connect_delete_event(move |_, _| {
                // The signal fires outside any reactive scope, so one has to be
                // entered before the state can be touched.
                let refuse = runtime.in_scope(scope, || {
                    let mut state = state;
                    state.write().guard(PendingAction::Quit);
                    let settled = state.read().quit_requested;
                    !settled
                });
                if refuse {
                    // Refused: the window is never unmapped, so a tiling layout
                    // never sees it go, and it stays exactly where it was.
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        });
    }

    // Once the question has been answered, the window closes for real. This
    // path does not go through the toolkit signal above, so nothing refuses it.
    {
        let window = use_window();
        use_effect(move || {
            if state.read().quit_requested {
                window.set_close_behavior(WindowCloseBehaviour::WindowCloses);
                window.close();
            }
        });
    }

    rsx! {
        div {
            class: "drag-region",
            onmousedown: {
                let window = window.clone();
                move |_| window.drag()
            },
            ondoubleclick: {
                let window = window.clone();
                move |_| window.toggle_maximized()
            },
            div { class: "title-text", b { "{title}" } " \u{2014} {APP_NAME}" }
        }

        div { class: "wincontrols",
            button {
                class: "wc", title: "Minimize",
                onclick: {
                    let window = window.clone();
                    move |_| window.set_minimized(true)
                },
                {icon("win-min", 15)}
            }
            button {
                class: "wc", title: "Maximize",
                onclick: {
                    let window = window.clone();
                    move |_| window.toggle_maximized()
                },
                {icon("win-max", 13)}
            }
            button {
                class: "wc close", title: "Close",
                onclick: move |_| state.write().guard(PendingAction::Quit),
                {icon("win-close", 14)}
            }
        }
    }
}

/// The webview-free build keeps the operating system's own decorations, so it
/// only needs the document title here.
#[cfg(not(feature = "desktop"))]
#[component]
fn WindowChrome(title: String) -> Element {
    rsx! {
        div { class: "drag-region",
            div { class: "title-text", b { "{title}" } " \u{2014} {APP_NAME}" }
        }
    }
}

/// Run whatever a Quick Access Toolbar button stands for.
pub fn run_qat(state: &mut Signal<AppState>, command: QatCommand) {
    match command {
        QatCommand::New => state.write().backstage = Some(BackstagePage::New),
        QatCommand::Open => state.write().backstage = Some(BackstagePage::Open),
        QatCommand::Save => {
            let saved = state.write().save();
            if !saved {
                state.write().backstage = Some(BackstagePage::SaveAs);
            }
        }
        QatCommand::Print => state.write().backstage = Some(BackstagePage::Print),
        QatCommand::Export => state.write().backstage = Some(BackstagePage::Export),
        QatCommand::Undo => state.write().undo(),
        QatCommand::Redo => state.write().redo(),
        QatCommand::Link => state.write().link_selected(),
        QatCommand::Unlink => state.write().unlink_selected(),
        QatCommand::TaskInformation => {
            let row = state.read().primary();
            if let Some(row) = row {
                state.write().dialog = Some(Dialog::TaskInformation(row));
            }
        }
        QatCommand::AssignResources => {
            state.write().dialog = Some(Dialog::AssignResources)
        }
        QatCommand::ProjectInformation => {
            state.write().dialog = Some(Dialog::ProjectInformation)
        }
        QatCommand::SetBaseline => state.write().set_baseline(),
        QatCommand::ScrollToTask => state.write().scroll_to_task(),
        QatCommand::ZoomIn => {
            let z = state.read().zoom.zoom_in();
            state.write().zoom = z;
        }
        QatCommand::ZoomOut => {
            let z = state.read().zoom.zoom_out();
            state.write().zoom = z;
        }
    }
}

#[component]
pub fn TabStrip() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (tab, view, focus) = {
        let s = state.read();
        (s.tab, s.view, s.pane_focus)
    };

    // The contextual tools belong to the chart. With the table taking the whole
    // window there is no chart to format, so the banner and its tab go with it,
    // the way Office drops a contextual tab when its object is not in view.
    let chart_shown = !view.has_chart() || focus != PaneFocus::TableOnly;

    // The banner keeps its space when it has nothing to say. Taking it out of
    // the layout instead would shift the whole ribbon up and down every time
    // the chart is hidden or brought back.
    let banner_class = if chart_shown {
        "tools-banner"
    } else {
        "tools-banner empty"
    };

    rsx! {
        div { class: "{banner_class}",
            // The banner has to sit exactly above the Format tab. Rather than
            // guessing pixel widths, it repeats the tabs as hidden ghosts so
            // the label lands wherever Format actually is.
            span { class: "tab file ghost", "File" }
            for entry in RibbonTab::ORDER {
                span { key: "{entry:?}", class: "tab ghost", "{entry.label()}" }
            }
            if chart_shown {
                div { class: "tools-label", "{view.tools_label()}" }
            }
        }
        div { class: "tabstrip",
            button { class: "tab file",
                onclick: move |_| state.write().backstage = Some(BackstagePage::Info),
                "File"
            }
            for entry in RibbonTab::ORDER {
                {
                    let class = if tab == entry { "tab active" } else { "tab" };
                    rsx! {
                        button { key: "{entry:?}", class: "{class}",
                            onclick: move |_| state.write().tab = entry,
                            "{entry.label()}"
                        }
                    }
                }
            }
            if chart_shown {
                {
                    let class = if tab == RibbonTab::Format {
                        "tab contextual active"
                    } else {
                        "tab contextual"
                    };
                    rsx! {
                        button { class: "{class}",
                            onclick: move |_| state.write().tab = RibbonTab::Format,
                            "Format"
                        }
                    }
                }
            }
            div { class: "filler" }
        }
    }
}

// ---------------------------------------------------------------- ribbon

#[component]
pub fn Ribbon() -> Element {
    let state = use_context::<Signal<AppState>>();
    let (tab, collapsed) = {
        let s = state.read();
        (s.tab, s.ribbon_collapsed)
    };

    if collapsed {
        return rsx! { div { class: "ribbon collapsed" } };
    }

    rsx! {
        div { class: "ribbon",
            div { class: "ribbon-scroll",
                match tab {
                    RibbonTab::Task => rsx! { TaskTab {} },
                    RibbonTab::Resource => rsx! { ResourceTab {} },
                    RibbonTab::Report => rsx! { ReportTab {} },
                    RibbonTab::Project => rsx! { ProjectTab {} },
                    RibbonTab::View => rsx! { ViewTab {} },
                    RibbonTab::Format => rsx! { FormatTab {} },
                    RibbonTab::Help => rsx! { HelpTab {} },
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Task tab

#[component]
fn TaskTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (has_selection, multi, view, pinned) = {
        let s = state.read();
        (
            !s.selection.is_empty(),
            s.selection.len() > 1,
            s.view,
            s.selection_is_pinned(),
        )
    };

    rsx! {
        Group { title: "View".to_string(), launcher: false,
            MenuBtn {
                glyph: "gantt".to_string(), caption: view.label().to_string(),
                large: true, enabled: true, options: task_view_options(),
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
        }

        Group { title: "Clipboard".to_string(), launcher: true,
            MenuBtn {
                glyph: "paste".to_string(), caption: "Paste".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("paste", "Paste", "paste"),
                    MenuOption::new("copy", "Paste as a copy below", "paste"),
                ],
                on_pick: move |_| state.write().paste(),
            }
            div { class: "rcol",
                SmallBtn { glyph: "cut".to_string(), caption: "Cut".to_string(), enabled: has_selection,
                    on: move |_| state.write().cut_selected() }
                SmallBtn { glyph: "copy".to_string(), caption: "Copy".to_string(), enabled: has_selection,
                    on: move |_| state.write().copy_selected() }
                SmallBtn { glyph: "format-painter".to_string(), caption: "Format Painter".to_string(), enabled: has_selection,
                    on: move |_| state.write().not_implemented("Format Painter") }
            }
        }

        Group { title: "Font".to_string(), launcher: true,
            div { class: "rcol",
                div { class: "font-row",
                    ComboBox {
                        value: "Calibri".to_string(),
                        options: ["Calibri", "Segoe UI", "Inter", "Arial", "Times New Roman"]
                            .iter().map(|f| Choice::plain(*f)).collect(),
                        width: 116.0,
                        on_pick: move |_| state.write().not_implemented("Font"),
                    }
                    ComboBox {
                        value: "11".to_string(),
                        options: ["8", "9", "10", "11", "12", "14", "16", "18"]
                            .iter().map(|f| Choice::plain(*f)).collect(),
                        width: 54.0,
                        on_pick: move |_| state.write().not_implemented("Font size"),
                    }
                }
                div { class: "font-row",
                    button { class: "rbtn-icon", title: "Bold",
                        onclick: move |_| state.write().not_implemented("Bold"), {icon("bold", 15)} }
                    button { class: "rbtn-icon", title: "Italic",
                        onclick: move |_| state.write().not_implemented("Italic"), {icon("italic", 15)} }
                    button { class: "rbtn-icon", title: "Underline",
                        onclick: move |_| state.write().not_implemented("Underline"), {icon("underline", 15)} }
                    div { class: "qat-sep", style: "background: var(--line);" }
                    button { class: "rbtn-icon", title: "Background Color",
                        onclick: move |_| state.write().not_implemented("Background Color"), {icon("fill-color", 15)} }
                    button { class: "rbtn-icon", title: "Font Color",
                        onclick: move |_| state.write().not_implemented("Font Color"), {icon("font-color", 15)} }
                }
            }
        }

        Group { title: "Schedule".to_string(), launcher: false,
            div { class: "rcol",
                div { class: "rrow",
                    MenuBtn {
                        glyph: "mark-on-track".to_string(), caption: "Mark on Track".to_string(),
                        large: false, enabled: has_selection,
                        options: vec![
                            MenuOption::new("mark-on-track", "Mark complete (100%)", "100"),
                            MenuOption::new("percent", "Mark 75% complete", "75"),
                            MenuOption::new("percent", "Mark 50% complete", "50"),
                            MenuOption::new("percent", "Mark 25% complete", "25"),
                            MenuOption::new("clear", "Mark not started (0%)", "0"),
                        ],
                        on_pick: move |value: String| {
                            let percent = value.parse::<u8>().unwrap_or(100);
                            state.write().set_percent_complete(percent);
                        },
                    }
                    SmallBtn { glyph: "respect-links".to_string(), caption: "Respect Links".to_string(), enabled: pinned,
                        on: move |_| state.write().respect_links() }
                }
                div { class: "rrow",
                    SmallBtn { glyph: "link".to_string(), caption: "Link".to_string(), enabled: multi,
                        on: move |_| state.write().link_selected() }
                    SmallBtn { glyph: "unlink".to_string(), caption: "Unlink".to_string(), enabled: has_selection,
                        on: move |_| state.write().unlink_selected() }
                    SmallBtn { glyph: "inactivate".to_string(), caption: "Inactivate".to_string(), enabled: has_selection,
                        on: move |_| state.write().toggle_active() }
                }
            }
        }

        Group { title: "Tasks".to_string(), launcher: false,
            BigBtn { glyph: "manual-schedule".to_string(), caption: "Manually Schedule".to_string(), enabled: has_selection,
                on: move |_| state.write().set_task_mode(TaskMode::Manual) }
            BigBtn { glyph: "auto-schedule".to_string(), caption: "Auto Schedule".to_string(), enabled: has_selection,
                on: move |_| state.write().set_task_mode(TaskMode::Auto) }
            div { class: "rcol",
                MenuBtn {
                    glyph: "inspect".to_string(), caption: "Inspect".to_string(),
                    large: false, enabled: has_selection,
                    options: vec![
                        MenuOption::new("information", "Task Information...", "info"),
                        MenuOption::new("assign-resources", "Assign Resources...", "resources"),
                        MenuOption::new("network", "Show on the network diagram", "network"),
                    ],
                    on_pick: move |value: String| {
                        let row = state.read().primary();
                        match value.as_str() {
                            "resources" => state.write().dialog = Some(Dialog::AssignResources),
                            "network" => state.write().view = ViewKind::NetworkDiagram,
                            _ => {
                                if let Some(row) = row {
                                    state.write().dialog = Some(Dialog::TaskInformation(row));
                                }
                            }
                        }
                    },
                }
                MenuBtn {
                    glyph: "move".to_string(), caption: "Move".to_string(),
                    large: false, enabled: has_selection,
                    options: vec![
                        MenuOption::new("move", "Move up", "up"),
                        MenuOption::new("move", "Move down", "down"),
                        MenuOption::separator(),
                        MenuOption::new("outline", "Indent", "indent"),
                        MenuOption::new("outline", "Outdent", "outdent"),
                    ],
                    on_pick: move |value: String| match value.as_str() {
                        "up" => state.write().move_selected(-1),
                        "down" => state.write().move_selected(1),
                        "indent" => state.write().indent_selected(),
                        _ => state.write().outdent_selected(),
                    },
                }
                MenuBtn {
                    glyph: "mode".to_string(), caption: "Mode".to_string(),
                    large: false, enabled: has_selection,
                    options: vec![
                        MenuOption::new("auto-schedule", "Auto Scheduled", "auto"),
                        MenuOption::new("manual-schedule", "Manually Scheduled", "manual"),
                        MenuOption::separator(),
                        MenuOption::new("respect-links", "As Soon As Possible", "asap"),
                    ],
                    on_pick: move |value: String| match value.as_str() {
                        "manual" => state.write().set_task_mode(TaskMode::Manual),
                        "asap" => state.write().respect_links(),
                        _ => state.write().set_task_mode(TaskMode::Auto),
                    },
                }
            }
        }

        Group { title: "Insert".to_string(), launcher: false,
            MenuBtn {
                glyph: "task-add".to_string(), caption: "Task".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("task-add", "Task", "task"),
                    MenuOption::new("summary", "Summary task", "summary"),
                    MenuOption::new("milestone", "Milestone", "milestone"),
                ],
                on_pick: move |value: String| match value.as_str() {
                    "summary" => state.write().insert_summary(),
                    "milestone" => state.write().insert_milestone(),
                    _ => state.write().insert_task(),
                },
            }
            div { class: "rcol",
                SmallBtn { glyph: "summary".to_string(), caption: "Summary".to_string(), enabled: true,
                    on: move |_| state.write().insert_summary() }
                SmallBtn { glyph: "milestone".to_string(), caption: "Milestone".to_string(), enabled: true,
                    on: move |_| state.write().insert_milestone() }
                SmallBtn { glyph: "deliverable".to_string(), caption: "Deliverable".to_string(), enabled: true,
                    on: move |_| state.write().insert_milestone() }
            }
        }

        Group { title: "Properties".to_string(), launcher: false,
            BigBtn { glyph: "information".to_string(), caption: "Information".to_string(), enabled: has_selection,
                on: move |_| {
                    let row = state.read().primary();
                    if let Some(row) = row {
                        state.write().dialog = Some(Dialog::TaskInformation(row));
                    }
                } }
            div { class: "rcol",
                SmallBtn { glyph: "notes".to_string(), caption: "Notes".to_string(), enabled: has_selection,
                    on: move |_| {
                        let row = state.read().primary();
                        if let Some(row) = row {
                            state.write().dialog = Some(Dialog::TaskInformation(row));
                        }
                    } }
                SmallBtn { glyph: "details".to_string(), caption: "Details".to_string(), enabled: true,
                    on: move |_| state.write().view = ViewKind::TaskUsage }
                SmallBtn { glyph: "add-to-timeline".to_string(), caption: "Add to Timeline".to_string(), enabled: has_selection,
                    on: move |_| {
                        let on = state.read().show_timeline;
                        state.write().show_timeline = !on;
                    } }
            }
        }

        Group { title: "Editing".to_string(), launcher: false,
            BigBtn { glyph: "scroll-to-task".to_string(), caption: "Scroll to Task".to_string(), enabled: has_selection,
                on: move |_| state.write().scroll_to_task() }
            div { class: "rcol",
                MenuBtn {
                    glyph: "find".to_string(), caption: "Find".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("scroll-to-task", "Go to selected task", "goto"),
                        MenuOption::new("critical-tasks", "Go to the critical path", "critical"),
                    ],
                    on_pick: move |value: String| {
                        if value == "critical" {
                            let row = {
                                let s = state.read();
                                (0..s.project.tasks.len()).find(|&i| {
                                    !s.project.is_summary(i) && s.project.tasks[i].scheduled.critical
                                })
                            };
                            match row {
                                Some(row) => {
                                    state.write().select(row);
                                    state.write().note("Jumped to the first critical task");
                                }
                                None => state.write().note("No critical tasks in this plan"),
                            }
                        } else {
                            state.write().scroll_to_task();
                        }
                    },
                }
                MenuBtn {
                    glyph: "clear".to_string(), caption: "Clear".to_string(),
                    large: false, enabled: has_selection,
                    options: vec![
                        MenuOption::new("clear", "Delete task", "delete"),
                        MenuOption::new("unlink", "Clear links", "links"),
                        MenuOption::new("percent", "Clear progress", "progress"),
                        MenuOption::new("respect-links", "Clear constraint", "constraint"),
                    ],
                    on_pick: move |value: String| match value.as_str() {
                        "links" => state.write().unlink_selected(),
                        "progress" => state.write().set_percent_complete(0),
                        "constraint" => state.write().respect_links(),
                        _ => state.write().delete_selected(),
                    },
                }
                MenuBtn {
                    glyph: "fill-down".to_string(), caption: "Fill".to_string(),
                    large: false, enabled: has_selection,
                    options: vec![MenuOption::new("fill-down", "Fill down", "down")],
                    on_pick: move |_| state.write().not_implemented("Fill Down"),
                }
            }
        }
    }
}

// ------------------------------------------------------------ Resource tab

#[component]
fn ResourceTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (has_selection, overallocated) = {
        let s = state.read();
        let over = s
            .report
            .as_ref()
            .map(|r| !r.overallocations.is_empty())
            .unwrap_or(false);
        (!s.selection.is_empty(), over)
    };

    rsx! {
        Group { title: "View".to_string(), launcher: false,
            MenuBtn {
                glyph: "team-planner".to_string(), caption: "Team Planner".to_string(),
                large: true, enabled: true, options: resource_view_options(),
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
        }

        Group { title: "Assignments".to_string(), launcher: false,
            BigBtn { glyph: "assign-resources".to_string(), caption: "Assign Resources".to_string(), enabled: has_selection,
                on: move |_| state.write().dialog = Some(Dialog::AssignResources) }
            div { class: "rcol",
                MenuBtn {
                    glyph: "resource-pool".to_string(), caption: "Resource Pool".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("resource-sheet", "Open the resource sheet", "sheet"),
                        MenuOption::new("resource-usage", "Open resource usage", "usage"),
                    ],
                    on_pick: move |value: String| {
                        state.write().view = if value == "usage" {
                            ViewKind::ResourceUsage
                        } else {
                            ViewKind::ResourceSheet
                        };
                    },
                }
            }
        }

        Group { title: "Insert".to_string(), launcher: false,
            MenuBtn {
                glyph: "add-resource".to_string(), caption: "Add Resources".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("add-resource", "Work resource", "Work"),
                    MenuOption::new("deliverable", "Material resource", "Material"),
                    MenuOption::new("report-costs", "Cost resource", "Cost"),
                ],
                on_pick: move |kind: String| {
                    let name = format!("New {kind} Resource");
                    state.write().add_resource(&name);
                    let index = state.read().project.resources.len().saturating_sub(1);
                    state.write().commit_resource_cell(index, "kind", &kind);
                    state.write().view = ViewKind::ResourceSheet;
                },
            }
        }

        Group { title: "Properties".to_string(), launcher: false,
            BigBtn { glyph: "information".to_string(), caption: "Information".to_string(), enabled: true,
                on: move |_| state.write().view = ViewKind::ResourceSheet }
            div { class: "rcol",
                SmallBtn { glyph: "notes".to_string(), caption: "Notes".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Resource Notes") }
                SmallBtn { glyph: "details".to_string(), caption: "Details".to_string(), enabled: true,
                    on: move |_| state.write().view = ViewKind::ResourceUsage }
            }
        }

        Group { title: "Level".to_string(), launcher: false,
            div { class: "rcol",
                SmallBtn { glyph: "level-options".to_string(), caption: "Leveling Options".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Leveling Options") }
                SmallBtn { glyph: "level".to_string(), caption: "Level Resource".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Level Resource") }
                SmallBtn { glyph: "level".to_string(), caption: "Level All".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Level All") }
            }
            div { class: "rcol",
                SmallBtn { glyph: "clear".to_string(), caption: "Clear Leveling".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Clear Leveling") }
                SmallBtn { glyph: "next-over".to_string(), caption: "Next Overallocation".to_string(), enabled: overallocated,
                    on: move |_| {
                        let message = {
                            let s = state.read();
                            s.report.as_ref().ok().and_then(|r| {
                                r.overallocations.first().map(|o| {
                                    format!(
                                        "{} is booked at {:.0}% on {}",
                                        o.resource_name, o.peak_units * 100.0, o.first_date
                                    )
                                })
                            })
                        };
                        if let Some(message) = message {
                            state.write().note(message);
                            state.write().view = ViewKind::ResourceSheet;
                        }
                    } }
                SmallBtn { glyph: "selected-tasks".to_string(), caption: "Level Selection".to_string(), enabled: has_selection,
                    on: move |_| state.write().not_implemented("Level Selection") }
            }
        }
    }
}

// -------------------------------------------------------------- Report tab

#[component]
fn ReportTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    rsx! {
        Group { title: "View Reports".to_string(), launcher: false,
            MenuBtn {
                glyph: "dashboard".to_string(), caption: "Dashboards".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("project-info", "Project overview", "info"),
                    MenuOption::new("report-progress", "Work in progress", "tracking"),
                ],
                on_pick: move |value: String| {
                    if value == "tracking" {
                        state.write().view = ViewKind::TrackingGantt;
                    } else {
                        state.write().backstage = Some(BackstagePage::Info);
                    }
                },
            }
            MenuBtn {
                glyph: "resource-sheet".to_string(), caption: "Resources".to_string(),
                large: true, enabled: true, options: resource_view_options(),
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
            MenuBtn {
                glyph: "report-costs".to_string(), caption: "Costs".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("task-sheet", "Cost by task", "TaskSheet"),
                    MenuOption::new("resource-sheet", "Cost by resource", "ResourceSheet"),
                ],
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
            MenuBtn {
                glyph: "report-progress".to_string(), caption: "In Progress".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("tracking-gantt", "Tracking Gantt", "TrackingGantt"),
                    MenuOption::new("task-usage", "Task Usage", "TaskUsage"),
                ],
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
            MenuBtn {
                glyph: "report-custom".to_string(), caption: "Custom".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("network", "Network Diagram", "NetworkDiagram"),
                    MenuOption::new("calendar", "Calendar", "CalendarView"),
                ],
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
        }

        Group { title: "Export".to_string(), launcher: false,
            div { class: "rcol",
                SmallBtn { glyph: "export".to_string(), caption: "Export".to_string(), enabled: true,
                    on: move |_| state.write().backstage = Some(BackstagePage::Export) }
                SmallBtn { glyph: "print".to_string(), caption: "Print".to_string(), enabled: true,
                    on: move |_| state.write().backstage = Some(BackstagePage::Print) }
            }
        }
    }
}

// ------------------------------------------------------------- Project tab

#[component]
fn ProjectTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let has_baseline = state.read().project.has_baseline();

    rsx! {
        Group { title: "Insert".to_string(), launcher: false,
            BigBtn { glyph: "subproject".to_string(), caption: "Subproject".to_string(), enabled: true,
                on: move |_| state.write().not_implemented("Subproject") }
        }

        Group { title: "Properties".to_string(), launcher: false,
            BigBtn { glyph: "project-info".to_string(), caption: "Project Information".to_string(), enabled: true,
                on: move |_| state.write().dialog = Some(Dialog::ProjectInformation) }
            div { class: "rcol",
                SmallBtn { glyph: "custom-fields".to_string(), caption: "Custom Fields".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Custom Fields") }
                SmallBtn { glyph: "links-between".to_string(), caption: "Links Between Projects".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Links Between Projects") }
                MenuBtn {
                    glyph: "wbs".to_string(), caption: "WBS".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("outline-number", "Show outline numbers", "show"),
                        MenuOption::new("clear", "Hide outline numbers", "hide"),
                    ],
                    on_pick: move |value: String| {
                        state.write().show_outline_number = value == "show";
                    },
                }
            }
            BigBtn { glyph: "working-time".to_string(), caption: "Change Working Time".to_string(), enabled: true,
                on: move |_| state.write().dialog = Some(Dialog::ChangeWorkingTime) }
        }

        Group { title: "Schedule".to_string(), launcher: false,
            div { class: "rcol",
                SmallBtn { glyph: "calculate".to_string(), caption: "Calculate Project".to_string(), enabled: true,
                    on: move |_| {
                        state.write().reschedule();
                        state.write().note("Project recalculated");
                    } }
                MenuBtn {
                    glyph: "baseline".to_string(), caption: "Set Baseline".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("baseline", "Set baseline", "set"),
                        MenuOption::new("clear", "Clear baseline", "clear"),
                        MenuOption::separator(),
                        MenuOption::new("slack", "Show baseline on the chart", "show"),
                    ],
                    on_pick: move |value: String| match value.as_str() {
                        "clear" => state.write().clear_baseline(),
                        "show" => {
                            let on = state.read().show_baseline;
                            state.write().show_baseline = !on;
                        }
                        _ => state.write().set_baseline(),
                    },
                }
                SmallBtn { glyph: "clear".to_string(), caption: "Clear Baseline".to_string(), enabled: has_baseline,
                    on: move |_| state.write().clear_baseline() }
                SmallBtn { glyph: "move-project".to_string(), caption: "Move Project".to_string(), enabled: true,
                    on: move |_| state.write().dialog = Some(Dialog::ProjectInformation) }
            }
        }

        Group { title: "Status".to_string(), launcher: false,
            div { class: "rcol",
                SmallBtn { glyph: "status-date".to_string(), caption: "Status Date".to_string(), enabled: true,
                    on: move |_| state.write().dialog = Some(Dialog::ProjectInformation) }
                SmallBtn { glyph: "update-project".to_string(), caption: "Update Project".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Update Project") }
            }
        }

        Group { title: "Proofing".to_string(), launcher: false,
            BigBtn { glyph: "spelling".to_string(), caption: "Spelling".to_string(), enabled: true,
                on: move |_| state.write().not_implemented("Spelling") }
        }
    }
}

// ---------------------------------------------------------------- View tab

#[component]
fn ViewTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (view, zoom, show_timeline, has_selection, filter) = {
        let s = state.read();
        (
            s.view,
            s.zoom,
            s.show_timeline,
            !s.selection.is_empty(),
            s.filter,
        )
    };

    rsx! {
        Group { title: "Task Views".to_string(), launcher: false,
            MenuBtn {
                glyph: "gantt".to_string(), caption: "Gantt Chart".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("gantt", "Gantt Chart", "GanttChart"),
                    MenuOption::new("tracking-gantt", "Tracking Gantt", "TrackingGantt"),
                ],
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
            MenuBtn {
                glyph: "task-usage".to_string(), caption: "Task Usage".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("task-usage", "Task Usage", "TaskUsage"),
                    MenuOption::new("task-sheet", "Task Sheet", "TaskSheet"),
                ],
                on_pick: move |value: String| {
                    if let Some(kind) = view_from(&value) { state.write().view = kind; }
                },
            }
            div { class: "rcol",
                SmallBtn { glyph: "network".to_string(), caption: "Network Diagram".to_string(), enabled: true,
                    on: move |_| state.write().view = ViewKind::NetworkDiagram }
                SmallBtn { glyph: "calendar".to_string(), caption: "Calendar".to_string(), enabled: true,
                    on: move |_| state.write().view = ViewKind::CalendarView }
                MenuBtn {
                    glyph: "other-views".to_string(), caption: "Other Views".to_string(),
                    large: false, enabled: true, options: task_view_options(),
                    on_pick: move |value: String| {
                        if let Some(kind) = view_from(&value) { state.write().view = kind; }
                    },
                }
            }
        }

        Group { title: "Resource Views".to_string(), launcher: false,
            BigBtn { glyph: "team-planner".to_string(), caption: "Team Planner".to_string(), enabled: true,
                on: move |_| state.write().view = ViewKind::TeamPlanner }
            BigBtn { glyph: "resource-usage".to_string(), caption: "Resource Usage".to_string(), enabled: true,
                on: move |_| state.write().view = ViewKind::ResourceUsage }
            div { class: "rcol",
                SmallBtn { glyph: "resource-sheet".to_string(), caption: "Resource Sheet".to_string(), enabled: true,
                    on: move |_| state.write().view = ViewKind::ResourceSheet }
                MenuBtn {
                    glyph: "other-views".to_string(), caption: "Other Views".to_string(),
                    large: false, enabled: true, options: resource_view_options(),
                    on_pick: move |value: String| {
                        if let Some(kind) = view_from(&value) { state.write().view = kind; }
                    },
                }
            }
        }

        Group { title: "Data".to_string(), launcher: false,
            div { class: "rcol",
                MenuBtn {
                    glyph: "sort".to_string(), caption: "Sort".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("sort", "by Start date", "start"),
                        MenuOption::new("sort", "by Finish date", "finish"),
                        MenuOption::new("sort", "by Duration", "duration"),
                        MenuOption::new("sort", "by Cost", "cost"),
                    ],
                    on_pick: move |value: String| state.write().sort_tasks(&value),
                }
                MenuBtn {
                    glyph: "outline".to_string(), caption: "Outline".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("outline", "Show all subtasks", "expand"),
                        MenuOption::new("outline", "Collapse all", "collapse"),
                        MenuOption::separator(),
                        MenuOption::new("outline", "Indent selection", "indent"),
                        MenuOption::new("outline", "Outdent selection", "outdent"),
                    ],
                    on_pick: move |value: String| match value.as_str() {
                        "collapse" => state.write().expand_all(true),
                        "indent" => state.write().indent_selected(),
                        "outdent" => state.write().outdent_selected(),
                        _ => state.write().expand_all(false),
                    },
                }
                MenuBtn {
                    glyph: "tables".to_string(), caption: "Tables".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("tables", "Entry", "GanttChart"),
                        MenuOption::new("task-sheet", "Task Sheet", "TaskSheet"),
                        MenuOption::new("task-usage", "Task Usage", "TaskUsage"),
                    ],
                    on_pick: move |value: String| {
                        if let Some(kind) = view_from(&value) { state.write().view = kind; }
                    },
                }
            }
            div { class: "rcol",
                MenuBtn {
                    glyph: "highlight".to_string(), caption: "Highlight".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("critical-tasks", "Critical path", "critical"),
                        MenuOption::new("slack", "Tasks with slack", "slack"),
                    ],
                    on_pick: move |value: String| {
                        let mut writer = state.write();
                        if value == "slack" {
                            let on = writer.show_slack;
                            writer.show_slack = !on;
                        } else {
                            let on = writer.show_critical;
                            writer.show_critical = !on;
                        }
                    },
                }
                MenuBtn {
                    glyph: "filter".to_string(),
                    caption: format!("Filter: {}", filter.label()),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("other-views", "All tasks", "all"),
                        MenuOption::new("critical-tasks", "Critical tasks only", "critical"),
                        MenuOption::new("milestone", "Milestones only", "milestones"),
                        MenuOption::new("report-progress", "Incomplete tasks", "incomplete"),
                    ],
                    on_pick: move |value: String| state.write().set_filter(&value),
                }
                MenuBtn {
                    glyph: "group-by".to_string(), caption: "Group by".to_string(),
                    large: false, enabled: true,
                    options: vec![MenuOption::new("group-by", "No group", "none")],
                    on_pick: move |_| state.write().not_implemented("Group by"),
                }
            }
        }

        Group { title: "Zoom".to_string(), launcher: false,
            div { class: "rcol",
                div { class: "rrow",
                    span { class: "glyph", {icon("timescale", 16)} }
                    Dropdown {
                        value: zoom.label().to_string(),
                        options: Zoom::ORDER.iter().map(|z| Choice::plain(z.label())).collect(),
                        width: 98.0, large: false, disabled: false,
                        on_pick: move |picked: String| {
                            let choice = match picked.as_str() {
                                "Weeks" => Zoom::Weeks,
                                "Months" => Zoom::Months,
                                "Quarters" => Zoom::Quarters,
                                _ => Zoom::Days,
                            };
                            state.write().zoom = choice;
                        },
                    }
                }
                div { class: "rrow",
                    SmallBtn { glyph: "zoom-in".to_string(), caption: "Zoom In".to_string(), enabled: true,
                        on: move |_| { let z = state.read().zoom.zoom_in(); state.write().zoom = z; } }
                    SmallBtn { glyph: "zoom-out".to_string(), caption: "Zoom Out".to_string(), enabled: true,
                        on: move |_| { let z = state.read().zoom.zoom_out(); state.write().zoom = z; } }
                }
                div { class: "rrow",
                    SmallBtn { glyph: "entire-project".to_string(), caption: "Entire Project".to_string(), enabled: true,
                        on: move |_| state.write().zoom_to_fit() }
                    SmallBtn { glyph: "selected-tasks".to_string(), caption: "Selected Tasks".to_string(), enabled: has_selection,
                        on: move |_| state.write().scroll_to_task() }
                }
            }
        }

        Group { title: "Split View".to_string(), launcher: false,
            div { class: "rcol",
                CheckItem { label: "Timeline".to_string(), on_state: show_timeline,
                    on: move |_| { let on = state.read().show_timeline; state.write().show_timeline = !on; } }
                CheckItem { label: "Details".to_string(), on_state: view == ViewKind::TaskUsage,
                    on: move |_| {
                        let current = state.read().view;
                        state.write().view = if current == ViewKind::TaskUsage {
                            ViewKind::GanttChart
                        } else {
                            ViewKind::TaskUsage
                        };
                    } }
            }
        }

        Group { title: "Window".to_string(), launcher: false,
            div { class: "rcol",
                SmallBtn { glyph: "hide".to_string(), caption: "Collapse Ribbon".to_string(), enabled: true,
                    on: move |_| { let on = state.read().ribbon_collapsed; state.write().ribbon_collapsed = !on; } }
                MenuBtn {
                    glyph: "macros".to_string(), caption: "Macros".to_string(),
                    large: false, enabled: true,
                    options: vec![MenuOption::new("macros", "No macros recorded", "none")],
                    on_pick: move |_| state.write().not_implemented("Macros"),
                }
            }
        }
    }
}

// -------------------------------------------------------------- Format tab

#[component]
fn FormatTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (critical, slack, outline_number, summary, baseline, style, timeline) = {
        let s = state.read();
        (
            s.show_critical,
            s.show_slack,
            s.show_outline_number,
            s.project.show_project_summary,
            s.show_baseline,
            s.gantt_style,
            s.show_timeline,
        )
    };

    rsx! {
        Group { title: "Format".to_string(), launcher: false,
            BigBtn { glyph: "text-styles".to_string(), caption: "Text Styles".to_string(), enabled: true,
                on: move |_| state.write().not_implemented("Text Styles") }
            MenuBtn {
                glyph: "gridlines".to_string(), caption: "Gridlines".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("gridlines", "Gridlines are always shown", "none"),
                ],
                on_pick: move |_| state.write().not_implemented("Gridlines"),
            }
            BigBtn { glyph: "layout".to_string(), caption: "Layout".to_string(), enabled: true,
                on: move |_| state.write().not_implemented("Layout") }
        }

        Group { title: "Columns".to_string(), launcher: false,
            BigBtn { glyph: "insert-column".to_string(), caption: "Insert Column".to_string(), enabled: true,
                on: move |_| {
                    let at = state.read().columns.len();
                    state.write().dialog = Some(Dialog::InsertColumn(at));
                } }
            div { class: "rcol",
                MenuBtn {
                    glyph: "column-settings".to_string(), caption: "Column Settings".to_string(),
                    large: false, enabled: true,
                    options: vec![
                        MenuOption::new("insert-column", "Insert a column...", "insert"),
                        MenuOption::new("column-settings", "Reset to the Entry table", "reset"),
                        MenuOption::separator(),
                        MenuOption::new("outline-number", "Show outline numbers", "outline"),
                        MenuOption::new("wbs", "Hide outline numbers", "plain"),
                    ],
                    on_pick: move |value: String| match value.as_str() {
                        "insert" => {
                            let at = state.read().columns.len();
                            state.write().dialog = Some(Dialog::InsertColumn(at));
                        }
                        "reset" => state.write().reset_columns(),
                        "outline" => state.write().show_outline_number = true,
                        _ => state.write().show_outline_number = false,
                    },
                }
                SmallBtn { glyph: "custom-fields".to_string(), caption: "Custom Fields".to_string(), enabled: true,
                    on: move |_| state.write().not_implemented("Custom Fields") }
            }
        }

        Group { title: "Bar Styles".to_string(), launcher: true,
            MenuBtn {
                glyph: "gantt".to_string(), caption: "Format".to_string(),
                large: true, enabled: true,
                options: vec![
                    MenuOption::new("critical-tasks", "Toggle critical tasks", "critical"),
                    MenuOption::new("slack", "Toggle slack", "slack"),
                    MenuOption::new("baseline", "Toggle baseline", "baseline"),
                ],
                on_pick: move |value: String| {
                    let mut writer = state.write();
                    match value.as_str() {
                        "slack" => { let on = writer.show_slack; writer.show_slack = !on; }
                        "baseline" => { let on = writer.show_baseline; writer.show_baseline = !on; }
                        _ => { let on = writer.show_critical; writer.show_critical = !on; }
                    }
                },
            }
            div { class: "rcol",
                CheckItem { label: "Critical Tasks".to_string(), on_state: critical,
                    on: move |_| { let on = state.read().show_critical; state.write().show_critical = !on; } }
                CheckItem { label: "Slack".to_string(), on_state: slack,
                    on: move |_| { let on = state.read().show_slack; state.write().show_slack = !on; } }
                CheckItem { label: "Baseline".to_string(), on_state: baseline,
                    on: move |_| { let on = state.read().show_baseline; state.write().show_baseline = !on; } }
            }
        }

        Group { title: "Gantt Chart Style".to_string(), launcher: true,
            div { class: "gallery",
                for (index, (name, colours)) in aop_core::BarStyles::PRESETS.iter().enumerate() {
                    {
                        let class = if index == style { "gallery-item on" } else { "gallery-item" };
                        rsx! {
                            button { key: "{index}", class: "{class}", title: "{name}",
                                onclick: move |_| state.write().apply_bar_preset(index),
                                div { class: "g-bar", style: "background: {colours[2]}; width: 88%; margin-left: 0;" }
                                div { class: "g-bar", style: "background: {colours[0]}; width: 46%; margin-left: 8%;" }
                                div { class: "g-bar", style: "background: {colours[1]}; width: 38%; margin-left: 34%;" }
                                div { class: "g-bar", style: "background: {colours[0]}; width: 30%; margin-left: 60%;" }
                            }
                        }
                    }
                }
            }
            div { class: "rcol",
                SmallBtn { glyph: "fill-color".to_string(), caption: "Bar Colors...".to_string(), enabled: true,
                    on: move |_| state.write().dialog = Some(Dialog::BarStyles) }
            }
        }

        Group { title: "Show/Hide".to_string(), launcher: false,
            div { class: "rcol",
                CheckItem { label: "Outline Number".to_string(), on_state: outline_number,
                    on: move |_| { let on = state.read().show_outline_number; state.write().show_outline_number = !on; } }
                CheckItem { label: "Project Summary Task".to_string(), on_state: summary,
                    on: move |_| {
                        let on = state.read().project.show_project_summary;
                        state.write().project.show_project_summary = !on;
                    } }
                CheckItem { label: "Timeline".to_string(), on_state: timeline,
                    on: move |_| { let on = state.read().show_timeline; state.write().show_timeline = !on; } }
            }
        }

        Group { title: "Drawings".to_string(), launcher: false,
            MenuBtn {
                glyph: "drawing".to_string(), caption: "Drawing".to_string(),
                large: true, enabled: true,
                options: vec![MenuOption::new("drawing", "Drawing is not available", "none")],
                on_pick: move |_| state.write().not_implemented("Drawing"),
            }
        }
    }
}

// ---------------------------------------------------------------- Help tab

#[component]
fn HelpTab() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    rsx! {
        Group { title: "Help".to_string(), launcher: false,
            BigBtn { glyph: "help".to_string(), caption: "Help".to_string(), enabled: true,
                on: move |_| state.write().backstage = Some(BackstagePage::About) }
            BigBtn { glyph: "training".to_string(), caption: "About".to_string(), enabled: true,
                on: move |_| state.write().backstage = Some(BackstagePage::About) }
        }
    }
}
