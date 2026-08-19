//! The File menu: the start dashboard, template gallery, file browser, print
//! preview, export, and the Info page that doubles as a project dashboard.

use std::path::PathBuf;

use dioxus::prelude::*;

use aop_core::sheet::{DateOrder, Field as SheetField, Mapping, Report, Sheet};
use aop_core::{format_work, persist, templates, Project, APP_NAME};
use chrono::Datelike;

use crate::icons::icon;
use crate::preview::{mini_gantt, DARK};
use crate::controls::{Choice, Dropdown};
use crate::state::{
    documents_dir, format_date_long, next_monday, AppState, BackstagePage, Dialog, OptionsPage,
    PendingAction, ViewKind, Working,
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
                    BackstagePage::Import,
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
                    BackstagePage::Import => rsx! { ImportPage {} },
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
///
/// `on_pick` is how the Import page borrows it: with a handler the browser
/// hands the path over instead of opening it, and the page it sits in supplies
/// its own heading. Sharing the browser is the point, so that a file visible
/// on the Open page is visible on the Import page as well.
///
/// `accept` narrows the list to particular extensions for a caller that wants
/// something Open cannot open, such as a holiday calendar. Change Working Time
/// borrows it that way, which is why this is reachable outside this module.
#[component]
pub(crate) fn FileBrowser(
    saving: bool,
    on_pick: Option<EventHandler<PathBuf>>,
    accept: Option<Vec<String>>,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let embedded = on_pick.is_some();

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
                let offered = match &accept {
                    Some(wanted) => path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| {
                            wanted.iter().any(|want| extension.eq_ignore_ascii_case(want))
                        }),
                    None => crate::state::offered_in_browser(&path, saving),
                };
                if path.is_dir() {
                    folders.push((name, path));
                } else if offered {
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
        let target = dir().join(name);
        // Ask before writing over somebody's file. `save_to` writes without
        // looking, so Save As silently replaced whatever was already there,
        // which is the kind of loss nobody notices until they need the file.
        if target.exists() {
            let beside = crate::state::free_name_beside(&target).unwrap_or_else(|| target.clone());
            state.write().dialog = Some(Dialog::ConfirmOverwrite { path: target, beside });
            return;
        }
        state.write().save_to(target);
    };

    rsx! {
        if !embedded {
            h1 { class: "bs-title", "{title}" }
            Confirmation {}
        }

        if !saving && !embedded && !recent.is_empty() {
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

        if !embedded {
            h2 { class: "bs-sub", "Browse" }
        }
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
            // Home, because the folder box can be typed into and walked out
            // of, and there should always be one press back to somewhere with
            // plans in it.
            button { class: "btn",
                onclick: move |_| dir.set(crate::state::documents_dir()),
                "Home"
            }
            // Empty on any platform with a single root. On Windows the parent
            // of `C:\` is nothing, so without these a plan on another drive
            // cannot be navigated to at all.
            for (label, root) in crate::state::browser_roots() {
                button { class: "btn",
                    onclick: move |_| dir.set(root.clone()),
                    "{label}"
                }
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
                                if let Some(pick) = on_pick {
                                    pick.call(target.clone());
                                } else if saving {
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

        if !embedded {
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
    let mut shown_page = use_signal(|| 1usize);
    let mut target = use_signal(|| {
        let s = state.read();
        s.file_path
            .as_ref()
            .map(|p| p.with_extension("pdf"))
            .unwrap_or_else(|| documents_dir().join(format!("{}.pdf", s.project.name)))
            .display()
            .to_string()
    });

    // What the whole plan makes at these settings, which is what a range has
    // to be chosen against: "pages 2 to 4" means nothing without knowing there
    // are seven.
    let pages = {
        let s = state.read();
        aop_core::pdf::page_count(&s.project, &options())
    };
    // A range past the end of a shorter document is not an error, it just
    // stops early, so the shown page is pulled back rather than left dangling.
    let showing = shown_page().clamp(1, pages.max(1));
    if showing != shown_page() {
        shown_page.set(showing);
    }

    // What will actually be sent, honouring the chosen range.
    let document = {
        let s = state.read();
        aop_core::pdf::render(&s.project, &options())
    };
    let size_kb = document.len() as f64 / 1024.0;

    // The preview shows one page at a time, the way Project's does, so it is
    // rendered on its own rather than handing the reader a whole document to
    // scroll. It comes from the same writer as the print, so the two cannot
    // drift.
    let preview_src = {
        let s = state.read();
        let single = aop_core::pdf::PrintOptions {
            pages: Some((showing as u32, showing as u32)),
            ..options()
        };
        format!(
            "data:application/pdf;base64,{}",
            crate::spooler::base64(&aop_core::pdf::render(&s.project, &single))
        )
    };

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

                // Paging through the document, rather than scrolling a whole
                // one, so what is on screen is one printed sheet.
                div { class: "print-pager",
                    button {
                        class: "btn", disabled: showing <= 1,
                        onclick: move |_| shown_page.set(1),
                        "\u{00ab}"
                    }
                    button {
                        class: "btn", disabled: showing <= 1,
                        onclick: move |_| shown_page.set(showing.saturating_sub(1).max(1)),
                        "\u{2039}"
                    }
                    span { class: "print-pager-at", "Page {showing} of {pages}" }
                    button {
                        class: "btn", disabled: showing >= pages,
                        onclick: move |_| shown_page.set((showing + 1).min(pages)),
                        "\u{203a}"
                    }
                    button {
                        class: "btn", disabled: showing >= pages,
                        onclick: move |_| shown_page.set(pages.max(1)),
                        "\u{00bb}"
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

                h2 { class: "bs-sub", "Settings" }
                Dropdown {
                    value: (if options().pages.is_some() { "some" } else { "all" }).to_string(),
                    options: vec![
                        Choice::new("all", "Print the entire project"),
                        Choice::new("some", "Print specific pages"),
                    ],
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        let mut chosen = options();
                        // Switching to a range starts at the whole document
                        // rather than at whatever was typed and abandoned.
                        chosen.pages = (picked == "some").then_some((1, pages.max(1) as u32));
                        options.set(chosen);
                    },
                }
                if let Some((from, to)) = options().pages {
                    div { class: "print-range",
                        label { "Pages" }
                        input {
                            r#type: "number", min: "1",
                            value: "{from}",
                            onchange: move |event| {
                                let wanted = event.value().trim().parse::<u32>().unwrap_or(1);
                                let mut chosen = options();
                                chosen.pages = Some((wanted.max(1), to));
                                options.set(chosen);
                            },
                        }
                        span { class: "unit", "to" }
                        input {
                            r#type: "number", min: "1",
                            value: "{to}",
                            onchange: move |event| {
                                let wanted = event.value().trim().parse::<u32>().unwrap_or(1);
                                let mut chosen = options();
                                chosen.pages = Some((from, wanted.max(1)));
                                options.set(chosen);
                            },
                        }
                    }
                    div { class: "hint",
                        "Out of {pages}. A range the wrong way round or past the end is read as what was meant rather than refused."
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
    let mut project_xml_target = use_signal(|| format!("{}.xml", base.display()));
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

        h2 { class: "bs-sub", "Microsoft Project XML" }
        div { class: "hint", style: "margin: 0 0 10px;",
            "The whole plan in Microsoft's own interchange format: the outline, links with their lag, constraints, deadlines, resources, assignments and the calendars they are worked to. Microsoft Project opens it with File then Open, and can save it as .mpp itself." }
        div { class: "bs-field",
            label { "Save to" }
            input { class: "bs-input", value: "{project_xml_target}",
                oninput: move |event| project_xml_target.set(event.value()) }
            button { class: "btn primary",
                onclick: move |_| export_project_xml(&mut state, PathBuf::from(project_xml_target())),
                "Export Project XML"
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

/// Write the plan as Microsoft Project XML.
///
/// Reporting matches the other exports: a line on the page when it worked, a
/// dialog when it did not. The plan is only borrowed for as long as the
/// document takes to build, so the write itself is not holding the state open.
fn export_project_xml(state: &mut Signal<AppState>, path: PathBuf) {
    let path = path.with_extension("xml");
    let written = {
        let app = state.read();
        aop_core::mspdi_write::save(&path, &app.project)
    };
    let mut app = state.write();
    match written {
        Ok(()) => {
            let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            app.status = format!("Exported {}", path.display());
            app.backstage_message = Some(format!("Saved {} ({bytes} bytes)", path.display()));
        }
        Err(error) => {
            app.backstage_message = None;
            app.dialog = Some(Dialog::Message {
                title: "Could not export".into(),
                body: format!("{}: {error}", path.display()),
            });
        }
    }
}

// ----------------------------------------------------------------- Import

/// The Import page: a plan somebody else wrote, as a spreadsheet.
///
/// Holiday calendars used to sit here too and no longer do. A `.ics` is not a
/// plan, and importing one is an edit to a calendar rather than a new document,
/// so it belongs where calendars are edited: Change Working Time, where it can
/// also be aimed at one person rather than at everybody.
#[component]
fn ImportPage() -> Element {
    rsx! {
        h1 { class: "bs-title", "Import" }
        Confirmation {}
        SpreadsheetImport {}
    }
}

/// How many rows of real data are shown under each heading.
const SAMPLES: usize = 3;

#[component]
fn SpreadsheetImport() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let mut source = use_signal(|| Option::<PathBuf>::None);
    let mut sheets = use_signal(Vec::<Sheet>::new);
    let mut which = use_signal(|| 0usize);
    let mut mapping = use_signal(|| Option::<Mapping>::None);
    let mut trouble = use_signal(|| Option::<String>::None);
    // Whether to draw a card for every column of the sheet.
    //
    // Off by default, and that is not a detail of the drawing. A plan kept in
    // a spreadsheet often has a weekly timeline drawn across it, which is
    // dozens of columns of date headings and no data: this file runs out to
    // column CU. Somebody mapping a sheet cares about the handful of columns
    // that mean something, and putting a hundred cards in front of them buries
    // those columns in noise as well as costing a dropdown and three rows of
    // sampled data each, every time anything on the page changes.
    let mut every_column = use_signal(|| false);

    let plan_name = source()
        .as_ref()
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported plan".to_string());

    // Read once, on the click. Every later change of sheet, heading row or
    // column works on what is already in memory, so correcting a guess never
    // waits on the disk.
    let mut choose = move |path: PathBuf| {
        trouble.set(None);
        match aop_core::sheet::survey(&path) {
            Ok(found) => {
                // The sheet most likely to be the plan is the first one this
                // can find a name column in. A workbook usually opens on a
                // cover sheet, and the plan is behind it.
                let best = found
                    .iter()
                    .position(|sheet| {
                        Mapping::guess(sheet).column_of(SheetField::Name).is_some()
                    })
                    .unwrap_or(0);
                let guess = found.get(best).map(Mapping::guess);
                sheets.set(found);
                which.set(best);
                mapping.set(guess);
                source.set(Some(path));
            }
            Err(error) => {
                source.set(None);
                sheets.set(Vec::new());
                mapping.set(None);
                trouble.set(Some(error.to_string()));
            }
        }
    };

    let mut show_sheet = move |index: usize| {
        which.set(index);
        let guess = sheets.read().get(index).map(Mapping::guess);
        mapping.set(guess);
    };

    // Correcting the heading row re-reads the headings under it, since the
    // whole point of moving it is that the old row was not the headings.
    let mut use_heading_row = move |row: usize| {
        let rebuilt = {
            let held = sheets.read();
            let Some(sheet) = held.get(which()) else {
                return;
            };
            let mut map = mapping.read().clone().unwrap_or_else(|| Mapping::guess(sheet));
            map.heading_row = row;
            map.columns = aop_core::sheet::guess_columns(sheet, row);
            map
        };
        mapping.set(Some(rebuilt));
    };

    // Worked out from what is chosen, not from what is on screen, so it costs
    // nothing until something actually changes.
    let named = plan_name.clone();
    let outcome = use_memo(move || {
        let map = mapping.read().clone()?;
        let held = sheets.read();
        let sheet = held.get(which())?;
        Some(
            aop_core::sheet::read(sheet, &map, &named)
                .map(|import| import.report)
                .map_err(|error| error.to_string()),
        )
    });

    let chosen = source();
    let Some(path) = chosen else {
        return rsx! {
            h2 { class: "bs-sub", "Spreadsheet" }
            div { class: "hint", style: "margin: 0 0 10px;",
                "A plan somebody keeps in a spreadsheet of their own: any headings, any column order, "
                "and whatever else is in the file. Nothing is imported until the mapping is right and you say so."
            }
            if let Some(message) = trouble() {
                div { class: "info-alert",
                    span { class: "fix-icon", {icon("warning", 18)} }
                    div { style: "flex: 1;", "{message}" }
                }
            }
            FileBrowser { saving: false, on_pick: move |path: PathBuf| choose(path) }
        };
    };

    let held = sheets.read();
    let Some(sheet) = held.get(which()) else {
        return rsx! {};
    };
    let Some(map) = mapping.read().clone() else {
        return rsx! {};
    };

    let heading_row = map.heading_row;
    let dates_mapped = map.column_of(SheetField::Start).is_some()
        || map.column_of(SheetField::Finish).is_some();
    let evidence = map.evidence(sheet);

    // The first rows, so the heading row can be picked out by eye rather than
    // counted. Blank rows are skipped: nobody chooses one.
    let candidates: Vec<(usize, String)> = sheet
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.iter().any(|cell| !cell.is_empty()))
        .take(12)
        .map(|(index, row)| {
            let text = row
                .iter()
                .take(8)
                .map(|cell| cell.text())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join("  |  ");
            (index, text)
        })
        .collect();

    let placed = map.columns.iter().filter(|field| field.is_some()).count();
    // Nothing mapped means nothing to show, and an empty step with a button
    // under it is worse than the long list. That only happens on a sheet this
    // could make no sense of at all, which is exactly when every column is
    // worth looking at.
    let all_columns = every_column() || placed == 0;
    let shown: Vec<usize> = (0..sheet.width)
        .filter(|column| {
            all_columns || map.columns.get(*column).copied().flatten().is_some()
        })
        .collect();
    let hidden = sheet.width.saturating_sub(shown.len());
    // Sampled here rather than for the whole sheet, which is the other half of
    // the saving: reading three values out of a column means walking it until
    // three turn up, and an empty column is walked to the end.
    let columns: Vec<(usize, String, String, Vec<String>)> = shown
        .into_iter()
        .map(|column| {
            (
                column,
                sheet.heading(heading_row, column),
                map.columns
                    .get(column)
                    .copied()
                    .flatten()
                    .map(|field| field.code().to_string())
                    .unwrap_or_default(),
                sheet.samples(column, heading_row + 1, SAMPLES),
            )
        })
        .collect();

    let mut field_choices = vec![Choice::new("", "Leave this column out")];
    field_choices.extend(
        SheetField::ALL
            .iter()
            .map(|field| Choice::new(field.code(), field.label())),
    );

    let sheet_choices: Vec<Choice> = held
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            Choice::new(
                index.to_string(),
                format!("{} ({} rows)", sheet.name, sheet.rows.len()),
            )
        })
        .collect();
    let sheet_count = held.len();
    let file = path.display().to_string();

    rsx! {
        h2 { class: "bs-sub", "Spreadsheet" }
        div { class: "bs-field",
            label { "File" }
            input { class: "bs-input", value: "{file}", disabled: true }
            button { class: "btn",
                onclick: move |_| {
                    source.set(None);
                    sheets.set(Vec::new());
                    mapping.set(None);
                },
                "Choose another"
            }
        }

        if sheet_count > 1 {
            div { class: "bs-field",
                label { "Sheet" }
                Dropdown {
                    value: which().to_string(),
                    options: sheet_choices,
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        if let Ok(index) = picked.parse::<usize>() {
                            show_sheet(index);
                        }
                    },
                }
            }
        }

        h3 { class: "imp-step", "The heading row" }
        div { class: "hint", style: "margin: 0 0 8px;",
            "Guessed from the first row that reads like headings. Title blocks and logos sit above it more often than not."
        }
        div { class: "imp-rows",
            for (index, text) in candidates {
                {
                    let class = if index == heading_row { "imp-row on" } else { "imp-row" };
                    rsx! {
                        button { key: "r{index}", class: "{class}",
                            onclick: move |_| use_heading_row(index),
                            span { class: "imp-rownum", "Row {index + 1}" }
                            span { class: "imp-rowtext", "{text}" }
                        }
                    }
                }
            }
        }

        h3 { class: "imp-step", "The columns" }
        div { class: "hint", style: "margin: 0 0 8px;",
            "Every guess can be changed, and a column left out changes nothing in the plan. The rows under each heading are the file's own."
        }
        if hidden > 0 || every_column() {
            div { class: "bs-field",
                button { class: "btn",
                    onclick: move |_| {
                        let showing = every_column();
                        every_column.set(!showing);
                    },
                    if all_columns {
                        "Show only the columns with a guess"
                    } else {
                        "Show all {sheet.width} columns"
                    }
                }
                if hidden > 0 {
                    span { class: "hint",
                        "{hidden} column(s) this made nothing of are not shown. A timeline drawn across the sheet lands here."
                    }
                }
            }
        }
        div { class: "imp-cols",
            for (column, heading, code, samples) in columns {
                {
                    let class = if code.is_empty() { "imp-col" } else { "imp-col on" };
                    let choices = field_choices.clone();
                    rsx! {
                        div { key: "c{column}", class: "{class}",
                            div { class: "imp-head", title: "{heading}", "{heading}" }
                            Dropdown {
                                value: code.clone(),
                                options: choices,
                                width: 0.0, large: false, disabled: false,
                                on_pick: move |picked: String| {
                                    let mut writer = mapping.write();
                                    if let Some(map) = writer.as_mut() {
                                        map.assign(column, SheetField::from_code(&picked));
                                    }
                                },
                            }
                            div { class: "imp-samples",
                                if samples.is_empty() {
                                    div { class: "imp-sample", "(no data)" }
                                }
                                for (row, value) in samples.iter().enumerate() {
                                    div { key: "s{row}", class: "imp-sample", "{value}" }
                                }
                            }
                        }
                    }
                }
            }
        }

        if dates_mapped {
            h3 { class: "imp-step", "Dates" }
            div { class: "bs-field",
                label { "Read 12/03/2026 as" }
                Dropdown {
                    value: map.date_order.code().to_string(),
                    options: DateOrder::ALL
                        .iter()
                        .map(|order| Choice::new(order.code(), order.label()))
                        .collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        let mut writer = mapping.write();
                        if let Some(map) = writer.as_mut()
                            && let Some(order) = DateOrder::from_code(&picked)
                        {
                            map.date_order = order;
                        }
                    },
                }
            }
            div { class: "hint", style: "margin: 0 0 10px;",
                if evidence.contradictory() {
                    "This sheet contradicts itself: some dates can only be day first and others can only be month first. Whichever you choose, some of them will come in wrong, so it is worth a look at the file."
                } else if evidence.proves_day_first > 0 {
                    "{evidence.proves_day_first} date(s) in this sheet can only be day first, so that is what it was written in. "
                    "{evidence.ambiguous} more could be read either way and follow the setting above."
                } else if evidence.proves_month_first > 0 {
                    "{evidence.proves_month_first} date(s) in this sheet can only be month first, so that is what it was written in. "
                    "{evidence.ambiguous} more could be read either way and follow the setting above."
                } else if evidence.ambiguous > 0 {
                    "Nothing in this sheet settles the question: all {evidence.ambiguous} of its numeric dates could be read either way. "
                    "The setting above decides, and dates written as 2026-03-12 or 12 Mar 2026 ignore it."
                } else {
                    "Every date here says which way round it is, so the setting above changes nothing."
                }
            }
        }

        if map.column_of(SheetField::Duration).is_some() {
            div { class: "bs-field",
                label { "A bare number in Duration is" }
                Dropdown {
                    value: map.duration_unit.plural().to_string(),
                    options: aop_core::DurationUnit::ALL
                        .iter()
                        .map(|unit| Choice::plain(unit.plural()))
                        .collect(),
                    width: 0.0, large: true, disabled: false,
                    on_pick: move |picked: String| {
                        let mut writer = mapping.write();
                        if let Some(map) = writer.as_mut()
                            && let Some(unit) = aop_core::DurationUnit::from_plural(&picked)
                        {
                            map.duration_unit = unit;
                        }
                    },
                }
            }
        }

        h3 { class: "imp-step", "What will be imported" }
        match outcome() {
            None => rsx! {},
            Some(Err(message)) => rsx! {
                div { class: "info-alert",
                    span { class: "fix-icon", {icon("warning", 18)} }
                    div { style: "flex: 1;", "{message}" }
                }
            },
            Some(Ok(report)) => {
                let tasks = report.tasks;
                let target = path.clone();
                let title = plan_name.clone();
                rsx! {
                    Summary { report: report.clone() }
                    div { class: "hint", style: "margin: 14px 0 8px;",
                        "Importing replaces the plan that is open. Nothing has happened yet, and if there is unsaved work you will be asked about it first."
                    }
                    button { class: "btn primary",
                        disabled: tasks == 0,
                        onclick: move |_| {
                            // Built again here rather than held from the
                            // preview: the plan that arrives has to be the one
                            // the numbers on screen describe.
                            let built = {
                                let held = sheets.read();
                                let Some(sheet) = held.get(which()) else { return };
                                let Some(map) = mapping.read().clone() else { return };
                                aop_core::sheet::read(sheet, &map, &title)
                            };
                            match built {
                                Ok(import) => {
                                    let note = format!(
                                        "Imported {} tasks from {}. Save As to keep it as a .{} file.",
                                        import.report.tasks,
                                        target.display(),
                                        persist::FILE_EXTENSION
                                    );
                                    state.write().stage_import(import.project, target.clone(), note);
                                }
                                Err(error) => trouble.set(Some(error.to_string())),
                            }
                        },
                        "Import {tasks} task(s)"
                    }
                }
            }
        }

        if let Some(message) = trouble() {
            div { class: "info-alert",
                span { class: "fix-icon", {icon("warning", 18)} }
                div { style: "flex: 1;", "{message}" }
            }
        }
    }
}

/// What the import will do, before any of it happens.
#[component]
fn Summary(report: Report) -> Element {
    let figures: Vec<(&str, String)> = vec![
        ("Tasks", report.tasks.to_string()),
        ("Resources", report.resources.to_string()),
        ("Links", report.links.to_string()),
        ("Rows skipped", (report.blank_rows + report.skipped_rows).to_string()),
        ("Columns ignored", report.ignored.len().to_string()),
        ("Deepest level", report.deepest.to_string()),
    ];

    let notices = report.notices.clone();
    let ignored = report.ignored.join(", ");

    rsx! {
        div { class: "stat-row",
            for (label, value) in figures {
                div { key: "{label}", class: "stat-tile",
                    div { class: "stat-value", "{value}" }
                    div { class: "stat-label", "{label}" }
                }
            }
        }

        div { class: "info-card", style: "margin-top: 14px; max-width: 760px;",
            div { class: "info-line",
                span { class: "k", "Outline" }
                span { class: "v", "{report.structure.label()}" }
            }
            if !report.ignored.is_empty() {
                div { class: "info-line",
                    span { class: "k", "Left out" }
                    span { class: "v", "{ignored}" }
                }
            }
            if report.assumed_dates > 0 {
                div { class: "info-line",
                    span { class: "k", "Dates assumed" }
                    span { class: "v",
                        "{report.assumed_dates} date(s) could be read either way and follow the setting above"
                    }
                }
            }
            if report.dropped_links > 0 {
                div { class: "info-line",
                    span { class: "k", "Dependencies lost" }
                    span { class: "v", "{report.dropped_links} named a row that is not in this import, or were the other dependency column contradicting itself" }
                }
            }
            if report.looped_links > 0 {
                div { class: "info-line",
                    span { class: "k", "Dependency loops" }
                    span { class: "v",
                        "{report.looped_links} would have made a task wait for itself. A plan with one of those in it cannot be scheduled at all, so they were left out and listed below"
                    }
                }
            }
            if report.work_unplaced > 0 {
                div { class: "info-line",
                    span { class: "k", "Work not placed" }
                    span { class: "v",
                        "{report.work_unplaced} row(s) give an amount of work with nobody on the task to do it"
                    }
                }
            }
        }

        if !notices.is_empty() {
            h3 { class: "imp-step", "Rows and cells that could not be read" }
            div { class: "imp-list",
                for (index, notice) in notices.iter().enumerate() {
                    div { key: "n{index}", class: "imp-notice",
                        span { class: "imp-where", "Row {notice.row} \u{00b7} {notice.heading}" }
                        span { class: "imp-value", "{notice.value}" }
                        span { class: "imp-why", "{notice.why}" }
                    }
                }
                if report.unlisted_notices > 0 {
                    div { class: "imp-notice",
                        span { class: "imp-why", "and {report.unlisted_notices} more" }
                    }
                }
            }
        }

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
    let mut state = use_context::<Signal<AppState>>();
    let mut show_attributions = use_signal(|| false);
    let skipped = state.read().skip_version.clone();
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
                }
            }

            // ---- details -------------------------------------------------
            div { class: "about-rows",
                for (label, value) in [
                    ("Product", APP_NAME.to_string()),
                    ("Version", format!("v{}", env!("CARGO_PKG_VERSION"))),
                    ("File format", format!(".{} ({})", persist::FILE_EXTENSION, persist::FILE_TYPE_NAME)),
                    ("Engine", "AOP Project Engine v2".to_string()),
                    ("Calendars", "Working time with shifts, weekends and holidays".to_string()),
                    ("\u{00a9}", format!("{year} Alterion. All rights reserved.")),
                ] {
                    div { key: "{label}", class: "about-row",
                        span { class: "k", "{label}" }
                        span { class: "v", "{value}" }
                    }
                }
                // Beside the version that is running, since the pair is the
                // whole story: this is what you have, and that is the one you
                // asked not to be offered. Only while there is one, and where
                // Check for updates is, so the button and the reason it may
                // say nothing are read together.
                if !skipped.is_empty() {
                    div { class: "about-row",
                        span { class: "k", "Skipped" }
                        span { class: "v", "v{skipped}, cleared in Options under General" }
                    }
                }
            }

            div { class: "about-actions",
                button {
                    class: "about-attr-btn",
                    onclick: move |_| show_attributions.set(true),
                    {icon("package-mono", 15)}
                    span { "Open Source Attributions" }
                }
                // Reachable on purpose, rather than only when the application
                // happens to raise it after an update.
                button {
                    class: "about-attr-btn",
                    onclick: move |_| state.write().show_support(),
                    {icon("support", 15)}
                    span { "Support development" }
                }
                button {
                    class: "about-attr-btn",
                    onclick: move |_| {
                        crate::updates::ask_in_background(state);
                        state.write().dialog = Some(crate::state::Dialog::UpdateAvailable);
                    },
                    {icon("sync", 15)}
                    span { "Check for updates" }
                }
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
                    OptionsPage::Collaborate => rsx! { OptCollaborate {} },
                    OptionsPage::Keyboard => rsx! { OptKeyboard {} },
                    OptionsPage::CustomizeRibbon => rsx! { OptRibbon {} },
                    OptionsPage::QuickAccess => rsx! { OptQuickAccess {} },
                }
            }
        }
    }
}

/// Alterion Collaborate: which servers, and who this copy is signed in as.
///
/// Three addresses and no secret. Everything else about a provider comes from
/// its own discovery document, so somebody running their own changes the
/// address and nothing else follows. There are no defaults either: the
/// identity provider is self hosted, and shipping one address as the out of
/// the box answer would point every copy that never changed it at somebody
/// else's server.
///
/// Nothing here is a client secret. This is a native application doing
/// authorization code with PKCE, and a secret compiled into a desktop binary
/// is a secret every copy of the binary carries.
#[component]
fn OptCollaborate() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // Read the account back from the provider when this page is opened.
    //
    // The window focus handler in `main.rs` is the other trigger, and it is
    // the one that cannot be relied on: a compositor is under no obligation to
    // report focus the way an application expects, and on Wayland it often
    // does not. Opening this page is a deliberate act by somebody who wants to
    // see their account, so it is a trigger that cannot be missed. The
    // staleness floor is shared with the focus path, so opening the page
    // repeatedly does not turn into a stream of requests.
    use_effect(move || {
        let due = {
            let read = state.read();
            read.session.is_some()
                && read
                    .account_checked_at
                    .is_none_or(|at| at.elapsed() >= crate::state::ACCOUNT_RECHECK)
        };
        if due {
            state.write().account_checked_at = Some(std::time::Instant::now());
            crate::collaborate::refresh_account(state);
        }
    });

    let (on, issuer, client_id, server) = {
        let s = state.read();
        (
            s.collaborate,
            s.idp_issuer.clone(),
            s.idp_client_id.clone(),
            s.collaborate_server.clone(),
        )
    };
    let (working, message, linked) = {
        let s = state.read();
        (s.working, s.cloud_message.clone(), s.link.clone())
    };
    let cannot_publish = state.read().publish_blocked();

    let save = move |state: &mut AppState| {
        let settings = state.settings();
        settings.save();
    };

    rsx! {
        h2 { class: "opt-head", "Alterion Collaborate" }
        p { class: "opt-note",
            "Sign in to keep a plan on a server, see who changed what, and work on it with other people at the same time. Everything stays on this machine until you do."
        }

        OptCheck {
            label: "Offer signing in and syncing".to_string(),
            on_state: on,
            on: move |_| {
                let mut writer = state.write();
                writer.collaborate = !writer.collaborate;
                // Start up skips the token store when this is off, so turning
                // it on part way through a session is the other moment a sign
                // in from last time can be picked up. Reads a file and no
                // more, so it costs nothing when there is nothing there.
                if writer.collaborate && writer.account.is_none() {
                    writer.restore_session();
                }
                save(&mut writer);
            },
        }

        if on {
            div { class: "sep" }
            Setting {
                label: "Identity provider URL".to_string(),
                hint: "The address you sign in at. Everything else about it, including where the browser goes, is read from its own discovery document. Whoever runs your Collaborate server issues this address; running your own means putting it here.".to_string(),
                input {
                    class: "bs-input",
                    value: "{issuer}",
                    placeholder: "https://auth.example.com",
                    onchange: move |event| {
                        let value = event.value().trim().trim_end_matches('/').to_string();
                        let mut writer = state.write();
                        writer.idp_issuer = value;
                        save(&mut writer);
                    },
                }
            }
            Setting {
                label: "Client ID".to_string(),
                hint: "How this copy identifies itself to the identity provider. Your provider issues it when the application is registered. It is not a password, and there is no client secret to fill in: this application signs in with PKCE, and a secret inside a program anyone can download is not a secret.".to_string(),
                input {
                    class: "bs-input",
                    value: "{client_id}",
                    placeholder: "alterion-open-project",
                    onchange: move |event| {
                        let value = event.value().trim().to_string();
                        let mut writer = state.write();
                        writer.idp_client_id = value;
                        save(&mut writer);
                    },
                }
            }
            Setting {
                label: "AOP Collaborate server URL".to_string(),
                hint: "The server plans are kept and synced on. A separate address from the identity provider, and usually a separate machine: they go wrong one at a time. Whoever runs it gives you this address.".to_string(),
                input {
                    class: "bs-input",
                    value: "{server}",
                    placeholder: "https://collaborate.example.com",
                    onchange: move |event| {
                        let value = event.value().trim().trim_end_matches('/').to_string();
                        let mut writer = state.write();
                        writer.collaborate_server = value;
                        save(&mut writer);
                    },
                }
            }

            div { class: "sep" }
            h2 { class: "opt-head", "This machine" }

            AccountCard {}

            if let Some(working) = working {
                p { class: "opt-note", "{working.waiting()}" }
            } else if let Some(message) = &message {
                p { class: "opt-note", "{message}" }
            }

            div { class: "sep" }
            h2 { class: "opt-head", "This plan" }

            if let Some(link) = &linked {
                p { class: "opt-note",
                    "This plan is on the server as {link.project}. What is waiting to go, and \
                     whether the server agrees this is the latest version, are in History and \
                     Sync on the View tab."
                }
                SharedWith {}
                div { class: "opt-actions",
                    button {
                        class: "btn",
                        onclick: move |_| state.write().unlink_plan(),
                        "Unlink this plan from the server"
                    }
                    span { class: "opt-why",
                        "Nothing is removed from either side. This copy stops syncing and \
                         forgets how far it had read, which is what to do when the plan on the \
                         server turns out to be a different one."
                    }
                }
            } else {
                p { class: "opt-note",
                    "This plan is on this machine only. Putting it on the server uploads it as \
                     it stands and shares it with whoever you give access to."
                }
                div { class: "opt-actions",
                    match &cannot_publish {
                        Some(why) => rsx! {
                            button { class: "btn", disabled: true, "Put this plan on the server" }
                            span { class: "opt-why", "{why}" }
                        },
                        None => rsx! {
                            button {
                                class: "btn",
                                onclick: move |_| crate::collaborate::publish(state),
                                "Put this plan on the server"
                            }
                        },
                    }
                }
            }
        }
    }
}

/// Who a plan is shared with, and the only place any of it can be changed.
///
/// **Why here and not in History and Sync.** That panel answers questions
/// about the copy on screen: what is waiting to go, whether the server agrees
/// this is the latest version, which version to go back to, and who happens to
/// be looking at it right now. Those are all facts about a moment. Who has
/// access is not a fact about a moment, it is an arrangement, and it is
/// changed in the same breath as putting the plan on a server and taking it
/// off again. Those two controls are already here, and this is the third verb
/// of the same sentence. History and Sync goes on saying who is *here*; this
/// says who may *come*.
///
/// **Addresses.** The server sends them to the owner and to nobody else, so an
/// editor sees who is in the plan and not what their addresses are, and sees
/// no pending invitations at all. That is not a decision this page makes and
/// it is not one it can undo: what is not in the answer cannot be drawn.
#[component]
fn SharedWith() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // Read the list when this page is opened on a plan it has not been read
    // for. Not kept fresh afterwards, on purpose: somebody else can change the
    // sharing from their own machine and nothing here would hear about it, so
    // a list that refreshed itself would only be wrong less obviously. The
    // Refresh button is how somebody asks again.
    use_effect(move || {
        let due = {
            let read = state.read();
            let project = read.link.as_ref().map(|link| link.project.clone());
            project.is_some() && project != read.sharing_for && read.sharing_blocked().is_none()
        };
        if due {
            crate::collaborate::sharing(state);
        }
    });

    let (sharing, message, working, typed, role) = {
        let s = state.read();
        (
            s.sharing.clone(),
            s.sharing_message.clone(),
            s.working,
            s.invite_email.clone(),
            s.invite_role.clone(),
        )
    };
    let busy = working.is_some();
    let blocked = state.read().sharing_blocked();

    rsx! {
        div { class: "sep" }
        h2 { class: "opt-head", "Shared with" }

        match &sharing {
            None => rsx! {
                p { class: "opt-note",
                    match &blocked {
                        Some(why) => why.clone(),
                        None if busy => "Asking the server who this plan is shared with...".to_string(),
                        None => "Not read yet.".to_string(),
                    }
                }
            },
            Some(sharing) => {
                let owner = sharing.you_own_it();
                let you = sharing.you.clone();
                let owner_subject = sharing.owner.clone();
                let members = sharing.members.clone();
                let invites = sharing.invites.clone();
                rsx! {
                    p { class: "opt-note",
                        if owner {
                            "You made this plan, so you decide who else can reach it. Invite \
                             somebody by their email address; they join the first time they \
                             open the plan while signed in with that address. Nobody is looked \
                             up: until they turn up holding their own sign in, this application \
                             knows nothing about the address you typed."
                        } else {
                            "Whoever made this plan decides who can reach it. Ask them to \
                             invite somebody, or to change what you can do here."
                        }
                    }

                    table { class: "assign-table", style: "margin-top: 12px;",
                        thead {
                            tr {
                                th { "Who" }
                                th { style: "width: 132px;", "What they can do" }
                                if owner {
                                    th { style: "width: 96px;", "" }
                                }
                            }
                        }
                        tbody {
                            for member in members.iter() {
                                {
                                    let label = who_is(member, &owner_subject, &you);
                                    let is_owner = member.subject == owner_subject;
                                    let subject = member.subject.clone();
                                    let removing = label.clone();
                                    rsx! {
                                        tr { key: "m{member.subject}",
                                            td { "{label}" }
                                            td { "{what_they_can_do(&member.role)}" }
                                            if owner {
                                                td {
                                                    // The owner is not offered a
                                                    // button that would refuse. A
                                                    // plan whose owner has been
                                                    // removed is one nobody can
                                                    // share and nobody can delete.
                                                    if !is_owner {
                                                        button {
                                                            class: "btn danger",
                                                            disabled: busy,
                                                            onclick: move |_| crate::collaborate::remove_member(
                                                                state,
                                                                subject.clone(),
                                                                removing.clone(),
                                                            ),
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
                    }

                    match &invites {
                        // Null rather than empty, from a server that does not
                        // send other people's addresses to people who cannot
                        // act on them. Said plainly, because a heading with
                        // nothing under it reads as "nobody has been invited".
                        None => rsx! {
                            p { class: "hint", style: "margin-top: 10px;",
                                "Invitations that have not been taken up yet are shown to \
                                 whoever made the plan and to nobody else."
                            }
                        },
                        Some(waiting) if waiting.is_empty() => rsx! {
                            p { class: "hint", style: "margin-top: 10px;",
                                "No invitations are waiting to be taken up."
                            }
                        },
                        Some(waiting) => rsx! {
                            h3 { class: "opt-sub", style: "margin-top: 14px;", "Waiting to be taken up" }
                            table { class: "assign-table",
                                thead {
                                    tr {
                                        th { "Address" }
                                        th { style: "width: 132px;", "Would be able to" }
                                        th { style: "width: 96px;", "Invited" }
                                        th { style: "width: 96px;", "" }
                                    }
                                }
                                tbody {
                                    for invitation in waiting.iter() {
                                        {
                                            let email = invitation.email.clone();
                                            rsx! {
                                                tr { key: "i{invitation.email}",
                                                    td { "{invitation.email}" }
                                                    td { "{what_they_can_do(&invitation.role)}" }
                                                    td { "{invitation.sent_on}" }
                                                    td {
                                                        button {
                                                            class: "btn",
                                                            disabled: busy,
                                                            onclick: move |_| crate::collaborate::cancel_invite(
                                                                state,
                                                                email.clone(),
                                                            ),
                                                            "Withdraw"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        },
                    }

                    if owner {
                        div { class: "sep" }
                        h3 { class: "opt-sub", "Invite somebody" }
                        div { class: "ext-add",
                            input {
                                class: "bs-input",
                                r#type: "email",
                                placeholder: "their.address@example.com",
                                value: "{typed}",
                                oninput: move |event| state.write().invite_email = event.value(),
                            }
                            // Two buttons rather than a list to choose from.
                            // There are two answers, and what each one means is
                            // shorter written out than explained underneath.
                            button {
                                class: if role == "editor" { "btn primary" } else { "btn" },
                                onclick: move |_| state.write().invite_role = "editor".into(),
                                "Can edit"
                            }
                            button {
                                class: if role == "viewer" { "btn primary" } else { "btn" },
                                onclick: move |_| state.write().invite_role = "viewer".into(),
                                "Can only look"
                            }
                            button {
                                class: "btn primary",
                                disabled: busy,
                                onclick: move |_| crate::collaborate::invite(state),
                                "Invite"
                            }
                        }
                        p { class: "hint",
                            "The address has to be the one they sign in with, and their \
                             identity provider has to have confirmed it: an address nobody \
                             has confirmed is one anybody could have typed, so the server \
                             will not admit somebody on the strength of it. Send them the \
                             share link as well, so their copy knows which plan to ask for."
                        }
                    }
                }
            }
        }

        if let Some(message) = &message {
            p { class: "opt-note", "{message}" }
        }

        div { class: "opt-actions",
            match &blocked {
                Some(why) => rsx! {
                    button { class: "btn", disabled: true, "Refresh" }
                    span { class: "opt-why", "{why}" }
                },
                None => rsx! {
                    button {
                        class: "btn",
                        onclick: move |_| crate::collaborate::sharing(state),
                        "Refresh"
                    }
                    span { class: "opt-why",
                        "Read when this page opens, and not kept up to date afterwards. \
                         Somebody else can change this from their own machine and nothing \
                         here would hear about it."
                    }
                },
            }
        }
    }
}

/// What to call a member on screen.
///
/// The address when there is one, which there is for anybody who came in by an
/// invitation and for nobody else. The subject is never shown: it is a UUID an
/// identity provider minted, it means nothing to the person reading it, and a
/// row labelled with one is a row nobody can safely press Remove on.
fn who_is(member: &crate::cloud::collab::Member, owner: &str, you: &str) -> String {
    if member.subject == you {
        return "You".to_string();
    }
    match member.email.as_deref() {
        Some(email) => email.to_string(),
        None if member.subject == owner => "Whoever made this plan".to_string(),
        // Reachable only for a membership that predates invitations, or one
        // written into the database by hand. Saying so is better than showing
        // an identifier that looks like it might mean something.
        None => "Somebody added before invitations existed".to_string(),
    }
}

/// A role, said as what it lets somebody do.
fn what_they_can_do(role: &str) -> &'static str {
    match role {
        "owner" => "Everything",
        "editor" => "Edit the plan",
        "viewer" => "Look, not change",
        _ => "Something this copy does not know",
    }
}

/// Who this copy is signed in as, and the one place to do anything about it.
///
/// A card rather than a paragraph, because "who am I signed in as" is a thing
/// people look at rather than read. Everything that is true but reassuring
/// lives under Details: how long the pass lasts, which machine it is tied to,
/// where the tokens sit. Said once, and out of the way of what somebody came
/// here to do.
///
/// Managing the account itself happens in the browser, at the provider. The
/// browser already has the session, the provider owns changing a password or
/// an address, and a desktop application that collects a password is a place
/// credentials can be taken with nothing gained. So there is nothing to edit
/// here, on purpose.
#[component]
fn AccountCard() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (account, device, working) = {
        let s = state.read();
        (s.account.clone(), s.device.clone(), s.working)
    };
    // Read off the session rather than off the settings: what matters here is
    // which provider this sign in actually came from, which is not necessarily
    // what the boxes above say today.
    let signed_in_at = state.read().session_summary();
    let manage = state.read().account_page_url();
    let manage_offered = manage.is_some();
    let cannot_sign_in = state.read().sign_in_blocked();
    let issuer = state.read().idp_issuer.clone();

    let Some(account) = account else {
        return rsx! {
            div { class: "acct-card",
                div { class: "acct-avatar nobody", {icon("account", 22)} }
                div { class: "acct-who",
                    div { class: "acct-name", "Not signed in" }
                    div { class: "acct-email",
                        "Nothing leaves this machine until somebody signs in."
                    }
                }
                div { class: "acct-actions",
                    match (working, &cannot_sign_in) {
                        // The wait is the whole reason this state is drawn: a
                        // browser opens somewhere else and this window looks
                        // asleep. A button that says nothing gets pressed
                        // twice, and the second press is a second sign in.
                        (Some(Working::SigningIn), _) => rsx! {
                            button { class: "btn primary", disabled: true,
                                "Waiting for your browser..."
                            }
                        },
                        (_, Some(_)) => rsx! {
                            button { class: "btn primary", disabled: true,
                                "Sign in with your browser"
                            }
                        },
                        (_, None) => rsx! {
                            button {
                                class: "btn primary",
                                onclick: move |_| crate::collaborate::sign_in(state),
                                "Sign in with your browser"
                            }
                        },
                    }
                }
            }
            match (working, &cannot_sign_in) {
                (Some(Working::SigningIn), _) => rsx! {
                    p { class: "opt-aside",
                        "Your browser has opened. Finish signing in there and this window will \
                         carry on by itself."
                    }
                },
                (_, Some(why)) => rsx! { p { class: "opt-aside", "{why}" } },
                (_, None) => rsx! {
                    p { class: "opt-aside",
                        "This opens your browser at {issuer}. Come back here when it says you \
                         are done."
                    }
                },
            }
        };
    };

    rsx! {
        div { class: "acct-card",
            div { class: "acct-avatar",
                match &account.picture {
                    // Left to the webview, which fetches it the way it fetches
                    // anything else on a page. Nothing here waits on it, and
                    // the address was checked for a safe scheme where the
                    // claim was read.
                    Some(url) => rsx! { img { class: "acct-face", src: "{url}", alt: "" } },
                    // The ordinary case today: the provider serves no picture,
                    // so the initials are the picture rather than a placeholder
                    // waiting for one.
                    None => rsx! { span { class: "acct-initials", "{account.initials()}" } },
                }
            }
            div { class: "acct-who",
                div { class: "acct-name", "{account.name}" }
                if !account.email.is_empty() {
                    div { class: "acct-email", "{account.email}" }
                }
            }
            div { class: "acct-actions",
                if let Some(url) = manage {
                    button {
                        class: "btn primary",
                        disabled: working.is_some(),
                        onclick: move |_| {
                            // Handing an address to the desktop is a spawn and
                            // no more: nothing here goes near the network or
                            // waits for the browser. The one thing that can go
                            // wrong says so where the other cloud messages do.
                            match crate::cloud::oauth::open_in_browser(&url) {
                                // Remembered, so that coming back to this
                                // window is taken as "they may have changed
                                // something" and the details are read again.
                                // Nothing else ever says that they did.
                                Ok(()) => state.write().account_page_opened = true,
                                Err(why) => {
                                    state.write().cloud_message = Some(why.to_string())
                                }
                            }
                        },
                        "Manage account"
                    }
                }
                // A way to ask, rather than wait to be told. Opening this
                // page already refreshes, and returning to the window usually
                // does, but "usually" is doing real work in that sentence: a
                // compositor need not report focus, and somebody who has just
                // changed their picture in a browser wants a button, not a
                // rule about when it happens by itself.
                button {
                    class: "btn",
                    disabled: working.is_some(),
                    onclick: move |_| {
                        // Stamped here so the automatic triggers do not
                        // immediately ask again on top of this one.
                        state.write().account_checked_at = Some(std::time::Instant::now());
                        crate::collaborate::refresh_account(state);
                    },
                    "Refresh"
                }
                // Reachable, and deliberately quieter than managing the
                // account: signing out is the rarer of the two and the one
                // nobody wants to press by accident.
                button {
                    class: "btn",
                    disabled: working.is_some(),
                    onclick: move |_| crate::collaborate::sign_out(state),
                    "Sign out"
                }
            }
        }

        if manage_offered {
            p { class: "opt-aside",
                "Your name, address and picture are changed on your account page, in your \
                 browser. This application never asks for your password."
            }
        }

        // Collapsed, because these are answers to questions nobody asks twice.
        details { class: "acct-details",
            summary { "Details" }
            if let Some((at, until)) = &signed_in_at {
                p { "Signed in at {at}. This pass lasts until {until}, and is renewed by itself before then." }
            }
            if let Some(device) = &device {
                p {
                    "Tied to this machine ({device}), so the stored sign in is no use on another \
                     one. Replace the hardware and you simply sign in again."
                }
            }
            p { "Where the sign in is kept: {crate::cloud::tokens::store().describe()}" }
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
    let (update_check, patch_notes, support_page, skipped) = {
        let s = state.read();
        (
            s.update_check,
            s.patch_notes,
            s.support_page,
            s.skip_version.clone(),
        )
    };
    // The name on the account, while there is one. It is the name written
    // against every change in a shared plan, so it is the name to show here:
    // somebody who calls themselves one thing in this box and appears to their
    // colleagues as another has no way of finding that out.
    // Filtered the same way `display_name` filters it, so what is shown here
    // is exactly what will be written against a change rather than nearly.
    let from_server = {
        let s = state.read();
        s.account
            .as_ref()
            .map(|account| account.name.trim().to_string())
            .filter(|name| !name.is_empty())
    };
    let collaborate = state.read().collaborate;

    rsx! {
        h2 { class: "opt-head", "Personalize your copy of Alterion Open Project" }
        match &from_server {
            Some(server_name) => rsx! {
                Setting {
                    label: "User name".to_string(),
                    hint: "From your Alterion account. Changed on your account page, in Options under Alterion Collaborate.".to_string(),
                    input { class: "bs-input", value: "{server_name}", disabled: true }
                }
                p { class: "opt-aside",
                    "This is the name other people see against your changes in a shared plan."
                }
                // Not thrown away, and said so. Somebody who typed a name here
                // before signing in should be able to see that it is still
                // there rather than wonder what happened to it.
                if !name.trim().is_empty() && name.trim() != server_name {
                    p { class: "opt-aside",
                        "The name you typed here, {name}, is kept on this machine and used again \
                         when you sign out."
                    }
                }
            },
            None => rsx! {
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
                if collaborate {
                    p { class: "opt-aside",
                        "Signing in shows your account name here instead, since that is the one \
                         your colleagues see. This one is kept either way."
                    }
                }
            },
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

        h2 { class: "opt-head", "Updates" }
        p { class: "opt-note",
            "Nothing is downloaded or installed without being asked for. Turning the check off              stops it happening at all, including at start up."
        }
        OptCheck { label: "Check for a newer version".to_string(), on_state: update_check,
            on: move |_| { let on = state.read().update_check; state.write().update_check = !on; } }
        OptCheck { label: "Show what changed after an update".to_string(), on_state: patch_notes,
            on: move |_| { let on = state.read().patch_notes; state.write().patch_notes = !on; } }
        OptCheck { label: "Offer the support page after an update".to_string(), on_state: support_page,
            on: move |_| { let on = state.read().support_page; state.write().support_page = !on; } }
        // Only while there is one. A row saying nothing is skipped would be a
        // permanent fixture explaining a feature nobody had used, and the
        // reason this is here at all is the opposite case: a skip that cannot
        // be found again is somebody quietly not being offered a fix.
        if !skipped.is_empty() {
            Setting {
                label: "Skipped version".to_string(),
                hint: "Not offered again. Anything newer still is.".to_string(),
                div { style: "display: flex; align-items: center; gap: 10px;",
                    span { class: "opt-hint", "v{skipped}" }
                    button {
                        class: "btn",
                        onclick: move |_| state.write().offer_the_skipped_version_again(),
                        "Offer it again"
                    }
                }
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
