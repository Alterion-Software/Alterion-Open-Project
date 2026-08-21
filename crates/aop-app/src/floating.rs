//! Panels that have to hang over the window rather than sit inside it.
//!
//! A dropdown belongs visually to the button that opened it and belongs
//! structurally nowhere near it. The list has to be able to cover whatever is
//! underneath, and it has to be able to leave the box it was opened from: the
//! ribbon is ninety four pixels tall, collapses to nothing, and clips its
//! contents to do so, which a list dropped from a ribbon button must escape.
//!
//! In a browser `position: fixed` says all of that in one word, because fixed
//! means "against the window" no matter how deeply nested the element is. That
//! is why every panel here was written with it.
//!
//! **It is a word not every layout engine has.** Taffy has `relative` and
//! `absolute` and no `fixed` at all, and its `absolute` means "against my
//! parent" rather than "against the nearest positioned ancestor". So a fixed
//! panel is not taken out of the flow, and a `top` of four hundred pixels then
//! means four hundred below wherever the panel already happened to sit, which
//! is how a dropdown ends up floating far under its own button.
//!
//! The fix is to make the two sentences mean the same thing: put the panel
//! where its parent **is** the window. A panel rendered at the root, given
//! `position: absolute`, lands on the same pixel under both readings, and the
//! coordinates the opener already computes from the click are correct as they
//! stand.
//!
//! So a component that opens a panel does not render it. It hands it here, and
//! [`Host`] renders it at the root.

use dioxus::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Who put the current panel up.
///
/// Two menus must not be able to take each other's panel down. Only the opener
/// may clear what it opened, so a component closing while another is showing
/// leaves that one alone.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// A claim on the layer, one per component that can open a panel.
#[derive(Clone, Copy, PartialEq)]
pub struct Claim(u64);

/// Take out a claim. Called once per component, from a hook.
pub fn claim() -> Claim {
    Claim(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// One row of a panel.
///
/// Flat rather than a tree, and data rather than markup, for a reason worth
/// keeping. The first attempt at this stored the panel as an `Element` built
/// with `rsx!` inside an effect. Node paths are bookkeeping from a render
/// pass, so a node built outside one and kept across renders goes stale, and a
/// later mutation walks a path into a node that is no longer there:
/// `invalid key`, from `blitz_dom`'s mutator, on the next interaction.
/// Describing the panel and letting the host build it means every node is made
/// during the render that shows it.
#[derive(Clone, PartialEq)]
pub struct Row {
    pub value: String,
    pub label: String,
    /// An icon name from [`crate::icons`], for menus that carry one.
    pub glyph: Option<String>,
}

impl Row {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into(), glyph: None }
    }

    pub fn with_glyph(mut self, glyph: &str) -> Self {
        if !glyph.is_empty() {
            self.glyph = Some(glyph.to_string());
        }
        self
    }

    /// A rule between groups rather than something to pick.
    pub fn is_separator(&self) -> bool {
        self.value == "-"
    }
}

/// A panel hanging over the window: what is in it, and where it goes.
#[derive(Clone, PartialEq)]
pub struct Panel {
    pub owner: Claim,
    pub at: (f64, f64),
    pub min_width: f64,
    /// A fixed width, for a list that has to match the control it drops from.
    pub width: Option<f64>,
    pub rows: Vec<Row>,
    /// Which row is shown as chosen.
    pub chosen: String,
    /// What to say when a filter has left nothing.
    pub empty: Option<String>,
    pub on_pick: EventHandler<String>,
    pub on_close: EventHandler<()>,
}

impl Panel {
    /// Whether two panels would draw identically.
    ///
    /// Everything except the callbacks, which are rebuilt on every describe
    /// and so never compare equal, and which cannot differ while the rest is
    /// the same anyway: they close over the same component's signals.
    fn looks_the_same_as(&self, other: &Panel) -> bool {
        self.owner == other.owner
            && self.at == other.at
            && self.min_width == other.min_width
            && self.width == other.width
            && self.chosen == other.chosen
            && self.empty == other.empty
            && self.rows == other.rows
    }
}

/// The panel currently over the window.
#[derive(Clone, Copy)]
pub struct Layer(Signal<Option<Panel>>);

impl Layer {
    pub fn new() -> Self {
        Layer(Signal::new(None))
    }

    /// Put a panel up, replacing whatever was there.
    ///
    /// Replacing rather than refusing: opening a second menu while one is open
    /// should show the second, which is what every other application does.
    ///
    /// Silent when nothing has actually changed. A panel is rebuilt every time
    /// the effect that describes it runs, and it carries fresh `EventHandler`s
    /// each time, so it never compares equal to the one already up even when
    /// every visible part of it is identical. Setting the signal regardless
    /// re-renders this layer for nothing, and doing that often enough starves
    /// the frame the interface is drawn in.
    pub fn put(&mut self, panel: Panel) {
        if self.0.read().as_ref().is_some_and(|up| up.looks_the_same_as(&panel)) {
            return;
        }
        self.0.set(Some(panel));
    }

    /// Take the panel down, if it is yours.
    pub fn clear(&mut self, owner: Claim) {
        if self.0.read().as_ref().is_some_and(|panel| panel.owner == owner) {
            self.0.set(None);
        }
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders whatever panel is up, at the root of the application.
///
/// A direct child of the root and nothing else, which is the entire point: its
/// children's parent is the window, so `absolute` reaches the window the same
/// way `fixed` was meant to.
/// Renders whatever panel is up, at the root of the application.
///
/// A direct child of the root and nothing else, which is the entire point: its
/// children's parent is the window, so `absolute` reaches the window the same
/// way `fixed` was meant to.
///
/// **The shape it returns never changes**, and that is deliberate rather than
/// tidy. Returning an empty `rsx!` when nothing is up and a two element tree
/// when something is gives this component two different templates, and every
/// row being a `button` except separators being a `div` gives its list two
/// more. Dioxus addresses nodes by a path through the template it expects, so
/// a template that changes underneath a diff sends `assign_node_id` walking
/// into a child that is not there, and `blitz_dom` says `invalid key` and
/// takes the process down. Same markup every time, hidden when there is
/// nothing to show, and one element type for every row.
#[component]
pub fn Host() -> Element {
    let layer = use_context::<Layer>();
    let panel = layer.0.read().clone();

    let showing = panel.is_some();
    let (x, y) = panel.as_ref().map(|p| p.at).unwrap_or((0.0, 0.0));
    let sizing = match panel.as_ref().and_then(|p| p.width) {
        Some(w) => format!("width: {w}px;"),
        None => format!("min-width: {}px;", panel.as_ref().map(|p| p.min_width).unwrap_or(0.0)),
    };
    let hidden = if showing { "" } else { "display: none;" };
    let chosen = panel.as_ref().map(|p| p.chosen.clone()).unwrap_or_default();
    let rows: Vec<Row> = panel.as_ref().map(|p| p.rows.clone()).unwrap_or_default();
    let empty = panel
        .as_ref()
        .filter(|p| p.rows.is_empty())
        .and_then(|p| p.empty.clone())
        .unwrap_or_default();
    let on_close = panel.as_ref().map(|p| p.on_close);
    let on_pick = panel.as_ref().map(|p| p.on_pick);

    rsx! {
        div {
            class: "ctx-scrim",
            style: "{hidden}",
            onclick: move |event| {
                event.stop_propagation();
                if let Some(close) = on_close { close.call(()); }
            },
            oncontextmenu: move |event| {
                event.prevent_default();
                if let Some(close) = on_close { close.call(()); }
            },
        }
        div {
            class: "dd-list",
            style: "left: {x.max(4.0)}px; top: {y.max(4.0)}px; {sizing} {hidden}",
            onclick: move |event| event.stop_propagation(),

            div { class: "dd-empty", style: if empty.is_empty() { "display: none;" } else { "" }, "{empty}" }

            for (index, row) in rows.iter().enumerate() {
                {
                    let separator = row.is_separator();
                    let picked = !separator && row.value == chosen;
                    let class = if separator {
                        "ctxsep"
                    } else if picked {
                        "dd-item on"
                    } else {
                        "dd-item"
                    };
                    let value = row.value.clone();
                    let label = row.label.clone();
                    let glyph = row.glyph.clone().unwrap_or_default();
                    rsx! {
                        button {
                            key: "row{index}",
                            class: "{class}",
                            onclick: move |event| {
                                event.stop_propagation();
                                if separator {
                                    return;
                                }
                                if let Some(close) = on_close { close.call(()); }
                                if let Some(pick) = on_pick { pick.call(value.clone()); }
                            },
                            span {
                                class: "glyph",
                                style: if glyph.is_empty() { "display: none;" } else { "" },
                                {crate::icons::icon(&glyph, 15)}
                            }
                            span {
                                class: "tick",
                                style: if glyph.is_empty() { "" } else { "display: none;" },
                                if picked { "\u{2713}" }
                            }
                            span { "{label}" }
                        }
                    }
                }
            }
        }
    }
}
