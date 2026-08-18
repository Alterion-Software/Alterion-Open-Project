//! The plan as a PDF, drawn directly rather than through a browser.
//!
//! Printing needs a real document, not a web page and a hope that whatever
//! renders it agrees with us. Everything here is drawn from the same geometry
//! the on-screen chart uses, so what comes out of a printer is what was on the
//! screen rather than a second implementation that drifts from it.
//!
//! Text is set in Helvetica, one of the fourteen typefaces every PDF reader is
//! required to have. That means no font file is embedded and none has to be
//! shipped: a plan printed here opens the same on a machine that has never seen
//! this application.

use chrono::{Datelike, NaiveDate, NaiveDateTime};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};

use crate::model::Project;
use crate::{format_duration, format_work};

/// Which way round the paper goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Landscape,
    Portrait,
}

impl Orientation {
    pub const ORDER: [Orientation; 2] = [Orientation::Landscape, Orientation::Portrait];

    pub fn label(self) -> &'static str {
        match self {
            Orientation::Landscape => "Landscape",
            Orientation::Portrait => "Portrait",
        }
    }
}

/// A paper size, in PostScript points (72 to the inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paper {
    pub name: &'static str,
    pub width: f32,
    pub height: f32,
}

pub const A4: Paper = Paper {
    name: "A4",
    width: 595.28,
    height: 841.89,
};
pub const LETTER: Paper = Paper {
    name: "Letter",
    width: 612.0,
    height: 792.0,
};
pub const A3: Paper = Paper {
    name: "A3",
    width: 841.89,
    height: 1190.55,
};

pub const PAPERS: [Paper; 3] = [A4, LETTER, A3];

/// What to put in the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrintOptions {
    pub paper: Paper,
    pub orientation: Orientation,
    /// Margin on every edge, in millimetres.
    pub margin_mm: f32,
    pub include_chart: bool,
    pub include_table: bool,
    pub include_resources: bool,
    /// Draw the critical path in its own colour.
    pub show_critical: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        PrintOptions {
            paper: A4,
            orientation: Orientation::Landscape,
            margin_mm: 12.0,
            include_chart: true,
            include_table: true,
            include_resources: true,
            show_critical: false,
        }
    }
}

impl PrintOptions {
    /// The page box, with orientation applied.
    fn page(&self) -> (f32, f32) {
        match self.orientation {
            Orientation::Landscape => (self.paper.height, self.paper.width),
            Orientation::Portrait => (self.paper.width, self.paper.height),
        }
    }

    fn margin(&self) -> f32 {
        // 72 points to the inch, 25.4 millimetres to the inch.
        self.margin_mm * 72.0 / 25.4
    }
}

/// Ink, as the document uses it. Print is on white paper, so this is its own
/// palette rather than the screen's.
mod ink {
    pub const TEXT: (f32, f32, f32) = (0.06, 0.13, 0.12);
    pub const SOFT: (f32, f32, f32) = (0.33, 0.44, 0.44);
    pub const FAINT: (f32, f32, f32) = (0.54, 0.64, 0.64);
    pub const RULE: (f32, f32, f32) = (0.83, 0.88, 0.88);
    pub const BAND: (f32, f32, f32) = (0.93, 0.96, 0.96);
    pub const BRAND: (f32, f32, f32) = (0.18, 0.37, 0.37);
    pub const BAR: (f32, f32, f32) = (0.29, 0.55, 0.55);
    pub const CRITICAL: (f32, f32, f32) = (0.70, 0.34, 0.36);
    pub const SUMMARY: (f32, f32, f32) = (0.13, 0.25, 0.25);
}

/// Width of a string in Helvetica at a given size.
///
/// Helvetica's real widths vary per character, and a rough average would make
/// truncation either cut early or overrun. These are the metrics for the
/// standard font, scaled from units of 1000 to the em.
fn text_width(text: &str, size: f32) -> f32 {
    let units: u32 = text.chars().map(helvetica_width).sum();
    units as f32 / 1000.0 * size
}

/// Character widths for Helvetica, in units of 1000 to the em.
fn helvetica_width(c: char) -> u32 {
    match c {
        ' ' | '!' | '|' | '\'' | ',' | '.' | '/' | ':' | ';' | '`' => 278,
        'i' | 'j' | 'l' | '(' | ')' | '[' | ']' | '{' | '}' | 'I' => 222,
        'f' | 't' | '-' => 278,
        'r' => 333,
        'm' => 833,
        'w' => 722,
        'M' | 'W' => 889,
        'A'..='Z' => 667,
        '0'..='9' => 556,
        'a'..='z' => 556,
        _ => 500,
    }
}

/// Cut a string to fit a width, with an ellipsis if it had to be cut.
fn fit(text: &str, size: f32, width: f32) -> String {
    if text_width(text, size) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        if text_width(&format!("{out}{c}\u{2026}"), size) > width {
            break;
        }
        out.push(c);
    }
    if out.is_empty() {
        return String::new();
    }
    out.push('\u{2026}');
    out
}

/// PDF text strings are bytes, and the standard fonts are single byte encoded,
/// so anything outside that range is replaced rather than silently mangled.
fn encode(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect()
}

/// Draw a line of text.
#[allow(clippy::too_many_arguments)]
fn text(
    content: &mut Content,
    font: Name,
    size: f32,
    colour: (f32, f32, f32),
    x: f32,
    y: f32,
    value: &str,
) {
    if value.is_empty() {
        return;
    }
    content.set_fill_rgb(colour.0, colour.1, colour.2);
    content.begin_text();
    content.set_font(font, size);
    content.next_line(x, y);
    content.show(Str(&encode(value)));
    content.end_text();
}

fn filled_rect(content: &mut Content, colour: (f32, f32, f32), x: f32, y: f32, w: f32, h: f32) {
    content.set_fill_rgb(colour.0, colour.1, colour.2);
    content.rect(x, y, w, h);
    content.fill_nonzero();
}

/// Everything the document needs to know about one page's worth of rows.
struct Layout {
    page_w: f32,
    page_h: f32,
    margin: f32,
    /// Width of the task-name column beside the chart.
    label_w: f32,
    row_h: f32,
}

impl Layout {
    fn new(options: &PrintOptions) -> Self {
        let (page_w, page_h) = options.page();
        Layout {
            page_w,
            page_h,
            margin: options.margin(),
            label_w: 150.0,
            row_h: 12.0,
        }
    }

    fn content_w(&self) -> f32 {
        self.page_w - self.margin * 2.0
    }

    /// How many rows fit below the header on a page.
    fn rows_per_page(&self, header: f32) -> usize {
        let usable = self.page_h - self.margin * 2.0 - header;
        (usable / self.row_h).floor().max(1.0) as usize
    }
}

/// Render the plan as a PDF document.
pub fn render(project: &Project, options: &PrintOptions) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let font = Ref::new(3);
    let mut next = 4;

    // Helvetica is one of the standard fourteen, so it is named rather than
    // embedded and every reader already has it.
    pdf.type1_font(font).base_font(Name(b"Helvetica"));
    let bold = Ref::new(next);
    next += 1;
    pdf.type1_font(bold).base_font(Name(b"Helvetica-Bold"));

    let layout = Layout::new(options);
    let mut pages: Vec<Ref> = Vec::new();
    let mut bodies: Vec<(Ref, Ref, Vec<u8>)> = Vec::new();

    let sheets = compose(project, options, &layout);
    for sheet in sheets {
        let page = Ref::new(next);
        next += 1;
        let body = Ref::new(next);
        next += 1;
        pages.push(page);
        bodies.push((page, body, sheet));
    }

    // A plan with nothing in it still produces a page, so the document is never
    // structurally invalid.
    if bodies.is_empty() {
        let page = Ref::new(next);
        next += 1;
        let body = Ref::new(next);
        pages.push(page);
        bodies.push((page, body, Content::new().finish().to_vec()));
    }

    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids(pages.iter().copied()).count(pages.len() as i32);

    for (page, body, content) in &bodies {
        let mut page_writer = pdf.page(*page);
        page_writer
            .parent(tree)
            .media_box(Rect::new(0.0, 0.0, layout.page_w, layout.page_h))
            .contents(*body);
        page_writer
            .resources()
            .fonts()
            .pair(Name(b"F1"), font)
            .pair(Name(b"F2"), bold);
        page_writer.finish();
        pdf.stream(*body, content);
    }

    pdf.finish()
}

/// Build the content stream for every page.
fn compose(project: &Project, options: &PrintOptions, layout: &Layout) -> Vec<Vec<u8>> {
    let mut sheets = Vec::new();
    let rows: Vec<usize> = (0..project.tasks.len()).collect();

    if options.include_chart && !rows.is_empty() {
        let header = 96.0;
        let per_page = layout.rows_per_page(header);
        for (number, chunk) in rows.chunks(per_page).enumerate() {
            sheets.push(chart_page(project, options, layout, chunk, number));
        }
    }

    if options.include_table && !rows.is_empty() {
        let header = 60.0;
        let per_page = layout.rows_per_page(header);
        for (number, chunk) in rows.chunks(per_page).enumerate() {
            sheets.push(table_page(project, options, layout, chunk, number));
        }
    }

    if options.include_resources && !project.resources.is_empty() {
        sheets.push(resource_page(project, layout));
    }

    sheets
}

/// The title band that opens every page.
fn masthead(
    content: &mut Content,
    project: &Project,
    layout: &Layout,
    title: &str,
    subtitle: &str,
) -> f32 {
    let top = layout.page_h - layout.margin;
    text(content, Name(b"F2"), 16.0, ink::TEXT, layout.margin, top - 14.0, title);

    let brand = "ALTERION OPEN PROJECT";
    let brand_w = text_width(brand, 8.0);
    text(
        content,
        Name(b"F2"),
        8.0,
        ink::BRAND,
        layout.page_w - layout.margin - brand_w,
        top - 12.0,
        brand,
    );

    text(content, Name(b"F1"), 9.0, ink::SOFT, layout.margin, top - 27.0, subtitle);

    // The rule under the title, in the brand colour, as on screen.
    let rule_y = top - 34.0;
    content.set_stroke_rgb(ink::BRAND.0, ink::BRAND.1, ink::BRAND.2);
    content.set_line_width(1.2);
    content.move_to(layout.margin, rule_y);
    content.line_to(layout.page_w - layout.margin, rule_y);
    content.stroke();

    let _ = project;
    rule_y
}

/// One page of the chart, drawn against the whole plan's timescale.
fn chart_page(
    project: &Project,
    options: &PrintOptions,
    layout: &Layout,
    rows: &[usize],
    number: usize,
) -> Vec<u8> {
    let mut content = Content::new();

    let title = if number == 0 {
        project.name.clone()
    } else {
        format!("{} (continued)", project.name)
    };
    let subtitle = format!(
        "{} to {}",
        project.start_date.format("%d %B %Y"),
        project.finish_date.format("%d %B %Y")
    );
    let mut y = masthead(&mut content, project, layout, &title, &subtitle);

    if number == 0 {
        y = figures(&mut content, project, layout, y);
    }

    // The timescale covers every task, not just this page's rows, so the pages
    // line up with one another when laid side by side.
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
    let span = (finish - start).num_minutes().max(1) as f32;

    let chart_x = layout.margin + layout.label_w;
    let chart_w = layout.content_w() - layout.label_w;
    let at = |value: NaiveDateTime| {
        chart_x + (value - start).num_minutes() as f32 / span * chart_w
    };

    y -= 16.0;
    let top = y;

    // Month ticks give the bars something to read against.
    let mut tick = NaiveDate::from_ymd_opt(start.year(), start.month(), 1).unwrap_or(start.date());
    let bottom = top - rows.len() as f32 * layout.row_h - 4.0;
    while tick <= finish.date() {
        let x = at(tick.and_hms_opt(0, 0, 0).unwrap_or(start));
        if x >= chart_x {
            content.set_stroke_rgb(ink::RULE.0, ink::RULE.1, ink::RULE.2);
            content.set_line_width(0.5);
            content.move_to(x, top);
            content.line_to(x, bottom);
            content.stroke();
            text(
                &mut content,
                Name(b"F1"),
                6.5,
                ink::FAINT,
                x + 2.0,
                top + 4.0,
                &tick.format("%b %y").to_string(),
            );
        }
        tick = if tick.month() == 12 {
            NaiveDate::from_ymd_opt(tick.year() + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(tick.year(), tick.month() + 1, 1)
        }
        .unwrap_or(finish.date() + chrono::Duration::days(1));
    }

    for (line, &index) in rows.iter().enumerate() {
        let task = &project.tasks[index];
        let row_y = top - (line as f32 + 1.0) * layout.row_h;
        let summary = project.is_summary(index);
        let indent = 4.0 + task.outline_level as f32 * 7.0;

        let name_font = if summary { Name(b"F2") } else { Name(b"F1") };
        let name_ink = if summary { ink::TEXT } else { ink::SOFT };
        text(
            &mut content,
            name_font,
            7.0,
            name_ink,
            layout.margin + indent,
            row_y + 3.0,
            &fit(&task.name, 7.0, layout.label_w - indent - 6.0),
        );

        let left = at(task.scheduled.start);
        let right = at(task.scheduled.finish);
        let width = (right - left).max(1.2);
        let critical = options.show_critical && crate::issues::shows_as_critical(project, index);

        if project.is_marker(index) {
            // A milestone is a diamond, so it reads as an instant rather than a
            // span a reader might try to measure.
            let size = 3.0;
            let mid = row_y + layout.row_h * 0.35;
            content.set_fill_rgb(ink::SUMMARY.0, ink::SUMMARY.1, ink::SUMMARY.2);
            content.move_to(left, mid + size);
            content.line_to(left + size, mid);
            content.line_to(left, mid - size);
            content.line_to(left - size, mid);
            content.close_path();
            content.fill_nonzero();
        } else if summary {
            filled_rect(&mut content, ink::SUMMARY, left, row_y + 4.0, width, 2.5);
        } else {
            let colour = if critical { ink::CRITICAL } else { ink::BAR };
            filled_rect(&mut content, colour, left, row_y + 2.0, width, 6.0);
        }
    }

    content.finish().to_vec()
}

/// The headline figures under the title on the first page.
fn figures(content: &mut Content, project: &Project, layout: &Layout, y: f32) -> f32 {
    let critical = (0..project.tasks.len())
        .filter(|&i| !project.is_summary(i) && project.tasks[i].scheduled.critical)
        .count();
    let work: i64 = project.tasks.iter().map(|t| t.scheduled.work_minutes).sum();
    let cost: f64 = project.tasks.iter().map(|t| t.scheduled.cost).sum();
    let duration = format_duration(
        project
            .calendar
            .work_minutes_between(project.start_date, project.finish_date),
    );

    let cells = [
        ("DURATION", duration),
        ("TASKS", project.tasks.len().to_string()),
        ("CRITICAL", critical.to_string()),
        ("WORK", format_work(work)),
        ("COST", format!("{}{:.2}", project.currency_symbol, cost)),
    ];

    let gap = 6.0;
    let width = (layout.content_w() - gap * (cells.len() as f32 - 1.0)) / cells.len() as f32;
    let height = 30.0;
    let top = y - 10.0;

    for (index, (label, value)) in cells.iter().enumerate() {
        let x = layout.margin + index as f32 * (width + gap);
        filled_rect(content, ink::BAND, x, top - height, width, height);
        text(content, Name(b"F2"), 11.0, ink::TEXT, x + 7.0, top - 14.0, value);
        text(content, Name(b"F1"), 6.0, ink::SOFT, x + 7.0, top - 24.0, label);
    }

    top - height
}

/// One page of the task table.
fn table_page(
    project: &Project,
    options: &PrintOptions,
    layout: &Layout,
    rows: &[usize],
    number: usize,
) -> Vec<u8> {
    let mut content = Content::new();
    let title = if number == 0 {
        "Task Table".to_string()
    } else {
        "Task Table (continued)".to_string()
    };
    let mut y = masthead(&mut content, project, layout, &project.name, &title);
    y -= 14.0;

    // Proportions of the content width, so the table fits any paper.
    let widths = [0.05, 0.34, 0.09, 0.13, 0.13, 0.12, 0.07, 0.07];
    let headings = [
        "ID",
        "Task Name",
        "Duration",
        "Start",
        "Finish",
        "Predecessors",
        "%",
        "Cost",
    ];

    let mut x_of = Vec::new();
    let mut cursor = layout.margin;
    for share in widths {
        x_of.push(cursor);
        cursor += share * layout.content_w();
    }

    filled_rect(&mut content, ink::BAND, layout.margin, y - 12.0, layout.content_w(), 14.0);
    for (index, heading) in headings.iter().enumerate() {
        text(&mut content, Name(b"F2"), 6.5, ink::SOFT, x_of[index] + 3.0, y - 8.0, heading);
    }
    y -= 14.0;

    for &index in rows {
        let task = &project.tasks[index];
        let summary = project.is_summary(index);
        let critical =
            options.show_critical && !summary && crate::issues::shows_as_critical(project, index);
        let row_ink = if critical { ink::CRITICAL } else { ink::TEXT };
        let font = if summary { Name(b"F2") } else { Name(b"F1") };

        let predecessors = project
            .predecessors_of(task.id)
            .iter()
            .filter_map(|link| project.index_of(link.predecessor))
            .map(|row| (row + 1).to_string())
            .collect::<Vec<_>>()
            .join(",");

        let indent = task.outline_level as f32 * 6.0;
        let cells = [
            (0usize, (index + 1).to_string(), 0.0),
            (1, task.name.clone(), indent),
            (2, format_duration(task.scheduled.duration_minutes), 0.0),
            (3, task.scheduled.start.format("%d/%m/%y").to_string(), 0.0),
            (4, task.scheduled.finish.format("%d/%m/%y").to_string(), 0.0),
            (5, predecessors, 0.0),
            (6, format!("{}%", task.percent_complete), 0.0),
            (7, format!("{:.0}", task.scheduled.cost), 0.0),
        ];

        for (column, value, offset) in cells {
            let width = widths[column] * layout.content_w() - 6.0 - offset;
            text(
                &mut content,
                font,
                7.0,
                row_ink,
                x_of[column] + 3.0 + offset,
                y - 8.0,
                &fit(&value, 7.0, width),
            );
        }

        content.set_stroke_rgb(ink::RULE.0, ink::RULE.1, ink::RULE.2);
        content.set_line_width(0.4);
        content.move_to(layout.margin, y - 11.0);
        content.line_to(layout.page_w - layout.margin, y - 11.0);
        content.stroke();

        y -= layout.row_h;
    }

    content.finish().to_vec()
}

/// The resource sheet.
fn resource_page(project: &Project, layout: &Layout) -> Vec<u8> {
    let mut content = Content::new();
    let mut y = masthead(&mut content, project, layout, &project.name, "Resources");
    y -= 14.0;

    let widths = [0.06, 0.34, 0.15, 0.2, 0.12, 0.13];
    let headings = ["ID", "Resource Name", "Type", "Group", "Max Units", "Std Rate"];
    let mut x_of = Vec::new();
    let mut cursor = layout.margin;
    for share in widths {
        x_of.push(cursor);
        cursor += share * layout.content_w();
    }

    filled_rect(&mut content, ink::BAND, layout.margin, y - 12.0, layout.content_w(), 14.0);
    for (index, heading) in headings.iter().enumerate() {
        text(&mut content, Name(b"F2"), 6.5, ink::SOFT, x_of[index] + 3.0, y - 8.0, heading);
    }
    y -= 14.0;

    for (number, resource) in project.resources.iter().enumerate() {
        let cells = [
            (number + 1).to_string(),
            resource.name.clone(),
            resource.kind.label().to_string(),
            resource.group.clone(),
            format!("{:.0}%", resource.max_units * 100.0),
            format!("{}{:.2}/hr", project.currency_symbol, resource.standard_rate),
        ];
        for (column, value) in cells.iter().enumerate() {
            let width = widths[column] * layout.content_w() - 6.0;
            text(
                &mut content,
                Name(b"F1"),
                7.0,
                ink::TEXT,
                x_of[column] + 3.0,
                y - 8.0,
                &fit(value, 7.0, width),
            );
        }
        y -= layout.row_h;
    }

    content.finish().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    fn plan() -> Project {
        let start = NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut project = templates::build(templates::by_id("simple").unwrap(), start);
        crate::schedule(&mut project).unwrap();
        project
    }

    #[test]
    fn a_rendered_plan_is_a_pdf() {
        let bytes = render(&plan(), &PrintOptions::default());
        assert!(bytes.starts_with(b"%PDF-"), "it has to say what it is");
        assert!(bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF"));
        assert!(bytes.len() > 1000, "and actually contain the plan");
    }

    #[test]
    fn an_empty_plan_still_produces_a_valid_document() {
        // A printer handed a zero page document is a worse outcome than a
        // printer handed a blank sheet.
        let mut project = plan();
        project.tasks.clear();
        project.resources.clear();
        let bytes = render(&project, &PrintOptions::default());
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn leaving_a_section_out_makes_a_shorter_document() {
        let project = plan();
        let everything = render(&project, &PrintOptions::default());
        let chart_only = render(
            &project,
            &PrintOptions {
                include_table: false,
                include_resources: false,
                ..PrintOptions::default()
            },
        );
        assert!(chart_only.len() < everything.len());
    }

    #[test]
    fn orientation_turns_the_page_over() {
        let landscape = PrintOptions::default();
        let portrait = PrintOptions {
            orientation: Orientation::Portrait,
            ..landscape
        };
        assert_eq!(landscape.page(), (A4.height, A4.width));
        assert_eq!(portrait.page(), (A4.width, A4.height));
    }

    #[test]
    fn a_margin_is_given_in_millimetres_and_used_in_points() {
        // 25.4mm is an inch, and an inch is 72 points.
        let options = PrintOptions {
            margin_mm: 25.4,
            ..PrintOptions::default()
        };
        assert!((options.margin() - 72.0).abs() < 0.01);
    }

    #[test]
    fn text_is_measured_rather_than_guessed_at() {
        // A rough average would make truncation cut early or overrun.
        let narrow = text_width("lllll", 10.0);
        let wide = text_width("MMMMM", 10.0);
        assert!(wide > narrow * 2.0, "Helvetica is not monospaced");
    }

    #[test]
    fn a_name_too_long_for_its_column_is_cut_with_an_ellipsis() {
        let cut = fit("Deliver workstream one and everything after it", 7.0, 40.0);
        assert!(cut.ends_with('\u{2026}'));
        assert!(text_width(&cut, 7.0) <= 40.0);
    }

    #[test]
    fn a_name_that_fits_is_left_alone() {
        assert_eq!(fit("Kickoff", 7.0, 200.0), "Kickoff");
    }

    #[test]
    fn text_outside_the_single_byte_range_does_not_corrupt_the_stream() {
        // The standard fonts are single byte encoded. Replacing is honest;
        // writing the raw bytes would produce a broken document.
        assert_eq!(encode("Phase \u{2192} two"), b"Phase ? two".to_vec());
    }

    #[test]
    fn a_long_plan_runs_onto_more_pages_than_a_short_one() {
        let short = plan();
        let mut long = plan();
        let template = long.tasks[1].clone();
        for _ in 0..200 {
            let mut task = template.clone();
            task.id = long.allocate_task_id();
            long.tasks.push(task);
        }
        crate::schedule(&mut long).unwrap();

        let layout = Layout::new(&PrintOptions::default());
        let short_pages = compose(&short, &PrintOptions::default(), &layout).len();
        let long_pages = compose(&long, &PrintOptions::default(), &layout).len();
        assert!(long_pages > short_pages);
    }
}
