//! Reading and writing a plan as a spreadsheet.
//!
//! Not a second file format: `.aprj` is the plan, and a workbook is a view of
//! it that other people can open. The point is the exchange, since a
//! spreadsheet is what a plan usually has to become before it reaches somebody
//! who does not have a scheduler.
//!
//! Which means the import has to be forgiving. A workbook that comes back has
//! been through somebody else's hands: columns reordered, renamed, some
//! deleted, rows added by hand. Columns are matched by their heading rather
//! than their position, anything unrecognised is left alone, and a row that
//! cannot be read is skipped rather than failing the file.

use chrono::{NaiveDate, NaiveDateTime};
use rust_xlsxwriter::{Format, FormatAlign, Workbook};

use crate::model::{LinkType, Project, Task, TaskMode};
use crate::{format_duration, parse_duration};

/// What went wrong.
#[derive(Debug)]
pub enum ExcelError {
    Io(std::io::Error),
    /// The file is not a workbook, or is one this cannot open.
    NotAWorkbook(String),
    /// It opened, but held nothing that looks like a plan.
    NoTasks,
    Write(String),
}

impl std::fmt::Display for ExcelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExcelError::Io(error) => write!(f, "{error}"),
            ExcelError::NotAWorkbook(why) => write!(f, "This is not a workbook this can read: {why}"),
            ExcelError::NoTasks => write!(
                f,
                "No task rows were found. The sheet needs a heading row with at least a Task Name column."
            ),
            ExcelError::Write(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ExcelError {}

impl From<std::io::Error> for ExcelError {
    fn from(error: std::io::Error) -> Self {
        ExcelError::Io(error)
    }
}

/// The columns a plan is written out as, in order.
///
/// The headings double as what the import looks for, so a workbook written
/// here always reads back.
pub(crate) const COLUMNS: [&str; 11] = [
    "ID",
    "WBS",
    "Task Name",
    "Duration",
    "Start",
    "Finish",
    "Predecessors",
    "Resources",
    "% Complete",
    "Work",
    "Cost",
];

/// Reduce a heading to something matchable, so "Task Name", "task name" and
/// "TASK_NAME" are all the same column.
///
/// Shared with `sheet`, which matches a stranger's headings against a longer
/// list of names for the same thing: two rules for what makes two headings the
/// same would disagree eventually, and the import would then depend on which
/// reader opened the file.
pub(crate) fn key(heading: &str) -> String {
    heading
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Write the plan as a workbook.
pub fn save(path: &std::path::Path, project: &Project) -> Result<(), ExcelError> {
    let mut book = Workbook::new();

    let heading = Format::new()
        .set_bold()
        .set_background_color(0xEEF4F4)
        .set_font_color(0x55706F)
        .set_border_bottom(rust_xlsxwriter::FormatBorder::Thin);
    let summary = Format::new().set_bold();
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let money = Format::new().set_num_format("#,##0.00");
    let right = Format::new().set_align(FormatAlign::Right);

    // ---- the plan ------------------------------------------------------
    let sheet = book.add_worksheet().set_name("Tasks").map_err(wrote)?;
    for (column, name) in COLUMNS.iter().enumerate() {
        sheet
            .write_string_with_format(0, column as u16, *name, &heading)
            .map_err(wrote)?;
    }
    // Widths chosen so a reader does not have to resize before reading.
    for (column, width) in [(0, 6.0), (1, 9.0), (2, 44.0), (3, 11.0), (4, 12.0), (5, 12.0), (6, 16.0), (7, 24.0), (8, 11.0), (9, 10.0), (10, 11.0)] {
        sheet.set_column_width(column, width).map_err(wrote)?;
    }

    for (index, task) in project.tasks.iter().enumerate() {
        let row = index as u32 + 1;
        let is_summary = project.is_summary(index);
        let name_format = if is_summary { &summary } else { &right };

        sheet.write_number(row, 0, (index + 1) as f64).map_err(wrote)?;
        sheet.write_string(row, 1, project.wbs(index)).map_err(wrote)?;
        // Indented by outline level, so the structure survives the trip.
        let indented = format!(
            "{}{}",
            "    ".repeat(task.outline_level as usize),
            task.name
        );
        if is_summary {
            sheet
                .write_string_with_format(row, 2, &indented, name_format)
                .map_err(wrote)?;
        } else {
            sheet.write_string(row, 2, &indented).map_err(wrote)?;
        }
        sheet
            .write_string(row, 3, format_duration(task.scheduled.duration_minutes))
            .map_err(wrote)?;
        sheet
            .write_string_with_format(row, 4, task.scheduled.start.format("%Y-%m-%d").to_string(), &date)
            .map_err(wrote)?;
        sheet
            .write_string_with_format(row, 5, task.scheduled.finish.format("%Y-%m-%d").to_string(), &date)
            .map_err(wrote)?;
        sheet
            .write_string(row, 6, project.predecessor_text(task.id))
            .map_err(wrote)?;
        sheet
            .write_string(row, 7, project.resource_text(task))
            .map_err(wrote)?;
        sheet
            .write_number(row, 8, task.percent_complete as f64)
            .map_err(wrote)?;
        sheet
            .write_string(row, 9, crate::format_work(task.scheduled.work_minutes))
            .map_err(wrote)?;
        sheet
            .write_number_with_format(row, 10, task.scheduled.cost, &money)
            .map_err(wrote)?;
    }

    // ---- who is on it --------------------------------------------------
    if !project.resources.is_empty() {
        let sheet = book.add_worksheet().set_name("Resources").map_err(wrote)?;
        for (column, name) in ["ID", "Resource Name", "Type", "Group", "Max Units", "Std Rate"]
            .iter()
            .enumerate()
        {
            sheet
                .write_string_with_format(0, column as u16, *name, &heading)
                .map_err(wrote)?;
        }
        sheet.set_column_width(1, 30.0).map_err(wrote)?;
        for (index, resource) in project.resources.iter().enumerate() {
            let row = index as u32 + 1;
            sheet.write_number(row, 0, (index + 1) as f64).map_err(wrote)?;
            sheet.write_string(row, 1, &resource.name).map_err(wrote)?;
            sheet.write_string(row, 2, resource.kind.label()).map_err(wrote)?;
            sheet.write_string(row, 3, &resource.group).map_err(wrote)?;
            sheet
                .write_number(row, 4, resource.max_units * 100.0)
                .map_err(wrote)?;
            sheet
                .write_number_with_format(row, 5, resource.standard_rate, &money)
                .map_err(wrote)?;
        }
    }

    book.save(path).map_err(wrote)?;
    Ok(())
}

fn wrote(error: rust_xlsxwriter::XlsxError) -> ExcelError {
    ExcelError::Write(format!("Could not write the workbook: {error}"))
}

/// Read a plan out of a workbook.
///
/// Columns are found by their heading wherever they sit, so a sheet whose
/// columns have been reordered or partly deleted still reads. A row without a
/// name is the end of the plan, not an error.
pub fn open(path: &std::path::Path) -> Result<Project, ExcelError> {
    use calamine::{open_workbook_auto, Data, Reader};

    let mut book =
        open_workbook_auto(path).map_err(|error| ExcelError::NotAWorkbook(error.to_string()))?;

    // The sheet called Tasks if there is one, otherwise the first with a
    // heading this recognises.
    let names = book.sheet_names().to_vec();
    let mut chosen = None;
    for name in &names {
        let Ok(range) = book.worksheet_range(name) else {
            continue;
        };
        let has_name = range
            .rows()
            .next()
            .is_some_and(|row| row.iter().any(|cell| key(&cell.to_string()) == "taskname"));
        if has_name {
            chosen = Some(range);
            if name.eq_ignore_ascii_case("tasks") {
                break;
            }
        }
    }
    let Some(range) = chosen else {
        return Err(ExcelError::NoTasks);
    };

    let mut rows = range.rows();
    let Some(headings) = rows.next() else {
        return Err(ExcelError::NoTasks);
    };
    let column_of = |wanted: &str| -> Option<usize> {
        headings
            .iter()
            .position(|cell| key(&cell.to_string()) == key(wanted))
    };

    let name_at = column_of("Task Name").ok_or(ExcelError::NoTasks)?;
    let duration_at = column_of("Duration");
    let start_at = column_of("Start");
    let percent_at = column_of("% Complete");
    let predecessors_at = column_of("Predecessors");

    let text = |row: &[Data], at: Option<usize>| -> String {
        at.and_then(|index| row.get(index))
            .map(|cell| cell.to_string().trim().to_string())
            .unwrap_or_default()
    };
    // The name is read untrimmed: its leading spaces are the outline level,
    // which is the only place a spreadsheet can carry structure.
    let raw = |row: &[Data], at: usize| -> String {
        row.get(at).map(|cell| cell.to_string()).unwrap_or_default()
    };

    let mut project = Project::blank(
        NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap(),
    );
    project.tasks.clear();
    project.name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported plan".into());

    // Kept so predecessors, which are written as row numbers, can be resolved
    // once every row exists.
    let mut links: Vec<(usize, String)> = Vec::new();
    let mut earliest: Option<NaiveDateTime> = None;

    for row in rows {
        let raw_name = raw(row, name_at);
        // Trimmed for the emptiness test, kept untrimmed for the indent: a
        // cell of spaces is a blank row, not a deeply nested nameless task.
        if raw_name.trim().is_empty() {
            continue;
        }
        // The export indents by outline level; that is how the structure comes
        // back, since a spreadsheet has no other place to put it.
        let level = raw_name.len() - raw_name.trim_start().len();
        let name = raw_name.trim().to_string();

        // parse_duration also reports whether the figure was marked as an
        // estimate, which the sheet carries through.
        let (minutes, estimated) =
            parse_duration(&text(row, duration_at)).unwrap_or((480, false));
        let id = project.allocate_task_id();
        let mut task = Task::new(id, name, minutes);
        task.estimated = estimated;
        task.outline_level = (level / 4) as u16;

        if let Ok(percent) = text(row, percent_at)
            .trim_end_matches('%')
            .trim()
            .parse::<f64>()
        {
            task.percent_complete = percent.clamp(0.0, 100.0) as u8;
        }

        // A start given in the sheet is honoured, since somebody put it there
        // on purpose. Anything without one is left to the scheduler.
        if let Some(start) = parse_cell_date(&text(row, start_at)) {
            task.mode = TaskMode::Auto;
            task.constraint = crate::model::ConstraintType::StartNoEarlierThan;
            task.constraint_date = Some(start);
            earliest = Some(earliest.map_or(start, |held: NaiveDateTime| held.min(start)));
        }

        let predecessors = text(row, predecessors_at);
        if !predecessors.is_empty() {
            links.push((project.tasks.len(), predecessors));
        }
        project.tasks.push(task);
    }

    if project.tasks.is_empty() {
        return Err(ExcelError::NoTasks);
    }

    // The people, from their own sheet. Written out on export, so not reading
    // them back made the round trip quietly lossy.
    read_resources(&mut book, &mut project);
    if let Some(start) = earliest {
        project.start_date = start;
    }

    // Now every row exists, the row numbers can be turned into links.
    for (index, text) in links {
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            let Ok(number) = digits.parse::<usize>() else {
                continue;
            };
            let Some(from) = project.tasks.get(number.saturating_sub(1)).map(|t| t.id) else {
                continue;
            };
            let to = project.tasks[index].id;
            if from == to {
                continue;
            }
            let kind = LinkType::parse(part.trim_start_matches(|c: char| c.is_ascii_digit()))
                .unwrap_or(LinkType::FS);
            project.add_link(crate::model::Link {
                predecessor: from,
                successor: to,
                kind,
                lag_minutes: 0,
            });
        }
    }

    Ok(project)
}

/// Pull the resources out of their sheet, if the workbook has one.
///
/// Absent is not an error: plenty of workbooks are just a task list, and a
/// plan with no resources is a perfectly good plan.
fn read_resources<R>(book: &mut R, project: &mut Project)
where
    R: calamine::Reader<std::io::BufReader<std::fs::File>>,
{
    use crate::model::{Resource, ResourceKind};

    let name = book
        .sheet_names()
        .iter()
        .find(|name| name.eq_ignore_ascii_case("resources"))
        .cloned();
    let Some(name) = name else { return };
    let Ok(range) = book.worksheet_range(&name) else {
        return;
    };

    let mut rows = range.rows();
    let Some(headings) = rows.next() else { return };
    let column_of = |wanted: &str| -> Option<usize> {
        headings
            .iter()
            .position(|cell| key(&cell.to_string()) == key(wanted))
    };

    let name_at = match column_of("Resource Name").or_else(|| column_of("Name")) {
        Some(at) => at,
        None => return,
    };
    let group_at = column_of("Group");
    let units_at = column_of("Max Units");
    let rate_at = column_of("Std Rate").or_else(|| column_of("Standard Rate"));
    let kind_at = column_of("Type");

    for row in rows {
        let cell = |at: Option<usize>| -> String {
            at.and_then(|index| row.get(index))
                .map(|cell| cell.to_string().trim().to_string())
                .unwrap_or_default()
        };
        let resource_name = cell(Some(name_at));
        if resource_name.is_empty() {
            continue;
        }

        let id = project.allocate_resource_id();
        let mut resource = Resource::new(id, resource_name);
        resource.group = cell(group_at);
        if let Ok(units) = cell(units_at).trim_end_matches('%').trim().parse::<f64>() {
            // Written out as a percentage, so it comes back as one.
            resource.max_units = if units > 5.0 { units / 100.0 } else { units };
        }
        if let Ok(rate) = cell(rate_at).replace(['$', ',', ' '], "").parse::<f64>() {
            resource.standard_rate = rate;
        }
        let kind = cell(kind_at).to_lowercase();
        if kind.starts_with("mat") {
            resource.kind = ResourceKind::Material;
        } else if kind.starts_with("cost") {
            resource.kind = ResourceKind::Cost;
        }
        project.resources.push(resource);
    }
}

/// A date as a spreadsheet might hold it.
///
/// Dates arrive as text in any of several shapes depending on who saved the
/// file and where they live, so several are tried rather than insisting on one.
fn parse_cell_date(value: &str) -> Option<NaiveDateTime> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    // Excel writes a date as a number of days since 1899-12-30 when the cell
    // has no format attached.
    if let Ok(serial) = value.parse::<f64>()
        && (1.0..60_000.0).contains(&serial)
    {
        let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?;
        return (epoch + chrono::Duration::days(serial as i64)).and_hms_opt(8, 0, 0);
    }

    let head = value.split_whitespace().next().unwrap_or(value);
    for pattern in ["%Y-%m-%d", "%d/%m/%Y", "%m/%d/%Y", "%d-%m-%Y", "%d/%m/%y"] {
        if let Ok(date) = NaiveDate::parse_from_str(head, pattern) {
            return date.and_hms_opt(8, 0, 0);
        }
    }

    // A written out date keeps its spaces, so it is matched against the whole
    // value rather than the first word. %e takes a day written without a
    // leading zero, which is how a person writes one.
    for pattern in ["%e %b %Y", "%d %b %Y", "%e %B %Y", "%b %e, %Y"] {
        if let Ok(date) = NaiveDate::parse_from_str(value, pattern) {
            return date.and_hms_opt(8, 0, 0);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;

    fn plan() -> Project {
        let start = NaiveDate::from_ymd_opt(2026, 1, 5)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let mut project = templates::build(templates::by_id("simple").unwrap(), start);
        crate::schedule(&mut project).unwrap();
        project
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        // Two test runs at once would otherwise write the same file and read
        // back each other's workbook, so the process id goes in the path.
        let dir = std::env::temp_dir().join(format!("aop-excel-tests-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    #[test]
    fn a_written_workbook_reads_back_with_its_tasks() {
        let project = plan();
        let path = scratch("round-trip.xlsx");
        save(&path, &project).expect("written");

        let back = open(&path).expect("read");
        assert_eq!(back.tasks.len(), project.tasks.len());
        assert_eq!(back.tasks[1].name, project.tasks[1].name);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_outline_survives_the_trip() {
        // A spreadsheet has nowhere to put structure except the indent, so if
        // that is not read back the plan comes home flat.
        let project = plan();
        let path = scratch("outline.xlsx");
        save(&path, &project).expect("written");

        let back = open(&path).expect("read");
        let levels: Vec<u16> = back.tasks.iter().map(|t| t.outline_level).collect();
        assert!(levels.contains(&1), "children came back flat: {levels:?}");
        assert_eq!(
            levels,
            project.tasks.iter().map(|t| t.outline_level).collect::<Vec<_>>()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn links_survive_the_trip() {
        let project = plan();
        let path = scratch("links.xlsx");
        save(&path, &project).expect("written");

        let back = open(&path).expect("read");
        assert_eq!(back.links.len(), project.links.len());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resources_come_back_too() {
        // The export writes a Resources sheet, so not reading it made the
        // round trip quietly lossy: a plan went out with seven people on it
        // and came back with none.
        let project = plan();
        assert!(!project.resources.is_empty(), "the fixture has resources");
        let path = scratch("resources.xlsx");
        save(&path, &project).expect("written");

        let back = open(&path).expect("read");
        assert_eq!(back.resources.len(), project.resources.len());
        assert_eq!(back.resources[0].name, project.resources[0].name);
        assert!(
            (back.resources[0].standard_rate - project.resources[0].standard_rate).abs() < 0.01,
            "the rate has to survive or costs come back wrong"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_workbook_with_no_resource_sheet_is_not_an_error() {
        // Plenty of workbooks are just a task list.
        let path = scratch("tasks-only.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Task Name").unwrap();
        sheet.write_string(1, 0, "Only a task").unwrap();
        book.save(&path).unwrap();

        let back = open(&path).expect("read");
        assert_eq!(back.tasks.len(), 1);
        assert!(back.resources.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn trailing_space_in_a_name_is_normalised_away() {
        // Names arriving from other tools often carry trailing whitespace. It
        // is dropped rather than preserved, so the round trip is not quite
        // byte exact and that is the intended trade.
        let path = scratch("spacey.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Task Name").unwrap();
        sheet.write_string(1, 0, "Ledger Unit Test   ").unwrap();
        book.save(&path).unwrap();

        let back = open(&path).expect("read");
        assert_eq!(back.tasks[0].name, "Ledger Unit Test");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_sheet_with_no_task_name_column_says_so_rather_than_returning_nothing() {
        let path = scratch("wrong.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Fruit").unwrap();
        sheet.write_string(1, 0, "Apple").unwrap();
        book.save(&path).unwrap();

        assert!(matches!(open(&path), Err(ExcelError::NoTasks)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn columns_are_found_by_heading_wherever_they_sit() {
        // A workbook that comes back has been through somebody else's hands.
        let path = scratch("reordered.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        for (column, name) in ["Duration", "Task Name", "Notes"].iter().enumerate() {
            sheet.write_string(0, column as u16, *name).unwrap();
        }
        sheet.write_string(1, 0, "3 days").unwrap();
        sheet.write_string(1, 1, "Survey the site").unwrap();
        book.save(&path).unwrap();

        let back = open(&path).expect("read");
        assert_eq!(back.tasks.len(), 1);
        assert_eq!(back.tasks[0].name, "Survey the site");
        assert_eq!(back.tasks[0].duration_minutes, 1440, "3 days of 8 hours");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_row_with_no_name_is_skipped_rather_than_becoming_a_blank_task() {
        let path = scratch("gappy.xlsx");
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.write_string(0, 0, "Task Name").unwrap();
        sheet.write_string(1, 0, "One").unwrap();
        sheet.write_string(2, 0, "   ").unwrap();
        sheet.write_string(3, 0, "Two").unwrap();
        book.save(&path).unwrap();

        let back = open(&path).expect("read");
        assert_eq!(back.tasks.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn dates_are_read_in_whatever_shape_they_arrive() {
        assert!(parse_cell_date("2026-03-01").is_some());
        assert!(parse_cell_date("01/03/2026").is_some());
        assert!(parse_cell_date("1 Mar 2026").is_some());
        // An unformatted cell arrives as a serial number.
        // Serial 46082 counted from 1899-12-30, which Excel treats as day 0.
        let serial = parse_cell_date("46082").expect("a serial date");
        assert_eq!(serial.date(), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert!(parse_cell_date("not a date").is_none());
        assert!(parse_cell_date("").is_none());
    }

    #[test]
    fn headings_match_however_they_are_written() {
        assert_eq!(key("Task Name"), key("task name"));
        assert_eq!(key("Task Name"), key("TASK_NAME"));
        assert_eq!(key("% Complete"), key("Complete"));
    }
}
