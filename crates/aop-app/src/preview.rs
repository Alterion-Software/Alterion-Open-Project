//! A compact read-only Gantt, shared by the template thumbnails, the template
//! preview dialog, the task properties panel and the print sheet.
//!
//! Built as one string and handed over whole, rather than as a tree of `rsx!`
//! nodes inside an `<svg>`.
//!
//! The webview-free renderer serialises an inline drawing, hands the markup to
//! a parser and keeps the answer as a picture, and the nodes its children
//! would have had are thrown away with them. Dioxus does not know that: it
//! goes on holding an id for every one of those children, and the next time it
//! patches one it asks the document for a node the document dropped. That is
//! `invalid key`, out of `blitz-dom`'s mutator, and it takes the process with
//! it. The gallery pages are nothing but these drawings, which is why moving
//! between Home and New was the way to find it.
//!
//! One element with a string inside it has no children for dioxus to hold, so
//! there is nothing to go stale. It is the same conclusion the chart came to
//! in `gantt`, reached by a cheaper route: the chart needed to be hit tested
//! and had to become boxes, and a thumbnail is only ever looked at.

use chrono::Duration;
use dioxus::prelude::*;

use aop_core::Project;

/// Colours for a preview drawn on a dark surface.
pub struct MiniPalette {
    pub bar: &'static str,
    pub critical: &'static str,
    pub summary: &'static str,
    pub milestone: &'static str,
    pub rule: &'static str,
    pub ground: &'static str,
}

pub const DARK: MiniPalette = MiniPalette {
    bar: "#3f7d7d",
    critical: "#9d474d",
    summary: "#cfe3e3",
    milestone: "#a5d3d3",
    rule: "rgba(216,231,232,0.07)",
    ground: "none",
};

/// Draw the plan as a small chart. `row_limit` caps how many rows are shown so
/// a long plan still fits the space available.
pub fn mini_gantt_markup(
    project: &Project,
    width: f64,
    height: f64,
    row_limit: usize,
    palette: &MiniPalette,
    show_critical: bool,
) -> String {
    markup(project, width, height, row_limit, palette, show_critical)
}

pub fn mini_gantt(
    project: &Project,
    width: f64,
    height: f64,
    row_limit: usize,
    palette: &MiniPalette,
    show_critical: bool,
) -> Element {
    let drawing = markup(project, width, height, row_limit, palette, show_critical);
    rsx! { div { class: "mini-gantt", dangerous_inner_html: "{drawing}" } }
}

/// The same drawing as markup, for the callers that want to keep it.
fn markup(
    project: &Project,
    width: f64,
    height: f64,
    row_limit: usize,
    palette: &MiniPalette,
    show_critical: bool,
) -> String {
    let rows: Vec<usize> = (0..project.tasks.len()).take(row_limit).collect();
    if rows.is_empty() {
        let mut out = open_svg(width, height, palette);
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" \
             style=\"font-size: 11px; fill: #5f7676;\">Empty plan</text></svg>",
            width / 2.0,
            height / 2.0,
        ));
        return out;
    }

    let pad_x = 10.0;
    let pad_y = 8.0;
    let row_h = ((height - pad_y * 2.0) / rows.len() as f64).clamp(2.5, 11.0);
    let bar_h = (row_h * 0.62).clamp(2.0, 7.0);

    let start = project
        .tasks
        .iter()
        .map(|t| t.scheduled.start)
        .min()
        .unwrap_or(project.start_date);
    let finish = project
        .tasks
        .iter()
        .map(|t| t.scheduled.finish)
        .max()
        .unwrap_or(start + Duration::days(1))
        .max(start + Duration::days(1));
    let span = (finish - start).num_minutes().max(1) as f64;
    let usable = width - pad_x * 2.0;
    let x = |at: chrono::NaiveDateTime| pad_x + (at - start).num_minutes() as f64 / span * usable;

    let mut out = open_svg(width, height, palette);
    for (line, &index) in rows.iter().enumerate() {
        let task = &project.tasks[index];
        let summary = project.is_summary(index);
        let y = pad_y + line as f64 * row_h;
        let centre = y + row_h / 2.0;
        let left = x(task.scheduled.start);
        let right = x(task.scheduled.finish);
        let w = (right - left).max(2.0);
        let critical = show_critical && aop_core::issues::shows_as_critical(project, index);
        let indent = (task.outline_level as f64 * 3.0).min(9.0);
        let fill = if summary {
            palette.summary
        } else if critical {
            palette.critical
        } else {
            palette.bar
        };
        let done = w * task.percent_complete as f64 / 100.0;

        out.push_str(&format!(
            "<line x1=\"0\" y1=\"{y}\" x2=\"{width}\" y2=\"{y}\" \
             stroke=\"{}\" stroke-width=\"1\"/>",
            palette.rule,
        ));

        if project.is_marker(index) {
            let s = (bar_h * 0.9).max(2.0);
            out.push_str(&format!(
                "<polygon points=\"{left},{} {},{centre} {left},{} {},{centre}\" fill=\"{}\"/>",
                centre - s,
                left + s,
                centre + s,
                left - s,
                palette.milestone,
            ));
        } else if summary {
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\"/>",
                left + indent,
                centre - bar_h * 0.3,
                (w - indent).max(2.0),
                bar_h * 0.6,
            ));
        } else {
            out.push_str(&format!(
                "<rect x=\"{left}\" y=\"{}\" width=\"{w}\" height=\"{bar_h}\" \
                 rx=\"1\" fill=\"{fill}\"/>",
                centre - bar_h / 2.0,
            ));
            if done > 0.6 {
                out.push_str(&format!(
                    "<rect x=\"{left}\" y=\"{}\" width=\"{done}\" height=\"{}\" \
                     fill=\"{}\"/>",
                    centre - bar_h / 4.0,
                    bar_h / 2.0,
                    palette.milestone,
                ));
            }
        }
    }
    out.push_str("</svg>");
    out
}

/// The opening tag, and the ground under it if the palette asks for one.
fn open_svg(width: f64, height: f64, palette: &MiniPalette) -> String {
    let mut out = format!(
        "<svg width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">"
    );
    if palette.ground != "none" {
        out.push_str(&format!(
            "<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" fill=\"{}\"/>",
            palette.ground,
        ));
    }
    out
}
