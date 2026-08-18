//! Popup cell editors: a predecessor picker and a resource picker, opened from
//! the grid instead of making the user type raw cell syntax.

use dioxus::prelude::*;

use aop_core::{format_duration, parse_duration, LinkType, TaskId};

use crate::controls::{Choice, Dropdown};
use crate::icons::icon;
use crate::state::{AppState, Column};

/// Shared shell: a scrim that closes on click, plus an anchored panel.
#[component]
fn Anchored(width: f64, children: Element) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (x, y) = state.read().popup_at;
    let left = x.max(6.0);
    let top = (y + 4.0).max(6.0);

    rsx! {
        div {
            class: "ctx-scrim",
            onclick: move |_| state.write().editing = None,
            oncontextmenu: move |event| {
                event.prevent_default();
                state.write().editing = None;
            },
        }
        div {
            class: "ctxmenu",
            style: "left: {left}px; top: {top}px; width: {width}px; max-height: 70vh; overflow-y: auto; padding: 10px;",
            onclick: move |event| event.stop_propagation(),
            {children}
        }
    }
}

// ------------------------------------------------------------ predecessors

#[component]
pub fn PredecessorPopup(row: usize) -> Element {
    rsx! {
        Anchored { width: 560.0,
            PredecessorPicker { row }
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

/// The predecessor picker itself: every task that could be depended on, shown
/// at its outline level with a tick box, plus the typed form for planners who
/// know the row numbers.
///
/// Used both by the popup the grid opens and by the Predecessors tab of Task
/// Information, so the two cannot drift apart.
#[component]
pub fn PredecessorPicker(row: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // Everything that could be a predecessor: the whole outline except this
    // task and the rows nested underneath it, which cannot depend on it.
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
        let existing: Vec<(TaskId, LinkType, i64)> = project
            .predecessors_of(task.id)
            .into_iter()
            .map(|l| (l.predecessor, l.kind, l.lag_minutes))
            .collect();

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

    // Seeded with what the cell already says, so typing edits rather than
    // starts from nothing.
    let mut typed = use_signal(|| {
        let s = state.read();
        s.project
            .tasks
            .get(row)
            .map(|task| s.project.predecessor_text(task.id))
            .unwrap_or_default()
    });
    let mut commit = move || {
        let text = typed();
        state.write().commit_cell(row, Column::Predecessors, &text);
    };

    rsx! {
        div { class: "picker",
            div { class: "ctxheader", "Predecessors of {task_name}" }

            if rows.is_empty() {
                div { class: "hint", "There is no other task to depend on yet." }
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
                                                state.write().remove_link(row, id);
                                            } else {
                                                state.write().set_link(row, id, LinkType::FS, 0);
                                            }
                                        },
                                        span { class: "{box_class}", if checked { "\u{2713}" } }
                                        span { class: "pred-id", "{entry.number}" }
                                        span { class: "pred-name", "{entry.name}" }
                                    }

                                    // Type and lag only matter once it is picked.
                                    if checked {
                                        div { class: "pred-detail",
                                            Dropdown {
                                                value: kind.code().to_string(),
                                                options: LinkType::ALL.iter()
                                                    .map(|k| Choice::new(k.code(), k.label()))
                                                    .collect(),
                                                width: 62.0, large: false, disabled: false,
                                                on_pick: move |picked: String| {
                                                    let chosen = LinkType::parse(&picked).unwrap_or(LinkType::FS);
                                                    state.write().set_link(row, id, chosen, lag);
                                                },
                                            }
                                            input {
                                                class: "rselect", style: "width: 92px;",
                                                title: "Lag; use a negative value to overlap",
                                                value: "{signed_lag(lag)}",
                                                onchange: move |event| {
                                                    let minutes = parse_signed_lag(&event.value());
                                                    state.write().set_link(row, id, kind, minutes);
                                                },
                                            }
                                            button {
                                                class: "iconbtn danger",
                                                title: "Remove this link",
                                                onclick: move |_| state.write().remove_link(row, id),
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

            div { class: "hint",
                "Tick a task to depend on it. The type sets which ends are tied together, and lag "
                "delays the successor: 2 days waits, -1 day overlaps."
            }

            // Typing is faster than hunting for a row once you know the number,
            // so the same cell text Project accepts is accepted here.
            div { class: "pred-type",
                label { "Or type it" }
                input {
                    class: "bs-input",
                    placeholder: "3, 5FS+2d, 7SS",
                    value: "{typed}",
                    oninput: move |event| typed.set(event.value()),
                    onkeydown: move |event| if event.key() == Key::Enter { commit() },
                }
                button { class: "btn", onclick: move |_| commit(), "Set" }
            }

            div { class: "pred-foot",
                span { class: "recent-path",
                    {
                        if chosen == 1 {
                            "1 predecessor".to_string()
                        } else {
                            format!("{chosen} predecessors")
                        }
                    }
                }
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
                                                state.write().set_assignment(
                                                    row, id,
                                                    if assigned { None } else { Some(1.0) },
                                                );
                                            },
                                            span { class: "{box_class}", style: "display: inline-grid; width: 12px; height: 12px;",
                                                if assigned { "\u{2713}" } }
                                        }
                                        td {
                                            onclick: move |_| {
                                                state.write().set_assignment(
                                                    row, id,
                                                    if assigned { None } else { Some(1.0) },
                                                );
                                            },
                                            "{label}"
                                        }
                                        td { "{currency}{rate:.2}/hr" }
                                        td {
                                            if assigned {
                                                input {
                                                    class: "rselect", style: "width: 100%;",
                                                    value: "{shown * 100.0:.0}%",
                                                    onchange: move |event| {
                                                        let cleaned = event.value().trim().trim_end_matches('%').to_string();
                                                        if let Ok(percent) = cleaned.parse::<f64>() {
                                                            state.write().set_assignment(row, id, Some((percent / 100.0).max(0.0)));
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
