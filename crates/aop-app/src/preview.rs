//! A compact read-only Gantt, shared by the template thumbnails, the template
//! preview dialog, the task properties panel and the print sheet.

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
pub fn mini_gantt(
    project: &Project,
    width: f64,
    height: f64,
    row_limit: usize,
    palette: &MiniPalette,
    show_critical: bool,
) -> Element {
    let rows: Vec<usize> = (0..project.tasks.len()).take(row_limit).collect();
    if rows.is_empty() {
        return rsx! {
            svg { width: "{width}", height: "{height}", view_box: "0 0 {width} {height}",
                if palette.ground != "none" {
                    rect { x: "0", y: "0", width: "{width}", height: "{height}", fill: "{palette.ground}" }
                }
                text {
                    x: "{width / 2.0}", y: "{height / 2.0}",
                    text_anchor: "middle",
                    style: "font-size: 11px; fill: #5f7676;",
                    "Empty plan"
                }
            }
        };
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

    rsx! {
        svg { width: "{width}", height: "{height}", view_box: "0 0 {width} {height}",
            if palette.ground != "none" {
                rect { x: "0", y: "0", width: "{width}", height: "{height}", fill: "{palette.ground}" }
            }
            for (line, &index) in rows.iter().enumerate() {
                {
                    let task = &project.tasks[index];
                    let summary = project.is_summary(index);
                    let y = pad_y + line as f64 * row_h;
                    let centre = y + row_h / 2.0;
                    let left = x(task.scheduled.start);
                    let right = x(task.scheduled.finish);
                    let w = (right - left).max(2.0);
                    let critical =
                        show_critical && aop_core::issues::shows_as_critical(project, index);
                    let indent = (task.outline_level as f64 * 3.0).min(9.0);
                    let fill = if summary {
                        palette.summary
                    } else if critical {
                        palette.critical
                    } else {
                        palette.bar
                    };
                    let done = w * task.percent_complete as f64 / 100.0;

                    rsx! {
                        g { key: "mg{index}",
                            line {
                                x1: "0", y1: "{y}", x2: "{width}", y2: "{y}",
                                stroke: "{palette.rule}", stroke_width: "1",
                            }
                            if project.is_marker(index) {
                                {
                                    let s = (bar_h * 0.9).max(2.0);
                                    let points = format!(
                                        "{left},{} {},{centre} {left},{} {},{centre}",
                                        centre - s, left + s, centre + s, left - s
                                    );
                                    rsx! { polygon { points: "{points}", fill: "{palette.milestone}" } }
                                }
                            } else if summary {
                                rect {
                                    x: "{left + indent}", y: "{centre - bar_h * 0.3}",
                                    width: "{(w - indent).max(2.0)}", height: "{bar_h * 0.6}",
                                    fill: "{fill}",
                                }
                            } else {
                                g {
                                    rect {
                                        x: "{left}", y: "{centre - bar_h / 2.0}",
                                        width: "{w}", height: "{bar_h}",
                                        rx: "1", fill: "{fill}",
                                    }
                                    if done > 0.6 {
                                        rect {
                                            x: "{left}", y: "{centre - bar_h / 4.0}",
                                            width: "{done}", height: "{bar_h / 2.0}",
                                            fill: "{palette.milestone}",
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
