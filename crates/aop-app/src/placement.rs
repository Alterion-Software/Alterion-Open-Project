//! Where a panel that places itself goes.
//!
//! Menus, dropdowns and the cell pickers are all written against the window:
//! somebody clicked at a point, and a panel has to appear near that point,
//! turned around when it would otherwise run off an edge. That decision was
//! made twice, once in the context menu and once in the pickers, in terms of a
//! viewport size read from a signal.
//!
//! **The viewport is not always known.** It starts at zero and is only filled
//! in when the window reports a resize, which does not happen on every
//! renderer and does not happen before the first paint on any of them. Read
//! naively, a viewport of zero does not say "I do not know", it says "the
//! window is zero by zero", and every clamp and every flip then agrees that
//! the only place a panel can go is the top left corner, so a picker opened
//! from a cell near the top of the table appeared at the bottom of the window
//! with its buttons cut off.
//!
//! So the question is asked here, once, in a form that can be tested, and an
//! unknown viewport is treated as unknown: place the panel where it was asked
//! for and do not pretend to know which edges it would cross.

use crate::state::Viewport;

/// How far from an edge a panel is allowed to sit.
const MARGIN: f64 = 6.0;

/// Past which fraction of the window a panel turns around rather than run off.
const FLIP_AT: f64 = 0.55;

/// A window whose size is actually known.
///
/// Zero is not a size. Neither is a negative one, and a window narrower than
/// the panel being placed in it cannot be reasoned about either, so all three
/// are the same answer: place it where it was asked for.
fn known(viewport: Viewport) -> Option<(f64, f64)> {
    let (w, h) = viewport;
    (w > 0.0 && h > 0.0).then_some((w, h))
}

/// The horizontal half of a placement, as the CSS that expresses it.
///
/// `width` is the panel's own width, which is what decides whether anchoring
/// it by its left edge would push its right edge off the window.
pub fn horizontal(x: f64, width: f64, viewport: Viewport) -> String {
    match known(viewport) {
        Some((w, _)) if x + width + MARGIN > w => {
            format!("right: {}px;", (w - x).max(MARGIN))
        }
        Some((w, _)) => format!("left: {}px;", x.min(w - width - MARGIN).max(MARGIN)),
        None => format!("left: {}px;", x.max(MARGIN)),
    }
}

/// The vertical half, which turns the panel upward low on the screen so a long
/// list grows towards the middle of the window rather than off the bottom.
///
/// Anchoring by the bottom edge is what lets it do that without anything
/// having to know how tall the list turned out to be.
pub fn vertical(y: f64, viewport: Viewport) -> String {
    match known(viewport) {
        Some((_, h)) if y > h * FLIP_AT => format!("bottom: {}px;", (h - y).max(MARGIN)),
        _ => format!("top: {}px;", (y + 4.0).max(MARGIN)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_of_no_size_means_unknown_rather_than_tiny() {
        // The bug this exists to prevent. Every clamp and flip below agrees
        // that a zero by zero window has room nowhere, so a panel asked for
        // near the top of the table was placed at the bottom left corner with
        // its buttons off the screen. Unknown has to mean unknown.
        assert_eq!(horizontal(400.0, 260.0, (0.0, 0.0)), "left: 400px;");
        assert_eq!(vertical(120.0, (0.0, 0.0)), "top: 124px;");
    }

    #[test]
    fn a_panel_near_the_top_hangs_downward() {
        assert_eq!(vertical(100.0, (1600.0, 900.0)), "top: 104px;");
    }

    #[test]
    fn a_panel_low_on_the_screen_hangs_upward_instead() {
        // Anchored by its bottom edge, so however tall the list turns out to
        // be it grows towards the middle of the window rather than past the
        // edge it was already near.
        assert_eq!(vertical(800.0, (1600.0, 900.0)), "bottom: 100px;");
    }

    #[test]
    fn a_panel_that_would_cross_the_right_edge_is_anchored_by_its_right() {
        assert_eq!(horizontal(1500.0, 260.0, (1600.0, 900.0)), "right: 100px;");
    }

    #[test]
    fn an_ordinary_panel_is_anchored_by_its_left() {
        assert_eq!(horizontal(400.0, 260.0, (1600.0, 900.0)), "left: 400px;");
    }

    #[test]
    fn nothing_is_ever_placed_hard_against_an_edge() {
        // Including the case that used to produce it by accident: a negative
        // coordinate, or one so close to an edge that the margin is all that
        // is left.
        assert_eq!(horizontal(-50.0, 260.0, (1600.0, 900.0)), "left: 6px;");
        assert_eq!(horizontal(-50.0, 260.0, (0.0, 0.0)), "left: 6px;");
        assert_eq!(vertical(-50.0, (1600.0, 900.0)), "top: 6px;");
    }

    #[test]
    fn a_window_narrower_than_the_panel_is_not_reasoned_about() {
        // Nothing sensible can be said here, and the old arithmetic said
        // something insensible with confidence.
        assert_eq!(horizontal(40.0, 600.0, (300.0, 900.0)), "right: 260px;");
    }
}
