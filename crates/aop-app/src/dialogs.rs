//! Modal dialogs: Task Information, Project Information, Assign Resources,
//! Change Working Time, and the message and about boxes.

use chrono::NaiveDate;
use dioxus::prelude::*;

use aop_core::{
    format_duration, format_work, ConstraintType, DayShifts, LinkType, ScheduleFrom, TaskMode,
};

use crate::controls::{Choice, Dropdown};
use crate::icons::icon;
use aop_core::{Field, FieldGroup};

use crate::state::{format_date, parse_date, AppState, Dialog, PendingAction, QatCommand};

#[component]
pub fn DialogHost(dialog: Dialog) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    rsx! {
        div { class: "scrim",
            oncontextmenu: move |event| event.prevent_default(),
            onclick: move |_| state.write().dialog = None,
            div { class: "dlg", onclick: move |event| event.stop_propagation(),
                match dialog {
                    Dialog::TaskInformation(row) => rsx! { TaskInformation { row } },
                    Dialog::TemplatePreview(id) => rsx! { TemplatePreview { id } },
                    Dialog::ProjectInformation => rsx! { ProjectInformation {} },
                    Dialog::AssignResources => rsx! { AssignResources {} },
                    Dialog::ChangeWorkingTime => rsx! { ChangeWorkingTime {} },
                    Dialog::CustomizeQat => rsx! { CustomizeQat {} },
                    Dialog::BarStyles => rsx! { BarStylesDialog {} },
                    Dialog::FixIssue => rsx! { FixIssue {} },
                    Dialog::InsertColumn(at) => rsx! { InsertColumn { at } },
                    Dialog::UnsavedChanges(action) => rsx! { UnsavedChanges { action } },
                    Dialog::Recover(found) => rsx! { Recover { found } },
                    Dialog::Message { title, body } => rsx! { MessageBox { title, body } },
                }
            }
        }
    }
}

#[component]
fn Head(title: String) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    rsx! {
        div { class: "dlg-head",
            span { "{title}" }
            button { class: "dlg-close", onclick: move |_| state.write().dialog = None, "\u{2715}" }
        }
    }
}

// -------------------------------------------------------- task information

#[component]
fn TaskInformation(row: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut tab = use_signal(|| 0usize);

    let snapshot = {
        let s = state.read();
        s.project.tasks.get(row).cloned()
    };
    let Some(task) = snapshot else {
        return rsx! { MessageBox { title: "Task Information".to_string(), body: "That task no longer exists.".to_string() } };
    };

    let mut name = use_signal(|| task.name.clone());
    let mut duration = use_signal(|| format_duration(task.duration_minutes));
    let mut percent = use_signal(|| task.percent_complete.to_string());
    let mut notes = use_signal(|| task.notes.clone());
    let mut constraint = use_signal(|| task.constraint);
    let mut constraint_date = use_signal(|| {
        task.constraint_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    });
    let mut deadline = use_signal(|| {
        task.deadline
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    });
    let mut mode = use_signal(|| task.mode);

    let (predecessors, resources, currency, is_summary) = {
        let s = state.read();
        let project = &s.project;
        let predecessors: Vec<(String, LinkType, i64)> = project
            .predecessors_of(task.id)
            .into_iter()
            .filter_map(|link| {
                project.index_of(link.predecessor).map(|index| {
                    (
                        format!("{}  {}", index + 1, project.tasks[index].name),
                        link.kind,
                        link.lag_minutes,
                    )
                })
            })
            .collect();
        let resources: Vec<(String, f64)> = task
            .assignments
            .iter()
            .filter_map(|a| project.resource(a.resource).map(|r| (r.name.clone(), a.units)))
            .collect();
        (
            predecessors,
            resources,
            project.currency_symbol.clone(),
            project.is_summary(row),
        )
    };

    let apply = move |_| {
        let new_name = name();
        let new_duration = duration();
        let new_percent = percent();
        let new_notes = notes();
        let new_constraint = constraint();
        let new_constraint_date = constraint_date();
        let new_deadline = deadline();
        let new_mode = mode();

        let mut writer = state.write();
        writer.checkpoint();
        if let Some(target) = writer.project.tasks.get_mut(row) {
            target.name = new_name;
            if let Some((minutes, estimated)) = aop_core::parse_duration(&new_duration) {
                target.duration_minutes = minutes;
                target.estimated = estimated;
            }
            if let Ok(value) = new_percent.trim_end_matches('%').trim().parse::<f64>() {
                target.percent_complete = value.clamp(0.0, 100.0) as u8;
            }
            target.notes = new_notes;
            target.mode = new_mode;
            target.constraint = new_constraint;
            target.constraint_date = if new_constraint.needs_date() {
                parse_date(&new_constraint_date)
            } else {
                None
            };
            target.deadline = parse_date(&new_deadline);
        }
        writer.reschedule();
        writer.dialog = None;
    };

    rsx! {
        Head { title: "Task Information".to_string() }
        div { class: "dlg-tabs",
            for (index, label) in ["General", "Predecessors", "Resources", "Advanced", "Notes"].iter().enumerate() {
                {
                    let class = if tab() == index { "dlg-tab active" } else { "dlg-tab" };
                    rsx! {
                        button { key: "{label}", class: "{class}", onclick: move |_| tab.set(index), "{label}" }
                    }
                }
            }
        }
        div { class: "dlg-body", style: "min-height: 260px;",
            match tab() {
                // ---- General --------------------------------------------
                0 => rsx! {
                    div { class: "form-row",
                        label { "Name" }
                        input { class: "grow", value: "{name}", oninput: move |e| name.set(e.value()) }
                    }
                    div { class: "form-row",
                        label { "Duration" }
                        input { value: "{duration}", disabled: is_summary,
                            oninput: move |e| duration.set(e.value()) }
                        label { style: "width: auto; margin-left: 12px;", "Percent complete" }
                        input { style: "width: 70px;", value: "{percent}",
                            oninput: move |e| percent.set(e.value()) }
                    }
                    div { class: "form-row",
                        label { "Schedule mode" }
                        Dropdown {
                            value: mode().label().to_string(),
                            options: vec![
                                Choice::plain(TaskMode::Auto.label()),
                                Choice::plain(TaskMode::Manual.label()),
                            ],
                            width: 0.0, large: true, disabled: false,
                            on_pick: move |picked: String| {
                                mode.set(if picked == TaskMode::Manual.label() { TaskMode::Manual } else { TaskMode::Auto });
                            },
                        }
                    }
                    div { class: "info-grid", style: "margin-top: 16px;",
                        div { class: "k", "Start" }        div { "{format_date(task.scheduled.start)}" }
                        div { class: "k", "Finish" }       div { "{format_date(task.scheduled.finish)}" }
                        div { class: "k", "Total slack" }  div { "{format_duration(task.scheduled.total_slack_minutes.abs())}" }
                        div { class: "k", "Critical" }     div { if task.scheduled.critical { "Yes" } else { "No" } }
                        div { class: "k", "Work" }         div { "{format_work(task.scheduled.work_minutes)}" }
                        div { class: "k", "Cost" }         div { "{currency}{task.scheduled.cost:.2}" }
                    }
                },

                // ---- Predecessors ---------------------------------------
                1 => rsx! {
                    if predecessors.is_empty() {
                        div { class: "hint", "This task has no predecessors. Add them in the Predecessors column, using entries like 3FS+2 days." }
                    } else {
                        table { class: "assign-table",
                            thead { tr { th { "Task" } th { "Type" } th { "Lag" } } }
                            tbody {
                                for (index, (label, kind, lag)) in predecessors.iter().enumerate() {
                                    tr { key: "p{index}",
                                        td { "{label}" }
                                        td { "{kind.label()}" }
                                        td { {if *lag == 0 { "0 days".to_string() } else { format_duration(*lag) }} }
                                    }
                                }
                            }
                        }
                    }
                },

                // ---- Resources ------------------------------------------
                2 => rsx! {
                    if resources.is_empty() {
                        div { class: "hint", "Nothing is booked onto this task yet. Use Assign Resources on the Resource tab." }
                    } else {
                        table { class: "assign-table",
                            thead { tr { th { "Resource Name" } th { "Units" } } }
                            tbody {
                                for (index, (label, units)) in resources.iter().enumerate() {
                                    tr { key: "r{index}",
                                        td { "{label}" }
                                        td { "{units * 100.0:.0}%" }
                                    }
                                }
                            }
                        }
                    }
                },

                // ---- Advanced -------------------------------------------
                3 => rsx! {
                    div { class: "form-row",
                        label { "Constrain task" }
                        Dropdown {
                            value: constraint().label().to_string(),
                            options: ConstraintType::ALL.iter().map(|c| Choice::plain(c.label())).collect(),
                            width: 0.0, large: true, disabled: false,
                            on_pick: move |picked: String| {
                                if let Some(found) = ConstraintType::ALL.iter().find(|c| c.label() == picked) {
                                    constraint.set(*found);
                                }
                            },
                        }
                    }
                    div { class: "form-row",
                        label { "Constraint date" }
                        input {
                            class: "grow",
                            placeholder: "YYYY-MM-DD",
                            disabled: !constraint().needs_date(),
                            value: "{constraint_date}",
                            oninput: move |e| constraint_date.set(e.value()),
                        }
                    }
                    div { class: "form-row",
                        label { "Deadline" }
                        input {
                            class: "grow",
                            placeholder: "YYYY-MM-DD",
                            value: "{deadline}",
                            oninput: move |e| deadline.set(e.value()),
                        }
                    }
                    div { class: "hint",
                        "A deadline does not move the task. It shows up as negative slack when the schedule runs past it."
                    }
                },

                // ---- Notes ----------------------------------------------
                _ => rsx! {
                    textarea {
                        style: "width: 100%; height: 200px; resize: vertical;",
                        value: "{notes}",
                        oninput: move |e| notes.set(e.value()),
                    }
                },
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button { class: "btn primary", onclick: apply, "OK" }
        }
    }
}

// ----------------------------------------------------- project information

#[component]
fn ProjectInformation() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let snapshot = {
        let s = state.read();
        (
            s.project.name.clone(),
            s.project.start_date,
            s.project.finish_date,
            s.project.schedule_from,
            s.project.status_date,
            s.project.calendar.name.clone(),
            s.project.currency_symbol.clone(),
        )
    };

    let mut name = use_signal(|| snapshot.0.clone());
    let mut start = use_signal(|| snapshot.1.format("%Y-%m-%d").to_string());
    let mut schedule_from = use_signal(|| snapshot.3);
    let mut status = use_signal(|| {
        snapshot
            .4
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    });

    let report = state.read().report.clone();

    let apply = move |_| {
        let new_name = name();
        let new_start = parse_date(&start());
        let new_from = schedule_from();
        let new_status = parse_date(&status());

        let mut writer = state.write();
        writer.checkpoint();
        writer.project.name = new_name;
        if let Some(date) = new_start {
            writer.project.start_date = date;
        }
        writer.project.schedule_from = new_from;
        writer.project.status_date = new_status;
        writer.reschedule();
        writer.dialog = None;
    };

    rsx! {
        Head { title: "Project Information".to_string() }
        div { class: "dlg-body",
            div { class: "form-row",
                label { "Project name" }
                input { class: "grow", value: "{name}", oninput: move |e| name.set(e.value()) }
            }
            div { class: "form-row",
                label { "Start date" }
                input { class: "grow", placeholder: "YYYY-MM-DD", value: "{start}",
                    oninput: move |e| start.set(e.value()) }
            }
            div { class: "form-row",
                label { "Schedule from" }
                Dropdown {
                    value: if schedule_from() == ScheduleFrom::ProjectFinishDate {
                        "Project Finish Date".to_string()
                    } else {
                        "Project Start Date".to_string()
                    },
                    options: vec![
                        Choice::plain("Project Start Date"),
                        Choice::plain("Project Finish Date"),
                    ],
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        schedule_from.set(if picked.starts_with("Project Finish") {
                            ScheduleFrom::ProjectFinishDate
                        } else {
                            ScheduleFrom::ProjectStartDate
                        });
                    },
                }
            }
            div { class: "form-row",
                label { "Status date" }
                input { class: "grow", placeholder: "YYYY-MM-DD (optional)", value: "{status}",
                    oninput: move |e| status.set(e.value()) }
            }
            div { class: "form-row",
                label { "Calendar" }
                input { class: "grow", value: "{snapshot.5}", readonly: true }
            }

            h3 { style: "font-size: 13px; margin: 18px 0 8px;", "Statistics" }
            div { class: "info-grid",
                match &report {
                    Ok(report) => rsx! {
                        div { class: "k", "Finish" }         div { "{format_date(report.finish)}" }
                        div { class: "k", "Duration" }       div { "{format_duration(report.duration_minutes)}" }
                        div { class: "k", "Critical tasks" } div { "{report.critical_task_count}" }
                        div { class: "k", "Work" }           div { "{format_work(report.total_work_minutes)}" }
                        div { class: "k", "Cost" }           div { "{snapshot.6}{report.total_cost:.2}" }
                    },
                    Err(message) => rsx! {
                        div { class: "k", "Schedule" }
                        div { style: "color: var(--bar-critical);", "{message}" }
                    },
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button { class: "btn primary", onclick: apply, "OK" }
        }
    }
}

// -------------------------------------------------------- assign resources

#[component]
fn AssignResources() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut new_name = use_signal(String::new);

    let (task_name, resources, booked, currency) = {
        let s = state.read();
        let row = s.primary();
        let task_name = row
            .and_then(|r| s.project.tasks.get(r))
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "No task selected".into());
        let booked: Vec<u32> = row
            .and_then(|r| s.project.tasks.get(r))
            .map(|t| t.assignments.iter().map(|a| a.resource).collect())
            .unwrap_or_default();
        let resources: Vec<(usize, String, String, f64)> = s
            .project
            .resources
            .iter()
            .enumerate()
            .map(|(index, r)| (index, r.name.clone(), r.group.clone(), r.standard_rate))
            .collect();
        (task_name, resources, booked, s.project.currency_symbol.clone())
    };

    let ids: Vec<u32> = {
        let s = state.read();
        s.project.resources.iter().map(|r| r.id).collect()
    };

    rsx! {
        Head { title: "Assign Resources".to_string() }
        div { class: "dlg-body",
            div { class: "form-row",
                label { "Task" }
                input { class: "grow", value: "{task_name}", readonly: true }
            }

            if resources.is_empty() {
                div { class: "hint", "There are no resources yet. Add one below." }
            } else {
                table { class: "assign-table",
                    thead { tr { th { style: "width: 34px;", "" } th { "Resource Name" } th { "Group" } th { "Std. Rate" } } }
                    tbody {
                        for (index, name, group, rate) in resources {
                            {
                                let assigned = ids.get(index).is_some_and(|id| booked.contains(id));
                                let class = if assigned { "on" } else { "" };
                                let box_class = if assigned { "box on" } else { "box" };
                                rsx! {
                                    tr { key: "ar{index}", class: "{class}",
                                        onclick: move |_| state.write().toggle_assignment(index),
                                        td { span { class: "{box_class}", style: "display: inline-grid;",
                                            if assigned { "\u{2713}" } } }
                                        td { "{name}" }
                                        td { "{group}" }
                                        td { "{currency}{rate:.2}/hr" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "form-row", style: "margin-top: 14px;",
                label { "Add resource" }
                input { class: "grow", placeholder: "Name", value: "{new_name}",
                    oninput: move |e| new_name.set(e.value()) }
                button { class: "btn",
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
            div { class: "hint", "Click a row to book or unbook that resource against the selected task." }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

// ----------------------------------------------------- change working time

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

#[component]
fn ChangeWorkingTime() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut holiday_name = use_signal(|| String::from("Holiday"));
    let mut holiday_date = use_signal(String::new);

    let (week, exceptions, calendar_name) = {
        let s = state.read();
        let week: Vec<bool> = s.project.calendar.week.iter().map(|d| d.is_working()).collect();
        let exceptions: Vec<(String, NaiveDate)> = s
            .project
            .calendar
            .exceptions
            .iter()
            .map(|e| (e.name.clone(), e.from))
            .collect();
        (week, exceptions, s.project.calendar.name.clone())
    };

    rsx! {
        Head { title: "Change Working Time".to_string() }
        div { class: "dlg-body",
            div { class: "form-row",
                label { "For calendar" }
                input { class: "grow", value: "{calendar_name}", readonly: true }
            }

            h3 { style: "font-size: 13px; margin: 14px 0 8px;", "Working week" }
            for (index, day) in WEEKDAYS.iter().enumerate() {
                {
                    let working = week.get(index).copied().unwrap_or(false);
                    let box_class = if working { "box on" } else { "box" };
                    rsx! {
                        div { key: "{day}", class: "rcheck", style: "height: 24px;",
                            onclick: move |_| {
                                let mut writer = state.write();
                                writer.checkpoint();
                                if let Some(slot) = writer.project.calendar.week.get_mut(index) {
                                    *slot = if working { DayShifts::nonworking() } else { DayShifts::standard() };
                                }
                                writer.reschedule();
                            },
                            span { class: "{box_class}", if working { "\u{2713}" } }
                            span { style: "width: 100px;", "{day}" }
                            span { style: "color: var(--ink-soft);",
                                if working { "08:00 - 12:00, 13:00 - 17:00" } else { "Nonworking" }
                            }
                        }
                    }
                }
            }

            h3 { style: "font-size: 13px; margin: 18px 0 8px;", "Exceptions" }
            if exceptions.is_empty() {
                div { class: "hint", "No holidays or shutdowns have been added." }
            } else {
                table { class: "assign-table",
                    thead { tr { th { "Name" } th { "Date" } th { style: "width: 60px;", "" } } }
                    tbody {
                        for (index, (name, date)) in exceptions.iter().enumerate() {
                            tr { key: "ex{index}",
                                td { "{name}" }
                                td { "{date}" }
                                td {
                                    button { class: "btn", style: "padding: 1px 8px;",
                                        onclick: move |_| {
                                            let mut writer = state.write();
                                            writer.checkpoint();
                                            if index < writer.project.calendar.exceptions.len() {
                                                writer.project.calendar.exceptions.remove(index);
                                            }
                                            writer.reschedule();
                                        },
                                        "Remove"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "form-row", style: "margin-top: 12px;",
                label { "Add exception" }
                input { style: "flex: 1;", placeholder: "Name", value: "{holiday_name}",
                    oninput: move |e| holiday_name.set(e.value()) }
                input { style: "width: 130px;", placeholder: "YYYY-MM-DD", value: "{holiday_date}",
                    oninput: move |e| holiday_date.set(e.value()) }
                button { class: "btn",
                    onclick: move |_| {
                        let Some(date) = parse_date(&holiday_date()) else { return };
                        let name = holiday_name();
                        let mut writer = state.write();
                        writer.checkpoint();
                        writer.project.calendar.exceptions.push(aop_core::CalendarException {
                            name: if name.trim().is_empty() { "Holiday".into() } else { name },
                            from: date.date(),
                            to: date.date(),
                            shifts: DayShifts::nonworking(),
                        });
                        writer.reschedule();
                        holiday_date.set(String::new());
                    },
                    "Add"
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "OK" }
        }
    }
}

// ------------------------------------------------------------ message/about

#[component]
fn MessageBox(title: String, body: String) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    rsx! {
        Head { title: title.clone() }
        div { class: "dlg-body",
            div { style: "display: flex; gap: 12px; align-items: flex-start;",
                {icon("warning", 28)}
                div { style: "line-height: 1.5;", "{body}" }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "OK" }
        }
    }
}

// ------------------------------------------------------ unsaved changes

/// Ask what to do with unsaved work before something throws it away.
///
/// The wording names the plan and what is about to happen, because the whole
/// point of the question is that the answer is not recoverable: Don't Save
/// cannot be undone once the plan is gone.
#[component]
fn UnsavedChanges(action: PendingAction) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (name, saved_to) = {
        let s = state.read();
        (
            s.project.name.clone(),
            s.file_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        )
    };
    let what = action.describe();

    // Each button owns its own copy, since a click handler outlives this render.
    let for_save = action.clone();
    let for_discard = action;

    rsx! {
        Head { title: "Alterion Open Project".to_string() }
        div { class: "dlg-body",
            div { style: "display: flex; gap: 14px; align-items: flex-start;",
                span { style: "color: var(--contextual); flex: none;", {icon("warning", 28)} }
                div { style: "line-height: 1.6;",
                    div { style: "font-weight: 600; margin-bottom: 4px;",
                        "Save changes to \"{name}\"?" }
                    div { style: "color: var(--ink-soft); font-size: 12px;",
                        "Your changes will be lost if you continue without saving." }
                    if !saved_to.is_empty() {
                        div { style: "color: var(--ink-faint); font-size: 11px; margin-top: 6px;",
                            "{saved_to}" }
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button {
                class: "btn",
                onclick: move |_| state.write().dialog = None,
                "Cancel {what}"
            }
            button {
                class: "btn danger",
                onclick: move |_| state.write().carry_out(for_discard.clone()),
                "Don't Save"
            }
            button {
                class: "btn primary",
                onclick: move |_| state.write().save_then(for_save.clone()),
                "Save"
            }
        }
    }
}

// ------------------------------------------------------------ recovery

/// Offer back work left behind by a session that never finished.
///
/// The offer is deliberately not automatic. Loading it without asking would
/// replace whatever the user opened the application to look at, and a snapshot
/// is not necessarily the version they want.
#[component]
fn Recover(found: crate::recovery::Recovered) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let origin = found
        .origin
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "a plan that was never saved".into());

    let to_take = found.clone();
    let to_drop = found.clone();

    rsx! {
        Head { title: "Recover unsaved work".to_string() }
        div { class: "dlg-body",
            div { style: "display: flex; gap: 14px; align-items: flex-start;",
                span { style: "color: var(--accent-bright); flex: none;", {icon("history", 28)} }
                div { style: "line-height: 1.6;",
                    div { style: "font-weight: 600; margin-bottom: 4px;",
                        "\"{found.name}\" was not saved before it closed." }
                    div { style: "color: var(--ink-soft); font-size: 12px;",
                        "A snapshot of it was kept. Opening it brings the changes back unsaved, so you choose where they go." }
                    div { style: "color: var(--ink-faint); font-size: 11px; margin-top: 6px;",
                        "From {origin}" }
                }
            }
        }
        div { class: "dlg-foot",
            button {
                class: "btn danger",
                onclick: move |_| {
                    crate::recovery::clear(&to_drop.snapshot);
                    state.write().dialog = None;
                },
                "Discard"
            }
            button {
                class: "btn primary",
                onclick: move |_| state.write().recover(to_take.clone()),
                "Recover"
            }
        }
    }
}

// ----------------------------------------------------- template preview

/// Preview a starter template, with a real chart of what it will create.
#[component]
fn TemplatePreview(id: String) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let Some(spec) = aop_core::templates::by_id(&id) else {
        return rsx! { MessageBox { title: "Preview".to_string(), body: "That template is not available.".to_string() } };
    };

    // Build and schedule the template so the preview shows real dates.
    let built = use_signal({
        let id = id.clone();
        move || {
            let start = chrono::Local::now().naive_local().date();
            let start = crate::state::next_monday(start)
                .and_hms_opt(8, 0, 0)
                .expect("valid time");
            aop_core::templates::by_id(&id).map(|spec| {
                let mut project = aop_core::templates::build(spec, start);
                let report = aop_core::schedule(&mut project).ok();
                (project, report)
            })
        }
    });

    let snapshot = built.read();
    let Some((project, report)) = snapshot.as_ref() else {
        return rsx! { MessageBox { title: "Preview".to_string(), body: "Could not build that template.".to_string() } };
    };

    let create_id = id.clone();

    rsx! {
        Head { title: format!("{} \u{2014} preview", spec.name) }
        div { class: "dlg-body", style: "min-width: 780px;",
            div { class: "tpl-desc", style: "font-size: 12.5px; margin-bottom: 14px;", "{spec.description}" }

            div { style: "background: var(--surface-2); border: 1px solid var(--line); border-radius: 5px; margin-bottom: 16px;",
                {crate::preview::mini_gantt(project, 760.0, 260.0, 40, &crate::preview::DARK, true)}
            }

            div { class: "info-grid",
                div { class: "k", "Tasks" }     div { "{project.tasks.len()}" }
                div { class: "k", "Links" }     div { "{project.links.len()}" }
                div { class: "k", "Resources" } div { "{project.resources.len()}" }
                match report {
                    Some(report) => rsx! {
                        div { class: "k", "Duration" }       div { "{format_duration(report.duration_minutes)}" }
                        div { class: "k", "Finish" }         div { "{format_date(report.finish)}" }
                        div { class: "k", "Critical tasks" } div { "{report.critical_task_count}" }
                        div { class: "k", "Work" }           div { "{format_work(report.total_work_minutes)}" }
                    },
                    None => rsx! {},
                }
            }

            h3 { style: "font-size: 13px; margin: 18px 0 7px;", "Outline" }
            div { style: "max-height: 210px; overflow-y: auto; border: 1px solid var(--line); border-radius: 4px;",
                table { class: "assign-table",
                    tbody {
                        for index in 0..project.tasks.len() {
                            {
                                let task = &project.tasks[index];
                                let indent = task.outline_level as f64 * 16.0;
                                let bold = if project.is_summary(index) { "600" } else { "400" };
                                rsx! {
                                    tr { key: "tp{index}",
                                        td { style: "padding-left: {indent + 8.0}px; font-weight: {bold};", "{task.name}" }
                                        td { style: "width: 90px;", "{format_duration(task.scheduled.duration_minutes)}" }
                                        td { style: "width: 110px;", "{format_date(task.scheduled.start)}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button { class: "btn primary",
                onclick: move |_| {
                    state.write().dialog = None;
                    state
                        .write()
                        .guard(PendingAction::NewFromTemplate(create_id.clone()));
                },
                "Create"
            }
        }
    }
}


// -------------------------------------------------- quick access toolbar

/// Choose which commands sit on the Quick Access Toolbar, and their order.
#[component]
fn CustomizeQat() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let current = state.read().qat.clone();

    rsx! {
        Head { title: "Customize Quick Access Toolbar".to_string() }
        div { class: "dlg-body", style: "min-width: 640px;",
            div { class: "hint", style: "margin: 0 0 14px;",
                "Ticked commands appear next to the title, in the order shown on the right." }

            div { style: "display: flex; gap: 18px; align-items: flex-start;",

                // ---- every available command ---------------------------
                div { style: "flex: 1;",
                    h3 { style: "font-size: 12px; margin: 0 0 8px; color: var(--ink-soft);", "Available commands" }
                    div { style: "border: 1px solid var(--line); border-radius: 4px; max-height: 320px; overflow-y: auto;",
                        for command in QatCommand::ALL {
                            {
                                let on = current.contains(&command);
                                let box_class = if on { "box on" } else { "box" };
                                rsx! {
                                    div {
                                        key: "{command:?}",
                                        class: "rcheck",
                                        style: "height: 30px; padding: 0 10px; border-radius: 0;",
                                        onclick: move |_| state.write().toggle_qat(command),
                                        span { class: "{box_class}", if on { "\u{2713}" } }
                                        span { class: "glyph", style: "display: grid; place-items: center; width: 18px; color: var(--accent);",
                                            {icon(command.glyph(), 15)} }
                                        span { "{command.label()}" }
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- what is on the bar, in order ----------------------
                div { style: "flex: 1;",
                    h3 { style: "font-size: 12px; margin: 0 0 8px; color: var(--ink-soft);", "On the toolbar" }
                    div { class: "qat-list", style: "max-height: 320px; overflow-y: auto;",
                        if current.is_empty() {
                            div { class: "hint", style: "padding: 12px;", "The toolbar is empty." }
                        } else {
                            for (index, command) in current.iter().copied().enumerate() {
                                div { key: "on{command:?}", class: "qat-item",
                                    span { class: "qat-glyph", {icon(command.glyph(), 15)} }
                                    span { class: "qat-name", "{command.label()}" }
                                    div { class: "btn-group",
                                        button {
                                            class: "iconbtn", title: "Move up",
                                            disabled: index == 0,
                                            onclick: move |_| state.write().move_qat(command, -1),
                                            "\u{2191}"
                                        }
                                        button {
                                            class: "iconbtn", title: "Move down",
                                            disabled: index + 1 == current.len(),
                                            onclick: move |_| state.write().move_qat(command, 1),
                                            "\u{2193}"
                                        }
                                        button {
                                            class: "iconbtn danger", title: "Remove",
                                            onclick: move |_| state.write().toggle_qat(command),
                                            "\u{2715}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().reset_qat(), "Reset" }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}


// ------------------------------------------------------------- bar colours

/// Recolour the chart. The colours are stored on the plan, so they travel with
/// the file rather than being a setting of this machine.
#[component]
fn BarStylesDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let styles = state.read().project.bar_styles.clone();
    let current = state.read().gantt_style;

    rsx! {
        Head { title: "Bar Styles".to_string() }
        div { class: "dlg-body", style: "min-width: 520px;",

            h3 { style: "font-size: 12.5px; margin: 0 0 10px;", "Palettes" }
            div { class: "gallery", style: "flex-wrap: wrap; gap: 8px;",
                for (index, (name, colours)) in aop_core::BarStyles::PRESETS.iter().enumerate() {
                    {
                        let class = if index == current { "gallery-item on" } else { "gallery-item" };
                        rsx! {
                            button { key: "{name}", class: "{class}", title: "{name}",
                                style: "width: 78px; height: 54px;",
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

            h3 { style: "font-size: 12.5px; margin: 22px 0 10px;", "Individual colours" }
            for (label, value) in styles.fields() {
                {
                    let key = label.to_string();
                    let current_value = value.to_string();
                    rsx! {
                        div { key: "{label}", class: "colour-row",
                            span { class: "colour-swatch", style: "background: {current_value};" }
                            span { class: "colour-name", "{label}" }
                            input {
                                class: "colour-picker",
                                r#type: "color",
                                value: "{current_value}",
                                oninput: move |event| state.write().set_bar_colour(&key, &event.value()),
                            }
                            span { class: "colour-hex", "{current_value}" }
                        }
                    }
                }
            }

            div { class: "hint",
                "Critical task colour is only used while Critical Tasks is switched on, from the Format tab." }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().apply_bar_preset(0), "Reset" }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}


// ---------------------------------------------------------------- repair

/// Offer a repair for a plan that will not schedule, showing exactly what it
/// would change before anything is touched.
#[component]
fn FixIssue() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let remedy = state.read().remedy();

    let Some(remedy) = remedy else {
        return rsx! {
            Head { title: "Fix issue".to_string() }
            div { class: "dlg-body",
                div { style: "display: flex; gap: 12px; align-items: flex-start;",
                    {icon("mark-on-track", 26)}
                    div { style: "line-height: 1.55;",
                        "There is nothing to fix. The plan schedules cleanly."
                    }
                }
            }
            div { class: "dlg-foot",
                button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
            }
        };
    };

    let to_apply = remedy.clone();

    rsx! {
        Head { title: "Fix issue".to_string() }
        div { class: "dlg-body", style: "min-width: 560px;",

            div { class: "fix-problem",
                span { class: "fix-icon", {icon("warning", 20)} }
                div { "{remedy.problem}" }
            }

            h3 { class: "fix-head", "What this will do" }
            div { class: "fix-action", "{remedy.action}" }

            h3 { class: "fix-head", "Changes" }
            div { class: "fix-changes",
                for (index, change) in remedy.changes.iter().enumerate() {
                    div { key: "chg{index}", class: "fix-change",
                        span { class: "fix-bullet", {icon("clear", 13)} }
                        span { "{change}" }
                    }
                }
            }

            div { class: "hint",
                "Nothing has changed yet. Applying this can be undone with Ctrl+Z."
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button {
                class: "btn primary",
                onclick: move |_| state.write().apply_remedy(&to_apply),
                "Apply the fix"
            }
        }
    }
}


// --------------------------------------------------------- insert column

/// Pick a field to show as a column, the way Project's Insert Column works:
/// every field, grouped, searchable, with a line saying what it holds.
#[component]
fn InsertColumn(at: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut search = use_signal(String::new);

    let shown: Vec<Field> = {
        let s = state.read();
        s.columns.iter().map(|c| c.field).collect()
    };
    let needle = search().to_lowercase();

    rsx! {
        Head { title: "Insert Column".to_string() }
        div { class: "dlg-body", style: "min-width: 620px;",
            div { class: "bs-field", style: "margin: 0 0 12px;",
                input {
                    class: "bs-input",
                    autofocus: true,
                    placeholder: "Search fields",
                    value: "{search}",
                    oninput: move |event| search.set(event.value()),
                }
            }

            div { class: "field-list",
                for group in FieldGroup::ORDER {
                    {
                        let matching: Vec<Field> = Field::ALL
                            .iter()
                            .copied()
                            .filter(|f| f.group() == group)
                            .filter(|f| {
                                needle.is_empty()
                                    || f.label().to_lowercase().contains(&needle)
                                    || f.description().to_lowercase().contains(&needle)
                            })
                            .collect();

                        if matching.is_empty() {
                            rsx! {}
                        } else {
                            rsx! {
                                div { key: "g{group:?}",
                                    div { class: "field-group", "{group.label()}" }
                                    for field in matching {
                                        {
                                            let already = shown.contains(&field);
                                            let class = if already { "field-row shown" } else { "field-row" };
                                            rsx! {
                                                div {
                                                    key: "f{field:?}",
                                                    class: "{class}",
                                                    onclick: move |_| {
                                                        if !already {
                                                            state.write().insert_column(at, field);
                                                            state.write().dialog = None;
                                                        }
                                                    },
                                                    div { class: "field-text",
                                                        div { class: "field-name", "{field.label()}" }
                                                        div { class: "field-desc", "{field.description()}" }
                                                    }
                                                    if already {
                                                        span { class: "field-badge", "Shown" }
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

            div { class: "hint",
                "The new column goes in before the one that was right-clicked. Drag a column edge to "
                "resize it, and right-click a heading to move or hide it."
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().reset_columns(), "Reset table" }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}
