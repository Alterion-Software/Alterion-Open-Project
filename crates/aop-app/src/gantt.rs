//! The Gantt chart: timescale header, bars, progress, baselines, slack and
//! dependency arrows.
//!
//! The timescale and the bars are two boxes of ordinary elements, drawn into
//! two rows of the split, so the timescale stays put while the rows scroll
//! under it. Both are slid sideways by one number, so a date is at the same
//! pixel in each. Neither is an inline drawing: this renderer serialises one,
//! hands it to a parser and keeps the answer as a picture, which leaves its
//! children with no layout boxes, no reachable styling and nothing a pointer
//! can land on.


use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime};
use dioxus::prelude::*;

use aop_core::draw::{place, snap_vertical, ChartMap, Drawing, LineStyle, Placement, ShapeKind};
use aop_core::grouping::GroupRow;
use aop_core::{Link, LinkType, Project, TaskId, WorkCalendar};

use crate::viewport::{ChartScroll, Part, Reach, RowWindow, SpanWindow};
use crate::state::{AppState, BarDragKind, Dialog, DrawDragKind, ViewKind, Zoom};
use crate::theme::Palette;

/// Row height, matched to the grid so the two panes line up.
pub const ROW_H: f64 = 22.0;
/// Height of the two-tier timescale, matched to the grid's header row.
pub const HEADER_H: f64 = 37.0;
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

#[derive(Clone, PartialEq)]
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
    boxes: BarBoxes,
    lines: TaskLines,
    band_lines: Vec<usize>,
    links: LinkIndex,
}

/// Where each task's bar was drawn, indexed by the task's own id.
///
/// A dense table, not a hash map. Ids come off a counter and are never reused
/// within a plan, so an id already is an index and hashing it buys nothing.
/// On a plan of a hundred and fifty thousand tasks, filling two hash maps was
/// three quarters of the time the whole layout took; filling two vectors is a
/// pass over the rows and no hashing at all.
///
/// The cost is one slot per id ever issued rather than one per task, which is
/// four bytes a slot for the lines and a little more for the boxes. A plan
/// that has had rows deleted carries the gaps. That is the right trade at
/// this size and it is worth knowing it is being made.
#[derive(Default, PartialEq)]
struct BarBoxes {
    slots: Vec<Option<BarBox>>,
}

impl BarBoxes {
    fn with_room_for(bound: usize) -> Self {
        BarBoxes { slots: vec![None; bound] }
    }

    fn set(&mut self, id: TaskId, at: BarBox) {
        if let Some(slot) = self.slots.get_mut(id as usize) {
            *slot = Some(at);
        }
    }

    fn get(&self, id: TaskId) -> Option<&BarBox> {
        self.slots.get(id as usize).and_then(Option::as_ref)
    }

    /// How many tasks actually have geometry, for the tests that count them.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }
}

/// Which line each task sits on, indexed the same way.
///
/// `NOT_DRAWN` rather than an `Option`, so the table is a flat run of `u32`
/// and a plan's worth of it stays in cache while the link index is built.
#[derive(Default, PartialEq)]
struct TaskLines {
    slots: Vec<u32>,
}

impl TaskLines {
    const NOT_DRAWN: u32 = u32::MAX;

    fn with_room_for(bound: usize) -> Self {
        TaskLines { slots: vec![Self::NOT_DRAWN; bound] }
    }

    fn set(&mut self, id: TaskId, line: usize) {
        if let Some(slot) = self.slots.get_mut(id as usize) {
            *slot = line as u32;
        }
    }

    fn get(&self, id: TaskId) -> Option<usize> {
        match self.slots.get(id as usize) {
            Some(&line) if line != Self::NOT_DRAWN => Some(line as usize),
            _ => None,
        }
    }
}

/// One past the highest task id the plan has issued.
///
/// The tables are sized from this rather than from the task count, because an
/// id is only an index into them while every id is inside them.
fn id_bound(project: &Project) -> usize {
    project.tasks.iter().map(|task| task.id).max().unwrap_or(0) as usize + 1
}

/// How far apart two rows can be before a link between them stops being filed
/// under the higher of the two and is simply looked at every frame instead.
///
/// The index files a link under its top row, so a query has to look back as
/// far as the longest filed link reaches. That is cheap while links are local,
/// which nearly all of them are: a predecessor sits a few rows above its
/// successor. One link from the first row of the plan to the last would
/// otherwise make every query walk the whole thing, so links that long are
/// kept in a list of their own and looked at in full. There are never many.
const LONG_LINK: usize = 512;

/// Which links cross which rows.
///
/// A link is drawn whenever the stretch of rows between its two ends touches
/// the window, so what has to be answered once per frame is an interval
/// overlap against a table that, at the size this build is aimed at, holds
/// hundreds of thousands of entries. Walking it was costing a pass over the
/// whole plan per frame, and a node in the tree per link besides, because an
/// arrow that is not drawn still had to be decided against.
///
/// Held the way a sparse matrix is. `starts` has one entry per row plus a
/// sentinel, and `starts[line]..starts[line + 1]` is that row's stretch of
/// `filed`; the rows are contiguous, so the whole window is one slice rather
/// than a walk over its rows. Built by counting sort, which is one pass to
/// count and one to place.
#[derive(Default, PartialEq)]
struct LinkIndex {
    starts: Vec<u32>,
    /// `(link, bottom row)`, grouped by top row. The bottom row is carried
    /// here so the query needs no lookup back into the plan to reject a link
    /// that ends above the window.
    filed: Vec<(u32, u32)>,
    /// `(link, top row, bottom row)` for the links that reach too far to file.
    long: Vec<(u32, u32, u32)>,
    /// The longest reach among the filed links, so a plan whose links are all
    /// short does not look back the full `LONG_LINK` rows for nothing.
    reach: usize,
}

impl LinkIndex {
    /// File every link that has both ends on a drawn row.
    ///
    /// A link to a task that is filtered out, collapsed into a summary or
    /// simply not in this report is dropped here rather than being rejected
    /// once per frame for the life of the view.
    fn build(links: &[Link], lines: &TaskLines, rows: usize) -> Self {
        let mut short: Vec<(u32, u32, u32)> = Vec::new();
        let mut long: Vec<(u32, u32, u32)> = Vec::new();
        let mut reach = 0usize;

        for (index, link) in links.iter().enumerate() {
            let (Some(a), Some(b)) =
                (lines.get(link.predecessor), lines.get(link.successor))
            else {
                continue;
            };
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            if high - low > LONG_LINK {
                long.push((index as u32, low as u32, high as u32));
            } else {
                reach = reach.max(high - low);
                short.push((index as u32, low as u32, high as u32));
            }
        }

        // Count into `starts[line + 1]` so the prefix sum below leaves each
        // row's own offset in `starts[line]`, with the sentinel at the end.
        let mut starts = vec![0u32; rows + 1];
        for &(_, low, _) in &short {
            starts[low as usize + 1] += 1;
        }
        for line in 1..starts.len() {
            starts[line] += starts[line - 1];
        }

        let mut cursor = starts.clone();
        let mut filed = vec![(0u32, 0u32); short.len()];
        for &(index, low, high) in &short {
            let slot = &mut cursor[low as usize];
            filed[*slot as usize] = (index, high);
            *slot += 1;
        }

        LinkIndex { starts, filed, long, reach }
    }

    /// The links whose span touches `window`, in the plan's own order.
    ///
    /// The same answer `RowWindow::spans` gives, reached without asking it
    /// about every link in the plan: the filed links come out of one slice,
    /// and only the handful that reach past `LONG_LINK` rows are looked at
    /// one by one.
    fn crossing(&self, window: RowWindow) -> impl Iterator<Item = usize> + '_ {
        let rows = self.starts.len().saturating_sub(1);
        let from = window.first.saturating_sub(self.reach);
        let to = window.end.min(rows);
        let slice = if from < to {
            &self.filed[self.starts[from] as usize..self.starts[to] as usize]
        } else {
            &self.filed[0..0]
        };
        let first = window.first as u32;

        slice
            .iter()
            // The slice already holds only links whose top row is above the
            // bottom of the window; this is the other half of the overlap.
            .filter(move |(_, high)| *high >= first)
            .map(|(index, _)| *index as usize)
            .chain(
                // The few that reach too far to file are asked the question
                // in its original form, so the rule the index is reproducing
                // is still written down in exactly one place.
                self.long
                    .iter()
                    .filter(move |(_, low, high)| {
                        window.spans(*low as usize, *high as usize)
                    })
                    .map(|(index, _, _)| *index as usize),
            )
    }
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
    let bound = id_bound(project);
    let mut boxes = BarBoxes::with_room_for(bound);
    // Which line each task sits on, so an arrow can be skipped without first
    // working out the path it would have taken.
    let mut lines = TaskLines::with_room_for(bound);
    for (line, index) in rows.iter().enumerate().filter_map(task_line) {
        let task = &project.tasks[index];
        lines.set(task.id, line);
        let Some((left, right)) = bar_edges(project, &scale, s.round_bars, index) else {
            continue;
        };
        boxes.set(
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

    // Which arrow crosses which row, worked out here with everything else
    // that does not change when the viewport moves.
    let links = LinkIndex::build(&project.links, &lines, rows.len());

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
        links,
    }
}

/// The stretch of a run of ticks that falls inside `span`.
///
/// The ticks run left to right and do not overlap, so both edges of the window
/// are a binary search rather than a walk. A year of day columns is a few
/// thousand ticks and a pane shows a screenful, so this is the difference
/// between reading the whole timescale every frame and reading the part of it
/// that is on screen.
fn tick_window(ticks: &[Tick], scale: &Scale, span: &SpanWindow) -> std::ops::Range<usize> {
    let left = |tick: &Tick| scale.x_date(tick.from);
    let right =
        |tick: &Tick| left(tick) + (tick.to - tick.from).num_days() as f64 * scale.px_per_day;
    let from = ticks.partition_point(|tick| right(tick) < span.left);
    let to = ticks.partition_point(|tick| left(tick) <= span.right);
    from..to.max(from)
}

#[component]
pub fn GanttChart(
    /// The rows to draw. None means the plan's own visible rows, which is what
    /// the main view wants. Some means draw exactly these, in this order,
    /// which is how a report shows one chain.
    rows: Option<Vec<usize>>,
    /// Whether the chart can be edited. A report is a picture.
    interactive: bool,
    /// The timescale, or the bars. Drawn into different rows of the split so
    /// that the timescale is above the scrolling box rather than inside it.
    part: Part,
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
        band_lines,
        links: link_index,
        // The chart's date range lives in the layout for the memo's sake; the
        // scale already carries the origin, so nothing here reads it again.
        from: _,
        to: _,
        // Only the index reads these now: which line a task sits on is a
        // question the arrows used to ask once per link per frame, and the
        // index answers it once per plan instead.
        lines: _,
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
    // Written by the split, which owns the scrolling for both panes so their
    // rows cannot drift apart, and only when the answer to "what is on screen"
    // has changed.
    // How far there is to scroll sideways. The split cannot work this out: it
    // is the length of the plan at the current zoom, which only the chart
    // knows. The strip along the bottom of the pane is sized from it.
    let mut reach = use_context::<Signal<Reach>>();
    use_effect(use_reactive!(|width| {
        let now = reach();
        if interactive && now.chart != width {
            reach.set(Reach { chart: width, ..now });
        }
    }));

    let scroll = use_context::<ChartScroll>().0;
    let rows_len = rows.len();
    let seen = scroll();
    // How much of the plan the pane shows is now only a question of how much
    // of it is worth building.
    //
    // The chart used to be drawn into a picture exactly the size of the pane,
    // with the offset carried in that picture's own coordinate system rather
    // than by sliding it sideways, because a picture is an image, an image is
    // cut off at its own edges, and an image that has been slid takes its
    // edges with it. A tree of boxes is cut off by the pane the way anything
    // else is, so the offset goes back where the timescale's already is: one
    // margin on the box that holds both, applied to head and body alike, which
    // is the only way the two can be guaranteed to agree on where a date sits.

    // A report is never scrolled, so there is no viewport to window against:
    // it draws every row and the whole timescale, or the chain would print
    // with pieces of itself missing.
    let (window, span) = if interactive {
        (
            // No head to subtract any more. The timescale is not in the box
            // that scrolls, so the first pixel of the scroll is the first
            // pixel of the first row.
            RowWindow::new(seen.top, seen.height, rows_len),
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

    // Everything the two windows cut down to, worked out once here rather than
    // as a test carried through a walk over the whole plan. Each of these
    // collections is held in the order it is drawn in, so what is on screen is
    // a range of it.
    let minor_window = tick_window(minor, &scale, &span);
    let nonworking_window = {
        let day = scale.px_per_day;
        let from = nonworking.partition_point(|x| x + day < span.left);
        let to = nonworking.partition_point(|x| *x <= span.right);
        from..to.max(from)
    };
    let band_window = {
        let from = band_lines.partition_point(|line| *line < window.first);
        let to = band_lines.partition_point(|line| *line < window.end);
        from..to.max(from)
    };
    // How far down the drawn rows reach, for the furniture that runs the
    // height of the chart rather than sitting on a row of its own.
    //
    // A gridline used to be written `top: 0; bottom: 0`, which makes it as
    // tall as the canvas, and the canvas is as tall as the plan: at a hundred
    // and fifty thousand rows that is a box one pixel wide and three and a
    // third million pixels tall. This renderer only skips a box whose bounds
    // fall wholly outside the window, and one that tall never does, so every
    // one of them was drawn to its full height and turned into strips a
    // scanline at a time. Two hundred of those was half a second a frame, and
    // it scaled with the plan rather than with what was on screen, which is
    // the whole thing the windowing exists to stop.
    let band_top = window.first as f64 * ROW_H;
    let band_h = (window.end.saturating_sub(window.first)) as f64 * ROW_H;

    // Collected rather than borrowed: the arrows are read inside the body's
    // `rsx!`, which outlives the borrow of the index, and there are only ever
    // a windowful of them.
    let arrows: Vec<usize> = if show_links {
        link_index.crossing(window).collect()
    } else {
        Vec::new()
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

    let head = rsx! {
            // ---- pinned timescale ---------------------------------------
            //
            // Boxes, not an SVG. A tick is a rectangle with a label in it and
            // a rule down its left edge, which is what a div already is, and
            // drawing it as one takes the whole timescale out of the class of
            // fault that has cost this build most of a day: a stylesheet does
            // not reach inside an inline SVG here, nothing inside one is
            // clipped by an ancestor's `overflow`, and a viewBox against a
            // stale width magnifies everything it holds. None of those exist
            // for an ordinary element.
            div { class: "chart-head", style: "width: {width}px; height: {HEADER_H}px;",
                div { class: "tl-tier tl-tier-major", style: "height: {TIER_H}px;",
                    for (index, tick) in tick_window(major, &scale, &span).map(|i| (i, &major[i])) {
                        {
                            let x = scale.x_date(tick.from);
                            let w = (tick.to - tick.from).num_days() as f64 * scale.px_per_day;
                            let label = fit(&tick.label, w);
                            rsx! {
                                div {
                                    key: "mj{index}",
                                    class: "tl-cell",
                                    style: "left: {x}px; width: {w}px; height: {TIER_H}px;",
                                    "{label}"
                                }
                            }
                        }
                    }
                }
                div { class: "tl-tier tl-tier-minor",
                    style: "top: {TIER_H}px; height: {HEADER_H - TIER_H}px;",
                    for (index, tick) in tick_window(minor, &scale, &span).map(|i| (i, &minor[i])) {
                        {
                            let x = scale.x_date(tick.from);
                            let w = (tick.to - tick.from).num_days() as f64 * scale.px_per_day;
                            let weekend = !project.calendar.is_working_day(tick.from);
                            let class = if weekend { "tl-cell weekend" } else { "tl-cell" };
                            let label = fit(&tick.label, w);
                            rsx! {
                                div {
                                    key: "mn{index}",
                                    class: "{class}",
                                    style: "left: {x}px; width: {w}px; height: {HEADER_H - TIER_H}px;",
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
    };

    let body = rsx! {
        div {
            // A report sizes to the chart instead of being a pane the chart is
            // scrolled inside, so the whole chain is on the page at once.
            class: if interactive { "chart-body" } else { "chart-body report" },
            style: "width: {width}px;",
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
            div { class: "chart-canvas", style: "width: {width}px; height: {body_h}px;",
            // ---- chart body ---------------------------------------------
            //
            // Boxes, not an inline drawing, for the same reasons the timescale
            // above is. This renderer serialises an inline drawing, hands the
            // markup to a parser and keeps the answer as a picture, and the
            // boxes its children would have had are thrown away with them. A
            // stylesheet does not reach inside one, an ancestor's `overflow`
            // does not clip anything in one, and, worst of the three, nothing
            // in one can be hit tested, so every handler this chart carried
            // had never once fired.
            //
            // The body is as wide as the plan is long and is slid sideways by
            // the same margin the timescale is slid by, so the two cannot
            // disagree about where a date sits. It used to draw itself into a
            // picture the width of the pane and carry the offset in that
            // picture's own coordinates, because a picture is cut off at its
            // own edges while a picture that has been slid is not. A tree of
            // boxes is clipped by the pane like anything else, so that trick
            // has nothing left to buy.

                // First, so everything else is over it and it takes a pointer
                // only where the chart is otherwise bare. It exists to answer
                // one question, which is where the chart's own coordinates
                // start on the screen, and it is the whole canvas placed at
                // the canvas's own origin, so a position measured against it
                // is already a chart coordinate. A report has no live session
                // to tell, so it does not get one.
                if interactive {
                    div {
                        class: "chart-probe",
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

                // Shaded days, banded rows and gridlines are all held in
                // order, so the stretch of each that is on screen is a pair of
                // binary searches. They used to be a walk over the whole plan
                // that threw away everything but a screenful.
                for (index, x) in nonworking_window.clone().map(|i| (i, nonworking[i])) {
                    div {
                        key: "nw{index}",
                        class: "gc-nonworking",
                        style: "left: {x}px; width: {scale.px_per_day}px; \
                                top: {band_top}px; height: {band_h}px;",
                    }
                }

                for line in band_window.clone().map(|i| band_lines[i]) {
                    div {
                        key: "bd{line}",
                        class: "gc-band",
                        style: "top: {line as f64 * ROW_H}px; height: {ROW_H}px;",
                    }
                }

                for line_index in window.first..(window.end + 1).min(rows_len + 1) {
                    {
                        let y = line_index as f64 * ROW_H;
                        rsx! {
                            if grid_rows {
                                div { key: "rl{line_index}", class: "gc-rule-h", style: "top: {y}px;" }
                            }
                        }
                    }
                }

                for (index, tick) in minor_window.clone().map(|i| (i, &minor[i])) {
                    {
                        let x = scale.x_date(tick.from);
                        rsx! {
                            if grid_columns {
                                div {
                                    key: "vl{index}",
                                    class: "gc-rule-v",
                                    style: "left: {x}px; top: {band_top}px; height: {band_h}px;",
                                }
                            }
                        }
                    }
                }

                // ---- annotations drawn under the plan --------------------
                if show_drawings {
                    div { class: "drawings",
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
                //
                // The index gives back the arrows that cross the window and
                // nothing else. Asking the plan instead meant a pass over
                // every link it holds, and a node in the tree for each of
                // them, because an arrow that is not drawn still had to be
                // decided against and a decision is an element either way.
                for (index, link) in arrows.iter().map(|&i| (i, &project.links[i])) {
                    {
                        match arrow_path(boxes, link.predecessor, link.successor, link.kind) {
                            Some((runs, head_x, head_y, downward)) => rsx! {
                                div { key: "lk{index}", class: "gc-link",
                                    for (leg, run) in runs.iter().enumerate() {
                                        div {
                                            key: "rn{leg}",
                                            class: "gc-link-run",
                                            style: "left: {run.x}px; top: {run.y}px; \
                                                    width: {run.w}px; height: {run.h}px;",
                                        }
                                    }
                                    {arrow_head(head_x, head_y, downward)}
                                }
                            },
                            None => rsx! {},
                        }
                    }
                }

                // ---- bars ------------------------------------------------
                //
                // Sliced, not filtered. The rows are held in the order they
                // are drawn in, so the rows on screen are a range of the plan
                // rather than a property of it, and a plan of a hundred and
                // fifty thousand tasks costs the same per frame as a plan of
                // fifty. What is left to test is the other axis: a row can be
                // on screen while its bar is a year off to the side.
                for (line_index, index) in rows[window.lines()]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| (window.first + offset, row))
                    .filter_map(task_line)
                    .filter(|(_, i)| {
                        let t = &project.tasks[*i];
                        let left = scale.x_work(&project.calendar, t.scheduled.start);
                        let right = scale.x_work(&project.calendar, t.scheduled.finish);
                        // The trailing label reaches past the bar's own end.
                        span.overlaps(left, right + BAR_LABEL_REACH)
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
                        // A border is drawn inside the box it is on, while the
                        // stroke it replaces straddled the bar's own edge, so
                        // the box is grown by the width of the border and
                        // moved back by half of it. That is what puts the bar
                        // back on the pixels it was already on.
                        let live = dragging_this || hovered;
                        let edge = if live { 1.4 } else { 0.6 };
                        let bar_class = match (live, dragging_this) {
                            (true, true) => "gc-bar live dragged",
                            (true, false) => "gc-bar live",
                            _ => "gc-bar",
                        };
                        let grip = (bar_w * 0.25).min(7.0);
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
                            div {
                                key: "bar{index}",
                                // The stand-in for the group this was: a box
                                // over the whole canvas that takes no pointer
                                // of its own, so it hears exactly what its
                                // children hear and nothing else, and they go
                                // on being placed in the chart's coordinates
                                // rather than the row's.
                                class: "gc-row",
                                // The tooltip is an attribute rather than an
                                // element. A `title` element is the document's
                                // title here, which is the window's own title,
                                // so pointing at a bar would have renamed the
                                // window.
                                title: "{tip}",

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

                                if s.show_baseline {
                                    if let Some(base) = baseline {
                                        div {
                                            class: "gc-baseline",
                                            style: "left: {scale.x_work(&project.calendar, base.start)}px; \
                                                    top: {y + ROW_H - 5.0}px; \
                                                    width: {(scale.x_work(&project.calendar, base.finish) - scale.x_work(&project.calendar, base.start)).max(2.0)}px; \
                                                    background: {styles.baseline};",
                                        }
                                    }
                                }

                                if s.show_slack && !summary && has_slack {
                                    div {
                                        class: "gc-slack",
                                        style: "left: {right}px; top: {centre - 0.5}px; \
                                                width: {(slack_x - right).max(0.0)}px;",
                                    }
                                }

                                // A band across the whole row, so a milestone,
                                // which has no bar edge to outline, still says
                                // it is the one being pointed at.
                                if hovered {
                                    div { class: "gc-hover", style: "top: {y}px; height: {ROW_H}px;" }
                                }

                                if project.is_marker(index) {
                                    div {
                                        class: if interactive { "gc-grab live" } else { "gc-grab" },
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
                                        {milestone_marker(
                                            left + ghost_dx,
                                            centre,
                                            if critical { &styles.critical } else { &styles.milestone },
                                        )}
                                    }
                                } else if summary {
                                    {summary_bar(left, right, y + (ROW_H - SUMMARY_H) / 2.0, &styles.summary)}
                                } else {
                                    div {
                                        class: "{bar_class}",
                                        style: "left: {left + ghost_dx - edge / 2.0}px; top: {bar_y - edge / 2.0}px; \
                                                width: {ghost_w + edge}px; height: {BAR_H + edge}px; \
                                                background: {fill};",
                                    }
                                    if done_w > 0.5 {
                                        div {
                                            class: "gc-progress",
                                            style: "left: {left + ghost_dx}px; top: {centre - 1.5}px; \
                                                    width: {done_w}px; background: {progress_fill};",
                                        }
                                    }

                                    // Hit zones: left sets progress, right resizes,
                                    // the middle moves the whole bar. They come
                                    // after the bar, so the pointer reaches them
                                    // first. A report has none of them at all,
                                    // rather than three invisible boxes that do
                                    // nothing.
                                    if interactive {
                                        div {
                                            class: "gc-grip progress",
                                            style: "left: {left}px; top: {bar_y}px; \
                                                    width: {grip}px; height: {BAR_H}px;",
                                            onmousedown: move |event| {
                                                event.stop_propagation();
                                                let x = event.client_coordinates().x;
                                                state.write().begin_bar_drag(index, BarDragKind::Progress, x, bar_w);
                                            },
                                        }
                                        div {
                                            class: "gc-grip resize",
                                            style: "left: {right - grip}px; top: {bar_y}px; \
                                                    width: {grip}px; height: {BAR_H}px;",
                                            onmousedown: move |event| {
                                                event.stop_propagation();
                                                let x = event.client_coordinates().x;
                                                state.write().begin_bar_drag(index, BarDragKind::Resize, x, bar_w);
                                            },
                                        }
                                        div {
                                            class: "gc-grip whole",
                                            style: "left: {left + grip}px; top: {bar_y}px; \
                                                    width: {(bar_w - 2.0 * grip).max(0.0)}px; height: {BAR_H}px;",
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

                                if bar_text && !label.is_empty() {
                                    // A row's worth of height centred on the
                                    // line the drawn text sat on, because a box
                                    // holds its text by the middle of the box
                                    // and the text it replaces was held by the
                                    // middle of the glyphs.
                                    div {
                                        class: "bar-label",
                                        style: "left: {right + 6.0}px; top: {centre + 0.5 - ROW_H / 2.0}px; \
                                                height: {ROW_H}px;",
                                        "{label}"
                                    }
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
                for (line_index, index) in rows[window.lines()]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| (window.first + offset, row))
                    .filter_map(task_line)
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
                                        div {
                                            key: "ex{index}-{entry.id}",
                                            class: "gc-external",
                                            title: "{entry.reference}: {entry.label}",
                                            // A pin through the row: it is a
                                            // date nothing in the plan can move.
                                            div {
                                                class: "gc-pin",
                                                style: "left: {x - 0.75}px; top: {y + 1.0}px; \
                                                        height: {ROW_H - 2.0}px;",
                                            }
                                            div {
                                                class: "gc-flag",
                                                style: "left: {x - 4.0}px; top: {mid - 5.0}px;",
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
                    div { class: "drawings",
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
                    div {
                        class: "gc-today",
                        style: "left: {x - 0.5}px; top: {band_top}px; height: {band_h}px;",
                    }
                }

                if tracking {
                    if let Some(x) = status_x {
                        div {
                            class: "gc-status",
                            style: "left: {x - 0.7}px; top: {band_top}px; height: {band_h}px;",
                        }
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
                // over everything else and so the pointer reaches it first. It
                // is the whole canvas at the canvas's own origin and carries
                // nothing that could move it, so a position measured against it
                // already is a chart coordinate: no bounding rectangles, no
                // eval. A report never gets one: there is nothing on it to
                // draw on.
                if interactive && (draw_tool.is_some() || draw_drag.is_some()) {
                    div {
                        class: if draw_tool.is_some() { "chart-sheet drawing" } else { "chart-sheet moving" },
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

            // Inside the pane, in the chart's own coordinates, so the pane
            // carries other people's pointers along as it scrolls and clips
            // the ones that have gone off it. A report has no peers.
            if interactive {
                crate::cursors::ChartCursors {}
            }
            }
        }
    };

    match part {
        Part::Head => head,
        Part::Body => body,
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

/// The diamond that stands for a milestone.
///
/// A square with its corners cut off, and not a square turned through forty
/// five degrees. A transformed box is painted outside its parent's clip on
/// this renderer, so a diamond made by rotation would still be on screen after
/// the pane it belongs to had scrolled past it. Cutting the shape out of an
/// upright box leaves the box upright, and an upright box is clipped normally.
fn milestone_marker(x: f64, y: f64, fill: &str) -> Element {
    let size = 5.5;
    rsx! {
        div {
            class: "gc-milestone",
            style: "left: {x - size}px; top: {y - size}px; \
                    width: {size * 2.0}px; height: {size * 2.0}px; background: {fill};",
        }
    }
}

/// Project draws a summary as a flat spanning bar with a downward spike at
/// each end, so the rolled-up range reads at a glance.
///
/// Three boxes: the bar, and a right angled triangle cut out of a box at each
/// end. The spikes are the only part that is not a rectangle, so they are the
/// only part that needs cutting.
fn summary_bar(left: f64, right: f64, y: f64, fill: &str) -> Element {
    let width = (right - left).max(4.0);
    let right = left + width;
    let cap = (width / 2.0).min(5.0);
    let spike = SUMMARY_H + 5.0;

    rsx! {
        div {
            class: "gc-summary",
            style: "left: {left}px; top: {y}px; width: {width}px; height: {SUMMARY_H}px; \
                    background: {fill};",
        }
        div {
            class: "gc-cap left",
            style: "left: {left}px; top: {y + SUMMARY_H}px; \
                    width: {cap}px; height: {spike - SUMMARY_H}px; background: {fill};",
        }
        div {
            class: "gc-cap right",
            style: "left: {right - cap}px; top: {y + SUMMARY_H}px; \
                    width: {cap}px; height: {spike - SUMMARY_H}px; background: {fill};",
        }
    }
}

/// The boxes that paint an elbow through a list of corners.
///
/// Each leg is a box one pixel across rather than a stroke. A stroke straddles
/// the line it is drawn on, so the box is pulled back half a pixel to sit
/// where the stroke sat, and it is stretched half a pixel into each corner it
/// shares with the next leg, so a join leaves no notch. The free ends are left
/// alone: a stroke ends flush at its last point and so does this.
fn elbow_runs(corners: &[(f64, f64)]) -> Vec<Placement> {
    corners
        .windows(2)
        .enumerate()
        .map(|(leg, pair)| {
            let ((x1, y1), (x2, y2)) = (pair[0], pair[1]);
            let back = if leg == 0 { 0.0 } else { 0.5 };
            let on = if leg + 2 == corners.len() { 0.0 } else { 0.5 };
            if (y2 - y1).abs() < (x2 - x1).abs() {
                let (from, to) = if x1 <= x2 {
                    (x1 - back, x2 + on)
                } else {
                    (x2 - on, x1 + back)
                };
                Placement { x: from, y: y1 - 0.5, w: to - from, h: 1.0 }
            } else {
                let (from, to) = if y1 <= y2 {
                    (y1 - back, y2 + on)
                } else {
                    (y2 - on, y1 + back)
                };
                Placement { x: x1 - 0.5, y: from, w: 1.0, h: to - from }
            }
        })
        .collect()
}

/// Route an elbow from one bar to another. Returns the legs to paint, the
/// arrow head position, and whether the head points down rather than right.
fn arrow_path(
    boxes: &BarBoxes,
    predecessor: TaskId,
    successor: TaskId,
    kind: LinkType,
) -> Option<(Vec<Placement>, f64, f64, bool)> {
    let from = boxes.get(predecessor)?;
    let to = boxes.get(successor)?;

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
        let runs = elbow_runs(&[(start_x, y1), (start_x, y2 - 6.0)]);
        return Some((runs, start_x, y2 - 6.0, true));
    }

    // The straightforward case: the successor sits clear of the predecessor.
    if (approach > 0.0 && end_x >= start_x + stub) || (approach < 0.0 && end_x <= start_x - stub) {
        let corner = end_x - approach;
        let runs = elbow_runs(&[(start_x, y1), (corner, y1), (corner, y2), (end_x, y2)]);
        return Some((runs, end_x, y2, false));
    }

    // Otherwise the successor starts before the predecessor ends, so the line
    // has to double back. Run it between the two rows rather than through them.
    let between = y1 + ROW_H / 2.0;
    let out = start_x + approach;
    let runs = elbow_runs(&[
        (start_x, y1),
        (out, y1),
        (out, between),
        (end_x, between),
        (end_x, y2 - 6.0),
    ]);
    Some((runs, end_x, y2 - 6.0, true))
}

/// The head of a dependency arrow, at the end it points at.
///
/// A box with no size of its own and three borders, two of them transparent,
/// is a triangle. Made that way rather than by cutting one out of a box,
/// because a border costs the frame nothing while a clip path costs it one of
/// its layers, and there is one of these per link on screen.
fn arrow_head(x: f64, y: f64, downward: bool) -> Element {
    let (class, left, top) = if downward {
        ("gc-link-head down", x - 3.0, y)
    } else {
        ("gc-link-head right", x - 5.0, y - 3.0)
    };
    rsx! { div { class: "{class}", style: "left: {left}px; top: {top}px;" } }
}

// ---------------------------------------------------------------- drawings

/// The chart, as the placement maths in `aop_core::draw` wants to see it.
///
/// Borrowing the layout's bar geometry rather than rebuilding it means a shape
/// pinned to a bar lands on exactly the bar that was drawn, snapping and all.
struct ChartView<'a> {
    scale: Scale,
    boxes: &'a BarBoxes,
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
            .get(task)
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

/// The upright box a set of corners fits in, and those corners again as the
/// clip path that cuts the shape back out of it.
///
/// A box cannot be leaned over without a transform, and a transformed box
/// escapes the clip of the pane it is in on this renderer, so anything that is
/// not upright is cut out of a box that is. Cutting costs the frame one of its
/// clipping layers, so it is kept for the shapes that genuinely lean: a
/// triangle standing square on can be had from borders for nothing.
///
/// `None` for a set of corners with no area, which has nothing to paint.
fn clipped_box(corners: &[(f64, f64)]) -> Option<(Placement, String)> {
    let (first, rest) = corners.split_first()?;
    let (mut low, mut high) = (*first, *first);
    for (x, y) in rest {
        low = (low.0.min(*x), low.1.min(*y));
        high = (high.0.max(*x), high.1.max(*y));
    }
    let (w, h) = (high.0 - low.0, high.1 - low.1);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let points = corners
        .iter()
        .map(|(x, y)| format!("{:.2}px {:.2}px", x - low.0, y - low.1))
        .collect::<Vec<_>>()
        .join(", ");
    Some((Placement { x: low.0, y: low.1, w, h }, format!("polygon({points})")))
}

/// The four corners of a stroke of the given weight laid along a line.
fn stroke_corners(x1: f64, y1: f64, x2: f64, y2: f64, weight: f64) -> Option<[(f64, f64); 4]> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let length = dx.hypot(dy);
    if length < 0.001 {
        return None;
    }
    // The perpendicular, half a stroke long, which is how far the stroke
    // stands off the line on either side.
    let (nx, ny) = (-dy / length * weight / 2.0, dx / length * weight / 2.0);
    Some([
        (x1 + nx, y1 + ny),
        (x2 + nx, y2 + ny),
        (x2 - nx, y2 - ny),
        (x1 - nx, y1 - ny),
    ])
}

/// A colour broken up the way a dash pattern breaks up a stroke, running along
/// the direction given.
///
/// A background is the only thing a box has that can be broken up, so a dashed
/// line is a repeating gradient of colour and nothing. The angle is measured
/// the way CSS measures one, clockwise from straight up, while the direction
/// handed in is measured the way a screen counts, downwards.
fn dashed_paint(dx: f64, dy: f64, colour: &str, pattern: &str) -> String {
    let mut lengths = pattern.split_whitespace().filter_map(|n| n.parse::<f64>().ok());
    let on = lengths.next().unwrap_or(1.0).max(0.1);
    let off = lengths.next().unwrap_or(on).max(0.1);
    let angle = dx.atan2(-dy).to_degrees();
    format!(
        "repeating-linear-gradient({angle:.2}deg, {colour} 0 {on}px, \
         transparent {on}px {}px)",
        on + off
    )
}

/// How wide a pointer target a drawn hairline is given.
const LINE_GRAB: f64 = 8.0;

/// The most boxes one drawn line's pointer target is allowed to cost.
const LINE_GRAB_STEPS: usize = 24;

/// Pointer targets along a drawn line.
///
/// A clip path decides what is painted and not what is pointed at here, so a
/// leaning line given one box would answer for the whole rectangle it leans
/// through, which on a long diagonal is most of the chart. A short chain of
/// boxes following the line keeps the target on the line itself, which is what
/// the fat invisible companion stroke was always for. A line that does not
/// lean needs one box, and gets one.
fn line_targets(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<Placement> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    if dx.hypot(dy) < 0.001 {
        return Vec::new();
    }
    // Only the shorter of the two extents makes a staircase necessary: a run
    // that is flat in one direction is one box however long it is.
    let drift = dx.abs().min(dy.abs());
    let steps = ((drift / LINE_GRAB).ceil() as usize).clamp(1, LINE_GRAB_STEPS);
    (0..steps)
        .map(|step| {
            let (near, far) = (step as f64 / steps as f64, (step + 1) as f64 / steps as f64);
            let (ax, ay) = (x1 + dx * near, y1 + dy * near);
            let (bx, by) = (x1 + dx * far, y1 + dy * far);
            Placement {
                x: ax.min(bx) - LINE_GRAB / 2.0,
                y: ay.min(by) - LINE_GRAB / 2.0,
                w: (bx - ax).abs() + LINE_GRAB,
                h: (by - ay).abs() + LINE_GRAB,
            }
        })
        .collect()
}

/// The CSS keyword that breaks a border up the way a line style asks for.
///
/// `aop_core` keeps the pattern as a dash array, because that is what anything
/// printing the plan will want, and a border is broken up by a keyword rather
/// than by lengths, so the two are matched here.
fn border_dashes(style: LineStyle) -> &'static str {
    match style {
        LineStyle::Solid => "solid",
        LineStyle::Dashed => "dashed",
        LineStyle::Dotted => "dotted",
    }
}

/// A drawn straight line, as an upright box with the lean cut out of it.
fn drawn_line(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    stroke: &str,
    weight: f64,
    style: LineStyle,
) -> Element {
    let Some(corners) = stroke_corners(x1, y1, x2, y2, weight) else {
        return rsx! {};
    };
    let Some((at, clip)) = clipped_box(&corners) else {
        return rsx! {};
    };
    let paint = match style.dasharray() {
        Some(pattern) => dashed_paint(x2 - x1, y2 - y1, stroke, pattern),
        None => stroke.to_string(),
    };
    rsx! {
        div {
            class: "gc-line",
            style: "left: {at.x}px; top: {at.y}px; width: {at.w}px; height: {at.h}px; \
                    background: {paint}; clip-path: {clip};",
        }
    }
}

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
    let edge = border_dashes(d.style.line_style);
    // An unset fill means unfilled, which a box says as `transparent`: `none`
    // is a paint an SVG understands and a background does not.
    let fill = match palette.literal(d.style.fill()) {
        "none" => "transparent",
        colour => colour,
    };
    let ink = palette.literal(d.style.ink());
    let weight = if d.style.bold { "600" } else { "400" };
    let slant = if d.style.italic { "italic" } else { "normal" };
    // A border is drawn inside the box it is on, while the stroke it stands in
    // for straddled the shape's own edge, so the box is grown by a whole
    // stroke and moved back by half of one. The outline then covers exactly
    // the pixels the stroke covered.
    let half = stroke_w / 2.0;
    // A locked shape lets the pointer through to the bars underneath, which is
    // the point of locking one: mark the plan up, then get back to work on it.
    // A report is locked all over, since nothing on it is meant to be touched.
    let inert = d.locked || !interactive;
    let hit = if inert { "none" } else { "auto" };

    rsx! {
        div {
            key: "dw{id}",
            // The whole canvas, taking no pointer of its own, so the shapes
            // inside it keep the chart's coordinates and it hears only what
            // they hear. The group it replaces had no geometry either.
            class: "gc-drawing",
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
                    {drawn_line(at.x, at.y, end_x, end_y, stroke, stroke_w, d.style.line_style)}
                    // An open shape has no interior to click and a hairline is
                    // a hard target, so invisible fat companions carry the
                    // pointer events for it.
                    for (step, target) in line_targets(at.x, at.y, end_x, end_y).into_iter().enumerate() {
                        div {
                            key: "hit{step}",
                            class: "gc-line-grab",
                            style: "left: {target.x}px; top: {target.y}px; \
                                    width: {target.w}px; height: {target.h}px; \
                                    pointer-events: {hit};",
                        }
                    }
                    if d.kind == ShapeKind::Arrow {
                        {arrow_tip(at.x, at.y, end_x, end_y, stroke, stroke_w)}
                    }
                },
                ShapeKind::Rectangle => rsx! {
                    div {
                        class: "gc-shape",
                        // An unfilled box is still a box: without a background
                        // the planner would have to hit the one pixel of its
                        // outline, which is what `pointer-events` said before.
                        style: "left: {outer.x - half}px; top: {outer.y - half}px; \
                                width: {outer.w + stroke_w}px; height: {outer.h + stroke_w}px; \
                                background: {fill}; border: {stroke_w}px {edge} {stroke}; \
                                pointer-events: {hit};",
                    }
                },
                ShapeKind::Oval => rsx! {
                    div {
                        class: "gc-shape oval",
                        style: "left: {outer.x - half}px; top: {outer.y - half}px; \
                                width: {outer.w + stroke_w}px; height: {outer.h + stroke_w}px; \
                                background: {fill}; border: {stroke_w}px {edge} {stroke}; \
                                pointer-events: {hit};",
                    }
                },
                ShapeKind::TextBox => rsx! {
                    div {
                        class: "gc-shape",
                        style: "left: {outer.x - half}px; top: {outer.y - half}px; \
                                width: {outer.w + stroke_w}px; height: {outer.h + stroke_w}px; \
                                background: {fill}; border: {stroke_w}px {edge} {stroke}; \
                                pointer-events: {hit};",
                    }
                    div {
                        class: "draw-text",
                        // A box the height of the shape, holding its caption by
                        // the middle, which is where the drawn text sat.
                        style: "left: {outer.x + 4.0}px; top: {outer.y}px; height: {outer.h}px; \
                                color: {ink}; font-size: {d.style.font_size()}pt; \
                                font-weight: {weight}; font-style: {slant};",
                        "{d.text}"
                    }
                },
            }

            if selected {
                div {
                    class: "gc-outline",
                    style: "left: {outer.x - SELECT_PAD - 0.5}px; top: {outer.y - SELECT_PAD - 0.5}px; \
                            width: {outer.w + 2.0 * SELECT_PAD + 1.0}px; \
                            height: {outer.h + 2.0 * SELECT_PAD + 1.0}px;",
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
    let corners = [
        (x2, y2),
        (base_x + px, base_y + py),
        (base_x - px, base_y - py),
    ];
    let Some((at, clip)) = clipped_box(&corners) else {
        return rsx! {};
    };
    rsx! {
        div {
            class: "gc-tip",
            style: "left: {at.x}px; top: {at.y}px; width: {at.w}px; height: {at.h}px; \
                    background: {stroke}; clip-path: {clip};",
        }
    }
}

/// The outline pulled out while a new shape is being drawn.
///
/// Deliberately not the shape itself: a dashed box says "this is where it will
/// go" without pretending the shape exists before the pointer is let go.
fn rubber_band(kind: ShapeKind, x: f64, y: f64, dx: f64, dy: f64, palette: Palette) -> Element {
    if matches!(kind, ShapeKind::Line | ShapeKind::Arrow) {
        let colour = palette.paint("--accent-bright");
        let Some(corners) = stroke_corners(x, y, x + dx, y + dy, 1.5) else {
            return rsx! {};
        };
        let Some((at, clip)) = clipped_box(&corners) else {
            return rsx! {};
        };
        let paint = dashed_paint(dx, dy, colour, "4 3");
        return rsx! {
            div {
                class: "gc-line",
                style: "left: {at.x}px; top: {at.y}px; width: {at.w}px; height: {at.h}px; \
                        background: {paint}; clip-path: {clip};",
            }
        };
    }
    let band = Placement { x, y, w: dx, h: dy }.normalised();
    rsx! {
        div {
            class: "gc-outline",
            style: "left: {band.x - 0.5}px; top: {band.y - 0.5}px; \
                    width: {band.w + 1.0}px; height: {band.h + 1.0}px;",
        }
    }
}

// ---------------------------------------------------------------- timeline

// The bands run the full width of the strip. What used to be inset here, in
// pixels, was room for the two date labels at the ends; those sit on the line
// above the bands instead, so there is nothing left to reserve.

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

/// How wide a band has to be before it is worth naming.
///
/// The strip is the whole plan squeezed into the width of the window, so there
/// is no viewport to leave anything out of: every band is drawn, and on a plan
/// of a hundred and fifty thousand tasks that is seven and a half thousand of
/// them. Each one used to cost two elements, a blip and a label, and at that
/// count most of the bands are a fraction of a pixel across. A name written
/// beside a band nobody can point at is not telling anybody which phase it
/// belongs to; it is a second element per band, drawn on every frame, above
/// the panes, whatever the panes are doing.
///
/// Three pixels. Below that the band keeps its blip and loses its name, which
/// also keeps its `span` down to the bar itself and lets the packing put more
/// of them on a line. Nothing is left out of the picture of the plan: every
/// phase is still there to be seen and still holds its place.
const BAND_LABEL_MIN: f64 = 3.0;

/// How many lines of the timeline are on screen before it starts scrolling.
///
/// Not a limit on how many there are. A plan of three hundred phases needs a
/// hundred and fifty lines, and it gets them; this is only how much of the
/// window the band is willing to take before the rest is reached by scrolling.
/// It used to be a cap, and a cap on a plan that size meant three hundred and
/// seven phases were quietly left off the picture of the plan.
const BAND_LANES_SHOWN: usize = 8;

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
pub fn measure_band(
    index: usize,
    left: f64,
    right: f64,
    name: &str,
    marker: bool,
    // How many pixels the strip is across, so a label measured in pixels can be
    // compared with a band measured as a fraction of it. An estimate is enough:
    // this decides which line a band is packed onto, never where it is drawn.
    across: f64,
) -> Band {
    // A milestone is a diamond of fixed size rather than a bar, so its width
    // says nothing about whether it can be identified: its name is the only
    // thing that can. Those keep their labels whatever the plan's size.
    let readable = marker || (right - left) * across.max(1.0) >= BAND_LABEL_MIN;
    let label = if readable {
        name.trim().to_string()
    } else {
        String::new()
    };
    let label_w = label.chars().count() as f64 * BAND_CHAR_W / across.max(1.0);

    let inside = !marker && !label.is_empty() && label_w + BAND_LABEL_PAD / across.max(1.0) <= right - left;
    let span_right = if label.is_empty() || inside {
        right
    } else {
        right + (BAND_LABEL_GAP / across.max(1.0)) + label_w
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
/// How many pixels of strip a band is allowed to claim before the strip starts
/// putting them together.
///
/// Two. Below that a band is a mark rather than a bar, and two marks in the
/// same two pixels are one mark that has been drawn twice.
const BAND_MERGE_PX: f64 = 2.0;

/// Put together the bands that land in the same sliver of the strip.
///
/// The strip is the whole plan squeezed into the width of the window, so
/// unlike everything else in the chart there is no viewport to leave anything
/// out of: a plan with seven and a half thousand phases asks for seven and a
/// half thousand bands and their labels, drawn every frame, and measured
/// against a plan of that size it was nine tenths of the time the whole window
/// took to paint.
///
/// Nothing is left out of the picture of the plan. Every phase still puts a
/// mark on the strip; what stops is drawing several of them into the same two
/// pixels, where they were already indistinguishable. A merged mark spans from
/// the first of them to the last and loses its label, because a name on a mark
/// standing for four phases would be naming one of them.
///
/// Untouched when the plan is small enough for every band to have room, which
/// is every ordinary plan: the work here begins only once the strip is asked
/// for more bands than it has pixels.
fn merge_crowded(bands: Vec<Band>, across: f64) -> Vec<Band> {
    let room = (across / BAND_MERGE_PX).max(1.0) as usize;
    if bands.len() <= room {
        return bands;
    }

    // Which sliver of the strip a fraction falls in. The bands arrive sorted
    // by where they start, so anything sharing a sliver arrives together and
    // one pass is enough.
    let sliver = |left: f64| (left * across / BAND_MERGE_PX).floor() as i64;

    let mut out: Vec<Band> = Vec::with_capacity(room + 1);
    for band in bands {
        match out.last_mut() {
            Some(last) if sliver(last.left) == sliver(band.left) => {
                last.right = last.right.max(band.right);
                last.span.1 = last.span.1.max(band.span.1);
                // A merged mark stands for more than one phase, so it stops
                // claiming to be any of them, and stops reserving the room a
                // label would have needed beside it.
                last.label.clear();
                last.inside = false;
            }
            _ => out.push(band),
        }
    }
    out
}

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

/// Where an instant falls along the timeline band, as a fraction of it.
///
/// A fraction rather than a pixel, because the band is laid out in percentages
/// and never learns its own width. Pulled out of the component so it can be
/// tested: two tasks that run back to back must land on exactly the same
/// point, with no gap for the night between them.
pub fn timeline_fraction(
    calendar: &WorkCalendar,
    first_day: NaiveDate,
    total_days: f64,
    at: NaiveDateTime,
) -> f64 {
    (calendar.day_offset(first_day, at) / total_days.max(1.0)).clamp(0.0, 1.0)
}

/// The Timeline band above the workspace, showing the plan at a glance.
///
/// Boxes placed by percentage, not an SVG drawn at a measured width.
///
/// It used to ask the renderer how wide it was, at mount and then again on a
/// timer until the answer settled. That is what "the timeline fails to load"
/// turned out to mean: measuring an element takes the document's `RefCell`
/// unconditionally, and a timer that wakes while a render is in flight panics
/// with "RefCell already borrowed" and takes the whole process with it. On a
/// plan of any size there is always a render in flight, so the bigger the
/// plan the more certain the crash.
///
/// A percentage needs no measuring. A phase that runs a third of the way
/// through the plan is at `left: 33%`, and the renderer works out what that
/// comes to when it lays the strip out, every time, including after a resize.
/// There is nothing to go stale, nothing to sample, and nothing to ask.
#[component]
pub fn TimelineBand() -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;
    let palette = s.theme.palette();

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

    let start = project.start_date;
    let finish = project.finish_date.max(start + Duration::days(1));
    // The band is measured in whole days so consecutive tasks touch, matching
    // the way the chart projects working time onto day columns.
    let first_day = start.date();
    let last_day = finish.date() + Duration::days(1);
    let total_days = (last_day - first_day).num_days().max(1) as f64;
    // Where an instant falls along the strip, as a fraction of it.
    let at = |when: NaiveDateTime| {
        timeline_fraction(&project.calendar, first_day, total_days, when)
    };

    // Only for deciding whether a label fits inside its own band and how much
    // room it needs beside it when it does not. The window's width is known
    // and stays known through a resize, so this is a fair estimate; and being
    // an estimate is all it has to be, because it decides which line a band is
    // packed onto and never where the band is drawn.
    let across = {
        let (width, _) = use_context::<Signal<crate::state::Viewport>>()();
        // The window, less the strip's own padding and the two insets the
        // plan is drawn between. It has to be the width of the box the bands
        // actually live in, or a label reserves the wrong share of it.
        let lanes_across = width - 20.0 - 68.0 - 50.0;
        if lanes_across > 1.0 { lanes_across } else { 1000.0 }
    };

    // The top level of the outline is what a timeline is for.
    let mut bands: Vec<Band> = (0..project.tasks.len())
        .filter(|&i| project.tasks[i].outline_level == 0)
        .map(|index| {
            let task = &project.tasks[index];
            let left = at(task.scheduled.start);
            if project.is_marker(index) {
                let half = BAND_DIAMOND / across;
                measure_band(index, left - half, left + half, &task.name, true, across)
            } else {
                let right = at(task.scheduled.finish).max(left + 3.0 / across);
                measure_band(index, left, right, &task.name, false, across)
            }
        })
        .collect();

    // First fit only packs tightly if the bands arrive in the order they are
    // drawn in, and the outline order is not always the order they start in.
    bands.sort_by(|a, b| a.span.0.total_cmp(&b.span.0));

    // How many phases the plan actually has, before any of them are merged
    // into one another. The caption reports this, not what survived.
    let phases = bands.len();
    let bands = merge_crowded(bands, across);

    let (lane_of, lanes) = pack_lanes(&bands.iter().map(|b| b.span).collect::<Vec<_>>());

    // Every band is drawn. Whichever line the packing put it on, that is the
    // line it goes on, and the strip is as tall as the packing made it.
    let placed: Vec<(Band, usize)> = bands.into_iter().zip(lane_of).collect();

    // Sixteen of band and four of air. The bands were three pixels shorter
    // and sat two apart, which packs more of them into the strip and reads as
    // a stack rather than as a row of things.
    let lane_h = 20.0;
    // What the strip is, and what is on screen of it. When the second is
    // smaller than the first the band scrolls; the dates and the rule they
    // hang off do not, because they are not in the part that scrolls.
    let full = lanes as f64 * lane_h + 4.0;
    let shown = full.min(BAND_LANES_SHOWN as f64 * lane_h + 4.0);

    let caption = if lanes > BAND_LANES_SHOWN || placed.len() < phases {
        let noun = if phases == 1 { "phase" } else { "phases" };
        format!("Timeline \u{2022} {phases} {noun}")
    } else {
        "Timeline".to_string()
    };

    rsx! {
        div { class: "timeline",
            // The dates and the rule they hang off, and the word over the
            // corner. Above the part that scrolls, so a plan long enough to
            // need scrolling does not scroll its own dates away.
            div { class: "tl-head",
                div { class: "timeline-caption", "{caption}" }
                div { class: "tl-edge start", "{crate::state::format_date(start)}" }
                div { class: "tl-edge end", "{crate::state::format_date(finish)}" }
                div { class: "tl-axis" }
            }

            div { class: "tl-strip", style: "height: {shown}px;",
                // The plan runs inside this, not inside the strip. It is inset
                // from both ends so the two dates above have somewhere to sit.
                // Here the inset is the box, so a percentage inside it is
                // still a percentage of the plan and nothing has to know how
                // wide anything is.
                div { class: "tl-lanes", style: "height: {full}px;",

                for (slot, (band, lane)) in placed.into_iter().enumerate() {
                    {
                        let y = lane as f64 * lane_h;
                        let left = band.left * 100.0;
                        let width = ((band.right - band.left) * 100.0).max(0.2);
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
                        let marker = project.is_marker(band.index);
                        // Pale shades need dark ink, dark shades need pale.
                        let ink = if band.inside {
                            shade(&fill, -0.75)
                        } else {
                            palette.paint("--ink-soft").to_string()
                        };
                        // A label within its band starts just inside the edge;
                        // one beside it starts just clear of the right edge.
                        let label_style = if band.inside {
                            format!("left: {left}%; top: {y}px; color: {ink};")
                        } else {
                            format!(
                                "left: {}%; top: {y}px; color: {ink};",
                                (band.right * 100.0).min(100.0)
                            )
                        };
                        let label_class = if band.inside { "tl-blip-label in" } else { "tl-blip-label" };
                        rsx! {
                            if marker {
                                div {
                                    key: "tm{band.index}",
                                    class: "tl-blip marker",
                                    style: "left: {left + width / 2.0}%; top: {y + 1.0}px; \
                                            background: {project.bar_styles.milestone};",
                                }
                            } else {
                                div {
                                    key: "tb{band.index}",
                                    class: "tl-blip",
                                    style: "left: {left}%; width: {width}%; top: {y}px; \
                                            background: {fill}; border-color: {edge};",
                                }
                            }
                            if !band.label.is_empty() {
                                div {
                                    key: "tl{band.index}",
                                    class: "{label_class}",
                                    style: "{label_style}",
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
                .get(task.id)
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

    /// What one frame of a very large plan actually costs, and where.
    ///
    /// Not a correctness test, and not run with the suite: `cargo test -p
    /// aop-app --release -- --ignored --nocapture bench_` prints the figures.
    /// It exists so a claim about this chart's speed is a measurement rather
    /// than an opinion, and so the split between what is paid once per change
    /// and what is paid once per frame stays visible.
    #[test]
    #[ignore = "a benchmark, not a test"]
    fn bench_a_very_large_plan() {
        use std::time::Instant;

        let count = 150_000;
        let t = Instant::now();
        let state = a_plan_of(count);
        println!(
            "\n  plan          {:>9.1?}  ({count} tasks, {} links, scheduled)",
            t.elapsed(),
            state.project.links.len(),
        );

        let t = Instant::now();
        let rows = chart_rows(&state, None);
        let rows_len = rows.len();
        println!("  layout_rows   {:>9.1?}  ({rows_len} rows)", t.elapsed());

        let t = Instant::now();
        let count_only = state.layout_row_count();
        println!("  row_count     {:>9.1?}  ({count_only})", t.elapsed());

        // Where the layout's time actually goes, phase by phase, so the fix
        // lands on the part that is costing rather than on the whole of it.
        {
            let s = &state;
            let (from, to) = chart_range(&s.project);
            let scale = Scale { origin: from, px_per_day: s.zoom.px_per_day() };

            let t = Instant::now();
            let major = major_ticks(s.zoom, from, to);
            let minor = minor_ticks(s.zoom, from, to);
            println!("    ticks       {:>9.1?}  ({} major, {} minor)",
                     t.elapsed(), major.len(), minor.len());

            let t = Instant::now();
            let mut nonworking: Vec<f64> = Vec::new();
            let mut cursor = from;
            while cursor < to {
                if !s.project.calendar.is_working_day(cursor) {
                    nonworking.push(scale.x_date(cursor));
                }
                cursor += Duration::days(1);
            }
            println!("    nonworking  {:>9.1?}  ({})", t.elapsed(), nonworking.len());

            let rows2 = chart_rows(s, None);
            let t = Instant::now();
            let bound = id_bound(&s.project);
            let mut boxes = BarBoxes::with_room_for(bound);
            let mut lines = TaskLines::with_room_for(bound);
            for (line, index) in rows2.iter().enumerate().filter_map(task_line) {
                let task = &s.project.tasks[index];
                lines.set(task.id, line);
                if let Some((left, right)) = bar_edges(&s.project, &scale, s.round_bars, index) {
                    boxes.set(
                        task.id,
                        BarBox { left, right, mid: line as f64 * ROW_H + ROW_H / 2.0 },
                    );
                }
            }
            println!("    geometry    {:>9.1?}  <- two dense tables, {} bars",
                     t.elapsed(), boxes.len());

            let t = Instant::now();
            let index = LinkIndex::build(&s.project.links, &lines, rows2.len());
            println!("    link index  {:>9.1?}  ({} filed, {} long)",
                     t.elapsed(), index.filed.len(), index.long.len());
        }

        let t = Instant::now();
        let layout = build_layout(&state, rows, None);
        println!("  build_layout  {:>9.1?}  <- once per change, not per frame", t.elapsed());

        // What a frame actually walks now: the windows, and the arrows.
        let scale = Scale { origin: layout.from, px_per_day: state.zoom.px_per_day() };
        let window = RowWindow::new(300_000.0, 900.0, rows_len);
        let span = SpanWindow::new(40_000.0, 1_500.0);

        let t = Instant::now();
        let rounds = 100u32;
        let mut seen = 0usize;
        for _ in 0..rounds {
            seen += tick_window(&layout.minor, &scale, &span).len();
            seen += layout.links.crossing(window).count();
            seen += layout.rows[window.lines()].len();
        }
        println!(
            "  one frame     {:>9.1?}  ({} elements windowed)\n",
            t.elapsed() / rounds,
            seen / rounds as usize,
        );
    }

    /// What a frame costs at the size this build is aimed at must not depend
    /// on the size of the plan.
    ///
    /// The benchmark above prints the figures; this one fails the suite when
    /// they stop holding. Everything asserted here was untrue before the
    /// windowing went in: the chart walked the whole plan for its bars, its
    /// arrows, its ticks and its shaded days, and drew gridlines as tall as
    /// the whole canvas, so a hundred and fifty thousand rows cost a hundred
    /// and fifty thousand rows' worth of work to show forty of them.
    ///
    /// A hundred and fifty thousand tasks takes a moment to build and to
    /// schedule even optimised, so it is out of the ordinary run:
    ///
    ///     cargo test -p aop-app --release -- --ignored at_a_hundred
    #[test]
    #[ignore = "builds a hundred and fifty thousand tasks"]
    fn a_frame_at_a_hundred_and_fifty_thousand_rows_costs_a_windowful() {
        let count = 150_000;
        let state = a_plan_of(count);

        let rows = chart_rows(&state, None);
        let rows_len = rows.len();
        assert_eq!(rows_len, count, "every task is a row");
        let layout = build_layout(&state, rows, None);

        // Somewhere down the middle, so nothing is being saved by the window
        // happening to sit at an end of the plan.
        let half = rows_len as f64 * ROW_H / 2.0;
        let tall = 900.0;
        let window = RowWindow::new(half, tall, rows_len);
        let span = SpanWindow::new(4_000.0, 1_500.0);
        let scale = Scale { origin: layout.from, px_per_day: state.zoom.px_per_day() };

        // Generous: the window is the viewport plus its overscan plus the
        // block it snaps to, and this is about orders of magnitude, not about
        // pinning the constants in `viewport`.
        let ceiling = (tall / ROW_H) as usize + BLOCK_ALLOWANCE;

        let drawn = window.lines().len();
        assert!(
            drawn <= ceiling,
            "{drawn} rows drawn for a {tall}px pane: the row window is not cutting the plan down"
        );
        // Not vacuous: a window over the middle of the plan has rows in it,
        // and a test that passes because nothing was drawn is a test that
        // passes when the chart is blank.
        assert!(drawn > 0, "the window is over the middle of the plan and drew nothing");

        let arrows = layout.links.crossing(window).count();
        assert!(
            arrows <= ceiling,
            "{arrows} arrows for {drawn} rows out of {} links: the link index is not cutting the \
             plan down",
            state.project.links.len()
        );
        assert!(
            arrows > 0,
            "every row but one in twenty is linked to the next, so a window of {drawn} rows \
             crosses arrows"
        );
        // The point of the index, stated as a ratio rather than a count: what
        // comes back is a windowful whatever the plan holds.
        assert!(
            arrows * 100 < state.project.links.len(),
            "{arrows} of {} links came back, which is not a window",
            state.project.links.len()
        );

        let ticks = tick_window(&layout.minor, &scale, &span).len();
        assert!(
            ticks <= layout.minor.len(),
            "the tick window cannot hand back more ticks than the timescale has"
        );

        // The furniture that runs down the chart is cut to the drawn rows, not
        // to the canvas. A gridline as tall as the plan is three and a third
        // million pixels of box that this renderer turns into strips a
        // scanline at a time, and it was half a second a frame.
        let band_h = window.lines().len() as f64 * ROW_H;
        let canvas_h = rows_len as f64 * ROW_H;
        assert!(
            band_h < canvas_h / 100.0,
            "the drawn band is {band_h}px of a {canvas_h}px canvas, which is not a window"
        );
    }

    /// What the timeline band costs on a plan of this size, which is the one
    /// part of the window that has no viewport to be windowed against.
    ///
    /// The strip is the whole plan squeezed into the width of the window, so
    /// "what is on screen" is all of it by construction, and the code says so:
    /// *every* band is drawn. A band is a top level task, and it costs two
    /// elements, a blip and its label. On a plan with a phase every twentieth
    /// row that is a fixed cost proportional to the plan and paid on every
    /// frame, above the panes, whatever the panes are doing.
    ///
    /// This does not assert a budget, because what to do about it is a choice
    /// rather than a bug: the comment on the strip says nothing is left out of
    /// the picture of the plan, and that is a decision. It prints the numbers
    /// so the decision is made against them.
    ///
    ///     cargo test -p aop-app --release -- --ignored --nocapture what_the_timeline
    #[test]
    #[ignore = "builds a hundred and fifty thousand tasks"]
    fn what_the_timeline_band_costs_on_a_very_large_plan() {
        // The strip's own width, near enough: a maximised window less the
        // caption, the two dates and the padding.
        let across = 1700.0;

        for count in [500usize, 150_000] {
            let state = a_plan_of(count);
            let project = &state.project;

            // Measured by the strip's own function, not by arithmetic written
            // here to look like it. A number from a copy of the code is a
            // number about the copy.
            let bands: Vec<Band> = (0..project.tasks.len())
                .filter(|&i| project.tasks[i].outline_level == 0)
                .map(|index| {
                    let task = &project.tasks[index];
                    let left = (task.scheduled.start - project.start_date).num_minutes() as f64
                        / (project.finish_date - project.start_date).num_minutes().max(1) as f64;
                    let right = (task.scheduled.finish - project.start_date).num_minutes() as f64
                        / (project.finish_date - project.start_date).num_minutes().max(1) as f64;
                    measure_band(index, left, right.max(left), &task.name, false, across)
                })
                .collect();

            let labelled = bands.iter().filter(|b| !b.label.is_empty()).count();
            let (_, lanes) = pack_lanes(&bands.iter().map(|b| b.span).collect::<Vec<_>>());

            println!(
                "\n  {count} tasks\n    bands        {}\n    labelled     {labelled}\
                 \n    elements     {}  (was {} with a label on every band)\
                 \n    lanes        {lanes}",
                bands.len(),
                bands.len() + labelled,
                bands.len() * 2,
            );
        }
    }

    /// How much wider than the viewport the row window is allowed to be.
    ///
    /// The window snaps to a block so that scrolling does not rebuild the
    /// chart every row, and it draws an overscan either side of that. Both are
    /// counted in rows and both are small; this is their sum with room to
    /// spare, so tuning either does not fail the test above.
    const BLOCK_ALLOWANCE: usize = 64;

    /// A plan of `count` tasks: a summary every twentieth row, its children
    /// chained under it. The same shape `aop-core`'s `big_plan` example
    /// writes, so a figure from the suite and a figure from the application
    /// are measuring the same thing.
    fn a_plan_of(count: usize) -> AppState {
        let mut state = AppState::new();
        // Read before the loop: the tasks are borrowed mutably inside it.
        let start = state.project.start_date;
        for index in 0..count {
            let at = state.project.tasks.len();
            state.project.insert_task(at, format!("Task {index}"));
            if let Some(task) = state.project.tasks.last_mut() {
                task.outline_level = if index % GROUP == 0 { 0 } else { 1 };
                task.duration_minutes = 480;
                task.estimated = false;
                // The first step of each phase is pinned, which drags the
                // phase with it; pinning the summary would do nothing,
                // because a summary is driven by its children.
                if index % GROUP == 1 {
                    let offset = (index / GROUP) as i64 % SPREAD;
                    task.constraint = aop_core::ConstraintType::StartNoEarlierThan;
                    task.constraint_date = Some(start + Duration::days(offset));
                }
            }
        }
        let ids: Vec<TaskId> = state.project.tasks.iter().map(|task| task.id).collect();
        for index in 0..count.saturating_sub(1) {
            if index % GROUP != 0 && index % GROUP != GROUP - 1 {
                state
                    .project
                    .links
                    .push(aop_core::Link::finish_to_start(ids[index], ids[index + 1]));
            }
        }
        state.reschedule();
        state
    }

    /// Rows per summary, and how many days the phases are spread over before
    /// wrapping. The same two numbers `aop-core`'s `big_plan` example uses, so
    /// a figure from the suite and a figure from the application are measuring
    /// the same plan.
    const GROUP: usize = 20;
    const SPREAD: i64 = 500;

    /// The index has to give the same answer the walk gave.
    ///
    /// The walk asked `RowWindow::spans` about every link in the plan. The
    /// index answers from a slice and a short list of the links that reach too
    /// far to file, which is a different route to what has to be the same set:
    /// an arrow that goes missing here is an arrow missing from the chart, and
    /// nothing on screen would say why.
    #[test]
    fn the_link_index_answers_what_the_walk_answered() {
        let mut state = AppState::new();
        // Long enough that the window is a small part of it and that links can
        // reach further than `LONG_LINK`, so both halves of the index are
        // exercised rather than only the filed one.
        let count = LONG_LINK * 3;
        for index in 0..count {
            state.append_task(&format!("T{index}"));
        }
        let ids: Vec<TaskId> = state.project.tasks.iter().map(|t| t.id).collect();

        // A spread of reaches: the neighbourly ones that get filed, and a few
        // that cross most of the plan and go in the long list.
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for from in (0..count - 1).step_by(7) {
            pairs.push((from, from + 1));
        }
        for from in (0..count / 2).step_by(101) {
            pairs.push((from, from + count / 2));
        }
        // Backwards, so the index cannot be relying on the predecessor being
        // the higher of the two rows.
        for from in (10..count).step_by(313) {
            pairs.push((from, from - 10));
        }
        for &(a, b) in &pairs {
            state.project.links.push(aop_core::Link::finish_to_start(ids[a], ids[b]));
        }
        state.reschedule();

        let rows = chart_rows(&state, None);
        let rows_len = rows.len();
        let layout = build_layout(&state, rows, None);
        assert_eq!(layout.rows.len(), count, "every task is a row");
        assert!(!layout.links.long.is_empty(), "the long list has to be under test too");
        assert!(!layout.links.filed.is_empty(), "so does the filed one");

        for top in [0.0, 137.0, 4000.0, 19_000.0, 90_000.0] {
            let window = RowWindow::new(top, 700.0, rows_len);

            // What the walk would have said, straight out of the plan.
            let mut expected: Vec<usize> = state
                .project
                .links
                .iter()
                .enumerate()
                .filter(|(_, link)| {
                    match (
                        layout.lines.get(link.predecessor),
                        layout.lines.get(link.successor),
                    ) {
                        (Some(a), Some(b)) => window.spans(a, b),
                        _ => false,
                    }
                })
                .map(|(index, _)| index)
                .collect();

            let mut got: Vec<usize> = layout.links.crossing(window).collect();
            expected.sort_unstable();
            got.sort_unstable();
            assert_eq!(got, expected, "at scroll {top}");
            // Scrolled past the last row there is nothing to cross, and the
            // two agreeing on nothing is still the two agreeing. Anywhere the
            // window holds rows, this plan links every seventh one, so an
            // empty answer there would mean the index had lost them.
            if window.end > window.first {
                assert!(!got.is_empty(), "at scroll {top} the window should cross links");
            }
        }
    }

    /// Scrolling must not cost a pass over the plan.
    ///
    /// The point of the index is that what comes back is a windowful whatever
    /// the plan's size, so this pins the size of the answer rather than the
    /// time it took to reach: a regression that goes back to walking would
    /// hand back every link that spans the window, which on a plan this shape
    /// is most of them.
    #[test]
    fn a_window_costs_a_windowful_of_arrows() {
        let mut state = AppState::new();
        let count = 4_000;
        for index in 0..count {
            state.append_task(&format!("T{index}"));
        }
        let ids: Vec<TaskId> = state.project.tasks.iter().map(|t| t.id).collect();
        for from in 0..count - 1 {
            state
                .project
                .links
                .push(aop_core::Link::finish_to_start(ids[from], ids[from + 1]));
        }
        state.reschedule();

        let rows = chart_rows(&state, None);
        let rows_len = rows.len();
        let layout = build_layout(&state, rows, None);

        let window = RowWindow::new(20_000.0, 700.0, rows_len);
        let drawn = layout.links.crossing(window).count();
        let on_screen = window.end - window.first;
        assert!(
            drawn <= on_screen + 2,
            "{drawn} arrows for {on_screen} rows: the window is not cutting the plan down"
        );
        assert!(drawn > 0, "the window is over linked rows, so it should have arrows");
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

        // The same two steps the bar loop takes: slice to the rows on screen,
        // then keep the ones whose bar is inside the stretch of timescale.
        let drawn = layout.rows[window.lines()]
            .iter()
            .enumerate()
            .map(|(offset, row)| (window.first + offset, row))
            .filter_map(task_line)
            .filter(|(_, index)| {
                // The geometry the layout already worked out, which is the
                // same span the bar loop tests against.
                let id = state.project.tasks[*index].id;
                match layout.boxes.get(id) {
                    Some(box_) => span.overlaps(box_.left, box_.right),
                    None => false,
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
        let marker = measure_band(0, 95.0, 105.0, "Compliance checklist created", true, 1.0);
        assert!(!marker.inside, "a marker never carries its name within it");
        assert!(
            marker.span.1 > 105.0 + 100.0,
            "the name has to be counted, got {:?}",
            marker.span
        );
    }

    #[test]
    fn a_band_packed_after_a_marker_clears_its_name() {
        let marker = measure_band(0, 95.0, 105.0, "Compliance checklist created", true, 1.0);
        let bar = measure_band(1, 120.0, 400.0, "Phase 7: Dashboards", false, 1.0);
        let (lanes, used) = pack_lanes(&[marker.span, bar.span]);
        assert_eq!(used, 2, "the bar starts inside the marker's name");
        assert_ne!(lanes[0], lanes[1]);
    }

    #[test]
    fn a_name_that_fits_its_bar_is_drawn_within_it() {
        let bar = measure_band(0, 0.0, 400.0, "Phase 9: Mobile App", false, 1.0);
        assert!(bar.inside);
        assert_eq!(bar.span, (0.0, 400.0), "an inside name needs no extra room");
    }

    #[test]
    fn a_name_too_long_for_its_bar_moves_beside_it() {
        // Rather than elide the name down to nothing on a narrow bar.
        let bar = measure_band(0, 0.0, 20.0, "Phase 2: Core Infrastructure", false, 1.0);
        assert!(!bar.inside);
        assert!(bar.span.1 > 20.0);
        assert_eq!(bar.label, "Phase 2: Core Infrastructure", "nothing is cut");
    }

    #[test]
    fn bands_that_do_not_overlap_share_a_line() {
        let a = measure_band(0, 0.0, 100.0, "A", false, 1.0);
        let b = measure_band(1, 200.0, 300.0, "B", false, 1.0);
        let (lanes, used) = pack_lanes(&[a.span, b.span]);
        assert_eq!(used, 1);
        assert_eq!(lanes[0], lanes[1]);
    }

    #[test]
    fn the_timeline_leaves_no_gap_between_back_to_back_tasks() {
        let calendar = WorkCalendar::standard();
        let first_day = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap();
        let total_days = 19.0;

        // One task ends Wednesday 17:00, the next starts Thursday 08:00.
        let ends = timeline_fraction(&calendar, first_day, total_days, at(2026, 9, 2, 17));
        let starts = timeline_fraction(&calendar, first_day, total_days, at(2026, 9, 3, 8));

        assert_eq!(ends, starts, "the overnight break must not show as a gap");
    }

    #[test]
    fn the_timeline_still_shows_a_weekend() {
        let calendar = WorkCalendar::standard();
        let first_day = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap();
        let total_days = 19.0;
        // One day column, as a fraction of the whole strip.
        let day = 1.0 / total_days;

        let friday = timeline_fraction(&calendar, first_day, total_days, at(2026, 8, 21, 17));
        let monday = timeline_fraction(&calendar, first_day, total_days, at(2026, 8, 24, 8));

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

#[cfg(test)]
mod body_boxes_tests {
    use super::*;

    /// This file, so the shape of what it draws can be asserted on.
    ///
    /// What is being guarded here does not show up in a value anywhere: it is
    /// which elements the chart asks for. An inline drawing costs the whole
    /// chart its handlers and its styling on this renderer, quietly and with
    /// no error, and the last time it was reintroduced it took a day to find.
    const SOURCE: &str = include_str!("gantt.rs");

    /// The chart, from the component down to the last of its helpers. The
    /// timeline strip below is a separate thing and is not in scope here.
    fn chart_source() -> &'static str {
        let from = SOURCE.find("pub fn GanttChart(").expect("the chart component");
        let to = SOURCE[from..]
            .find("// ---------------------------------------------------------------- timeline")
            .expect("the end of the chart");
        &SOURCE[from..from + to]
    }

    #[test]
    fn the_chart_draws_no_inline_pictures() {
        // An inline drawing is serialised, parsed and kept as a picture here,
        // and its children never get layout boxes. Nothing inside one can be
        // styled by the sheet and, worse, nothing inside one can be hit
        // tested, so every handler in it is dead without saying so.
        // Read as elements rather than as text: `show_baseline {` ends in the
        // name of one and is not one, so what counts is a line that opens with
        // the tag and nothing else.
        for tag in ["svg", "rect", "line", "path", "polygon", "ellipse", "text", "g"] {
            let opens = format!("{tag} {{");
            let drawn = chart_source()
                .lines()
                .find(|line| line.trim_start().starts_with(&opens));
            assert!(
                drawn.is_none(),
                "the chart still asks for `{tag}`, which puts it back inside a \
                 picture: {}",
                drawn.unwrap_or("").trim()
            );
        }
    }

    #[test]
    fn the_tooltip_is_an_attribute_and_never_an_element() {
        // A `title` element is the document's title on this renderer, and the
        // document's title is the window's, so a `title` in the chart would
        // rename the window to whatever bar the pointer was over.
        assert!(!chart_source().contains("title {"));
        assert!(chart_source().contains("title: \"{tip}\""));
    }

    #[test]
    fn nothing_the_pane_has_to_clip_is_moved_by_a_transform() {
        // The pane is a clipped box holding something much larger than itself,
        // and a transformed box is painted outside its parent's clip here. The
        // milestone diamond is the shape that invites one, so it is cut out of
        // an upright box instead. The same rule is asserted on the stylesheet
        // in `crate::theme`.
        assert!(
            !chart_source().contains("transform:"),
            "something in the chart is moved by a transform, which takes it \
             outside the clip that is supposed to be cutting it off"
        );
        assert!(chart_source().contains("clip-path: {clip}"));
    }

    #[test]
    fn an_elbow_leg_sits_where_its_stroke_sat() {
        // A stroke straddles the line it is drawn on and a box does not, so a
        // leg is pulled back half its own thickness. Getting this wrong moves
        // every dependency arrow half a pixel off the bar it points at.
        let legs = elbow_runs(&[(10.0, 20.0), (40.0, 20.0), (40.0, 60.0)]);
        assert_eq!(legs.len(), 2);
        assert_eq!((legs[0].x, legs[0].y, legs[0].h), (10.0, 19.5, 1.0));
        assert_eq!((legs[1].x, legs[1].w), (39.5, 1.0));
        // The free ends stop where the polyline did, and the shared corner is
        // grown into from both sides so the join leaves no notch.
        assert_eq!(legs[0].x + legs[0].w, 40.5);
        assert_eq!(legs[1].y, 19.5);
        assert_eq!(legs[1].y + legs[1].h, 60.0);
    }

    #[test]
    fn an_arrow_still_ends_where_it_used_to() {
        // The route is untouched; only what paints it changed. A finish to
        // start link between two bars clear of each other elbows through one
        // corner and arrives pointing right.
        let mut boxes = BarBoxes::with_room_for(3);
        boxes.set(1, BarBox { left: 0.0, right: 40.0, mid: 11.0 });
        boxes.set(2, BarBox { left: 90.0, right: 130.0, mid: 33.0 });
        let (legs, head_x, head_y, downward) =
            arrow_path(&boxes, 1, 2, LinkType::FS).expect("both bars are drawn");
        assert_eq!((head_x, head_y, downward), (90.0, 33.0, false));
        assert_eq!(legs.len(), 3);
        // Out of the predecessor, down between the rows, into the successor.
        assert_eq!(legs[0].y, 10.5);
        assert_eq!(legs[1].x, 82.5);
        assert_eq!(legs[2].y, 32.5);
    }

    #[test]
    fn a_leaning_shape_is_cut_out_of_an_upright_box() {
        // The corners come back as a clip path measured from the box's own top
        // left, because that is the only origin a clip path has.
        let corners = stroke_corners(0.0, 0.0, 10.0, 10.0, 2.0).expect("a real line");
        let (at, clip) = clipped_box(&corners).expect("an area to paint");
        assert!(at.x < 0.0 && at.y < 0.0, "the stroke stands off the line both ways");
        assert!(clip.starts_with("polygon("), "{clip}");
        assert_eq!(clip.matches("px").count(), 8, "four corners, two lengths each");
        // Nothing is left over: the box is exactly what the corners span.
        assert!((at.w - (10.0 + 2.0f64.sqrt())).abs() < 0.01);
    }

    #[test]
    fn a_line_that_does_not_lean_needs_one_pointer_target() {
        // The staircase exists for diagonals. A horizontal or vertical line is
        // its own bounding box, so giving it more boxes would be waste.
        assert_eq!(line_targets(0.0, 0.0, 300.0, 0.0).len(), 1);
        assert_eq!(line_targets(0.0, 0.0, 0.0, 300.0).len(), 1);
        // A diagonal gets a chain, and a bounded one.
        let steps = line_targets(0.0, 0.0, 400.0, 400.0);
        assert!(steps.len() > 1 && steps.len() <= LINE_GRAB_STEPS);
        // Every one of them is on the line rather than beside it.
        for target in &steps {
            let (cx, cy) = (target.x + target.w / 2.0, target.y + target.h / 2.0);
            assert!((cx - cy).abs() < 0.01, "the chain has wandered off the line");
        }
    }

    #[test]
    fn a_dash_pattern_becomes_a_gradient_along_the_line() {
        // A background is the only part of a box that can be broken up, and it
        // has to be broken up along the line rather than down the page.
        assert!(dashed_paint(10.0, 0.0, "red", "6 3").starts_with("repeating-linear-gradient(90.00deg"));
        assert!(dashed_paint(0.0, 10.0, "red", "6 3").starts_with("repeating-linear-gradient(180.00deg"));
        assert_eq!(
            dashed_paint(0.0, 10.0, "red", "6 3"),
            "repeating-linear-gradient(180.00deg, red 0 6px, transparent 6px 9px)"
        );
    }
}
