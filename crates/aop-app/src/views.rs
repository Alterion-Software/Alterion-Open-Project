//! The views other than the Gantt chart.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;

use aop_core::agile::{Basis, Iteration, Metrics};
use aop_core::{format_duration, format_work, Resource, ResourceKind, TaskId};

use crate::gantt::{chart_range, Scale};
use crate::state::{format_date, AppState, Dialog, ViewKind};

// -------------------------------------------------------- resource sheet

/// What one booking costs, charged the way the scheduler charges it.
///
/// Hours times rate is only right for a work resource. A material is bought by
/// the unit and a cost resource is a lump sum with no per-use charge on top, so
/// running every kind through the same sum puts a number in the Cost column
/// that the plan's own totals disagree with.
fn booking_cost(resource: &Resource, duration_minutes: i64, units: f64) -> f64 {
    match resource.kind {
        ResourceKind::Work => {
            let minutes = (duration_minutes as f64 * units).round() as i64;
            minutes as f64 / 60.0 * resource.standard_rate + resource.cost_per_use
        }
        ResourceKind::Material => units * resource.standard_rate + resource.cost_per_use,
        ResourceKind::Cost => units * resource.standard_rate,
    }
}

/// Work a booking books, which only a work resource does at all.
fn booking_work(resource: &Resource, duration_minutes: i64, units: f64) -> i64 {
    match resource.kind {
        ResourceKind::Work => (duration_minutes as f64 * units).round() as i64,
        _ => 0,
    }
}

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

    // Booked work per resource, summed across every leaf task, and what those
    // bookings cost at the rates the scheduler itself charges.
    let mut booked: HashMap<u32, i64> = HashMap::new();
    let mut costs: HashMap<u32, f64> = HashMap::new();
    for index in 0..project.tasks.len() {
        if project.is_summary(index) {
            continue;
        }
        let task = &project.tasks[index];
        for assignment in &task.assignments {
            let Some(resource) = project.resource(assignment.resource) else {
                continue;
            };
            *booked.entry(assignment.resource).or_insert(0) +=
                booking_work(resource, task.duration_minutes, assignment.units);
            *costs.entry(assignment.resource).or_insert(0.0) +=
                booking_cost(resource, task.duration_minutes, assignment.units);
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
                            let cost = costs.get(&resource.id).copied().unwrap_or(0.0);
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
                                        let resource = project.resource(assignment.resource);
                                        let name = resource.map(|r| r.name.clone()).unwrap_or_default();
                                        // A material or a cost resource books no work,
                                        // so hours beside it would be a number the
                                        // scheduler never counted.
                                        let work = match resource {
                                            Some(r) if r.kind == ResourceKind::Work => {
                                                format_work(booking_work(r, task.duration_minutes, assignment.units))
                                            }
                                            _ => String::new(),
                                        };
                                        rsx! {
                                            tr { key: "tu{index}a{assignment.resource}",
                                                style: "color: var(--ink-soft);",
                                                td { "" }
                                                td { style: "padding-left: {indent + 28.0}px; font-style: italic;", "{name}" }
                                                td { "{work}" }
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

// ------------------------------------------------------- resource usage

/// Resource Usage: every resource with the tasks booked against it.
///
/// The mirror of Task Usage, which lists the resources under each task. It is
/// its own view rather than a relabelled Resource Sheet, because a sheet says
/// what a resource is and this says what it is doing and when. Tasks nobody is
/// booked on are gathered at the end: an empty plan for a resource is easy to
/// see, but work with nobody on it is what actually goes unnoticed.
#[component]
pub fn ResourceUsage() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;
    let currency = project.currency_symbol.clone();

    let mut bookings: HashMap<u32, Vec<(usize, f64)>> = HashMap::new();
    let mut unassigned: Vec<usize> = Vec::new();
    for index in 0..project.tasks.len() {
        if project.is_summary(index) {
            continue;
        }
        let task = &project.tasks[index];
        if task.assignments.is_empty() {
            unassigned.push(index);
            continue;
        }
        for assignment in &task.assignments {
            bookings
                .entry(assignment.resource)
                .or_default()
                .push((index, assignment.units));
        }
    }

    rsx! {
        div { class: "sheet-pane",
            table { class: "sheet",
                colgroup {
                    col { style: "width: 40px;" }
                    col { style: "width: 300px;" }
                    col { style: "width: 90px;" }
                    col { style: "width: 110px;" }
                    col { style: "width: 80px;" }
                    col { style: "width: 110px;" }
                    col { style: "width: 130px;" }
                    col { style: "width: 130px;" }
                }
                thead {
                    tr {
                        th { "ID" }
                        th { "Resource Name" }
                        th { "Type" }
                        th { "Work" }
                        th { "Units" }
                        th { "Cost" }
                        th { "Start" }
                        th { "Finish" }
                    }
                }
                tbody {
                    for (index, resource) in project.resources.iter().enumerate() {
                        {
                            let rows = bookings.get(&resource.id).cloned().unwrap_or_default();
                            let work: i64 = rows
                                .iter()
                                .map(|(task, units)| {
                                    booking_work(resource, project.tasks[*task].duration_minutes, *units)
                                })
                                .sum();
                            let cost: f64 = rows
                                .iter()
                                .map(|(task, units)| {
                                    booking_cost(resource, project.tasks[*task].duration_minutes, *units)
                                })
                                .sum();
                            let span = rows
                                .iter()
                                .map(|(task, _)| project.tasks[*task].scheduled)
                                .fold(None, |acc: Option<(chrono::NaiveDateTime, chrono::NaiveDateTime)>, s| {
                                    Some(match acc {
                                        Some((start, finish)) => {
                                            (start.min(s.start), finish.max(s.finish))
                                        }
                                        None => (s.start, s.finish),
                                    })
                                });

                            rsx! {
                                tr { key: "ru{index}", class: "usage-head",
                                    td { "{index + 1}" }
                                    td { style: "font-weight: 700;", "{resource.name}" }
                                    td { "{resource.kind.label()}" }
                                    td {
                                        if resource.kind == ResourceKind::Work {
                                            "{format_work(work)}"
                                        }
                                    }
                                    // Units belong to the bookings, not to the
                                    // resource: what it is capable of is the
                                    // sheet's business, not this view's.
                                    td { "" }
                                    td { "{currency}{cost:.2}" }
                                    td { if let Some((start, _)) = span { "{format_date(start)}" } }
                                    td { if let Some((_, finish)) = span { "{format_date(finish)}" } }
                                }

                                if rows.is_empty() {
                                    tr { key: "ru{index}none", style: "color: var(--ink-soft);",
                                        td { "" }
                                        td { style: "padding-left: 28px; font-style: italic;",
                                            "Not booked on anything" }
                                        td {} td {} td {} td {} td {} td {}
                                    }
                                }

                                for (task, units) in rows.iter() {
                                    {
                                        let row = &project.tasks[*task];
                                        let work = booking_work(resource, row.duration_minutes, *units);
                                        let cost = booking_cost(resource, row.duration_minutes, *units);
                                        rsx! {
                                            tr { key: "ru{index}t{task}", style: "color: var(--ink-soft);",
                                                td { "{task + 1}" }
                                                td { style: "padding-left: 28px;", "{row.name}" }
                                                td { "" }
                                                td {
                                                    if resource.kind == ResourceKind::Work {
                                                        "{format_work(work)}"
                                                    }
                                                }
                                                td { "{units * 100.0:.0}%" }
                                                td { "{currency}{cost:.2}" }
                                                td { "{format_date(row.scheduled.start)}" }
                                                td { "{format_date(row.scheduled.finish)}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Work with nobody on it. Project shows this the same way,
                    // and it is the half of the report that gets acted on.
                    if !unassigned.is_empty() {
                        tr { class: "usage-head",
                            td { "" }
                            td { style: "font-weight: 700;", "Unassigned" }
                            td {} td {} td {} td {} td {} td {}
                        }
                        for task in unassigned.iter() {
                            {
                                let row = &project.tasks[*task];
                                rsx! {
                                    tr { key: "run{task}", style: "color: var(--ink-soft);",
                                        td { "{task + 1}" }
                                        td { style: "padding-left: 28px;", "{row.name}" }
                                        td { "" }
                                        td { "" }
                                        td { "" }
                                        td { "{currency}{row.fixed_cost:.2}" }
                                        td { "{format_date(row.scheduled.start)}" }
                                        td { "{format_date(row.scheduled.finish)}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if project.resources.is_empty() {
                div { class: "hint", style: "padding: 14px;",
                    "Nobody is booked on this plan yet. Add resources on the Resource Sheet, then book them onto tasks from the Resource Names column."
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

    fn resource(kind: ResourceKind, rate: f64, per_use: f64) -> Resource {
        let mut resource = Resource::new(1, "Someone");
        resource.kind = kind;
        resource.standard_rate = rate;
        resource.cost_per_use = per_use;
        resource
    }

    #[test]
    fn a_work_booking_is_charged_by_the_hour_plus_its_per_use_fee() {
        // Two days at full time, charged at the rate, with the call-out fee
        // the scheduler also charges.
        let cost = booking_cost(&resource(ResourceKind::Work, 50.0, 25.0), 960, 1.0);
        assert!((cost - (16.0 * 50.0 + 25.0)).abs() < 0.01);
    }

    #[test]
    fn a_material_is_charged_by_the_unit_rather_than_by_the_hour() {
        // Hours times rate against a material invents a cost out of how long
        // the task runs, which the plan's own totals never counted.
        let cost = booking_cost(&resource(ResourceKind::Material, 12.0, 30.0), 4800, 5.0);
        assert!((cost - (5.0 * 12.0 + 30.0)).abs() < 0.01);
    }

    #[test]
    fn a_cost_resource_is_a_lump_sum_with_nothing_added_on_top() {
        let cost = booking_cost(&resource(ResourceKind::Cost, 400.0, 99.0), 4800, 2.0);
        assert!((cost - 800.0).abs() < 0.01, "the per-use fee is not charged again");
    }

    #[test]
    fn only_a_work_resource_books_work() {
        assert_eq!(booking_work(&resource(ResourceKind::Work, 0.0, 0.0), 960, 0.5), 480);
        assert_eq!(booking_work(&resource(ResourceKind::Material, 0.0, 0.0), 960, 5.0), 0);
        assert_eq!(booking_work(&resource(ResourceKind::Cost, 0.0, 0.0), 960, 1.0), 0);
    }

    #[test]
    fn a_series_that_stops_early_is_drawn_on_the_full_axis() {
        // The actual line ends at the status date. Drawn over its own length
        // it would stretch across the whole chart and read as complete.
        let plot = Plot::new(200.0, 100.0);
        let short = plot.partial_path(&[10, 5], 5, 10);
        let full = plot.path(&[10, 5], 10);
        assert_ne!(short, full, "half a series must not fill the width");
        assert!(short.ends_with(&format!("{:.1},{:.1}", plot.left + plot.inner_w() / 4.0, plot.top + plot.inner_h() / 2.0)));
    }

    #[test]
    fn an_axis_names_its_unit_once_and_a_count_names_it_never_twice() {
        assert_eq!(measure_title(Basis::Work, "remaining", 4800), "work remaining (days)");
        assert_eq!(measure_title(Basis::Count, "remaining", 12), "tasks remaining");
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
        self.partial_path(values, values.len(), peak)
    }

    /// A series that stops short of the right edge, on the same scale as the
    /// ones that do not. The actual line ends at the status date, and drawing
    /// it over its own length would stretch it across the whole chart.
    fn partial_path(&self, values: &[i64], count: usize, peak: i64) -> String {
        if values.len() < 2 {
            return String::new();
        }
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let (x, y) = self.point(index, count, *value, peak);
                format!("{x:.1},{y:.1}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A tick label in the axis's own unit, without repeating the unit each time.
///
/// Formatting each tick on its own gives a scale reading "200000 mins", then
/// "2500 hrs", then "0 days", which cannot be compared by eye and is worse
/// than no labels at all. The unit is named once, in the axis title.
fn axis_tick(value: i64, per_unit: f64) -> String {
    let scaled = value as f64 / per_unit;
    if scaled.fract().abs() < 0.05 {
        format!("{scaled:.0}")
    } else {
        format!("{scaled:.1}")
    }
}

/// What the numbers up an axis are counting, said once and in the unit they
/// are scaled to. A count of tasks is already its own unit, so naming it twice
/// only adds noise.
fn measure_title(basis: Basis, what: &str, peak: i64) -> String {
    match basis {
        Basis::Count => format!("tasks {what}"),
        Basis::Work => format!("work {what} ({})", basis.axis_unit(peak).0),
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
    dates: Vec<NaiveDate>,
    basis: Basis,
    what: String,
) -> Element {
    let plot = Plot::new(plot_w, plot_h);
    let steps = 4;
    let (_, per_unit) = basis.axis_unit(peak);

    // Six dates at most: more overlap and stop being readable.
    let stride = (dates.len() / 5).max(1);
    let ticks: Vec<(usize, NaiveDate)> = dates
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
                "{measure_title(basis, &what, peak)}"
            }
        }
    }
}

/// The line marking the day the report is written against, so that the actual
/// series stopping there reads as "nothing has happened yet" rather than as a
/// chart that gave up.
#[component]
fn StatusRule(plot_w: f64, plot_h: f64, index: usize, count: usize, peak: i64) -> Element {
    let plot = Plot::new(plot_w, plot_h);
    let (x, _) = plot.point(index, count, 0, peak);
    rsx! {
        g { class: "status-rule",
            line {
                x1: "{x:.1}", y1: "{plot.top}", x2: "{x:.1}", y2: "{plot.top + plot.inner_h()}",
                stroke: "var(--contextual)", stroke_width: "1", stroke_dasharray: "3 3",
            }
            text {
                x: "{x - 4.0:.1}", y: "{plot.top + 9.0}",
                text_anchor: "end", class: "axis-label",
                "status date"
            }
        }
    }
}

/// Said when there is nothing to draw, rather than drawing empty axes and
/// leaving the reader to wonder what went wrong.
#[component]
fn ChartNote(text: String) -> Element {
    rsx! { p { class: "rep-chart-note", "{text}" } }
}

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
                ViewKind::Burndown => rsx! { BurndownPage { metrics: metrics.clone(), w, h } },
                ViewKind::Burnup => rsx! { BurnupPage { metrics: metrics.clone(), w, h } },
                ViewKind::Velocity => rsx! { VelocityPage { metrics: metrics.clone() } },
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

/// How far off the plan the page is, in words, from the one drift figure.
fn against_plan(metrics: &Metrics) -> String {
    use std::cmp::Ordering;
    let drift = metrics.against_plan();
    match drift.cmp(&0) {
        Ordering::Equal => "on plan".into(),
        Ordering::Greater => format!("{} behind", metrics.basis.format(drift)),
        Ordering::Less => format!("{} ahead", metrics.basis.format(-drift)),
    }
}

#[component]
fn BurndownPage(metrics: Metrics, w: f64, h: f64) -> Element {
    let basis = metrics.basis;
    let peak = nice_peak(
        metrics
            .points
            .iter()
            .map(|p| p.remaining.unwrap_or(0).max(p.ideal_remaining))
            .max()
            .unwrap_or(1),
    );
    // The ideal runs the length of the plan; the actual stops at the status
    // date, because past it nothing has had the chance to happen.
    let ideal: Vec<i64> = metrics.points.iter().map(|p| p.ideal_remaining).collect();
    let actual: Vec<i64> = metrics.points.iter().filter_map(|p| p.remaining).collect();
    let dates: Vec<NaiveDate> = metrics.points.iter().map(|p| p.date).collect();
    let plot = Plot::new(w, h);
    let status_index = metrics
        .points
        .iter()
        .position(|p| p.date == metrics.status_date);

    let comparison = if metrics.ideal_from_baseline {
        "The ideal line is the baseline's own remaining curve, stepping down on the dates the baseline said each task would finish."
    } else {
        "No baseline has been taken, so the ideal line is a straight run from the total to zero. Set a baseline to compare against what was planned."
    };

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Burndown" }
            p { class: "rep-sub",
                "{basis.label()} still to do, day by day. The actual line stops at the status date: nothing after it has happened yet. {comparison}" }
        }
        Figures { cells: vec![
            ("Total".into(), basis.format(metrics.total)),
            ("Remaining".into(), basis.format(metrics.remaining())),
            ("Complete".into(), format!("{:.0}%", metrics.percent_complete())),
            ("Against plan".into(), against_plan(&metrics)),
        ] }
        div { class: "rep-chart-box",
            svg { class: "report-chart", view_box: "0 0 {w} {h}", width: "100%", height: "{h}",
                Axes { plot_w: w, plot_h: h, peak, dates: dates.clone(), basis,
                    what: "remaining".to_string() }
                if let Some(index) = status_index {
                    StatusRule { plot_w: w, plot_h: h, index, count: dates.len(), peak }
                }
                polyline { points: "{plot.path(&ideal, peak)}", fill: "none",
                    stroke: "var(--ink-faint)", stroke_width: "1.5", stroke_dasharray: "5 4" }
                polyline { points: "{plot.partial_path(&actual, dates.len(), peak)}", fill: "none",
                    stroke: "var(--accent-bright)", stroke_width: "2.5" }
            }
            if dates.len() < 2 {
                ChartNote { text: "The plan is one day long, so there is no line to draw across it yet. The figures above are the whole story.".to_string() }
            } else if actual.len() < 2 {
                ChartNote { text: "The status date is on or before the plan's first day, so there is no progress to draw yet.".to_string() }
            }
            div { class: "report-legend",
                span { span { class: "sw ideal" } "Ideal" }
                span { span { class: "sw actual" } "Remaining" }
            }
        }
        BurndownTable { metrics }
    }
}

#[component]
fn BurnupPage(metrics: Metrics, w: f64, h: f64) -> Element {
    let basis = metrics.basis;
    let peak = nice_peak(metrics.points.iter().map(|p| p.scope).max().unwrap_or(1));
    let done: Vec<i64> = metrics.points.iter().filter_map(|p| p.completed).collect();
    let scope: Vec<i64> = metrics.points.iter().map(|p| p.scope).collect();
    let dates: Vec<NaiveDate> = metrics.points.iter().map(|p| p.date).collect();
    let plot = Plot::new(w, h);
    let status_index = metrics
        .points
        .iter()
        .position(|p| p.date == metrics.status_date);

    // The one scope movement that can be shown without a history of the plan.
    let fourth = match metrics.scope_change() {
        Some(0) => ("Since baseline".into(), "unchanged".to_string()),
        Some(change) if change > 0 => ("Since baseline".into(), format!("{} added", basis.format(change))),
        Some(change) => ("Since baseline".into(), format!("{} dropped", basis.format(-change))),
        None => (
            "Forecast finish".into(),
            metrics
                .projected_finish
                .map(|d| d.format("%d %b %Y").to_string())
                .unwrap_or_else(|| "not yet".into()),
        ),
    };

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Burnup" }
            p { class: "rep-sub",
                "{basis.label()} completed, rising towards the plan's total. The scope line is that total as it stands today: no record is kept of when scope was added, so it cannot rise part way along. What the plan has gained or lost since the baseline is the figure beside it." }
        }
        Figures { cells: vec![
            ("Scope".into(), basis.format(metrics.total)),
            ("Completed".into(), basis.format(metrics.completed)),
            ("Remaining".into(), basis.format(metrics.remaining())),
            fourth,
        ] }
        div { class: "rep-chart-box",
            svg { class: "report-chart", view_box: "0 0 {w} {h}", width: "100%", height: "{h}",
                Axes { plot_w: w, plot_h: h, peak, dates: dates.clone(), basis,
                    what: "done".to_string() }
                if let Some(index) = status_index {
                    StatusRule { plot_w: w, plot_h: h, index, count: dates.len(), peak }
                }
                polyline { points: "{plot.path(&scope, peak)}", fill: "none",
                    stroke: "var(--ink-faint)", stroke_width: "1.5" }
                polyline { points: "{plot.partial_path(&done, dates.len(), peak)}", fill: "none",
                    stroke: "var(--bar-progress)", stroke_width: "2.5" }
            }
            if dates.len() < 2 {
                ChartNote { text: "The plan is one day long, so there is no line to draw across it yet. The figures above are the whole story.".to_string() }
            } else if done.len() < 2 {
                ChartNote { text: "The status date is on or before the plan's first day, so there is nothing completed to draw yet.".to_string() }
            }
            div { class: "report-legend",
                span { span { class: "sw scope" } "Scope" }
                span { span { class: "sw done" } "Completed" }
            }
        }
        BurnupTable { metrics }
    }
}

#[component]
fn VelocityPage(metrics: Metrics) -> Element {
    let basis = metrics.basis;
    let today = metrics.status_date;
    // A velocity chart shows the iterations that have happened. One that has
    // not started yet has no velocity to report, only a plan.
    let shown: Vec<Iteration> = metrics
        .iterations
        .iter()
        .filter(|iteration| iteration.start <= today)
        .cloned()
        .collect();
    let fastest = shown
        .iter()
        .map(|i| i.planned.max(i.completed))
        .max()
        .unwrap_or(1)
        .max(metrics.average_velocity)
        .max(1);
    let average_height = (metrics.average_velocity as f64 / fastest as f64 * 100.0).min(100.0);

    let cadence = if metrics.iterations_declared {
        "Iterations are the sprints the plan declares."
    } else {
        "The plan declares no sprints, so iterations are fixed windows from its start."
    };

    rsx! {
        div { class: "rep-head",
            h1 { class: "rep-title", "Velocity" }
            p { class: "rep-sub",
                "{basis.label()} delivered per iteration. Only tasks reported finished count, because a task half done was not delivered, and each is credited to the iteration its actual finish falls in, or its scheduled finish where the plan records no actual. A planning aid for forecasting the next iteration, not a measure of anybody's productivity. {cadence}" }
        }
        Figures { cells: vec![
            ("Average".into(), basis.format(metrics.average_velocity)),
            ("Iterations".into(), format!("{} of {} closed", metrics.closed_iterations(), metrics.iterations.len())),
            ("Not yet finished".into(), basis.format(metrics.incomplete)),
            ("Forecast finish".into(), metrics.projected_finish
                .map(|d| d.format("%d %b %Y").to_string())
                .unwrap_or_else(|| "not yet".into())),
        ] }
        div { class: "rep-chart-box",
            div { class: "velocity tall",
                // What the bars are counting, so a bar can be read as a
                // quantity rather than only against the bar beside it.
                div { class: "vel-title", "{measure_title(basis, \"delivered\", fastest)}" }
                div { class: "vel-axis",
                    for step in 0..=4 {
                        {
                            let value = fastest * step / 4;
                            let from_bottom = step as f64 / 4.0 * 100.0;
                            let (_, per_unit) = basis.axis_unit(fastest);
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
                for iteration in shown.iter() {
                    {
                        let planned = (iteration.planned as f64 / fastest as f64 * 100.0).min(100.0);
                        let done = (iteration.completed as f64 / fastest as f64 * 100.0).min(100.0);
                        let running = if iteration.is_running(today) { " running" } else { "" };
                        rsx! {
                            div { key: "v{iteration.number}", class: "vel-col",
                                title: "{iteration.name}: {iteration.start} to {iteration.end}",
                                div { class: "vel-stack",
                                    div { class: "vel-planned", style: "height: {planned}%;" }
                                    div { class: "vel-done{running}", style: "height: {done}%;" }
                                }
                                span { class: "vel-label", "{iteration.number}" }
                            }
                        }
                    }
                }
                // The average is the one line a velocity chart is read against.
                if metrics.average_velocity > 0 {
                    div { class: "vel-average", style: "bottom: calc({average_height}% + 16px);" }
                }
            }
            if shown.is_empty() {
                ChartNote { text: "No iteration has started yet, so there is no velocity to show.".to_string() }
            }
            div { class: "report-legend",
                span { span { class: "sw planned" } "Planned" }
                span { span { class: "sw done" } "Delivered" }
                span { span { class: "sw average" } "Average" }
            }
        }
        IterationTable { metrics }
    }
}

/// The rows behind the burndown. A burndown is a daily chart, so its table is
/// the daily burn rather than a summary by iteration.
#[component]
fn BurndownTable(metrics: Metrics) -> Element {
    let basis = metrics.basis;
    // Only the days the actual line covers: the ideal runs on past the status
    // date, but a row of it beside an empty Remaining says nothing.
    let rows: Vec<(NaiveDate, i64, i64)> = metrics
        .points
        .iter()
        .filter_map(|p| p.remaining.map(|remaining| (p.date, remaining, p.ideal_remaining)))
        .collect();

    rsx! {
        h2 { class: "rep-section", "Day by day, to the status date" }
        table { class: "rep-table",
            thead {
                tr {
                    th { "Date" }
                    th { class: "n", "Remaining" }
                    th { class: "n", "Ideal" }
                    th { class: "n", "Variance" }
                }
            }
            tbody {
                for (date, remaining, ideal) in rows.iter() {
                    {
                        let variance = remaining - ideal;
                        rsx! {
                            tr { key: "d{date}",
                                td { "{date.format(\"%d %b %Y\")}" }
                                td { class: "n", "{basis.format(*remaining)}" }
                                td { class: "n", "{basis.format(*ideal)}" }
                                td { class: "n",
                                    if variance == 0 { "on plan" }
                                    else if variance > 0 { "{basis.format(variance)} behind" }
                                    else { "{basis.format(-variance)} ahead" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The rows behind the burnup, on the same daily footing.
#[component]
fn BurnupTable(metrics: Metrics) -> Element {
    let basis = metrics.basis;
    let rows: Vec<(NaiveDate, i64, i64, i64)> = metrics
        .points
        .iter()
        .filter_map(|p| Some((p.date, p.completed?, p.remaining?, p.scope)))
        .collect();

    rsx! {
        h2 { class: "rep-section", "Day by day, to the status date" }
        table { class: "rep-table",
            thead {
                tr {
                    th { "Date" }
                    th { class: "n", "Completed" }
                    th { class: "n", "Remaining" }
                    th { class: "n", "Scope" }
                }
            }
            tbody {
                for (date, done, remaining, scope) in rows.iter() {
                    tr { key: "u{date}",
                        td { "{date.format(\"%d %b %Y\")}" }
                        td { class: "n", "{basis.format(*done)}" }
                        td { class: "n", "{basis.format(*remaining)}" }
                        td { class: "n", "{basis.format(*scope)}" }
                    }
                }
            }
        }
    }
}

/// The rows behind the velocity chart. A report without them is only a
/// picture.
#[component]
fn IterationTable(metrics: Metrics) -> Element {
    let basis = metrics.basis;
    let today = metrics.status_date;
    rsx! {
        h2 { class: "rep-section", "By iteration" }
        table { class: "rep-table",
            thead {
                tr {
                    th { "#" } th { "Iteration" } th { "From" } th { "To" }
                    th { class: "n", "Tasks" } th { class: "n", "Finished" }
                    th { class: "n", "Planned" } th { class: "n", "Delivered" }
                    th { "Status" }
                }
            }
            tbody {
                for iteration in metrics.iterations.iter() {
                    tr { key: "r{iteration.number}",
                        td { "{iteration.number}" }
                        td { "{iteration.name}" }
                        td { "{iteration.start.format(\"%d %b %Y\")}" }
                        td { "{iteration.end.format(\"%d %b %Y\")}" }
                        td { class: "n", "{iteration.planned_tasks}" }
                        td { class: "n", "{iteration.completed_tasks}" }
                        td { class: "n", "{basis.format(iteration.planned)}" }
                        td { class: "n", "{basis.format(iteration.completed)}" }
                        td {
                            if iteration.is_finished(today) { "Closed" }
                            else if iteration.is_running(today) { "In progress" }
                            else { "Not started" }
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
    let mut hover = use_context::<crate::state::Hovered>().0;
    let hovered = hover();

    let path = aop_core::critical_path(project);
    // The elapsed working span, not the sum of the task durations. A chain
    // carrying a ten day lag occupies twelve days and did two days of work, and
    // the figure is labelled as the span, so it has to be the span.
    let minutes = aop_core::critical_path_span_minutes(project, &path);
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
            div { class: "cp-legend",
                // The chart beside this draws the chain in the plan's own
                // critical bar colour, so the swatch takes it from there
                // rather than naming a colour the reader is not looking at.
                span { class: "cp-swatch", style: "background: {project.bar_styles.critical};" }
                "The chart beside this is the chain, in order. Every task on it has zero slack, so delaying any one of them moves the project finish."
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
                            let row = step.index;
                            let hot = if hovered == Some(row) { "hot" } else { "" };
                            rsx! {
                                // Pointing at a row lights up its bar in the
                                // chart beside this, and the other way round.
                                // The two panes are one answer read two ways.
                                tr {
                                    key: "c{step.index}",
                                    class: "{hot}",
                                    onmouseenter: move |_| hover.set(Some(row)),
                                    onmouseleave: move |_| {
                                        if hover() == Some(row) {
                                            hover.set(None);
                                        }
                                    },
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
