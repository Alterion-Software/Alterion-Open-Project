//! Alterion Open Project: a project scheduler with a critical path engine,
//! a Gantt chart, and a ribbon that follows Microsoft Project's layout.

// Windows gives a program a console unless it is told otherwise, so a release
// build would open a black terminal behind the window. Kept in debug builds,
// where printing to a console is how anything gets diagnosed.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
mod handoff;
mod keymap;
mod icons;
mod macros;
mod popups;
mod preview;
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
#[cfg(feature = "desktop")]
use aop_core::APP_NAME;

use crate::icons::icon;
use crate::state::{AppState, BackstagePage, Column, PaneFocus, ViewKind, Zoom};

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

    // A link clicked while this application is already open belongs in the
    // window that is already open. Two copies of one plan, each with its own
    // change log and its own sync cursor, is the drift the sync protocol has a
    // whole case for detecting, and starting one on purpose would be making it
    // happen.
    if handoff::claim(link_argument().as_deref()) == handoff::Claim::HandedOver {
        return;
    }

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
    if handoff::claim(link_argument().as_deref()) == handoff::Claim::HandedOver {
        return;
    }
    dioxus_native::launch(App);
}

/// The link this launch was asked to open, if it was asked to open one.
///
/// Told from a file path by its scheme and by nothing else. Guessing would be
/// guessing about whether a network request gets made, and the desktop hands
/// over a URL and a path through the same argument.
fn link_argument() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|argument| cloud::share::looks_like_a_link(argument))
}

/// The stylesheet, in a component of its own so it is written once.
///
/// It takes no props, so it never re-renders. Left inside `App` it would be
/// diffed on every state write, and re-setting a two thousand line stylesheet
/// is the kind of thing an engine can answer with a repaint of everything.
#[component]
fn Stylesheet() -> Element {
    rsx! { style { dangerous_inner_html: theme::CSS } }
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
                    dangerous_inner_html: crate::brand::LOGO_SVG,
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
    let mut viewport = use_context_provider(|| Signal::new(crate::state::Viewport::default()));

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

    // The live socket runs on a thread of its own and the plan may only be
    // written where the interface does, so what arrives is collected here
    // rather than pushed from the socket. Cheap when nothing is connected:
    // `poll_live` returns at once when there is no socket.
    use_future(move || async move {
        let interval = std::time::Duration::from_millis(crate::state::LIVE_POLL_MILLIS);
        loop {
            tokio::time::sleep(interval).await;
            // Asked before the write, because taking a write handle is what
            // marks the state dirty. Doing that every tick would redraw the
            // whole window three times a second for a socket nobody opened.
            if state.read().live.is_some() {
                // Peeked rather than read: a future that subscribed to the
                // pointer would be torn down and started again every time the
                // mouse moved.
                let at = *pointing.peek();
                let mut writer = state.write();
                writer.poll_live();
                writer.announce(at);
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
        // After the splash, so a slow name resolution cannot be the first
        // thing a start up does.
        tokio::time::sleep(std::time::Duration::from_secs(updates::STARTUP_DELAY_SECONDS)).await;
        updates::ask_in_background(state);
    });

    // Links handed over by later launches. Looked for on a timer because they
    // arrive on a thread of its own and the plan may only be written where the
    // interface runs, which is the same arrangement the live socket uses.
    // Nothing is written unless something actually arrived, so an application
    // nobody is sending links to is not redrawn by this.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(HANDOFF_POLL_MILLIS)).await;
            // The last one, because they are all a request to open a plan and
            // only one plan is open at a time. Asking about three in a row
            // would be three dialogs for one intention.
            if let Some(link) = handoff::arrivals().pop() {
                state.write().open_link_asked(&link);
                come_forward();
            }
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
            onkeydown: move |event| handle_shortcut(&mut state, event),
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

/// Keeps the table and the chart scrolled to the same row.
///
/// Each pane owns its horizontal scrollbar, which means each is its own scroll
/// container, so the vertical positions have to be linked explicitly.
#[component]
fn SyncPaneScroll() -> Element {
    use_effect(move || {
        document::eval(
            r#"
            (function () {
              const grid = document.querySelector('.grid-pane');
              const chart = document.querySelector('.chart-pane');
              if (!grid || !chart) return;
              if (grid.dataset.aopSynced === '1') return;
              grid.dataset.aopSynced = '1';

              let echo = false;
              const link = (from, to) => from.addEventListener('scroll', () => {
                if (echo) return;
                echo = true;
                to.scrollTop = from.scrollTop;
                echo = false;
              }, { passive: true });

              link(grid, chart);
              link(chart, grid);

              // A plan is a long list, and one wheel notch moving three rows
              // makes getting down it feel like work. The panes are linked
              // above, so boosting either one carries the other with it.
              const SPEED = 1.8;
              const boost = (pane) => pane.addEventListener('wheel', (event) => {
                // Leave zoom and sideways scrolling alone: those are other
                // gestures that happen to arrive on the same event.
                if (event.ctrlKey || event.shiftKey) return;
                if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
                // deltaMode counts lines (1) or pages (2) rather than pixels
                // on some devices, so it is converted before being scaled.
                const unit = event.deltaMode === 1
                  ? 16
                  : event.deltaMode === 2 ? pane.clientHeight : 1;
                event.preventDefault();
                pane.scrollTop += event.deltaY * unit * SPEED;
              }, { passive: false });

              boost(grid);
              boost(chart);
            })();
            "#,
        );
    });

    rsx! {}
}

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
    left: Element,
    right: Element,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    // The grip stays under the pointer however the panes are scrolled, so the
    // drag is tracked from where it started rather than from the grip itself.
    let mut resize_from = use_signal(|| None::<(f64, f64)>);

    let (grid_width, focus) = {
        let s = state.read();
        (s.table_view_width(), s.pane_focus)
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
            SyncPaneScroll {}
            div { class: "pane-bar",
                if focus != PaneFocus::ChartOnly {
                    // flex: none, or the bar would shrink this tab and it
                    // would stop lining up with the pane below it.
                    div { style: "width: {grid_width}px; flex: none; display: flex;",
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

                    div { class: "pane-left",
                        {left}
                        div {
                            class: "splitter",
                            onmousedown: move |event| {
                                event.prevent_default();
                                event.stop_propagation();
                                let width = state.read().table_view_width();
                                resize_from.set(Some((event.client_coordinates().x, width)));
                            },
                        }
                    }
                    {right}
                }
            }
        }
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
                left: rsx! { grid::TaskGrid {} },
                right: rsx! { gantt::GanttChart { rows: None, interactive: true } },
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
                    left: rsx! {
                        div { class: "grid-pane cp-report", style: "width: {width}px;",
                            views::ReportPage { kind: view }
                        }
                    },
                    right: rsx! {
                        gantt::GanttChart { rows: Some(chain), interactive: false }
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
                        ViewKind::TaskSheet => rsx! {
                            div { class: "split",
                                div { class: "pane-left", grid::TaskGrid {} }
                            }
                        },
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
