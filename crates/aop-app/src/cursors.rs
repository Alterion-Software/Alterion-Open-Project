//! Other people's pointers, drawn over the table and the chart.
//!
//! Positions travel in plan coordinates and never in pixels. Everybody has a
//! different window, a different zoom and a different scroll offset, so a pixel
//! position drawn on somebody else's screen points at the wrong thing. What
//! goes on the wire is what the pointer is *over*: a row and a column in the
//! table, a row and a distance along the timescale in the chart.
//!
//! ```text
//!   mine                     the wire                    theirs
//!   ----                     --------                    ------
//!   pointer over a cell  ->  Table { row, column }   ->  their column widths
//!   pointer over canvas  ->  Chart { row, minutes }  ->  their zoom and scale
//! ```
//!
//! **Where the overlay lives, and why that is the whole trick.** Each overlay
//! is an absolutely positioned child of the pane it belongs to, written in that
//! pane's own content coordinates. Panes scroll their content, so the browser
//! moves these with everything else: nothing here reads a scroll offset, and
//! nothing has to be told when one changes.
//!
//! **A pointer on a row that is not on screen is not drawn.** Two ways that
//! happens, and both end the same way. A row scrolled out of the pane is
//! clipped by the pane, because the overlay is inside it. A row this copy is
//! not showing at all, because it is collapsed under a summary or filtered
//! out, has no line to be drawn on and is skipped. Neither invents an edge
//! marker: an arrow pinned to the top of the pane would say somebody is at the
//! top of the pane, and they are not.
//!
//! Nothing here takes a click. The overlay is `pointer-events: none` all the
//! way down, so a pointer sitting over a cell never stops that cell being
//! edited.

use dioxus::prelude::*;

use aop_core::grouping::GroupRow;

use crate::cloud::live::{Peer, Pointer};
use crate::cloud::{Account, oauth};
use crate::gantt::{HEADER_H, ROW_H};
use crate::state::AppState;

/// How far from the pointer's tip the label sits.
///
/// Down and to the right, so the label never covers what the pointer is
/// pointing at, which is the only reason the pointer is worth drawing.
const LABEL_OFFSET: (f64, f64) = (13.0, 15.0);

/// A colour for a subject, the same one everywhere and every time.
///
/// Derived rather than assigned, because assigning would mean agreeing: a
/// server handing out colours would have to keep them, reissue them on a
/// reconnect and reconcile two clients that joined at once. A hash of the
/// subject needs none of that and gives the same answer on every machine.
///
/// FNV-1a rather than the standard library's hasher, and that is not a detail.
/// `DefaultHasher` is seeded differently in every process, so the same person
/// would be a different colour in every copy of the application, which is the
/// one thing this must not be.
fn hue_for(subject: &str) -> u16 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in subject.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % 360) as u16
}

/// The colour a peer is drawn in.
///
/// Fixed saturation and lightness so every peer is equally readable and no
/// hash lands on something that disappears into either theme.
pub fn colour_for(subject: &str) -> String {
    format!("hsl({} 62% 58%)", hue_for(subject))
}

/// Which line of the current layout a plan row falls on.
///
/// `None` when this copy is not showing that row: collapsed under a summary,
/// or filtered out. Bands take a line of their own, so a line number is not a
/// task index and the two must not be used for each other.
fn line_of(rows: &[GroupRow], row: i64) -> Option<usize> {
    let row = usize::try_from(row).ok()?;
    rows.iter()
        .position(|line| matches!(line, GroupRow::Task(index) if *index == row))
}

/// Where a peer's pointer lands in the table, in the pane's own coordinates.
fn in_table(state: &AppState, rows: &[GroupRow], at: Pointer) -> Option<(f64, f64)> {
    let Pointer::Table { row, column } = at else {
        return None;
    };
    let line = line_of(rows, row)?;
    // Their column, measured with this copy's widths. The two need not agree,
    // which is exactly why the column travels as a number and not as an x.
    let column = usize::from(column).min(state.columns.len().saturating_sub(1));
    let x: f64 = state.columns.iter().take(column).map(|c| c.width).sum();
    Some((x + 7.0, HEADER_H + line as f64 * ROW_H + ROW_H / 2.0))
}

/// Where a peer's pointer lands on the chart, in the pane's own coordinates.
fn in_chart(state: &AppState, rows: &[GroupRow], at: Pointer) -> Option<(f64, f64)> {
    let Pointer::Chart { row, minutes } = at else {
        return None;
    };
    let line = line_of(rows, row)?;
    // The scale comes from the plan, so two copies of the same plan work the
    // same origin out and the minutes mean the same thing on both.
    let x = minutes as f64 / 1440.0 * state.chart_scale().px_per_day;
    Some((x, HEADER_H + line as f64 * ROW_H + ROW_H / 2.0))
}

/// One peer and where their pointer lands, in a pane's own coordinates.
type Placed = (Peer, f64, f64);

/// How one pane turns a plan position into its own coordinates.
type Placing = fn(&AppState, &[GroupRow], Pointer) -> Option<(f64, f64)>;

/// The peers worth drawing in one pane, already placed.
fn placed(state: &AppState, which: Placing) -> Vec<Placed> {
    if state.live.is_none() {
        return Vec::new();
    }
    let rows = state.layout_rows();
    state
        .peers
        .iter()
        .filter_map(|peer| {
            let at = peer.at?;
            let (x, y) = which(state, &rows, at)?;
            Some((peer.clone(), x, y))
        })
        .collect()
}

/// Other people's pointers over the task table.
#[component]
pub fn TableCursors() -> Element {
    let state = use_context::<Signal<AppState>>();
    let people = placed(&state.read(), in_table);
    rsx! { Overlay { people } }
}

/// Other people's pointers over the Gantt chart.
#[component]
pub fn ChartCursors() -> Element {
    let state = use_context::<Signal<AppState>>();
    let people = placed(&state.read(), in_chart);
    rsx! { Overlay { people } }
}

/// The pointers themselves, wherever they were worked out.
#[component]
fn Overlay(people: Vec<Placed>) -> Element {
    // Nothing at all rather than an empty box, which is the ordinary case:
    // most of the time there is no live session and nobody to draw.
    if people.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "cursors",
            for (peer, x, y) in people {
                {
                    let colour = colour_for(&peer.subject);
                    // No picture travels with a presence, so this is initials
                    // today and every day. It goes through `Account` anyway
                    // rather than around it: when a picture does arrive there
                    // is one rule for what an initial and an image mean, and
                    // one gate a URL has to pass.
                    let who = Account {
                        subject: peer.subject.clone(),
                        name: peer.name.clone(),
                        email: String::new(),
                        picture: None,
                    };
                    let (dx, dy) = LABEL_OFFSET;
                    rsx! {
                        div { key: "{peer.subject}", class: "cursor", style: "left: {x}px; top: {y}px;",
                            svg {
                                class: "cursor-arrow",
                                view_box: "0 0 12 18", width: "12", height: "18",
                                // The outline is what makes an arrow readable
                                // over a dark bar and a light cell alike,
                                // whatever colour the hash picked.
                                path {
                                    d: "M1 1 L1 15 L4.6 11.6 L7 17 L9.6 15.8 L7.2 10.7 L11.5 10.4 Z",
                                    fill: "{colour}",
                                    stroke: "var(--surface)",
                                    stroke_width: "1.2",
                                    stroke_linejoin: "round",
                                }
                            }
                            div {
                                class: "cursor-label",
                                style: "left: {dx}px; top: {dy}px; background: {colour};",
                                Avatar { who: who.clone(), colour: colour.clone() }
                                span { class: "cursor-name", "{name_of(&who)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// What to call somebody whose copy never said.
fn name_of(who: &Account) -> String {
    match who.name.trim() {
        "" => "Someone".to_string(),
        name => name.to_string(),
    }
}

/// A peer's face, or the letters that stand in for one.
#[component]
fn Avatar(who: Account, colour: String) -> Element {
    // One rule for whether a URL may be handed to the webview, and it is the
    // same one the sign in uses. A picture address is a string the provider
    // chose, and this is the check that stops it being a plain HTTP fetch or
    // something that is not a fetch at all.
    if let Some(picture) = who.picture.as_deref().filter(|url| oauth::transport_is_safe(url)) {
        return rsx! {
            img { class: "cursor-face", src: "{picture}", alt: "" }
        };
    }
    rsx! {
        span { class: "cursor-face initials", style: "color: {colour};", "{who.initials()}" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_person_is_the_same_colour_everywhere_and_every_run() {
        // The point of hashing rather than assigning. A process seeded hasher
        // would give a different answer in every copy, which would make the
        // colour worse than useless: it would look like it meant something.
        assert_eq!(hue_for("0198f0c2-1111-4222-8333-444455556666"), 247);
        assert_eq!(colour_for("0198f0c2-1111-4222-8333-444455556666"), "hsl(247 62% 58%)");
        assert_eq!(hue_for(""), 77);
    }

    #[test]
    fn different_people_are_told_apart() {
        let one = colour_for("0198f0c2-aaaa-4222-8333-444455556666");
        let two = colour_for("0198f0c2-bbbb-4222-8333-444455556666");
        assert_ne!(one, two);
    }

    #[test]
    fn a_row_this_copy_is_not_showing_has_no_line_to_be_drawn_on() {
        // Collapsed, filtered or grouped away: the answer is nothing, not a
        // guess. Drawing it at a line that belongs to a different task would
        // say somebody is somewhere they are not.
        let rows = vec![
            GroupRow::Band {
                label: "Phase 1".into(),
                count: 2,
                work_minutes: 0,
                cost: 0.0,
                depth: 0,
            },
            GroupRow::Task(4),
            GroupRow::Task(9),
        ];
        assert_eq!(line_of(&rows, 4), Some(1));
        assert_eq!(line_of(&rows, 9), Some(2));
        assert_eq!(line_of(&rows, 5), None);
        assert_eq!(line_of(&rows, -1), None);
    }

    #[test]
    fn a_band_never_stands_in_for_a_task() {
        // Bands take a line each in both panes, so a line number is not a task
        // index and one must never be used as the other.
        let rows = vec![
            GroupRow::Band {
                label: "Phase 1".into(),
                count: 1,
                work_minutes: 0,
                cost: 0.0,
                depth: 0,
            },
            GroupRow::Task(0),
        ];
        assert_eq!(line_of(&rows, 0), Some(1));
    }
}
