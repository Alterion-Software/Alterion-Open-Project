//! Annotation shapes drawn over the Gantt chart.
//!
//! A drawing stores intent, never pixels. Where a shape sits is a date and a
//! row, or an offset from a bar; how big it is, is a span of calendar time or a
//! size on screen. That is the whole reason a marked-up plan survives being
//! zoomed, rescheduled or reopened after a task was inserted above: pixels
//! recorded at day zoom mean nothing at quarter zoom, and mean something wrong
//! rather than nothing once the chart's origin shifts by a week.
//!
//! Placing a shape is done here rather than in the chart, so the screen and any
//! future printed copy cannot drift apart. Coordinates are top-down throughout,
//! the way a screen counts them; a PDF flips at draw time, not here.

use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::model::TaskId;

/// Identifies a drawing within the plan.
pub type DrawingId = u32;

/// Minutes in a day of wall clock.
///
/// The chart's x axis is wall clock, not working time, so a scaled extent is
/// measured against this rather than against `MINUTES_PER_DAY`.
const MINUTES_PER_CALENDAR_DAY: f64 = 1440.0;

/// The shapes a plan can be marked up with.
///
/// Project also offers a polygon and an arc. Neither is here: both need a
/// multi-click editor of their own, and neither turns up on a real plan often
/// enough to earn one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Line,
    Arrow,
    Rectangle,
    Oval,
    TextBox,
}

impl ShapeKind {
    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Oval => "Oval",
            ShapeKind::TextBox => "Text Box",
        }
    }

    /// The name a menu carries the choice under.
    pub fn key(self) -> &'static str {
        match self {
            ShapeKind::Line => "Line",
            ShapeKind::Arrow => "Arrow",
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Oval => "Oval",
            ShapeKind::TextBox => "TextBox",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "Line" => ShapeKind::Line,
            "Arrow" => ShapeKind::Arrow,
            "Rectangle" => ShapeKind::Rectangle,
            "Oval" => ShapeKind::Oval,
            "TextBox" => ShapeKind::TextBox,
            _ => return None,
        })
    }
}

/// Which end of a bar a shape hangs off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarPoint {
    Start,
    Middle,
    Finish,
}

/// What a shape is pinned to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Anchor {
    /// A date and a row of the chart. The row is fractional so a shape can sit
    /// between two rows rather than being forced onto one.
    Timescale { at: NaiveDateTime, row: f64 },
    /// An offset from a point on a task's bar, so the shape moves when the bar
    /// does. This is what keeps a callout beside the task it is about after the
    /// plan is rescheduled.
    Task {
        task: TaskId,
        point: BarPoint,
        dx: f64,
        dy: f64,
    },
}

/// How big a shape is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    /// A span of calendar time and a count of rows, so the shape covers the
    /// same stretch of the plan however far the chart is zoomed in.
    Scaled { minutes: i64, rows: f64 },
    /// A size in pixels, so the shape stays legible at every zoom. What a text
    /// box wants: stretching one across a quarter view leaves a caption in a
    /// box the width of the screen.
    Fixed { w: f64, h: f64 },
}

/// How a stroke is broken up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl LineStyle {
    /// The SVG dash pattern, or nothing for a solid stroke.
    pub fn dasharray(self) -> Option<&'static str> {
        match self {
            LineStyle::Solid => None,
            LineStyle::Dashed => Some("6 3"),
            LineStyle::Dotted => Some("1 3"),
        }
    }
}

/// How a shape is painted.
///
/// Every colour here treats an empty string as "not set, use the theme's",
/// never as black, which is what lets a marked-up plan still read in both
/// palettes. A palette token such as `var(--ink)` is as welcome as a literal
/// colour and travels between palettes better.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawStyle {
    #[serde(default)]
    pub line_colour: String,
    #[serde(default)]
    pub line_width: f64,
    #[serde(default)]
    pub line_style: LineStyle,
    /// Empty means the shape is not filled, which is the useful default: a
    /// filled box over a bar hides the thing it is drawing attention to.
    #[serde(default)]
    pub fill_colour: String,
    #[serde(default)]
    pub text_colour: String,
    #[serde(default)]
    pub font_size_pt: f64,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self {
            line_colour: String::new(),
            line_width: 1.5,
            line_style: LineStyle::Solid,
            fill_colour: String::new(),
            text_colour: String::new(),
            font_size_pt: 9.0,
            bold: false,
            italic: false,
        }
    }
}

impl DrawStyle {
    /// The stroke colour to paint with, falling back to the theme's ink.
    pub fn stroke(&self) -> &str {
        if self.line_colour.trim().is_empty() {
            "var(--ink)"
        } else {
            &self.line_colour
        }
    }

    /// The fill to paint with. Unset means unfilled, not black.
    pub fn fill(&self) -> &str {
        if self.fill_colour.trim().is_empty() {
            "none"
        } else {
            &self.fill_colour
        }
    }

    /// The ink for a caption, falling back to the theme's.
    pub fn ink(&self) -> &str {
        if self.text_colour.trim().is_empty() {
            "var(--ink)"
        } else {
            &self.text_colour
        }
    }

    /// Stroke width, guarded so a hand-edited file cannot make a shape vanish.
    pub fn width(&self) -> f64 {
        if self.line_width > 0.0 {
            self.line_width
        } else {
            1.5
        }
    }

    /// Font size in points, guarded the same way.
    pub fn font_size(&self) -> f64 {
        if self.font_size_pt > 0.0 {
            self.font_size_pt
        } else {
            9.0
        }
    }
}

/// One annotation on the chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: DrawingId,
    pub kind: ShapeKind,
    pub anchor: Anchor,
    pub extent: Extent,
    #[serde(default)]
    pub style: DrawStyle,
    /// The caption, for a text box. Empty for every other shape.
    #[serde(default)]
    pub text: String,
    /// Stacking order. The plan keeps its drawings in ascending `z`, so the
    /// chart draws them in the order it finds them and never sorts per frame.
    #[serde(default)]
    pub z: i32,
    /// Whether the shape goes under the bars rather than over them. A band
    /// highlighting a stretch of the plan wants to be under; a callout does not.
    #[serde(default)]
    pub behind_bars: bool,
    /// Locked shapes are still drawn, but the pointer goes straight through
    /// them, so an annotation cannot be nudged while working on the bars.
    #[serde(default)]
    pub locked: bool,
}

impl Drawing {
    pub fn new(id: DrawingId, kind: ShapeKind, anchor: Anchor, extent: Extent) -> Self {
        Self {
            id,
            kind,
            anchor,
            extent,
            style: DrawStyle::default(),
            text: String::new(),
            z: 0,
            behind_bars: false,
            locked: false,
        }
    }

    /// The task this shape rides on, when it rides on one.
    pub fn anchored_task(&self) -> Option<TaskId> {
        match self.anchor {
            Anchor::Task { task, .. } => Some(task),
            Anchor::Timescale { .. } => None,
        }
    }

    /// The stretch of the timescale the shape occupies, when it is pinned to a
    /// date rather than to a bar.
    ///
    /// The chart widens its date range to take this in, so a shape dated past
    /// the end of the plan still has canvas under it rather than being clipped
    /// away at the edge.
    pub fn date_span(&self) -> Option<(NaiveDateTime, NaiveDateTime)> {
        let Anchor::Timescale { at, .. } = self.anchor else {
            return None;
        };
        let minutes = match self.extent {
            Extent::Scaled { minutes, .. } => minutes,
            // A fixed extent is a size on screen, so it says nothing about how
            // much calendar it covers. Its anchor date is all there is.
            Extent::Fixed { .. } => 0,
        };
        let other = at + Duration::minutes(minutes);
        Some((at.min(other), at.max(other)))
    }

    /// Slide the shape by a distance on screen.
    ///
    /// A date-anchored shape moves to a new date, a bar-anchored one keeps its
    /// bar and changes its offset, so in both cases the move is stored in the
    /// same terms the shape was already stored in.
    pub fn nudge(&mut self, dx: f64, dy: f64, px_per_day: f64, row_h: f64) {
        match &mut self.anchor {
            Anchor::Timescale { at, row } => {
                if px_per_day > 0.0 {
                    let minutes = (dx / px_per_day * MINUTES_PER_CALENDAR_DAY).round() as i64;
                    *at += Duration::minutes(minutes);
                }
                if row_h > 0.0 {
                    *row += dy / row_h;
                }
            }
            Anchor::Task { dx: ax, dy: ay, .. } => {
                *ax += dx;
                *ay += dy;
            }
        }
    }
}

/// Where a shape lands on a chart, in that chart's own pixels.
///
/// `w` and `h` are signed, because a line drawn right to left is not the same
/// line drawn left to right: an arrow's head is at the far end. Closed shapes
/// take `normalised` before they are drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Placement {
    /// The same box with its corner at the top left and no negative sides.
    pub fn normalised(&self) -> Placement {
        Placement {
            x: if self.w < 0.0 { self.x + self.w } else { self.x },
            y: if self.h < 0.0 { self.y + self.h } else { self.y },
            w: self.w.abs(),
            h: self.h.abs(),
        }
    }

    /// Where the shape ends, which for a line is the end it points at.
    pub fn end(&self) -> (f64, f64) {
        (self.x + self.w, self.y + self.h)
    }
}

/// What a renderer has to be able to answer for a shape to be placed on it.
///
/// Kept to four questions so the screen chart and anything that prints one can
/// both satisfy it, and so the placement maths exists in exactly one place.
pub trait ChartMap {
    fn px_per_day(&self) -> f64;
    fn row_h(&self) -> f64;
    /// Where an instant falls on the x axis.
    fn x_at(&self, at: NaiveDateTime) -> f64;
    /// A bar's left edge, right edge and the top of its row. `None` when the
    /// task is not drawn on this map, which happens when it has been filtered
    /// out or rolled up into a collapsed summary.
    fn bar(&self, task: TaskId) -> Option<(f64, f64, f64)>;
}

/// Work out where a shape goes.
///
/// `None` means the shape has nothing to hang off on this map, which is not an
/// error: a callout on a task hidden by a filter simply is not drawn.
pub fn place(d: &Drawing, map: &dyn ChartMap) -> Option<Placement> {
    let (x, y) = match d.anchor {
        Anchor::Timescale { at, row } => (map.x_at(at), row * map.row_h()),
        Anchor::Task {
            task,
            point,
            dx,
            dy,
        } => {
            let (left, right, top) = map.bar(task)?;
            let x = match point {
                BarPoint::Start => left,
                BarPoint::Middle => (left + right) / 2.0,
                BarPoint::Finish => right,
            };
            (x + dx, top + dy)
        }
    };

    let (w, h) = match d.extent {
        Extent::Scaled { minutes, rows } => (
            minutes as f64 / MINUTES_PER_CALENDAR_DAY * map.px_per_day(),
            rows * map.row_h(),
        ),
        Extent::Fixed { w, h } => (w, h),
    };

    Some(Placement { x, y, w, h })
}

/// How far off vertical a line may lean and still be snapped upright.
///
/// Roughly eight degrees. A vertical rule through the rows at a date, marking a
/// gate or a release, is the one use of a drawn line that turns up on real
/// plans again and again, and one drawn by hand is always a degree or two out.
/// A rule that is nearly vertical reads as a mistake, so the last few degrees
/// are given away.
const VERTICAL_SNAP: f64 = 0.14;

/// Shortest line worth snapping, in pixels. Below this the lean is noise.
const SNAP_FLOOR: f64 = 4.0;

/// Straighten a line that was drawn near enough to vertical.
pub fn snap_vertical(dx: f64, dy: f64) -> (f64, f64) {
    if dy.abs() >= SNAP_FLOOR && dx.abs() <= dy.abs() * VERTICAL_SNAP {
        (0.0, dy)
    } else {
        (dx, dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    /// A map standing in for the chart, with one bar on row two.
    struct FakeChart {
        px_per_day: f64,
        origin: NaiveDateTime,
        bar: Option<(f64, f64, f64)>,
    }

    const ROW_H: f64 = 22.0;

    impl FakeChart {
        fn at(px_per_day: f64) -> Self {
            FakeChart {
                px_per_day,
                origin: at(2026, 1, 5, 0),
                // Row two, so the top is 44 whatever the zoom.
                bar: Some((3.0 * px_per_day, 8.0 * px_per_day, 2.0 * ROW_H)),
            }
        }
    }

    impl ChartMap for FakeChart {
        fn px_per_day(&self) -> f64 {
            self.px_per_day
        }
        fn row_h(&self) -> f64 {
            ROW_H
        }
        fn x_at(&self, at: NaiveDateTime) -> f64 {
            (at - self.origin).num_minutes() as f64 / 1440.0 * self.px_per_day
        }
        fn bar(&self, _task: TaskId) -> Option<(f64, f64, f64)> {
            self.bar
        }
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|date| date.and_hms_opt(h, 0, 0))
            .expect("a real date")
    }

    fn dated(extent: Extent) -> Drawing {
        Drawing::new(
            1,
            ShapeKind::Rectangle,
            Anchor::Timescale {
                at: at(2026, 1, 7, 0),
                row: 3.0,
            },
            extent,
        )
    }

    #[test]
    fn a_dated_shape_covers_the_same_days_at_every_zoom() {
        // Three days wide, starting two days into the chart.
        let d = dated(Extent::Scaled {
            minutes: 3 * 1440,
            rows: 2.0,
        });

        for px_per_day in [4.0, 26.0, 90.0] {
            let map = FakeChart::at(px_per_day);
            let at = place(&d, &map).expect("a dated shape always places");
            assert_eq!(at.x, 2.0 * px_per_day, "two days in at {px_per_day}");
            assert_eq!(at.w, 3.0 * px_per_day, "three days wide at {px_per_day}");
            // Rows are the same height whatever the timescale does.
            assert_eq!(at.y, 3.0 * ROW_H);
            assert_eq!(at.h, 2.0 * ROW_H);
        }
    }

    #[test]
    fn a_fixed_extent_is_the_same_size_at_every_zoom() {
        // Which is what a caption wants: legible, not stretched across a quarter.
        let d = dated(Extent::Fixed { w: 120.0, h: 30.0 });

        for px_per_day in [4.0, 26.0, 90.0] {
            let map = FakeChart::at(px_per_day);
            let at = place(&d, &map).expect("a dated shape always places");
            assert_eq!((at.w, at.h), (120.0, 30.0), "at {px_per_day}");
            // It still moves with the timescale, it just does not grow with it.
            assert_eq!(at.x, 2.0 * px_per_day);
        }
    }

    #[test]
    fn a_bar_anchored_shape_rides_its_bar_at_every_zoom() {
        let d = Drawing::new(
            1,
            ShapeKind::Arrow,
            Anchor::Task {
                task: 7,
                point: BarPoint::Finish,
                dx: 6.0,
                dy: -4.0,
            },
            Extent::Fixed { w: 40.0, h: 0.0 },
        );

        for px_per_day in [4.0, 26.0, 90.0] {
            let map = FakeChart::at(px_per_day);
            let at = place(&d, &map).expect("the bar is on this map");
            // The bar finishes eight days in, and the offset is in pixels, so
            // the shape sits the same distance clear of the bar at every zoom.
            assert_eq!(at.x, 8.0 * px_per_day + 6.0, "at {px_per_day}");
            assert_eq!(at.y, 2.0 * ROW_H - 4.0);
        }
    }

    #[test]
    fn the_three_points_of_a_bar_are_where_they_should_be() {
        let map = FakeChart::at(20.0);
        let of = |point| {
            let d = Drawing::new(
                1,
                ShapeKind::Line,
                Anchor::Task {
                    task: 7,
                    point,
                    dx: 0.0,
                    dy: 0.0,
                },
                Extent::Fixed { w: 0.0, h: 0.0 },
            );
            place(&d, &map).map(|at| at.x)
        };

        assert_eq!(of(BarPoint::Start), Some(60.0));
        assert_eq!(of(BarPoint::Middle), Some(110.0));
        assert_eq!(of(BarPoint::Finish), Some(160.0));
    }

    #[test]
    fn a_shape_on_a_bar_that_is_not_drawn_places_nowhere() {
        // A filter can hide the task a callout belongs to. Not drawing the
        // callout is right; drawing it at the origin would not be.
        let mut map = FakeChart::at(26.0);
        map.bar = None;
        let d = Drawing::new(
            1,
            ShapeKind::Oval,
            Anchor::Task {
                task: 7,
                point: BarPoint::Start,
                dx: 0.0,
                dy: 0.0,
            },
            Extent::Fixed { w: 10.0, h: 10.0 },
        );
        assert_eq!(place(&d, &map), None);
    }

    #[test]
    fn a_backwards_drag_normalises_to_a_box_but_keeps_its_direction() {
        let at = Placement {
            x: 100.0,
            y: 50.0,
            w: -40.0,
            h: -10.0,
        };
        assert_eq!(at.end(), (60.0, 40.0), "an arrow still points where it was aimed");
        let box_ = at.normalised();
        assert_eq!((box_.x, box_.y, box_.w, box_.h), (60.0, 40.0, 40.0, 10.0));
    }

    #[test]
    fn a_line_drawn_nearly_upright_is_drawn_upright() {
        assert_eq!(snap_vertical(3.0, 90.0), (0.0, 90.0));
        assert_eq!(snap_vertical(-3.0, -90.0), (0.0, -90.0));
    }

    #[test]
    fn a_line_meant_to_lean_is_left_leaning() {
        assert_eq!(snap_vertical(60.0, 90.0), (60.0, 90.0));
        // A horizontal line is nowhere near vertical and must survive intact.
        assert_eq!(snap_vertical(120.0, 0.0), (120.0, 0.0));
    }

    #[test]
    fn a_dated_shape_moves_to_a_new_date_rather_than_a_new_pixel() {
        let mut d = dated(Extent::Scaled {
            minutes: 1440,
            rows: 1.0,
        });
        // Two days to the right at day zoom.
        d.nudge(52.0, 22.0, 26.0, 22.0);

        let Anchor::Timescale { at: moved, row } = d.anchor else {
            unreachable!("the anchor kind does not change on a move");
        };
        assert_eq!(moved, at(2026, 1, 9, 0));
        assert_eq!(row, 4.0);
    }

    #[test]
    fn a_bar_anchored_shape_moves_by_offset_and_keeps_its_bar() {
        let mut d = Drawing::new(
            1,
            ShapeKind::Rectangle,
            Anchor::Task {
                task: 7,
                point: BarPoint::Start,
                dx: 2.0,
                dy: 3.0,
            },
            Extent::Fixed { w: 10.0, h: 10.0 },
        );
        d.nudge(20.0, -5.0, 26.0, 22.0);

        assert_eq!(
            d.anchor,
            Anchor::Task {
                task: 7,
                point: BarPoint::Start,
                dx: 22.0,
                dy: -2.0,
            }
        );
    }

    #[test]
    fn a_dated_shape_reports_the_days_it_needs_canvas_for() {
        let d = dated(Extent::Scaled {
            minutes: 3 * 1440,
            rows: 1.0,
        });
        assert_eq!(d.date_span(), Some((at(2026, 1, 7, 0), at(2026, 1, 10, 0))));

        // Drawn right to left, the span still reads forwards.
        let back = dated(Extent::Scaled {
            minutes: -2 * 1440,
            rows: 1.0,
        });
        assert_eq!(back.date_span(), Some((at(2026, 1, 5, 0), at(2026, 1, 7, 0))));
    }

    #[test]
    fn an_unset_style_asks_the_theme_rather_than_painting_black() {
        let style = DrawStyle::default();
        assert_eq!(style.stroke(), "var(--ink)");
        assert_eq!(style.fill(), "none", "an unfilled box shows the bars under it");
        assert_eq!(style.ink(), "var(--ink)");

        // And a hand-edited file cannot make a shape invisible.
        let broken = DrawStyle {
            line_width: 0.0,
            font_size_pt: -3.0,
            ..DrawStyle::default()
        };
        assert!(broken.width() > 0.0 && broken.font_size() > 0.0);
    }
}
