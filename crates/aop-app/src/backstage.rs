//! The File menu: the start dashboard, template gallery, file browser, print
//! preview, export, and the Info page that doubles as a project dashboard.

use std::path::PathBuf;

use dioxus::prelude::*;

use aop_core::{format_work, persist, templates, Project, APP_NAME};
use chrono::Datelike;

use crate::icons::icon;
use crate::preview::{mini_gantt, DARK};
use crate::controls::{Choice, Dropdown};
use crate::state::{
    documents_dir, format_date_long, next_monday, AppState, BackstagePage, Dialog, OptionsPage,
    PendingAction, ViewKind,
};

#[component]
pub fn Backstage(page: BackstagePage) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    rsx! {
        div { class: "backstage",
            oncontextmenu: move |event| event.prevent_default(),
            nav { class: "bs-nav",
                button { class: "bs-back",
                    onclick: move |_| {
                        state.write().backstage = None;
                        state.write().backstage_message = None;
                    },
                    {icon("back", 18)}
                    span { "Back" }
                }
                for entry in [
                    BackstagePage::Home,
                    BackstagePage::Info,
                    BackstagePage::New,
                    BackstagePage::Open,
                    BackstagePage::Save,
                    BackstagePage::SaveAs,
                    BackstagePage::Print,
                    BackstagePage::Export,
                ] {
                    {
                        let class = if entry == page { "bs-item active" } else { "bs-item" };
                        rsx! {
                            button { key: "{entry:?}", class: "{class}",
                                onclick: move |_| {
                                    let mut writer = state.write();
                                    writer.backstage_message = None;
                                    drop(writer);
                                    if entry == BackstagePage::Save {
                                        let saved = state.write().save();
                                        if !saved {
                                            state.write().backstage = Some(BackstagePage::SaveAs);
                                        }
                                    } else {
                                        state.write().backstage = Some(entry);
                                    }
                                },
                                span { class: "glyph", {icon(entry.glyph(), 16)} }
                                span { "{entry.label()}" }
                            }
                        }
                    }
                }
                // Everything above is about the document; what follows is
                // about the application, so it sits at the foot of the list.
                div { class: "bs-spacer" }
                div { class: "bs-sep" }
                for entry in [BackstagePage::About, BackstagePage::Options] {
                    {
                        let class = if entry == page { "bs-item active" } else { "bs-item" };
                        rsx! {
                            button { key: "{entry:?}", class: "{class}",
                                onclick: move |_| state.write().backstage = Some(entry),
                                span { class: "glyph", {icon(entry.glyph(), 16)} }
                                span { "{entry.label()}" }
                            }
                        }
                    }
                }
                div { class: "bs-sep" }
                button { class: "bs-item",
                    onclick: move |_| state.write().guard(PendingAction::CloseProject),
                    span { class: "glyph", {icon("x", 16)} }
                    span { "Close" }
                }
            }

            section { class: "bs-body",
                match page {
                    BackstagePage::Home => rsx! { HomePage {} },
                    BackstagePage::Info => rsx! { InfoPage {} },
                    BackstagePage::New => rsx! { NewPage {} },
                    BackstagePage::Open => rsx! { FileBrowser { saving: false } },
                    BackstagePage::Save | BackstagePage::SaveAs => rsx! { FileBrowser { saving: true } },
                    BackstagePage::Print => rsx! { PrintPage {} },
                    BackstagePage::Export => rsx! { ExportPage {} },
                    BackstagePage::About => rsx! { AboutPage {} },
                    BackstagePage::Options => rsx! { OptionsPageView {} },
                }
            }
        }
    }
}

/// A confirmation strip shown after a page does something.
#[component]
fn Confirmation() -> Element {
    let state = use_context::<Signal<AppState>>();
    let message = state.read().backstage_message.clone();
    match message {
        Some(text) => rsx! { div { class: "ok-banner", "{text}" } },
        None => rsx! {},
    }
}

// ------------------------------------------------------------------- Info

#[component]
fn InfoPage() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;

    let path = s
        .file_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("Not saved yet \u{00b7} .{}", persist::FILE_EXTENSION));

    let report = s.report.as_ref().ok();
    let dash = "-".to_string();

    let figures: Vec<(&str, String)> = vec![
        (
            "Duration",
            report
                .map(|r| aop_core::format_duration(r.duration_minutes))
                .unwrap_or_else(|| dash.clone()),
        ),
        ("Tasks", project.tasks.len().to_string()),
        (
            "Critical",
            report
                .map(|r| r.critical_task_count.to_string())
                .unwrap_or_else(|| dash.clone()),
        ),
        ("Complete", format!("{}%", project.percent_complete())),
        (
            "Work",
            report
                .map(|r| format_work(r.total_work_minutes))
                .unwrap_or_else(|| dash.clone()),
        ),
        (
            "Cost",
            report
                .map(|r| format!("{}{:.2}", project.currency_symbol, r.total_cost))
                .unwrap_or_else(|| dash.clone()),
        ),
    ];

    let leaf_count = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i))
        .count();
    let milestones = project.tasks.iter().filter(|t| t.is_milestone()).count();
    let summaries = project.tasks.len() - leaf_count;

    let schedule_rows: Vec<(&str, String)> = vec![
        (
            "Start",
            report
                .map(|r| format_date_long(r.start))
                .unwrap_or_else(|| dash.clone()),
        ),
        (
            "Finish",
            report
                .map(|r| format_date_long(r.finish))
                .unwrap_or_else(|| dash.clone()),
        ),
        ("Calendar", project.calendar.name.clone()),
        (
            "Holidays",
            project.calendar.exceptions.len().to_string(),
        ),
        (
            "Baseline",
            if project.has_baseline() { "Saved".into() } else { "Not set".into() },
        ),
    ];

    let content_rows: Vec<(&str, String)> = vec![
        ("Working tasks", leaf_count.to_string()),
        ("Summary tasks", summaries.to_string()),
        ("Milestones", milestones.to_string()),
        ("Links", project.links.len().to_string()),
        ("Resources", project.resources.len().to_string()),
    ];

    let overallocations = report
        .map(|r| r.overallocations.clone())
        .unwrap_or_default();

    rsx! {
        div { class: "info-head",
            h1 { class: "bs-title", style: "margin: 0;", "{project.name}" }
            div { class: "recent-path", "{path}" }
        }

        if let Err(message) = &s.report {
            div { class: "info-alert",
                span { class: "fix-icon", {icon("warning", 18)} }
                div { style: "flex: 1;", "{message}" }
            }
        }

        div { class: "stat-row",
            for (label, value) in figures {
                div { key: "{label}", class: "stat-tile",
                    div { class: "stat-value", "{value}" }
                    div { class: "stat-label", "{label}" }
                }
            }
        }

        if !project.tasks.is_empty() {
            div { class: "info-chart",
                {mini_gantt(project, 720.0, 180.0, 34, &DARK, s.show_critical)}
            }
        }

        div { class: "info-cards",
            div { class: "info-card",
                h3 { "Schedule" }
                for (label, value) in schedule_rows {
                    div { key: "{label}", class: "info-line",
                        span { class: "k", "{label}" }
                        span { class: "v", "{value}" }
                    }
                }
            }
            div { class: "info-card",
                h3 { "Contents" }
                for (label, value) in content_rows {
                    div { key: "{label}", class: "info-line",
                        span { class: "k", "{label}" }
                        span { class: "v", "{value}" }
                    }
                }
            }
        }

        if !overallocations.is_empty() {
            div { class: "info-card", style: "margin-top: 18px; max-width: 720px;",
                h3 { "Overallocated resources" }
                for entry in overallocations.iter() {
                    div { key: "{entry.resource}", class: "info-line",
                        span { class: "k", "{entry.resource_name}" }
                        span { class: "v",
                            "{entry.peak_units * 100.0:.0}% peak \u{00b7} {entry.days} day(s) from {entry.first_date}"
                        }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------- Home

/// Load each recent file so its card can show a real thumbnail. Files that no
/// longer open are dropped rather than shown as broken cards.
fn load_recent_previews(entries: &[crate::state::RecentEntry]) -> Vec<(String, PathBuf, Project)> {
    entries
        .iter()
        .take(6)
        .filter_map(|entry| {
            // Whatever the Open page will open, a preview card must show.
            let mut project = persist::open_any(&entry.path).ok()?;
            let _ = aop_core::schedule(&mut project);
            Some((entry.name.clone(), entry.path.clone(), project))
        })
        .collect()
}

#[component]
fn HomePage() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let recent = state.read().recent.clone();
    let previews = use_signal(|| load_recent_previews(&recent));
    let cards = previews.read();

    // Blank first, then a few starters, the way the Office home screen leads.
    let featured = use_signal(build_previews);
    let built = featured.read();
    let show_critical = state.read().show_critical;

    rsx! {
        h1 { class: "bs-title", style: "margin-bottom: 6px;", "Home" }

        div { class: "home-section", style: "margin-top: 18px;",
            h2 { class: "bs-sub", style: "margin-top: 0;", "New" }
            button { class: "bs-link", onclick: move |_| state.write().backstage = Some(BackstagePage::New),
                "More templates \u{2192}"
            }
        }
        div { class: "tpl-grid",
            for (spec, project) in built.iter() {
                {
                    let id = spec.id;
                    let count = spec.task_count();
                    rsx! {
                        button { key: "{id}", class: "tpl-card",
                            onclick: move |_| {
                                if id == "blank" {
                                    state.write().guard(PendingAction::CloseProject);
                                } else {
                                    state.write().dialog = Some(Dialog::TemplatePreview(id.to_string()));
                                }
                            },
                            div { class: "tpl-thumb",
                                {mini_gantt(project, 300.0, 128.0, 24, &DARK, show_critical)}
                            }
                            div { class: "tpl-meta",
                                div { class: "tpl-name", "{spec.name}" }
                                div { class: "tpl-count",
                                    if count > 0 { "{count} tasks" } else { "Start from nothing" }
                                }
                            }
                        }
                    }
                }
            }
        }

        div { class: "home-section",
            h2 { class: "bs-sub", "Recent" }
            button { class: "bs-link", onclick: move |_| state.write().backstage = Some(BackstagePage::Open),
                "Browse \u{2192}"
            }
        }

        if cards.is_empty() {
            div { class: "home-empty",
                {icon("open", 26)}
                div {
                    div { style: "color: var(--ink); font-size: 13px; margin-bottom: 3px;", "No recent projects" }
                    div { "Plans you save appear here with a preview." }
                }
            }
        } else {
            div { class: "tpl-grid",
                for (name, path, project) in cards.iter() {
                    {
                        let target = path.clone();
                        let where_from = path
                            .parent()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        let tasks = project.tasks.len();
                        let finish = project.finish_date;
                        rsx! {
                            button { key: "{path.display()}", class: "tpl-card",
                                onclick: move |_| {
                                    state.write().guard(PendingAction::Open(target.clone()));
                                },
                                div { class: "tpl-thumb",
                                    {mini_gantt(project, 300.0, 128.0, 24, &DARK, show_critical)}
                                }
                                div { class: "tpl-meta",
                                    div { class: "tpl-name", "{name}" }
                                    div { class: "tpl-desc", "{where_from}" }
                                    div { class: "tpl-count",
                                        "{tasks} tasks \u{00b7} ends {crate::state::format_date(finish)}"
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

// -------------------------------------------------------------------- New

/// Build and schedule every template once, so the tiles can show real charts.
fn build_previews() -> Vec<(&'static templates::TemplateSpec, Project)> {
    let start = next_monday(chrono::Local::now().naive_local().date())
        .and_hms_opt(8, 0, 0)
        .expect("valid time");
    templates::all()
        .iter()
        .map(|spec| {
            let mut project = templates::build(spec, start);
            let _ = aop_core::schedule(&mut project);
            (spec, project)
        })
        .collect()
}

#[component]
fn NewPage() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut search = use_signal(String::new);
    let previews = use_signal(build_previews);

    let needle = search().to_lowercase();
    let built = previews.read();

    rsx! {
        h1 { class: "bs-title", "New" }

        div { class: "bs-field", style: "max-width: 430px;",
            input {
                class: "bs-input",
                placeholder: "Search for a template",
                value: "{search}",
                oninput: move |event| search.set(event.value()),
            }
        }

        div { class: "tpl-grid",
            for (spec, project) in built.iter() {
                {
                    let matches = needle.is_empty()
                        || spec.name.to_lowercase().contains(&needle)
                        || spec.description.to_lowercase().contains(&needle);
                    if !matches {
                        rsx! {}
                    } else {
                        let id = spec.id;
                        let count = spec.task_count();
                        let finish = project.finish_date;
                        let show_critical = state.read().show_critical;
                        rsx! {
                            button { key: "{id}", class: "tpl-card",
                                onclick: move |_| {
                                    if id == "blank" {
                                        state.write().guard(PendingAction::CloseProject);
                                    } else {
                                        state.write().dialog = Some(Dialog::TemplatePreview(id.to_string()));
                                    }
                                },
                                div { class: "tpl-thumb",
                                    {mini_gantt(project, 300.0, 128.0, 24, &DARK, show_critical)}
                                }
                                div { class: "tpl-meta",
                                    div { class: "tpl-name", "{spec.name}" }
                                    div { class: "tpl-desc", "{spec.description}" }
                                    if count > 0 {
                                        div { class: "tpl-count",
                                            "{count} tasks \u{00b7} ends {crate::state::format_date(finish)}"
                                        }
                                    } else {
                                        div { class: "tpl-count", "Start from nothing" }
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

// ----------------------------------------------------------- Open/Save As

/// Project XML gets its own glyph so it is obvious what a row is.
fn glyph_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .as_deref()
    {
        Some("xml") | Some("mpp") => "export",
        _ => "new",
    }
}

/// A minimal file browser so Open and Save As work without a native dialog.
#[component]
fn FileBrowser(saving: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let initial_dir = {
        let s = state.read();
        s.file_path
            .as_ref()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(documents_dir)
    };
    let initial_name = {
        let s = state.read();
        s.file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| s.project.name.clone())
    };

    let mut dir = use_signal(|| initial_dir);
    let mut filename = use_signal(|| initial_name);

    // Folders first, then project files, both sorted by name.
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
                } else if path.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case(persist::FILE_EXTENSION)
                        || (!saving && (e.eq_ignore_ascii_case("xml") || e.eq_ignore_ascii_case("mpp")))
                }) {
                    files.push((name, path));
                }
            }
        }
        folders.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        (folders, files)
    };

    let recent = state.read().recent.clone();
    let title = if saving { "Save As" } else { "Open" };
    let current = dir().display().to_string();

    let mut do_save = move || {
        let name = filename();
        let name = if name.trim().is_empty() {
            "Project1".to_string()
        } else {
            name.trim().to_string()
        };
        state.write().save_to(dir().join(name));
    };

    rsx! {
        h1 { class: "bs-title", "{title}" }
        Confirmation {}

        if !saving && !recent.is_empty() {
            h2 { class: "bs-sub", "Recent" }
            div { class: "recent-list",
                for entry in recent.iter() {
                    {
                        let path = entry.path.clone();
                        rsx! {
                            button { key: "{entry.path.display()}", class: "recent-row",
                                onclick: move |_| {
                                    state.write().guard(PendingAction::Open(path.clone()));
                                },
                                span { class: "glyph", {icon("open", 22)} }
                                div {
                                    div { class: "recent-name", "{entry.name}" }
                                    div { class: "recent-path", "{entry.path.display()}" }
                                }
                            }
                        }
                    }
                }
            }
        }

        h2 { class: "bs-sub", "Browse" }
        div { class: "bs-field",
            label { "Folder" }
            input {
                class: "bs-input",
                value: "{current}",
                onchange: move |event| {
                    let candidate = PathBuf::from(event.value());
                    if candidate.is_dir() {
                        dir.set(candidate);
                    }
                },
            }
            button { class: "btn",
                onclick: move |_| {
                    let parent = dir().parent().map(PathBuf::from);
                    if let Some(parent) = parent { dir.set(parent); }
                },
                "Up"
            }
        }

        if saving {
            div { class: "bs-field",
                label { "File name" }
                input {
                    class: "bs-input",
                    value: "{filename}",
                    oninput: move |event| filename.set(event.value()),
                    onkeydown: move |event| if event.key() == Key::Enter { do_save() },
                }
                span { class: "recent-path", ".{persist::FILE_EXTENSION}" }
            }
            div { class: "bs-field",
                label { "" }
                button { class: "btn primary", onclick: move |_| do_save(), "Save" }
            }
        }

        div { class: "recent-list", style: "margin-top: 10px; max-height: 320px; overflow-y: auto;",
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
                    let stem = path
                        .file_stem()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    rsx! {
                        button { key: "f{name}", class: "recent-row",
                            onclick: move |_| {
                                if saving {
                                    filename.set(stem.clone());
                                } else {
                                    state.write().guard(PendingAction::Open(target.clone()));
                                }
                            },
                            span { class: "glyph", {icon(glyph_for(&target), 20)} }
                            div { class: "recent-name", "{name}" }
                        }
                    }
                }
            }
        }

        div { class: "hint",
            "{APP_NAME} plans are stored as .{persist::FILE_EXTENSION} files, a compact binary container."
            if !saving {
                br {}
                "Microsoft Project plans open here too, both .mpp and XML Format (*.xml). "
                "They come in as a new plan, so use Save As to keep one as a ."
                "{persist::FILE_EXTENSION} file."
            }
        }
    }
}

// ------------------------------------------------------------------ Print

/// The Print page: what it will look like on the left, where it is going on
/// the right.
///
/// The document is produced by the application rather than by the web engine,
/// so what the preview shows and what the printer receives are the same bytes
/// rather than two renderings that can disagree.
#[component]
fn PrintPage() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // The document starts out matching what is on screen, so a plan showing the
    // critical path prints it without the option having to be found first.
    let mut options = use_signal(|| aop_core::pdf::PrintOptions {
        show_critical: state.read().show_critical,
        ..aop_core::pdf::PrintOptions::default()
    });

    // Asked for once rather than on every render: it shells out to CUPS, and
    // the list does not change while a page is being set up.
    let queues = use_signal(crate::spooler::printers);
    let mut chosen = use_signal(|| None::<String>);
    let mut copies = use_signal(|| 1u16);
    let mut target = use_signal(|| {
        let s = state.read();
        s.file_path
            .as_ref()
            .map(|p| p.with_extension("pdf"))
            .unwrap_or_else(|| documents_dir().join(format!("{}.pdf", s.project.name)))
            .display()
            .to_string()
    });

    let document = {
        let s = state.read();
        aop_core::pdf::render(&s.project, &options())
    };
    let pages = document.windows(10).filter(|w| w == b"/Type /Page").count();
    let size_kb = document.len() as f64 / 1024.0;

    // The preview is the document itself, handed to the engine's own viewer, so
    // it cannot drift from what gets printed.
    let preview_src = format!(
        "data:application/pdf;base64,{}",
        crate::spooler::base64(&document)
    );

    let default_queue = queues()
        .as_ref()
        .ok()
        .and_then(|list| list.iter().find(|p| p.default).or_else(|| list.first()))
        .map(|p| p.name.clone());
    let selected = chosen().or(default_queue.clone());

    rsx! {
        h1 { class: "bs-title", "Print" }
        Confirmation {}

        div { class: "print-layout",
            // ---- the document, at the size it will be printed --------------
            div { class: "print-preview",
                object {
                    class: "print-frame",
                    r#type: "application/pdf",
                    data: "{preview_src}",
                    div { class: "print-fallback",
                        "{pages} page(s), {size_kb:.0} KB. Save it to look at it outside the application."
                    }
                }
            }

            // ---- where it is going ----------------------------------------
            div { class: "print-settings",

                // The command comes first, with the copy count beside it, the
                // way Project puts them. Everything under it is what the
                // command will do, read top to bottom.
                div { class: "print-action",
                    {
                        let to_print = document.clone();
                        let queue = selected.clone();
                        rsx! {
                            button {
                                class: "btn primary print-go",
                                disabled: queue.is_none(),
                                onclick: move |_| {
                                    let Some(queue) = queue.clone() else { return };
                                    let title = state.read().project.name.clone();
                                    let outcome =
                                        crate::spooler::spool(&queue, &title, &to_print, copies());
                                    let mut writer = state.write();
                                    match outcome {
                                        Ok(said) => writer.status = said,
                                        Err(complaint) => {
                                            writer.dialog = Some(Dialog::Message {
                                                title: "Could not print".into(),
                                                body: complaint,
                                            })
                                        }
                                    }
                                },
                                span { class: "glyph", {icon("printer", 17)} }
                                span { "Print" }
                            }
                        }
                    }
                    div { class: "print-copies",
                        label { "Copies" }
                        input {
                            r#type: "number",
                            min: "1",
                            max: "99",
                            value: "{copies}",
                            onchange: move |event| {
                                let wanted = event.value().trim().parse::<u16>().unwrap_or(1);
                                copies.set(wanted.clamp(1, 99));
                            },
                        }
                    }
                }

                h2 { class: "bs-sub", "Printer" }

                match queues().as_ref() {
                    Ok(list) => rsx! {
                        div { class: "queue-list",
                            for printer in list.clone() {
                                {
                                    let name = printer.name.clone();
                                    let picked = selected.as_deref() == Some(name.as_str());
                                    let class = if picked { "queue on" } else { "queue" };
                                    rsx! {
                                        button { key: "{name}", class: "{class}",
                                            onclick: move |_| chosen.set(Some(name.clone())),
                                            span { class: "glyph", {icon("printer", 16)} }
                                            span { class: "queue-text",
                                                span { class: "queue-name",
                                                    "{printer.name}"
                                                    if printer.default {
                                                        span { class: "queue-default", "default" }
                                                    }
                                                }
                                                span { class: "queue-status", "{printer.status}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(reason) => {
                        let message = reason.message();
                        rsx! {
                            div { class: "hint", style: "margin: 0 0 12px;", "{message}" }
                        }
                    }
                }

                if false {
                    {
                    rsx! {}
                    }
                }

                h2 { class: "bs-sub", "Save as PDF" }
                div { class: "hint", style: "margin: 0 0 8px;",
                    "Works whether or not a printer is set up." }
                input {
                    class: "bs-input",
                    style: "width: 100%; margin-bottom: 8px;",
                    value: "{target}",
                    oninput: move |event| target.set(event.value()),
                }
                {
                let to_save = document.clone();
                rsx! {
                button {
                    class: "btn",
                    style: "width: 100%;",
                    onclick: move |_| {
                        let path = PathBuf::from(target());
                        let outcome = crate::spooler::save(&path, &to_save);
                        let mut writer = state.write();
                        match outcome {
                            Ok(written) => writer.status = format!("Saved {}", written.display()),
                            Err(complaint) => {
                                writer.dialog = Some(Dialog::Message {
                                    title: "Could not save".into(),
                                    body: complaint,
                                })
                            }
                        }
                    },
                    span { class: "glyph", {icon("save-mono", 16)} }
                    span { "Save PDF" }
                }
                }
                }

                h2 { class: "bs-sub", "Layout" }
                Setting { label: "Paper".to_string(), hint: String::new(),
                    Dropdown {
                        value: options().paper.name.to_string(),
                        options: aop_core::pdf::PAPERS.iter().map(|p| Choice::plain(p.name)).collect(),
                        width: 0.0, large: true, disabled: false,
                        on_pick: move |value: String| {
                            if let Some(paper) = aop_core::pdf::PAPERS.iter().find(|p| p.name == value) {
                                options.write().paper = *paper;
                            }
                        },
                    }
                }
                Setting { label: "Orientation".to_string(), hint: String::new(),
                    Dropdown {
                        value: options().orientation.label().to_string(),
                        options: aop_core::pdf::Orientation::ORDER
                            .iter()
                            .map(|o| Choice::plain(o.label()))
                            .collect(),
                        width: 0.0, large: true, disabled: false,
                        on_pick: move |value: String| {
                            let picked = aop_core::pdf::Orientation::ORDER
                                .into_iter()
                                .find(|o| o.label() == value)
                                .unwrap_or_default();
                            options.write().orientation = picked;
                        },
                    }
                }
                Setting { label: "Margin".to_string(), hint: "Millimetres".to_string(),
                    input {
                        class: "bs-input",
                        style: "max-width: 90px;",
                        value: "{options().margin_mm}",
                        oninput: move |event| {
                            if let Ok(value) = event.value().parse::<f32>() {
                                options.write().margin_mm = value.clamp(0.0, 50.0);
                            }
                        },
                    }
                }

                h2 { class: "bs-sub", "Include" }
                OptCheck { label: "Gantt chart".to_string(), on_state: options().include_chart,
                    on: move |_| { let on = options().include_chart; options.write().include_chart = !on; } }
                OptCheck { label: "Task table".to_string(), on_state: options().include_table,
                    on: move |_| { let on = options().include_table; options.write().include_table = !on; } }
                OptCheck { label: "Resource sheet".to_string(), on_state: options().include_resources,
                    on: move |_| { let on = options().include_resources; options.write().include_resources = !on; } }
                OptCheck { label: "Mark the critical path".to_string(), on_state: options().show_critical,
                    on: move |_| { let on = options().show_critical; options.write().show_critical = !on; } }

                div { class: "print-fact", style: "margin-top: 14px;",
                    span { class: "pf-label", "Document" }
                    span { class: "pf-value", "{pages} page(s) \u{00b7} {size_kb:.0} KB" }
                }
            }
        }
    }
}

// ----------------------------------------------------------------- Export

#[component]
fn ExportPage() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let base = {
        let s = state.read();
        s.file_path
            .as_ref()
            .map(|p| p.with_extension(""))
            .unwrap_or_else(|| documents_dir().join(s.project.name.clone()))
    };

    let mut excel_target = use_signal(|| format!("{}.xlsx", base.display()));
    let mut csv_target = use_signal(|| format!("{}.csv", base.display()));
    let mut html_target = use_signal(|| format!("{}.html", base.display()));

    rsx! {
        h1 { class: "bs-title", "Export" }
        Confirmation {}

        h2 { class: "bs-sub", "Excel workbook" }
        div { class: "hint", style: "margin: 0 0 10px;",
            "Two sheets: the task table with its outline kept as indentation, and the resources. It reads back in, so a workbook sent out for comment can be brought home." }
        div { class: "bs-field",
            label { "Save to" }
            input { class: "bs-input", value: "{excel_target}",
                oninput: move |event| excel_target.set(event.value()) }
            button { class: "btn primary",
                onclick: move |_| state.write().export_excel_to(PathBuf::from(excel_target())),
                "Export workbook"
            }
        }

        h2 { class: "bs-sub", "CSV" }
        div { class: "hint", style: "margin: 0 0 10px;",
            "The task table with outline, dates, predecessors, resources, work, cost and the critical path flag." }
        div { class: "bs-field",
            label { "Save to" }
            input { class: "bs-input", value: "{csv_target}", oninput: move |event| csv_target.set(event.value()) }
            button { class: "btn primary",
                onclick: move |_| state.write().export_csv_to(PathBuf::from(csv_target())),
                "Export CSV"
            }
        }

        h2 { class: "bs-sub", "Web page" }
        div { class: "hint", style: "margin: 0 0 10px;",
            "A standalone page with the chart drawn as SVG and the full task table. Opens in any browser." }
        div { class: "bs-field",
            label { "Save to" }
            input { class: "bs-input", value: "{html_target}", oninput: move |event| html_target.set(event.value()) }
            button { class: "btn primary",
                onclick: move |_| state.write().export_html_to(PathBuf::from(html_target())),
                "Export page"
            }
        }

        h2 { class: "bs-sub", "Project file" }
        div { class: "hint", style: "margin: 0 0 10px;",
            "Use Save As to write a .{persist::FILE_EXTENSION} file that this app can reopen." }
        button { class: "btn", onclick: move |_| state.write().backstage = Some(BackstagePage::SaveAs), "Go to Save As" }
    }
}

// ------------------------------------------------------------------ About

/// One credited dependency: what it is called, its licence, and where it lives.
type Attribution = (&'static str, &'static str, &'static str);

/// Attribution rows, grouped the way the Dunespan About panel groups them.
const ATTRIBUTIONS: [(&str, &[Attribution]); 5] = [
    (
        "Application",
        &[
            ("Dioxus", "MIT / Apache-2.0", "https://github.com/DioxusLabs/dioxus"),
            ("wry", "MIT / Apache-2.0", "https://github.com/tauri-apps/wry"),
            ("tao", "MIT / Apache-2.0", "https://github.com/tauri-apps/tao"),
            ("ureq", "MIT / Apache-2.0", "https://github.com/algesten/ureq"),
            ("sha2", "MIT / Apache-2.0", "https://github.com/RustCrypto/hashes"),
        ],
    ),
    (
        "Scheduling core",
        &[
            ("chrono", "MIT / Apache-2.0", "https://github.com/chronotope/chrono"),
            ("serde", "MIT / Apache-2.0", "https://github.com/serde-rs/serde"),
            ("serde_json", "MIT / Apache-2.0", "https://github.com/serde-rs/json"),
            ("rmp-serde", "MIT", "https://github.com/3Hren/msgpack-rust"),
            ("flate2", "MIT / Apache-2.0", "https://github.com/rust-lang/flate2-rs"),
        ],
    ),
    (
        "Reading and writing files",
        &[
            ("alterion-mpp-parser", "Apache-2.0", "https://gitlab.com/alterion-software/alterion-mpp-parser"),
            ("cfb", "MIT", "https://github.com/mdsteele/rust-cfb"),
            ("quick-xml", "MIT", "https://github.com/tafia/quick-xml"),
            ("calamine", "MIT", "https://github.com/tafia/calamine"),
            ("rust_xlsxwriter", "MIT / Apache-2.0", "https://github.com/jmcnamara/rust_xlsxwriter"),
            ("pdf-writer", "MIT / Apache-2.0", "https://github.com/typst/pdf-writer"),
        ],
    ),
    (
        "Artwork",
        &[(
            "Lucide",
            "ISC",
            "https://github.com/lucide-icons/lucide",
        )],
    ),
    (
        "Dictionaries",
        &[(
            "LibreOffice dictionaries",
            "downloaded on request, licensed per language",
            "https://github.com/LibreOffice/dictionaries",
        )],
    ),
];

#[component]
fn AboutPage() -> Element {
    let mut show_attributions = use_signal(|| false);
    let year = chrono::Local::now().year();
    let (logo_w, logo_h) = crate::brand::LOGO_VIEWBOX;
    let logo_height = 210.0 * logo_h / logo_w;

    rsx! {
        div { class: "about-wrap",
        div { class: "about-card",

            // ---- brand header -------------------------------------------
            div { class: "about-brand",
                div {
                    class: "about-logo",
                    style: "width: 260px; height: {logo_height * 260.0 / 210.0}px;",
                    dangerous_inner_html: crate::brand::LOGO_SVG,
                }
                div { class: "about-name", "Open Project" }
                div { class: "about-pills",
                    span { class: "pill accent", "v{env!(\"CARGO_PKG_VERSION\")}" }
                    span { class: "pill", "{persist::FILE_TYPE_NAME}" }
                    span { class: "pill", "Rust \u{00b7} Dioxus" }
                }
            }

            // ---- details -------------------------------------------------
            div { class: "about-rows",
                for (label, value) in [
                    ("Product", APP_NAME.to_string()),
                    ("Version", format!("v{}", env!("CARGO_PKG_VERSION"))),
                    ("File format", format!(".{} ({})", persist::FILE_EXTENSION, persist::FILE_TYPE_NAME)),
                    ("Engine", "Critical path method, forward and backward pass".to_string()),
                    ("Calendars", "Working time with shifts, weekends and holidays".to_string()),
                    ("\u{00a9}", format!("{year} Alterion. All rights reserved.")),
                ] {
                    div { key: "{label}", class: "about-row",
                        span { class: "k", "{label}" }
                        span { class: "v", "{value}" }
                    }
                }
            }

            button {
                class: "about-attr-btn",
                onclick: move |_| show_attributions.set(true),
                {icon("package-mono", 15)}
                span { "Open Source Attributions" }
            }
        }
        }

        if show_attributions() {
            div {
                class: "scrim",
                onclick: move |_| show_attributions.set(false),
                div {
                    class: "dlg",
                    style: "min-width: 560px;",
                    onclick: move |event| event.stop_propagation(),
                    div { class: "dlg-head",
                        span { style: "display: flex; align-items: center; gap: 9px;",
                            span { style: "display: grid; place-items: center;",
                                {icon("package-mono", 16)} }
                            span { "Open Source Attributions" }
                        }
                        button { class: "dlg-close",
                            onclick: move |_| show_attributions.set(false),
                            "\u{2715}"
                        }
                    }
                    div { class: "dlg-body",
                        for (heading, items) in ATTRIBUTIONS {
                            div { key: "{heading}", style: "margin-bottom: 18px;",
                                h3 { style: "font-size: 12px; color: var(--accent); margin: 0 0 8px; letter-spacing: 0.3px;",
                                    "{heading}" }
                                for (name, license, url) in items.iter() {
                                    div { key: "{name}", class: "attr-row",
                                        span { class: "attr-name", "{name}" }
                                        span { class: "attr-license", "{license}" }
                                        span { class: "attr-url", "{url}" }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "dlg-foot",
                        button { class: "btn primary",
                            onclick: move |_| show_attributions.set(false),
                            "Close"
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- Options

/// One labelled setting row.
#[component]
pub fn Setting(label: String, hint: String, children: Element) -> Element {
    rsx! {
        div { class: "opt-row",
            div { class: "opt-label",
                span { "{label}" }
                if !hint.is_empty() {
                    span { class: "opt-hint", "{hint}" }
                }
            }
            div { class: "opt-control", {children} }
        }
    }
}

#[component]
pub fn OptCheck(label: String, on_state: bool, on: EventHandler<()>) -> Element {
    let box_class = if on_state { "box on" } else { "box" };
    rsx! {
        div { class: "rcheck", style: "height: 26px;", onclick: move |_| on.call(()),
            span { class: "{box_class}", if on_state { "\u{2713}" } }
            span { "{label}" }
        }
    }
}

#[component]
fn OptionsPageView() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let page = state.read().options_page;

    rsx! {
        h1 { class: "bs-title", "Options" }

        div { class: "opt-layout",
            nav { class: "opt-nav",
                for entry in OptionsPage::ORDER {
                    {
                        let class = if entry == page { "opt-nav-item active" } else { "opt-nav-item" };
                        rsx! {
                            button { key: "{entry:?}", class: "{class}",
                                onclick: move |_| state.write().options_page = entry,
                                "{entry.label()}"
                            }
                        }
                    }
                }
            }

            section { class: "opt-body",
                match page {
                    OptionsPage::General => rsx! { OptGeneral {} },
                    OptionsPage::Display => rsx! { OptDisplay {} },
                    OptionsPage::Schedule => rsx! { OptSchedule {} },
                    OptionsPage::Save => rsx! { OptSave {} },
                    OptionsPage::Advanced => rsx! { OptAdvanced {} },
                    OptionsPage::Keyboard => rsx! { OptKeyboard {} },
                    OptionsPage::CustomizeRibbon => rsx! { OptRibbon {} },
                    OptionsPage::QuickAccess => rsx! { OptQuickAccess {} },
                }
            }
        }
    }
}

#[component]
fn OptGeneral() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (name, initials, default_view) = {
        let s = state.read();
        (s.user_name.clone(), s.user_initials.clone(), s.default_view)
    };

    rsx! {
        h2 { class: "opt-head", "Personalize your copy of Alterion Open Project" }
        Setting { label: "User name".to_string(), hint: "Stored as the project author".to_string(),
            input { class: "bs-input", value: "{name}",
                oninput: move |event| {
                    let value = event.value();
                    let mut writer = state.write();
                    writer.project.author = value.clone();
                    writer.user_name = value;
                },
            }
        }
        Setting { label: "Initials".to_string(), hint: String::new(),
            input { class: "bs-input", style: "max-width: 110px;", value: "{initials}",
                oninput: move |event| state.write().user_initials = event.value(),
            }
        }
        Setting { label: "Company".to_string(), hint: String::new(),
            input { class: "bs-input", value: "{state.read().project.company}",
                oninput: move |event| state.write().project.company = event.value(),
            }
        }

        h2 { class: "opt-head", "Project view" }
        Setting { label: "Default view".to_string(), hint: "Opened when a plan is created".to_string(),
            Dropdown {
                value: format!("{default_view:?}"),
                options: vec![
                    Choice::new("GanttChart", "Gantt Chart"),
                    Choice::new("TrackingGantt", "Tracking Gantt"),
                    Choice::new("TaskSheet", "Task Sheet"),
                    Choice::new("TaskUsage", "Task Usage"),
                    Choice::new("NetworkDiagram", "Network Diagram"),
                    Choice::new("CalendarView", "Calendar"),
                    Choice::new("ResourceSheet", "Resource Sheet"),
                ],
                width: 0.0, large: true, disabled: false,
                on_pick: move |value: String| {
                    let kind = match value.as_str() {
                        "TrackingGantt" => ViewKind::TrackingGantt,
                        "TaskSheet" => ViewKind::TaskSheet,
                        "TaskUsage" => ViewKind::TaskUsage,
                        "NetworkDiagram" => ViewKind::NetworkDiagram,
                        "CalendarView" => ViewKind::CalendarView,
                        "ResourceSheet" => ViewKind::ResourceSheet,
                        _ => ViewKind::GanttChart,
                    };
                    let mut writer = state.write();
                    writer.default_view = kind;
                    writer.view = kind;
                },
            }
        }
    }
}

#[component]
fn OptDisplay() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (currency, date_format, outline, summary, timeline, theme) = {
        let s = state.read();
        (
            s.project.currency_symbol.clone(),
            s.date_format,
            s.show_outline_number,
            s.project.show_project_summary,
            s.show_timeline,
            s.theme,
        )
    };
    let sample = crate::state::DATE_FORMATS
        .get(date_format)
        .map(|f| f.0)
        .unwrap_or("Mon 17/08/26");

    rsx! {
        h2 { class: "opt-head", "Appearance" }
        Setting {
            label: "Theme".to_string(),
            hint: "System follows the desktop and changes with it".to_string(),
            Dropdown {
                value: theme.label().to_string(),
                options: crate::theme::ThemeChoice::ORDER
                    .iter()
                    .map(|choice| Choice::plain(choice.label()))
                    .collect(),
                width: 0.0, large: true, disabled: false,
                on_pick: move |value: String| {
                    state.write().theme = crate::theme::ThemeChoice::from_label(&value);
                },
            }
        }

        h2 { class: "opt-head", "Calendar and currency" }
        Setting { label: "Currency symbol".to_string(), hint: "Used by cost columns and reports".to_string(),
            input { class: "bs-input", style: "max-width: 110px;", value: "{currency}",
                oninput: move |event| state.write().project.currency_symbol = event.value(),
            }
        }
        Setting { label: "Date format".to_string(), hint: "Applies to every date column".to_string(),
            Dropdown {
                value: sample.to_string(),
                options: crate::state::DATE_FORMATS.iter().map(|f| Choice::plain(f.0)).collect(),
                width: 0.0, large: true, disabled: false,
                on_pick: move |value: String| {
                    if let Some(index) = crate::state::DATE_FORMATS.iter().position(|f| f.0 == value) {
                        state.write().date_format = index;
                        crate::state::set_date_format(index);
                    }
                },
            }
        }

        h2 { class: "opt-head", "Show these elements" }
        OptCheck { label: "Outline numbers in the Task Name column".to_string(), on_state: outline,
            on: move |_| { let on = state.read().show_outline_number; state.write().show_outline_number = !on; } }
        OptCheck { label: "Project summary task".to_string(), on_state: summary,
            on: move |_| {
                let on = state.read().project.show_project_summary;
                state.write().project.show_project_summary = !on;
            } }
        OptCheck { label: "Timeline band".to_string(), on_state: timeline,
            on: move |_| { let on = state.read().show_timeline; state.write().show_timeline = !on; } }
    }
}

#[component]
fn OptSchedule() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (mode, calendar_name, start) = {
        let s = state.read();
        (
            s.new_tasks_mode,
            s.project.calendar.name.clone(),
            s.project.start_date.format("%Y-%m-%d").to_string(),
        )
    };

    rsx! {
        h2 { class: "opt-head", "Scheduling options for this project" }
        Setting { label: "New tasks created as".to_string(), hint: "Manual tasks ignore their links".to_string(),
            Dropdown {
                value: mode.label().to_string(),
                options: vec![
                    Choice::plain(aop_core::TaskMode::Auto.label()),
                    Choice::plain(aop_core::TaskMode::Manual.label()),
                ],
                width: 0.0, large: true, disabled: false,
                on_pick: move |value: String| {
                    state.write().new_tasks_mode = if value.starts_with("Manually") {
                        aop_core::TaskMode::Manual
                    } else {
                        aop_core::TaskMode::Auto
                    };
                },
            }
        }
        Setting { label: "Project start date".to_string(), hint: String::new(),
            input { class: "bs-input", style: "max-width: 190px;", value: "{start}",
                onchange: move |event| {
                    if let Some(date) = crate::state::parse_date(&event.value()) {
                        state.write().set_project_start(date);
                    }
                },
            }
        }
        Setting { label: "Calendar".to_string(), hint: "Edit it from Project, Change Working Time".to_string(),
            div { style: "display: flex; gap: 8px; align-items: center;",
                input { class: "bs-input", value: "{calendar_name}", readonly: true }
                button { class: "btn",
                    onclick: move |_| {
                        state.write().backstage = None;
                        state.write().dialog = Some(Dialog::ChangeWorkingTime);
                    },
                    "Change Working Time..."
                }
            }
        }

        h2 { class: "opt-head", "Duration units" }
        div { class: "opt-static",
            for (label, value) in [
                ("Hours per day", format!("{}", aop_core::MINUTES_PER_DAY / 60)),
                ("Hours per week", format!("{}", aop_core::MINUTES_PER_WEEK / 60)),
                ("Days per month", format!("{}", aop_core::MINUTES_PER_MONTH / aop_core::MINUTES_PER_DAY)),
            ] {
                div { key: "{label}", class: "opt-static-row",
                    span { "{label}" }
                    span { class: "v", "{value}" }
                }
            }
        }
        div { class: "hint",
            "A duration of one day means this many working hours, not twenty-four hours of wall clock. These are fixed in this build." }
    }
}

#[component]
fn OptSave() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let folder = state.read().default_folder.clone();

    rsx! {
        h2 { class: "opt-head", "Save projects" }
        Setting { label: "Save files in this format".to_string(), hint: String::new(),
            input { class: "bs-input", readonly: true,
                value: "{persist::FILE_TYPE_NAME} (*.{persist::FILE_EXTENSION})" }
        }
        Setting { label: "Default file location".to_string(), hint: "Where Open and Save As start".to_string(),
            input { class: "bs-input", value: "{folder}",
                oninput: move |event| state.write().default_folder = event.value(),
            }
        }
        div { class: "hint",
            "There is no auto-save in this build. The title bar shows an asterisk while a plan has unsaved changes." }
    }
}

#[component]
fn OptAdvanced() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (critical, slack, baseline) = {
        let s = state.read();
        (s.show_critical, s.show_slack, s.show_baseline)
    };

    rsx! {
        h2 { class: "opt-head", "Display options for this project" }
        OptCheck { label: "Show the critical path in red".to_string(), on_state: critical,
            on: move |_| { let on = state.read().show_critical; state.write().show_critical = !on; } }
        OptCheck { label: "Show slack on the chart".to_string(), on_state: slack,
            on: move |_| { let on = state.read().show_slack; state.write().show_slack = !on; } }
        OptCheck { label: "Show baseline bars".to_string(), on_state: baseline,
            on: move |_| { let on = state.read().show_baseline; state.write().show_baseline = !on; } }

        h2 { class: "opt-head", "Undo" }
        div { class: "opt-static",
            div { class: "opt-static-row",
                span { "Levels of undo" }
                span { class: "v", "60" }
            }
        }

        h2 { class: "opt-head", "Reset" }
        button { class: "btn danger",
            onclick: move |_| {
                let mut writer = state.write();
                writer.show_critical = false;
                writer.show_slack = false;
                writer.show_baseline = false;
                writer.show_outline_number = false;
                writer.show_timeline = true;
                writer.date_format = 0;
                crate::state::set_date_format(0);
                writer.note("Display options reset");
            },
            "Reset display options"
        }
    }
}

#[component]
fn OptRibbon() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let collapsed = state.read().ribbon_collapsed;

    rsx! {
        h2 { class: "opt-head", "Customize the Ribbon" }
        OptCheck { label: "Collapse the ribbon".to_string(), on_state: collapsed,
            on: move |_| { let on = state.read().ribbon_collapsed; state.write().ribbon_collapsed = !on; } }

        div { class: "opt-static", style: "margin-top: 14px;",
            for tab in crate::state::RibbonTab::ORDER {
                div { key: "{tab:?}", class: "opt-static-row",
                    span { "{tab.label()} tab" }
                    span { class: "v", "Shown" }
                }
            }
            div { class: "opt-static-row",
                span { "Format tab" }
                span { class: "v", "Contextual" }
            }
        }
        div { class: "hint",
            "Adding and removing individual ribbon commands is not available in this build. The Quick Access Toolbar is fully customizable." }
    }
}

#[component]
fn OptQuickAccess() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let qat = state.read().qat.clone();

    rsx! {
        h2 { class: "opt-head", "Customize the Quick Access Toolbar" }
        div { class: "opt-static",
            for command in qat.iter().copied() {
                div { key: "{command:?}", class: "opt-static-row",
                    span { style: "display: flex; align-items: center; gap: 9px;",
                        span { style: "color: var(--accent); display: grid; place-items: center;",
                            {icon(command.glyph(), 15)} }
                        span { "{command.label()}" }
                    }
                    span { class: "v", "On the toolbar" }
                }
            }
        }
        div { style: "display: flex; gap: 8px; margin-top: 16px;",
            button { class: "btn primary",
                onclick: move |_| {
                    state.write().backstage = None;
                    state.write().dialog = Some(Dialog::CustomizeQat);
                },
                "Customize..."
            }
            button { class: "btn", onclick: move |_| state.write().reset_qat(), "Reset" }
        }
    }
}

// --------------------------------------------------------- keyboard options

/// Every command the keyboard can reach, bound or not.
///
/// Listing the unbound ones is the point: a list of only what is already bound
/// tells you what the keyboard does, not what it could do. The list is
/// generated from the action table, so a command added later turns up here
/// without anyone remembering to add it.
#[component]
fn OptKeyboard() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let keys = state.read().keys.clone();

    // Which action is waiting for a key press, and what the last binding
    // displaced, so the user is told rather than left to discover it.
    let mut recording = use_signal(|| None::<crate::keymap::Action>);
    let mut displaced = use_signal(|| None::<(crate::keymap::Action, crate::keymap::Action)>);

    rsx! {
        div { style: "display: flex; align-items: baseline; justify-content: space-between; gap: 16px;",
            h2 { class: "opt-head", style: "margin: 0;", "Keyboard shortcuts" }
            button {
                class: "btn",
                onclick: move |_| {
                    state.write().keys.reset_all();
                    recording.set(None);
                    displaced.set(None);
                },
                "Reset all"
            }
        }
        div { class: "hint", style: "margin: 6px 0 16px;",
            "Click a shortcut and press the keys you want. A key press already in use is taken from whatever had it." }

        if let Some((taken, from)) = displaced() {
            div { class: "opt-note",
                "{taken.label()} took that key press from {from.label()}, which now has none."
            }
        }

        for group in crate::keymap::Action::GROUPS {
            div { key: "{group}", class: "key-group",
                h2 { class: "opt-head", "{group}" }
                div { class: "key-list",
                    for action in crate::keymap::Action::ALL.iter().copied().filter(|a| a.group() == group) {
                        {
                            let binding = keys.binding(action);
                            let listening = recording() == Some(action);
                            let row_class = if listening { "key-row recording" } else { "key-row" };
                            rsx! {
                                div { key: "{action:?}", class: "{row_class}",
                                    span { class: "key-name", "{action.label()}" }
                                    if keys.is_customised(action) {
                                        span { class: "key-changed", title: "Changed from the default", "changed" }
                                    }

                                    button {
                                        class: "key-bind",
                                        // The press is read here rather than by
                                        // the global handler, which is exactly
                                        // what lets a key that is already in use
                                        // be captured instead of running.
                                        onkeydown: move |event| {
                                            if recording() != Some(action) {
                                                return;
                                            }
                                            event.prevent_default();
                                            if event.key() == Key::Escape {
                                                recording.set(None);
                                                return;
                                            }
                                            let pressed = crate::keymap::shortcut_for(
                                                &event.key(),
                                                event.modifiers(),
                                            );
                                            if let Some(pressed) = pressed {
                                                let taken = state.write().keys.bind(action, &pressed);
                                                displaced.set(taken.map(|from| (action, from)));
                                                recording.set(None);
                                            }
                                        },
                                        onclick: move |_| {
                                            displaced.set(None);
                                            recording.set(Some(action));
                                        },
                                        if listening {
                                            span { class: "key-listening", "Press keys, Escape to cancel" }
                                        } else if let Some(binding) = binding.clone() {
                                            span { class: "key-combo", "{binding}" }
                                        } else {
                                            span { class: "key-none", "Not assigned" }
                                        }
                                    }

                                    button {
                                        class: "key-clear",
                                        title: "Remove this shortcut",
                                        onclick: move |_| {
                                            state.write().keys.clear(action);
                                            recording.set(None);
                                        },
                                        {icon("x", 13)}
                                    }
                                    button {
                                        class: "key-clear",
                                        title: "Put the default back",
                                        onclick: move |_| {
                                            state.write().keys.reset(action);
                                            recording.set(None);
                                        },
                                        {icon("undo", 13)}
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
