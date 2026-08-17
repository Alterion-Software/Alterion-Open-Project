//! The views other than the Gantt chart.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;

use aop_core::{format_duration, format_work, ResourceKind, TaskId};

use crate::gantt::{chart_range, Scale};
use crate::state::{format_date, AppState};

// -------------------------------------------------------- resource sheet

#[component]
fn ResourceCell(index: usize, field: String, initial: String) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut draft = use_signal(|| initial.clone());
    let mut settled = use_signal(|| false);
    // Held in a signal so the commit closure stays Copy and can be used twice.
    let key = use_signal(|| field.clone());

    let mut commit = move || {
        if settled() {
            return;
        }
        settled.set(true);
        let text = draft();
        state.write().commit_resource_cell(index, &key(), &text);
        state.write().selected_resource = None;
    };

    rsx! {
        input {
            class: "cell-input",
            autofocus: true,
            value: "{draft}",
            // Clicks inside the editor must not reach the row underneath:
            // selecting a row clears the edit, so moving the caret with the
            // mouse would otherwise throw away what was being typed.
            onclick: move |event| event.stop_propagation(),
            onmousedown: move |event| event.stop_propagation(),
            ondoubleclick: move |event| event.stop_propagation(),
            onmouseup: move |event| event.stop_propagation(),
            oninput: move |event| draft.set(event.value()),
            onblur: move |_| commit(),
            onkeydown: move |event| match event.key() {
                Key::Enter => commit(),
                Key::Escape => {
                    settled.set(true);
                    state.write().selected_resource = None;
                }
                _ => {}
            },
        }
    }
}

#[component]
pub fn ResourceSheet() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;
    let currency = project.currency_symbol.clone();

    // Overallocated resources are called out in red, as in Project.
    let overallocated: Vec<u32> = s
        .report
        .as_ref()
        .map(|r| r.overallocations.iter().map(|o| o.resource).collect())
        .unwrap_or_default();

    // Booked work per resource, summed across every leaf task.
    let mut booked: HashMap<u32, i64> = HashMap::new();
    for index in 0..project.tasks.len() {
        if project.is_summary(index) {
            continue;
        }
        let task = &project.tasks[index];
        for assignment in &task.assignments {
            *booked.entry(assignment.resource).or_insert(0) +=
                (task.duration_minutes as f64 * assignment.units).round() as i64;
        }
    }

    rsx! {
        div { class: "sheet-pane",
            table { class: "sheet",
                colgroup {
                    col { style: "width: 40px;" }
                    col { style: "width: 200px;" }
                    col { style: "width: 90px;" }
                    col { style: "width: 70px;" }
                    col { style: "width: 130px;" }
                    col { style: "width: 90px;" }
                    col { style: "width: 110px;" }
                    col { style: "width: 110px;" }
                    col { style: "width: 100px;" }
                    col { style: "width: 120px;" }
                }
                thead {
                    tr {
                        th { "ID" }
                        th { "Resource Name" }
                        th { "Type" }
                        th { "Initials" }
                        th { "Group" }
                        th { "Max. Units" }
                        th { "Std. Rate" }
                        th { "Work Booked" }
                        th { "Cost" }
                        th { "Base Calendar" }
                    }
                }
                tbody {
                    for (index, resource) in project.resources.iter().enumerate() {
                        {
                            let over = overallocated.contains(&resource.id);
                            let selected = s.selected_resource == Some(index);
                            let mut class = String::new();
                            if over { class.push_str("over "); }
                            if selected { class.push_str("selected"); }
                            let minutes = booked.get(&resource.id).copied().unwrap_or(0);
                            let cost = minutes as f64 / 60.0 * resource.standard_rate;
                            let editing_field = s.editing_resource_field.clone();

                            rsx! {
                                tr { key: "res{index}", class: "{class}",
                                    onclick: move |_| state.write().selected_resource = Some(index),
                                    oncontextmenu: move |event| {
                                        event.prevent_default();
                                        let point = event.client_coordinates();
                                        state.write().open_resource_menu(index, point.x, point.y);
                                    },

                                    td { "{index + 1}" }
                                    td {
                                        ondoubleclick: move |_| {
                                            state.write().selected_resource = Some(index);
                                            state.write().editing_resource_field = Some("name".into());
                                        },
                                        if selected && editing_field.as_deref() == Some("name") {
                                            ResourceCell { index, field: "name".to_string(), initial: resource.name.clone() }
                                        } else {
                                            "{resource.name}"
                                        }
                                    }
                                    td { "{resource.kind.label()}" }
                                    td { "{resource.initials}" }
                                    td {
                                        ondoubleclick: move |_| {
                                            state.write().selected_resource = Some(index);
                                            state.write().editing_resource_field = Some("group".into());
                                        },
                                        if selected && editing_field.as_deref() == Some("group") {
                                            ResourceCell { index, field: "group".to_string(), initial: resource.group.clone() }
                                        } else {
                                            "{resource.group}"
                                        }
                                    }
                                    td { "{resource.max_units * 100.0:.0}%" }
                                    td {
                                        ondoubleclick: move |_| {
                                            state.write().selected_resource = Some(index);
                                            state.write().editing_resource_field = Some("rate".into());
                                        },
                                        if selected && editing_field.as_deref() == Some("rate") {
                                            ResourceCell { index, field: "rate".to_string(),
                                                initial: format!("{:.2}", resource.standard_rate) }
                                        } else {
                                            "{currency}{resource.standard_rate:.2}/hr"
                                        }
                                    }
                                    td {
                                        if resource.kind == ResourceKind::Work {
                                            "{format_work(minutes)}"
                                        }
                                    }
                                    td { "{currency}{cost:.2}" }
                                    td { "{resource.base_calendar}" }
                                }
                            }
                        }
                    }

                    tr { class: "add-row",
                        td { "{project.resources.len() + 1}" }
                        td {
                            onclick: move |_| state.write().add_resource("New Resource"),
                            "Click to add a resource"
                        }
                        td {} td {} td {} td {} td {} td {} td {} td {}
                    }
                }
            }

            if project.resources.is_empty() {
                div { class: "hint", style: "padding: 14px;",
                    "Add people, equipment or materials here, then book them onto tasks from the Resource Names column or the Assign Resources dialog."
                }
            }
        }
    }
}

// ------------------------------------------------------------ task usage

#[component]
pub fn TaskUsage() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    rsx! {
        div { class: "sheet-pane",
            table { class: "sheet",
                colgroup {
                    col { style: "width: 46px;" }
                    col { style: "width: 320px;" }
                    col { style: "width: 100px;" }
                    col { style: "width: 110px;" }
                    col { style: "width: 130px;" }
                    col { style: "width: 130px;" }
                }
                thead {
                    tr {
                        th { "ID" }
                        th { "Task Name" }
                        th { "Work" }
                        th { "Duration" }
                        th { "Start" }
                        th { "Finish" }
                    }
                }
                tbody {
                    for index in 0..project.tasks.len() {
                        {
                            let task = &project.tasks[index];
                            let summary = project.is_summary(index);
                            let indent = task.outline_level as f64 * 14.0;
                            let weight = if summary { "700" } else { "400" };
                            rsx! {
                                tr { key: "tu{index}",
                                    td { "{index + 1}" }
                                    td { style: "padding-left: {indent + 6.0}px; font-weight: {weight};", "{task.name}" }
                                    td { "{format_work(task.scheduled.work_minutes)}" }
                                    td { "{format_duration(task.scheduled.duration_minutes)}" }
                                    td { "{format_date(task.scheduled.start)}" }
                                    td { "{format_date(task.scheduled.finish)}" }
                                }
                                // Assignment rows sit under their task, indented further.
                                for assignment in task.assignments.iter() {
                                    {
                                        let name = project
                                            .resource(assignment.resource)
                                            .map(|r| r.name.clone())
                                            .unwrap_or_default();
                                        let minutes = (task.duration_minutes as f64 * assignment.units).round() as i64;
                                        rsx! {
                                            tr { key: "tu{index}a{assignment.resource}",
                                                style: "color: var(--ink-soft);",
                                                td { "" }
                                                td { style: "padding-left: {indent + 28.0}px; font-style: italic;", "{name}" }
                                                td { "{format_work(minutes)}" }
                                                td { "{assignment.units * 100.0:.0}%" }
                                                td { "{format_date(task.scheduled.start)}" }
                                                td { "{format_date(task.scheduled.finish)}" }
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
    }
}

// -------------------------------------------------------- network diagram

const NODE_W: f64 = 168.0;
const NODE_H: f64 = 62.0;
const COL_GAP: f64 = 56.0;
const ROW_GAP: f64 = 26.0;

#[component]
pub fn NetworkDiagram() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    let leaves: Vec<usize> = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i))
        .collect();

    if leaves.is_empty() {
        return rsx! {
            div { class: "network-pane",
                div { class: "empty-state", "Add tasks to see the network diagram" }
            }
        };
    }

    // Column = longest chain of predecessors, so dependencies always run left
    // to right. Relaxing repeatedly is enough for the sizes a plan reaches.
    let mut depth: HashMap<TaskId, usize> = leaves
        .iter()
        .map(|&i| (project.tasks[i].id, 0usize))
        .collect();
    for _ in 0..leaves.len().min(200) {
        let mut changed = false;
        for link in &project.links {
            let Some(&pred) = depth.get(&link.predecessor) else {
                continue;
            };
            if let Some(succ) = depth.get_mut(&link.successor) {
                if *succ < pred + 1 {
                    *succ = pred + 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut per_column: HashMap<usize, usize> = HashMap::new();
    let mut placed: HashMap<TaskId, (f64, f64)> = HashMap::new();
    let mut nodes: Vec<(usize, f64, f64)> = Vec::new();

    for &index in &leaves {
        let task = &project.tasks[index];
        let column = depth.get(&task.id).copied().unwrap_or(0);
        let row = per_column.entry(column).or_insert(0);
        let x = column as f64 * (NODE_W + COL_GAP);
        let y = *row as f64 * (NODE_H + ROW_GAP);
        *row += 1;
        placed.insert(task.id, (x, y));
        nodes.push((index, x, y));
    }

    let width = placed.values().map(|(x, _)| x + NODE_W).fold(400.0, f64::max) + 40.0;
    let height = placed.values().map(|(_, y)| y + NODE_H).fold(300.0, f64::max) + 40.0;

    rsx! {
        div { class: "network-pane",
            div { style: "position: relative; width: {width}px; height: {height}px;",
                svg {
                    style: "position: absolute; inset: 0; pointer-events: none;",
                    width: "{width}", height: "{height}",
                    for (index, link) in project.links.iter().enumerate() {
                        {
                            match (placed.get(&link.predecessor), placed.get(&link.successor)) {
                                (Some(&(x1, y1)), Some(&(x2, y2))) => {
                                    let sx = x1 + NODE_W;
                                    let sy = y1 + NODE_H / 2.0;
                                    let ex = x2;
                                    let ey = y2 + NODE_H / 2.0;
                                    let mid = (sx + ex) / 2.0;
                                    let d = format!("M{sx},{sy} L{mid},{sy} L{mid},{ey} L{ex},{ey}");
                                    rsx! {
                                        g { key: "nl{index}",
                                            path { d: "{d}", fill: "none", stroke: "var(--link-arrow)", stroke_width: "1.2" }
                                            polygon {
                                                points: "{ex - 6.0},{ey - 3.5} {ex - 6.0},{ey + 3.5} {ex},{ey}",
                                                fill: "var(--link-arrow)",
                                            }
                                        }
                                    }
                                }
                                _ => rsx! {},
                            }
                        }
                    }
                }

                for (index, x, y) in nodes {
                    {
                        let task = &project.tasks[index];
                        let mut class = String::from("node");
                        if task.scheduled.critical && s.show_critical { class.push_str(" critical"); }
                        rsx! {
                            div { key: "nn{index}", class: "{class}",
                                style: "left: {x}px; top: {y}px; height: {NODE_H}px;",
                                div { class: "n-name", "{task.name}" }
                                div { class: "n-row",
                                    span { "ID {index + 1}" }
                                    span { "{format_duration(task.scheduled.duration_minutes)}" }
                                }
                                div { class: "n-row",
                                    span { "{format_date(task.scheduled.start)}" }
                                }
                                div { class: "n-row",
                                    span { "{format_date(task.scheduled.finish)}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------- calendar view

#[component]
pub fn CalendarView() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    let (from, to) = chart_range(project);
    let first_month = NaiveDate::from_ymd_opt(from.year(), from.month(), 1).unwrap_or(from);

    // Cap how many months are drawn so a long plan stays responsive.
    let mut months: Vec<NaiveDate> = Vec::new();
    let mut cursor = first_month;
    while cursor < to && months.len() < 6 {
        months.push(cursor);
        cursor = if cursor.month() == 12 {
            NaiveDate::from_ymd_opt(cursor.year() + 1, 1, 1).unwrap_or(to)
        } else {
            NaiveDate::from_ymd_opt(cursor.year(), cursor.month() + 1, 1).unwrap_or(to)
        };
    }

    rsx! {
        div { class: "calendar-pane",
            for month in months {
                {
                    let label = month.format("%B %Y").to_string();
                    let days_in_month = {
                        let next = if month.month() == 12 {
                            NaiveDate::from_ymd_opt(month.year() + 1, 1, 1)
                        } else {
                            NaiveDate::from_ymd_opt(month.year(), month.month() + 1, 1)
                        };
                        next.map(|n| (n - month).num_days()).unwrap_or(30)
                    };
                    let lead = month.weekday().num_days_from_monday() as i64;

                    rsx! {
                        div { key: "cal{month}", style: "margin-bottom: 22px;",
                            h3 { style: "font-size: 14px; font-weight: 600; margin: 0 0 8px;", "{label}" }
                            div { class: "cal-grid",
                                for day_name in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                                    div { key: "dow{day_name}", class: "cal-dow", "{day_name}" }
                                }
                                for slot in 0..(lead + days_in_month) {
                                    {
                                        if slot < lead {
                                            rsx! { div { key: "blank{slot}", class: "cal-cell nonworking" } }
                                        } else {
                                            let date = month + Duration::days(slot - lead);
                                            let working = project.calendar.is_working_day(date);
                                            let class = if working { "cal-cell" } else { "cal-cell nonworking" };
                                            // Tasks whose span covers this day.
                                            let on_this_day: Vec<usize> = (0..project.tasks.len())
                                                .filter(|&i| {
                                                    let t = &project.tasks[i];
                                                    t.scheduled.start.date() <= date
                                                        && t.scheduled.finish.date() >= date
                                                })
                                                .take(4)
                                                .collect();
                                            rsx! {
                                                div { key: "cell{slot}", class: "{class}",
                                                    div { class: "d", "{date.day()}" }
                                                    for index in on_this_day {
                                                        {
                                                            let task = &project.tasks[index];
                                                            let mut chip = String::from("cal-chip");
                                                            if project.is_summary(index) { chip.push_str(" summary"); }
                                                            else if task.scheduled.critical && s.show_critical { chip.push_str(" critical"); }
                                                            rsx! {
                                                                div { key: "chip{index}", class: "{chip}", title: "{task.name}",
                                                                    "{task.name}"
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
                        }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------ team planner

#[component]
pub fn TeamPlanner() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    if project.resources.is_empty() {
        return rsx! {
            div { class: "network-pane",
                div { class: "empty-state",
                    "No resources yet.\nAdd them on the Resource tab, then book them onto tasks."
                }
            }
        };
    }

    let (from, to) = chart_range(project);
    let scale = Scale {
        origin: from,
        px_per_day: s.zoom.px_per_day(),
    };
    let width = ((to - from).num_days() as f64 * scale.px_per_day).max(600.0);
    let lane = 34.0;
    // One lane per resource, plus a final lane for unassigned work.
    let height = (project.resources.len() + 2) as f64 * lane + 20.0;

    let unassigned: Vec<usize> = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i) && project.tasks[i].assignments.is_empty())
        .collect();

    rsx! {
        div { class: "chart-pane", style: "width: {width + 190.0}px;",
            svg { width: "{width + 190.0}", height: "{height}",
                for (slot, resource) in project.resources.iter().enumerate() {
                    {
                        let y = slot as f64 * lane;
                        let assigned: Vec<usize> = (0..project.tasks.len())
                            .filter(|&i| {
                                !project.is_summary(i)
                                    && project.tasks[i]
                                        .assignments
                                        .iter()
                                        .any(|a| a.resource == resource.id)
                            })
                            .collect();
                        rsx! {
                            g { key: "tp{slot}",
                                rect { x: "0", y: "{y}", width: "{width + 190.0}", height: "{lane}",
                                    fill: if slot % 2 == 0 { "#fff" } else { "#fafafa" } }
                                line { x1: "0", y1: "{y}", x2: "{width + 190.0}", y2: "{y}",
                                    stroke: "var(--grid-line)", stroke_width: "1" }
                                text { x: "8", y: "{y + lane / 2.0 + 4.0}",
                                    style: "font-size: 11px; fill: var(--ink);", "{resource.name}" }
                                line { x1: "185", y1: "0", x2: "185", y2: "{height}",
                                    stroke: "var(--line)", stroke_width: "1" }

                                for index in assigned {
                                    {
                                        let task = &project.tasks[index];
                                        let left = 190.0 + scale.x_work(&project.calendar, task.scheduled.start);
                                        let right = 190.0 + scale.x_work(&project.calendar, task.scheduled.finish);
                                        let w = (right - left).max(3.0);
                                        let fill = if task.scheduled.critical && s.show_critical {
                                            "var(--bar-critical)"
                                        } else {
                                            "var(--bar)"
                                        };
                                        rsx! {
                                            g { key: "tpb{index}",
                                                rect { x: "{left}", y: "{y + 8.0}", width: "{w}", height: "18",
                                                    rx: "2", fill: "{fill}" }
                                                text { x: "{left + 5.0}", y: "{y + 20.0}",
                                                    style: "font-size: 10px; fill: #fff;", "{task.name}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Unassigned tasks get their own lane at the bottom.
                {
                    let y = project.resources.len() as f64 * lane;
                    rsx! {
                        g {
                            rect { x: "0", y: "{y}", width: "{width + 190.0}", height: "{lane}", fill: "#f4f4f4" }
                            line { x1: "0", y1: "{y}", x2: "{width + 190.0}", y2: "{y}",
                                stroke: "var(--grid-line)", stroke_width: "1" }
                            text { x: "8", y: "{y + lane / 2.0 + 4.0}",
                                style: "font-size: 11px; fill: var(--ink-soft); font-style: italic;", "Unassigned" }
                            for index in unassigned {
                                {
                                    let task = &project.tasks[index];
                                    let left = 190.0 + scale.x_work(&project.calendar, task.scheduled.start);
                                    let right = 190.0 + scale.x_work(&project.calendar, task.scheduled.finish);
                                    let w = (right - left).max(3.0);
                                    rsx! {
                                        rect { key: "un{index}", x: "{left}", y: "{y + 8.0}",
                                            width: "{w}", height: "18", rx: "2", fill: "var(--bar-inactive)" }
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
