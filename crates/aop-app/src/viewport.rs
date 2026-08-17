//! Which rows a scrolled pane is actually showing.
//!
//! A plan of a few hundred tasks draws tens of thousands of nodes if every row
//! is laid out, and all of them are re-diffed whenever anything in the plan
//! changes, whether or not it is on screen. Since rows are a fixed height, the
//! ones inside the scrolled viewport can be worked out arithmetically, and the
//! rest replaced by a single spacer of the height they would have taken. The
//! pane scrolls exactly as before; it just has far less in it.

use crate::gantt::ROW_H;

/// How many rows to draw beyond each edge of the viewport.
///
/// Scrolling arrives a frame after the pointer moves, so drawing only what is
/// strictly visible leaves a blank band at the leading edge. A margin of a few
/// rows covers that without costing much.
const OVERSCAN: usize = 6;

/// The height to assume before a pane has told us its own.
///
/// A pane reports its height with its first scroll or mount event, which is
/// after the first paint. Guessing high means that first paint draws some rows
/// nobody can see, which is wasteful but invisible; guessing low would leave a
/// gap at the bottom of the pane until something moved.
pub const ASSUMED_HEIGHT: f64 = 1400.0;

/// The width to assume before a pane has told us its own. See `ASSUMED_HEIGHT`.
pub const ASSUMED_WIDTH: f64 = 2400.0;

/// The rows a pane is showing, as a half open range, with the blank space
/// standing in for the rows above and below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowWindow {
    /// Index into the visible-row list, not into the plan.
    pub first: usize,
    /// One past the last row to draw.
    pub end: usize,
    /// Height of the spacer standing in for the rows before `first`.
    pub above: f64,
    /// Height of the spacer standing in for the rows from `end` on.
    pub below: f64,
}

impl RowWindow {
    /// Work out which rows fall inside a pane scrolled to `scroll_top`.
    pub fn new(scroll_top: f64, viewport: f64, total: usize) -> Self {
        let first_visible = (scroll_top.max(0.0) / ROW_H).floor() as usize;
        let first = first_visible.saturating_sub(OVERSCAN).min(total);

        let spanned = (viewport.max(0.0) / ROW_H).ceil() as usize;
        let end = first_visible
            .saturating_add(spanned)
            .saturating_add(OVERSCAN)
            .min(total);
        // Guard the empty case so `end` is never behind `first`.
        let end = end.max(first);

        RowWindow {
            first,
            end,
            above: first as f64 * ROW_H,
            below: (total - end) as f64 * ROW_H,
        }
    }

    /// Whether a row is drawn.
    pub fn holds(&self, line: usize) -> bool {
        line >= self.first && line < self.end
    }

    /// Whether anything between two rows is drawn.
    ///
    /// A link between two rows that are both off screen still crosses the pane
    /// when one is above it and the other below, so a link is kept whenever its
    /// span touches the window rather than only when an end does.
    pub fn spans(&self, a: usize, b: usize) -> bool {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        low < self.end && high >= self.first
    }
}

/// The stretch of the timescale a pane is showing, in pixels.
///
/// The chart is as wide as the plan is long, so a year at day zoom is a few
/// thousand pixels of gridlines, tick labels and weekend shading that exist
/// whatever the pane happens to be scrolled to. Only the stretch on screen is
/// worth drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanWindow {
    pub left: f64,
    pub right: f64,
}

impl SpanWindow {
    /// How far past each edge to keep drawing, so a sideways scroll does not
    /// arrive at bare canvas.
    const MARGIN: f64 = 400.0;

    pub fn new(scroll_left: f64, viewport: f64) -> Self {
        SpanWindow {
            left: scroll_left - Self::MARGIN,
            right: scroll_left + viewport.max(0.0) + Self::MARGIN,
        }
    }

    /// Whether anything drawn at this x is worth drawing.
    pub fn holds(&self, x: f64) -> bool {
        x >= self.left && x <= self.right
    }

    /// Whether something spanning `left..right` shows at all.
    pub fn overlaps(&self, left: f64, right: f64) -> bool {
        right >= self.left && left <= self.right
    }
}

/// A pane's scroll position and size, as its own events report them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneScroll {
    pub top: f64,
    pub height: f64,
    pub left: f64,
    pub width: f64,
}

impl Default for PaneScroll {
    fn default() -> Self {
        PaneScroll {
            top: 0.0,
            height: ASSUMED_HEIGHT,
            left: 0.0,
            width: ASSUMED_WIDTH,
        }
    }
}

impl PaneScroll {
    pub fn window(&self, total: usize) -> RowWindow {
        RowWindow::new(self.top, self.height, total)
    }

    pub fn span(&self) -> SpanWindow {
        SpanWindow::new(self.left, self.width)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unscrolled_pane_starts_at_the_first_row() {
        let w = RowWindow::new(0.0, 220.0, 200);
        assert_eq!(w.first, 0);
        assert_eq!(w.above, 0.0);
        // Ten rows fit, plus the margin below.
        assert_eq!(w.end, 10 + OVERSCAN);
    }

    #[test]
    fn the_spacers_stand_in_for_exactly_the_rows_left_out() {
        let total = 200;
        let w = RowWindow::new(1000.0, 300.0, total);
        assert_eq!(w.above, w.first as f64 * ROW_H);
        assert_eq!(w.below, (total - w.end) as f64 * ROW_H);
        // The pane must scroll as though every row were still there.
        let drawn = (w.end - w.first) as f64 * ROW_H;
        assert_eq!(w.above + drawn + w.below, total as f64 * ROW_H);
    }

    #[test]
    fn the_window_never_runs_past_the_last_row() {
        let w = RowWindow::new(100_000.0, 400.0, 20);
        assert!(w.first <= 20 && w.end <= 20);
        assert!(w.first <= w.end);
        assert_eq!(w.below, 0.0);
    }

    #[test]
    fn an_empty_plan_yields_an_empty_window() {
        let w = RowWindow::new(0.0, 400.0, 0);
        assert_eq!(w.first, 0);
        assert_eq!(w.end, 0);
        assert_eq!(w.above, 0.0);
        assert_eq!(w.below, 0.0);
    }

    #[test]
    fn a_row_scrolled_just_out_of_sight_is_still_drawn() {
        // The margin is what stops a blank band appearing at the leading edge
        // while the pane catches up with the pointer.
        let w = RowWindow::new(ROW_H * 30.0, 220.0, 200);
        assert!(w.holds(30 - OVERSCAN), "the margin above must be drawn");
        assert!(!w.holds(30 - OVERSCAN - 1));
    }

    #[test]
    fn the_timescale_window_covers_the_pane_and_a_margin() {
        let span = SpanWindow::new(1000.0, 800.0);
        assert!(span.holds(1000.0) && span.holds(1800.0), "the pane itself");
        assert!(span.holds(700.0), "and the margin before it");
        assert!(!span.holds(100.0), "but not the far side of the plan");
    }

    #[test]
    fn a_bar_reaching_into_the_pane_is_kept_though_it_starts_before_it() {
        // A task running from January into June is drawn when the pane shows
        // March, even though neither of its ends is on screen.
        let span = SpanWindow::new(2000.0, 800.0);
        assert!(span.overlaps(0.0, 5000.0));
        assert!(!span.overlaps(0.0, 100.0));
    }

    #[test]
    fn a_link_crossing_the_window_is_kept_though_neither_end_shows() {
        let w = RowWindow::new(ROW_H * 50.0, 220.0, 200);
        assert!(!w.holds(2));
        assert!(!w.holds(150));
        assert!(w.spans(2, 150), "its line still crosses the pane");
        assert!(!w.spans(0, 1), "this one is wholly above");
    }
}
