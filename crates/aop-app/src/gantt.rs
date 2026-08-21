//! The Gantt chart: timescale header, bars, progress, baselines, slack and
//! dependency arrows.
//!
//! The header and the body are two separate SVGs stacked in one column. The
//! header is sticky, so the timescale stays put while the rows scroll under it,
//! and both share the same horizontal scroll because they sit in the same pane.

use std::collections::HashMap;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime};
use dioxus::prelude::*;

use aop_core::draw::{place, snap_vertical, ChartMap, Drawing, Placement, ShapeKind};
use aop_core::grouping::GroupRow;
use aop_core::{LinkType, Project, TaskId, WorkCalendar};

use crate::viewport::{PaneScroll, RowWindow, SpanWindow};
use crate::state::{AppState, BarDragKind, Dialog, DrawDragKind, ViewKind, Zoom};
use crate::theme::Palette;

/// Row height, matched to the grid so the two panes line up.
pub const ROW_H: f64 = 22.0;
/// Height of the two-tier timescale, matched to the grid's header row.
pub const HEADER_H: f64 = 38.0;
const TIER_H: f64 = 19.0;
const BAR_H: f64 = 11.0;
const SUMMARY_H: f64 = 5.0;
/// Days of chart drawn past the end of the plan, so the last bar is not flush
/// against the edge. Nothing is padded before the start: rounding back to the
/// previous Monday would leave a whole empty week in front of the plan.
const PAD_DAYS_AFTER: i64 = 10;

/// Maps dates onto x positions.
#[derive(Clone, Copy)]
pub struct Scale {
    pub origin: NaiveDate,
    pub px_per_day: f64,
}

impl Scale {
    /// Midnight on the chart's first day, which is x zero.
    fn zero(&self) -> NaiveDateTime {
        // Midnight exists on every date, so there is nothing here to fail.
        self.origin.and_hms_opt(0, 0, 0).expect("valid midnight")
    }

    /// Wall-clock position. Used by the timescale, where a day is a day.
    pub fn x(&self, at: NaiveDateTime) -> f64 {
        (at - self.zero()).num_minutes() as f64 / 1440.0 * self.px_per_day
    }

    /// The instant an x position falls on, the inverse of `x`.
    ///
    /// What a pointer landing on the canvas means, which is how a drawing gets
    /// a date to be pinned to rather than a pixel that stops meaning anything
    /// the moment the chart is zoomed.
    pub fn at_x(&self, x: f64) -> NaiveDateTime {
        if self.px_per_day <= 0.0 {
            return self.zero();
        }
        self.zero() + Duration::minutes((x / self.px_per_day * 1440.0).round() as i64)
    }

    pub fn x_date(&self, date: NaiveDate) -> f64 {
        (date - self.origin).num_days() as f64 * self.px_per_day
    }

    /// Position for a bar edge.
    ///
    /// Bars are drawn against *working* time stretched across the whole day
    /// column, which is what Project does. A task finishing at 17:00 therefore
    /// reaches the right-hand edge of its day and the next task, starting at
    /// 08:00 the following morning, begins exactly there. Measuring the
    /// overnight gap in wall-clock hours instead would leave a false gap
    /// between two tasks that actually run back to back.
    pub fn x_work(&self, calendar: &WorkCalendar, at: NaiveDateTime) -> f64 {
        calendar.day_offset(self.origin, at) * self.px_per_day
    }
}

#[derive(PartialEq)]
struct Tick {
    from: NaiveDate,
    to: NaiveDate,
    label: String,
}

fn start_of_week(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn start_of_month(date: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap_or(date)
}

fn next_month(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(date)
}

fn start_of_quarter(date: NaiveDate) -> NaiveDate {
    let month = ((date.month() - 1) / 3) * 3 + 1;
    NaiveDate::from_ymd_opt(date.year(), month, 1).unwrap_or(date)
}

fn next_quarter(date: NaiveDate) -> NaiveDate {
    let mut cursor = start_of_quarter(date);
    for _ in 0..3 {
        cursor = next_month(cursor);
    }
    cursor
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn weeks(from: NaiveDate, to: NaiveDate, label: impl Fn(NaiveDate) -> String) -> Vec<Tick> {
    let mut ticks = Vec::new();
    let mut cursor = start_of_week(from);
    while cursor < to {
        let end = cursor + Duration::days(7);
        ticks.push(Tick { from: cursor, to: end, label: label(cursor) });
        cursor = end;
    }
    ticks
}

fn months(from: NaiveDate, to: NaiveDate, label: impl Fn(NaiveDate) -> String) -> Vec<Tick> {
    let mut ticks = Vec::new();
    let mut cursor = start_of_month(from);
    while cursor < to {
        let end = next_month(cursor);
        ticks.push(Tick { from: cursor, to: end, label: label(cursor) });
        cursor = end;
    }
    ticks
}

fn quarters(from: NaiveDate, to: NaiveDate, label: impl Fn(NaiveDate) -> String) -> Vec<Tick> {
    let mut ticks = Vec::new();
    let mut cursor = start_of_quarter(from);
    while cursor < to {
        let end = next_quarter(cursor);
        ticks.push(Tick { from: cursor, to: end, label: label(cursor) });
        cursor = end;
    }
    ticks
}

fn years(from: NaiveDate, to: NaiveDate) -> Vec<Tick> {
    let mut ticks = Vec::new();
    let mut cursor = NaiveDate::from_ymd_opt(from.year(), 1, 1).unwrap_or(from);
    while cursor < to {
        let end = NaiveDate::from_ymd_opt(cursor.year() + 1, 1, 1).unwrap_or(to);
        ticks.push(Tick { from: cursor, to: end, label: cursor.year().to_string() });
        cursor = end;
    }
    ticks
}

/// Project labels a day scale with the initial of the weekday, not the date.
const DAY_INITIALS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

fn days(from: NaiveDate, to: NaiveDate) -> Vec<Tick> {
    let mut ticks = Vec::new();
    let mut cursor = from;
    while cursor < to {
        let initial = DAY_INITIALS[cursor.weekday().num_days_from_monday() as usize];
        ticks.push(Tick {
            from: cursor,
            to: cursor + Duration::days(1),
            label: initial.to_string(),
        });
        cursor += Duration::days(1);
    }
    ticks
}

/// The upper tier of the timescale.
fn major_ticks(zoom: Zoom, from: NaiveDate, to: NaiveDate) -> Vec<Tick> {
    match zoom {
        Zoom::Days => weeks(from, to, |d| {
            format!("{} {} '{:02}", d.day(), MONTHS[d.month0() as usize], d.year() % 100)
        }),
        Zoom::Weeks => months(from, to, |d| format!("{} {}", MONTHS[d.month0() as usize], d.year())),
        Zoom::Months => quarters(from, to, |d| format!("Q{} {}", (d.month0() / 3) + 1, d.year())),
        Zoom::Quarters => years(from, to),
    }
}

/// The lower tier of the timescale.
fn minor_ticks(zoom: Zoom, from: NaiveDate, to: NaiveDate) -> Vec<Tick> {
    match zoom {
        Zoom::Days => days(from, to),
        Zoom::Weeks => weeks(from, to, |d| d.day().to_string()),
        Zoom::Months => months(from, to, |d| MONTHS[d.month0() as usize].to_string()),
        Zoom::Quarters => quarters(from, to, |d| format!("Q{}", (d.month0() / 3) + 1)),
    }
}

/// The date window the chart covers.
pub fn chart_range(project: &Project) -> (NaiveDate, NaiveDate) {
    let start = project
        .tasks
        .iter()
        .map(|t| t.scheduled.start.date())
        .min()
        .unwrap_or(project.start_date.date())
        .min(project.start_date.date());
    let finish = project
        .tasks
        .iter()
        .map(|t| t.scheduled.finish.date())
        .max()
        .unwrap_or(project.start_date.date())
        .max(start);

    // A shape can be dated outside the plan, and one drawn past the last bar
    // has to have canvas under it or it is clipped away at the edge.
    let (start, finish) = project
        .drawings
        .iter()
        .filter_map(|d| d.date_span())
        .fold((start, finish), |(from, to), (at, until)| {
            (from.min(at.date()), to.max(until.date()))
        });

    (
        start_of_week(start),
        start_of_week(finish + Duration::days(PAD_DAYS_AFTER + 7)),
    )
}

/// The span a given set of rows covers, padded the way `chart_range` pads.
///
/// A report drawing one chain out of a long plan should be as wide as that
/// chain. Using the whole plan's range leaves a short path as a small cluster
/// of bars in a mostly empty chart, which is exactly what the report is meant
/// to show clearly.
fn rows_range(project: &Project, rows: &[usize]) -> Option<(NaiveDate, NaiveDate)> {
    let start = rows
        .iter()
        .filter_map(|&index| project.tasks.get(index))
        .map(|task| task.scheduled.start.date())
        .min()?;
    let finish = rows
        .iter()
        .filter_map(|&index| project.tasks.get(index))
        .map(|task| task.scheduled.finish.date())
        .max()?
        .max(start);

    Some((
        start_of_week(start),
        start_of_week(finish + Duration::days(PAD_DAYS_AFTER)),
    ))
}

/// Left and right edge of a task's bar, snapped the way the chart snaps them.
///
/// Shared with the drawing tools: a shape dropped on a bar has to hit-test
/// against exactly the edges the chart drew, or it would anchor to a task the
/// pointer was never over.
pub fn bar_edges(
    project: &Project,
    scale: &Scale,
    round_bars: bool,
    index: usize,
) -> Option<(f64, f64)> {
    let task = project.tasks.get(index)?;
    let (mut left, mut right) = (
        scale.x_work(&project.calendar, task.scheduled.start),
        scale.x_work(&project.calendar, task.scheduled.finish),
    );
    if round_bars {
        // Snap out to whole days, so half a day of work still reads as a day
        // wide. The picture changes, the schedule does not.
        let day = scale.px_per_day.max(1.0);
        left = (left / day).floor() * day;
        right = (right / day).ceil() * day;
    }
    Some((left, right))
}

#[derive(PartialEq)]
struct BarBox {
    left: f64,
    right: f64,
    mid: f64,
}

/// Everything about the chart that does not depend on how far it is scrolled.
///
/// Scrolling used to rebuild all of this on every step: a tick per day of the
/// plan, each carrying its own label string, the bar geometry for every task,
/// and a walk over every calendar day looking for weekends. None of it changes
/// when the viewport moves, so it is worked out once and reused until the plan,
/// the zoom or the grouping actually changes.
#[derive(PartialEq)]
struct ChartLayout {
    rows: Vec<GroupRow>,
    from: NaiveDate,
    to: NaiveDate,
    width: f64,
    body_h: f64,
    major: Vec<Tick>,
    minor: Vec<Tick>,
    nonworking: Vec<f64>,
    today_x: Option<f64>,
    boxes: HashMap<TaskId, BarBox>,
    lines: HashMap<TaskId, usize>,
    band_lines: Vec<usize>,
}

/// The lines the chart draws.
///
/// `None` is the plan's own visible rows, bands and all, which is what the
/// main view wants. `Some` is exactly those tasks in exactly that order, which
/// is how a report shows one chain rather than a plan.
fn chart_rows(state: &AppState, rows: Option<&[usize]>) -> Vec<GroupRow> {
    match rows {
        Some(rows) => rows.iter().copied().map(GroupRow::Task).collect(),
        None => state.layout_rows(),
    }
}

/// Work the layout out for one set of rows.
fn build_layout(s: &AppState, rows: Vec<GroupRow>, only: Option<&[usize]>) -> ChartLayout {
    let project = &s.project;
    // A report showing a subset is scaled to that subset; the main view, which
    // shows everything, is scaled to the plan.
    let (from, to) = only
        .and_then(|rows| rows_range(project, rows))
        .unwrap_or_else(|| chart_range(project));
    let scale = Scale { origin: from, px_per_day: s.zoom.px_per_day() };

    let width = ((to - from).num_days() as f64 * scale.px_per_day).max(600.0);
    let body_h = (rows.len().max(1) as f64 * ROW_H) + 40.0;

    let major = major_ticks(s.zoom, from, to);
    let minor = minor_ticks(s.zoom, from, to);

    // Non-working days are shaded, but only when a day is wide enough to see.
    let mut nonworking: Vec<f64> = Vec::new();
    if scale.px_per_day >= 4.0 {
        let mut cursor = from;
        while cursor < to {
            if !project.calendar.is_working_day(cursor) {
                nonworking.push(scale.x_date(cursor));
            }
            cursor += Duration::days(1);
        }
    }

    let today = Local::now().naive_local().date();
    let today_x = (today >= from && today < to).then(|| scale.x_date(today));

    // Bar geometry, keyed by task so the arrows can find their endpoints.
    let mut boxes: HashMap<TaskId, BarBox> = HashMap::new();
    // Which line each task sits on, so an arrow can be skipped without first
    // working out the path it would have taken.
    let mut lines: HashMap<TaskId, usize> = HashMap::new();
    for (line, index) in rows.iter().enumerate().filter_map(task_line) {
        let task = &project.tasks[index];
        lines.insert(task.id, line);
        let Some((left, right)) = bar_edges(project, &scale, s.round_bars, index) else {
            continue;
        };
        boxes.insert(
            task.id,
            BarBox {
                left,
                right,
                mid: line as f64 * ROW_H + ROW_H / 2.0,
            },
        );
    }

    // A band draws nothing but the strip that holds its line open, so the
    // rows below it sit where the grid put them.
    let band_lines: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(line, row)| matches!(row, GroupRow::Band { .. }).then_some(line))
        .collect();

    ChartLayout {
        rows,
        from,
        to,
        width,
        body_h,
        major,
        minor,
        nonworking,
        today_x,
        boxes,
        lines,
        band_lines,
    }
}

#[component]
pub fn GanttChart(
    /// The rows to draw. None means the plan's own visible rows, which is what
    /// the main view wants. Some means draw exactly these, in this order,
    /// which is how a report shows one chain.
    rows: Option<Vec<usize>>,
    /// Whether the chart can be edited. A report is a picture.
    interactive: bool,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();

    let (grid_rows, grid_columns) = (s.grid_rows, s.grid_columns);
    // Its own signal, so pointing at a bar does not invalidate the layout.
    let mut hover = use_context::<crate::state::Hovered>().0;
    let hovered_task = hover();
    let px_per_day = s.zoom.px_per_day();
    drop(s);

    // A report's rows come from the caller and can change from one render to
    // the next, which a memo closure, built once, would never see. So the memo
    // keeps to the plan's own rows and a report bypasses it and lays out
    // inline: a handful of rows costs nothing, and the interactive path, the
    // one that has to stay fast, is left exactly as it was.
    let report = rows.is_some();

    // Worked out once per change to the plan, not once per scroll step. The
    // memo reads `state` and nothing else, so moving the viewport leaves it
    // alone and scrolling costs only the windowing below.
    let layout = use_memo(move || {
        if report {
            return None;
        }
        let s = state.read();
        let rows = chart_rows(&s, None);
        Some(build_layout(&s, rows, None))
    });

    let memo_layout = layout.read();
    let inline_layout = rows.map(|rows| {
        let s = state.read();
        let lines = chart_rows(&s, Some(&rows));
        build_layout(&s, lines, Some(&rows))
    });
    // Exactly one of the two is filled in, but the compiler cannot know that.
    let Some(layout) = inline_layout.as_ref().or((*memo_layout).as_ref()) else {
        return rsx! {};
    };
    let scale = Scale { origin: layout.from, px_per_day };
    let ChartLayout {
        rows,
        width,
        body_h,
        major,
        minor,
        nonworking,
        today_x,
        boxes,
        lines,
        band_lines,
        // The chart's date range lives in the layout for the memo's sake; the
        // scale already carries the origin, so nothing here reads it again.
        from: _,
        to: _,
    } = layout;
    let (width, body_h, today_x) = (*width, *body_h, *today_x);
    let s = state.read();
    let project = &s.project;
    // Written into SVG attributes rather than named there, because a
    // presentation attribute is not a CSS declaration and a var() in one is not
    // a paint. See `crate::theme`.
    let palette = s.theme.palette();

    let tracking = s.view == ViewKind::TrackingGantt;
    // A hidden status line is simply never given an x to draw at.
    let status_x = s
        .grid_status_date
        .then(|| s.project.status_date.map(|d| scale.x(d)))
        .flatten();
    let show_links = s.show_links;
    // The critical colouring is a view option, off by default, and a report of
    // the critical path that drew it in the ordinary bar colour would be
    // telling the reader nothing. Forced on here so the bars come out of the
    // plan's own critical style rather than a colour of the report's own.
    let show_critical = s.show_critical || !interactive;
    let bar_text = s.bar_text;
    let px_per_day = scale.px_per_day;
    let drag = s.bar_drag;
    let styles = project.bar_styles.clone();
    let (draw_tool, draw_drag) = (s.draw_tool, s.draw_drag);
    let (show_drawings, selected_drawing) = (s.show_drawings, s.selected_drawing);

    // Only the rows inside the scrolled viewport are drawn. The body keeps its
    // full height, so every bar stays where it belongs and the pane scrolls
    // exactly as it did; what changes is how much of it exists at once.
    let mut scroll = use_signal(PaneScroll::default);
    let rows_len = rows.len();
    let seen = scroll();
    // A report is never scrolled, so there is no viewport to window against:
    // it draws every row and the whole timescale, or the chain would print
    // with pieces of itself missing.
    let (window, span) = if interactive {
        (
            RowWindow::new(seen.top - HEADER_H, seen.height, rows_len),
            // The same idea sideways: a year of day columns is thousands of
            // pixels of gridline and tick label, and the pane shows a
            // screenful of it.
            seen.span(),
        )
    } else {
        (
            RowWindow { first: 0, end: rows_len, above: 0.0, below: 0.0 },
            SpanWindow { left: f64::NEG_INFINITY, right: f64::INFINITY },
        )
    };

    // Where this planner's pointer is, for the others in a live session, and
    // what it takes to work that out.
    //
    // A mouse move over a bar has the bar as its target, and a bar's own
    // coordinates say nothing about the chart. Client coordinates are the same
    // for every element, so what is missing is one offset between the two, and
    // the transparent sheet under the whole chart is the one element that sees
    // both on the same event. It is under everything, so it is crossed
    // constantly and the offset is never stale for long.
    let mut pointing = use_context::<crate::state::Pointing>().0;
    let mut chart_origin = use_signal(|| None::<(f64, f64)>);

    // The placement maths lives in the core so screen and print cannot drift.
    // The map borrows the layout's bar geometry, so nothing here is rebuilt and
    // the plan's drawings are never copied to be drawn.
    let map = ChartView { scale, boxes };
    // A shape being slid follows the pointer without the plan being touched:
    // the move lands as one undo step on mouseup rather than one per frame.
    let placed = |d: &Drawing| {
        let (dx, dy) = match draw_drag {
            Some(drag) if drag.kind == DrawDragKind::Move(d.id) => drag.delta(),
            _ => (0.0, 0.0),
        };
        place(d, &map)
            .map(|at| Placement { x: at.x + dx, y: at.y + dy, ..at })
            .filter(|at| shape_shows(*at, &span, &window))
    };

    rsx! {
        div {
            // A report sizes to the chart instead of being a pane the chart is
            // scrolled inside, so the whole chain is on the page at once.
            class: if interactive { "chart-pane" } else { "chart-pane report" },
            oncontextmenu: move |event| {
                if !interactive {
                    return;
                }
                event.prevent_default();
                let point = event.client_coordinates();
                state.write().open_chart_menu(point.x, point.y);
            },
            onmousemove: move |event| {
                if !interactive {
                    return;
                }
                if state.read().bar_drag.is_some() {
                    state.write().update_bar_drag(event.client_coordinates().x);
                }
                // Every move, wherever in the pane it landed and whatever it
                // landed on, once the sheet underneath has said where the
                // chart begins.
                if let Some((from_x, from_y)) = *chart_origin.peek() {
                    let point = event.client_coordinates();
                    let at = state
                        .peek()
                        .chart_pointer(point.x - from_x, point.y - from_y);
                    if at.is_some() && *pointing.peek() != at {
                        crate::applog::applog_verbose!("chart pointer moved to {at:?}");
                        pointing.set(at);
                    }
                }
            },
            onmousedown: move |_| {
                // A click on bare canvas lets go of any selected shape. A click
                // on a shape stops before it reaches here, and one on a bar
                // clears the selection through `select` anyway.
                if interactive && state.read().selected_drawing.is_some() {
                    state.write().selected_drawing = None;
                }
            },
            onmouseup: move |_| {
                if !interactive {
                    return;
                }
                if state.read().bar_drag.is_some() {
                    state.write().finish_bar_drag(px_per_day);
                }
                // A click quick enough to finish before the overlay is laid out
                // would otherwise leave the drag running and the overlay stuck
                // over the chart.
                if state.read().draw_drag.is_some() {
                    state.write().finish_draw_drag();
                }
            },
            onmouseleave: move |_| {
                if !interactive {
                    return;
                }
                let mut s = state.write();
                s.cancel_bar_drag();
                s.cancel_draw_drag();
            },
            onscroll: move |event| {
                if !interactive {
                    return;
                }
                let data = event.data();
                let now = PaneScroll {
                    top: data.scroll_top(),
                    height: data.client_height() as f64,
                    left: data.scroll_left(),
                    width: data.client_width() as f64,
                };
                // Redraw only when the scroll has actually brought something
                // else into view, not on every frame of the scroll itself. The
                // sideways margin is wide enough that this is most frames.
                let before = scroll();
                let same = RowWindow::new(before.top - HEADER_H, before.height, rows_len)
                    == RowWindow::new(now.top - HEADER_H, now.height, rows_len)
                    && before.span() == now.span();
                if !same {
                    scroll.set(now);
                }
            },

            // The canvas carries the content width. Putting it on the pane
            // would stop the pane from scrolling, because a scroll container
            // cannot be wider than its own content.
            div { class: "chart-canvas", style: "min-width: {width}px;",

            // ---- pinned timescale ---------------------------------------
            div { class: "chart-head",
                svg { class: "chart-svg", width: "{width}", height: "{HEADER_H}",
                    view_box: "0 0 {width} {HEADER_H}", font_family: palette.font(),
                    style: "width: {width}px; height: {HEADER_H}px; flex: none;",
                    rect { x: "0", y: "0", width: "{width}", height: "{HEADER_H}", fill: palette.paint("--grid-header") }
                    for (index, tick) in major.iter().enumerate().filter(|(_, t)| {
                        let x = scale.x_date(t.from);
                        span.overlaps(x, x + (t.to - t.from).num_days() as f64 * scale.px_per_day)
                    }) {
                        {
                            let x = scale.x_date(tick.from);
                            let w = (tick.to - tick.from).num_days() as f64 * scale.px_per_day;
                            let label = fit(&tick.label, w);
                            rsx! {
                                g { key: "mj{index}",
                                    line { x1: "{x}", y1: "0", x2: "{x}", y2: "{TIER_H}",
                                        stroke: palette.paint("--line"), stroke_width: "1" }
                                    text { class: "tl-major", x: "{x + w / 2.0}", y: "13", font_size: "10",
                                        text_anchor: "middle", fill: palette.paint("--ink"),
                                        "{label}" }
                                }
                            }
                        }
                    }
                    line { x1: "0", y1: "{TIER_H}", x2: "{width}", y2: "{TIER_H}",
                        stroke: palette.paint("--line"), stroke_width: "1" }
                    for (index, tick) in minor.iter().enumerate().filter(|(_, t)| {
                        let x = scale.x_date(t.from);
                        span.overlaps(x, x + (t.to - t.from).num_days() as f64 * scale.px_per_day)
                    }) {
                        {
                            let x = scale.x_date(tick.from);
                            let w = (tick.to - tick.from).num_days() as f64 * scale.px_per_day;
                            let weekend = !project.calendar.is_working_day(tick.from);
                            let class = if weekend { "tl-minor weekend" } else { "tl-minor" };
                            let tint = if weekend { "--ink-faint" } else { "--ink-soft" };
                            let label = fit(&tick.label, w);
                            rsx! {
                                g { key: "mn{index}",
                                    line { x1: "{x}", y1: "{TIER_H}", x2: "{x}", y2: "{HEADER_H}",
                                        stroke: palette.paint("--line"), stroke_width: "1" }
                                    text { class: "{class}", x: "{x + w / 2.0}", y: "{TIER_H + 13.0}", font_size: "10",
                                        text_anchor: "middle", fill: palette.paint(tint),
                                        "{label}" }
                                }
                            }
                        }
                    }
                    line { x1: "0", y1: "{HEADER_H - 0.5}", x2: "{width}", y2: "{HEADER_H - 0.5}",
                        stroke: palette.paint("--line"), stroke_width: "1" }
                }
            }

            // ---- chart body ---------------------------------------------
            svg { class: "chart-svg", width: "{width}", height: "{body_h}",
                view_box: "0 0 {width} {body_h}", font_family: palette.font(),
                style: "width: {width}px; height: {body_h}px; flex: none;",

                // First, so everything else is painted over it and it takes a
                // pointer only where the chart is otherwise bare. It exists to
                // answer one question, which is where the chart's own
                // coordinates start on the screen. A report has no live
                // session to tell, so it does not get one.
                if interactive {
                    rect {
                        x: "0", y: "0", width: "{width}", height: "{body_h}",
                        fill: "transparent",
                        onmousemove: move |event| {
                            let here = event.element_coordinates();
                            let screen = event.client_coordinates();
                            let found = (screen.x - here.x, screen.y - here.y);
                            // Only when it has actually moved. The pane scrolls
                            // and the window moves, but neither happens on most
                            // of the events this receives.
                            let stale = chart_origin
                                .peek()
                                .is_none_or(|(x, y): (f64, f64)| {
                                    (x - found.0).abs() >= 0.5 || (y - found.1).abs() >= 0.5
                                });
                            if stale {
                                chart_origin.set(Some(found));
                            }
                        },
                    }
                }

                for (index, x) in nonworking
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| span.overlaps(**x, **x + scale.px_per_day))
                {
                    rect {
                        key: "nw{index}",
                        x: "{x}", y: "0",
                        width: "{scale.px_per_day}", height: "{body_h}",
                        fill: palette.paint("--nonworking"),
                    }
                }

                for line in band_lines.iter().copied().filter(|line| window.holds(*line)) {
                    rect {
                        key: "bd{line}",
                        x: "0", y: "{line as f64 * ROW_H}",
                        width: "{width}", height: "{ROW_H}",
                        fill: palette.paint("--grid-header"),
                    }
                }

                for line_index in window.first..(window.end + 1).min(rows_len + 1) {
                    {
                        let y = line_index as f64 * ROW_H;
                        rsx! {
                            if grid_rows {
                                line { key: "rl{line_index}", x1: "0", y1: "{y}", x2: "{width}", y2: "{y}",
                                    stroke: palette.paint("--grid-line"), stroke_width: "1" }
                            }
                        }
                    }
                }

                for (index, tick) in minor
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| span.holds(scale.x_date(t.from)))
                {
                    {
                        let x = scale.x_date(tick.from);
                        rsx! {
                            if grid_columns {
                                line { key: "vl{index}", x1: "{x}", y1: "0", x2: "{x}", y2: "{body_h}",
                                    stroke: palette.paint("--grid-line"), stroke_width: "1" }
                            }
                        }
                    }
                }

                // ---- annotations drawn under the plan --------------------
                if show_drawings {
                    g { class: "drawings",
                        for d in project.drawings.iter().filter(|d| d.behind_bars) {
                            {
                                match placed(d) {
                                    Some(at) => {
                                        drawn_shape(d, at, selected_drawing == Some(d.id), state, interactive, palette)
                                    }
                                    None => rsx! {},
                                }
                            }
                        }
                    }
                }

                // ---- dependency arrows -----------------------------------
                for (index, link) in project.links.iter().enumerate() {
                    {
                        let shown = show_links
                            && match (lines.get(&link.predecessor), lines.get(&link.successor)) {
                                (Some(&a), Some(&b)) => window.spans(a, b),
                                _ => false,
                            };
                        match shown
                            .then(|| arrow_path(boxes, link.predecessor, link.successor, link.kind))
                            .flatten()
                        {
                            Some((d, head_x, head_y, downward)) => rsx! {
                                g { key: "lk{index}",
                                    path { d: "{d}", fill: "none", stroke: palette.paint("--link-arrow"), stroke_width: "1" }
                                    {arrow_head(head_x, head_y, downward, palette)}
                                }
                            },
                            None => rsx! {},
                        }
                    }
                }

                // ---- bars ------------------------------------------------
                for (line_index, index) in rows
                    .iter()
                    .enumerate()
                    .filter_map(task_line)
                    .filter(|(line, i)| {
                        // A bar has to be on screen both ways: on a drawn row,
                        // and within the stretch of timescale the pane shows.
                        window.holds(*line) && {
                            let t = &project.tasks[*i];
                            let left = scale.x_work(&project.calendar, t.scheduled.start);
                            let right = scale.x_work(&project.calendar, t.scheduled.finish);
                            // The trailing label reaches past the bar's own end.
                            span.overlaps(left, right + BAR_LABEL_REACH)
                        }
                    })
                {
                    {
                        let task = &project.tasks[index];
                        let summary = project.is_summary(index);
                        let y = line_index as f64 * ROW_H;
                        let centre = y + ROW_H / 2.0;
                        let left = scale.x_work(&project.calendar, task.scheduled.start);
                        let right = scale.x_work(&project.calendar, task.scheduled.finish);
                        let bar_w = (right - left).max(2.0);
                        let bar_y = y + (ROW_H - BAR_H) / 2.0;
                        let critical =
                            show_critical && aop_core::issues::shows_as_critical(project, index);
                        let fill = if !task.active {
                            palette.paint("--bar-inactive").to_string()
                        } else if critical {
                            styles.critical.clone()
                        } else {
                            styles.task.clone()
                        };
                        let progress_fill = styles.progress.clone();
                        let label = if summary { String::new() } else { project.resource_text(task) };
                        let baseline = task.baseline;
                        let slack_x = scale.x_work(&project.calendar, task.scheduled.late_finish);
                        let has_slack = task.scheduled.total_slack_minutes > 0;

                        // A live drag previews by shifting the bar as the pointer moves.
                        let (ghost_dx, ghost_w, ghost_pct) = match drag {
                            Some(d) if d.row == index => match d.kind {
                                BarDragKind::Move => (d.delta_x, bar_w, task.percent_complete),
                                BarDragKind::Resize => (0.0, (bar_w + d.delta_x).max(2.0), task.percent_complete),
                                BarDragKind::Progress => (0.0, bar_w, d.preview_percent()),
                                BarDragKind::Link => (0.0, bar_w, task.percent_complete),
                            },
                            _ => (0.0, bar_w, task.percent_complete),
                        };
                        let dragging_this = drag.is_some_and(|d| d.row == index);
                        let hovered = hovered_task == Some(index);
                        let done_w = ghost_w * ghost_pct as f64 / 100.0;
                        let tip = format!(
                            "{}\n{} \u{2192} {}\n{} \u{00b7} {}% complete{}",
                            if task.name.is_empty() { "(unnamed task)" } else { &task.name },
                            crate::state::format_date(task.scheduled.start),
                            crate::state::format_date(task.scheduled.finish),
                            aop_core::format_duration(task.scheduled.duration_minutes),
                            task.percent_complete,
                            if task.scheduled.critical { "\n\u{2022} on the critical path" } else { "" },
                        );

                        rsx! {
                            g {
                                key: "bar{index}",

                                onmouseenter: move |_| {
                                    // Pointing at something is reading, not
                                    // editing, so it works in a report too:
                                    // the row it belongs to lights up beside it.
                                    hover.set(Some(index));
                                    if interactive {
                                        state.write().set_bar_hover(index);
                                    }
                                },
                                onmouseleave: move |_| {
                                    if hover() == Some(index) {
                                        hover.set(None);
                                    }
                                },
                                onclick: move |_| {
                                    if interactive {
                                        state.write().select(index);
                                    }
                                },
                                ondoubleclick: move |_| {
                                    if interactive {
                                        state.write().dialog = Some(Dialog::TaskInformation(index));
                                    }
                                },
                                oncontextmenu: move |event| {
                                    if !interactive {
                                        return;
                                    }
                                    event.prevent_default();
                                    event.stop_propagation();
                                    let point = event.client_coordinates();
                                    state.write().open_task_menu(index, point.x, point.y);
                                },

                                title { "{tip}" }

                                if s.show_baseline {
                                    if let Some(base) = baseline {
                                        rect {
                                            x: "{scale.x_work(&project.calendar, base.start)}",
                                            y: "{y + ROW_H - 5.0}",
                                            width: "{(scale.x_work(&project.calendar, base.finish) - scale.x_work(&project.calendar, base.start)).max(2.0)}",
                                            height: "3",
                                            fill: "{styles.baseline}",
                                        }
                                    }
                                }

                                if s.show_slack && !summary && has_slack {
                                    line {
                                        x1: "{right}", y1: "{centre}", x2: "{slack_x}", y2: "{centre}",
                                        stroke: palette.paint("--slack"), stroke_width: "1", stroke_dasharray: "3 2",
                                    }
                                }

                                // A band across the whole row, so a milestone,
                                // which has no bar edge to outline, still says
                                // it is the one being pointed at.
                                if hovered {
                                    rect {
                                        x: "0", y: "{y}", width: "{width}", height: "{ROW_H}",
                                        fill: palette.paint("--selection"),
                                        style: "pointer-events: none;",
                                    }
                                }

                                if project.is_marker(index) {
                                    g {
                                        onmousedown: move |event| {
                                            if !interactive {
                                                return;
                                            }
                                            let kind = if event.modifiers().shift() {
                                                BarDragKind::Link
                                            } else {
                                                BarDragKind::Move
                                            };
                                            let x = event.client_coordinates().x;
                                            state.write().begin_bar_drag(index, kind, x, 11.0);
                                        },
                                        style: if interactive { "cursor: move;" } else { "" },
                                        {milestone_marker(
                                            left + ghost_dx,
                                            centre,
                                            if critical { &styles.critical } else { &styles.milestone },
                                        )}
                                    }
                                } else if summary {
                                    {summary_bar(left, right, y + (ROW_H - SUMMARY_H) / 2.0, &styles.summary)}
                                } else {
                                    g {
                                        rect {
                                            x: "{left + ghost_dx}", y: "{bar_y}",
                                            width: "{ghost_w}", height: "{BAR_H}",
                                            rx: "1.5", fill: "{fill}",
                                            stroke: if dragging_this || hovered {
                                                palette.paint("--accent-bright")
                                            } else {
                                                palette.paint("--bar-edge")
                                            },
                                            stroke_width: if dragging_this || hovered { "1.4" } else { "0.6" },
                                            opacity: if dragging_this { "0.85" } else { "1" },
                                        }
                                        if done_w > 0.5 {
                                            rect {
                                                x: "{left + ghost_dx}", y: "{centre - 1.5}",
                                                width: "{done_w}", height: "3",
                                                fill: "{progress_fill}",
                                            }
                                        }

                                        // Hit zones: left sets progress, right resizes,
                                        // the middle moves the whole bar. A report
                                        // has none of them at all, rather than three
                                        // invisible rectangles that do nothing.
                                        if interactive {
                                            rect {
                                                x: "{left}", y: "{bar_y}",
                                                width: "{(bar_w * 0.25).min(7.0)}", height: "{BAR_H}",
                                                fill: "transparent", style: "cursor: col-resize;",
                                                onmousedown: move |event| {
                                                    event.stop_propagation();
                                                    let x = event.client_coordinates().x;
                                                    state.write().begin_bar_drag(index, BarDragKind::Progress, x, bar_w);
                                                },
                                            }
                                            rect {
                                                x: "{right - (bar_w * 0.25).min(7.0)}", y: "{bar_y}",
                                                width: "{(bar_w * 0.25).min(7.0)}", height: "{BAR_H}",
                                                fill: "transparent", style: "cursor: ew-resize;",
                                                onmousedown: move |event| {
                                                    event.stop_propagation();
                                                    let x = event.client_coordinates().x;
                                                    state.write().begin_bar_drag(index, BarDragKind::Resize, x, bar_w);
                                                },
                                            }
                                            rect {
                                                x: "{left + (bar_w * 0.25).min(7.0)}", y: "{bar_y}",
                                                width: "{(bar_w - 2.0 * (bar_w * 0.25).min(7.0)).max(0.0)}",
                                                height: "{BAR_H}",
                                                fill: "transparent", style: "cursor: move;",
                                                onmousedown: move |event| {
                                                    event.stop_propagation();
                                                    let kind = if event.modifiers().shift() {
                                                        BarDragKind::Link
                                                    } else {
                                                        BarDragKind::Move
                                                    };
                                                    let x = event.client_coordinates().x;
                                                    state.write().begin_bar_drag(index, kind, x, bar_w);
                                                },
                                            }
                                        }
                                    }
                                }

                                if bar_text && !label.is_empty() {
                                    text { class: "bar-label", x: "{right + 6.0}", y: "{centre + 0.5}", font_size: "10",
                                        fill: palette.paint("--ink-soft"), "{label}" }
                                }
                            }
                        }
                    }
                }

                // ---- what the plan is waiting on ------------------------
                //
                // Something outside the plan gets its own mark on the row that
                // waits for it, at the date it lands. Held only in a dialog it
                // is invisible in the one place a planner is actually looking
                // when they wonder why a bar will not move.
                for (line_index, index) in rows
                    .iter()
                    .enumerate()
                    .filter_map(task_line)
                    .filter(|(line, _)| window.holds(*line))
                {
                    {
                        let waits = project.externals_of(index);
                        let y = line_index as f64 * ROW_H;
                        rsx! {
                            for entry in waits {
                                {
                                    let x = scale.x_work(&project.calendar, entry.available);
                                    let mid = y + ROW_H / 2.0;
                                    rsx! {
                                        g { key: "ex{index}-{entry.id}",
                                            title { "{entry.reference}: {entry.label}" }
                                            // A pin through the row: it is a
                                            // date nothing in the plan can move.
                                            line {
                                                x1: "{x}", y1: "{y + 1.0}",
                                                x2: "{x}", y2: "{y + ROW_H - 1.0}",
                                                stroke: palette.paint("--contextual"), stroke_width: "1.5",
                                            }
                                            polygon {
                                                points: "{x - 4.0},{mid - 5.0} {x + 4.0},{mid - 5.0} {x},{mid + 1.0}",
                                                fill: palette.paint("--contextual"),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // ---- annotations drawn over the plan ---------------------
                if show_drawings {
                    g { class: "drawings",
                        for d in project.drawings.iter().filter(|d| !d.behind_bars) {
                            {
                                match placed(d) {
                                    Some(at) => {
                                        drawn_shape(d, at, selected_drawing == Some(d.id), state, interactive, palette)
                                    }
                                    None => rsx! {},
                                }
                            }
                        }
                    }
                }

                if let Some(x) = today_x {
                    line {
                        x1: "{x}", y1: "0", x2: "{x}", y2: "{body_h}",
                        stroke: palette.paint("--today"), stroke_width: "1", stroke_dasharray: "4 3",
                    }
                }

                if tracking {
                    if let Some(x) = status_x {
                        line { x1: "{x}", y1: "0", x2: "{x}", y2: "{body_h}",
                            stroke: palette.paint("--contextual"), stroke_width: "1.4" }
                    }
                }

                {
                    // What the shape will be, while it is still being pulled out.
                    match draw_drag.and_then(|drag| match drag.kind {
                        DrawDragKind::New(kind) => drag.band().map(|band| (kind, band)),
                        DrawDragKind::Move(_) => None,
                    }) {
                        Some((kind, (x, y, dx, dy))) => {
                            let (dx, dy) = if kind == ShapeKind::Line {
                                snap_vertical(dx, dy)
                            } else {
                                (dx, dy)
                            };
                            rubber_band(kind, x, y, dx, dy, palette)
                        }
                        None => rsx! {},
                    }
                }

                // The drawing surface, and the last thing in the body so it is
                // over everything else. This SVG carries no view box and no
                // transform, so a pointer position measured against it already
                // is a chart coordinate: no bounding rectangles, no eval.
                // A report never gets one: there is nothing on it to draw on.
                if interactive && (draw_tool.is_some() || draw_drag.is_some()) {
                    rect {
                        x: "0", y: "0", width: "{width}", height: "{body_h}",
                        fill: "transparent",
                        style: if draw_tool.is_some() { "cursor: crosshair;" } else { "cursor: move;" },
                        onmousedown: move |event| {
                            let point = event.element_coordinates();
                            state.write().begin_draw(point.x, point.y);
                        },
                        onmousemove: move |event| {
                            let point = event.element_coordinates();
                            state.write().update_draw_drag(point.x, point.y);
                        },
                        onmouseup: move |_| state.write().finish_draw_drag(),
                    }
                }
            }

            // Inside the pane, in the chart's own coordinates, so the pane
            // carries other people's pointers along as it scrolls and clips
            // the ones that have gone off it. A report has no peers.
            if interactive {
                crate::cursors::ChartCursors {}
            }
            }
        }
    }
}

/// The line a task sits on, or nothing when that line belongs to a band.
///
/// Both panes lay out the same list, bands included, so the chart counts every
/// line but only draws on the ones that carry a task.
fn task_line((line, row): (usize, &GroupRow)) -> Option<(usize, usize)> {
    match row {
        &GroupRow::Task(index) => Some((line, index)),
        GroupRow::Band { .. } => None,
    }
}

/// Drop a tick label that will not fit in the space available.
fn fit(label: &str, width: f64) -> String {
    if width > label.chars().count() as f64 * 6.2 {
        label.to_string()
    } else {
        String::new()
    }
}

fn milestone_marker(x: f64, y: f64, fill: &str) -> Element {
    let size = 5.5;
    let points = format!(
        "{x},{} {},{y} {x},{} {},{y}",
        y - size,
        x + size,
        y + size,
        x - size
    );
    rsx! { polygon { points: "{points}", fill: "{fill}" } }
}

/// Project draws a summary as a flat spanning bar with a downward spike at
/// each end, so the rolled-up range reads at a glance.
fn summary_bar(left: f64, right: f64, y: f64, fill: &str) -> Element {
    let width = (right - left).max(4.0);
    let right = left + width;
    let cap = (width / 2.0).min(5.0);
    let spike = SUMMARY_H + 5.0;

    let body = format!("M{left},{y} L{right},{y} L{right},{} L{left},{} Z", y + SUMMARY_H, y + SUMMARY_H);
    let left_cap = format!(
        "M{left},{} L{},{} L{left},{} Z",
        y + SUMMARY_H,
        left + cap,
        y + SUMMARY_H,
        y + spike
    );
    let right_cap = format!(
        "M{right},{} L{},{} L{right},{} Z",
        y + SUMMARY_H,
        right - cap,
        y + SUMMARY_H,
        y + spike
    );

    rsx! {
        g {
            path { d: "{body}", fill: "{fill}" }
            path { d: "{left_cap}", fill: "{fill}" }
            path { d: "{right_cap}", fill: "{fill}" }
        }
    }
}

/// Route an elbow from one bar to another. Returns the path, the arrow head
/// position, and whether the head points down rather than right.
fn arrow_path(
    boxes: &HashMap<TaskId, BarBox>,
    predecessor: TaskId,
    successor: TaskId,
    kind: LinkType,
) -> Option<(String, f64, f64, bool)> {
    let from = boxes.get(&predecessor)?;
    let to = boxes.get(&successor)?;

    let (start_x, end_x) = match kind {
        LinkType::FS => (from.right, to.left),
        LinkType::SS => (from.left, to.left),
        LinkType::FF => (from.right, to.right),
        LinkType::SF => (from.left, to.right),
    };
    let (y1, y2) = (from.mid, to.mid);
    let stub = 7.0;
    let approach = if matches!(kind, LinkType::FF | LinkType::SF) { -stub } else { stub };

    // The two ends meet at the same instant, which is what a milestone handing
    // straight on to the next task looks like. A plain drop reads far better
    // than an elbow that jogs sideways and back for no reason.
    if (end_x - start_x).abs() < 1.0 {
        let path = format!("M{start_x},{y1} L{start_x},{}", y2 - 6.0);
        return Some((path, start_x, y2 - 6.0, true));
    }

    // The straightforward case: the successor sits clear of the predecessor.
    if (approach > 0.0 && end_x >= start_x + stub) || (approach < 0.0 && end_x <= start_x - stub) {
        let corner = end_x - approach;
        let path = format!("M{start_x},{y1} L{corner},{y1} L{corner},{y2} L{end_x},{y2}");
        return Some((path, end_x, y2, false));
    }

    // Otherwise the successor starts before the predecessor ends, so the line
    // has to double back. Run it between the two rows rather than through them.
    let between = y1 + ROW_H / 2.0;
    let out = start_x + approach;
    let path = format!(
        "M{start_x},{y1} L{out},{y1} L{out},{between} L{end_x},{between} L{end_x},{}",
        y2 - 6.0
    );
    Some((path, end_x, y2 - 6.0, true))
}

fn arrow_head(x: f64, y: f64, downward: bool, palette: Palette) -> Element {
    let points = if downward {
        format!("{},{y} {},{y} {x},{}", x - 3.0, x + 3.0, y + 5.0)
    } else {
        format!("{},{} {},{} {x},{y}", x - 5.0, y - 3.0, x - 5.0, y + 3.0)
    };
    rsx! { polygon { points: "{points}", fill: palette.paint("--link-arrow") } }
}

// ---------------------------------------------------------------- drawings

/// The chart, as the placement maths in `aop_core::draw` wants to see it.
///
/// Borrowing the layout's bar geometry rather than rebuilding it means a shape
/// pinned to a bar lands on exactly the bar that was drawn, snapping and all.
struct ChartView<'a> {
    scale: Scale,
    boxes: &'a HashMap<TaskId, BarBox>,
}

impl ChartMap for ChartView<'_> {
    fn px_per_day(&self) -> f64 {
        self.scale.px_per_day
    }

    fn row_h(&self) -> f64 {
        ROW_H
    }

    fn x_at(&self, at: NaiveDateTime) -> f64 {
        self.scale.x(at)
    }

    fn bar(&self, task: TaskId) -> Option<(f64, f64, f64)> {
        self.boxes
            .get(&task)
            .map(|b| (b.left, b.right, b.mid - ROW_H / 2.0))
    }
}

/// Whether a placed shape falls inside what the pane is showing.
///
/// The same two-way test the bars get: on a drawn row, and within the stretch
/// of timescale on screen.
fn shape_shows(at: Placement, span: &SpanWindow, window: &RowWindow) -> bool {
    let b = at.normalised();
    span.overlaps(b.x, b.x + b.w)
        && b.y + b.h >= window.first as f64 * ROW_H
        && b.y <= window.end as f64 * ROW_H
}

/// How far a selection outline stands off the shape it is around.
const SELECT_PAD: f64 = 3.0;

/// One annotation, drawn where `place` put it.
fn drawn_shape(
    d: &Drawing,
    at: Placement,
    selected: bool,
    mut state: Signal<AppState>,
    interactive: bool,
    palette: Palette,
) -> Element {
    let id = d.id;
    let outer = at.normalised();
    let (end_x, end_y) = at.end();
    // A shape's own colours ride with the plan and may name a token or give a
    // colour outright, and `aop_core` has no idea which palette is up, so they
    // are resolved here rather than there.
    let stroke = palette.literal(d.style.stroke());
    let stroke_w = d.style.width();
    let dash = d.style.line_style.dasharray().unwrap_or("none");
    let fill = palette.literal(d.style.fill());
    let ink = palette.literal(d.style.ink());
    // A locked shape lets the pointer through to the bars underneath, which is
    // the point of locking one: mark the plan up, then get back to work on it.
    // A report is locked all over, since nothing on it is meant to be touched.
    let inert = d.locked || !interactive;
    let closed_hit = if inert { "none" } else { "all" };
    let stroke_hit = if inert { "none" } else { "stroke" };

    rsx! {
        g {
            key: "dw{id}",
            onmousedown: move |event| {
                if !interactive {
                    return;
                }
                event.stop_propagation();
                state.write().begin_drawing_move(id);
            },
            // Opening the shape's own settings, the way double clicking a task
            // opens its information.
            ondoubleclick: move |event| {
                if !interactive {
                    return;
                }
                event.stop_propagation();
                let mut writer = state.write();
                writer.cancel_draw_drag();
                writer.dialog = Some(Dialog::FormatDrawing(id));
            },
            oncontextmenu: move |event| {
                if !interactive {
                    return;
                }
                event.prevent_default();
                event.stop_propagation();
                let mut writer = state.write();
                writer.cancel_draw_drag();
                writer.selected_drawing = Some(id);
                writer.dialog = Some(Dialog::FormatDrawing(id));
            },

            match d.kind {
                ShapeKind::Line | ShapeKind::Arrow => rsx! {
                    line {
                        x1: "{at.x}", y1: "{at.y}", x2: "{end_x}", y2: "{end_y}",
                        stroke: "{stroke}", stroke_width: "{stroke_w}",
                        stroke_dasharray: "{dash}", style: "pointer-events: none;",
                    }
                    // An open shape has no interior to click and a hairline is
                    // a hard target, so an invisible fat companion carries the
                    // pointer events for it.
                    line {
                        x1: "{at.x}", y1: "{at.y}", x2: "{end_x}", y2: "{end_y}",
                        stroke: "transparent", stroke_width: "8",
                        style: "pointer-events: {stroke_hit}; cursor: move;",
                    }
                    if d.kind == ShapeKind::Arrow {
                        {arrow_tip(at.x, at.y, end_x, end_y, stroke, stroke_w)}
                    }
                },
                ShapeKind::Rectangle => rsx! {
                    rect {
                        x: "{outer.x}", y: "{outer.y}",
                        width: "{outer.w}", height: "{outer.h}",
                        fill: "{fill}", stroke: "{stroke}", stroke_width: "{stroke_w}",
                        stroke_dasharray: "{dash}",
                        // An unfilled box is still a box: without this the
                        // planner has to hit the one pixel of its outline.
                        style: "pointer-events: {closed_hit}; cursor: move;",
                    }
                },
                ShapeKind::Oval => rsx! {
                    ellipse {
                        cx: "{outer.x + outer.w / 2.0}", cy: "{outer.y + outer.h / 2.0}",
                        rx: "{outer.w / 2.0}", ry: "{outer.h / 2.0}",
                        fill: "{fill}", stroke: "{stroke}", stroke_width: "{stroke_w}",
                        stroke_dasharray: "{dash}",
                        style: "pointer-events: {closed_hit}; cursor: move;",
                    }
                },
                ShapeKind::TextBox => rsx! {
                    rect {
                        x: "{outer.x}", y: "{outer.y}",
                        width: "{outer.w}", height: "{outer.h}",
                        fill: "{fill}", stroke: "{stroke}", stroke_width: "{stroke_w}",
                        stroke_dasharray: "{dash}",
                        style: "pointer-events: {closed_hit}; cursor: move;",
                    }
                    text {
                        class: "draw-text",
                        x: "{outer.x + 4.0}", y: "{outer.y + outer.h / 2.0}",
                        fill: "{ink}",
                        font_size: "{d.style.font_size()}pt",
                        font_weight: if d.style.bold { "600" } else { "400" },
                        font_style: if d.style.italic { "italic" } else { "normal" },
                        style: "pointer-events: none;",
                        "{d.text}"
                    }
                },
            }

            if selected {
                rect {
                    x: "{outer.x - SELECT_PAD}", y: "{outer.y - SELECT_PAD}",
                    width: "{outer.w + 2.0 * SELECT_PAD}", height: "{outer.h + 2.0 * SELECT_PAD}",
                    fill: "none", stroke: palette.paint("--accent-bright"), stroke_width: "1",
                    stroke_dasharray: "3 2", style: "pointer-events: none;",
                }
            }
        }
    }
}

/// The head of an arrow, at the end it was drawn towards.
fn arrow_tip(x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str, stroke_w: f64) -> Element {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let length = dx.hypot(dy);
    if length < 0.001 {
        return rsx! {};
    }
    let (ux, uy) = (dx / length, dy / length);
    // The head grows with the stroke, so a heavy line does not end in a pin.
    let size = (stroke_w * 4.0).clamp(7.0, 16.0);
    let (base_x, base_y) = (x2 - ux * size, y2 - uy * size);
    // The perpendicular, for the two back corners.
    let (px, py) = (-uy * size * 0.45, ux * size * 0.45);
    let points = format!(
        "{x2},{y2} {},{} {},{}",
        base_x + px,
        base_y + py,
        base_x - px,
        base_y - py
    );
    rsx! { polygon { points: "{points}", fill: "{stroke}", style: "pointer-events: none;" } }
}

/// The outline pulled out while a new shape is being drawn.
///
/// Deliberately not the shape itself: a dashed box says "this is where it will
/// go" without pretending the shape exists before the pointer is let go.
fn rubber_band(kind: ShapeKind, x: f64, y: f64, dx: f64, dy: f64, palette: Palette) -> Element {
    if matches!(kind, ShapeKind::Line | ShapeKind::Arrow) {
        return rsx! {
            line {
                x1: "{x}", y1: "{y}", x2: "{x + dx}", y2: "{y + dy}",
                stroke: palette.paint("--accent-bright"), stroke_width: "1.5",
                stroke_dasharray: "4 3", style: "pointer-events: none;",
            }
        };
    }
    let band = Placement { x, y, w: dx, h: dy }.normalised();
    rsx! {
        rect {
            x: "{band.x}", y: "{band.y}", width: "{band.w}", height: "{band.h}",
            fill: "none", stroke: palette.paint("--accent-bright"), stroke_width: "1",
            stroke_dasharray: "4 3", style: "pointer-events: none;",
        }
    }
}

// ---------------------------------------------------------------- timeline

/// Left and right padding inside the timeline band.
///
/// The left padding also has to clear the band's own caption, which is drawn
/// over the strip rather than in it; too small and the first date label reads
/// straight through the word "Timeline".
const BAND_LEFT: f64 = 68.0;
const BAND_RIGHT: f64 = 50.0;

/// Clearance between two bands sharing a line. Small on purpose: bands should
/// only drop to another line when they genuinely overlap.
const BAND_GAP: f64 = 2.0;

/// Half the width a milestone diamond needs.
const BAND_DIAMOND: f64 = 5.0;

/// How far a bar's trailing resource label can reach past the bar itself.
///
/// Used only to decide whether a bar is worth drawing, so it errs long: a bar
/// kept needlessly costs a few nodes, one dropped wrongly leaves a gap.
const BAR_LABEL_REACH: f64 = 260.0;

/// Rough width of one character in the band's label font.
///
/// Only used to reserve room, so an estimate is enough: too generous leaves a
/// little air after a label, too mean lets the next band sit on top of it.
const BAND_CHAR_W: f64 = 5.0;

/// Clearance between a bar or diamond and a label drawn beside it.
const BAND_LABEL_GAP: f64 = 5.0;

/// Padding a label needs to sit inside a bar rather than beside it.
const BAND_LABEL_PAD: f64 = 10.0;

/// How tall the band is allowed to grow before it stops taking bands.
const BAND_MAX_LANES: usize = 8;

/// How far each band's fill is shifted from the plan's bar colour.
///
/// Consecutive phases in a plan are usually adjacent in time as well, so a
/// single flat colour makes the strip read as one long bar. Cycling through a
/// few shades of the same colour separates them without introducing a second
/// hue that would then have to mean something.
const BAND_SHADES: [f64; 5] = [0.18, -0.06, 0.30, 0.06, -0.18];

/// Mix a colour toward white or black.
///
/// `amount` runs from -1 (black) to 1 (white). Anything that is not a plain
/// six digit hex colour is handed back untouched, so a CSS variable or a named
/// colour still works, it just does not vary.
pub fn shade(colour: &str, amount: f64) -> String {
    let hex = colour.trim().strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return colour.to_string();
    }

    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
    let mixed = |value: u8| -> u8 {
        let value = value as f64;
        let target = if amount >= 0.0 { 255.0 } else { 0.0 };
        (value + (target - value) * amount.abs()).round().clamp(0.0, 255.0) as u8
    };

    format!(
        "#{:02x}{:02x}{:02x}",
        mixed(channel(0)),
        mixed(channel(2)),
        mixed(channel(4))
    )
}

/// One entry on the timeline, with the room its label needs accounted for.
#[derive(Debug, Clone)]
pub struct Band {
    /// Index of the task in the plan.
    pub index: usize,
    /// Left and right edge of the bar itself, or of the diamond for a marker.
    pub left: f64,
    pub right: f64,
    /// What the label says, and whether it fits within the bar.
    pub label: String,
    pub inside: bool,
    /// Left and right of everything drawn for this band, label included. This
    /// is what the lanes are packed by; packing by the bar alone is what lets
    /// the next band be drawn straight over a marker's name.
    pub span: (f64, f64),
}

/// Work out what a band covers once its label is taken into account.
///
/// A marker is a diamond with its name beside it, so its label is always
/// outside. A bar carries its name within it when there is room, and beside it
/// when there is not, which is what stops a short task from losing its name
/// altogether.
pub fn measure_band(index: usize, left: f64, right: f64, name: &str, marker: bool) -> Band {
    let label = name.trim().to_string();
    let label_w = label.chars().count() as f64 * BAND_CHAR_W;

    let inside = !marker && !label.is_empty() && label_w + BAND_LABEL_PAD <= right - left;
    let span_right = if label.is_empty() || inside {
        right
    } else {
        right + BAND_LABEL_GAP + label_w
    };

    Band {
        index,
        left,
        right,
        label,
        inside,
        span: (left, span_right),
    }
}

/// Lay spans out on as few lines as possible.
///
/// Each span goes on the first line it fits, so a milestone followed straight
/// away by a task shares a line with it rather than starting a new one.
pub fn pack_lanes(spans: &[(f64, f64)]) -> (Vec<usize>, usize) {
    let mut lane_ends: Vec<f64> = Vec::new();
    let mut lanes = Vec::with_capacity(spans.len());

    for &(left, right) in spans {
        let lane = lane_ends
            .iter()
            .position(|end| left + BAND_GAP >= *end)
            .unwrap_or_else(|| {
                lane_ends.push(f64::NEG_INFINITY);
                lane_ends.len() - 1
            });
        lane_ends[lane] = right;
        lanes.push(lane);
    }

    let used = lane_ends.len().max(1);
    (lanes, used)
}

/// Map an instant onto the timeline band.
///
/// Pulled out of the component so it can be tested: two tasks that run back to
/// back must land on exactly the same x, with no gap for the night between
/// them.
pub fn timeline_x(
    calendar: &WorkCalendar,
    first_day: NaiveDate,
    total_days: f64,
    width: f64,
    at: NaiveDateTime,
) -> f64 {
    let usable = width - BAND_LEFT - BAND_RIGHT;
    BAND_LEFT + calendar.day_offset(first_day, at) / total_days.max(1.0) * usable
}

/// The Timeline band above the workspace, showing the plan at a glance.
#[component]
pub fn TimelineBand() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;
    let palette = s.theme.palette();

    // The pane reports its width once it is laid out. Until then a plausible
    // width keeps the first paint from being nonsense.
    let mut measured = use_signal(|| None::<f64>);
    let band_width = move || measured().unwrap_or(1400.0);

    if project.tasks.is_empty() {
        return rsx! {
            div { class: "timeline",
                div { class: "timeline-caption", "Timeline" }
                div { class: "empty-state", style: "height: 100%; font-size: 11px;",
                    "Add tasks to see them on the timeline"
                }
            }
        };
    }

    // The band is drawn at its real pixel size. Stretching a fixed coordinate
    // space to fit the pane would scale the glyphs sideways with it, which is
    // what makes the labels look wrong before anything else does.
    let width = band_width().max(600.0);
    let start = project.start_date;
    let finish = project.finish_date.max(start + Duration::days(1));
    // The band is measured in whole days so consecutive tasks touch, matching
    // the way the chart projects working time onto day columns.
    let first_day = start.date();
    let last_day = finish.date() + Duration::days(1);
    let total_days = (last_day - first_day).num_days().max(1) as f64;
    let x = |at: NaiveDateTime| {
        timeline_x(&project.calendar, first_day, total_days, width, at)
    };

    // The top level of the outline is what a timeline is for.
    let mut bands: Vec<Band> = (0..project.tasks.len())
        .filter(|&i| project.tasks[i].outline_level == 0)
        .map(|index| {
            let task = &project.tasks[index];
            let left = x(task.scheduled.start);
            if project.is_marker(index) {
                measure_band(index, left - BAND_DIAMOND, left + BAND_DIAMOND, &task.name, true)
            } else {
                let right = x(task.scheduled.finish).max(left + 3.0);
                measure_band(index, left, right, &task.name, false)
            }
        })
        .collect();

    // First fit only packs tightly if the bands arrive in the order they are
    // drawn in, and the outline order is not always the order they start in.
    bands.sort_by(|a, b| a.span.0.total_cmp(&b.span.0));

    let (lane_of, lanes) = pack_lanes(&bands.iter().map(|b| b.span).collect::<Vec<_>>());

    // A band that would push the strip past its height is left off rather than
    // drawn somewhere it does not belong, and the caption says how many.
    let lanes = lanes.min(BAND_MAX_LANES);
    let placed: Vec<(Band, usize)> = bands
        .into_iter()
        .zip(lane_of)
        .filter(|(_, lane)| *lane < lanes)
        .collect();
    let dropped = project
        .tasks
        .iter()
        .filter(|t| t.outline_level == 0)
        .count()
        .saturating_sub(placed.len());

    let lane_h = 16.0;
    let top = 24.0;
    let height = top + lanes as f64 * lane_h + 4.0;

    let caption = if dropped > 0 {
        format!("Timeline \u{2022} {dropped} more")
    } else {
        "Timeline".to_string()
    };

    rsx! {
        div {
            class: "timeline",
            style: "height: {height + 8.0}px;",
            onresize: move |event| {
                if let Ok(size) = event.get_content_box_size() {
                    let seen = size.width.round();
                    // Redraw only on a real change in width, not on every
                    // frame of a window drag.
                    if (band_width() - seen).abs() >= 1.0 {
                        measured.set(Some(seen));
                    }
                }
            },
            // And measured on arrival as well, because `onresize` is an event
            // not every renderer sends. Where it is not sent the band never
            // learns its width and draws itself at the fallback, which is a
            // band far narrower than the room it has.
            //
            // Asked again until there is something to measure: mounting
            // happens before layout, so the first answer is honestly zero.
            onmounted: move |event| async move {
                // Sampled until the answer stops changing, not until it is
                // merely non-zero. Layout settles in stages here, so the first
                // width that is not zero is often an intermediate one, and a
                // band that believes it is 630 pixels wide inside an 830 pixel
                // panel simply stops two hundred pixels short.
                let mut steady = 0u32;
                let mut last = 0.0f64;
                for _ in 0..24u32 {
                    match event.get_client_rect().await {
                        Ok(rect) if rect.width() > 1.0 => {
                            let seen = rect.width().round();
                            if (seen - last).abs() < 1.0 {
                                steady += 1;
                            } else {
                                steady = 0;
                                last = seen;
                            }
                            if (band_width() - seen).abs() >= 1.0 {
                                measured.set(Some(seen));
                            }
                            if steady >= 2 {
                                break;
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                }
            },
            div { class: "timeline-caption", "{caption}" }
            // No viewBox. It gives the contents their own coordinate space, and
            // a space that does not match the element is scaled to fit it, so a
            // band drawn at a stale width is not merely narrow, it is magnified
            // until a date reads like a headline. Without one the contents are
            // drawn at their own size whatever the element turns out to be,
            // and being narrow is the worst that can happen.
            svg { width: "{width}", height: "{height}", font_family: palette.font(),
                style: "width: {width}px; height: {height}px; flex: none;",
                line { x1: "{BAND_LEFT}", y1: "16", x2: "{width - BAND_RIGHT}", y2: "16",
                    stroke: palette.paint("--line"), stroke_width: "1" }
                text { x: "{BAND_LEFT}", y: "11", class: "tl-minor", font_size: "10", fill: palette.paint("--ink-soft"),
                    "{crate::state::format_date(start)}" }
                text { x: "{width - BAND_RIGHT}", y: "11", class: "tl-minor", text_anchor: "end", font_size: "10", fill: palette.paint("--ink-soft"),
                    "{crate::state::format_date(finish)}" }

                for (slot, (band, lane)) in placed.into_iter().enumerate() {
                    {
                        let w = (band.right - band.left).max(3.0);
                        let y = top + lane as f64 * lane_h;
                        let critical =
                            s.show_critical && aop_core::issues::shows_as_critical(project, band.index);
                        // Each band takes its own shade of the plan's colour so
                        // one phase reads as separate from the next.
                        let base = if critical {
                            &project.bar_styles.critical
                        } else {
                            &project.bar_styles.task
                        };
                        let fill = shade(base, BAND_SHADES[slot % BAND_SHADES.len()]);
                        // A darker edge of the same colour gives the block an
                        // outline without a border colour of its own.
                        let edge = shade(&fill, -0.35);
                        // A label that sits beside its band starts clear of it;
                        // one that sits within starts just inside the edge.
                        let label_x = if band.inside {
                            band.left + 5.0
                        } else {
                            band.right + BAND_LABEL_GAP
                        };
                        let label_class = if band.inside { "band-label in" } else { "band-label" };
                        // Pale shades need dark ink, dark shades need pale.
                        let label_ink = if band.inside { shade(&fill, -0.75) } else { String::new() };
                        // Size and colour on the element, not from the class.
                        // A stylesheet class does not reach inside an inline
                        // SVG here, so `.band-label { font-size: 10px }` never
                        // applied and these labels were drawn at whatever they
                        // inherited, which is why the band came out looking
                        // like a headline whenever it was rebuilt.
                        let label_style = if band.inside {
                            format!("font-size: 10px; fill: {label_ink};")
                        } else {
                            format!("font-size: 10px; fill: {};", palette.paint("--ink-soft"))
                        };
                        rsx! {
                            g { key: "tb{band.index}",
                                if project.is_marker(band.index) {
                                    {milestone_marker(band.left + BAND_DIAMOND, y + 6.0, &project.bar_styles.milestone)}
                                } else {
                                    rect {
                                        x: "{band.left}", y: "{y}",
                                        width: "{w}", height: "13", rx: "3",
                                        fill: "{fill}",
                                        stroke: "{edge}", stroke_width: "1",
                                    }
                                }
                                if !band.label.is_empty() {
                                    text {
                                        x: "{label_x}", y: "{y + 9.5}",
                                        class: "{label_class}", style: "{label_style}",
                                        "{band.label}"
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use aop_core::{WorkCalendar, MINUTES_PER_DAY};
    use chrono::NaiveDate;

    /// The property the report mode is not allowed to cost. The main view
    /// passes no rows of its own, and when it does the chart has to draw
    /// exactly the lines the plan lays out, bands included: one line more or
    /// fewer here slides every bar off its row in the grid beside it.
    #[test]
    fn a_bar_is_placed_at_the_same_origin_the_layout_used() {
        // The render body used to build its own scale from the whole plan
        // while the layout positioned bars from the range it actually chose.
        // For a report those are different dates, so every bar landed
        // thousands of pixels off the canvas and only the dependency arrows,
        // which read the layout's own geometry, appeared.
        let mut state = AppState::new();
        for name in ["A", "B", "C", "D"] {
            state.append_task(name);
        }
        state.reschedule();

        // A report over the last two rows, whose range starts later than the
        // plan's does.
        let picked: Vec<usize> = vec![2, 3];
        let lines = chart_rows(&state, Some(&picked));
        let layout = build_layout(&state, lines, Some(&picked));

        let scale = Scale { origin: layout.from, px_per_day: 26.0 };
        for &index in &picked {
            let task = &state.project.tasks[index];
            let drawn = scale.x_work(&state.project.calendar, task.scheduled.start);
            let geometry = layout
                .boxes
                .get(&task.id)
                .map(|b| b.left)
                .expect("every reported row has geometry");
            assert!(
                (drawn - geometry).abs() < 0.001,
                "{} draws at {drawn} but its geometry says {geometry}",
                task.name
            );
            assert!(drawn >= 0.0, "{} would be off the left of the canvas", task.name);
        }
    }

    #[test]
    fn a_report_actually_has_bars_to_draw() {
        // The chart came out with dependency arrows and no bars at all, which
        // means the geometry existed but the bar loop's own filter rejected
        // every row. Arrows read the layout; bars are filtered again against
        // the window and the span, so those are what this pins.
        let mut state = AppState::new();
        for name in ["A", "B", "C"] {
            state.append_task(name);
        }
        state.reschedule();

        let picked: Vec<usize> = vec![0, 1, 2];
        let lines = chart_rows(&state, Some(&picked));
        let layout = build_layout(&state, lines, Some(&picked));

        assert_eq!(layout.rows.len(), 3, "every asked for row has to be a line");
        assert_eq!(layout.boxes.len(), 3, "every line needs bar geometry");

        let rows_len = layout.rows.len();
        let window = RowWindow { first: 0, end: rows_len, above: 0.0, below: 0.0 };
        let span = SpanWindow { left: f64::NEG_INFINITY, right: f64::INFINITY };

        let drawn = layout
            .rows
            .iter()
            .enumerate()
            .filter_map(task_line)
            .filter(|(line, index)| {
                window.holds(*line) && {
                    // The geometry the layout already worked out, which is
                    // the same span the bar loop tests against.
                    let id = state.project.tasks[*index].id;
                    match layout.boxes.get(&id) {
                        Some(box_) => span.overlaps(box_.left, box_.right),
                        None => false,
                    }
                }
            })
            .count();

        assert_eq!(drawn, 3, "all three bars have to survive the filter");
    }

    #[test]
    fn a_report_is_scaled_to_the_rows_it_draws_not_the_whole_plan() {
        // A short chain inside a long plan would otherwise be a small cluster
        // of bars in a mostly empty chart, which defeats the point of the
        // report drawing it at all.
        let mut state = AppState::new();
        for name in ["Early", "Middle", "Late"] {
            state.append_task(name);
        }
        // Push the last task far out so the plan is much longer than any pair.
        state.select(2);
        state.commit_cell(2, crate::state::Column::Duration, "200 days");
        state.reschedule();

        let whole = chart_range(&state.project);
        let pair = rows_range(&state.project, &[0, 1]).expect("two real rows");

        assert!(
            pair.1 < whole.1,
            "a range over the first two rows must be shorter than the plan's: {pair:?} against {whole:?}"
        );
        assert!(pair.0 <= pair.1, "a range cannot run backwards: {pair:?}");
        assert_eq!(
            rows_range(&state.project, &[]),
            None,
            "no rows means no range of their own, so the plan's is used"
        );
    }

    #[test]
    fn with_no_rows_of_its_own_the_chart_draws_the_plans_layout() {
        let mut state = AppState::new();
        state.project.tasks.clear();
        state.project.links.clear();
        for name in ["Phase", "Child A", "Child B", "Standalone"] {
            state.project.push_task(name, MINUTES_PER_DAY);
        }
        state.project.tasks[1].outline_level = 1;
        state.project.tasks[2].outline_level = 1;
        state.reschedule();

        assert_eq!(chart_rows(&state, None), state.layout_rows());

        // And once grouping puts bands in the list, still exactly that list.
        state.set_group_by("milestone");
        let grouped = state.layout_rows();
        assert!(
            grouped.iter().any(|row| matches!(row, GroupRow::Band { .. })),
            "the grouped plan should have bands to lose"
        );
        assert_eq!(chart_rows(&state, None), grouped);
    }

    /// A report draws the chain it was handed, in the order it was handed it,
    /// and nothing else: no bands, no rows the plan would have put between.
    #[test]
    fn the_rows_a_report_asks_for_are_the_rows_it_gets() {
        let state = AppState::new();
        assert_eq!(
            chart_rows(&state, Some(&[3, 0, 1])),
            vec![GroupRow::Task(3), GroupRow::Task(0), GroupRow::Task(1)]
        );
        assert!(chart_rows(&state, Some(&[])).is_empty());
    }

    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    #[test]
    fn bands_share_a_line_until_they_actually_overlap() {
        // Two spans that merely touch belong on the same line.
        let (lanes, used) = pack_lanes(&[(0.0, 100.0), (100.0, 160.0)]);
        assert_eq!(lanes, vec![0, 0]);
        assert_eq!(used, 1);

        // Spans that genuinely overlap cannot share one.
        let (lanes, used) = pack_lanes(&[(0.0, 100.0), (60.0, 160.0)]);
        assert_eq!(lanes, vec![0, 1]);
        assert_eq!(used, 2);
    }

    #[test]
    fn a_milestone_does_not_push_the_next_band_onto_another_line() {
        // A marker at 100 reserves only its diamond, so a task starting at the
        // same instant still fits beside it.
        let diamond = (100.0 - BAND_DIAMOND, 100.0 + BAND_DIAMOND);
        let following = (100.0 + BAND_DIAMOND, 200.0);
        let (lanes, used) = pack_lanes(&[diamond, following]);

        assert_eq!(lanes, vec![0, 0], "a milestone should not start a new line");
        assert_eq!(used, 1);
    }

    #[test]
    fn a_third_band_reuses_the_first_line_once_it_is_free() {
        let (lanes, used) = pack_lanes(&[(0.0, 100.0), (50.0, 150.0), (160.0, 200.0)]);
        assert_eq!(lanes, vec![0, 1, 0], "the third fits back on line one");
        assert_eq!(used, 2);
    }

    #[test]
    fn a_markers_name_is_part_of_what_it_occupies() {
        // The name is drawn beside the diamond, so packing by the diamond
        // alone is what lets the next band be drawn straight over the name.
        let marker = measure_band(0, 95.0, 105.0, "Compliance checklist created", true);
        assert!(!marker.inside, "a marker never carries its name within it");
        assert!(
            marker.span.1 > 105.0 + 100.0,
            "the name has to be counted, got {:?}",
            marker.span
        );
    }

    #[test]
    fn a_band_packed_after_a_marker_clears_its_name() {
        let marker = measure_band(0, 95.0, 105.0, "Compliance checklist created", true);
        let bar = measure_band(1, 120.0, 400.0, "Phase 7: Dashboards", false);
        let (lanes, used) = pack_lanes(&[marker.span, bar.span]);
        assert_eq!(used, 2, "the bar starts inside the marker's name");
        assert_ne!(lanes[0], lanes[1]);
    }

    #[test]
    fn a_name_that_fits_its_bar_is_drawn_within_it() {
        let bar = measure_band(0, 0.0, 400.0, "Phase 9: Mobile App", false);
        assert!(bar.inside);
        assert_eq!(bar.span, (0.0, 400.0), "an inside name needs no extra room");
    }

    #[test]
    fn a_name_too_long_for_its_bar_moves_beside_it() {
        // Rather than elide the name down to nothing on a narrow bar.
        let bar = measure_band(0, 0.0, 20.0, "Phase 2: Core Infrastructure", false);
        assert!(!bar.inside);
        assert!(bar.span.1 > 20.0);
        assert_eq!(bar.label, "Phase 2: Core Infrastructure", "nothing is cut");
    }

    #[test]
    fn bands_that_do_not_overlap_share_a_line() {
        let a = measure_band(0, 0.0, 100.0, "A", false);
        let b = measure_band(1, 200.0, 300.0, "B", false);
        let (lanes, used) = pack_lanes(&[a.span, b.span]);
        assert_eq!(used, 1);
        assert_eq!(lanes[0], lanes[1]);
    }

    #[test]
    fn the_timeline_leaves_no_gap_between_back_to_back_tasks() {
        let calendar = WorkCalendar::standard();
        let first_day = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap();
        let total_days = 19.0;
        let width = 1400.0;

        // One task ends Wednesday 17:00, the next starts Thursday 08:00.
        let ends = timeline_x(&calendar, first_day, total_days, width, at(2026, 9, 2, 17));
        let starts = timeline_x(&calendar, first_day, total_days, width, at(2026, 9, 3, 8));

        assert_eq!(ends, starts, "the overnight break must not show as a gap");
    }

    #[test]
    fn the_timeline_still_shows_a_weekend() {
        let calendar = WorkCalendar::standard();
        let first_day = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap();
        let total_days = 19.0;
        let width = 1400.0;
        let day = (width - BAND_LEFT - BAND_RIGHT) / total_days;

        let friday = timeline_x(&calendar, first_day, total_days, width, at(2026, 8, 21, 17));
        let monday = timeline_x(&calendar, first_day, total_days, width, at(2026, 8, 24, 8));

        assert!(
            (monday - friday - 2.0 * day).abs() < 1e-6,
            "a weekend should still be two day columns wide"
        );
    }

    /// The exact shape that looked wrong on screen: a collapsed summary whose
    /// last child is the predecessor of a following top level task.
    #[test]
    fn a_summary_bar_ends_where_the_next_task_begins() {
        use aop_core::{Link, Project, MINUTES_PER_DAY};

        let start = NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut project = Project::blank(start);
        project.push_task("tets", MINUTES_PER_DAY);
        project.push_task("child a", MINUTES_PER_DAY * 5);
        project.push_task("child b", MINUTES_PER_DAY * 7);
        project.push_task("test4", MINUTES_PER_DAY * 2);
        project.tasks[1].outline_level = 1;
        project.tasks[2].outline_level = 1;

        let (a, b, next) = (
            project.tasks[1].id,
            project.tasks[2].id,
            project.tasks[3].id,
        );
        project.add_link(Link::finish_to_start(a, b));
        project.add_link(Link::finish_to_start(b, next));
        aop_core::schedule(&mut project).unwrap();

        // The summary really does roll up to its last child.
        assert_eq!(
            project.tasks[0].scheduled.finish,
            project.tasks[2].scheduled.finish
        );

        let (origin, _) = chart_range(&project);
        let scale = Scale { origin, px_per_day: 26.0 };

        let summary_right = scale.x_work(&project.calendar, project.tasks[0].scheduled.finish);
        let child_right = scale.x_work(&project.calendar, project.tasks[2].scheduled.finish);
        let next_left = scale.x_work(&project.calendar, project.tasks[3].scheduled.start);

        assert_eq!(
            summary_right, child_right,
            "the summary bar must reach the end of its last child"
        );
        assert_eq!(
            summary_right, next_left,
            "the next task must start exactly where the summary ends"
        );
    }

    #[test]
    fn a_bar_covers_every_day_column_it_spans() {
        let calendar = WorkCalendar::standard();
        // The chart origin is the Monday before the plan starts.
        let origin = NaiveDate::from_ymd_opt(2026, 8, 10).unwrap();
        let scale = Scale { origin, px_per_day: 26.0 };

        // A task running Mon 17 Aug to Wed 2 Sep.
        let left = scale.x_work(&calendar, at(2026, 8, 17, 8));
        let right = scale.x_work(&calendar, at(2026, 9, 2, 17));

        // Its left edge is the start of the 17 Aug column.
        assert_eq!(left, scale.x_date(NaiveDate::from_ymd_opt(2026, 8, 17).unwrap()));
        // Its right edge is the start of the 3 Sep column, so the bar covers
        // the whole of Wednesday 2 Sep and stops there.
        assert_eq!(right, scale.x_date(NaiveDate::from_ymd_opt(2026, 9, 3).unwrap()));
        // Which is one full column past the start of the finish day.
        let wednesday = scale.x_date(NaiveDate::from_ymd_opt(2026, 9, 2).unwrap());
        assert_eq!(right - wednesday, scale.px_per_day);
    }

    #[test]
    fn a_bar_edge_lands_on_the_day_column_it_belongs_to() {
        let calendar = WorkCalendar::standard();
        let scale = Scale {
            origin: NaiveDate::from_ymd_opt(2026, 8, 17).unwrap(),
            px_per_day: 26.0,
        };
        // Monday 08:00 is the very start of the first column.
        assert_eq!(scale.x_work(&calendar, at(2026, 8, 17, 8)), 0.0);
        // Monday 17:00 is one whole column along.
        assert_eq!(scale.x_work(&calendar, at(2026, 8, 17, 17)), 26.0);
        // Tuesday 08:00 is the same place, so the bars meet.
        assert_eq!(scale.x_work(&calendar, at(2026, 8, 18, 8)), 26.0);
    }
}
