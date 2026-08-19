//! Modal dialogs: Task Information, Project Information, Assign Resources,
//! Change Working Time, and the message and about boxes.

use chrono::{Datelike, NaiveDate};
use dioxus::prelude::*;

use aop_core::holidays;
use aop_core::{
    format_duration, format_work, CalendarTarget, ConstraintType, DayShifts, ResourceKind,
    ScheduleFrom, TaskMode,
};

use std::path::PathBuf;

use aop_core::leveling::{LevelOrder, LevelScope};
use aop_core::textstyle::{StyleTarget, TextStyle};
use aop_core::update::UpdateOptions;

use crate::backstage::OptCheck;

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
                    Dialog::ResourceInformation { row, tab } => rsx! { ResourceInformation { row, tab } },
                    Dialog::LevelingOptions => rsx! { LevelingOptionsDialog {} },
                    Dialog::InsertSubproject => rsx! { InsertSubproject {} },
                    Dialog::LinksBetweenProjects => rsx! { LinksBetweenProjects {} },
                    Dialog::UpdateProject => rsx! { UpdateProjectDialog {} },
                    Dialog::TextStyles => rsx! { TextStylesDialog {} },
                    Dialog::Layout => rsx! { LayoutDialog {} },
                    Dialog::FormatDrawing(id) => rsx! { FormatDrawing { id } },
                    Dialog::TemplatePreview(id) => rsx! { TemplatePreview { id } },
                    Dialog::ProjectInformation => rsx! { ProjectInformation {} },
                    Dialog::AssignResources => rsx! { AssignResources {} },
                    Dialog::ChangeWorkingTime => rsx! { ChangeWorkingTime {} },
                    Dialog::CustomizeQat => rsx! { CustomizeQat {} },
                    Dialog::BarStyles => rsx! { BarStylesDialog {} },
                    Dialog::FixIssue => rsx! { FixIssue {} },
                    Dialog::CustomFields => rsx! { CustomFieldsDialog {} },
                    Dialog::ExternalDependencies => rsx! { ExternalDependenciesDialog {} },
                    Dialog::InsertColumn(at) => rsx! { InsertColumn { at } },
                    Dialog::History => rsx! { HistoryDialog {} },
                    Dialog::UnsavedChanges(action) => rsx! { UnsavedChanges { action } },
                    Dialog::Recover(found) => rsx! { Recover { found } },
                    Dialog::SyncBehind {
                        head, sentence, differences, changes, replayed, asked, more,
                    } => rsx! {
                        SyncBehind { head, sentence, differences, changes, replayed, asked, more }
                    },
                    Dialog::SyncAhead { head, cursor } => rsx! { SyncAhead { head, cursor } },
                    Dialog::FreshCopy { why } => rsx! { FreshCopy { why } },
                    Dialog::OpenLink(share) => rsx! { OpenLink { share } },
                    Dialog::RestoreVersion(index) => rsx! { RestoreVersion { index } },
                    Dialog::HealthCheck => rsx! { HealthCheck {} },
                    Dialog::UpdateAvailable => rsx! { UpdateAvailable {} },
                    Dialog::ConfirmOverwrite { path, beside } => rsx! {
                        ConfirmOverwrite { path, beside }
                    },
                    Dialog::Message { title, body } => rsx! { MessageBox { title, body } },
                }
            }
        }
    }
}

/// A newer release, and what this copy is allowed to do about it.
///
/// The refusals are the interesting part. A copy a package manager installed
/// is told the new version exists and told the command that fetches it, rather
/// than being given a button that would write over files pacman owns and leave
/// the next system upgrade to clean up after it. A plan with unsaved changes
/// stops the install too: it replaces the running program, and work that only
/// exists in this window would go with it.
#[component]
fn UpdateAvailable() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (found, message, working, ready) = {
        let s = state.read();
        (
            s.update_found.clone(),
            s.update_message.clone(),
            s.updating,
            s.update_ready.clone(),
        )
    };
    let blocked = state.read().update_blocked();

    rsx! {
        Head { title: "Update".to_string() }
        div { class: "dlg-body", style: "min-width: 460px;",
            match (&found, &ready) {
                (_, Some(crate::updates::Installed::Downloaded { installer, .. })) => rsx! {
                    p { class: "hint", style: "margin-top: 0;",
                        "The installer has been downloaded and checked against the checksum \
                         published with it. Installing closes this application, which is what \
                         frees its files to be replaced, and starts the new version when it is \
                         done."
                    }
                    div { class: "sync-row",
                        span { class: "sync-key", "Installer" }
                        span { class: "sync-value mono", "{installer.display()}" }
                    }
                    // Closing is no longer something to walk through a wizard
                    // first: this window goes the moment the button is
                    // pressed, so anything unsaved has to be said before it
                    // rather than after.
                    if let Some(why) = &blocked {
                        p { class: "sync-why", style: "margin-top: 12px;", "{why}" }
                    }
                },
                (Some(found), _) => rsx! {
                    div { class: "sync-row",
                        span { class: "sync-key", "Installed" }
                        span { class: "sync-value", "{crate::welcome::RUNNING}" }
                    }
                    div { class: "sync-row",
                        span { class: "sync-key", "Available" }
                        span { class: "sync-value good", "{found.version}" }
                    }
                    if let Some(artefact) = &found.artefact {
                        div { class: "sync-row",
                            span { class: "sync-key", "Download" }
                            span { class: "sync-value mono", "{artefact.name}" }
                        }
                    }
                    div { class: "sync-row",
                        span { class: "sync-key", "Release page" }
                        span { class: "sync-value mono", "{found.page}" }
                    }
                    if let Some(why) = &blocked {
                        p { class: "sync-why", style: "margin-top: 12px;", "{why}" }
                    } else {
                        p { class: "hint",
                            "The download is checked against the checksum published beside it, and                              refused if it does not match. The current version is kept, so a new                              one that will not start can be put back."
                        }
                    }
                },
                (None, _) => rsx! {
                    p { class: "hint", style: "margin-top: 0;",
                        {message.clone().unwrap_or_else(|| {
                            format!("Version {} is the newest there is.", crate::welcome::RUNNING)
                        })}
                    }
                },
            }

            if working {
                p { class: "hint", "Fetching and checking the new version..." }
            } else if let Some(message) = message.clone().filter(|_| found.is_some()) {
                p { class: "sync-why", style: "margin-top: 12px;", "{message}" }
            }
        }
        div { class: "dlg-foot",
            button {
                class: "btn",
                onclick: move |_| crate::updates::ask_in_background(state),
                disabled: working,
                "Check again"
            }
            div { class: "grow" }
            if let Some(crate::updates::Installed::Downloaded { installer, sha256 }) = ready {
                button {
                    class: "btn primary",
                    disabled: blocked.is_some() || working,
                    onclick: move |_| {
                        match crate::updates::run_installer(&installer, &sha256) {
                            Ok(()) => state.write().quit_requested = true,
                            // Refusing to run it is the point of checking, so
                            // say so rather than closing on a failure.
                            Err(why) => state.write().update_message = Some(why),
                        }
                    },
                    "Install and restart"
                }
            } else if found.as_ref().is_some_and(|found| found.installable()) {
                button {
                    class: "btn primary",
                    disabled: blocked.is_some() || working,
                    onclick: move |_| crate::updates::install_in_background(state),
                    "Install it"
                }
            }
            // Offered for any release that was found, including one this copy
            // will not install itself: somebody told to run a package manager
            // command is exactly the person who may want to be left alone
            // about this version and told about the next.
            if found.is_some() {
                button {
                    class: "btn",
                    disabled: working,
                    title: "This version is never offered again. Later versions still are.",
                    onclick: move |_| {
                        let mut writer = state.write();
                        writer.skip_the_found_version();
                        writer.dialog = None;
                    },
                    "Skip this version"
                }
            }
            // What closing the dialog has always meant, said out loud. Leaving
            // it as a dismissal makes "ask me again" the one answer of the
            // three with no button, which reads as though it were not an
            // answer at all.
            button {
                class: "btn",
                onclick: move |_| state.write().dialog = None,
                if found.is_some() { "Not now" } else { "Close" }
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
    let mut task_calendar = use_signal(|| task.calendar.clone());
    let mut ignore_resource_calendars = use_signal(|| task.ignore_resource_calendars);

    let (currency, is_summary, calendar_choices) = {
        let s = state.read();
        // An empty name is what "the project's" is stored as, so the picker
        // offers it under the project calendar's own name rather than blank.
        let mut choices = vec![Choice::new(
            "",
            format!("Project calendar ({})", s.project.calendar.name),
        )];
        for calendar in s.project.calendars.iter() {
            choices.push(Choice::plain(calendar.name.clone()));
        }
        (
            s.project.currency_symbol.clone(),
            s.project.is_summary(row),
            choices,
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
        let new_calendar = task_calendar();
        let new_ignore = ignore_resource_calendars();

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
            target.calendar = new_calendar;
            target.ignore_resource_calendars = new_ignore;
        }
        writer.reschedule();
        writer.dialog = None;
    };

    rsx! {
        Head { title: "Task Information".to_string() }
        div { class: "dlg-tabs",
            for (index, label) in ["General", "Predecessors", "Successors", "Resources", "Advanced", "Notes"].iter().enumerate() {
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
                // The same picker the grid opens, so there is one way to set a
                // dependency rather than two that behave differently.
                1 => rsx! {
                    crate::popups::LinkPicker { row, end: crate::popups::LinkEnd::Predecessors }
                },

                // ---- Successors -----------------------------------------
                // The same picker again, pointed the other way. A successor is
                // the link this task is the predecessor of, so the tab that
                // sets one is the tab that sets a predecessor read backwards.
                2 => rsx! {
                    crate::popups::LinkPicker { row, end: crate::popups::LinkEnd::Successors }
                },

                // ---- Resources ------------------------------------------
                3 => rsx! {
                    crate::popups::ResourcePicker { row }
                },

                // ---- Advanced -------------------------------------------
                4 => rsx! {
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

                    h3 { style: "font-size: 13px; margin: 18px 0 8px;", "Calendar" }
                    div { class: "form-row",
                        label { "Work to" }
                        Dropdown {
                            value: "{task_calendar}",
                            options: calendar_choices,
                            width: 0.0, large: true, disabled: false,
                            on_pick: move |picked: String| task_calendar.set(picked),
                        }
                    }
                    div { class: "rcheck", style: "height: 24px;",
                        onclick: move |_| {
                            let was = ignore_resource_calendars();
                            ignore_resource_calendars.set(!was);
                        },
                        span {
                            class: if ignore_resource_calendars() { "box on" } else { "box" },
                            if ignore_resource_calendars() { "\u{2713}" }
                        }
                        span { "Scheduling ignores resource calendars" }
                    }
                    div { class: "hint",
                        "The task is worked only in time that is working in both this calendar \
                         and every assigned person's, unless the box is ticked. The box does \
                         nothing until a calendar other than the project's is chosen."
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

// ---------------------------------------------------------- format drawing

/// One annotation shape: what it says, and how it looks.
///
/// Everything is applied as it is changed rather than on OK, because the shape
/// is visible behind the dialog and watching it change is the point.
#[component]
fn FormatDrawing(id: aop_core::draw::DrawingId) -> Element {
    use aop_core::draw::{LineStyle, ShapeKind};

    let mut state = use_context::<Signal<AppState>>();

    let snapshot = state
        .read()
        .project
        .drawings
        .iter()
        .find(|d| d.id == id)
        .cloned();
    let Some(drawing) = snapshot else {
        return rsx! {
            MessageBox {
                title: "Format Drawing".to_string(),
                body: "That drawing is no longer there.".to_string(),
            }
        };
    };

    let holds_text = drawing.kind == ShapeKind::TextBox;
    let closed = matches!(
        drawing.kind,
        ShapeKind::Rectangle | ShapeKind::Oval | ShapeKind::TextBox
    );

    rsx! {
        Head { title: format!("Format {}", drawing.kind.label()) }
        div { class: "dlg-body", style: "min-width: 520px; min-height: 250px;",

            if holds_text {
                div { class: "form-row",
                    label { "Text" }
                    input {
                        class: "grow",
                        value: "{drawing.text}",
                        oninput: move |event| {
                            let text = event.value();
                            state.write().amend_drawing(id, move |d| d.text = text.clone());
                        },
                    }
                }
                div { class: "sep" }
            }

            div { class: "form-row",
                label { "Line colour" }
                Dropdown {
                    value: drawing.style.line_colour.clone(),
                    options: STYLE_COLOURS.iter().map(|(name, token)| Choice::new(*token, *name)).collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        state.write().amend_drawing(id, move |d| d.style.line_colour = picked.clone());
                    },
                }
            }
            div { class: "form-row",
                label { "Line style" }
                Dropdown {
                    value: match drawing.style.line_style {
                        LineStyle::Solid => "solid",
                        LineStyle::Dashed => "dashed",
                        LineStyle::Dotted => "dotted",
                    }.to_string(),
                    options: vec![
                        Choice::new("solid", "Solid"),
                        Choice::new("dashed", "Dashed"),
                        Choice::new("dotted", "Dotted"),
                    ],
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        let chosen = match picked.as_str() {
                            "dashed" => LineStyle::Dashed,
                            "dotted" => LineStyle::Dotted,
                            _ => LineStyle::Solid,
                        };
                        state.write().amend_drawing(id, move |d| d.style.line_style = chosen);
                    },
                }
                label { style: "width: auto; margin-left: 12px;", "Width" }
                input {
                    style: "width: 70px;",
                    value: "{drawing.style.line_width}",
                    onchange: move |event| {
                        if let Ok(width) = event.value().trim().parse::<f64>() {
                            let width = width.clamp(0.0, 12.0);
                            state.write().amend_drawing(id, move |d| d.style.line_width = width);
                        }
                    },
                }
            }

            if closed {
                div { class: "form-row",
                    label { "Fill" }
                    Dropdown {
                        value: drawing.style.fill_colour.clone(),
                        options: std::iter::once(Choice::new("", "None"))
                            .chain(STYLE_COLOURS.iter().skip(1).map(|(name, token)| Choice::new(*token, *name)))
                            .collect(),
                        width: 0.0, large: true, disabled: false,
                        on_pick: move |picked: String| {
                            state.write().amend_drawing(id, move |d| d.style.fill_colour = picked.clone());
                        },
                    }
                }
            }

            div { class: "sep" }
            OptCheck {
                label: "Draw behind the task bars".to_string(),
                on_state: drawing.behind_bars,
                on: move |_| {
                    let now = !drawing.behind_bars;
                    state.write().amend_drawing(id, move |d| d.behind_bars = now);
                },
            }
            OptCheck {
                label: "Lock so it cannot be dragged".to_string(),
                on_state: drawing.locked,
                on: move |_| {
                    let now = !drawing.locked;
                    state.write().amend_drawing(id, move |d| d.locked = now);
                },
            }
            div { class: "hint",
                "A shape drawn over a bar hides it. Sending it behind is how a highlight sits under the bar it is marking rather than covering it."
            }
        }
        div { class: "dlg-foot",
            button {
                class: "btn",
                onclick: move |_| {
                    let mut writer = state.write();
                    writer.selected_drawing = Some(id);
                    writer.delete_selected_drawing();
                    writer.dialog = None;
                },
                "Delete"
            }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

// ------------------------------------------------------------------ layout

/// Flip a display choice and write it straight to the config.
///
/// These follow the planner rather than the plan, so they belong in the
/// config file, not in the saved project.
fn keep(mut state: Signal<AppState>, edit: fn(&mut AppState)) {
    let settings = {
        let mut writer = state.write();
        edit(&mut writer);
        writer.settings()
    };
    settings.save();
}

#[component]
fn LayoutDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (round_bars, show_links, bar_text, rows, columns, status_line) = {
        let s = state.read();
        (
            s.round_bars,
            s.show_links,
            s.bar_text,
            s.grid_rows,
            s.grid_columns,
            s.grid_status_date,
        )
    };

    rsx! {
        Head { title: "Layout".to_string() }
        div { class: "dlg-body", style: "min-width: 520px; min-height: 280px;",
            h3 { class: "dlg-sub", "Bars" }
            OptCheck {
                label: "Round bars to whole days".to_string(),
                on_state: round_bars,
                on: move |_| keep(state, |s| s.round_bars = !s.round_bars),
            }
            OptCheck {
                label: "Show the task name beside its bar".to_string(),
                on_state: bar_text,
                on: move |_| keep(state, |s| s.bar_text = !s.bar_text),
            }
            OptCheck {
                label: "Draw dependency arrows".to_string(),
                on_state: show_links,
                on: move |_| keep(state, |s| s.show_links = !s.show_links),
            }
            div { class: "hint",
                "Rounding draws a bar to whole days, so half a day of work still reads as a day wide. It changes the picture only, never the schedule."
            }

            div { class: "sep" }
            h3 { class: "dlg-sub", "Gridlines" }
            OptCheck {
                label: "Row lines".to_string(),
                on_state: rows,
                on: move |_| keep(state, |s| s.grid_rows = !s.grid_rows),
            }
            OptCheck {
                label: "Column lines".to_string(),
                on_state: columns,
                on: move |_| keep(state, |s| s.grid_columns = !s.grid_columns),
            }
            OptCheck {
                label: "Status date line".to_string(),
                on_state: status_line,
                on: move |_| keep(state, |s| s.grid_status_date = !s.grid_status_date),
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

// -------------------------------------------------------------- text styles

/// Colours offered for text and fill, as palette tokens so both themes work.
const STYLE_COLOURS: [(&str, &str); 8] = [
    ("Theme default", ""),
    ("Muted", "var(--ink-soft)"),
    ("Critical", "var(--danger)"),
    ("Accent", "var(--accent)"),
    ("Warning", "var(--warning)"),
    ("Success", "var(--success)"),
    ("Ink", "var(--ink)"),
    ("Surface", "var(--surface)"),
];

/// Change one field of a category's style, dropping the entry entirely when
/// nothing is left set, so an untouched category stays absent rather than
/// holding an empty record.
fn amend(mut state: Signal<AppState>, target: StyleTarget, edit: impl Fn(&mut TextStyle)) {
    let mut writer = state.write();
    let mut style = writer.text_styles.style_of(target);
    edit(&mut style);
    if style.is_unset() {
        writer.text_styles.clear(target);
    } else {
        writer.text_styles.set(target, style);
    }
    writer.dirty = true;
}

#[component]
fn TextStylesDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut target = use_signal(|| StyleTarget::All);

    // Read back whatever this category already carries, so the boxes show the
    // current answer rather than a blank form.
    let current = state.read().text_styles.style_of(target());


    rsx! {
        Head { title: "Text Styles".to_string() }
        div { class: "dlg-body", style: "min-width: 600px; min-height: 300px;",
            div { class: "form-row",
                label { "Item to change" }
                Dropdown {
                    value: format!("{:?}", target()),
                    options: StyleTarget::ALL
                        .iter()
                        .map(|entry| Choice::new(format!("{entry:?}"), entry.label()))
                        .collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        if let Some(found) = StyleTarget::ALL.iter().find(|t| format!("{t:?}") == picked) {
                            target.set(*found);
                        }
                    },
                }
            }
            div { class: "sep" }

            div { class: "form-row",
                label { "Emphasis" }
                {
                    let (bold, italic, underline) = (current.bold, current.italic, current.underline);
                    rsx! {
                        button {
                            class: if bold { "rbtn-icon on" } else { "rbtn-icon" }, title: "Bold",
                            onclick: move |_| amend(state, target(), |s: &mut TextStyle| s.bold = !bold),
                            {icon("bold", 15)}
                        }
                        button {
                            class: if italic { "rbtn-icon on" } else { "rbtn-icon" }, title: "Italic",
                            onclick: move |_| amend(state, target(), |s: &mut TextStyle| s.italic = !italic),
                            {icon("italic", 15)}
                        }
                        button {
                            class: if underline { "rbtn-icon on" } else { "rbtn-icon" }, title: "Underline",
                            onclick: move |_| amend(state, target(), |s: &mut TextStyle| s.underline = !underline),
                            {icon("underline", 15)}
                        }
                    }
                }
            }

            div { class: "form-row",
                label { "Font" }
                Dropdown {
                    value: current.family.clone(),
                    options: std::iter::once(Choice::new("", "Theme default"))
                        .chain(
                            ["Calibri", "Segoe UI", "Inter", "Arial", "Times New Roman"]
                                .iter()
                                .map(|family| Choice::plain(*family)),
                        )
                        .collect(),
                    width: 180.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        amend(state, target(), |s: &mut TextStyle| s.family = picked.clone());
                    },
                }
                label { style: "width: auto; margin-left: 12px;", "Size" }
                Dropdown {
                    value: if current.size_pt > 0.0 {
                        format!("{:.0}", current.size_pt)
                    } else {
                        "0".to_string()
                    },
                    options: std::iter::once(Choice::new("0", "Theme default"))
                        .chain(
                            ["8", "9", "10", "11", "12", "14", "16", "18"]
                                .iter()
                                .map(|size| Choice::plain(*size)),
                        )
                        .collect(),
                    width: 180.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        let value = picked.parse::<f32>().unwrap_or(0.0);
                        amend(state, target(), |s: &mut TextStyle| s.size_pt = value);
                    },
                }
            }

            div { class: "form-row",
                label { "Text colour" }
                Dropdown {
                    value: current.colour.clone(),
                    options: STYLE_COLOURS
                        .iter()
                        .map(|(name, token)| Choice::new(*token, *name))
                        .collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        amend(state, target(), |s: &mut TextStyle| s.colour = picked.clone());
                    },
                }
            }
            div { class: "form-row",
                label { "Fill" }
                Dropdown {
                    value: current.background.clone(),
                    options: STYLE_COLOURS
                        .iter()
                        .map(|(name, token)| Choice::new(*token, *name))
                        .collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        amend(state, target(), |s: &mut TextStyle| s.background = picked.clone());
                    },
                }
            }

            div { class: "sep" }
            div { class: "form-row",
                label { "Preview" }
                span { style: "{current.to_css()} padding: 3px 9px; border: 1px solid var(--grid-line);",
                    "Design the foundations" }
            }
            div { class: "hint",
                "A category's look sits under a row's own formatting, so a row you have coloured by hand keeps its colour. All applies first, then the category, so setting a font on All changes every row that has not been told otherwise."
            }
        }
        div { class: "dlg-foot",
            button { class: "btn",
                onclick: move |_| {
                    let chosen = target();
                    let mut writer = state.write();
                    writer.text_styles.clear(chosen);
                    writer.dirty = true;
                },
                "Reset this item"
            }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

// ---------------------------------------------------------- update project

#[component]
fn UpdateProjectDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let (today, has_selection) = {
        let s = state.read();
        (
            s.project.status_date.unwrap_or(s.project.start_date),
            !s.selection.is_empty(),
        )
    };

    // Two modes, one date field, exactly as Project puts it.
    let mut rescheduling = use_signal(|| false);
    let mut whole_only = use_signal(|| false);
    let mut selected_only = use_signal(|| false);
    let mut move_manual = use_signal(|| false);
    let mut when = use_signal(|| today.format("%Y-%m-%d").to_string());

    let apply = move |_| {
        let Some(date) = crate::state::parse_date(&when()) else {
            state.write().note("That date could not be read. Use YYYY-MM-DD.");
            return;
        };

        let mut options = if rescheduling() {
            let options = UpdateOptions::reschedule_after(date);
            if move_manual() { options.moving_manually_scheduled() } else { options }
        } else {
            let options = UpdateOptions::complete_through(date);
            if whole_only() { options.whole_tasks_only() } else { options }
        };
        if selected_only() {
            let rows = state.read().selection.clone();
            options = options.for_rows(rows);
        }
        state.write().update_project(options);
    };

    rsx! {
        Head { title: "Update Project".to_string() }
        div { class: "dlg-body", style: "min-width: 560px; min-height: 280px;",
            div { class: "form-row",
                label { "Action" }
                Dropdown {
                    value: (if rescheduling() { "reschedule" } else { "complete" }).to_string(),
                    options: vec![
                        Choice::new("complete", "Update work as complete through"),
                        Choice::new("reschedule", "Reschedule uncompleted work to start after"),
                    ],
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| rescheduling.set(picked == "reschedule"),
                }
            }
            div { class: "form-row",
                label { "Date" }
                input { placeholder: "YYYY-MM-DD", value: "{when}",
                    oninput: move |e| when.set(e.value()) }
            }
            div { class: "sep" }

            if rescheduling() {
                OptCheck {
                    label: "Move manually scheduled tasks too".to_string(),
                    on_state: move_manual(),
                    on: move |_| move_manual.toggle(),
                }
                div { class: "hint",
                    "Work that has not started moves to begin after the date. A task already part done keeps the work it has done, and the rest picks up after the date. Manually scheduled tasks stay where they were put unless the box above is ticked."
                }
            } else {
                OptCheck {
                    label: "Set 0% or 100% complete only".to_string(),
                    on_state: whole_only(),
                    on: move |_| whole_only.toggle(),
                }
                div { class: "hint",
                    if whole_only() {
                        "Only tasks that finished on or before the date are marked complete. Everything else is left exactly as it is."
                    } else {
                        "Each task gets the share of its working time that falls before the date, so a task halfway through reads about 50%. This assumes the plan was followed, and will overwrite progress already reported."
                    }
                }
            }

            div { class: "sep" }
            OptCheck {
                label: "Selected tasks only".to_string(),
                on_state: selected_only(),
                on: move |_| selected_only.toggle(),
            }
            if selected_only() && !has_selection {
                div { class: "hint", "Nothing is selected, so this would update nothing." }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button { class: "btn primary", onclick: apply, "OK" }
        }
    }
}

// -------------------------------------------------- links between projects

#[component]
fn LinksBetweenProjects() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // Two kinds of reach outside this plan: work waiting on another system,
    // and rows that came in from another plan.
    let (waiting, inserted) = {
        let s = state.read();
        let project = &s.project;
        let mut waiting: Vec<(String, String, String, String)> = Vec::new();
        for task in &project.tasks {
            for id in &task.external_predecessors {
                if let Some(entry) = project.external.iter().find(|e| e.id == *id) {
                    waiting.push((
                        task.name.clone(),
                        entry.reference.clone(),
                        entry.label.clone(),
                        entry.available.format("%Y-%m-%d").to_string(),
                    ));
                }
            }
        }

        let inserted: Vec<(String, String)> = project
            .tasks
            .iter()
            .enumerate()
            .filter(|(index, _)| project.is_summary(*index))
            .filter_map(|(_, task)| {
                // The subproject reader leaves this note on the summary row,
                // which is the only record that the rows came from elsewhere.
                task.notes
                    .lines()
                    .find_map(|line| line.strip_prefix("Inserted from "))
                    .map(|rest| (task.name.clone(), rest.trim_end_matches('.').to_string()))
            })
            .collect();

        (waiting, inserted)
    };

    rsx! {
        Head { title: "Links Between Projects".to_string() }
        div { class: "dlg-body", style: "min-width: 640px; max-height: 62vh; overflow-y: auto;",
            div { class: "hint", style: "margin: 0 0 12px;",
                "Everything in this plan that reaches outside it. Nothing here is checked against the other side, so a date shown is the one recorded here, not one read back from another system."
            }

            h3 { class: "dlg-sub", "Waiting on something outside the plan" }
            if waiting.is_empty() {
                p { class: "hint", "Nothing in this plan waits on an outside dependency." }
            } else {
                table { class: "assign-table",
                    thead {
                        tr {
                            th { "Task" }
                            th { "Reference" }
                            th { "What it is" }
                            th { "Expected" }
                        }
                    }
                    tbody {
                        for (task, reference, label, when) in waiting.iter() {
                            tr { key: "{task}{reference}",
                                td { "{task}" }
                                td { "{reference}" }
                                td { "{label}" }
                                td { "{when}" }
                            }
                        }
                    }
                }
            }

            div { class: "sep" }

            h3 { class: "dlg-sub", "Rows brought in from another plan" }
            if inserted.is_empty() {
                p { class: "hint", "No subprojects have been inserted." }
            } else {
                table { class: "assign-table",
                    thead {
                        tr {
                            th { "Summary row" }
                            th { "Came from" }
                        }
                    }
                    tbody {
                        for (name, source) in inserted.iter() {
                            tr { key: "{name}",
                                td { "{name}" }
                                td { "{source}" }
                            }
                        }
                    }
                }
                p { class: "hint",
                    "Inserted rows are a copy taken at the time. Changes made to the other file afterwards do not appear here."
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = Some(Dialog::ExternalDependencies),
                "External Dependencies" }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

// ------------------------------------------------------------- change log

/// Everything that has been done to this plan, newest first.
///
/// The command each entry holds is shown beside the sentence about it, because
/// the command is the part that replays and the part a planner can check. A
/// summary on its own would be a description of the plan's history rather than
/// the history itself.
#[component]
fn HistoryDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // How many entries the panel will show at once. A long session runs to
    // thousands, and a list nobody can scroll to the end of is not a list.
    const SHOWN: usize = 250;

    let (entries, kept, unsent) = {
        let s = state.read();
        let log = &s.project.history;
        let entries: Vec<(u64, String, String, String, String, usize)> = log
            .recent(SHOWN)
            .map(|change| {
                (
                    change.id,
                    change.at.format("%Y-%m-%d %H:%M").to_string(),
                    change.author.clone(),
                    change.summary.clone(),
                    change.first_line().to_string(),
                    change.command_count(),
                )
            })
            .collect();
        (entries, log.len(), log.unsent().len())
    };

    rsx! {
        Head { title: "Change Log".to_string() }
        div { class: "dlg-body", style: "min-width: 760px; max-height: 62vh; overflow-y: auto;",
            div { class: "hint", style: "margin: 0 0 12px;",
                "Every edit made to this plan, newest first, kept as the command that made it. The command is what a replay runs and what the panel shows, so what is stored and what is read are the same thing."
            }

            div { class: "hist-tally",
                span { "{kept} change(s) recorded" }
                span { class: "hist-unsent",
                    "{unsent} not yet copied to a server"
                }
            }
            p { class: "hint", style: "margin-top: 4px;",
                "Nothing sends them anywhere yet. The count is here because it is the work a shared plan would have to catch up on."
            }

            if entries.is_empty() {
                p { class: "hint", "Nothing has been changed in this plan yet." }
            } else {
                table { class: "assign-table", style: "margin-top: 12px;",
                    thead {
                        tr {
                            th { style: "width: 128px;", "When" }
                            th { style: "width: 132px;", "Who" }
                            th { "What it did" }
                            th { style: "width: 250px;", "Command" }
                        }
                    }
                    tbody {
                        for (id, when, who, what, command, count) in entries.iter() {
                            tr { key: "{id}",
                                td { class: "hist-when", "{when}" }
                                td { "{who}" }
                                td { "{what}" }
                                td { class: "hist-cmd",
                                    "{command}"
                                    if *count > 1 {
                                        // One entry, several commands: a
                                        // grouped step was one thing the
                                        // planner did.
                                        span { class: "hist-more", " and {count - 1} more" }
                                    }
                                }
                            }
                        }
                    }
                }
                if kept > entries.len() {
                    p { class: "hint",
                        "Showing the most recent {entries.len()} of {kept}."
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

// ------------------------------------------------------ insert subproject

#[component]
fn InsertSubproject() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let start_in = {
        let s = state.read();
        s.file_path
            .as_ref()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(crate::state::documents_dir)
    };
    let mut dir = use_signal(|| start_in);

    // Folders, then anything the plan reader can open. Same rules the Open
    // page uses, so a file visible there is visible here.
    let (folders, files) = {
        let mut folders: Vec<(String, PathBuf)> = Vec::new();
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    folders.push((name, path));
                } else if crate::state::offered_in_browser(&path, false) {
                    files.push((name, path));
                }
            }
        }
        folders.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        (folders, files)
    };

    let recent = state.read().recent.clone();
    let current = dir().display().to_string();

    rsx! {
        Head { title: "Insert Subproject".to_string() }
        div { class: "dlg-body", style: "min-height: 340px;",
            p { class: "hint", style: "margin-top: 0;",
                "The plan you pick comes in as a summary row with its tasks beneath it, at the row selected here. It is copied in, not linked, so saving this plan never writes to the other file."
            }
            div { class: "bs-field",
                label { "Folder" }
                input {
                    class: "bs-input",
                    value: "{current}",
                    onchange: move |event| {
                        let candidate = PathBuf::from(event.value());
                        if candidate.is_dir() { dir.set(candidate); }
                    },
                }
                button { class: "btn",
                    onclick: move |_| {
                        let parent = dir().parent().map(PathBuf::from);
                        if let Some(parent) = parent { dir.set(parent); }
                    },
                    "Up"
                }
                // Empty on every platform with a single root. On Windows the
                // parent of `C:\` is nothing, so without these a workbook on
                // another drive can never be navigated to.
                for (label, root) in crate::state::browser_roots() {
                    button { class: "btn",
                        onclick: move |_| dir.set(root.clone()),
                        "{label}"
                    }
                }
            }

            if !recent.is_empty() {
                div { class: "recent-list", style: "max-height: 90px; overflow-y: auto;",
                    for entry in recent.iter().take(5) {
                        {
                            let target = entry.path.clone();
                            let label = entry.name.clone();
                            rsx! {
                                button { key: "r{label}", class: "recent-row",
                                    onclick: move |_| state.write().insert_subproject(target.clone()),
                                    span { class: "glyph", {icon("subproject", 20)} }
                                    div { class: "recent-name", "{label}" }
                                }
                            }
                        }
                    }
                }
                div { class: "sep" }
            }

            div { class: "recent-list", style: "max-height: 190px; overflow-y: auto;",
                for (name, path) in folders {
                    {
                        let target = path.clone();
                        rsx! {
                            button { key: "d{name}", class: "recent-row",
                                onclick: move |_| dir.set(target.clone()),
                                span { class: "glyph", {icon("folder", 20)} }
                                div { class: "recent-name", "{name}" }
                            }
                        }
                    }
                }
                for (name, path) in files {
                    {
                        let target = path.clone();
                        rsx! {
                            button { key: "f{name}", class: "recent-row",
                                onclick: move |_| state.write().insert_subproject(target.clone()),
                                span { class: "glyph", {icon("subproject", 20)} }
                                div { class: "recent-name", "{name}" }
                            }
                        }
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
        }
    }
}

// ------------------------------------------------------- leveling options

#[component]
fn LevelingOptionsDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let current = state.read().leveling.clone();

    let mut only_within_slack = use_signal(|| current.only_within_slack);
    let mut level_manual = use_signal(|| current.level_manual);
    let mut order = use_signal(|| current.order);

    let apply = move |_| {
        let (slack, manual, chosen) = (only_within_slack(), level_manual(), order());
        let mut writer = state.write();
        writer.leveling.only_within_slack = slack;
        writer.leveling.level_manual = manual;
        writer.leveling.order = chosen;
        writer.dialog = None;
        writer.note("Levelling options saved.");
    };

    let level_now = move |_| {
        let (slack, manual, chosen) = (only_within_slack(), level_manual(), order());
        {
            let mut writer = state.write();
            writer.leveling.only_within_slack = slack;
            writer.leveling.level_manual = manual;
            writer.leveling.order = chosen;
            writer.dialog = None;
        }
        state.write().level(LevelScope::EntireProject);
    };

    rsx! {
        Head { title: "Resource Leveling".to_string() }
        div { class: "dlg-body", style: "min-height: 230px;",
            div { class: "form-row",
                label { "Order" }
                Dropdown {
                    value: match order() {
                        LevelOrder::IdOnly => "id",
                        LevelOrder::PriorityFirst => "priority",
                        LevelOrder::Standard => "standard",
                    }.to_string(),
                    options: vec![
                        Choice::new("standard", "Standard"),
                        Choice::new("id", "ID only"),
                        Choice::new("priority", "Priority, then standard"),
                    ],
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| order.set(match picked.as_str() {
                        "id" => LevelOrder::IdOnly,
                        "priority" => LevelOrder::PriorityFirst,
                        _ => LevelOrder::Standard,
                    }),
                }
            }
            div { class: "hint",
                "Standard moves the task with the most slack, so the tasks holding up the finish stay put. ID only moves whichever comes later in the plan. Priority looks first at whether the task has a deadline or a date the planner pinned."
            }
            div { class: "sep" }
            OptCheck {
                label: "Level only within available slack".to_string(),
                on_state: only_within_slack(),
                on: move |_| only_within_slack.toggle(),
            }
            OptCheck {
                label: "Level manually scheduled tasks".to_string(),
                on_state: level_manual(),
                on: move |_| level_manual.toggle(),
            }
            div { class: "hint",
                "Levelling within slack never pushes the finish date out, so it will leave some overallocations alone rather than break the plan. Manually scheduled tasks are left where the planner put them unless the second box is ticked."
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button { class: "btn", onclick: apply, "OK" }
            button { class: "btn primary", onclick: level_now, "Level All" }
        }
    }
}

// ---------------------------------------------------- resource information

#[component]
fn ResourceInformation(row: usize, tab: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut tab = use_signal(|| tab.min(2));

    let snapshot = {
        let s = state.read();
        s.project.resources.get(row).cloned()
    };
    let Some(resource) = snapshot else {
        return rsx! { MessageBox { title: "Resource Information".to_string(), body: "That resource no longer exists.".to_string() } };
    };

    let mut name = use_signal(|| resource.name.clone());
    let mut initials = use_signal(|| resource.initials.clone());
    let mut email = use_signal(|| resource.email.clone());
    let mut group = use_signal(|| resource.group.clone());
    let mut code = use_signal(|| resource.code.clone());
    let mut kind = use_signal(|| resource.kind);
    let mut max_units = use_signal(|| format!("{:.0}%", resource.max_units * 100.0));
    let mut standard = use_signal(|| format!("{:.2}", resource.standard_rate));
    let mut overtime = use_signal(|| format!("{:.2}", resource.overtime_rate));
    let mut per_use = use_signal(|| format!("{:.2}", resource.cost_per_use));
    let mut notes = use_signal(|| resource.notes.clone());

    // What this person is already carrying, so the dialog is not just a form.
    let (currency, workload) = {
        let s = state.read();
        let project = &s.project;
        let mut minutes = 0i64;
        let mut cost = 0.0f64;
        let mut tasks: Vec<(String, f64)> = Vec::new();
        for task in &project.tasks {
            for assignment in &task.assignments {
                if assignment.resource != resource.id {
                    continue;
                }
                let share = (task.scheduled.work_minutes as f64 * assignment.units).round() as i64;
                minutes += share;
                cost += share as f64 / 60.0 * resource.standard_rate;
                tasks.push((task.name.clone(), assignment.units));
            }
        }
        (project.currency_symbol.clone(), (minutes, cost, tasks))
    };
    let (assigned_minutes, assigned_cost, assigned_tasks) = workload;

    let apply = move |_| {
        let (new_name, new_initials, new_email) = (name(), initials(), email());
        let (new_group, new_code, new_kind) = (group(), code(), kind());
        let (new_units, new_standard) = (max_units(), standard());
        let (new_overtime, new_per_use, new_notes) = (overtime(), per_use(), notes());

        let mut writer = state.write();
        writer.checkpoint();
        if let Some(target) = writer.project.resources.get_mut(row) {
            target.name = new_name;
            target.initials = new_initials;
            target.email = new_email;
            target.group = new_group;
            target.code = new_code;
            target.kind = new_kind;
            if let Ok(units) = new_units.trim_end_matches('%').trim().parse::<f64>() {
                // Typed as a percentage because that is how the sheet shows it.
                target.max_units = (units / 100.0).max(0.0);
            }
            if let Ok(rate) = money(&new_standard) {
                target.standard_rate = rate;
            }
            if let Ok(rate) = money(&new_overtime) {
                target.overtime_rate = rate;
            }
            if let Ok(rate) = money(&new_per_use) {
                target.cost_per_use = rate;
            }
            target.notes = new_notes;
        }
        writer.reschedule();
        writer.dialog = None;
    };

    rsx! {
        Head { title: "Resource Information".to_string() }
        div { class: "dlg-tabs",
            for (index, label) in ["General", "Costs", "Notes"].iter().enumerate() {
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
                        label { style: "width: auto; margin-left: 12px;", "Initials" }
                        input { style: "width: 70px;", value: "{initials}",
                            oninput: move |e| initials.set(e.value()) }
                    }
                    div { class: "form-row",
                        label { "Email" }
                        input { class: "grow", value: "{email}", oninput: move |e| email.set(e.value()) }
                    }
                    div { class: "form-row",
                        label { "Group" }
                        input { value: "{group}", oninput: move |e| group.set(e.value()) }
                        label { style: "width: auto; margin-left: 12px;", "Code" }
                        input { style: "width: 110px;", value: "{code}",
                            oninput: move |e| code.set(e.value()) }
                    }
                    div { class: "form-row",
                        label { "Type" }
                        Dropdown {
                            value: match kind() {
                                ResourceKind::Work => "work",
                                ResourceKind::Material => "material",
                                ResourceKind::Cost => "cost",
                            }.to_string(),
                            options: vec![
                                Choice::new("work", "Work"),
                                Choice::new("material", "Material"),
                                Choice::new("cost", "Cost"),
                            ],
                            width: 0.0, large: true, disabled: false,
                            on_pick: move |picked: String| kind.set(match picked.as_str() {
                                "material" => ResourceKind::Material,
                                "cost" => ResourceKind::Cost,
                                _ => ResourceKind::Work,
                            }),
                        }
                        label { style: "width: auto; margin-left: 12px;", "Max units" }
                        input { style: "width: 80px;", value: "{max_units}",
                            disabled: kind() != ResourceKind::Work,
                            oninput: move |e| max_units.set(e.value()) }
                    }
                    p { class: "hint",
                        "Max units is how much of this person the plan may book at once. 100% is one full-time unit, 50% is half time, 300% is a crew of three."
                    }
                },

                // ---- Costs ----------------------------------------------
                1 => rsx! {
                    div { class: "form-row",
                        label { "Standard rate" }
                        input { value: "{standard}", oninput: move |e| standard.set(e.value()) }
                        span { class: "unit", "{currency} per hour" }
                    }
                    div { class: "form-row",
                        label { "Overtime rate" }
                        input { value: "{overtime}", oninput: move |e| overtime.set(e.value()) }
                        span { class: "unit", "{currency} per hour" }
                    }
                    div { class: "form-row",
                        label { "Cost per use" }
                        input { value: "{per_use}", oninput: move |e| per_use.set(e.value()) }
                        span { class: "unit", "{currency} each time booked" }
                    }
                    div { class: "sep" }
                    div { class: "form-row",
                        label { "Assigned work" }
                        span { "{format_duration(assigned_minutes)}" }
                        label { style: "width: auto; margin-left: 12px;", "Cost so far" }
                        span { "{currency}{assigned_cost:.2}" }
                    }
                    p { class: "hint",
                        "Cost so far is this person's share of the work already booked, at the standard rate. It updates when the rate above is saved."
                    }
                    if assigned_tasks.is_empty() {
                        p { class: "hint", "Not assigned to anything yet." }
                    } else {
                        div { class: "dlg-list",
                            for (task_name, units) in assigned_tasks.iter().take(12) {
                                div { class: "dlg-list-row", key: "{task_name}",
                                    span { class: "grow", "{task_name}" }
                                    span { "{units * 100.0:.0}%" }
                                }
                            }
                        }
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

/// Read a rate the planner typed, tolerating a currency symbol and separators.
fn money(text: &str) -> Result<f64, std::num::ParseFloatError> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return Ok(0.0);
    }
    cleaned.parse::<f64>()
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

/// Decode the value the target picker carries.
///
/// Encoded as a string so it can ride through the same `Dropdown` every other
/// picker in this file uses. The three cases are deliberately different things,
/// which is why `CalendarTarget` names them in the core rather than here: a day
/// the organisation is closed belongs on a shared calendar, and one person
/// being away belongs on that person.
fn calendar_target_of(value: &str) -> CalendarTarget {
    if let Some(name) = value.strip_prefix("base:") {
        return CalendarTarget::Base(name.to_string());
    }
    if let Some(id) = value.strip_prefix("resource:")
        && let Ok(id) = id.parse::<u32>()
    {
        return CalendarTarget::Resource(id);
    }
    CalendarTarget::Project
}

/// Everything a calendar can be pointed at, in the order a picker offers it.
///
/// Built once and used by both the picker at the top of the dialog and the
/// destination picker in the import step, so the two cannot come to describe
/// the same calendar differently.
fn calendar_targets(project: &aop_core::Project) -> Vec<Choice> {
    let mut targets = vec![Choice::new(
        "project",
        format!("Project calendar ({})", project.calendar.name),
    )];
    for calendar in project.calendars.iter() {
        targets.push(Choice::new(
            format!("base:{}", calendar.name),
            calendar.name.clone(),
        ));
    }
    for resource in project.resources.iter().filter(|r| r.kind == ResourceKind::Work) {
        targets.push(Choice::new(
            format!("resource:{}", resource.id),
            format!("{} (person)", resource.name),
        ));
    }
    targets
}

/// What the dialog is showing, pulled out of the plan in one read.
struct CalendarView {
    /// Whether each weekday is worked, and the hours, from the calendar in
    /// force for the target.
    week: Vec<(bool, String)>,
    exceptions: Vec<(String, NaiveDate, NaiveDate)>,
    /// Set for a person: the week is their base's and is not theirs to edit
    /// here, so it is shown rather than offered.
    base_of_resource: Option<String>,
}

fn read_calendar(state: &AppState, target: &CalendarTarget) -> CalendarView {
    let project = &state.project;
    let (calendar, base_of_resource) = match target {
        CalendarTarget::Project => (&project.calendar, None),
        CalendarTarget::Base(name) => (project.calendar_or_project(name), None),
        CalendarTarget::Resource(id) => {
            let base = project
                .resource(*id)
                .map(|r| r.base_calendar.clone())
                .unwrap_or_default();
            let calendar = project.calendar_or_project(&base);
            (calendar, Some(calendar.name.clone()))
        }
    };

    let week = calendar
        .week
        .iter()
        .map(|day| {
            let hours = day
                .shifts
                .iter()
                .map(|shift| format!("{} - {}", shift.start.format("%H:%M"), shift.end.format("%H:%M")))
                .collect::<Vec<_>>()
                .join(", ");
            (
                day.is_working(),
                if hours.is_empty() { "Nonworking".to_string() } else { hours },
            )
        })
        .collect();

    CalendarView {
        week,
        exceptions: project
            .exceptions_for(target)
            .iter()
            .map(|ex| (ex.name.clone(), ex.from, ex.to))
            .collect(),
        base_of_resource,
    }
}

#[component]
fn ChangeWorkingTime() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut holiday_name = use_signal(|| String::from("Holiday"));
    let mut holiday_from = use_signal(String::new);
    let mut holiday_to = use_signal(String::new);
    let mut target_value = use_signal(|| String::from("project"));

    // ---- the import step ------------------------------------------------
    //
    // Its destination is a signal of its own rather than a read of the picker
    // above. It starts on whatever calendar is being edited, which is nearly
    // always the one meant, but dropping a national holiday file onto one
    // person is a real mistake to make and a silent one afterwards: the plan
    // simply schedules everybody else through Christmas. So the destination is
    // named and changeable at the moment of import, and the confirm step says
    // where the days are going before anything is added.
    let mut importing = use_signal(|| false);
    let mut import_into = use_signal(|| String::from("project"));
    let mut source = use_signal(|| Option::<PathBuf>::None);
    let mut found = use_signal(|| Option::<holidays::Found>::None);
    let mut trouble = use_signal(|| Option::<String>::None);

    // A plan spans a year or two and a downloaded calendar spans ten, so the
    // range starts on the plan rather than on the file.
    let planned = {
        let s = state.read();
        let start = s.project.start_date.year();
        let finish = s
            .report
            .as_ref()
            .map(|report| report.finish.year())
            .unwrap_or(start);
        (start, finish.max(start))
    };
    let mut first_year = use_signal(|| planned.0);
    let mut last_year = use_signal(|| planned.1);

    let target = calendar_target_of(&target_value());
    let into = calendar_target_of(&import_into());
    let editable_week = !target.is_person();

    let (view, targets, bases, into_name, into_base) = {
        let s = state.read();
        let view = read_calendar(&s, &target);
        let targets = calendar_targets(&s.project);
        let bases: Vec<Choice> = s
            .project
            .calendar_library()
            .map(|calendar| Choice::plain(calendar.name.clone()))
            .collect();
        let into_name = s.project.calendar_target_name(&into);
        // Only wanted for a person, to say plainly that their shared week is
        // not what an import touches.
        let into_base = match &into {
            CalendarTarget::Resource(id) => s
                .project
                .resource(*id)
                .map(|r| s.project.calendar_or_project(&r.base_calendar).name.clone()),
            _ => None,
        };
        (view, targets, bases, into_name, into_base)
    };

    let target_for_week = target_value();
    let target_for_remove = target_value();
    let target_for_add = target_value();
    let target_for_base = target_value();

    let mut choose = move |path: PathBuf| {
        trouble.set(None);
        match holidays::read(&path) {
            Ok(read) => {
                found.set(Some(read));
                source.set(Some(path));
            }
            Err(error) => {
                found.set(None);
                source.set(None);
                trouble.set(Some(error.to_string()));
            }
        }
    };

    // Closing the panel throws the file away too, so reopening it starts from
    // the picker rather than from somebody else's half-finished import.
    let mut close_import = move || {
        importing.set(false);
        found.set(None);
        source.set(None);
        trouble.set(None);
    };

    let file = found();
    let import_destination = rsx! {
        div { class: "form-row",
            label { "Add days off to" }
            Dropdown {
                value: "{import_into}",
                options: targets.clone(),
                width: 0.0,
                large: true,
                disabled: false,
                on_pick: move |picked: String| import_into.set(picked),
            }
        }
    };

    rsx! {
        Head { title: "Change Working Time".to_string() }
        div { class: "dlg-body",
            div { class: "form-row",
                label { "For calendar" }
                Dropdown {
                    value: "{target_value}",
                    options: targets.clone(),
                    width: 0.0,
                    large: true,
                    disabled: false,
                    on_pick: move |picked: String| target_value.set(picked),
                }
                button { class: "btn",
                    onclick: move |_| {
                        let mut writer = state.write();
                        writer.checkpoint();
                        let name = writer.project.add_base_calendar(aop_core::WorkCalendar::standard());
                        target_value.set(format!("base:{name}"));
                    },
                    "New calendar"
                }
            }

            if let Some(base) = view.base_of_resource.clone() {
                div { class: "form-row",
                    label { "Follows" }
                    Dropdown {
                        value: "{base}",
                        options: bases,
                        width: 0.0,
                        large: true,
                        disabled: false,
                        on_pick: move |picked: String| {
                            let CalendarTarget::Resource(id) = calendar_target_of(&target_for_base) else { return };
                            let mut writer = state.write();
                            writer.checkpoint();
                            if let Some(resource) = writer.project.resources.iter_mut().find(|r| r.id == id) {
                                resource.base_calendar = picked;
                            }
                            writer.reschedule();
                        },
                    }
                }
                div { class: "hint",
                    "The working week below is this calendar's and is shared. \
                     Time off for this person alone goes in Exceptions."
                }
            }

            h3 { style: "font-size: 13px; margin: 14px 0 8px;", "Working week" }
            for (index, day) in WEEKDAYS.iter().enumerate() {
                {
                    let (working, hours) = view
                        .week
                        .get(index)
                        .cloned()
                        .unwrap_or((false, "Nonworking".to_string()));
                    let box_class = if working { "box on" } else { "box" };
                    let row_class = if editable_week { "rcheck" } else { "rcheck disabled" };
                    let target_for_day = target_for_week.clone();
                    rsx! {
                        div { key: "{day}", class: "{row_class}", style: "height: 24px;",
                            onclick: move |_| {
                                if !editable_week {
                                    return;
                                }
                                let target = calendar_target_of(&target_for_day);
                                let mut writer = state.write();
                                writer.checkpoint();
                                let calendar = match &target {
                                    CalendarTarget::Base(name) => writer.project.calendar_named_mut(name),
                                    _ => Some(&mut writer.project.calendar),
                                };
                                if let Some(calendar) = calendar
                                    && let Some(slot) = calendar.week.get_mut(index) {
                                        *slot = if working { DayShifts::nonworking() } else { DayShifts::standard() };
                                    }
                                writer.reschedule();
                            },
                            span { class: "{box_class}", if working { "\u{2713}" } }
                            span { style: "width: 100px;", "{day}" }
                            span { style: "color: var(--ink-soft);", "{hours}" }
                        }
                    }
                }
            }

            h3 { style: "font-size: 13px; margin: 18px 0 8px;", "Exceptions" }
            if view.exceptions.is_empty() {
                div { class: "hint",
                    if view.base_of_resource.is_some() {
                        "No leave or other time off has been recorded for this person."
                    } else {
                        "No holidays or shutdowns have been added."
                    }
                }
            } else {
                table { class: "assign-table",
                    thead { tr { th { "Name" } th { "From" } th { "To" } th { style: "width: 60px;", "" } } }
                    tbody {
                        for (index, (name, from, to)) in view.exceptions.iter().enumerate() {
                            tr { key: "ex{index}",
                                td { "{name}" }
                                td { "{from}" }
                                td { "{to}" }
                                td {
                                    {
                                        let target_for_row = target_for_remove.clone();
                                        rsx! {
                                            button { class: "btn", style: "padding: 1px 8px;",
                                                onclick: move |_| {
                                                    let target = calendar_target_of(&target_for_row);
                                                    let mut writer = state.write();
                                                    writer.checkpoint();
                                                    if let Some(list) = writer.project.exceptions_for_mut(&target)
                                                        && index < list.len() {
                                                            list.remove(index);
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
                }
            }

            div { class: "form-row", style: "margin-top: 12px;",
                label { "Add exception" }
                input { style: "flex: 1;", placeholder: "Name", value: "{holiday_name}",
                    oninput: move |e| holiday_name.set(e.value()) }
                input { style: "width: 120px;", placeholder: "From YYYY-MM-DD", value: "{holiday_from}",
                    oninput: move |e| holiday_from.set(e.value()) }
                input { style: "width: 120px;", placeholder: "To (optional)", value: "{holiday_to}",
                    oninput: move |e| holiday_to.set(e.value()) }
                button { class: "btn",
                    onclick: move |_| {
                        let Some(from) = parse_date(&holiday_from()) else { return };
                        // A single day is the common case, so an empty "to"
                        // means the same day rather than an error.
                        let to = parse_date(&holiday_to()).unwrap_or(from);
                        let name = holiday_name();
                        let entry = aop_core::CalendarException {
                            name: if name.trim().is_empty() { "Holiday".into() } else { name },
                            from: from.date(),
                            to: to.date().max(from.date()),
                            shifts: DayShifts::nonworking(),
                        };
                        let target = calendar_target_of(&target_for_add);
                        let mut writer = state.write();
                        writer.checkpoint();
                        if let Some(list) = writer.project.exceptions_for_mut(&target) {
                            list.push(entry);
                        }
                        writer.reschedule();
                        holiday_from.set(String::new());
                        holiday_to.set(String::new());
                    },
                    "Add"
                }
            }

            // ---- import from a calendar file ---------------------------
            if !importing() {
                div { class: "form-row", style: "margin-top: 10px;",
                    label { "" }
                    button { class: "btn",
                        onclick: move |_| {
                            // Default the destination to whatever is being
                            // edited, which is the reason this control is here
                            // rather than on a page of its own.
                            import_into.set(target_value());
                            importing.set(true);
                        },
                        {icon("open", 15)}
                        if target.is_person() { "Import time off from a file" } else { "Import holidays from a file" }
                    }
                    span { class: "hint", style: "margin: 0;",
                        if target.is_person() {
                            "An .ics export from their calendar application."
                        } else {
                            "An .ics holiday calendar."
                        }
                    }
                }
            } else {
                h3 { style: "font-size: 13px; margin: 18px 0 8px;",
                    if into.is_person() { "Import time off" } else { "Import public holidays" }
                }

                {import_destination}

                if let Some(base) = into_base.clone() {
                    div { class: "hint",
                        "Days land on {into_name} alone. The {base} calendar they follow is not changed, \
                         so nobody else is given the time off."
                    }
                } else {
                    div { class: "hint",
                        "Days land on the {into_name} calendar, so they are non-working for everybody who follows it."
                    }
                }

                if let Some(message) = trouble() {
                    div { class: "info-alert",
                        span { class: "fix-icon", {icon("warning", 18)} }
                        div { style: "flex: 1;", "{message}" }
                    }
                }

                match file.as_ref() {
                    None => rsx! {
                        div { class: "hint", style: "margin-top: 8px;",
                            if into.is_person() {
                                "An iCalendar (.ics) file, the export every calendar application can produce. \
                                 Somebody can send you theirs and their absences come across as their own time off, \
                                 without any of it being typed in."
                            } else {
                                "An iCalendar (.ics) file, which is what governments, Google and Outlook publish \
                                 holiday calendars as. Whole days in it become non-working days, so nothing is \
                                 scheduled on them."
                            }
                        }
                        crate::backstage::FileBrowser {
                            saving: false,
                            accept: vec!["ics".to_string()],
                            on_pick: move |path: PathBuf| choose(path),
                        }
                        div { class: "form-row",
                            label { "" }
                            button { class: "btn", onclick: move |_| close_import(), "Cancel" }
                        }
                    },
                    Some(file) => {
                        let list = file.between(first_year(), last_year());
                        let unhandled = file.unhandled();
                        let known: Vec<bool> = {
                            let s = state.read();
                            // Checked against the destination chosen here, not
                            // against the calendar being edited, so the count
                            // below is a count of what will really be added.
                            let carrier = crate::state::target_calendar(&s.project, &into);
                            list.iter()
                                .map(|holiday| holidays::already_held(&carrier, holiday))
                                .collect()
                        };
                        let fresh = known.iter().filter(|held| !**held).count();
                        let name = file.name.clone().unwrap_or_else(|| "Holidays".to_string());
                        let path = source().map(|p| p.display().to_string()).unwrap_or_default();
                        // The sentence that has to be read before anything is
                        // added, in the same words the picker above uses.
                        let going = if into.is_person() {
                            format!(
                                "{fresh} day(s) off will be added to {into_name}, \
                                 moving only the tasks they are assigned to."
                            )
                        } else {
                            format!(
                                "{fresh} day(s) off will be added to the {into_name} calendar, \
                                 and will apply to everybody who follows it."
                            )
                        };

                        rsx! {
                            div { class: "form-row",
                                label { "File" }
                                input { class: "grow", value: "{path}", readonly: true }
                                button { class: "btn",
                                    onclick: move |_| {
                                        found.set(None);
                                        source.set(None);
                                    },
                                    "Choose another"
                                }
                            }
                            div { class: "form-row",
                                label { "Contains" }
                                span { style: "color: var(--ink-soft);",
                                    "{name} \u{00b7} {file.occasions.len()} event(s)"
                                }
                            }
                            div { class: "form-row",
                                label { "Years" }
                                input {
                                    style: "width: 80px;",
                                    value: "{first_year}",
                                    onchange: move |event| {
                                        if let Ok(year) = event.value().trim().parse::<i32>() {
                                            first_year.set(year);
                                        }
                                    },
                                }
                                span { style: "color: var(--ink-soft);", "to" }
                                input {
                                    style: "width: 80px;",
                                    value: "{last_year}",
                                    onchange: move |event| {
                                        if let Ok(year) = event.value().trim().parse::<i32>() {
                                            last_year.set(year);
                                        }
                                    },
                                }
                                span { class: "hint", style: "margin: 0;",
                                    "A downloaded file often covers ten years and a plan covers one."
                                }
                            }

                            if file.timed > 0 {
                                div { class: "hint",
                                    "{file.timed} event(s) in this file have a time of day rather than being whole days. \
                                     Those are left alone: a day off is a whole day, and the rest are meetings somebody saved."
                                }
                            }
                            if !unhandled.is_empty() {
                                div { class: "info-alert",
                                    span { class: "fix-icon", {icon("warning", 18)} }
                                    div { style: "flex: 1;",
                                        "These repeat in a way this does not work out, so only the dates written in the file \
                                         come across: {unhandled.join(\", \")}. Yearly repeats are handled, including ones \
                                         like the fourth Thursday in November."
                                    }
                                }
                            }

                            if list.is_empty() {
                                div { class: "hint", "Nothing in this file falls between those years." }
                            } else {
                                div { class: "dlg-list",
                                    for (index, holiday) in list.iter().enumerate() {
                                        div { key: "h{index}", class: "dlg-list-row",
                                            span { style: "width: 170px; color: var(--ink-soft);",
                                                if holiday.from == holiday.to {
                                                    "{holiday.from}"
                                                } else {
                                                    "{holiday.from} to {holiday.to}"
                                                }
                                            }
                                            span { style: "flex: 1;", "{holiday.name}" }
                                            span { style: "color: var(--ink-faint);",
                                                if known.get(index).copied().unwrap_or(false) {
                                                    "already there"
                                                } else if holiday.repeating {
                                                    "from a yearly rule"
                                                } else {
                                                    ""
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            div { class: "info-alert", style: "margin-top: 12px;",
                                span { class: "fix-icon", {icon("warning", 18)} }
                                div { style: "flex: 1;", "{going} Undo puts it back." }
                            }

                            div { class: "form-row",
                                label { "" }
                                button { class: "btn primary",
                                    disabled: fresh == 0,
                                    onclick: move |_| {
                                        // Worked out again on the click rather
                                        // than reused from the list above, so
                                        // what is added cannot drift from what
                                        // was read.
                                        let into = calendar_target_of(&import_into());
                                        let days = found()
                                            .map(|file| file.between(first_year(), last_year()))
                                            .unwrap_or_default();
                                        state.write().import_holidays(&into, &days);
                                        close_import();
                                    },
                                    if into.is_person() {
                                        "Add {fresh} day(s) off for {into_name}"
                                    } else {
                                        "Add {fresh} day(s) off"
                                    }
                                }
                                button { class: "btn", onclick: move |_| close_import(), "Cancel" }
                            }
                        }
                    }
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
        Head { title: format!("Preview: {}", spec.name) }
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

// ---------------------------------------------------------- custom fields

/// Set up the plan's spare fields.
///
/// Laid out the way Project's is: pick a type, pick a slot, then say what it is
/// for. The slots are fixed rather than freely named so that a column, a filter
/// or an export written against `Text3` still finds `Text3` in a plan that has
/// been passed to someone else.
#[component]
fn CustomFieldsDialog() -> Element {
    use aop_core::custom::{CustomField, CustomKind, Indicator, LookupValue, Slot, Test};

    let mut state = use_context::<Signal<AppState>>();
    let mut kind = use_signal(|| CustomKind::Text);
    let mut number = use_signal(|| 1u8);

    let slot = Slot::new(kind(), number());
    let field = state
        .read()
        .project
        .custom_fields
        .get(&slot)
        .cloned()
        .unwrap_or_else(|| CustomField::new(slot));

    // Every write goes through here so the plan is only touched once per edit.
    let mut put = move |updated: CustomField| {
        let mut writer = state.write();
        writer.checkpoint();
        if updated.is_in_use() {
            writer.project.custom_fields.insert(slot, updated);
        } else {
            writer.project.custom_fields.remove(&slot);
        }
        writer.dirty = true;
    };

    let in_use: Vec<(Slot, String)> = state
        .read()
        .project
        .custom_fields
        .iter()
        .filter(|(_, field)| field.is_in_use())
        .map(|(slot, field)| (*slot, field.title()))
        .collect();

    rsx! {
        Head { title: "Custom Fields".to_string() }
        div { class: "dlg-body", style: "min-width: 640px; max-height: 62vh; overflow-y: auto;",

            div { class: "cf-pick",
                div { class: "bs-field",
                    label { "Type" }
                    Dropdown {
                        value: kind().label().to_string(),
                        options: CustomKind::ALL.iter().map(|k| Choice::plain(k.label())).collect(),
                        width: 0.0, large: true, disabled: false,
                        on_pick: move |value: String| {
                            if let Some(picked) = CustomKind::ALL.into_iter().find(|k| k.label() == value) {
                                kind.set(picked);
                                // Slot 12 does not exist for a type with ten.
                                if number() > picked.count() {
                                    number.set(1);
                                }
                            }
                        },
                    }
                }
                div { class: "bs-field",
                    label { "Field" }
                    Dropdown {
                        value: slot.default_title(),
                        options: (1..=kind().count())
                            .map(|n| Choice::plain(format!("{}{}", kind().label(), n)))
                            .collect(),
                        width: 0.0, large: true, disabled: false,
                        on_pick: move |value: String| {
                            let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
                            if let Ok(n) = digits.parse::<u8>() {
                                number.set(n);
                            }
                        },
                    }
                }
            }

            crate::backstage::Setting {
                label: "Name".to_string(),
                hint: "What the column is called. Leave empty to keep the slot name.".to_string(),
                input {
                    class: "bs-input",
                    value: "{field.title}",
                    placeholder: "{slot.default_title()}",
                    oninput: {
                        let field = field.clone();
                        move |event: FormEvent| {
                            let mut updated = field.clone();
                            updated.title = event.value();
                            put(updated);
                        }
                    },
                }
            }

            crate::backstage::Setting {
                label: "Summary rows".to_string(),
                hint: "What a summary row shows for this field.".to_string(),
                Dropdown {
                    value: field.rollup.label().to_string(),
                    options: kind().rollups().iter().map(|r| Choice::plain(r.label())).collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: {
                        let field = field.clone();
                        let allowed = kind().rollups();
                        move |value: String| {
                            if let Some(picked) = allowed.iter().find(|r| r.label() == value) {
                                let mut updated = field.clone();
                                updated.rollup = *picked;
                                put(updated);
                            }
                        }
                    },
                }
            }

            h2 { class: "bs-sub", "Values" }
            div { class: "hint", style: "margin: 0 0 8px;",
                "One value per line. Leave empty to accept anything typed." }
            textarea {
                class: "bs-input",
                style: "width: 100%; min-height: 90px; font-family: var(--mono); font-size: 11.5px;",
                value: "{field.lookup.iter().map(|v| v.value.clone()).collect::<Vec<_>>().join(\"\\n\")}",
                oninput: {
                    let field = field.clone();
                    move |event: FormEvent| {
                        let mut updated = field.clone();
                        updated.lookup = event
                            .value()
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(|line| LookupValue {
                                value: line.to_string(),
                                description: String::new(),
                            })
                            .collect();
                        put(updated);
                    }
                },
            }
            crate::backstage::OptCheck {
                label: "Only allow these values".to_string(),
                on_state: field.lookup_only,
                on: {
                    let field = field.clone();
                    move |_| {
                        let mut updated = field.clone();
                        updated.lookup_only = !updated.lookup_only;
                        put(updated);
                    }
                },
            }

            h2 { class: "bs-sub", "Indicators" }
            div { class: "hint", style: "margin: 0 0 8px;",
                "Show a mark instead of the value when it meets a test. The first match wins, so put a catch-all last." }

            for (position, indicator) in field.indicators.iter().enumerate() {
                {
                    let field = field.clone();
                    let indicator = indicator.clone();
                    rsx! {
                        div { key: "ind{position}", class: "cf-indicator",
                            span { class: "cf-glyph", "{indicator.glyph}" }
                            span { class: "cf-rule",
                                "{indicator.test.label()} {indicator.against}" }
                            if !indicator.meaning.is_empty() {
                                span { class: "cf-meaning", "{indicator.meaning}" }
                            }
                            button {
                                class: "key-clear",
                                onclick: move |_| {
                                    let mut updated = field.clone();
                                    updated.indicators.remove(position);
                                    put(updated);
                                },
                                {icon("x", 13)}
                            }
                        }
                    }
                }
            }

            {
                let field = field.clone();
                rsx! {
                    button {
                        class: "btn",
                        style: "margin-top: 8px;",
                        onclick: move |_| {
                            let mut updated = field.clone();
                            updated.indicators.push(Indicator {
                                test: Test::IsNotEmpty,
                                against: String::new(),
                                glyph: "\u{25cf}".into(),
                                meaning: "has a value".into(),
                            });
                            put(updated);
                        },
                        "Add an indicator"
                    }
                }
            }

            if !in_use.is_empty() {
                h2 { class: "bs-sub", "In use in this plan" }
                div { class: "cf-inuse",
                    for (used_slot, title) in in_use {
                        button {
                            key: "{used_slot.default_title()}",
                            class: "cf-chip",
                            onclick: move |_| {
                                kind.set(used_slot.kind);
                                number.set(used_slot.number);
                            },
                            span { class: "cf-chip-name", "{title}" }
                            span { class: "cf-chip-slot", "{used_slot.default_title()}" }
                        }
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Done" }
        }
    }
}

// ----------------------------------------------------- external dependencies

/// Things outside the plan that work waits on, and which tasks wait on them.
///
/// A reference and a date, nothing more. Nothing here goes looking for the
/// truth of it: the plan records what it was told, so it stays openable by
/// someone with no access to the system the reference came from.
#[component]
fn ExternalDependenciesDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let mut reference = use_signal(String::new);
    let mut label = use_signal(String::new);
    let mut when = use_signal(|| {
        state
            .read()
            .project
            .start_date
            .format("%Y-%m-%d")
            .to_string()
    });

    let (entries, row, task_name, waiting) = {
        let s = state.read();
        let row = s.primary();
        let waiting: Vec<u32> = row
            .and_then(|r| s.project.tasks.get(r))
            .map(|task| task.external_predecessors.clone())
            .unwrap_or_default();
        (
            s.project.external.clone(),
            row,
            row.and_then(|r| s.project.tasks.get(r))
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "No task selected".into()),
            waiting,
        )
    };

    let can_add = !reference().trim().is_empty()
        && crate::state::parse_date(&when()).is_some();

    rsx! {
        Head { title: "External Dependencies".to_string() }
        div { class: "dlg-body", style: "min-width: 640px; max-height: 62vh; overflow-y: auto;",
            div { class: "hint", style: "margin: 0 0 12px;",
                "Something outside this plan that work waits on: a purchase order, a permit, a delivery, a sign-off held in another system. Work cannot start before the date given, and the date is not checked against anything." }

            div { class: "ext-add",
                input {
                    class: "bs-input",
                    placeholder: "Reference, e.g. PO-4471",
                    value: "{reference}",
                    oninput: move |event| reference.set(event.value()),
                }
                input {
                    class: "bs-input",
                    placeholder: "What it is",
                    value: "{label}",
                    oninput: move |event| label.set(event.value()),
                }
                input {
                    class: "bs-input",
                    style: "max-width: 150px;",
                    placeholder: "YYYY-MM-DD",
                    value: "{when}",
                    oninput: move |event| when.set(event.value()),
                }
                button {
                    class: "btn primary",
                    disabled: !can_add,
                    onclick: move |_| {
                        if let Some(date) = crate::state::parse_date(&when()) {
                            state.write().add_external(&reference(), &label(), date);
                            reference.set(String::new());
                            label.set(String::new());
                        }
                    },
                    "Add"
                }
            }

            if entries.is_empty() {
                div { class: "empty-state", style: "height: 120px; font-size: 12px;",
                    "Nothing recorded yet." }
            } else {
                h2 { class: "bs-sub", "Recorded" }
                div { class: "ext-list",
                    for entry in entries.iter().cloned() {
                        {
                            let id = entry.id;
                            let held = waiting.contains(&id);
                            let users = state
                                .read()
                                .project
                                .tasks
                                .iter()
                                .filter(|task| task.external_predecessors.contains(&id))
                                .count();
                            rsx! {
                                div { key: "x{id}", class: "ext-row",
                                    div { class: "ext-main",
                                        span { class: "ext-ref", "{entry.reference}" }
                                        span { class: "ext-label", "{entry.label}" }
                                        input {
                                            class: "bs-input ext-date",
                                            value: "{entry.available.format(\"%Y-%m-%d\")}",
                                            title: "When it becomes available. Work waiting on it cannot start before this.",
                                            onchange: move |event| {
                                                if let Some(date) = crate::state::parse_date(&event.value()) {
                                                    state.write().update_external(id, |entry| entry.available = date);
                                                }
                                            },
                                        }
                                        span { class: "ext-users", "{users} task(s) waiting" }
                                    }
                                    div { class: "ext-acts",
                                        if row.is_some() {
                                            button {
                                                class: if held { "spell-skip" } else { "spell-fix" },
                                                onclick: move |_| {
                                                    if let Some(row) = row {
                                                        state.write().toggle_external_on(row, id);
                                                    }
                                                },
                                                if held { "Stop waiting" } else { "This task waits" }
                                            }
                                        }
                                        button {
                                            class: "key-clear",
                                            title: "Remove, and unhook every task waiting on it",
                                            onclick: move |_| state.write().remove_external(id),
                                            {icon("x", 13)}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                div { class: "hint", style: "margin-top: 10px;",
                    "Selected task: {task_name}" }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Done" }
        }
    }
}

// ------------------------------------------------------------ collaborating

/// One difference as a line, with the task named once by the group above it.
///
/// Written here rather than on `Difference` because the wording belongs to
/// this dialog: the same difference reads differently in a change log, where
/// it is something that was done, and in this question, where it is something
/// somebody is being asked to accept.
fn difference_line(difference: &aop_core::compare::Difference) -> String {
    use aop_core::compare::Difference as D;
    match difference {
        D::TaskAdded { .. } => "added".to_string(),
        D::TaskRemoved { .. } => "removed".to_string(),
        D::TaskMoved { from, to, .. } => format!("moved from row {} to row {}", from + 1, to + 1),
        D::FieldChanged {
            field,
            before,
            after,
            ..
        } => format!("{}: {before} to {after}", field.label()),
        D::LinkAdded { kind, .. } => format!("now waits on another task ({})", kind.code()),
        D::LinkRemoved { kind, .. } => {
            format!("no longer waits on another task ({})", kind.code())
        }
        D::LinkChanged {
            before_kind,
            after_kind,
            ..
        } => format!(
            "the task it waits on changed from {} to {}",
            before_kind.code(),
            after_kind.code()
        ),
        D::ResourceAdded { name, .. } => format!("{name} added to the resource sheet"),
        D::ResourceRemoved { name, .. } => format!("{name} taken off the resource sheet"),
        D::ResourceChanged {
            field,
            before,
            after,
            ..
        } => format!("{}: {before} to {after}", field.label()),
        D::AssignmentAdded {
            resource_name,
            units,
            ..
        } => format!("{resource_name} assigned at {:.0}%", units * 100.0),
        D::AssignmentRemoved { resource_name, .. } => format!("{resource_name} taken off it"),
        D::AssignmentChanged {
            resource_name,
            before_units,
            after_units,
            ..
        } => format!(
            "{resource_name} from {:.0}% to {:.0}%",
            before_units * 100.0,
            after_units * 100.0
        ),
    }
}

/// Somebody pushed first. What they did, and the choice about it.
///
/// The decision this whole design exists to offer, so it is a dialog with the
/// difference in it rather than a message saying a sync failed. Both answers
/// are real: taking their work replays this planner's on top of it, and
/// keeping this copy for now leaves everything exactly where it is.
#[component]
fn SyncBehind(
    head: i64,
    sentence: String,
    differences: Vec<aop_core::compare::Difference>,
    changes: Vec<aop_core::history::Change>,
    replayed: usize,
    asked: usize,
    more: bool,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // How much of the difference the panel will list. A first sync against a
    // busy plan runs to hundreds, and the sentence above already says how big
    // it is; the list is for recognising the work, not for reading all of it.
    const SHOWN: usize = 60;

    let grouped = aop_core::compare::group_by_task(&differences);
    let listed: usize = grouped.tasks.len() + grouped.plan.len();
    let who: Vec<String> = {
        let mut names: Vec<String> = changes.iter().map(|change| change.author.clone()).collect();
        names.sort();
        names.dedup();
        names.retain(|name| !name.trim().is_empty());
        names
    };
    let by = match who.len() {
        0 => "Somebody else".to_string(),
        1 => who[0].clone(),
        _ => format!("{} and {} other(s)", who[0], who.len() - 1),
    };

    // Worked out before the question is asked, so the answer is against
    // something real: a command that will not replay here means the two copies
    // have already parted, and accepting would leave the plan half theirs.
    let drifted = replayed < asked;

    // Each button outlives this render, so it takes its own copy.
    let for_accept = (differences.clone(), changes.clone());

    rsx! {
        Head { title: "Somebody else changed this plan first".to_string() }
        div { class: "dlg-body", style: "min-width: 720px; max-height: 62vh; overflow-y: auto;",
            p { style: "line-height: 1.6;",
                "{by} pushed work to the server before this copy did. The server is at change \
                 {head}. Their work: {sentence}"
            }

            if drifted {
                div { class: "sync-drift",
                    {icon("warning", 16)}
                    span {
                        "{asked - replayed} of the {asked} commands that came in will not run \
                         against this copy of the plan. Bringing them in would leave it part \
                         theirs and part yours, so it will be refused and a fresh whole plan \
                         offered instead."
                    }
                }
            }

            if listed == 0 {
                p { class: "hint", "Their changes do not alter anything this copy can see." }
            } else {
                div { class: "diff-list",
                    for group in grouped.tasks.iter().take(SHOWN) {
                        div { key: "t{group.id}", class: "diff-group",
                            div { class: "diff-subject", "{group.name}" }
                            for (index, difference) in group.differences.iter().enumerate() {
                                div { key: "d{index}", class: "diff-line",
                                    "{difference_line(difference)}"
                                }
                            }
                        }
                    }
                    if !grouped.plan.is_empty() {
                        div { class: "diff-group",
                            div { class: "diff-subject", "Resource sheet" }
                            for (index, difference) in grouped.plan.iter().enumerate() {
                                div { key: "r{index}", class: "diff-line",
                                    "{difference_line(difference)}"
                                }
                            }
                        }
                    }
                }
                if grouped.tasks.len() > SHOWN {
                    p { class: "hint",
                        "Showing {SHOWN} of the {grouped.tasks.len()} tasks they touched."
                    }
                }
            }

            if more {
                p { class: "hint",
                    "There is more waiting beyond this batch. Taking these and syncing again \
                     brings the rest."
                }
            }

            p { class: "hint",
                "Nothing here has been changed yet. Taking their work keeps yours: it is put \
                 back on top afterwards and sent in the same step. Whichever you choose, the \
                 plan as it stands now is kept in History and Sync first."
            }
        }
        div { class: "dlg-foot",
            button {
                class: "btn",
                onclick: move |_| {
                    let mut writer = state.write();
                    writer.dialog = None;
                    writer.cloud_message = Some(
                        "Their changes were left on the server. Nothing here has been sent or \
                         changed. Sync again when you are ready to take them."
                            .into(),
                    );
                },
                "Keep mine for now"
            }
            button {
                class: "btn primary",
                onclick: move |_| {
                    let (differences, changes) = for_accept.clone();
                    crate::collaborate::accept_incoming(
                        state, head, differences, changes, replayed, asked,
                    );
                },
                "Take theirs and put mine on top"
            }
        }
    }
}

/// This copy's cursor is past the server's, so it is not the plan it thinks.
///
/// Refused rather than reconciled. Pushing would interleave two histories that
/// only look alike, and there is no answer to offer here that is not a guess
/// about which of two plans somebody meant.
#[component]
fn SyncAhead(head: i64, cursor: i64) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    rsx! {
        Head { title: "This is not the plan the server holds".to_string() }
        div { class: "dlg-body", style: "min-width: 560px;",
            div { style: "display: flex; gap: 14px; align-items: flex-start;",
                span { style: "color: var(--danger); flex: none;", {icon("warning", 28)} }
                div { style: "line-height: 1.6;",
                    p {
                        "This copy has read up to change {cursor}, but the server's log only \
                         reaches {head}. A copy cannot have read further than there is to read, \
                         so these are two different logs that share an address."
                    }
                    p { style: "margin-top: 10px;",
                        "Nothing has been sent. Pushing would lay this plan's history into \
                         somebody else's and leave both wrong."
                    }
                    p { style: "margin-top: 10px;",
                        "This usually means the project on the server was rebuilt, or this file \
                         was linked to a different project of the same name. Unlink it and put \
                         it on the server again, or take a fresh copy of the plan the server \
                         does hold."
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

/// A link somebody opened, and what opening it would do.
///
/// The server is the first thing on the page and the largest, because it is
/// the part that is not this planner's choice. A link is an instruction from
/// whoever sent it to go and talk to a host they picked, and the difference
/// between opening a plan and being pointed at somebody else's server is
/// whether that host was shown before the request went.
///
/// It is also honest about what a link is and is not. It admits nobody by
/// itself: it says where a plan is, and the server says who may have it. What
/// lets somebody in is an invitation the owner sent to their email address,
/// which opening the link claims on their behalf. Saying that here is better
/// than a refusal from the server that reads like the plan is missing.
#[component]
fn OpenLink(share: crate::cloud::share::Share) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (waiting, dirty, configured) = {
        let s = state.read();
        (
            s.working.is_some(),
            s.dirty,
            s.collaborate_server.trim().to_string(),
        )
    };
    // Named only when it differs, since agreeing with the setting is the
    // ordinary case and saying so every time would bury the case that matters.
    let elsewhere = (!configured.is_empty() && configured != share.server).then_some(configured);
    let asked = share.clone();

    rsx! {
        Head { title: "Open a plan from a link".to_string() }
        div { class: "dlg-body", style: "min-width: 600px;",
            div { class: "sync-row",
                span { class: "sync-key", "This link points at" }
                span { class: "sync-value mono", "{share.server}" }
            }
            div { class: "sync-row",
                span { class: "sync-key", "and asks for the plan" }
                span { class: "sync-value mono", "{share.project}" }
            }
            p { style: "margin-top: 12px; line-height: 1.6;",
                "Opening it asks that server for the plan and shows what comes back.                  If you are not in this plan yet, it also presents this account, in case                  whoever owns the plan has invited the address you sign in with. The link                  by itself grants nothing: without an invitation the server will say the                  plan is not yours to open."
            }
            if let Some(configured) = elsewhere {
                div { class: "sync-drift",
                    span { {icon("warning", 16)} }
                    div {
                        "This is not the server this copy is set to, which is "
                        b { "{configured}" }
                        ". Opening the link will use the address in the link for this plan                          from now on. Only carry on if you know that server."
                    }
                }
            }
            if dirty {
                div { class: "sync-drift",
                    span { {icon("warning", 16)} }
                    div {
                        "The plan on screen has changes that are not saved, and opening this                          one replaces it. Save it first if you want to keep them."
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            if waiting {
                button { class: "btn primary", disabled: true, "Fetching..." }
            } else {
                button {
                    class: "btn primary",
                    onclick: move |_| crate::collaborate::open_link(state, asked.clone()),
                    "Open the plan"
                }
            }
        }
    }
}

/// Replaying is no longer possible, so only a whole plan will do.
///
/// The offer is deliberate rather than automatic: fetching replaces everything
/// on screen, and doing that without asking is how a planner loses an
/// afternoon to a button they pressed for a different reason.
#[component]
fn FreshCopy(why: String) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let waiting = state.read().working.is_some();

    rsx! {
        Head { title: "This copy needs a fresh plan".to_string() }
        div { class: "dlg-body", style: "min-width: 600px;",
            div { style: "display: flex; gap: 14px; align-items: flex-start;",
                span { style: "color: var(--warn); flex: none;", {icon("warning", 28)} }
                div { style: "line-height: 1.6;",
                    p { "{why}" }
                    p { style: "margin-top: 10px;",
                        "Fetching one replaces what is on screen with the server's copy. The \
                         plan as it stands now is kept first, and is in History and Sync if you \
                         want it back."
                    }
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Not now" }
            if waiting {
                button { class: "btn primary", disabled: true, "Fetching..." }
            } else {
                button {
                    class: "btn primary",
                    onclick: move |_| crate::collaborate::fresh_copy(state),
                    "Fetch a fresh copy"
                }
            }
        }
    }
}

/// Put the plan back to one of its versions.
///
/// Named for what it does and confirmed on its own, because it is the one
/// action in History and Sync that changes the plan. A restore that happened
/// on a single click of a row would be a list somebody could not browse.
#[component]
fn RestoreVersion(index: usize) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let found = {
        let s = state.read();
        s.versions.get(index).map(|snapshot| {
            (
                snapshot.at.format("%Y-%m-%d %H:%M").to_string(),
                snapshot.author.clone(),
                snapshot.taken.label(),
                snapshot.plan.tasks.len(),
            )
        })
    };
    let Some((when, author, label, tasks)) = found else {
        return rsx! {
            MessageBox {
                title: "Go back to a version".to_string(),
                body: "That version is no longer kept.".to_string(),
            }
        };
    };

    let sentence = {
        let s = state.read();
        aop_core::compare::summarise(&s.versions.changed_after(index, &s.project)).sentence()
    };
    let against = state.read().versions.compared_with(index);
    let shared = state.read().link.is_some();

    rsx! {
        Head { title: "Go back to a version".to_string() }
        div { class: "dlg-body", style: "min-width: 560px;",
            p { style: "line-height: 1.6;",
                "This replaces the plan on screen with the one kept at {when} by {author} \
                 ({label}), which has {tasks} task(s)."
            }
            div { class: "ver-diff", style: "margin-top: 12px;",
                div { class: "ver-diff-head",
                    {icon("compare", 15)}
                    span { "Compared with {against}" }
                }
                p { "{sentence}" }
            }
            p { class: "hint",
                "The change log is kept as it is. Going back is one more thing that was done, \
                 not a reason to forget the record of the rest, and it can be undone."
            }
            if shared {
                p { class: "hint",
                    "This plan is on a server. Going back cannot be sent as a command, so the \
                     server is not told: sync afterwards to see what it makes of it."
                }
            }
        }
        div { class: "dlg-foot",
            button { class: "btn", onclick: move |_| state.write().dialog = None, "Cancel" }
            button {
                class: "btn danger",
                onclick: move |_| state.write().restore_version(index),
                "Go back to this version"
            }
        }
    }
}

/// What the health check found, one question at a time.
///
/// A list with a result each rather than one verdict, because which question
/// failed is the diagnosis: "server reachable, identity provider unreachable"
/// points at a name or a firewall, and "could not connect" points at nothing.
#[component]
fn HealthCheck() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (checks, running) = {
        let s = state.read();
        (s.health.clone(), s.working.is_some())
    };

    rsx! {
        Head { title: "Check Collaborate".to_string() }
        div { class: "dlg-body", style: "min-width: 640px;",
            p { class: "hint", style: "margin-top: 0;",
                "Each question is asked and answered on its own. The server's own check needs \
                 no sign in, so it still answers when signing in is the thing that is broken."
            }

            if running && checks.is_empty() {
                p { style: "margin-top: 14px;", "Asking..." }
            } else if checks.is_empty() {
                p { style: "margin-top: 14px;", "Nothing has been checked yet." }
            } else {
                div { class: "health-list",
                    for check in checks.iter() {
                        div { key: "{check.asked}", class: "health-row",
                            span { class: "health-badge {health_class(check.outcome)}",
                                "{check.outcome.label()}"
                            }
                            div { class: "health-text",
                                div { class: "health-asked", "{check.asked}" }
                                div { class: "health-detail", "{check.detail}" }
                            }
                        }
                    }
                }
                p { class: "hint",
                    "Worth quoting in a bug report: the server's name and version above, and \
                     which of these lines failed. No part of any sign in token is shown here, \
                     or written anywhere this application logs."
                }
            }
        }
        div { class: "dlg-foot",
            if running {
                button { class: "btn", disabled: true, "Checking..." }
            } else {
                button {
                    class: "btn",
                    onclick: move |_| crate::collaborate::health(state),
                    "Check again"
                }
            }
            button { class: "btn primary", onclick: move |_| state.write().dialog = None, "Close" }
        }
    }
}

/// Which colour a result gets. Not checked is its own thing rather than a
/// pale pass: a check that did not run has not said anything.
fn health_class(outcome: crate::cloud::health::Outcome) -> &'static str {
    use crate::cloud::health::Outcome;
    match outcome {
        Outcome::Good => "good",
        Outcome::Warning => "warn",
        Outcome::Bad => "bad",
        Outcome::NotChecked => "idle",
    }
}

/// Saving would write over a file that is already there.
///
/// Three answers rather than two. Replacing is what somebody usually means
/// when they type a name that already exists, but not always, and the way
/// out has to be as easy as the way through or people stop reading the
/// question. The suggested name is worked out before the question is asked,
/// so the button can say exactly what it will do rather than promising to
/// pick something.
#[component]
fn ConfirmOverwrite(path: std::path::PathBuf, beside: std::path::PathBuf) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let existing = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let suggested = beside
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| beside.display().to_string());
    let folder = path
        .parent()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    // Only offered when a free name was actually found. A folder holding
    // every numbered name up to the bound is stranger than the question
    // itself, and a button that might not work is worse than one fewer.
    let can_keep_both = beside != path;

    rsx! {
        div { class: "dialog", style: "width: 460px;",
            div { class: "dialog-head", "{existing} already exists" }
            div { class: "dialog-body",
                p { class: "opt-aside", "In {folder}." }
                p {
                    "Replacing it cannot be undone: the file on disk is gone, whatever \
                     is in this plan."
                }
                if can_keep_both {
                    p { class: "opt-aside", "Keeping both saves as {suggested}." }
                }
            }
            div { class: "dialog-foot",
                button {
                    class: "btn",
                    onclick: move |_| state.write().dialog = None,
                    "Cancel"
                }
                div { class: "grow" }
                if can_keep_both {
                    button {
                        class: "btn",
                        onclick: {
                            let beside = beside.clone();
                            move |_| {
                                let mut writer = state.write();
                                writer.dialog = None;
                                writer.save_to(beside.clone());
                            }
                        },
                        "Save as {suggested}"
                    }
                }
                button {
                    class: "btn danger",
                    onclick: {
                        let path = path.clone();
                        move |_| {
                            let mut writer = state.write();
                            writer.dialog = None;
                            writer.save_to(path.clone());
                        }
                    },
                    "Replace"
                }
            }
        }
    }
}
