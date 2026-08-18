//! Reading and writing `.aprj` files, the Alterion Open Project format.
//!
//! The container is JSON with a format tag and a version number so older files
//! stay loadable as the model grows.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use chrono::Datelike;

use crate::duration::{format_duration, format_work};
use crate::model::Project;

/// The file extension the Save As dialog defaults to.
pub const FILE_EXTENSION: &str = "aprj";
pub const FILE_TYPE_NAME: &str = "Alterion Project";

/// Every `.aprj` starts with these four bytes, so the type is identifiable
/// without looking at the name.
const MAGIC: &[u8; 4] = b"APRJ";
/// Container revision. Bumped when the header layout changes, not when the
/// model gains a field, which MessagePack tolerates on its own.
const CONTAINER_VERSION: u16 = 2;
/// Header flag: the payload is deflate compressed.
const FLAG_DEFLATE: u16 = 1 << 0;
/// Header is magic(4) + version(2) + flags(2) + raw length(4).
const HEADER_LEN: usize = 12;
/// Refuse absurd allocations from a corrupt or hostile header.
const MAX_PAYLOAD: u32 = 256 * 1024 * 1024;

/// The legacy JSON container, still readable so older files keep opening.
const LEGACY_TAG: &str = "alterion-open-project";

#[derive(Debug, Serialize, Deserialize)]
struct LegacyFile {
    format: String,
    version: u32,
    project: Project,
}

#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    /// The bytes are not a project file at all.
    NotAProject,
    /// Written by a newer build than this one.
    TooNew(u16),
    /// The container is intact but the contents could not be decoded.
    Corrupt(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "{e}"),
            FileError::NotAProject => write!(
                f,
                "This is not an Alterion Project file. Expected a .{FILE_EXTENSION} file starting with APRJ."
            ),
            FileError::TooNew(v) => write!(
                f,
                "This file was saved by a newer version of Alterion Open Project (container {v})."
            ),
            FileError::Corrupt(detail) => write!(f, "The file could not be read: {detail}"),
        }
    }
}

impl std::error::Error for FileError {}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::Io(e)
    }
}

/// Force the `.aprj` extension onto whatever the user typed.
pub fn with_extension(path: &Path) -> PathBuf {
    match path.extension() {
        Some(ext) if ext.eq_ignore_ascii_case(FILE_EXTENSION) => path.to_path_buf(),
        _ => path.with_extension(FILE_EXTENSION),
    }
}

/// Encode a plan into the binary container.
///
/// The payload is MessagePack with field names kept, so a file written by an
/// older build still loads once the model gains fields, then deflated.
pub fn to_bytes(project: &Project) -> Result<Vec<u8>, FileError> {
    let packed = rmp_serde::to_vec_named(project)
        .map_err(|e| FileError::Corrupt(format!("could not encode the plan: {e}")))?;
    let raw_len = packed.len() as u32;

    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &packed)?;
    let compressed = encoder.finish()?;

    // Only keep the compression if it actually paid for itself.
    let (flags, payload) = if compressed.len() < packed.len() {
        (FLAG_DEFLATE, compressed)
    } else {
        (0, packed)
    };

    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&CONTAINER_VERSION.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&raw_len.to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode a plan. Accepts the binary container and, for older saves, the
/// original JSON one.
pub fn from_bytes(bytes: &[u8]) -> Result<Project, FileError> {
    if bytes.len() >= HEADER_LEN && &bytes[..4] == MAGIC {
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version > CONTAINER_VERSION {
            return Err(FileError::TooNew(version));
        }
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        let raw_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        if raw_len > MAX_PAYLOAD {
            return Err(FileError::Corrupt(format!(
                "declared payload of {raw_len} bytes is implausible"
            )));
        }
        let body = &bytes[HEADER_LEN..];

        let packed = if flags & FLAG_DEFLATE != 0 {
            let mut out = Vec::with_capacity(raw_len as usize);
            let mut decoder = flate2::read::DeflateDecoder::new(body).take(MAX_PAYLOAD as u64 + 1);
            std::io::Read::read_to_end(&mut decoder, &mut out)
                .map_err(|e| FileError::Corrupt(format!("could not decompress: {e}")))?;
            out
        } else {
            body.to_vec()
        };

        return rmp_serde::from_slice(&packed)
            .map_err(|e| FileError::Corrupt(format!("could not decode the plan: {e}")));
    }

    // Anything starting with a brace is the JSON container this format replaced.
    if bytes.first().is_some_and(|b| *b == b'{') {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| FileError::Corrupt(format!("not valid UTF-8: {e}")))?;
        let file: LegacyFile = serde_json::from_str(text)
            .map_err(|e| FileError::Corrupt(format!("invalid JSON: {e}")))?;
        if file.format != LEGACY_TAG {
            return Err(FileError::NotAProject);
        }
        return Ok(file.project);
    }

    Err(FileError::NotAProject)
}

pub fn save(path: &Path, project: &Project) -> Result<PathBuf, FileError> {
    let path = with_extension(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, to_bytes(project)?)?;
    Ok(path)
}

pub fn open(path: &Path) -> Result<Project, FileError> {
    from_bytes(&std::fs::read(path)?)
}

/// Which readers can produce a plan, and the extensions that pick them.
///
/// Anything the application will open belongs here, so that everything which
/// has to open a file (the Open page, a recent entry, a preview card, the
/// command line) agrees on what is supported instead of each deciding for
/// itself. A preview card that quietly drops a format the Open page accepts is
/// the shape of bug this exists to prevent.
pub const IMPORTED_EXTENSIONS: [&str; 7] = ["xml", "mpp", "mspdi", "xlsx", "xlsm", "xls", "ods"];

/// Open any plan the application understands, whatever wrote it.
pub fn open_any(path: &Path) -> Result<Project, String> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let mut project = if matches!(extension.as_str(), "xlsx" | "xlsm" | "xls" | "ods") {
        crate::excel::open(path).map_err(|e| e.to_string())?
    } else if IMPORTED_EXTENSIONS.contains(&extension.as_str()) {
        crate::mspdi::open(path).map_err(|e| e.to_string())?
    } else {
        open(path).map_err(|e| e.to_string())?
    };

    // Everything derived comes from the scheduler: dates, slack, the critical
    // path, rolled up summary durations, cost. A plan handed back before that
    // has run carries whatever the file happened to hold, which for an import
    // is mostly nothing, so anything reading it straight away sees zeros. The
    // window reschedules after opening and so looked right; an export did not
    // and wrote a duration of zero for every task.
    //
    // A plan that will not schedule is still returned: the caller shows the
    // error and lets the planner fix it, which they cannot do if opening fails.
    let _ = crate::schedule(&mut project);
    Ok(project)
}

/// Export the task table as CSV, for the Export command.
pub fn to_csv(project: &Project) -> String {
    fn quote(value: &str) -> String {
        if value.contains([',', '"', '\n']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    }

    let mut out = String::from(
        "ID,WBS,Name,Duration,Start,Finish,Predecessors,Resources,% Complete,Work,Cost,Critical\n",
    );
    for index in 0..project.tasks.len() {
        let task = &project.tasks[index];
        let indent = "    ".repeat(task.outline_level as usize);
        let fields = [
            (index + 1).to_string(),
            project.wbs(index),
            format!("{indent}{}", task.name),
            format_duration(task.scheduled.duration_minutes),
            task.scheduled.start.format("%Y-%m-%d %H:%M").to_string(),
            task.scheduled.finish.format("%Y-%m-%d %H:%M").to_string(),
            project.predecessor_text(task.id),
            project.resource_text(task),
            format!("{}%", task.percent_complete),
            format_work(task.scheduled.work_minutes),
            format!("{}{:.2}", project.currency_symbol, task.scheduled.cost),
            if task.scheduled.critical { "Yes" } else { "No" }.to_string(),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|f| quote(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

/// Escape text for embedding in HTML.
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The printable body: a title band, headline figures, the chart as inline SVG,
/// a legend and the task table. Used for both the on-screen print preview and
/// the saved file, so the two can never drift apart.
pub fn print_body(project: &Project) -> String {
    let mut out = String::new();

    let critical = (0..project.tasks.len())
        // A count of what the schedule says, not of what the warning list is
        // currently showing.
        .filter(|&i| !project.is_summary(i) && project.tasks[i].scheduled.critical)
        .count();
    let duration = format_duration(
        project
            .calendar
            .work_minutes_between(project.start_date, project.finish_date),
    );

    out.push_str(&format!(
        "<header class=\"sheet-head\">\
           <div class=\"sheet-title\"><h1>{}</h1><p>{} to {}</p></div>\
           <div class=\"sheet-brand\">Alterion Open Project</div>\
         </header>\n",
        escape(&project.name),
        project.start_date.format("%d %B %Y"),
        project.finish_date.format("%d %B %Y"),
    ));

    let figures = [
        ("Duration", duration),
        ("Tasks", project.tasks.len().to_string()),
        ("Critical", critical.to_string()),
        ("Complete", format!("{}%", project.percent_complete())),
        ("Work", format_work(project.total_work_minutes())),
        (
            "Cost",
            format!("{}{:.2}", project.currency_symbol, project.total_cost()),
        ),
    ];
    out.push_str("<section class=\"figures\">");
    for (label, value) in figures {
        out.push_str(&format!(
            "<div class=\"figure\"><span class=\"fig-value\">{}</span><span class=\"fig-label\">{}</span></div>",
            escape(&value),
            label
        ));
    }
    out.push_str("</section>\n");

    let legend = "<section class=\"legend\">\
           <span><i class=\"sw bar\"></i>Task</span>\
           <span><i class=\"sw crit\"></i>Critical</span>\
           <span><i class=\"sw summ\"></i>Summary</span>\
           <span><i class=\"sw ms\"></i>Milestone</span>\
           <span><i class=\"sw prog\"></i>Complete</span>\
         </section>\n";

    // The chart takes as many pages as it needs, each starting a fresh sheet.
    let pages = gantt_pages(project, 1000.0);
    let last = pages.len().saturating_sub(1);
    for (number, page) in pages.into_iter().enumerate() {
        let class = if number == 0 { "chart-page" } else { "chart-page break" };
        out.push_str(&format!("<section class=\"{class}\">"));
        if number > 0 {
            out.push_str(&format!(
                "<h2 class=\"cont\">{} <span>chart, continued</span></h2>",
                escape(&project.name)
            ));
        }
        out.push_str(&page);
        if number == last {
            out.push_str(legend);
        }
        out.push_str("</section>\n");
    }

    // The table starts its own sheet rather than trailing off the chart's.
    out.push_str("<section class=\"table-page break\">\n");
    out.push_str("<h2>Task Table</h2>\n");
    out.push_str("<table>\n<thead><tr>\
        <th class=\"n\">ID</th><th class=\"n\">WBS</th><th>Task Name</th>\
        <th class=\"n\">Duration</th><th>Start</th><th>Finish</th>\
        <th>Predecessors</th><th>Resources</th>\
        <th class=\"n\">%</th><th class=\"n\">Work</th><th class=\"n\">Cost</th>\
        </tr></thead>\n<tbody>\n");

    for index in 0..project.tasks.len() {
        let task = &project.tasks[index];
        let summary = project.is_summary(index);
        let indent = task.outline_level as usize * 13;

        let mut classes = Vec::new();
        if summary {
            classes.push("summary");
        }
        if !summary && crate::issues::shows_as_critical(project, index) {
            classes.push("critical");
        }
        if project.is_marker(index) {
            classes.push("milestone");
        }
        let row_class = if classes.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", classes.join(" "))
        };

        out.push_str(&format!(
            "<tr{row_class}>\
               <td class=\"n\">{}</td><td class=\"n\">{}</td>\
               <td><span style=\"padding-left:{indent}px\">{}</span></td>\
               <td class=\"n\">{}</td><td>{}</td><td>{}</td>\
               <td>{}</td><td>{}</td>\
               <td class=\"n\">{}%</td><td class=\"n\">{}</td><td class=\"n\">{}{:.2}</td>\
             </tr>\n",
            index + 1,
            escape(&project.wbs(index)),
            escape(&task.name),
            format_duration(task.scheduled.duration_minutes),
            task.scheduled.start.format("%d/%m/%y"),
            task.scheduled.finish.format("%d/%m/%y"),
            escape(&project.predecessor_text(task.id)),
            escape(&project.resource_text(task)),
            task.percent_complete,
            format_work(task.scheduled.work_minutes),
            project.currency_symbol,
            task.scheduled.cost,
        ));
    }
    out.push_str("</tbody></table>\n");

    if !project.resources.is_empty() {
        out.push_str("<h2>Resources</h2>\n<table>\n<thead><tr>\
            <th class=\"n\">ID</th><th>Resource Name</th><th>Type</th><th>Group</th>\
            <th class=\"n\">Max. Units</th><th class=\"n\">Std. Rate</th></tr></thead>\n<tbody>\n");
        for (index, resource) in project.resources.iter().enumerate() {
            out.push_str(&format!(
                "<tr><td class=\"n\">{}</td><td>{}</td><td>{}</td><td>{}</td>\
                 <td class=\"n\">{:.0}%</td><td class=\"n\">{}{:.2}/hr</td></tr>\n",
                index + 1,
                escape(&resource.name),
                resource.kind.label(),
                escape(&resource.group),
                resource.max_units * 100.0,
                project.currency_symbol,
                resource.standard_rate,
            ));
        }
        out.push_str("</tbody></table>\n");
    }
    out.push_str("</section>\n");

    out
}

/// How many chart rows fit on one printed page.
///
/// A4 landscape less its margins is about 186mm of height, and a row is 13px,
/// which leaves room for the timescale and a little air.
const CHART_ROWS_PER_PAGE: usize = 46;

/// The plan as inline SVG, broken into one block per printed page.
///
/// Every page repeats the timescale and shares the same one, so a bar in March
/// sits under March on whichever page it lands. Pages exist because a plan of
/// a few hundred tasks is taller than any sheet of paper, and a single tall
/// SVG does not split across pages, it gets clipped.
fn gantt_pages(project: &Project, width: f64) -> Vec<String> {
    if project.tasks.is_empty() {
        return Vec::new();
    }
    (0..project.tasks.len())
        .collect::<Vec<_>>()
        .chunks(CHART_ROWS_PER_PAGE)
        .map(|chunk| gantt_svg(project, width, chunk))
        .collect()
}

/// One page of the chart: `rows` drawn against the whole plan's timescale.
fn gantt_svg(project: &Project, width: f64, rows: &[usize]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let row_h = 13.0f64;
    let label_w = 210.0f64;
    let height = rows.len() as f64 * row_h + 26.0;
    let chart_w = width - label_w - 12.0;

    // The timescale spans every task in the plan, not just this page's rows,
    // so the pages line up with each other when they are laid side by side.
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
        .unwrap_or(project.finish_date);
    let span = (finish - start).num_minutes().max(1) as f64;
    let at = |value: chrono::NaiveDateTime| {
        label_w + (value - start).num_minutes() as f64 / span * chart_w
    };

    let mut out = format!(
        "<svg class=\"chart\" viewBox=\"0 0 {width} {height}\" width=\"100%\" height=\"{height}\">"
    );

    // Month ticks along the top give the bars something to read against.
    let mut tick = chrono::NaiveDate::from_ymd_opt(start.year(), start.month(), 1)
        .unwrap_or(start.date());
    while tick <= finish.date() {
        let x = at(tick.and_hms_opt(0, 0, 0).unwrap_or(start));
        if x >= label_w {
            out.push_str(&format!(
                "<line x1=\"{x}\" y1=\"14\" x2=\"{x}\" y2=\"{height}\" class=\"tick\"/>\
                 <text x=\"{}\" y=\"10\" class=\"tickly\">{}</text>",
                x + 3.0,
                tick.format("%b %y")
            ));
        }
        tick = if tick.month() == 12 {
            chrono::NaiveDate::from_ymd_opt(tick.year() + 1, 1, 1)
        } else {
            chrono::NaiveDate::from_ymd_opt(tick.year(), tick.month() + 1, 1)
        }
        .unwrap_or(finish.date() + chrono::Duration::days(1));
    }

    for (line, &index) in rows.iter().enumerate() {
        let task = &project.tasks[index];
        let y = 20.0 + line as f64 * row_h;
        let left = at(task.scheduled.start);
        let right = at(task.scheduled.finish);
        let w = (right - left).max(1.5);
        let summary = project.is_summary(index);
        let indent = 6.0 + task.outline_level as f64 * 9.0;

        let name: String = task.name.chars().take(34).collect();
        out.push_str(&format!(
            "<text x=\"{indent}\" y=\"{}\" class=\"rowlbl{}\">{}</text>",
            y + 7.0,
            if summary { " b" } else { "" },
            escape(&name)
        ));

        if project.is_marker(index) {
            out.push_str(&format!(
                "<polygon points=\"{left},{} {},{} {left},{} {},{}\" class=\"ms\"/>",
                y + 1.5,
                left + 4.0,
                y + 5.5,
                y + 9.5,
                left - 4.0,
                y + 5.5
            ));
        } else if summary {
            out.push_str(&format!(
                "<rect x=\"{left}\" y=\"{}\" width=\"{w}\" height=\"3.5\" class=\"summ\"/>",
                y + 3.5
            ));
        } else {
            let class = if crate::issues::shows_as_critical(project, index) {
                "crit"
            } else {
                "bar"
            };
            out.push_str(&format!(
                "<rect x=\"{left}\" y=\"{}\" width=\"{w}\" height=\"7\" rx=\"1.5\" class=\"{class}\"/>",
                y + 1.5
            ));
            let done = w * task.percent_complete as f64 / 100.0;
            if done > 0.8 {
                out.push_str(&format!(
                    "<rect x=\"{left}\" y=\"{}\" width=\"{done}\" height=\"2.5\" class=\"prog\"/>",
                    y + 3.75
                ));
            }
        }
    }

    out.push_str("</svg>");
    out
}

/// A standalone, print-ready document.
pub fn to_print_html(project: &Project) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{}</title>\n<style>\n{}</style></head>\n<body>\n<main class=\"sheet\">\n{}</main>\n\
<footer class=\"sheet-foot\">Generated by Alterion Open Project</footer>\n</body></html>\n",
        escape(&project.name),
        PRINT_CSS,
        print_body(project)
    )
}

/// Styling shared by the saved page and the in-app print preview.
pub const PRINT_CSS: &str = r#"
:root {
  --ink: #10201f; --soft: #55706f; --faint: #8aa3a2;
  --rule: #d3e0e0; --band: #eef4f4; --brand: #2f5f5e;
  --bar: #4b8b8b; --crit: #b3565c; --summ: #20403f; --prog: #d8ecec;
}
* { box-sizing: border-box; }
body {
  margin: 0; padding: 26px;
  font-family: Inter, "Segoe UI", system-ui, sans-serif;
  color: var(--ink); background: #f2f5f5;
  -webkit-font-smoothing: antialiased;
}
.sheet { background: #fff; max-width: 1000px; margin: 0 auto; padding: 30px 32px 34px; box-shadow: 0 2px 14px rgba(16,32,31,0.10); }
.sheet-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 20px; padding-bottom: 16px; border-bottom: 2px solid var(--brand); }
.sheet-title h1 { margin: 0 0 4px; font-size: 21px; font-weight: 650; letter-spacing: -0.3px; }
.sheet-title p { margin: 0; font-size: 11.5px; color: var(--soft); }
.sheet-brand { font-size: 10.5px; letter-spacing: 1.4px; text-transform: uppercase; color: var(--brand); font-weight: 600; white-space: nowrap; padding-top: 4px; }
.figures { display: flex; flex-wrap: wrap; gap: 10px; margin: 18px 0 22px; }
.figure { flex: 1 1 120px; background: var(--band); border-radius: 7px; padding: 11px 13px; display: flex; flex-direction: column; gap: 3px; }
.fig-value { font-size: 16px; font-weight: 650; letter-spacing: -0.2px; }
.fig-label { font-size: 9.5px; letter-spacing: 0.9px; text-transform: uppercase; color: var(--soft); }
.chart { display: block; margin: 4px 0 10px; width: 100%; }
/* Each chart page and the table start their own sheet. On screen the same
   rule reads as a divider, so the preview shows where the paper will end. */
.chart-page, .table-page { display: block; }
.break { break-before: page; page-break-before: always; border-top: 1px dashed var(--rule); padding-top: 18px; margin-top: 22px; }
h2.cont { color: var(--soft); font-weight: 600; }
h2.cont span { font-weight: 400; color: var(--faint); }
.chart .bar { fill: var(--bar); } .chart .crit { fill: var(--crit); }
.chart .summ { fill: var(--summ); } .chart .ms { fill: var(--summ); }
.chart .prog { fill: var(--prog); }
.chart .tick { stroke: var(--rule); stroke-width: 1; }
.chart .tickly { font-size: 8px; fill: var(--faint); }
.chart .rowlbl { font-size: 8.5px; fill: var(--soft); }
.chart .rowlbl.b { fill: var(--ink); font-weight: 650; }
.legend { display: flex; gap: 16px; flex-wrap: wrap; margin: 4px 0 20px; font-size: 10px; color: var(--soft); }
.legend span { display: inline-flex; align-items: center; gap: 6px; }
.sw { width: 16px; height: 7px; border-radius: 2px; display: inline-block; }
.sw.bar { background: var(--bar); } .sw.crit { background: var(--crit); }
.sw.summ { background: var(--summ); height: 3px; }
.sw.ms { background: var(--summ); width: 8px; height: 8px; transform: rotate(45deg); border-radius: 1px; }
.sw.prog { background: var(--prog); height: 3px; }
h2 { font-size: 13px; margin: 26px 0 8px; padding-bottom: 5px; border-bottom: 1px solid var(--rule); font-weight: 650; }
table { width: 100%; border-collapse: collapse; font-size: 10px; }
thead th { background: var(--band); color: var(--soft); font-weight: 600; text-align: left; padding: 6px 7px; border-bottom: 1.5px solid var(--rule); font-size: 9.5px; letter-spacing: 0.3px; text-transform: uppercase; white-space: nowrap; }
tbody td { padding: 4px 7px; border-bottom: 1px solid #eaf1f1; vertical-align: top; }
tbody tr:nth-child(even) td { background: #fafcfc; }
tbody tr.summary td { font-weight: 650; background: #f3f8f8; }
tbody tr.critical td:nth-child(3) { color: var(--crit); }
tbody tr.milestone td:nth-child(3) { font-style: italic; }
th.n, td.n { text-align: right; white-space: nowrap; }
th:nth-child(3), td:nth-child(3) { width: 34%; }
.sheet-foot { max-width: 1000px; margin: 12px auto 0; font-size: 9.5px; color: var(--faint); text-align: right; }
@media print {
  body { background: #fff; padding: 0; }
  .sheet { box-shadow: none; max-width: none; padding: 0; }
  /* On paper the sheet edge is the divider, so the on-screen one goes away. */
  .break { border-top: none; padding-top: 0; margin-top: 0; }
  .chart-page, .table-page { break-inside: auto; }
  thead { display: table-header-group; }
  tbody tr { break-inside: avoid; }
  @page { size: A4 landscape; margin: 12mm; }
}
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn a_plan_comes_back_from_open_already_scheduled() {
        // Everything derived hangs off the scheduler. Handing a plan back
        // before it has run left every task with a scheduled duration of
        // zero, which the window hid by rescheduling itself and an export
        // did not.
        let mut project = Project::blank(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap(),
        );
        for name in ["One", "Two"] {
            let id = project.allocate_task_id();
            project.tasks.push(crate::model::Task::new(id, name, 480));
        }
        let dir = std::env::temp_dir().join(format!("aop-open-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("plan.aprj");
        save(&path, &project).expect("written");

        let back = open_any(&path).expect("read");
        for task in &back.tasks {
            assert_eq!(
                task.duration_minutes, task.scheduled.duration_minutes,
                "{} came back unscheduled",
                task.name
            );
        }
        let _ = std::fs::remove_file(&path);
    }


    use super::*;
    use crate::templates;
    use chrono::NaiveDate;

    fn sample() -> Project {
        let start = NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let spec = templates::by_id("simple").unwrap();
        let mut project = templates::build(spec, start);
        crate::schedule::schedule(&mut project).unwrap();
        project
    }

    #[test]
    fn a_project_survives_a_save_and_load_round_trip() {
        let project = sample();
        let bytes = to_bytes(&project).unwrap();
        let restored = from_bytes(&bytes).unwrap();

        assert_eq!(restored.name, project.name);
        assert_eq!(restored.tasks.len(), project.tasks.len());
        assert_eq!(restored.links.len(), project.links.len());
        assert_eq!(restored.resources.len(), project.resources.len());
        assert_eq!(
            restored.tasks[3].scheduled.start,
            project.tasks[3].scheduled.start
        );
    }

    #[test]
    fn the_file_starts_with_its_magic_bytes() {
        let bytes = to_bytes(&sample()).unwrap();
        assert_eq!(&bytes[..4], b"APRJ");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), CONTAINER_VERSION);
    }

    #[test]
    fn the_binary_container_is_smaller_than_the_json_it_replaced() {
        let project = sample();
        let binary = to_bytes(&project).unwrap();
        let json = serde_json::to_vec(&project).unwrap();
        assert!(
            binary.len() < json.len(),
            "binary {} bytes was not smaller than json {} bytes",
            binary.len(),
            json.len()
        );
    }

    #[test]
    fn files_written_by_the_old_json_container_still_open() {
        let project = sample();
        let legacy = serde_json::to_string(&LegacyFile {
            format: LEGACY_TAG.into(),
            version: 1,
            project: project.clone(),
        })
        .unwrap();

        let restored = from_bytes(legacy.as_bytes()).unwrap();
        assert_eq!(restored.tasks.len(), project.tasks.len());
        assert_eq!(restored.links.len(), project.links.len());
    }

    #[test]
    fn a_foreign_file_is_rejected() {
        assert!(matches!(
            from_bytes(b"PK\x03\x04 not a plan").unwrap_err(),
            FileError::NotAProject
        ));
        assert!(matches!(
            from_bytes(b"{\"format\":\"other-tool\",\"version\":1}").unwrap_err(),
            FileError::NotAProject | FileError::Corrupt(_)
        ));
    }

    #[test]
    fn a_file_from_a_newer_build_is_refused_rather_than_misread() {
        let mut bytes = to_bytes(&sample()).unwrap();
        bytes[4] = 99;
        assert!(matches!(from_bytes(&bytes).unwrap_err(), FileError::TooNew(99)));
    }

    #[test]
    fn a_truncated_file_reports_rather_than_panicking() {
        let bytes = to_bytes(&sample()).unwrap();
        let truncated = &bytes[..bytes.len() / 2];
        assert!(from_bytes(truncated).is_err());
    }

    #[test]
    fn an_implausible_header_length_is_refused() {
        let mut bytes = to_bytes(&sample()).unwrap();
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(from_bytes(&bytes).unwrap_err(), FileError::Corrupt(_)));
    }

    #[test]
    fn the_extension_is_forced_on() {
        assert_eq!(
            with_extension(Path::new("/tmp/plan")),
            PathBuf::from("/tmp/plan.aprj")
        );
        assert_eq!(
            with_extension(Path::new("/tmp/plan.aprj")),
            PathBuf::from("/tmp/plan.aprj")
        );
    }

    #[test]
    fn print_html_contains_the_plan() {
        let project = sample();
        let html = to_print_html(&project);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<svg"));
        for task in &project.tasks {
            if !task.name.is_empty() {
                assert!(html.contains(&task.name), "missing {}", task.name);
            }
        }
    }

    #[test]
    fn html_escaping_neutralises_markup_in_task_names() {
        let mut project = sample();
        project.tasks[1].name = "Ship <script>alert(1)</script>".into();
        let html = to_print_html(&project);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn csv_export_has_a_row_per_task() {
        let project = sample();
        let csv = to_csv(&project);
        assert_eq!(csv.lines().count(), project.tasks.len() + 1);
    }
}
