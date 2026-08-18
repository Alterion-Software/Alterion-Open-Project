//! The views other than the Gantt chart.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;

use aop_core::{format_duration, format_work, ResourceKind, TaskId};

use crate::gantt::{chart_range, GanttChart, Scale};
use crate::state::{format_date, AppState, Dialog, ViewKind};

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
                                    ondoubleclick: move |_| {
                                        let mut writer = state.write();
                                        writer.selected_resource = Some(index);
                                        writer.dialog = Some(Dialog::ResourceInformation { row: index, tab: 0 });
                                    },
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
            if let Some(succ) = depth.get_mut(&link.successor)
                && *succ < pred + 1 {
                    *succ = pred + 1;
                    changed = true;
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
                        if s.show_critical && aop_core::issues::shows_as_critical(&s.project, index) { class.push_str(" critical"); }
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
                                                            else if s.show_critical && aop_core::issues::shows_as_critical(&s.project, index) { chip.push_str(" critical"); }
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

/// Cut a bar's label to the bar.
///
/// A name longer than its bar would run across the ones beside it and read as
/// belonging to the wrong task, which is worse than not showing it. Below a
/// certain width there is no room for anything meaningful, so nothing is drawn
/// and the full name stays available on hover.
fn fit_bar_label(name: &str, bar_width: f64) -> String {
    /// Roughly the width of a character at the size these labels are drawn.
    const CHAR_W: f64 = 5.4;
    const PADDING: f64 = 10.0;

    let room = ((bar_width - PADDING) / CHAR_W).floor();
    if room < 4.0 {
        return String::new();
    }
    let room = room as usize;
    if name.chars().count() <= room {
        return name.to_string();
    }
    name.chars().take(room.saturating_sub(1)).collect::<String>() + "\u{2026}"
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
    let unassigned_count = unassigned.len();

    // Which resources are booked past what they have. Spotting this is the
    // reason the view is laid out by resource rather than by task.
    let overallocated: Vec<aop_core::ResourceId> = s
        .report
        .as_ref()
        .map(|report| report.overallocations.iter().map(|o| o.resource).collect())
        .unwrap_or_default();

    rsx! {
        div { class: "chart-pane", style: "width: {width + 190.0}px;",
            svg { width: "{width + 190.0}", height: "{height}",
                for (slot, resource) in project.resources.iter().enumerate() {
                    {
                        let y = slot as f64 * lane;
                        let over = overallocated.contains(&resource.id);
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
                                    // Banded from the theme's own surfaces. Hard
                                    // coding white here is what made this view
                                    // a slab of glare on the dark palette. A
                                    // resource booked past its capacity is
                                    // marked here, since that is what the whole
                                    // by-resource layout exists to show.
                                    fill: if over {
                                        "var(--danger-bg)"
                                    } else if slot % 2 == 0 {
                                        "var(--surface)"
                                    } else {
                                        "var(--surface-3)"
                                    } }
                                line { x1: "0", y1: "{y}", x2: "{width + 190.0}", y2: "{y}",
                                    stroke: "var(--grid-line)", stroke_width: "1" }
                                text { x: "8", y: "{y + lane / 2.0}",
                                    style: "font-size: 11px; fill: var(--ink);", "{resource.name}" }
                                if over {
                                    text { x: "8", y: "{y + lane / 2.0 + 12.0}",
                                        style: "font-size: 9.5px; fill: var(--danger);",
                                        "{assigned.len()} task(s) \u{00b7} overallocated" }
                                } else {
                                    text { x: "8", y: "{y + lane / 2.0 + 12.0}",
                                        style: "font-size: 9.5px; fill: var(--ink-faint);",
                                        "{assigned.len()} task(s)" }
                                }
                                line { x1: "185", y1: "0", x2: "185", y2: "{height}",
                                    stroke: "var(--line)", stroke_width: "1" }

                                for index in assigned {
                                    {
                                        let task = &project.tasks[index];
                                        let left = 190.0 + scale.x_work(&project.calendar, task.scheduled.start);
                                        let right = 190.0 + scale.x_work(&project.calendar, task.scheduled.finish);
                                        let w = (right - left).max(3.0);
                                        let fill = if s.show_critical && aop_core::issues::shows_as_critical(&s.project, index) {
                                            "var(--bar-critical)"
                                        } else {
                                            "var(--bar)"
                                        };
                                        // Cut to the bar rather than allowed to
                                        // run over the one beside it.
                                        let label = fit_bar_label(&task.name, w);
                                        rsx! {
                                            g { key: "tpb{index}",
                                                title { "{task.name}" }
                                                rect { x: "{left}", y: "{y + 8.0}", width: "{w}", height: "18",
                                                    rx: "2", fill: "{fill}" }
                                                if !label.is_empty() {
                                                    text { x: "{left + 5.0}", y: "{y + 20.0}",
                                                        style: "font-size: 10px; fill: var(--on-bar);", "{label}" }
                                                }
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
                            rect { x: "0", y: "{y}", width: "{width + 190.0}", height: "{lane}",
                                fill: "var(--surface-2)" }
                            line { x1: "0", y1: "{y}", x2: "{width + 190.0}", y2: "{y}",
                                stroke: "var(--grid-line)", stroke_width: "1" }
                            text { x: "8", y: "{y + lane / 2.0}",
                                style: "font-size: 11px; fill: var(--ink-soft); font-style: italic;",
                                "Unassigned" }
                            text { x: "8", y: "{y + lane / 2.0 + 12.0}",
                                style: "font-size: 9.5px; fill: var(--ink-faint);",
                                "{unassigned_count} task(s) with nobody booked" }
                            for index in unassigned {
                                {
                                    let task = &project.tasks[index];
                                    let left = 190.0 + scale.x_work(&project.calendar, task.scheduled.start);
                                    let right = 190.0 + scale.x_work(&project.calendar, task.scheduled.finish);
                                    let w = (right - left).max(3.0);
                                    let label = fit_bar_label(&task.name, w);
                                    rsx! {
                                        g { key: "un{index}",
                                            title { "{task.name}" }
                                            rect { x: "{left}", y: "{y + 8.0}",
                                                width: "{w}", height: "18", rx: "2",
                                                fill: "var(--bar-inactive)" }
                                            if !label.is_empty() {
                                                text { x: "{left + 5.0}", y: "{y + 20.0}",
                                                    style: "font-size: 10px; fill: var(--ink);", "{label}" }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_fits_its_bar_is_shown_whole() {
        assert_eq!(fit_bar_label("Kickoff", 200.0), "Kickoff");
    }

    #[test]
    fn a_name_too_long_for_its_bar_is_cut_rather_than_left_to_overrun() {
        // Left alone it would run across the bars beside it and read as
        // belonging to the wrong task.
        let cut = fit_bar_label("Deliver workstream one and review", 80.0);
        assert!(cut.ends_with('\u{2026}'));
        assert!(cut.chars().count() < "Deliver workstream one and review".chars().count());
    }

    #[test]
    fn a_bar_with_no_room_shows_nothing_rather_than_an_ellipsis() {
        // A lone "…" says less than a bar with the name on hover.
        assert_eq!(fit_bar_label("Kickoff", 12.0), "");
        assert_eq!(fit_bar_label("Kickoff", 0.0), "");
    }

    #[test]
    fn a_negative_width_is_handled_rather_than_panicking() {
        assert_eq!(fit_bar_label("Kickoff", -50.0), "");
    }
}

// ---------------------------------------------------------------- reports

/// Where the plot sits inside the drawing, leaving room for the labels.
///
/// A chart with no axes is two lines floating in space: it shows a shape but
/// no quantity, and cannot be read off. The margins here are what the labels
/// live in.
struct Plot {
    width: f64,
    height: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Plot {
    fn new(width: f64, height: f64) -> Self {
        Plot {
            width,
            height,
            left: 56.0,
            top: 10.0,
            right: 14.0,
            bottom: 30.0,
        }
    }

    fn inner_w(&self) -> f64 {
        self.width - self.left - self.right
    }

    fn inner_h(&self) -> f64 {
        self.height - self.top - self.bottom
    }

    /// Where a point sits, given its position in the series and its value.
    fn point(&self, index: usize, count: usize, value: i64, peak: i64) -> (f64, f64) {
        let x = if count <= 1 {
            self.left
        } else {
            self.left + index as f64 / (count - 1) as f64 * self.inner_w()
        };
        let y = if peak <= 0 {
            self.top + self.inner_h()
        } else {
            self.top + self.inner_h() - (value as f64 / peak as f64) * self.inner_h()
        };
        (x, y)
    }

    fn path(&self, values: &[i64], peak: i64) -> String {
        if values.len() < 2 {
            return String::new();
        }
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let (x, y) = self.point(index, values.len(), *value, peak);
                format!("{x:.1},{y:.1}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One unit for a whole axis, chosen from its largest value.
///
/// Formatting each tick on its own gives a scale reading "200000 mins", then
/// "2500 hrs", then "0 days", which cannot be compared by eye and is worse
/// than no labels at all. The unit is named once, in the axis title.
fn axis_unit(peak: i64) -> (&'static str, f64) {
    const HOUR: f64 = 60.0;
    const DAY: f64 = 480.0;
    if peak as f64 >= DAY * 5.0 {
        ("days", DAY)
    } else if peak as f64 >= HOUR * 3.0 {
        ("hours", HOUR)
    } else {
        ("minutes", 1.0)
    }
}

/// A tick label in the axis's own unit, without repeating the unit each time.
fn axis_tick(value: i64, per_unit: f64) -> String {
    let scaled = value as f64 / per_unit;
    if scaled.fract().abs() < 0.05 {
        format!("{scaled:.0}")
    } else {
        format!("{scaled:.1}")
    }
}

/// Round a peak up to something a person would choose, so the top gridline is
/// a round number rather than whatever the data happened to reach.
fn nice_peak(raw: i64) -> i64 {
    if raw <= 0 {
        return 1;
    }
    let magnitude = 10i64.pow(raw.to_string().len() as u32 - 1);
    let steps = [1, 2, 5, 10];
    for step in steps {
        let candidate = magnitude * step;
        if candidate >= raw {
            return candidate;
        }
    }
    raw
}

/// The axes: gridlines, the values up the side, the dates along the bottom.
#[component]
fn Axes(
    plot_w: f64,
    plot_h: f64,
    peak: i64,
    dates: Vec<chrono::NaiveDate>,
    unit: String,
) -> Element {
    let plot = Plot::new(plot_w, plot_h);
    let steps = 4;
    let (unit_name, per_unit) = axis_unit(peak);

    // Six dates at most: more overlap and stop being readable.
    let stride = (dates.len() / 5).max(1);
    let ticks: Vec<(usize, chrono::NaiveDate)> = dates
        .iter()
        .enumerate()
        .filter(|(index, _)| index % stride == 0)
        .map(|(index, date)| (index, *date))
        .collect();

    rsx! {
        g { class: "axes",
            // Horizontal gridlines with the value they stand for.
            for step in 0..=steps {
                {
                    let value = peak * step / steps;
                    let y = plot.top + plot.inner_h() - (step as f64 / steps as f64) * plot.inner_h();
                    rsx! {
                        g { key: "y{step}",
                            line {
                                x1: "{plot.left}", y1: "{y:.1}",
                                x2: "{plot.left + plot.inner_w()}", y2: "{y:.1}",
                                stroke: "var(--grid-line)", stroke_width: "1",
                            }
                            text {
                                x: "{plot.left - 9.0}", y: "{y + 3.5:.1}",
                                text_anchor: "end", class: "axis-label",
                                "{axis_tick(value, per_unit)}"
                            }
                        }
                    }
                }
            }

            // The axes themselves, drawn heavier than the gridlines.
            line {
                x1: "{plot.left}", y1: "{plot.top}",
                x2: "{plot.left}", y2: "{plot.top + plot.inner_h()}",
                stroke: "var(--line)", stroke_width: "1.5",
            }
            line {
                x1: "{plot.left}", y1: "{plot.top + plot.inner_h()}",
                x2: "{plot.left + plot.inner_w()}", y2: "{plot.top + plot.inner_h()}",
                stroke: "var(--line)", stroke_width: "1.5",
            }

            for (index, date) in ticks {
                {
                    let (x, _) = plot.point(index, dates.len(), 0, peak);
                    let base = plot.top + plot.inner_h();
                    rsx! {
                        g { key: "x{index}",
                            line {
                                x1: "{x:.1}", y1: "{base}", x2: "{x:.1}", y2: "{base + 4.0}",
                                stroke: "var(--line)", stroke_width: "1",
                            }
                            text {
                                x: "{x:.1}", y: "{base + 16.0}",
                                text_anchor: "middle", class: "axis-label",
                                "{date.format(\"%d %b\")}"
                            }
                        }
                    }
                }
            }

            // What the numbers up the side are counting, said once.
            text {
                x: "11", y: "{plot.top + plot.inner_h() / 2.0}",
                class: "axis-title",
                transform: "rotate(-90 11 {plot.top + plot.inner_h() / 2.0})",
                text_anchor: "middle",
                "{unit} ({unit_name})"
            }
        }
    }
}

/// A report page: the figures first, then the chart, then the detail.
/// A report page: the figures first, then the chart, then the detail.
///
/// A chart on its own says a shape but not a number, which is why each of
/// these leads with the figures it is drawn from and closes with the rows
/// behind them. That is the difference between a graph and a report.
#[component]
pub fn ReportPage(kind: ViewKind) -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    if project.tasks.is_empty() {
        return rsx! {
            div { class: "network-pane",
                div { class: "empty-state", "Nothing to report yet.\nAdd tasks first." }
            }
        };
    }

    let metrics = aop_core::agile::metrics(project, s.iteration_days);
    let basis = metrics.basis.label();

    // Measured rather than assumed. A fixed coordinate space is centred in
    // whatever room it is given, which left the plot stranded in the middle of
    // the card with dead space either side.
    let mut measured = use_signal(|| None::<f64>);
    let w = measured().unwrap_or(900.0);
    let h = 300.0;

    rsx! {
        div {
            class: "reports-pane",
            onresize: move |event| {
                if let Ok(size) = event.get_content_box_size() {
                    // The card's padding and the pane's, so the plot ends where
                    // the card does.
                    let usable = (size.width - 72.0).max(320.0);
                    if (measured().unwrap_or(0.0) - usable).abs() >= 2.0 {
                        measured.set(Some(usable));
                    }
                }
            },
            match kind {
                ViewKind::Burndown => rsx! { BurndownPage { metrics: metrics.clone(), w, h, basis } },
                ViewKind::Burnup => rsx! { BurnupPage { metrics: metrics.clone(), w, h, basis } },
                ViewKind::Velocity => rsx! { VelocityPage { metrics: metrics.clone(), basis } },
                _ => rsx! { CriticalPathPage {} },
            }
        }
    }
}

/// The headline figures a report opens with.
#[component]
fn Figures(cells: Vec<(String, String)>) -> Element {
    rsx! {
        div { class: "rep-figures",
            for (label, value) in cells {
                div { key: "{label}", class: "rep-figure",
                    span { class: "rep-value", "{value}" }
                    span { class: "rep-label", "{label}" }
                }
            }
        }
    }
}

#[component]
fn BurndownPage(
    metrics: aop_core::agile::Metrics,
    w: f64,
    h: f64,
    basis: &'static str,
) -> Element {
    let peak = nice_peak(
        metrics
            .points
            .iter()
            .map(|p| p.remaining_minutes.max(p.ideal_remaining_minutes))
            .max()
            .unwrap_or(1),
    );
    let remaining: Vec<i64> = metrics.points.iter().map(|p| p.remaining_minutes).collect();
    let ideal: Vec<i64> = metrics.points.iter().map(|p| p.ideal_remaining_minutes).collect();
    let dates: Vec<chrono::NaiveDate> = metrics.points.iter().map(|p| p.date).collect();
    let plot = Plot::new(w, h);

    // Where the plan stands against where it planned to be.
    let drift = metrics
        .points
        .last()
        .map(|_| {
            let today = chrono::Local::now().naive_local().date();
            metrics
                .points
                .iter()
                .find(|p| p.date >= today)
                .map(|p| p.remaining_minutes - p.ideal_remaining_minutes)
                .unwrap_or(0)
        })
        .unwrap_or(0);

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Burndown" }
            p { class: "rep-sub",
                "{basis} still to do, against a plan running exactly to schedule. The ideal line follows the baseline when one has been set." }
        }
        Figures { cells: vec![
            ("Total".into(), aop_core::format_duration(metrics.total_minutes)),
            ("Remaining".into(), aop_core::format_duration(metrics.remaining_minutes())),
            ("Complete".into(), format!("{:.0}%", metrics.percent_complete())),
            ("Against plan".into(), if drift > 0 {
                format!("{} behind", aop_core::format_duration(drift))
            } else if drift < 0 {
                format!("{} ahead", aop_core::format_duration(-drift))
            } else { "on plan".into() }),
        ] }
        div { class: "rep-chart-box",
            svg { class: "report-chart", view_box: "0 0 {w} {h}", width: "100%", height: "{h}",
                Axes { plot_w: w, plot_h: h, peak, dates: dates.clone(),
                    unit: format!("{basis} remaining") }
                polyline { points: "{plot.path(&ideal, peak)}", fill: "none",
                    stroke: "var(--ink-faint)", stroke_width: "1.5", stroke_dasharray: "5 4" }
                polyline { points: "{plot.path(&remaining, peak)}", fill: "none",
                    stroke: "var(--accent-bright)", stroke_width: "2.5" }
            }
            div { class: "report-legend",
                span { span { class: "sw ideal" } "Ideal" }
                span { span { class: "sw actual" } "Remaining" }
            }
        }
        IterationTable { metrics }
    }
}

#[component]
fn BurnupPage(
    metrics: aop_core::agile::Metrics,
    w: f64,
    h: f64,
    basis: &'static str,
) -> Element {
    let peak = nice_peak(metrics.points.iter().map(|p| p.scope_minutes).max().unwrap_or(1));
    let done: Vec<i64> = metrics.points.iter().map(|p| p.completed_minutes).collect();
    let scope: Vec<i64> = metrics.points.iter().map(|p| p.scope_minutes).collect();
    let dates: Vec<chrono::NaiveDate> = metrics.points.iter().map(|p| p.date).collect();
    let plot = Plot::new(w, h);

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Burnup" }
            p { class: "rep-sub",
                "{basis} completed rising towards the total. Drawn beside a burndown because this one shows scope being added, which a burndown hides." }
        }
        Figures { cells: vec![
            ("Scope".into(), aop_core::format_duration(metrics.total_minutes)),
            ("Completed".into(), aop_core::format_duration(metrics.completed_minutes)),
            ("Remaining".into(), aop_core::format_duration(metrics.remaining_minutes())),
            ("Forecast finish".into(), metrics.projected_finish
                .map(|d| d.format("%d %b %Y").to_string())
                .unwrap_or_else(|| "not yet".into())),
        ] }
        div { class: "rep-chart-box",
            svg { class: "report-chart", view_box: "0 0 {w} {h}", width: "100%", height: "{h}",
                Axes { plot_w: w, plot_h: h, peak, dates: dates.clone(),
                    unit: format!("{basis} done") }
                polyline { points: "{plot.path(&scope, peak)}", fill: "none",
                    stroke: "var(--ink-faint)", stroke_width: "1.5" }
                polyline { points: "{plot.path(&done, peak)}", fill: "none",
                    stroke: "var(--bar-progress)", stroke_width: "2.5" }
            }
            div { class: "report-legend",
                span { span { class: "sw scope" } "Scope" }
                span { span { class: "sw done" } "Completed" }
            }
        }
        IterationTable { metrics }
    }
}

#[component]
fn VelocityPage(metrics: aop_core::agile::Metrics, basis: &'static str) -> Element {
    let fastest = metrics
        .iterations
        .iter()
        .map(|i| i.planned_minutes.max(i.completed_minutes))
        .max()
        .unwrap_or(1)
        .max(1);
    let today = chrono::Local::now().naive_local().date();
    let closed = metrics.iterations.iter().filter(|i| i.is_finished(today)).count();

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Velocity" }
            p { class: "rep-sub",
                "{basis} completed per iteration. A planning aid for forecasting the next one, not a measure of anybody's productivity. Only finished iterations count towards the average." }
        }
        Figures { cells: vec![
            ("Average".into(), aop_core::format_duration(metrics.average_velocity_minutes)),
            ("Iterations".into(), format!("{} of {}", closed, metrics.iterations.len())),
            ("Remaining".into(), aop_core::format_duration(metrics.remaining_minutes())),
            ("Forecast finish".into(), metrics.projected_finish
                .map(|d| d.format("%d %b %Y").to_string())
                .unwrap_or_else(|| "not yet".into())),
        ] }
        div { class: "rep-chart-box",
            div { class: "velocity tall",
                // The scale, so a bar can be read as a quantity rather than
                // only compared with the bar beside it.
                div { class: "vel-axis",
                    for step in 0..=4 {
                        {
                            let value = fastest * step / 4;
                            let from_bottom = step as f64 / 4.0 * 100.0;
                            let (_, per_unit) = axis_unit(fastest);
                            rsx! {
                                span { key: "vt{step}", class: "vel-tick",
                                    style: "bottom: {from_bottom}%;",
                                    "{axis_tick(value, per_unit)}" }
                            }
                        }
                    }
                }
                for step in 0..=4 {
                    {
                        let from_bottom = step as f64 / 4.0 * 100.0;
                        rsx! {
                            div { key: "vg{step}", class: "vel-grid",
                                style: "bottom: calc({from_bottom}% + 16px);" }
                        }
                    }
                }
                for iteration in metrics.iterations.iter() {
                    {
                        let planned = (iteration.planned_minutes as f64 / fastest as f64 * 100.0).min(100.0);
                        let done = (iteration.completed_minutes as f64 / fastest as f64 * 100.0).min(100.0);
                        rsx! {
                            div { key: "v{iteration.number}", class: "vel-col",
                                title: "{iteration.start} to {iteration.end}",
                                div { class: "vel-stack",
                                    div { class: "vel-planned", style: "height: {planned}%;" }
                                    div { class: "vel-done", style: "height: {done}%;" }
                                }
                                span { class: "vel-label", "{iteration.number}" }
                            }
                        }
                    }
                }
            }
            div { class: "report-legend",
                span { span { class: "sw planned" } "Planned" }
                span { span { class: "sw done" } "Completed" }
            }
        }
        IterationTable { metrics }
    }
}

/// The rows behind the chart. A report without them is only a picture.
#[component]
fn IterationTable(metrics: aop_core::agile::Metrics) -> Element {
    let today = chrono::Local::now().naive_local().date();
    rsx! {
        h2 { class: "rep-section", "By iteration" }
        table { class: "rep-table",
            thead {
                tr {
                    th { "#" } th { "From" } th { "To" }
                    th { class: "n", "Tasks" } th { class: "n", "Done" }
                    th { class: "n", "Planned" } th { class: "n", "Completed" }
                    th { "Status" }
                }
            }
            tbody {
                for iteration in metrics.iterations.iter() {
                    tr { key: "r{iteration.number}",
                        td { "{iteration.number}" }
                        td { "{iteration.start.format(\"%d %b %Y\")}" }
                        td { "{iteration.end.format(\"%d %b %Y\")}" }
                        td { class: "n", "{iteration.planned_tasks}" }
                        td { class: "n", "{iteration.completed_tasks}" }
                        td { class: "n", "{aop_core::format_duration(iteration.planned_minutes)}" }
                        td { class: "n", "{aop_core::format_duration(iteration.completed_minutes)}" }
                        td {
                            if iteration.is_finished(today) { "Closed" }
                            else if iteration.start <= today { "In progress" }
                            else { "Ahead" }
                        }
                    }
                }
            }
        }
    }
}

/// The critical path as a page: the chain, then every task on it.
#[component]
fn CriticalPathPage() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    let path = aop_core::critical_path(project);
    let minutes = aop_core::critical_path_minutes(project, &path);
    // The scheduler's own answer, not the warning list's. Dismissing a
    // critical warning changes the colour a bar is drawn in; it does not give
    // the task slack, so it must not change the count either.
    let zero_slack = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i) && project.tasks[i].scheduled.critical)
        .count();

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Critical path" }
            p { class: "rep-sub",
                "The longest chain of dependent tasks, which is what sets the finish date. Every task on it has zero slack, so any delay here moves the whole project. Tasks with slack can move without doing that." }
        }
        Figures { cells: vec![
            ("Path length".into(), aop_core::format_duration(minutes)),
            ("Tasks on path".into(), path.len().to_string()),
            ("Zero slack".into(), zero_slack.to_string()),
            ("Project finish".into(), crate::state::format_date(project.finish_date)),
        ] }

        if path.is_empty() {
            div { class: "empty-state", style: "height: 160px;",
                "Nothing is critical: every task has slack." }
        } else {
            h2 { class: "rep-section", "The chain" }
            // Names beside the chart, the way the plan itself is laid out. A
            // chart on its own says when things happen but not what they are,
            // and the whole point of the report is to be read.
            //
            // The two line up because the heading matches the timescale's
            // height and every name row matches a chart row, both taken from
            // the chart's own constants rather than guessed at.
            div { class: "cp-split",
                div { class: "cp-names",
                    div { class: "cp-names-head" }
                    for (number, step) in path.iter().enumerate() {
                        {
                            let name = project
                                .tasks
                                .get(step.index)
                                .map(|task| task.name.clone())
                                .unwrap_or_default();
                            let joint = step
                                .link_from_previous
                                .map(|kind| kind.code().to_string())
                                .unwrap_or_else(|| "start".into());
                            rsx! {
                                div { class: "cp-names-row", key: "cn{number}",
                                    span { class: "cp-names-num", "{number + 1}" }
                                    span { class: "cp-names-name", title: "{name}", "{name}" }
                                    span { class: "cp-names-joint", "{joint}" }
                                }
                            }
                        }
                    }
                }
                // The same renderer the plan is drawn with, handed just the
                // chain, so a report and the view it reports on cannot drift.
                GanttChart {
                    rows: Some(path.iter().map(|step| step.index).collect::<Vec<_>>()),
                    interactive: false,
                }
            }
            div { class: "cp-legend",
                // The chart draws the chain in the plan's own critical bar
                // colour, so the swatch takes it too rather than naming a
                // colour the reader may not be looking at.
                span { class: "cp-swatch", style: "background: {project.bar_styles.critical};" }
                "These bars are the critical path. Every task here has zero slack, so delaying any one of them moves the project finish."
            }

            h2 { class: "rep-section", "Task by task" }
            table { class: "rep-table",
                thead {
                    tr {
                        th { "#" } th { "Task" } th { "Joined by" }
                        th { "Start" } th { "Finish" } th { class: "n", "Duration" }
                    }
                }
                tbody {
                    for (number, step) in path.iter().enumerate() {
                        {
                            let task = &project.tasks[step.index];
                            let joint = step.link_from_previous
                                .map(|kind| kind.label().to_string())
                                .unwrap_or_else(|| "starts the path".into());
                            rsx! {
                                tr { key: "c{step.index}",
                                    td { "{number + 1}" }
                                    td { "{task.name}" }
                                    td { class: "muted", "{joint}" }
                                    td { "{crate::state::format_date(task.scheduled.start)}" }
                                    td { "{crate::state::format_date(task.scheduled.finish)}" }
                                    td { class: "n", "{aop_core::format_duration(task.scheduled.duration_minutes)}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// --------------------------------------------------------------- spelling

/// The spelling panel: what the dictionary does not recognise, and what to do
/// about each one.
///
/// It floats beside the plan rather than replacing it. Correcting a word means
/// seeing the row it sits in, and a full screen list of mistakes takes away the
/// very thing being corrected.
///
/// The dictionary is built from the machine's own word lists, so this also has
/// to cope with there being none, which on a lean install is the normal case
/// rather than an error.
#[component]
pub fn SpellingPanel() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // Loaded once: assembling it means reading a hundred thousand words.
    let mut dictionary = use_signal(crate::dictionary::load);
    let sources = use_signal(crate::dictionary::describe_sources);

    let (found, known) = {
        let s = state.read();
        let dict = dictionary.read();
        (
            aop_core::spelling::check(&s.project, &dict, &s.ignored_words),
            dict.len(),
        )
    };

    rsx! {
        aside { class: "spell-panel",
            div { class: "spell-panel-head",
                span { class: "spell-panel-title", "Spelling" }
                button {
                    class: "dlg-close",
                    title: "Close",
                    onclick: move |_| state.write().spelling_open = false,
                    "\u{2715}"
                }
            }
            div { class: "spell-panel-body",
            if known == 0 {
                div { class: "report-card",
                    div { class: "report-head",
                        span { class: "report-title", "No dictionary yet" }
                    }
                    div { class: "hint", style: "margin: 10px 0 0; line-height: 1.6; max-width: 720px;",
                        "No word list is shipped with the application: the usual ones are licensed in ways that would spread to this product. It reads whatever the machine already has, and this one has none. Fetch one below, or install one with your package manager."
                    }
                }
            }

            DictionaryShelf { on_change: move |_| dictionary.set(crate::dictionary::rebuild()) }

            div { class: "report-card",
                div { class: "report-head",
                    span { class: "report-title", "Spelling" }
                    span { class: "report-note",
                        "{found.len()} to review \u{00b7} {known} words known" }
                }

                if found.is_empty() {
                    div { class: "empty-state", style: "height: 140px; font-size: 12px;",
                        "Nothing to correct." }
                } else {
                    div { class: "spell-list",
                        for (slot, item) in found.iter().enumerate() {
                            {
                                let word = item.word.clone();
                                let place = item.place;
                                let ignore_word = word.clone();
                                rsx! {
                                    div { key: "sp{slot}", class: "spell-row",
                                        div { class: "spell-main",
                                            span { class: "spell-word", "{item.word}" }
                                            span { class: "spell-where", "{item.place.label()}" }
                                            span { class: "spell-context", "{item.context}" }
                                        }
                                        div { class: "spell-acts",
                                            for suggestion in item.suggestions.iter().take(3).cloned() {
                                                {
                                                    let from = word.clone();
                                                    let to = suggestion.clone();
                                                    rsx! {
                                                        button {
                                                            key: "{suggestion}",
                                                            class: "spell-fix",
                                                            onclick: move |_| {
                                                                state.write().correct_spelling(place, &from, &to);
                                                            },
                                                            "{suggestion}"
                                                        }
                                                    }
                                                }
                                            }
                                            if item.suggestions.is_empty() {
                                                span { class: "spell-none", "no suggestion" }
                                            }
                                            button {
                                                class: "spell-skip",
                                                onclick: move |_| state.write().ignore_word(&ignore_word),
                                                "Ignore"
                                            }
                                            {
                                                let learn = word.clone();
                                                rsx! {
                                                    button {
                                                        class: "spell-skip",
                                                        title: "Remember this word on this machine",
                                                        onclick: move |_| {
                                                            // Written to the word list for next time,
                                                            // and put into the one already loaded, or
                                                            // the word stays flagged until a rescan.
                                                            crate::dictionary::remember(&learn);
                                                            dictionary.write().add(&learn);
                                                        },
                                                        "Add to dictionary"
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

                div { class: "spell-foot",
                    if sources().is_empty() {
                        span { "No word list found on this machine." }
                    } else {
                        span { "Built from {sources().len()} word list(s) on this machine." }
                    }
                    button {
                        class: "spell-skip",
                        title: "Read the machine's word lists again",
                        onclick: move |_| dictionary.set(crate::dictionary::rebuild()),
                        "Rescan"
                    }
                }
            }
            }
        }
    }
}

/// The dictionaries that can be fetched, and the ones already here.
///
/// Downloading is always the user's choice. Nothing is fetched at start up and
/// nothing is bundled, which is what keeps a word list's licence off this
/// product while still letting a machine with none become useful.
#[component]
fn DictionaryShelf(on_change: EventHandler<()>) -> Element {
    let mut busy = use_signal(|| None::<String>);
    let mut outcome = use_signal(|| None::<(bool, String)>);
    // Bumped after a change so the installed marks are re-read.
    let mut revision = use_signal(|| 0u32);

    rsx! {
        div { class: "report-card", style: "margin-bottom: 14px;",
            div { class: "report-head",
                span { class: "report-title", "Dictionaries" }
                span { class: "report-note", "downloaded on request, never bundled" }
            }

            if let Some((ok, message)) = outcome() {
                div { class: if ok { "dict-note ok" } else { "dict-note bad" }, "{message}" }
            }

            div { class: "dict-list",
                for entry in crate::dictionary::CATALOGUE {
                    {
                        let _ = revision();
                        let installed = entry.is_installed();
                        let working = busy() == Some(entry.code.to_string());
                        let size = entry.bytes as f64 / 1_048_576.0;
                        rsx! {
                            div { key: "{entry.code}", class: "dict-row",
                                span { class: "dict-name", "{entry.name}" }
                                span { class: "dict-code", "{entry.code}" }
                                span { class: "dict-size", "{size:.1} MB" }
                                if working {
                                    span { class: "dict-state", "Downloading\u{2026}" }
                                } else if installed {
                                    button {
                                        class: "spell-skip",
                                        onclick: move |_| {
                                            crate::dictionary::remove(&entry);
                                            outcome.set(Some((true, format!("Removed {}.", entry.name))));
                                            revision += 1;
                                            on_change.call(());
                                        },
                                        "Remove"
                                    }
                                } else {
                                    button {
                                        class: "spell-fix",
                                        onclick: move |_| {
                                            busy.set(Some(entry.code.to_string()));
                                            // Blocking, but a dictionary is a
                                            // second or two and pretending
                                            // otherwise would need a spinner
                                            // that lies about progress.
                                            let result = crate::dictionary::download(&entry);
                                            busy.set(None);
                                            match result {
                                                Ok(bytes) => {
                                                    outcome.set(Some((
                                                        true,
                                                        format!(
                                                            "{} installed, {:.1} MB, checksum verified.",
                                                            entry.name,
                                                            bytes as f64 / 1_048_576.0
                                                        ),
                                                    )));
                                                    revision += 1;
                                                    on_change.call(());
                                                }
                                                Err(complaint) => outcome.set(Some((false, complaint))),
                                            }
                                        },
                                        "Download"
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
