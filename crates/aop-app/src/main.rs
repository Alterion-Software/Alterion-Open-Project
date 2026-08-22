//! Alterion Open Project: a project scheduler with a critical path engine,
//! a Gantt chart, and a ribbon that follows Microsoft Project's layout.

// Windows gives a program a console unless it is told otherwise, so a release
// build would open a black terminal behind the window. Kept in debug builds,
// where printing to a console is how anything gets diagnosed.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod backstage;
mod brand;
mod cloud;
mod collaborate;
mod contextmenu;
mod cursors;
mod controls;
mod dictionary;
mod dialogs;
mod gantt;
mod grid;
mod floating;
mod fonts;
mod handoff;
mod keymap;
mod icons;
mod macros;
mod placement;
mod popups;
mod preview;
mod quiet;
mod spooler;
mod settings;
mod ribbon;
mod recovery;
mod state;
mod theme;
mod updates;
mod versions;
mod views;
mod viewport;
mod welcome;

#[cfg(feature = "desktop")]
use dioxus::desktop::tao::dpi::LogicalSize;
#[cfg(feature = "desktop")]
use dioxus::desktop::{Config, WindowBuilder, WindowCloseBehaviour};
use dioxus::prelude::*;

use aop_core::{format_duration, format_work, TaskMode};
use aop_core::APP_NAME;

use crate::icons::icon;
use crate::state::{AppState, BackstagePage, Column, PaneFocus, ViewKind, Zoom};
use crate::viewport::{ChartScroll, ColumnDrag, GridScroll, PaneScroll, Part, Reach, Shifted};

/// Work around a WebKitGTK renderer that blanks the window on some machines.
///
/// Under Wayland on hybrid graphics, WebKitGTK's DMABUF renderer intermittently
/// hands back an empty frame: the whole interface disappears for a few seconds
/// and then returns by itself. Nothing in the application causes it and nothing
/// in the application can detect it, so the only fix is to ask WebKit not to
/// use that path.
///
/// It is only a default. An explicit setting in the environment is left alone,
/// so anyone whose machine is fine with the fast path can have it back.
#[cfg(all(feature = "desktop", target_os = "linux"))]
fn steady_rendering() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        // Setting an environment variable is unsound once other threads are
        // running, because a reader can be part way through the environment
        // while it is rewritten. This is the first statement of `main`: nothing
        // has been spawned yet, and the webview that reads it is not built
        // until later in this same function.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

#[cfg(all(feature = "desktop", not(target_os = "linux")))]
fn steady_rendering() {}

/// How a close asked for from outside the application is handled.
///
/// The portable event loop cannot be made to refuse one: every handler that
/// sees the request runs before the close is carried out. Hiding the window
/// instead of destroying it is the only portable way to get a word in, and the
/// application then decides whether to bring it back or close for real.
///
/// Where the toolkit can refuse the close outright, that is much better and is
/// used instead, because hiding and re-showing makes a tiling window manager
/// treat the window as having closed and reopened, so it moves.
#[cfg(feature = "desktop")]
fn close_behaviour() -> WindowCloseBehaviour {
    WindowCloseBehaviour::WindowHides
}

/// The webview build, using wry and the platform's web engine.
#[cfg(feature = "desktop")]
fn main() {
    steady_rendering();

    // A link clicked, or a plan double clicked, while this application is
    // already open belongs in the window that is already open. Two copies of
    // one plan, each with its own change log and its own sync cursor, is the
    // drift the sync protocol has a whole case for detecting, and starting one
    // from the file manager would be making it happen.
    if handoff::claim(handed_argument().as_ref()) == handoff::Claim::HandedOver {
        return;
    }

    // After the handoff, never before it. The log is truncated when it opens,
    // and a launch that only passes its argument along and exits would
    // otherwise cut the log of the session that is still running, from
    // underneath a file handle that copy is still writing through.
    applog::start(env!("CARGO_PKG_VERSION"));

    let window = WindowBuilder::new()
        .with_title(APP_NAME)
        // The title bar, window controls and dragging are all drawn in-app.
        .with_decorations(false)
        .with_resizable(true)
        .with_inner_size(LogicalSize::new(1560.0, 980.0))
        .with_min_inner_size(LogicalSize::new(1024.0, 640.0));

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_menu(None)
                .with_window(window)
                .with_close_behaviour(close_behaviour()),
        )
        .launch(App);
}

/// The webview-free build. Blitz paints with wgpu and lays out with Stylo, so
/// there is no wry and no webkit2gtk. It has no window-chrome API of its own,
/// so this build keeps the operating system's decorations.
#[cfg(all(feature = "native", not(feature = "desktop")))]
fn main() {
    if handoff::claim(handed_argument().as_ref()) == handoff::Claim::HandedOver {
        return;
    }
    // After the handoff, for the reason given in the webview build's `main`.
    applog::start(env!("CARGO_PKG_VERSION"));
    dioxus_native::launch_cfg(
        App,
        Vec::new(),
        vec![Box::new(
            dioxus_native::Config::new()
                .with_window_attributes(
                    dioxus_native::WindowAttributes::default().with_title(APP_NAME),
                )
                // Carried rather than hoped for, and beside what the machine
                // already has rather than instead of it. See `fonts`.
                .with_font_ctx(crate::fonts::context()),
        )],
    );
}

/// The plan this launch was asked to open, if it was asked to open one.
///
/// A relative path is resolved here, in the process that was handed it, since
/// the copy already running has a working directory of its own.
fn handed_argument() -> Option<handoff::Handed> {
    std::env::args()
        .nth(1)
        .as_deref()
        .and_then(handoff::Handed::from_argument)
}

/// The stylesheet, in a component of its own so it is written once.
///
/// It takes no props, so it never re-renders. Left inside `App` it would be
/// diffed on every state write, and re-setting a two thousand line stylesheet
/// is the kind of thing an engine can answer with a repaint of everything.
#[component]
fn Stylesheet() -> Element {
    rsx! { style { dangerous_inner_html: theme::CSS.as_str() } }
}

/// The palette overlay, which changes only when the theme does.
///
/// Separated for the same reason as `Stylesheet`: as a component it is diffed
/// against its props, so typing in a cell cannot touch it.
#[component]
fn Palette(css: String) -> Element {
    if css.is_empty() {
        return rsx! {};
    }
    rsx! { style { dangerous_inner_html: "{css}" } }
}

/// The start-up screen: the mark on the left, a little chart art on the right.
/// It clears itself after a moment, or on the first click.
#[component]
fn Splash() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let (logo_w, logo_h) = crate::brand::LOGO_VIEWBOX;

    use_future(move || async move {
        tokio::time::sleep(std::time::Duration::from_millis(1700)).await;
        state.write().splash = false;
    });

    rsx! {
        div {
            class: "splash",
            onclick: move |_| state.write().splash = false,

            div { class: "splash-left",
                div {
                    class: "splash-logo",
                    style: "width: 300px; height: {300.0 * logo_h / logo_w}px;",
                    dangerous_inner_html: crate::brand::logo(300.0, state.read().theme.palette().paint("--ink")),
                }
                div { class: "splash-product", "Open Project" }
                div { class: "splash-version", "Version {env!(\"CARGO_PKG_VERSION\")}" }
                div { class: "splash-bar", div { class: "splash-fill" } }
                div { class: "splash-note", "A better free project scheduler" }
            }

            div { class: "splash-art",
                svg { view_box: "0 0 320 260", width: "320", height: "260",
                    // A small plan, drawn as art.
                    for (index, (x, w, kind)) in [
                        (10.0, 210.0, 2u8), (26.0, 96.0, 0), (56.0, 128.0, 0),
                        (128.0, 74.0, 1), (150.0, 0.0, 3), (40.0, 176.0, 2),
                        (58.0, 88.0, 0), (110.0, 132.0, 0), (176.0, 66.0, 1),
                        (208.0, 0.0, 3),
                    ].iter().enumerate() {
                        {
                            let y = 20.0 + index as f64 * 23.0;
                            let (fill, height, radius) = match kind {
                                2 => ("#cfe3e3", 5.0, 1.0),
                                1 => ("#9d474d", 10.0, 2.0),
                                _ => ("#3f7d7d", 10.0, 2.0),
                            };
                            rsx! {
                                g { key: "art{index}",
                                    line {
                                        x1: "0", y1: "{y + 5.0}", x2: "320", y2: "{y + 5.0}",
                                        stroke: "rgba(216,231,232,0.05)", stroke_width: "1",
                                    }
                                    if *kind == 3 {
                                        polygon {
                                            points: "{x},{y} {x + 7.0},{y + 7.0} {x},{y + 14.0} {x - 7.0},{y + 7.0}",
                                            fill: "#a5d3d3",
                                        }
                                    } else {
                                        rect {
                                            x: "{x}", y: "{y}", width: "{w}", height: "{height}",
                                            rx: "{radius}", fill: "{fill}",
                                            opacity: "{0.35 + 0.07 * index as f64}",
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

#[component]
fn App() -> Element {
    use_context_provider(|| Signal::new(crate::state::from_command_line()));
    let mut state = use_context::<Signal<AppState>>();

    // The window size is kept apart from the plan. It changes for reasons that
    // have nothing to do with the document, and anything sharing a signal with
    // it would be re-rendered every time the window moved a pixel.
    // Which task the pointer is over. Its own signal rather than a field on
    // the plan's state, so moving the pointer over the chart does not
    // invalidate the layout memo and rebuild every tick to move a highlight.
    use_context_provider(|| crate::state::Hovered(Signal::new(None)));
    // Where this planner's own pointer is, for the others to draw. Its own
    // signal so that moving a mouse never redraws a window: the live timer
    // below is the only thing that reads it, and it reads it with `peek`.
    let pointing = use_context_provider(|| crate::state::Pointing(Signal::new(None))).0;
    // What is being typed into a cell and not committed. Its own signal for
    // the same reason: a keystroke is not a change to the plan, and holding
    // it on the plan's state would redraw the window on every letter.
    let drafting = use_context_provider(|| crate::state::Drafting(Signal::new(None))).0;
    let mut viewport = use_context_provider(|| Signal::new(crate::state::Viewport::default()));
    use_context_provider(crate::floating::Layer::new);
    watch_the_window(viewport);

    // Snapshot the plan on a timer so that a crash, a kill, or a power cut
    // costs at most one interval of work rather than everything since the last
    // save. Nothing is written while the plan matches its file.
    use_future(move || async move {
        let interval = std::time::Duration::from_secs(recovery::INTERVAL_SECONDS);
        loop {
            tokio::time::sleep(interval).await;
            state.read().snapshot();
        }
    });

    // What arrives on the live socket. The socket runs on a thread of its own
    // and the plan may only be written where the interface does, so arrivals
    // are collected here rather than pushed.
    //
    // The write handle is taken only when something is actually queued, and
    // that is what makes this affordable at this rate: taking one marks the
    // state dirty and redraws the window, so a session where nobody is doing
    // anything must be able to find that out without one.
    use_future(move || async move {
        let interval = std::time::Duration::from_millis(crate::state::LIVE_POLL_MILLIS);
        loop {
            tokio::time::sleep(interval).await;
            let anything = state
                .read()
                .live
                .as_ref()
                .is_some_and(crate::cloud::live::Live::has_incoming);
            if anything {
                state.write().poll_live();
            }
        }
    });

    // What goes out: where this planner is, and the work they have done.
    //
    // Its own timer because the two directions want different things. What
    // arrives should arrive as soon as it is there; what is sent is capped
    // however fast a mouse moves, and the socket itself does that capping.
    //
    // Nothing here takes a write handle to say where a pointer is. That is
    // the whole arrangement: `announce` takes `&self`, the socket remembers
    // what it last said, and moving a mouse never redraws the window.
    use_future(move || async move {
        let interval = std::time::Duration::from_millis(crate::state::EPHEMERAL_POLL_MILLIS);
        // The one line that says this loop is still alive. It is the failure
        // that cost a day: the loop ran once and then stopped, and every
        // symptom of that looked like a fault in what it calls rather than in
        // whether it was called.
        static TURNS: crate::applog::Tally =
            crate::applog::Tally::new("live timer", crate::applog::HEARTBEAT_MILLIS);
        let mut turn: u64 = 0;
        loop {
            tokio::time::sleep(interval).await;
            turn += 1;
            // Peeked rather than read: a future that subscribed to these
            // would be torn down and started again on every keystroke.
            let (at, draft) = (*pointing.peek(), drafting.peek().clone());
            TURNS.note(format_args!("turn {turn}, pointer {at:?}"));

            let (held, due, unanswered) = {
                let live = state.read();
                if live.live.is_none() {
                    continue;
                }
                live.announce(at, draft);
                (
                    live.held_work_due(),
                    live.stream_due(),
                    live.stream_unanswered(),
                )
            };
            // A batch nobody answered, first of all, because until it is given
            // up on nothing else can be offered at all. Silence is what an
            // older or mismatched server gives, and without this the session
            // stops streaming for good and says nothing about it.
            crate::applog::applog_verbose!(
                "live timer: turn {turn}, held {held}, due {due}, unanswered {unanswered}"
            );
            if unanswered {
                state.write().gave_up_on_batch();
            }
            // Somebody else's work that waited for a cell editor to close
            // goes in first: it was made against a cursor this copy has not
            // reached, so anything offered before it would only be refused.
            if held {
                state.write().apply_held_live();
            }
            if due {
                state.write().stream_changes();
            }
            // The server asking for a fresh whole plan. Answered here rather
            // than by whoever next presses Sync, because with streaming
            // nobody presses Sync for hours and the ask would go unheard.
            if state.read().snapshot_wanted() {
                crate::collaborate::send_snapshot(state);
            }
        }
    });

    // Work a save asked to be sent, when there is no live session to carry it.
    //
    // Started here rather than inside the save itself, which is the whole
    // point: a save writes a file and marks a save point, and it may not wait
    // on a network to do either. What it does instead is leave a note, and a
    // server that is down means the note is read, nothing comes of it, and the
    // work stays in the log unsent exactly as it does today.
    //
    // Its own timer rather than a line in the live one above, because that one
    // gives up as soon as there is no socket, and no socket is precisely the
    // case this exists for.
    use_future(move || async move {
        let interval = std::time::Duration::from_millis(crate::state::SAVE_SYNC_POLL_MILLIS);
        loop {
            tokio::time::sleep(interval).await;
            if state.read().sync_after_save_due() {
                crate::collaborate::sync(state);
            }
        }
    });

    // The local copy of a plan that came off a server, kept up to date so
    // that closing the window does not lose work that has no file behind it.
    // Debounced, and the write itself happens on a thread of its own.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(
                crate::state::LOCAL_COPY_AFTER_MILLIS / 4,
            ))
            .await;
            if state.read().local_copy_due() {
                state.write().write_local_copy();
            }
        }
    });

    // Preferences are written whenever they actually differ from what is on
    // disk, rather than by each control that changes one remembering to ask.
    // Scattering the call is how a new setting silently fails to persist.
    // Seeded from the file rather than from the state, because start up
    // already changes one preference: the version this copy last ran as is
    // recorded before the first frame. Seeding from the state would make that
    // record equal to what is "on disk" without ever writing it, and the notes
    // for an update would then reappear on every launch.
    let mut written = use_signal(crate::settings::Settings::load);
    use_effect(move || {
        let current = state.read().settings();
        if written() != current {
            current.save();
            written.set(current);
        }
    });

    // Look for a newer release once, quietly, and only if that has been asked
    // for. Nothing waits on the answer and nothing is installed by it: a check
    // that found something sets a chip in the status bar, and a check that
    // could not reach anybody says nothing at all.
    use_future(move || async move {
        // The previous version, if an update left one beside this one. Windows
        // cannot delete a running executable, so an update renames the old one
        // aside and whoever starts next is the first moment it can go. Silent
        // and best effort: there is usually nothing there, and a copy that is
        // still locked is not news.
        updates::sweep_previous();
        // After the splash, so a slow name resolution cannot be the first
        // thing a start up does.
        tokio::time::sleep(std::time::Duration::from_secs(updates::STARTUP_DELAY_SECONDS)).await;
        updates::ask_in_background(state);
    });

    // Plans handed over by later launches. Looked for on a timer because they
    // arrive on a thread of its own and the plan may only be written where the
    // interface runs, which is the same arrangement the live socket uses.
    // Nothing is written unless something actually arrived, so an application
    // nobody is sending anything to is not redrawn by this.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(HANDOFF_POLL_MILLIS)).await;
            // The last one, because they are all a request to open a plan and
            // only one plan is open at a time. Asking about three in a row
            // would be three dialogs for one intention.
            let Some(handed) = handoff::arrivals().pop() else {
                continue;
            };
            match handed {
                // Nothing is fetched here. The link names a server, and which
                // server that is belongs in front of the person before a
                // request goes to it.
                handoff::Handed::Link(link) => state.write().open_link_asked(&link),
                // Through the guard, not around it. Opening a plan discards
                // whatever is on screen, and a plan arriving from the file
                // manager is no more entitled to throw away an afternoon's
                // unsaved work than one opened from the menu is.
                handoff::Handed::Path(path) => {
                    state.write().guard(crate::state::PendingAction::Open(path))
                }
            }
            come_forward();
        }
    });

    // The account page belongs to the provider, so a name, an address or a
    // picture changed there is a change nothing here is told about. Coming
    // back to this window after being sent to that page is the round trip in
    // which one is made, so it is the moment to read the details again. Only
    // then: this happens a handful of times in an account's life, and asking
    // on a timer would be a network call and a wake up for nothing.
    watch_for_account_changes(state);

    // Work left behind by a session that never finished is offered back once,
    // at the start, rather than silently loaded over whatever was opened.
    use_hook(|| {
        if let Some(found) = recovery::find_abandoned().into_iter().next() {
            state.write().dialog = Some(crate::state::Dialog::Recover(found));
        }
    });

    let splash = state.read().splash;
    // Only the front of the queue: the pages are shown one at a time, and
    // whichever is in front is the only one that exists as far as this is
    // concerned.
    let greeting = state.read().greetings.first().copied();
    let (backstage, dialog, show_timeline, view, error, menu, editing, spelling_open, sync_open) = {
        let s = state.read();
        (
            s.backstage,
            s.dialog.clone(),
            s.show_timeline,
            s.view,
            s.schedule_error(),
            s.context_menu,
            s.editing,
            s.spelling_open,
            s.sync_open,
        )
    };

    // The palette overlay restates the tokens and nothing else, so it takes
    // effect purely by coming after the main sheet. Following the desktop is a
    // media query inside it rather than a separate code path.
    let palette = state.read().theme.overlay();

    rsx! {
        Stylesheet {}
        Palette { css: palette }

        div {
            class: "app",
            tabindex: "0",
            // Focused at start up, because every keyboard shortcut in this
            // application is handled by the `onkeydown` below and a key press
            // goes to whatever holds focus. In a webview the document has it
            // by default; elsewhere nothing does until something asks, and
            // nothing asking is indistinguishable from every shortcut being
            // broken.
            autofocus: true,
            onkeydown: move |event| handle_shortcut(&mut state, event),
            // Measured on the webview build only, where an element can be
            // measured safely. See `watch_the_window` for the other build.
            onmounted: move |_event| async move {
                #[cfg(feature = "desktop")]
                match _event.get_client_rect().await {
                    Ok(rect) if rect.width() > 0.0 && rect.height() > 0.0 => {
                        applog::applog!(
                            "window: measured {:.0} by {:.0}",
                            rect.width(),
                            rect.height()
                        );
                        viewport.set((rect.width(), rect.height()));
                    }
                    Ok(_) => {}
                    Err(why) => applog::applog!("window: could not be measured: {why}"),
                }
            },
            onresize: move |event| {
                if let Ok(size) = event.get_content_box_size() {
                    let (was_w, was_h) = viewport();
                    // Two guards, and both are needed. The size is reported as
                    // a float, so an unchanged window can still differ in the
                    // last decimal place and would otherwise be written on
                    // every event. And writing re-renders, which reflows, which
                    // fires this again: without a threshold the two feed each
                    // other and the interface flickers continuously.
                    if (size.width - was_w).abs() >= 2.0 || (size.height - was_h).abs() >= 2.0 {
                        viewport.set((size.width, size.height));
                    }
                }
            },
            // Anywhere without its own menu simply swallows the right-click,
            // rather than letting the webview show its native one.
            oncontextmenu: move |event| event.prevent_default(),

            ribbon::TitleBar {}
            ribbon::TabStrip {}
            ribbon::Ribbon {}

            if let Some(message) = error {
                div { class: "error-banner",
                    {icon("warning", 15)}
                    span { class: "grow", "{message}" }
                    button {
                        class: "btn danger",
                        style: "padding: 3px 12px; font-size: 11px;",
                        onclick: move |_| state.write().dialog = Some(crate::state::Dialog::FixIssue),
                        "Fix this..."
                    }
                }
            }

            if show_timeline && view.has_chart() {
                gantt::TimelineBand {}
            }

            div {
                class: "workspace",
                oncontextmenu: move |event| event.prevent_default(),
                div { class: "viewbar", span { "{view.label()}" } }
                Workspace { view }
            }

            StatusBar {}
        }

        if let Some(page) = backstage {
            backstage::Backstage { page }
        }

        // Popup cell editors float above the grid so the table cannot clip them.
        if let Some((row, column)) = editing {
            match column {
                Column::Predecessors => rsx! { popups::PredecessorPopup { row } },
                Column::Successors => rsx! { popups::SuccessorPopup { row } },
                Column::Resources => rsx! { popups::ResourcePopup { row } },
                _ => rsx! {},
            }
        }

        if sync_open {
            versions::HistoryAndSync {}
        }

        if spelling_open {
            views::SpellingPanel {}
        }

        if let Some(menu) = menu {
            contextmenu::ContextMenuHost { menu }
        }

        if let Some(dialog) = dialog {
            dialogs::DialogHost { dialog }
        }

        // Last, and a direct child of the root. Panels handed to it are then
        // children of the window, which is what makes `absolute` mean the
        // same thing as `fixed` did. See `crate::floating`.
        floating::Host {}

        if splash {
            Splash {}
        }

        // Last, so it sits over the splash as well. On a first run the licence
        // is the only thing on screen and nothing behind it is reachable until
        // it has been answered.
        if let Some(greeting) = greeting {
            welcome::Welcome { greeting }
        }
    }
}

/// How often to look for a link handed over by another launch.
///
/// Slow enough to cost nothing, quick enough that clicking a link and having
/// the window answer feels like one action.
const HANDOFF_POLL_MILLIS: u64 = 250;

/// Bring this window to the front, for when something outside it asked for
/// something to happen in it.
///
/// Best effort, and it has to be said plainly: this asks, and a window manager
/// is entitled to refuse. Wayland compositors in particular will not let a
/// window that is not being interacted with raise itself, and will mark it as
/// wanting attention instead. There is nothing this side of the toolkit that
/// can do better, so what is here is the ask, and where it is declined the
/// plan still opens in the window that was already running rather than in a
/// second copy, which is the part that matters.
#[cfg(feature = "desktop")]
fn come_forward() {
    let window = dioxus::desktop::window();
    window.set_minimized(false);
    window.set_focus();
}

/// The webview-free build has no window handle to raise.
#[cfg(not(feature = "desktop"))]
fn come_forward() {}

/// Read the account's details again when this window comes back to the front.
///
/// Only when somebody was sent to the provider's account page, which is what
/// the flag records. The window gets and loses the focus for a dozen reasons
/// and none of the others is worth a request.
#[cfg(feature = "desktop")]
fn watch_for_account_changes(mut state: Signal<AppState>) {
    use dioxus::desktop::tao::event::{Event, WindowEvent};
    use dioxus::desktop::use_wry_event_handler;

    use_wry_event_handler(move |event, _| {
        if !matches!(
            event,
            Event::WindowEvent {
                event: WindowEvent::Focused(true),
                ..
            }
        ) {
            return;
        }
        // Read before the write, so a window regaining focus for any of the
        // ordinary reasons does not mark the state dirty and redraw.
        let due = {
            let read = state.read();
            // Nothing to refresh when nobody is signed in.
            read.session.is_some()
                && (read.account_page_opened
                    // Coming back from Manage account is the obvious moment,
                    // but it is not the only one: a browser tab left open can
                    // be used again without touching that button, and a second
                    // upload would otherwise never show. A floor keeps focus
                    // changes from turning into a stream of requests.
                    || read
                        .account_checked_at
                        .is_none_or(|at| at.elapsed() >= crate::state::ACCOUNT_RECHECK))
        };
        if !due {
            return;
        }
        {
            let mut write = state.write();
            write.account_page_opened = false;
            write.account_checked_at = Some(std::time::Instant::now());
        }
        collaborate::refresh_account(state);
    });
}

/// The webview-free build has no window events to hang this off, so the card
/// says what it last read until the next sign in.
#[cfg(not(feature = "desktop"))]
fn watch_for_account_changes(_state: Signal<AppState>) {}

/// Keep the window's own size to hand, for everything that places itself
/// against an edge.
///
/// Asked of the window rather than of the page. Measuring an element means
/// `NodeHandle::get_client_rect`, which takes the document's `RefCell`
/// unconditionally; a call that lands while a render is in flight panics with
/// "RefCell already borrowed" and takes the process with it. The first ask is
/// usually early enough to be safe, but the element has not been laid out that
/// early and honestly answers zero, so the old code asked again on a timer,
/// and a timer wakes whenever it wakes. That is what was killing the process a
/// third of a second after a plan was opened.
///
/// The window is not the document. Its size can be read at any moment and it
/// is the real answer besides: it stays right when the window is resized,
/// which the measured version never did.
#[cfg(all(feature = "native", not(feature = "desktop")))]
fn watch_the_window(mut viewport: Signal<crate::state::Viewport>) {
    use dioxus_native::winit::event::WindowEvent;

    let window = dioxus_native::use_window();
    // In logical pixels, the same units everything laid out by the stylesheet
    // is written in. The surface is in device pixels, which on a scaled
    // display is a different and much larger number.
    let read = {
        let window = window.clone();
        move || {
            let size = window.surface_size();
            let scale = window.scale_factor().max(0.01);
            (size.width as f64 / scale, size.height as f64 / scale)
        }
    };

    let first = read();
    use_hook(move || {
        if first.0 > 0.0 && first.1 > 0.0 {
            applog::applog!("window: {:.0} by {:.0}", first.0, first.1);
            viewport.set(first);
        }
    });

    dioxus_native::use_window_event(move |event, _| {
        if !matches!(event, WindowEvent::SurfaceResized(_)) {
            return;
        }
        let (width, height) = read();
        let (was_w, was_h) = viewport();
        // Written only on a real change. Writing re-renders, a re-render
        // reflows, and a reflow can produce another resize.
        if (width - was_w).abs() >= 1.0 || (height - was_h).abs() >= 1.0 {
            viewport.set((width, height));
        }
    });
}

/// The webview build measures its own root element instead.
#[cfg(feature = "desktop")]
fn watch_the_window(_viewport: Signal<crate::state::Viewport>) {}

/// Two internal windows side by side: a tab bar, a draggable splitter, and
/// each pane able to take the whole frame.
///
/// Every split view in the application is this same shape, so it lives once.
/// The plan puts its table and chart in it; the critical path report puts its
/// figures and the same chart. Copying the scaffolding instead would mean a
/// splitter that works in one view and not the other, which is exactly the
/// drift this exists to prevent.
#[component]
fn SplitPanes(
    left_name: String,
    left_subtitle: String,
    right_name: String,
    left_head: Element,
    left_body: Element,
    right_head: Element,
    right_body: Element,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    // The grip stays under the pointer however the panes are scrolled, so the
    // drag is tracked from where it started rather than from the grip itself.
    let mut resize_from = use_signal(|| None::<(f64, f64)>);
    let mut column_drag = use_context::<Signal<ColumnDrag>>();

    // Everything about scrolling lives here now, because both panes ride one
    // scroll container and neither of them owns it.
    let mut shifted = use_signal(Shifted::default);
    // How far down the panes are and how much of them is on screen. Written on
    // every frame of a scroll, and read by nothing but the bar on the right,
    // so a scroll redraws a thumb rather than a plan.
    let mut down = use_signal(|| (0.0f64, 0.0f64));
    // A sideways bar being dragged: which one, where the pointer started, and
    // what the offset was then. Tracked from where it started rather than from
    // the thumb, so a pointer moving faster than the thumb cannot drop it.
    let mut rail_drag = use_signal(|| RailDrag::None);
    // The bar down the right, being dragged: where the pointer started and what
    // the offset was then.
    let mut down_drag = use_signal(|| None::<(f64, f64)>);
    let reach = use_context::<Signal<Reach>>();
    let mut chart_scroll = use_context::<ChartScroll>().0;

    let (grid_width, focus, rows_len) = {
        let s = state.read();
        (s.table_view_width(), s.pane_focus, s.layout_rows().len())
    };
    // A zoom changes what a pixel of the chart means.
    //
    // Where a pane is scrolled to is held in pixels, and rezooming makes the
    // chart a different number of pixels wide, so the old offset points at a
    // different date, or past the end of the plan altogether, which is a pane
    // showing bare canvas.
    //
    // Rezooming takes the chart back to the start of the plan. That is what was
    // asked for and it is the honest answer for a timescale: a zoom is a change
    // of what you are looking at, not a nudge sideways, and the beginning is
    // the one place that means the same thing at every zoom.
    //
    // Read with `peek` rather than by calling: an effect that subscribes to
    // what it writes runs forever.
    let mut carried = use_signal(Reach::default);
    use_effect(move || {
        let now = reach();
        let before = *carried.peek();
        let rezoomed = (before.chart - now.chart).abs() >= 1.0;
        let recolumned = (before.table - now.table).abs() >= 1.0;
        if !rezoomed && !recolumned {
            return;
        }
        carried.set(now);
        let was = *shifted.peek();
        // The table's columns do not rescale when one is widened, they just
        // take more or less room, so its offset is only held in range.
        let table_port = state.peek().table_view_width();
        let next = Shifted {
            table: was.table.clamp(0.0, (now.table - table_port).max(0.0)),
            chart: if rezoomed { 0.0 } else { was.chart },
        };
        if next != was {
            shifted.set(next);
            // The chart draws the stretch of timescale it believes is on
            // screen, and that belief is now out of date too.
            let seen = *chart_scroll.peek();
            chart_scroll.set(PaneScroll { left: next.chart, ..seen });
        }
    });

    let panes = (use_context::<GridScroll>(), use_context::<ChartScroll>());

    let shift = shifted();
    let far = reach();
    // How wide each pane is: the table's is what the splitter was dragged to,
    // and the chart's is what is left of the window after it.
    //
    // Worked out rather than reported. Nothing in the split scrolls itself any
    // more, so nothing gets asked its size by the renderer, and measuring a box
    // means `get_client_rect`, which takes the document's `RefCell` and is the
    // call that was killing the process. It sizes the scrollbar thumbs and it
    // tells the chart how big a picture to draw itself into.
    let ports = {
        let (window_wide, _) = use_context::<Signal<crate::state::Viewport>>()();
        // The same arithmetic flexbox does, and it has to be, or the numbers
        // handed to the panes describe a layout that is not the one on screen.
        //
        // The table asks for whatever the splitter was dragged to, but it can
        // be squeezed: the chart has a floor of its own and takes what it needs
        // out of the table's width. Working the chart's width out from the
        // table's unsqueezed number left it at its own floor of a hundred and
        // twenty pixels while the pane on screen was half the window.
        let floor = crate::state::AppState::MIN_PANE;
        let room = (window_wide - SPLITTER_W - FRAME_W).max(floor * 2.0);
        let table = grid_width.clamp(floor, (room - floor).max(floor));
        Reach { table, chart: (room - table).max(floor) }
    };

    // The chart draws itself into a picture the width of its own pane, so it
    // has to be told what that is. Written only when it changes.
    {
        let mut seen = chart_scroll;
        let now = seen();
        if (now.width - ports.chart).abs() >= 1.0 {
            seen.set(PaneScroll { width: ports.chart, ..now });
        }
    }

    // The left column of all three rows is one width, so a column title, the
    // cells under it and the strip at the bottom are all cut off at the same
    // place. The five pixels are the splitter, which is part of the column.
    let left_cell = if focus == PaneFocus::TableOnly {
        "flex: 1 1 auto;".to_string()
    } else {
        // The splitter is a box of its own beside the pane now, not a child
        // of it, so the pane is exactly the width the splitter was dragged to.
        format!(
            "width: {}px; flex: 0 1 auto; min-width: {}px;",
            grid_width,
            crate::state::AppState::MIN_PANE
        )
    };

    let mut split_class = String::from("split");
    match focus {
        PaneFocus::TableOnly => split_class.push_str(" hide-chart"),
        PaneFocus::ChartOnly => split_class.push_str(" hide-table"),
        PaneFocus::Both => {}
    }
    if resize_from().is_some() {
        split_class.push_str(" resizing");
    }

    rsx! {
        // The splitter drag listens on the whole window, so a fast pointer
        // cannot outrun the 5 pixel grip and drop the drag.
        if resize_from().is_some() {
            div {
                class: "drag-shield col-resize",
                onmousemove: move |event| {
                    if let Some((from_x, from_width)) = resize_from() {
                        let moved = event.client_coordinates().x - from_x;
                        state.write().set_table_width(from_width + moved);
                    }
                },
                onmouseup: move |_| resize_from.set(None),
            }
        }

        div { class: "panes",
            // The two panes scroll sideways on their own, but their rows have
            // to stay level, so their vertical scroll is tied together.
            div { class: "pane-bar",
                if focus != PaneFocus::ChartOnly {
                    // Matched to the pane below it, which means matching how
                    // that pane is sized: fixed at the splitter's width while
                    // both are showing, and filling the workspace when the
                    // table is the only thing in it. A tab that stayed fixed
                    // while its pane grew would stop lining up, which is the
                    // fault this comment used to describe in the other
                    // direction.
                    div {
                        // Sized exactly as the pane below it is sized, which
                        // means sharing its flex arithmetic and not merely its
                        // number. The pane is `flex: 0 1 auto` with a floor and
                        // is squeezed by the chart's floor from the other side;
                        // a tab given the same width as a plain `flex: none`
                        // keeps the unsqueezed number and parts company with
                        // its own pane by exactly the amount flexbox took off.
                        style: if focus == PaneFocus::TableOnly {
                            "flex: 1 1 auto; display: flex;".to_string()
                        } else {
                            format!(
                                "width: {grid_width}px; flex: 0 1 auto; \
                                 min-width: {}px; display: flex;",
                                crate::state::AppState::MIN_PANE
                            )
                        },
                        PaneTab {
                            name: left_name,
                            subtitle: left_subtitle,
                            active: true,
                            grow: true,
                            maximised: focus == PaneFocus::TableOnly,
                            fills: "layout-left".to_string(),
                            splittable: true,
                            on_toggle: move |_| state.write().toggle_pane(PaneFocus::TableOnly),
                        }
                    }
                }
                // The right tab goes with the right pane. Leaving it behind is
                // a header for a pane that is not there.
                if focus != PaneFocus::TableOnly {
                    PaneTab {
                        name: right_name,
                        subtitle: String::new(),
                        active: true,
                        grow: true,
                        maximised: focus == PaneFocus::ChartOnly,
                        fills: "layout-right".to_string(),
                        splittable: true,
                        on_toggle: move |_| state.write().toggle_pane(PaneFocus::ChartOnly),
                    }
                }
            }

            div { class: "pane-frame",
                div {
                    class: "{split_class}",
                    onmouseup: move |_| {
                        // A row drag can end anywhere in the pane.
                        if state.read().drag_row.is_some() {
                            state.write().finish_drag();
                        }
                    },

                    // While a column is being widened the whole split listens,
                    // so a pointer moving faster than the grip cannot drop the
                    // drag. The grip is in the heading and the travelling is
                    // over the rows, which are two different boxes now, and
                    // this sheet is the one thing that covers both.
                    // ordinary clipped boxes again.
                    div { class: "pane left", style: "{left_cell}",
                        div { class: "pane-head",
                            div { class: "shift", style: "margin-left: -{shift.table}px;",
                                {left_head}
                            }
                        }
                        div {
                            class: "pane-body",
                            onwheel: move |event| wheel(event, &mut down, rows_len, panes),
                            div {
                                class: "shift",
                                style: "margin: -{down().0}px 0 0 -{shift.table}px;",
                                {left_body}
                            }
                        }
                        div { class: "pane-rail",
                            {thumb(false, shift.table, ports.table, far.table, Some(EventHandler::new(move |x| {
                                rail_drag.set(Some((RailSide::Table, x, shifted().table)));
                            })))}
                        }
                    }

                    Splitter { on_grab: move |x| {
                        let width = state.read().table_view_width();
                        resize_from.set(Some((x, width)));
                    } }

                    div { class: "pane right",
                        div { class: "pane-head",
                            div { class: "shift", style: "margin-left: -{shift.chart}px;",
                                {right_head}
                            }
                        }
                        div {
                            class: "pane-body",
                            onwheel: move |event| wheel(event, &mut down, rows_len, panes),
                            div {
                                // Both ways, and by the same number the
                                // timescale above is slid by, which is the
                                // only way head and body can be guaranteed to
                                // put a date on the same pixel. The chart used
                                // to carry its sideways offset itself, in the
                                // coordinates of the picture it drew itself
                                // into, because a picture is cut off at its
                                // own edges and a picture that has been slid is
                                // not. It is a tree of boxes now, and the pane
                                // cuts those off wherever they are.
                                class: "shift",
                                style: "margin: -{down().0}px 0 0 -{shift.chart}px;",
                                {right_body}
                            }
                        }
                        div { class: "pane-rail",
                            {thumb(false, shift.chart, ports.chart, far.chart, Some(EventHandler::new(move |x| {
                                rail_drag.set(Some((RailSide::Chart, x, shifted().chart)));
                            })))}
                        }
                    }

                    // The bar down the right, over both panes, because they
                    // fill the split and there is no column left to give it.
                    div { class: "vbar",
                        {thumb(true, down().0, down().1, rows_len as f64 * gantt::ROW_H,
                               Some(EventHandler::new(move |y| {
                                   down_drag.set(Some((y, down().0)));
                               })))}
                    }

                    // The sheets that carry a drag, and last of all.
                    //
                    // A sheet declared before the panes is behind them for the
                    // purpose of deciding what the pointer is over: this
                    // renderer answers that in tree order, and a z-index does
                    // not change its mind. So the thumb, which is drawn after,
                    // was taking every event the sheet was there to catch. The
                    // drag only moved while the pointer had run ahead of the
                    // thumb, and letting go over the thumb never reached the
                    // sheet that clears it, so the plan carried on following
                    // the mouse with nothing held down.
                    if column_drag().is_some() {
                        div {
                            class: "drag-shield col-resize",
                            onmousemove: move |event| {
                                if let Some((column, from_x, from_width)) = column_drag() {
                                    let moved = event.client_coordinates().x - from_x;
                                    state.write().set_column_width(column, from_width + moved);
                                }
                            },
                            onmouseup: move |_| column_drag.set(None),
                        }
                    }

                    // The same again for a scrollbar. A thumb is ten pixels
                    // thick and the pointer leaves it at once.
                    if rail_drag().is_some() {
                        div {
                            class: "drag-shield",
                            onmousemove: move |event| {
                                let Some((side, from_x, from_at)) = rail_drag() else {
                                    return;
                                };
                                let (port, content) = match side {
                                    RailSide::Table => (ports.table, far.table),
                                    RailSide::Chart => (ports.chart, far.chart),
                                };
                                if port <= 1.0 || content <= port {
                                    return;
                                }
                                // The thumb stands for the pane as a share of
                                // the plan, so one pixel of thumb is worth
                                // content-over-port pixels of plan.
                                let moved = event.client_coordinates().x - from_x;
                                let at = (from_at + moved * content / port)
                                    .clamp(0.0, content - port);
                                let now = shifted();
                                match side {
                                    RailSide::Table if now.table != at => {
                                        shifted.set(Shifted { table: at, ..now });
                                    }
                                    RailSide::Chart if now.chart != at => {
                                        shifted.set(Shifted { chart: at, ..now });
                                        // The chart draws only the stretch of
                                        // timescale on screen, worked out from
                                        // this. Coarse on purpose: the window
                                        // carries a margin either side, so it
                                        // changes far less often than the drag.
                                        let was = chart_scroll();
                                        let next =
                                            PaneScroll { left: at, width: port, ..was };
                                        if was.span() != next.span() {
                                            chart_scroll.set(next);
                                        }
                                    }
                                    _ => {}
                                }
                            },
                            onmouseup: move |_| rail_drag.set(None),
                        }
                    }

                    // And once more for the bar down the right.
                    if down_drag().is_some() {
                        div {
                            class: "drag-shield",
                            onmousemove: move |event| {
                                let Some((from_y, from_at)) = down_drag() else {
                                    return;
                                };
                                let (_, port) = down();
                                let content = rows_len as f64 * gantt::ROW_H;
                                if port <= 1.0 || content <= port {
                                    return;
                                }
                                let moved = event.client_coordinates().y - from_y;
                                let at = (from_at + moved * content / port)
                                    .clamp(0.0, content - port);
                                if (at - down().0).abs() >= 0.5 {
                                    down.set((at, port));
                                    settle(at, port, rows_len, panes);
                                }
                            },
                            onmouseup: move |_| down_drag.set(None),
                        }
                    }

                    // ---- the two panes ----------------------------------
                    //
                    // One box each, and each one clips its own contents once.
                    //
                    // They used to be three rows of the split, a row of
                    // headings above a row of bodies above a row of scrollbars,
                    // because the row of bodies was a scroll container and one
                    // scroll container holding both panes is how their rows
                    // stayed level. It also stopped either pane from clipping:
                    // the headings clipped, the scrollbars clipped, and the
                    // bodies, alone in being inside that container, did not, so
                    // the table's last columns were painted across the chart.
                    // Three rows with identical markup and only the one inside
                    // the scroller failing is as plain as evidence gets.
                    //
                    // So nothing here scrolls itself. Both panes are moved by
                    // the same two numbers, which is a stronger guarantee than
                    // two scroll positions kept in step, and the boxes are
                }
            }
        }
    }
}

/// The table on its own, in the same three rows the split uses.
///
/// The task sheet is one window rather than two, but a heading that scrolls
/// away is no better here than it is beside a chart, so it is built the same
/// way: titles above the box that scrolls, rows inside it, and a strip along
/// the bottom to move both sideways together.
#[component]
fn SoloGrid() -> Element {
    let mut shifted = use_signal(Shifted::default);
    let reach = use_context::<Signal<Reach>>();
    let grid_scroll = use_context::<GridScroll>().0;
    let mut column_drag = use_context::<Signal<ColumnDrag>>();
    let mut state = use_context::<Signal<AppState>>();
    let rows_len = state.read().layout_rows().len();
    let shift = shifted();
    let mut rail_drag = use_signal(|| RailDrag::None);
    let mut down_drag = use_signal(|| None::<(f64, f64)>);
    let mut down = use_signal(|| (0.0f64, 0.0f64));
    let panes = (use_context::<GridScroll>(), use_context::<ChartScroll>());
    {
        let (_, tall) = use_context::<Signal<crate::state::Viewport>>()();
        let seen = (tall - CHROME_H).max(120.0);
        if (down().1 - seen).abs() >= 1.0 {
            let at = down().0;
            down.set((at, seen));
        }
    }
    // One window, so the pane is the window less the frame around it.
    let port = {
        let (wide, _) = use_context::<Signal<crate::state::Viewport>>()();
        let seen = grid_scroll().width;
        if seen > 1.0 && seen < wide { seen } else { (wide - 16.0).max(120.0) }
    };

    rsx! {
        div { class: "split solo",
            div { class: "pane",
                div { class: "pane-head",
                    div { class: "shift", style: "margin-left: -{shift.table}px;",
                        grid::TaskGrid { part: Part::Head }
                    }
                }
                div {
                    class: "pane-body",
                    onwheel: move |event| wheel(event, &mut down, rows_len, panes),
                    div {
                        class: "shift",
                        style: "margin: -{down().0}px 0 0 -{shift.table}px;",
                        grid::TaskGrid { part: Part::Body }
                    }
                }
                div { class: "pane-rail",
                    {thumb(false, shift.table, port, reach().table, Some(EventHandler::new(move |x| {
                        rail_drag.set(Some((RailSide::Table, x, shifted().table)));
                    })))}
                }
            }
            div { class: "vbar",
                {thumb(true, down().0, down().1, rows_len as f64 * gantt::ROW_H,
                       Some(EventHandler::new(move |y| {
                           down_drag.set(Some((y, down().0)));
                       })))}
            }

            // Last, so the pointer finds it. See the note in `SplitPanes`.
            if column_drag().is_some() {
                div {
                    class: "drag-shield col-resize",
                    onmousemove: move |event| {
                        if let Some((column, from_x, from_width)) = column_drag() {
                            let moved = event.client_coordinates().x - from_x;
                            state.write().set_column_width(column, from_width + moved);
                        }
                    },
                    onmouseup: move |_| column_drag.set(None),
                }
            }
            if rail_drag().is_some() {
                div {
                    class: "drag-shield",
                    onmousemove: move |event| {
                        let Some((_, from_x, from_at)) = rail_drag() else { return };
                        let content = reach().table;
                        if port <= 1.0 || content <= port {
                            return;
                        }
                        let moved = event.client_coordinates().x - from_x;
                        let at = (from_at + moved * content / port).clamp(0.0, content - port);
                        let now = shifted();
                        if now.table != at {
                            shifted.set(Shifted { table: at, ..now });
                        }
                    },
                    onmouseup: move |_| rail_drag.set(None),
                }
            }
            if down_drag().is_some() {
                div {
                    class: "drag-shield",
                    onmousemove: move |event| {
                        let Some((from_y, from_at)) = down_drag() else { return };
                        let (_, tall) = down();
                        let content = rows_len as f64 * gantt::ROW_H;
                        if tall <= 1.0 || content <= tall {
                            return;
                        }
                        let moved = event.client_coordinates().y - from_y;
                        let at = (from_at + moved * content / tall).clamp(0.0, content - tall);
                        if (at - down().0).abs() >= 0.5 {
                            down.set((at, tall));
                            settle(at, tall, rows_len, panes);
                        }
                    },
                    onmouseup: move |_| down_drag.set(None),
                }
            }
        }
    }
}

/// Everything stacked above and below the panes: the ribbon, the timeline, the
/// view bar, the tabs, the sideways scrollbar and the status line.
///
/// Used only to guess how much of the plan is on screen, which decides how big
/// the thumb is drawn and how far the last row can be pulled up. Being a little
/// out costs a slightly wrong thumb, not a wrong plan.
const CHROME_H: f64 = 330.0;

/// What the frame around the panes takes off the window's width: the padding
/// either side of the panes and the border they are drawn in.
const FRAME_W: f64 = 16.0;

/// Move both panes together by a turn of the wheel.
///
/// Nothing in the split is a scroll container, so this is the whole of how a
/// wheel moves the plan. Both panes read the same number, which is why their
/// rows cannot come apart: there is one position, not two being kept in step.
fn wheel(
    event: Event<WheelData>,
    down: &mut Signal<(f64, f64)>,
    rows: usize,
    panes: (GridScroll, ChartScroll),
) {
    // A wheel reports in pixels on some devices and in lines on others, and a
    // line is not a pixel.
    let (at, port) = down();
    let step = match event.data().delta() {
        dioxus::html::geometry::WheelDelta::Pixels(v) => v.y,
        dioxus::html::geometry::WheelDelta::Lines(v) => v.y * 20.0,
        dioxus::html::geometry::WheelDelta::Pages(v) => v.y * port,
    };
    // A plan is a long list, and one notch moving three rows makes getting
    // down it feel like work.
    const SPEED: f64 = 1.8;
    let content = rows as f64 * gantt::ROW_H;
    let next = (at + step * SPEED).clamp(0.0, (content - port).max(0.0));
    if (next - at).abs() >= 0.5 {
        down.set((next, port));
        settle(next, port, rows, panes);
    }
}

/// Tell both panes which rows are on screen now.
///
/// Only when the answer has changed. Moving the panes is a transform and costs
/// nothing to redraw; working out which rows exist at all is the expensive
/// part, and that only changes once a block of them has gone by.
fn settle(at: f64, port: f64, rows: usize, (grid, chart): (GridScroll, ChartScroll)) {
    let mut grid = grid.0;
    let was = grid();
    let now = PaneScroll { top: at, height: port, ..was };
    if was.window(rows) != now.window(rows) {
        grid.set(now);
    }
    let mut chart = chart.0;
    let was = chart();
    let now = PaneScroll { top: at, height: port, ..was };
    if was.window(rows) != now.window(rows) {
        chart.set(now);
    }
}

/// How wide the splitter is. The left column of every row of the split
/// includes it, so a title, a cell and a scrollbar all end at the same pixel.
pub const SPLITTER_W: f64 = 5.0;

/// The grip between the panes.
///
/// Drawn once per row of the split so that the line between the panes runs
/// unbroken from the column titles to the scrollbars. Each one can be dragged,
/// because a grip you can see and not pull is worse than no grip.
#[component]
fn Splitter(on_grab: EventHandler<f64>) -> Element {
    rsx! {
        div {
            class: "splitter",
            onmousedown: move |event| {
                event.prevent_default();
                event.stop_propagation();
                on_grab.call(event.client_coordinates().x);
            },
        }
    }
}

/// One pane's sideways scrollbar.
///
/// A box that scrolls and holds nothing but a rule as wide as the pane's
/// contents. The renderer gives it a real bar with a real thumb that can be
/// dragged, clicked beside and thrown a wheel at, and what it reports is
/// handed back so the heading and the rows can be shifted by it. Drawing a
/// thumb by hand would have meant knowing the strip's own width in pixels, and
/// a window that has been resized since it was measured cannot be asked.
#[component]
fn Rail(content: f64, port: f64, at: f64, on_grab: EventHandler<f64>) -> Element {
    rsx! {
        div { class: "rail",
            {thumb(false, at, port, content, Some(on_grab))}
        }
    }
}

/// Where a scrollbar drag started: the pointer, and the offset it started from.
type RailDrag = Option<(RailSide, f64, f64)>;

/// Which pane's sideways bar is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailSide {
    Table,
    Chart,
}

/// A scrollbar thumb that is actually there to be seen.
///
/// The renderer draws its own, but as an overlay that fades out a moment after
/// the last scroll, so most of the time there is nothing on screen to say that
/// there is more to the right or below. This one is painted from the numbers
/// the scroll events report and never fades.
///
/// It takes no pointer. The real thumb is underneath it in the same place, and
/// that is the one that knows how to be dragged, so letting this one catch the
/// press would be taking the drag away from the thing that can do it.
fn thumb(
    down: bool,
    at: f64,
    port: f64,
    content: f64,
    on_grab: Option<EventHandler<f64>>,
) -> Element {
    // Nothing to say when everything already fits.
    if content <= port + 1.0 || port <= 1.0 {
        return rsx! {};
    }
    let share = (port / content).clamp(0.04, 1.0);
    // Of the content, not of the scrollable remainder: the thumb then stands
    // for the piece of the plan on screen, which is what it is for.
    let from = (at / content).clamp(0.0, 1.0 - share);
    let style = if down {
        format!("top: {}%; height: {}%;", from * 100.0, share * 100.0)
    } else {
        format!("left: {}%; width: {}%;", from * 100.0, share * 100.0)
    };
    let class = if down { "thumb down" } else { "thumb across" };
    match on_grab {
        // A bar that can be taken hold of. The renderer's own thumb is under
        // this one and can be dragged, but only while it is showing, and it
        // fades a moment after the last scroll: "a faded-out thumb doesn't
        // capture", says the comment beside the code that decides. So at rest
        // there was nothing there to grab, which is exactly what a scrollbar
        // is for.
        Some(grab) => rsx! {
            div {
                class: "{class} live",
                style: "{style}",
                onmousedown: move |event| {
                    event.prevent_default();
                    event.stop_propagation();
                    grab.call(event.client_coordinates().x);
                },
            }
        },
        None => rsx! { div { class: "{class}", style: "{style}" } },
    }
}

/// One internal window: a title tab plus whatever it frames.
#[component]
fn PaneTab(
    name: String,
    subtitle: String,
    active: bool,
    grow: bool,
    maximised: bool,
    /// Which half this pane fills when it takes over, so the button can show it.
    fills: String,
    splittable: bool,
    on_toggle: EventHandler<()>,
) -> Element {
    let mut class = String::from("pane-tab");
    if grow {
        class.push_str(" grow");
    }
    if active {
        class.push_str(" active");
    }
    // The glyph shows the layout the click produces: this pane filling the
    // frame, or the split coming back.
    let glyph = if maximised { "layout-split" } else { fills.as_str() };
    let hint = if maximised {
        "Restore the split"
    } else {
        "Fill the frame with this pane"
    };

    rsx! {
        div { class: "{class}",
            span { class: "pane-dot" }
            span { class: "pane-name", "{name}" }
            if !subtitle.is_empty() {
                span { class: "pane-sub", "{subtitle}" }
            }
            if splittable {
                button {
                    class: "iconbtn",
                    title: "{hint}",
                    onclick: move |_| on_toggle.call(()),
                    {icon(glyph, 13)}
                }
            }
        }
    }
}

#[component]
fn Workspace(view: ViewKind) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    // Provided above the panes rather than inside the split, so that a pane
    // handed to the split as a finished element finds them whichever way the
    // renderer decides to parent it.
    use_context_provider(|| Signal::new(Reach::default()));
    use_context_provider(|| Signal::new(ColumnDrag::None));
    use_context_provider(|| GridScroll(Signal::new(PaneScroll::default())));
    use_context_provider(|| ChartScroll(Signal::new(PaneScroll::default())));
    // The splitter and the pane widths belong to SplitPanes now; what is left
    // here is only what the single window views need.
    let (rows, filter) = {
        let s = state.read();
        (s.visible_rows().len(), s.filter)
    };
    let filter_note = if filter == crate::state::TaskFilter::All {
        format!("{rows} rows")
    } else {
        format!("{rows} rows \u{00b7} {}", filter.label())
    };

    match view {
        // The table and the chart are two internal windows sharing one scroll,
        // so their rows stay aligned however far down you go.
        // The table and the chart are two internal windows sharing one
        // scroll, so their rows stay aligned however far down you go.
        ViewKind::GanttChart | ViewKind::TrackingGantt => rsx! {
            SplitPanes {
                left_name: "Entry Table".to_string(),
                left_subtitle: filter_note.clone(),
                right_name: view.label().to_string(),
                left_head: rsx! { grid::TaskGrid { part: Part::Head } },
                left_body: rsx! { grid::TaskGrid { part: Part::Body } },
                right_head: rsx! { gantt::GanttChart { rows: None, interactive: true, part: Part::Head } },
                right_body: rsx! { gantt::GanttChart { rows: None, interactive: true, part: Part::Body } },
            }
        },

        // The critical path is two windows for the same reason the plan is:
        // the report says which tasks are on the chain, the chart says when,
        // and either on its own is half the answer.
        ViewKind::CriticalPath => {
            let chain: Vec<usize> = {
                let s = state.read();
                aop_core::critical_path(&s.project)
                    .into_iter()
                    .map(|step| step.index)
                    .collect()
            };
            let steps = chain.len();
            let width = state.read().table_view_width();

            rsx! {
                SplitPanes {
                    left_name: "Critical Path".to_string(),
                    left_subtitle: format!("{steps} on the chain"),
                    right_name: "Chart".to_string(),
                    // The report has no heading of its own to pin: it is a
                    // page, not a table, so its head row is empty and the
                    // page fills the body.
                    left_head: rsx! {},
                    left_body: rsx! {
                        div { class: "cp-report", style: "width: {width}px;",
                            views::ReportPage { kind: view }
                        }
                    },
                    right_head: rsx! {
                        gantt::GanttChart { rows: Some(chain.clone()), interactive: false, part: Part::Head }
                    },
                    right_body: rsx! {
                        gantt::GanttChart { rows: Some(chain), interactive: false, part: Part::Body }
                    },
                }
            }
        }

        // Every other view is a single internal window.
        _ => rsx! {
            div { class: "panes",
                div { class: "pane-bar",
                    PaneTab {
                        name: view.label().to_string(),
                        subtitle: match view {
                            ViewKind::ResourceSheet | ViewKind::ResourceUsage | ViewKind::TeamPlanner => {
                                let count = state.read().project.resources.len();
                                format!("{count} resources")
                            }
                            _ => filter_note.clone(),
                        },
                        active: true,
                        grow: true,
                        maximised: false,
                        fills: "layout-split".to_string(),
                        // A single-pane view has nothing to maximise into.
                        splittable: false,
                        on_toggle: move |_| state.write().view = ViewKind::GanttChart,
                    }
                }
                div { class: "pane-frame",
                    match view {
                        ViewKind::TaskSheet => rsx! { SoloGrid {} },
                        ViewKind::TaskUsage => rsx! { views::TaskUsage {} },
                        ViewKind::ResourceUsage => rsx! { views::ResourceUsage {} },
                        ViewKind::NetworkDiagram => rsx! { views::NetworkDiagram {} },
                        ViewKind::CalendarView => rsx! { views::CalendarView {} },
                        ViewKind::Burndown
                        | ViewKind::Burnup
                        | ViewKind::Velocity
                        | ViewKind::CriticalPath => rsx! { views::ReportPage { kind: view } },
                        ViewKind::TeamPlanner => rsx! {
                            div { class: "split", views::TeamPlanner {} }
                        },
                        _ => rsx! { views::ResourceSheet {} },
                    }
                }
            }
        },
    }
}

#[component]
fn StatusBar() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();

    let tasks = s.project.tasks.len();
    let percent = s.project.percent_complete();
    let currency = s.project.currency_symbol.clone();
    let zoom = s.zoom;
    let status = s.status.clone();
    let available = s.update_found.as_ref().map(|found| found.version.clone());

    let (finish, cost, work, critical, overallocated) = match &s.report {
        Ok(report) => (
            crate::state::format_date(report.finish),
            format!("{currency}{:.2}", report.total_cost),
            format_work(report.total_work_minutes),
            report.critical_task_count,
            report.overallocations.len(),
        ),
        Err(_) => (
            "-".into(),
            "-".into(),
            "-".into(),
            0,
            0,
        ),
    };
    let duration = s
        .report
        .as_ref()
        .map(|r| format_duration(r.duration_minutes))
        .unwrap_or_else(|_| "-".into());

    rsx! {
        div { class: "statusbar",
            span { class: "chip", "{status}" }
            span { class: "chip", "New Tasks: Auto Scheduled" }
            div { class: "grow" }
            span { class: "chip", "{tasks} tasks" }
            span { class: "chip", "{critical} critical" }
            span { class: "chip", "Duration {duration}" }
            span { class: "chip", "Finish {finish}" }
            span { class: "chip", "Work {work}" }
            span { class: "chip", "Cost {cost}" }
            span { class: "chip", "{percent}% complete" }
            if overallocated > 0 {
                span { class: "chip warn", "\u{26a0} {overallocated} overallocated" }
            }
            // Quiet, and out of the way. A new version is worth knowing about
            // and is not worth a modal in front of somebody's work.
            if let Some(version) = available {
                button {
                    class: "chip link",
                    title: "A newer version is available",
                    onclick: move |_| state.write().dialog = Some(crate::state::Dialog::UpdateAvailable),
                    "Version {version} available"
                }
            }
            div { class: "zoom-slider",
                button { class: "zoom-btn", title: "Zoom Out",
                    onclick: move |_| { let z = state.read().zoom.zoom_out(); state.write().zoom = z; },
                    "\u{2212}"
                }
                span { class: "zoom-label", "{zoom.label()}" }
                button { class: "zoom-btn", title: "Zoom In",
                    onclick: move |_| { let z = state.read().zoom.zoom_in(); state.write().zoom = z; },
                    "+"
                }
            }
        }
    }
}

/// Run whatever the keyboard is pointed at.
///
/// The key press is rendered in the same form a binding is written in and
/// looked up in the map, rather than matched against a fixed list here. That is
/// what lets the same table be listed in the settings, rebound, and saved.
fn handle_shortcut(state: &mut Signal<AppState>, event: Event<KeyboardData>) {
    // Never steal keys while a cell editor, a dialog or a menu has focus, or
    // while a start up page is waiting to be answered: nothing behind one of
    // those is meant to be reachable, and a shortcut would reach it.
    {
        let s = state.read();
        if s.editing.is_some()
            || s.dialog.is_some()
            || s.backstage.is_some()
            || s.context_menu.is_some()
            || !s.greetings.is_empty()
        {
            return;
        }
    }

    // Escape puts down whatever the chart is holding. It is not a bindable
    // action because it never means anything else: a planner who has armed a
    // shape by mistake should not have to find the menu again to disarm it.
    if event.key() == Key::Escape {
        let mut writer = state.write();
        if writer.draw_tool.is_some() {
            writer.arm_draw_tool_off();
            return;
        }
        if writer.selected_drawing.is_some() {
            writer.selected_drawing = None;
            return;
        }
    }

    let Some(pressed) = keymap::shortcut_for(&event.key(), event.modifiers()) else {
        return;
    };
    let Some(action) = state.read().keys.action_for(&pressed) else {
        return;
    };

    run_action(state, action);
    event.prevent_default();
}

/// Carry out one action, however it was asked for.
fn run_action(state: &mut Signal<AppState>, action: keymap::Action) {
    use keymap::Action;

    match action {
        Action::New => state.write().backstage = Some(BackstagePage::New),
        Action::Open => state.write().backstage = Some(BackstagePage::Open),
        Action::Save => {
            let saved = state.write().save();
            if !saved {
                state.write().backstage = Some(BackstagePage::SaveAs);
            }
        }
        Action::SaveAs => state.write().backstage = Some(BackstagePage::SaveAs),
        Action::Print => state.write().backstage = Some(BackstagePage::Print),
        Action::Export => state.write().backstage = Some(BackstagePage::Export),
        Action::CloseProject => state
            .write()
            .guard(crate::state::PendingAction::CloseProject),

        Action::Undo => state.write().undo(),
        Action::Redo => state.write().redo(),
        Action::Cut => state.write().cut_selected(),
        Action::Copy => state.write().copy_selected(),
        Action::Paste => state.write().paste(),
        Action::Delete => state.write().delete_selected(),
        Action::EditCell => {
            let row = state.read().primary();
            if let Some(row) = row {
                state.write().editing = Some((row, crate::state::Column::Name));
            }
        }

        Action::InsertTask => state.write().insert_task(),
        Action::InsertMilestone => state.write().insert_milestone(),
        Action::InsertSummary => state.write().insert_summary(),
        Action::Indent => state.write().indent_selected(),
        Action::Outdent => state.write().outdent_selected(),
        Action::MoveUp => state.write().move_selected(-1),
        Action::MoveDown => state.write().move_selected(1),
        Action::Link => state.write().link_selected(),
        Action::Unlink => state.write().unlink_selected(),
        Action::TaskInformation => {
            let row = state.read().primary();
            if let Some(row) = row {
                state.write().dialog = Some(crate::state::Dialog::TaskInformation(row));
            }
        }
        Action::ToggleActive => state.write().toggle_active(),
        Action::ManuallySchedule => state.write().set_task_mode(TaskMode::Manual),
        Action::AutoSchedule => state.write().set_task_mode(TaskMode::Auto),
        Action::RespectLinks => state.write().respect_links(),

        Action::ProjectInformation => {
            state.write().dialog = Some(crate::state::Dialog::ProjectInformation)
        }
        Action::AssignResources => {
            state.write().dialog = Some(crate::state::Dialog::AssignResources)
        }
        Action::SetBaseline => state.write().set_baseline(),
        Action::ScrollToTask => state.write().scroll_to_task(),

        Action::ZoomIn => {
            let zoom = state.read().zoom.zoom_in();
            state.write().zoom = zoom;
        }
        Action::ZoomOut => {
            let zoom = state.read().zoom.zoom_out();
            state.write().zoom = zoom;
        }
        Action::ToggleTimeline => {
            let on = state.read().show_timeline;
            state.write().show_timeline = !on;
        }
        Action::ToggleCriticalPath => {
            let on = state.read().show_critical;
            state.write().show_critical = !on;
        }
        Action::ToggleOutlineNumber => {
            let on = state.read().show_outline_number;
            state.write().show_outline_number = !on;
        }
        Action::ExpandAll => state.write().expand_all(false),
        Action::CollapseAll => state.write().expand_all(true),
        Action::MaximiseTable => state.write().toggle_pane(PaneFocus::TableOnly),
        Action::MaximiseChart => state.write().toggle_pane(PaneFocus::ChartOnly),

        Action::SelectDown => {
            let next = state.read().primary().map(|row| row + 1);
            let limit = state.read().project.tasks.len();
            if let Some(row) = next
                && row < limit {
                    state.write().select(row);
                }
        }
        Action::SelectUp => {
            let previous = state.read().primary().and_then(|row| row.checked_sub(1));
            if let Some(row) = previous {
                state.write().select(row);
            }
        }
    }
}

/// Zoom levels are exposed here so the status bar and ribbon agree on order.
#[allow(dead_code)]
const ZOOM_ORDER: [Zoom; 4] = Zoom::ORDER;

#[cfg(test)]
mod shield_order_tests {
    /// The sheets that carry a drag have to be declared after everything they
    /// are meant to cover.
    ///
    /// This renderer decides what the pointer is over in tree order, and a
    /// z-index does not change its mind. A sheet written before the panes is
    /// therefore behind them: the scrollbar thumb, drawn after, took every
    /// event the sheet existed to catch, so the drag only moved while the
    /// pointer had run ahead of the thumb, and letting go over the thumb never
    /// reached the sheet that clears the drag, leaving the plan following a
    /// mouse with nothing held down.
    #[test]
    fn a_drag_sheet_comes_after_what_it_covers() {
        let source = include_str!("main.rs");
        for shell in ["fn SplitPanes(", "fn SoloGrid("] {
            let at = source.find(shell).expect(shell);
            let body = &source[at..];
            let end = body.find("\n}\n").unwrap_or(body.len());
            let body = &body[..end];
            let last_pane = body.rfind("class: \"vbar\"").expect("the bar down the right");
            for sheet in ["column_drag().is_some()", "rail_drag().is_some()", "down_drag().is_some()"] {
                if let Some(at) = body.find(sheet) {
                    assert!(
                        at > last_pane,
                        "{shell}: the sheet for `{sheet}` is declared before the panes, \
                         so the pointer will never reach it"
                    );
                }
            }
        }
    }
}
