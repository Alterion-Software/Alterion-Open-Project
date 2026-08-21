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
/// Two dropdowns must not be able to take each other's panel down. Only the
/// opener may clear what it opened, so a component that closes while another
/// is showing leaves that one alone.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// A claim on the layer, one per component that can open a panel.
#[derive(Clone, Copy, PartialEq)]
pub struct Claim(u64);

/// Take out a claim. Called once per component, from a hook.
pub fn claim() -> Claim {
    Claim(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The panel currently over the window, and whose it is.
#[derive(Clone, Copy)]
pub struct Layer(Signal<Option<(Claim, Element)>>);

impl Layer {
    pub fn new() -> Self {
        Layer(Signal::new(None))
    }

    /// Put a panel up, replacing whatever was there.
    ///
    /// Replacing rather than refusing, because opening a second dropdown while
    /// one is open should show the second, which is what happens in every
    /// other application.
    pub fn put(&mut self, owner: Claim, panel: Element) {
        self.0.set(Some((owner, panel)));
    }

    /// Take down the panel, if it is yours.
    pub fn clear(&mut self, owner: Claim) {
        if self.0.read().as_ref().is_some_and(|(who, _)| *who == owner) {
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
/// Mounted as a child of the root and nothing else, which is the entire point:
/// its children's parent is the window, so `absolute` and `fixed` agree about
/// where they go.
#[component]
pub fn Host() -> Element {
    let layer = use_context::<Layer>();
    let panel = layer.0.read().as_ref().map(|(_, panel)| panel.clone());
    rsx! {
        if let Some(panel) = panel {
            {panel}
        }
    }
}
