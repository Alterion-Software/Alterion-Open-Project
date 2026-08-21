//! Right-click menus for task rows, the chart background and resource rows.

use dioxus::prelude::*;

use aop_core::TaskMode;

use crate::icons::icon;
use crate::state::{AppState, ContextMenu, Dialog, ViewKind};

#[component]
fn Item(
    glyph: String,
    label: String,
    shortcut: String,
    enabled: bool,
    checked: Option<bool>,
    on: EventHandler<()>,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let class = if enabled { "ctxitem" } else { "ctxitem disabled" };

    rsx! {
        button {
            class: "{class}",
            onclick: move |event| {
                event.stop_propagation();
                if enabled {
                    on.call(());
                    state.write().close_menu();
                }
            },
            match checked {
                Some(true) => rsx! { span { class: "tick", "\u{2713}" } },
                Some(false) => rsx! { span { class: "tick" } },
                None => rsx! { span { class: "glyph", {icon(&glyph, 15)} } },
            }
            span { class: "label", "{label}" }
            if !shortcut.is_empty() {
                span { class: "shortcut", "{shortcut}" }
            }
        }
    }
}

/// One button on the floating mini toolbar.
#[component]
fn MiniBtn(
    glyph: String,
    label: String,
    caret: bool,
    enabled: bool,
    /// A colour to underline the glyph with, for the commands that are about
    /// one. The same bucket and bar the ribbon shows, so the two read as the
    /// same command rather than two that happen to share a name.
    swatch: Option<String>,
    on: EventHandler<()>,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let class = if enabled { "minibtn" } else { "minibtn disabled" };
    rsx! {
        button {
            class: "{class}",
            title: "{label}",
            onclick: move |event| {
                event.stop_propagation();
                if enabled {
                    on.call(());
                    state.write().close_menu();
                }
            },
            {icon(&glyph, 16)}
            if let Some(colour) = swatch {
                span { class: "colour-bar", style: "background: {colour};" }
            }
            if caret { span { class: "caret", {crate::icons::icon("caret-down", 12)} } }
        }
    }
}

/// The floating strip that appears above the menu, the way Office puts a mini
/// toolbar over its context menu: clipboard first, then the formatting bits.
#[component]
fn MiniToolbar(row: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (has_selection, multi, summary) = {
        let s = state.read();
        (
            !s.selection.is_empty(),
            s.selection.len() > 1,
            s.project.is_summary(row),
        )
    };

    rsx! {
        div { class: "ctx-minibar", onclick: move |event| event.stop_propagation(),
            MiniBtn {
                glyph: "paste".to_string(), label: "Paste".to_string(),
                caret: true, enabled: true, swatch: None,
                on: move |_| state.write().paste(),
            }
            MiniBtn {
                glyph: "cut".to_string(), label: "Cut".to_string(),
                caret: false, enabled: has_selection, swatch: None,
                on: move |_| state.write().cut_selected(),
            }
            MiniBtn {
                glyph: "copy".to_string(), label: "Copy".to_string(),
                caret: false, enabled: has_selection, swatch: None,
                on: move |_| state.write().copy_selected(),
            }

            div { class: "minisep" }

            MiniBtn {
                glyph: "fill-color".to_string(), label: "Bar Colors".to_string(),
                caret: true, enabled: true,
                swatch: Some(state.read().project.bar_styles.task.clone()),
                on: move |_| state.write().dialog = Some(Dialog::BarStyles),
            }
            MiniBtn {
                glyph: "link".to_string(), label: "Link Tasks".to_string(),
                caret: false, enabled: multi, swatch: None,
                on: move |_| state.write().link_selected(),
            }
            MiniBtn {
                glyph: "unlink".to_string(), label: "Unlink Tasks".to_string(),
                caret: false, enabled: has_selection, swatch: None,
                on: move |_| state.write().unlink_selected(),
            }

            div { class: "minisep" }

            MiniBtn {
                glyph: "mark-on-track".to_string(), label: "Mark 100% Complete".to_string(),
                caret: false, enabled: !summary, swatch: None,
                on: move |_| state.write().set_percent_complete(100),
            }
            MiniBtn {
                glyph: "information".to_string(), label: "Task Information".to_string(),
                caret: false, enabled: true, swatch: None,
                on: move |_| state.write().dialog = Some(Dialog::TaskInformation(row)),
            }
        }
    }
}

/// How wide a context menu is allowed to get, from the stylesheet.
///
/// Used only to decide which side of the pointer the menu opens on, so it does
/// not have to be exact, only not smaller than the menu really is.
const MENU_WIDTH: f64 = 260.0;

#[component]
pub fn ContextMenuHost(menu: ContextMenu) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (x, y) = menu.position();
    let (view_w, view_h) = use_context::<Signal<crate::state::Viewport>>()();
    let task_row = match menu {
        ContextMenu::Task { row, .. } => Some(row),
        _ => None,
    };

    // Placed against the window when its size is known and where it was asked
    // for when it is not. See `crate::placement`: a viewport of zero means
    // nobody has said yet, not that the window has no room, and reading it the
    // other way put every menu in the same corner.
    //
    // The toolbar and the menu are anchored together as one stack rather than
    // separately. Placed apart, a menu that opened upward would grow over the
    // toolbar and leave it underneath, which is the one arrangement the
    // toolbar must never be in.
    let horizontal = crate::placement::horizontal(x, MENU_WIDTH, (view_w, view_h));
    let vertical = crate::placement::vertical(y, (view_w, view_h));

    rsx! {
        div {
            class: "ctx-scrim",
            onclick: move |_| state.write().close_menu(),
            oncontextmenu: move |event| {
                event.prevent_default();
                state.write().close_menu();
            },
        }

        div { class: "ctx-stack", style: "{horizontal} {vertical}",
            if let Some(row) = task_row {
                div { class: "ctx-minibar-wrap",
                    MiniToolbar { row }
                }
            }

            div {
                class: "ctxmenu",
                onclick: move |event| event.stop_propagation(),
                match menu {
                    ContextMenu::Task { row, .. } => rsx! { TaskMenu { row } },
                    ContextMenu::Chart { .. } => rsx! { ChartMenu {} },
                    ContextMenu::Resource { index, .. } => rsx! { ResourceMenu { index } },
                    ContextMenu::Column { index, .. } => rsx! { ColumnMenu { index } },
                }
            }
        }
    }
}

#[component]
fn TaskMenu(row: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let (name, summary, active, multi, level, has_links, pinned) = {
        let s = state.read();
        let Some(task) = s.project.tasks.get(row) else {
            return rsx! { div { class: "ctxheader", "Task no longer exists" } };
        };
        let has_links = !s.project.predecessors_of(task.id).is_empty()
            || !s.project.successors_of(task.id).is_empty();
        (
            task.name.clone(),
            s.project.is_summary(row),
            task.active,
            s.selection.len() > 1,
            task.outline_level,
            has_links,
            s.selection_is_pinned(),
        )
    };

    // What the row is flagged for, and what can be done about it. This is where
    // the warning in the table becomes actionable: the marker says something is
    // up, the menu says what and offers the change.
    let issues = aop_core::issues::task_issues(&state.read().project, row);
    let dismissed = state
        .read()
        .project
        .tasks
        .get(row)
        .is_some_and(|task| !task.ignored_issues.is_empty());

    rsx! {
        div { class: "ctxheader", if name.is_empty() { "(unnamed task)" } else { "{name}" } }

        if !issues.is_empty() {
            for issue in issues.iter().cloned() {
                {
                    let kind = issue.kind;
                    let ignored = issue.ignored;
                    let class = if ignored { "ctx-issue ignored" } else { "ctx-issue" };
                    rsx! {
                        div { key: "{kind:?}", class: "{class}",
                            span { class: "ctx-issue-text", "{issue.message}" }
                            div { class: "ctx-issue-acts",
                                if let Some(fix) = issue.fix {
                                    button {
                                        class: "ctx-issue-fix",
                                        onclick: move |event| {
                                            event.stop_propagation();
                                            state.write().fix_issue(row, fix);
                                            state.write().close_menu();
                                        },
                                        "{fix.label()}"
                                    }
                                }
                                if ignored {
                                    button {
                                        class: "ctx-issue-ignore",
                                        onclick: move |event| {
                                            event.stop_propagation();
                                            state.write().restore_issue(row, kind);
                                            state.write().close_menu();
                                        },
                                        "Stop ignoring"
                                    }
                                } else {
                                    button {
                                        class: "ctx-issue-ignore",
                                        onclick: move |event| {
                                            event.stop_propagation();
                                            state.write().ignore_issue(row, kind);
                                            state.write().close_menu();
                                        },
                                        "Ignore"
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "ctxsep" }
        }

        if dismissed {
            Item {
                glyph: "warning".to_string(), label: "Show dismissed warnings".to_string(),
                shortcut: String::new(), enabled: true, checked: None,
                on: move |_| state.write().restore_issues(row),
            }
            div { class: "ctxsep" }
        }

        Item {
            glyph: "information".to_string(), label: "Task Information...".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().dialog = Some(Dialog::TaskInformation(row)),
        }
        Item {
            glyph: "assign-resources".to_string(), label: "Assign Resources...".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().dialog = Some(Dialog::AssignResources),
        }

        div { class: "ctxsep" }

        Item {
            glyph: "task-add".to_string(), label: "Insert Task".to_string(),
            shortcut: "Ins".to_string(), enabled: true, checked: None,
            on: move |_| state.write().insert_task(),
        }
        Item {
            glyph: "milestone".to_string(), label: "Insert Milestone".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().insert_milestone(),
        }
        Item {
            glyph: "summary".to_string(), label: "Insert Summary Task".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().insert_summary(),
        }
        Item {
            glyph: "clear".to_string(), label: "Delete Task".to_string(),
            shortcut: "Del".to_string(), enabled: true, checked: None,
            on: move |_| state.write().delete_selected(),
        }

        div { class: "ctxsep" }

        Item {
            glyph: "outline".to_string(), label: "Indent Task".to_string(),
            shortcut: "Alt+Shift+\u{2192}".to_string(), enabled: row > 0, checked: None,
            on: move |_| state.write().indent_selected(),
        }
        Item {
            glyph: "outline".to_string(), label: "Outdent Task".to_string(),
            shortcut: "Alt+Shift+\u{2190}".to_string(), enabled: level > 0, checked: None,
            on: move |_| state.write().outdent_selected(),
        }
        Item {
            glyph: "move".to_string(), label: "Move Up".to_string(),
            shortcut: String::new(), enabled: row > 0, checked: None,
            on: move |_| state.write().move_selected(-1),
        }
        Item {
            glyph: "move".to_string(), label: "Move Down".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().move_selected(1),
        }

        div { class: "ctxsep" }

        Item {
            glyph: "link".to_string(), label: "Link Tasks".to_string(),
            shortcut: String::new(), enabled: multi, checked: None,
            on: move |_| state.write().link_selected(),
        }
        Item {
            glyph: "unlink".to_string(), label: "Unlink Tasks".to_string(),
            shortcut: String::new(), enabled: has_links, checked: None,
            on: move |_| state.write().unlink_selected(),
        }

        div { class: "ctxsep" }

        for percent in [0u8, 25, 50, 75, 100] {
            Item {
                key: "{percent}",
                glyph: "percent".to_string(),
                label: format!("Mark {percent}% Complete"),
                shortcut: String::new(),
                enabled: !summary,
                checked: None,
                on: move |_| state.write().set_percent_complete(percent),
            }
        }

        div { class: "ctxsep" }

        Item {
            glyph: "auto-schedule".to_string(), label: "Auto Schedule".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().set_task_mode(TaskMode::Auto),
        }
        Item {
            glyph: "manual-schedule".to_string(), label: "Manually Schedule".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().set_task_mode(TaskMode::Manual),
        }
        Item {
            glyph: "respect-links".to_string(), label: "As Soon As Possible".to_string(),
            shortcut: String::new(), enabled: pinned, checked: None,
            on: move |_| state.write().respect_links(),
        }
        Item {
            glyph: "inactivate".to_string(),
            label: if active { "Inactivate Task".to_string() } else { "Activate Task".to_string() },
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().toggle_active(),
        }
    }
}

#[component]
fn ChartMenu() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (critical, slack, baseline, timeline, outline, has_baseline) = {
        let s = state.read();
        (
            s.show_critical,
            s.show_slack,
            s.show_baseline,
            s.show_timeline,
            s.show_outline_number,
            s.project.has_baseline(),
        )
    };

    rsx! {
        div { class: "ctxheader", "Gantt Chart" }

        Item {
            glyph: "zoom-in".to_string(), label: "Zoom In".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| { let z = state.read().zoom.zoom_in(); state.write().zoom = z; },
        }
        Item {
            glyph: "zoom-out".to_string(), label: "Zoom Out".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| { let z = state.read().zoom.zoom_out(); state.write().zoom = z; },
        }
        Item {
            glyph: "entire-project".to_string(), label: "Zoom to Entire Project".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| {
                let span = {
                    let s = state.read();
                    (s.project.finish_date - s.project.start_date).num_days()
                };
                let zoom = if span > 720 { crate::state::Zoom::Quarters }
                    else if span > 200 { crate::state::Zoom::Months }
                    else if span > 60 { crate::state::Zoom::Weeks }
                    else { crate::state::Zoom::Days };
                state.write().zoom = zoom;
            },
        }

        div { class: "ctxsep" }

        Item {
            glyph: String::new(), label: "Critical Tasks".to_string(),
            shortcut: String::new(), enabled: true, checked: Some(critical),
            on: move |_| { let on = state.read().show_critical; state.write().show_critical = !on; },
        }
        Item {
            glyph: String::new(), label: "Slack".to_string(),
            shortcut: String::new(), enabled: true, checked: Some(slack),
            on: move |_| { let on = state.read().show_slack; state.write().show_slack = !on; },
        }
        Item {
            glyph: String::new(), label: "Baseline".to_string(),
            shortcut: String::new(), enabled: has_baseline, checked: Some(baseline),
            on: move |_| { let on = state.read().show_baseline; state.write().show_baseline = !on; },
        }
        Item {
            glyph: String::new(), label: "Timeline".to_string(),
            shortcut: String::new(), enabled: true, checked: Some(timeline),
            on: move |_| { let on = state.read().show_timeline; state.write().show_timeline = !on; },
        }
        Item {
            glyph: String::new(), label: "Outline Number".to_string(),
            shortcut: String::new(), enabled: true, checked: Some(outline),
            on: move |_| { let on = state.read().show_outline_number; state.write().show_outline_number = !on; },
        }

        div { class: "ctxsep" }

        Item {
            glyph: "baseline".to_string(), label: "Set Baseline".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().set_baseline(),
        }
        Item {
            glyph: "network".to_string(), label: "Network Diagram".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().view = ViewKind::NetworkDiagram,
        }
        Item {
            glyph: "project-info".to_string(), label: "Project Information...".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().dialog = Some(Dialog::ProjectInformation),
        }
    }
}

#[component]
fn ResourceMenu(index: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (name, has_task) = {
        let s = state.read();
        (
            s.project
                .resources
                .get(index)
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            s.primary().is_some(),
        )
    };

    rsx! {
        div { class: "ctxheader", "{name}" }

        Item {
            glyph: "assign-resources".to_string(),
            label: "Assign to Selected Task".to_string(),
            shortcut: String::new(), enabled: has_task, checked: None,
            on: move |_| state.write().toggle_assignment(index),
        }
        Item {
            glyph: "add-resource".to_string(), label: "Add Resource".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().add_resource("New Resource"),
        }
        Item {
            glyph: "clear".to_string(), label: "Delete Resource".to_string(),
            shortcut: "Del".to_string(), enabled: true, checked: None,
            on: move |_| state.write().delete_resource(index),
        }

        div { class: "ctxsep" }

        Item {
            glyph: "team-planner".to_string(), label: "Team Planner".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().view = ViewKind::TeamPlanner,
        }
        Item {
            glyph: "resource-usage".to_string(), label: "Resource Usage".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().view = ViewKind::ResourceUsage,
        }
    }
}


#[component]
fn ColumnMenu(index: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (label, only_column, at_start, at_end) = {
        let s = state.read();
        let Some(column) = s.columns.get(index) else {
            return rsx! { div { class: "ctxheader", "That column has gone" } };
        };
        (
            column.field.label().to_string(),
            s.columns.len() <= 1,
            index == 0,
            index + 1 == s.columns.len(),
        )
    };

    rsx! {
        div { class: "ctxheader", "{label}" }

        Item {
            glyph: "insert-column".to_string(), label: "Insert Column...".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().dialog = Some(Dialog::InsertColumn(index)),
        }
        Item {
            glyph: "clear".to_string(), label: "Hide Column".to_string(),
            shortcut: String::new(), enabled: !only_column, checked: None,
            on: move |_| state.write().remove_column(index),
        }

        div { class: "ctxsep" }

        Item {
            glyph: "move".to_string(), label: "Move Left".to_string(),
            shortcut: String::new(), enabled: !at_start, checked: None,
            on: move |_| state.write().move_column(index, -1),
        }
        Item {
            glyph: "move".to_string(), label: "Move Right".to_string(),
            shortcut: String::new(), enabled: !at_end, checked: None,
            on: move |_| state.write().move_column(index, 1),
        }

        div { class: "ctxsep" }

        Item {
            glyph: "column-settings".to_string(), label: "Reset to the Entry table".to_string(),
            shortcut: String::new(), enabled: true, checked: None,
            on: move |_| state.write().reset_columns(),
        }
    }
}
