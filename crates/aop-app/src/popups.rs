//! Popup cell editors: a dependency picker and a resource picker, opened from
//! the grid instead of making the user type raw cell syntax.
//!
//! The dependency picker serves both ends of a link. Predecessors and
//! successors are the same rows of `Project.links` read from opposite sides,
//! so they are picked by one component pointed either way.

use dioxus::prelude::*;

use crate::controls::{Choice, Dropdown};
use aop_core::{format_duration, parse_duration, LinkType, TaskId};

use crate::icons::icon;
use crate::state::{AppState, Column};

/// What the anchored panel is dressed and placed by.
///
/// Two classes, and the split is the point. `ctxmenu` is the look of a menu
/// panel and nothing else; `ctx-anchored` is what takes this one out of the
/// flow so that writing coordinates into its style places it. A context menu
/// gets that second half from the `ctx-stack` it shares with its mini toolbar,
/// which this has no use for, so it carries its own.
pub const ANCHORED_CLASS: &str = "ctx-anchored ctxmenu";

/// Shared shell: a scrim that closes on click, plus an anchored panel.
#[component]
fn Anchored(width: f64, children: Element) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (x, y) = state.read().popup_at;
    let (view_w, view_h) = use_context::<Signal<crate::state::Viewport>>()();

    // Kept inside the window when the window's size is known, and placed
    // where it was asked for when it is not. See `crate::placement` for why
    // that distinction is the whole point.
    let horizontal = crate::placement::horizontal(x, width, (view_w, view_h));
    let vertical = crate::placement::vertical(y, (view_w, view_h));

    rsx! {
        div {
            class: "ctx-scrim",
            // The cell's own text box commits on blur, and clicking here
            // blurs it, so this only has to close the picker.
            onclick: move |_| state.write().editing = None,
            oncontextmenu: move |event| {
                event.prevent_default();
                state.write().editing = None;
            },
        }
        div {
            class: "{ANCHORED_CLASS}",
            style: "{horizontal} {vertical} width: {width}px; max-height: 70vh; overflow-y: auto; padding: 10px;",
            onclick: move |event| event.stop_propagation(),
            // Keeps the caret in the cell's own text box while boxes are
            // ticked, so typing and picking are one continuous edit.
            onmousedown: move |event| event.prevent_default(),
            {children}
        }
    }
}

// ------------------------------------------------------------- dependencies

/// Which end of a link a picker is being pointed at.
///
/// The picker is one component rather than two because a successor is not a
/// second kind of relationship: it is the same `Link` read from the other end.
/// Two components would be two chances to disagree about what a link is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEnd {
    /// The listed tasks come before the task being edited.
    Predecessors,
    /// The listed tasks wait on the task being edited.
    Successors,
}

impl LinkEnd {
    fn heading(self, task: &str) -> String {
        match self {
            LinkEnd::Predecessors => format!("Predecessors of {task}"),
            LinkEnd::Successors => format!("Successors of {task}"),
        }
    }

    fn empty_hint(self) -> &'static str {
        match self {
            LinkEnd::Predecessors => "There is no other task to depend on yet.",
            LinkEnd::Successors => "There is no other task to wait on this one yet.",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            LinkEnd::Predecessors => {
                "Tick a task to depend on it. The type sets which ends are tied together, and lag                  delays the successor: 2 days waits, -1 day overlaps."
            }
            LinkEnd::Successors => {
                "Tick a task to make it wait on this one. The type sets which ends are tied                  together, and lag delays the successor: 2 days waits, -1 day overlaps."
            }
        }
    }

    fn tally(self, chosen: usize) -> String {
        match (self, chosen) {
            (LinkEnd::Predecessors, 1) => "1 predecessor".to_string(),
            (LinkEnd::Predecessors, n) => format!("{n} predecessors"),
            (LinkEnd::Successors, 1) => "1 successor".to_string(),
            (LinkEnd::Successors, n) => format!("{n} successors"),
        }
    }
}

#[component]
pub fn PredecessorPopup(row: usize) -> Element {
    rsx! {
        Anchored { width: 560.0,
            LinkPicker { row, end: LinkEnd::Predecessors }
            div { style: "display: flex; justify-content: flex-end; margin-top: 10px;",
                DonePopup {}
            }
        }
    }
}

#[component]
pub fn SuccessorPopup(row: usize) -> Element {
    rsx! {
        Anchored { width: 560.0,
            LinkPicker { row, end: LinkEnd::Successors }
            div { style: "display: flex; justify-content: flex-end; margin-top: 10px;",
                DonePopup {}
            }
        }
    }
}

/// Closes whichever picker popup is open.
#[component]
fn DonePopup() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    rsx! {
        button { class: "btn", onclick: move |_| state.write().editing = None, "Done" }
    }
}

/// The dependency picker itself: every task that could be on the other end of a
/// link, shown at its outline level with a tick box, plus the typed form for
/// planners who know the row numbers.
///
/// Used by the popup the grid opens and by the Predecessors and Successors tabs
/// of Task Information, so none of them can drift apart.
#[component]
pub fn LinkPicker(row: usize, end: LinkEnd) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // Everything that could be on the other end: the whole outline except this
    // task and the rows nested underneath it, which cannot be tied to it in
    // either direction. A summary already spans its children, so a link
    // between the two would be a task waiting on itself.
    struct Row {
        id: TaskId,
        number: usize,
        name: String,
        level: u16,
        summary: bool,
        linked: Option<(LinkType, i64)>,
    }

    let (task_name, rows) = {
        let s = state.read();
        let project = &s.project;
        let Some(task) = project.tasks.get(row) else {
            return rsx! {};
        };
        let own = project.descendants(row);
        // The very same links either way round, only read from the end this
        // picker is pointed at.
        let existing: Vec<(TaskId, LinkType, i64)> = match end {
            LinkEnd::Predecessors => project
                .predecessors_of(task.id)
                .into_iter()
                .map(|l| (l.predecessor, l.kind, l.lag_minutes))
                .collect(),
            LinkEnd::Successors => project
                .successors_of(task.id)
                .into_iter()
                .map(|l| (l.successor, l.kind, l.lag_minutes))
                .collect(),
        };

        let rows: Vec<Row> = (0..project.tasks.len())
            .filter(|&index| index != row && !own.contains(&index))
            .map(|index| {
                let candidate = &project.tasks[index];
                Row {
                    id: candidate.id,
                    number: index + 1,
                    name: if candidate.name.trim().is_empty() {
                        "(unnamed task)".to_string()
                    } else {
                        candidate.name.clone()
                    },
                    level: candidate.outline_level,
                    summary: project.is_summary(index),
                    linked: existing
                        .iter()
                        .find(|(id, _, _)| *id == candidate.id)
                        .map(|(_, kind, lag)| (*kind, *lag)),
                }
            })
            .collect();
        (task.name.clone(), rows)
    };

    let chosen = rows.iter().filter(|r| r.linked.is_some()).count();
    let heading = end.heading(&task_name);

    // Ticking a box writes the plan directly, and which way round it is written
    // is the only thing that differs between the two ends.
    let mut set = move |other: TaskId, kind: LinkType, lag: i64| {
        let mut w = state.write();
        match end {
            LinkEnd::Predecessors => w.set_link(row, other, kind, lag),
            LinkEnd::Successors => w.set_successor_link(row, other, kind, lag),
        }
        w.refresh_cell_draft();
    };
    let mut clear = move |other: TaskId| {
        let mut w = state.write();
        match end {
            LinkEnd::Predecessors => w.remove_link(row, other),
            LinkEnd::Successors => w.remove_successor_link(row, other),
        }
        w.refresh_cell_draft();
    };

    rsx! {
        div { class: "picker",
            div { class: "ctxheader", "{heading}" }

            if rows.is_empty() {
                div { class: "hint", "{end.empty_hint()}" }
            } else {
                div { class: "pred-list",
                    for entry in rows.iter() {
                        {
                            let id = entry.id;
                            let checked = entry.linked.is_some();
                            let (kind, lag) = entry.linked.unwrap_or((LinkType::FS, 0));
                            let box_class = if checked { "box on" } else { "box" };
                            let mut row_class = String::from("pred-row");
                            if checked { row_class.push_str(" on"); }
                            if entry.summary { row_class.push_str(" summary"); }
                            let indent = 6.0 + entry.level as f64 * 15.0;

                            rsx! {
                                div { key: "p{id}", class: "{row_class}",
                                    // The whole row toggles, like Project's list.
                                    div {
                                        class: "pred-pick",
                                        style: "padding-left: {indent}px;",
                                        onclick: move |_| {
                                            if checked {
                                                clear(id);
                                            } else {
                                                set(id, LinkType::FS, 0);
                                            }
                                        },
                                        span { class: "{box_class}", if checked { "\u{2713}" } }
                                        span { class: "pred-id", "{entry.number}" }
                                        span { class: "pred-name", "{entry.name}" }
                                    }

                                    // Type and lag only matter once it is picked.
                                    //
                                    // The type is offered here rather than only in the typed
                                    // form. It was briefly taken out on the grounds that
                                    // finish to start covers most dependencies; a planner
                                    // put it straight back, because the other three are how
                                    // overlapping work is expressed and hunting for the
                                    // syntax is not a substitute for a control.
                                    //
                                    // Both belong to the link rather than to either task, so
                                    // changing one here changes exactly the same link the
                                    // other end is showing.
                                    if checked {
                                        div { class: "pred-detail",
                                            Dropdown {
                                                value: kind.code().to_string(),
                                                options: LinkType::ALL.iter()
                                                    .map(|k| Choice::new(k.code(), k.label()))
                                                    .collect(),
                                                width: 62.0, large: false, disabled: false,
                                                on_pick: move |picked: String| {
                                                    let picked = LinkType::parse(&picked).unwrap_or(LinkType::FS);
                                                    set(id, picked, lag);
                                                },
                                            }
                                            input {
                                                class: "rselect", style: "width: 92px;",
                                                title: "Lag; use a negative value to overlap",
                                                value: "{signed_lag(lag)}",
                                                // `oninput`, not `onchange`.
                                                //
                                                // The webview-free renderer has no change event at
                                                // all: `blitz_traits::events::DomEventData` runs
                                                // from `PointerMove` to `Ime` and there is no
                                                // `Change` in it, so a handler waiting for one
                                                // waits for ever and the field cannot be edited.
                                                // Input is raised on both builds.
                                                //
                                                // Committing per keystroke is right here and would
                                                // not be everywhere: a lag is a few characters, and
                                                // `parse_signed_lag` reads a half typed one as the
                                                // number it has so far rather than as an error.
                                                oninput: move |event| {
                                                    set(id, kind, parse_signed_lag(&event.value()));
                                                },
                                            }
                                            button {
                                                class: "iconbtn danger",
                                                title: "Remove this link",
                                                onclick: move |_| clear(id),
                                                {icon("clear", 13)}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "hint", "{end.explanation()}" }

            div { class: "pred-foot",
                span { class: "recent-path", {end.tally(chosen)} }
            }
        }
    }
}

pub fn signed_lag(minutes: i64) -> String {
    if minutes < 0 {
        format!("-{}", format_duration(-minutes))
    } else {
        format_duration(minutes)
    }
}

pub fn parse_signed_lag(text: &str) -> i64 {
    let trimmed = text.trim();
    let negative = trimmed.starts_with('-');
    let body = trimmed.trim_start_matches(['-', '+']).trim();
    let magnitude = parse_duration(body).map(|(m, _)| m).unwrap_or(0);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

// --------------------------------------------------------------- resources

#[component]
pub fn ResourcePopup(row: usize) -> Element {
    rsx! {
        Anchored { width: 460.0,
            ResourcePicker { row }
            div { style: "display: flex; justify-content: flex-end; margin-top: 10px;",
                DonePopup {}
            }
        }
    }
}

/// The resource picker itself: every resource with a tick box and its units,
/// plus the typed form Project accepts in the Resource Names cell.
///
/// Used both by the popup the grid opens and by the Resources tab of Task
/// Information, so the two cannot drift apart.
#[component]
pub fn ResourcePicker(row: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut new_name = use_signal(String::new);

    // Seeded with what the cell already says, so typing edits rather than
    // starts from nothing.
    let mut typed = use_signal(|| {
        let s = state.read();
        s.project
            .tasks
            .get(row)
            .map(|task| s.project.resource_text(task))
            .unwrap_or_default()
    });
    let mut commit = move || {
        let text = typed();
        state.write().commit_cell(row, Column::Resources, &text);
    };

    let (task_name, resources, currency) = {
        let s = state.read();
        let Some(task) = s.project.tasks.get(row) else {
            return rsx! {};
        };
        let resources: Vec<(usize, u32, String, String, f64, Option<f64>)> = s
            .project
            .resources
            .iter()
            .enumerate()
            .map(|(index, r)| {
                let units = task
                    .assignments
                    .iter()
                    .find(|a| a.resource == r.id)
                    .map(|a| a.units);
                (index, r.id, r.name.clone(), r.group.clone(), r.standard_rate, units)
            })
            .collect();
        (task.name.clone(), resources, s.project.currency_symbol.clone())
    };

    rsx! {
        div { class: "picker",
            div { class: "ctxheader", "Resources for {task_name}" }

            if resources.is_empty() {
                div { class: "hint", style: "margin: 4px 0 10px;",
                    "No resources defined yet. Add one below." }
            } else {
                table { class: "assign-table", style: "margin-bottom: 10px;",
                    thead {
                        tr {
                            th { style: "width: 30px;", "" }
                            th { "Resource" }
                            th { style: "width: 96px;", "Std. Rate" }
                            th { style: "width: 78px;", "Units" }
                        }
                    }
                    tbody {
                        for (index, id, name, group, rate, units) in resources {
                            {
                                let assigned = units.is_some();
                                let class = if assigned { "on" } else { "" };
                                let box_class = if assigned { "box on" } else { "box" };
                                let shown = units.unwrap_or(1.0);
                                let label = if group.is_empty() { name.clone() } else { format!("{name}  \u{00b7}  {group}") };
                                rsx! {
                                    tr { key: "rp{index}", class: "{class}",
                                        td {
                                            onclick: move |_| {
                                                let mut w = state.write();
                                                w.set_assignment(
                                                    row, id,
                                                    if assigned { None } else { Some(1.0) },
                                                );
                                                w.refresh_cell_draft();
                                            },
                                            span { class: "{box_class}", style: "display: inline-grid; width: 12px; height: 12px;",
                                                if assigned { "\u{2713}" } }
                                        }
                                        td {
                                            onclick: move |_| {
                                                let mut w = state.write();
                                                w.set_assignment(
                                                    row, id,
                                                    if assigned { None } else { Some(1.0) },
                                                );
                                                w.refresh_cell_draft();
                                            },
                                            "{label}"
                                        }
                                        td { "{currency}{rate:.2}/hr" }
                                        td {
                                            if assigned {
                                                input {
                                                    class: "rselect", style: "width: 100%;",
                                                    value: "{shown * 100.0:.0}%",
                                                    // See the lag field above: there is no change
                                                    // event on the webview-free build. A half typed
                                                    // percentage simply does not parse, and nothing
                                                    // is written until it does.
                                                    oninput: move |event| {
                                                        let cleaned = event.value().trim().trim_end_matches('%').to_string();
                                                        if let Ok(percent) = cleaned.parse::<f64>() {
                                                            let mut w = state.write();
                                                            w.set_assignment(row, id, Some((percent / 100.0).max(0.0)));
                                                            w.refresh_cell_draft();
                                                        }
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { style: "display: flex; gap: 6px; align-items: center;",
                input {
                    class: "rselect", style: "flex: 1; height: 26px;",
                    placeholder: "New resource name",
                    value: "{new_name}",
                    oninput: move |event| new_name.set(event.value()),
                }
                button {
                    class: "btn primary", style: "padding: 4px 12px;",
                    onclick: move |_| {
                        let name = new_name();
                        if !name.trim().is_empty() {
                            state.write().add_resource(name.trim());
                            new_name.set(String::new());
                        }
                    },
                    "Add"
                }
            }

            div { class: "pred-type",
                label { "Or type it" }
                input {
                    class: "bs-input",
                    placeholder: "Alice[50%], Bob",
                    value: "{typed}",
                    oninput: move |event| typed.set(event.value()),
                    onkeydown: move |event| if event.key() == Key::Enter { commit() },
                }
                button { class: "btn", onclick: move |_| commit(), "Set" }
            }

            div { class: "hint",
                "Units are the share of a person's time. 50% on a 5 day task books 20 hours of work." }
        }
    }
}
