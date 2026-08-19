//! Reading a plan out of a spreadsheet this application did not write.
//!
//! `excel::open` finds its columns by heading, which covers a workbook this
//! wrote and one close to it. It does not cover the ordinary case: a plan
//! somebody keeps in a sheet of their own, where the task name column is
//! headed Activity, the real headings sit under three rows of title and logo,
//! and half the columns are about something else entirely. That file either
//! failed outright or, worse, imported a fraction of itself in silence.
//!
//! So nothing here decides anything on its own authority. The sheet, the
//! heading row, what each column is, and the order of a date written as three
//! numbers are all guesses, every one of them is shown, and every one can be
//! overridden. The reading is kept apart from the choosing so both can be
//! tested without a window, which is the point: a wrong import is silent data
//! loss, and nobody spots that by looking at a Gantt chart.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

use crate::duration::DurationUnit;
use crate::model::{ConstraintType, Project, Task, TaskMode};

/// What went wrong before a single row could be read.
#[derive(Debug)]
pub enum SheetError {
    Io(std::io::Error),
    /// The file is not a workbook, or is one this cannot open.
    NotAWorkbook(String),
    /// It opened and every sheet in it is empty.
    Empty,
    /// No column is mapped to the task name, so there is nothing to import.
    NoName,
    /// The mapping is sound and no row under the heading row carries a name.
    NoRows,
}

impl std::fmt::Display for SheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SheetError::Io(error) => write!(f, "{error}"),
            SheetError::NotAWorkbook(why) => {
                write!(f, "This is not a spreadsheet this can read: {why}")
            }
            SheetError::Empty => write!(f, "Every sheet in this workbook is empty."),
            SheetError::NoName => write!(
                f,
                "Nothing is mapped to Task Name yet. A row without a name is not a task, so there is nothing to bring in."
            ),
            SheetError::NoRows => write!(
                f,
                "No row under the heading row has a name in the Task Name column."
            ),
        }
    }
}

impl std::error::Error for SheetError {}

impl From<std::io::Error> for SheetError {
    fn from(error: std::io::Error) -> Self {
        SheetError::Io(error)
    }
}

// ------------------------------------------------------------------ cells

/// One cell, in the few shapes that matter here.
///
/// Deliberately not calamine's own type: what the workbook says a cell *is*
/// decides how it is read, and that decision belongs in one place rather than
/// spread over every field that wants a date.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    /// Held exactly as the sheet holds it, leading spaces and all. Indentation
    /// in a name column is the only place a spreadsheet can carry structure,
    /// so trimming here would flatten a plan on the way in.
    Text(String),
    Number(f64),
    /// A cell the workbook itself types as a date. No order has to be guessed
    /// for one of these, which is why it is worth keeping apart from a number.
    Stamp { at: NaiveDateTime, date_only: bool },
    /// A cell formatted as elapsed time (`[hh]:mm`), held in real minutes.
    Elapsed(i64),
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        match self {
            Cell::Empty => true,
            Cell::Text(text) => text.trim().is_empty(),
            _ => false,
        }
    }

    /// The cell as a person would see it in the sheet.
    pub fn text(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(text) => text.trim().to_string(),
            // Rust prints 5.0 as "5", which is what the sheet shows.
            Cell::Number(value) => format!("{value}"),
            Cell::Stamp { at, date_only } => {
                if *date_only {
                    at.format("%Y-%m-%d").to_string()
                } else {
                    at.format("%Y-%m-%d %H:%M").to_string()
                }
            }
            Cell::Elapsed(minutes) => crate::format_work(*minutes),
        }
    }

    /// The text with its indentation left on, for the one column that needs it.
    pub fn raw(&self) -> String {
        match self {
            Cell::Text(text) => text.clone(),
            other => other.text(),
        }
    }
}

/// One sheet, read into memory once so that changing the mapping costs nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
    /// The widest row, since a sheet is ragged and the mapping is not.
    pub width: usize,
}

impl Sheet {
    pub fn cell(&self, row: usize, column: usize) -> &Cell {
        self.rows
            .get(row)
            .and_then(|row| row.get(column))
            .unwrap_or(&Cell::Empty)
    }

    pub fn row(&self, row: usize) -> &[Cell] {
        self.rows.get(row).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The heading over a column, or the spreadsheet's own name for it.
    ///
    /// "Column F" is no use on its own, which is why the page shows data under
    /// it, but it is better than an empty box when a column has no heading.
    pub fn heading(&self, heading_row: usize, column: usize) -> String {
        let text = self.cell(heading_row, column).text();
        if text.is_empty() {
            format!("Column {}", column_letter(column))
        } else {
            text
        }
    }

    /// The first few values in a column, so a person can see what it holds.
    pub fn samples(&self, column: usize, from_row: usize, want: usize) -> Vec<String> {
        self.rows
            .iter()
            .skip(from_row)
            .filter_map(|row| row.get(column))
            .filter(|cell| !cell.is_empty())
            .take(want)
            .map(Cell::text)
            .collect()
    }
}

/// The spreadsheet's own name for a column: A, B, ... Z, AA, AB.
pub fn column_letter(mut index: usize) -> String {
    let mut out = String::new();
    loop {
        out.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            return out;
        }
        index = index / 26 - 1;
    }
}

/// Read every sheet of a workbook.
///
/// All of them, up front: the person choosing has to be able to look at the
/// second sheet without waiting for the file again, and switching sheets after
/// that must not touch the disk at all.
pub fn survey(path: &std::path::Path) -> Result<Vec<Sheet>, SheetError> {
    use calamine::{Reader, open_workbook_auto};

    let mut book =
        open_workbook_auto(path).map_err(|error| SheetError::NotAWorkbook(error.to_string()))?;

    let names = book.sheet_names().to_vec();
    let mut sheets = Vec::new();
    for name in names {
        let Ok(range) = book.worksheet_range(&name) else {
            continue;
        };
        let mut rows: Vec<Vec<Cell>> = range
            .rows()
            .map(|row| row.iter().map(convert).collect())
            .collect();
        // Trailing blank rows are formatting, not data, and they would count
        // as skipped rows in the report and worry somebody for no reason.
        while rows.last().is_some_and(|row| row.iter().all(Cell::is_empty)) {
            rows.pop();
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        sheets.push(Sheet { name, rows, width });
    }

    if sheets.iter().all(|sheet| sheet.rows.is_empty()) {
        return Err(SheetError::Empty);
    }
    Ok(sheets)
}

/// Excel counts days from this, which it treats as day zero.
const SERIAL_EPOCH: (i32, u32, u32) = (1899, 12, 30);

fn convert(data: &calamine::Data) -> Cell {
    use calamine::Data;
    match data {
        Data::Empty => Cell::Empty,
        Data::String(text) => {
            if text.trim().is_empty() {
                Cell::Empty
            } else {
                Cell::Text(text.clone())
            }
        }
        Data::Int(value) => Cell::Number(*value as f64),
        Data::Float(value) => Cell::Number(*value),
        Data::Bool(value) => Cell::Text(value.to_string()),
        Data::DateTime(stamp) => {
            if stamp.is_duration() {
                // A `[hh]:mm` cell is a fraction of a twenty four hour day,
                // which is a real quantity of time rather than a working day.
                Cell::Elapsed((stamp.as_f64() * 24.0 * 60.0).round() as i64)
            } else {
                let (year, month, day, hour, minute, second, _) = stamp.to_ymd_hms_milli();
                match NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)
                    .and_then(|date| {
                        date.and_hms_opt(hour as u32, minute as u32, second as u32)
                    }) {
                    Some(at) => Cell::Stamp {
                        at,
                        date_only: hour == 0 && minute == 0 && second == 0,
                    },
                    // Excel's 1900-02-29 does not exist. Keep the number
                    // rather than throwing the cell away.
                    None => Cell::Number(stamp.as_f64()),
                }
            }
        }
        Data::DateTimeIso(text) => match parse_iso(text) {
            Some(cell) => cell,
            None => Cell::Text(text.clone()),
        },
        Data::DurationIso(text) => Cell::Text(text.clone()),
        // An error cell holds no value, and #REF! is not a task name.
        Data::Error(_) => Cell::Empty,
    }
}

fn parse_iso(text: &str) -> Option<Cell> {
    if let Ok(at) = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S") {
        return Some(Cell::Stamp {
            at,
            date_only: false,
        });
    }
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    Some(Cell::Stamp {
        at: date.and_hms_opt(0, 0, 0)?,
        date_only: true,
    })
}

// ----------------------------------------------------------------- fields

/// A thing a column can be.
///
/// The list is what `excel::COLUMNS` writes out, plus the two the exporter
/// carries in the shape of the sheet rather than in a column of its own:
/// outline level, which it writes as indentation, and notes. WBS is not here
/// because a WBS code is a rendering of the outline level rather than a second
/// fact, so a column of them maps to Outline Level and is read for its depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The sheet's own row numbers, which is what its Predecessors column
    /// refers to. Mapped so those references land on the right rows even when
    /// blank rows and banners mean the sheet's numbering is not ours.
    Id,
    Name,
    OutlineLevel,
    Duration,
    Start,
    Finish,
    PercentComplete,
    Predecessors,
    Resources,
    Work,
    Cost,
    Notes,
}

impl Field {
    pub const ALL: [Field; 12] = [
        Field::Id,
        Field::Name,
        Field::OutlineLevel,
        Field::Duration,
        Field::Start,
        Field::Finish,
        Field::PercentComplete,
        Field::Predecessors,
        Field::Resources,
        Field::Work,
        Field::Cost,
        Field::Notes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Id => "ID",
            Field::Name => "Task Name",
            Field::OutlineLevel => "Outline Level",
            Field::Duration => "Duration",
            Field::Start => "Start",
            Field::Finish => "Finish",
            Field::PercentComplete => "% Complete",
            Field::Predecessors => "Predecessors",
            Field::Resources => "Resources",
            Field::Work => "Work",
            Field::Cost => "Cost",
            Field::Notes => "Notes",
        }
    }

    /// What the field is for, in the words the page shows under the picker.
    pub fn hint(self) -> &'static str {
        match self {
            Field::Id => "The numbers the Predecessors column counts in",
            Field::Name => "Required. A row with no name is not a task",
            Field::OutlineLevel => "A number, or a WBS code read for its depth",
            Field::Duration => "How long the task takes",
            Field::Start => "When it starts",
            Field::Finish => "When it ends",
            Field::PercentComplete => "How much of it is done",
            Field::Predecessors => "What it waits for, as row numbers",
            Field::Resources => "Who is on it",
            Field::Work => "Effort, spread over whoever is on the task",
            Field::Cost => "Money booked against the task",
            Field::Notes => "Anything else worth keeping",
        }
    }

    /// The value a picker stores, which has to survive being written down.
    pub fn code(self) -> &'static str {
        match self {
            Field::Id => "id",
            Field::Name => "name",
            Field::OutlineLevel => "level",
            Field::Duration => "duration",
            Field::Start => "start",
            Field::Finish => "finish",
            Field::PercentComplete => "percent",
            Field::Predecessors => "predecessors",
            Field::Resources => "resources",
            Field::Work => "work",
            Field::Cost => "cost",
            Field::Notes => "notes",
        }
    }

    pub fn from_code(code: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|field| field.code() == code)
    }

    /// The headings this field answers to.
    ///
    /// Matched through `excel::key`, so case, spaces and punctuation do not
    /// matter and only the word does. The first entry of each list is what the
    /// exporter writes, so a workbook that went out of here and came back
    /// through this page maps itself.
    fn aliases(self) -> &'static [&'static str] {
        match self {
            Field::Id => &["ID", "Task ID", "Activity ID", "No", "Number", "Task Number", "Seq"],
            Field::Name => &[
                "Task Name",
                "Name",
                "Task",
                "Activity",
                "Activity Name",
                "Description",
                "Task Description",
                "Work Item",
                "Item",
                "Title",
                "Step",
            ],
            Field::OutlineLevel => &[
                "Outline Level",
                "Level",
                "WBS",
                "WBS Level",
                "Outline Number",
                "Indent",
                "Tier",
            ],
            Field::Duration => &[
                "Duration",
                "Dur",
                "Days",
                "Length",
                "Elapsed",
                "Working Days",
                "Duration Days",
            ],
            Field::Start => &[
                "Start",
                "Start Date",
                "Planned Start",
                "Scheduled Start",
                "Begin",
                "Begins",
                "From",
            ],
            Field::Finish => &[
                "Finish",
                "Finish Date",
                "Planned Finish",
                "Scheduled Finish",
                "End",
                "End Date",
                "To",
                "Due",
                "Due Date",
                "Target Date",
            ],
            Field::PercentComplete => &[
                "% Complete",
                "Percent Complete",
                "Pct Complete",
                "Progress",
                "% Done",
                "Done",
            ],
            Field::Predecessors => &[
                "Predecessors",
                "Predecessor",
                "Depends On",
                "Dependencies",
                "Dependency",
                "Preceded By",
                "Blocked By",
                "After",
            ],
            Field::Resources => &[
                "Resources",
                "Resource Names",
                "Resource",
                "Assigned To",
                "Assignee",
                "Owner",
                "Responsible",
                "Who",
                "Team",
                "Lead",
            ],
            Field::Work => &[
                "Work",
                "Effort",
                "Hours",
                "Man Hours",
                "Person Hours",
                "Effort Hours",
            ],
            Field::Cost => &["Cost", "Budget", "Total Cost", "Amount", "Price", "Spend", "Fixed Cost"],
            Field::Notes => &["Notes", "Note", "Comments", "Comment", "Remarks", "Details"],
        }
    }

    /// Which field a heading names, if any.
    ///
    /// The whole heading first, then the heading with an aside in brackets and
    /// a trailing unit or qualifier taken off, so "Duration (hours)" is a
    /// duration and "Start Date" is a start. Only those: matching any word
    /// anywhere would make a Cost Centre column a Cost column, and a wrong
    /// guess that looks deliberate is worse than no guess at all.
    pub fn for_heading(heading: &str) -> Option<Field> {
        /// Words that qualify a heading without changing what it names.
        const QUALIFIERS: [&str; 22] = [
            "date", "dates", "day", "days", "hour", "hours", "hrs", "min", "mins", "minutes",
            "week", "weeks", "wks", "total", "planned", "actual", "baseline", "est", "estimate",
            "estimated", "target", "scheduled",
        ];

        let named = |wanted: &str| -> Option<Field> {
            if wanted.is_empty() {
                return None;
            }
            Field::ALL.into_iter().find(|field| {
                field
                    .aliases()
                    .iter()
                    .any(|alias| crate::excel::key(alias) == wanted)
            })
        };

        if let Some(field) = named(&crate::excel::key(heading)) {
            return Some(field);
        }
        let head = heading.split('(').next().unwrap_or(heading);
        let mut words: Vec<String> = head
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(|word| word.to_lowercase())
            .collect();
        while !words.is_empty() {
            if let Some(field) = named(&words.concat()) {
                return Some(field);
            }
            if words.len() == 1 || !QUALIFIERS.contains(&words[words.len() - 1].as_str()) {
                return None;
            }
            words.pop();
        }
        None
    }
}

// ------------------------------------------------------------------ dates

/// Which way round `12/03/2026` is read.
///
/// There is no answer to that question in the file, and there never will be:
/// the same nine characters are two different days in two different offices.
/// So the choice is the importer's, it is shown on the page with a worked
/// example, and a value whose own digits settle the matter ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    DayFirst,
    MonthFirst,
}

impl DateOrder {
    pub const ALL: [DateOrder; 2] = [DateOrder::DayFirst, DateOrder::MonthFirst];

    pub fn label(self) -> &'static str {
        match self {
            DateOrder::DayFirst => "Day first, so 12/03/2026 is 12 March",
            DateOrder::MonthFirst => "Month first, so 12/03/2026 is 3 December",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            DateOrder::DayFirst => "dmy",
            DateOrder::MonthFirst => "mdy",
        }
    }

    pub fn from_code(code: &str) -> Option<DateOrder> {
        DateOrder::ALL.into_iter().find(|order| order.code() == code)
    }
}

/// How a date cell was read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DateRead {
    Blank,
    /// The workbook, or the value's own digits, settle it.
    Certain { at: NaiveDateTime, date_only: bool },
    /// Only the chosen order settles it. Counted, and reported.
    Assumed { at: NaiveDateTime, date_only: bool },
    /// There is something in the cell and it is not a date.
    Unreadable,
}

/// What a numeric date proves about the order of a sheet, on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    YearFirst,
    /// A first part over twelve can only be a day.
    DayFirst,
    /// A second part over twelve can only be a month.
    MonthFirst,
    /// Both parts are twelve or under, so nothing but the setting decides.
    Ambiguous,
    NotADate,
}

/// What the sheet's own dates say about which order it was written in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DateEvidence {
    /// Values that can only be day first, such as 25/03/2026.
    pub proves_day_first: usize,
    /// Values that can only be month first, such as 03/25/2026.
    pub proves_month_first: usize,
    /// Values the setting alone decides.
    pub ambiguous: usize,
}

impl DateEvidence {
    /// The order the file itself proves, when it proves one.
    pub fn settled(self) -> Option<DateOrder> {
        match (self.proves_day_first, self.proves_month_first) {
            (0, 0) => None,
            (_, 0) => Some(DateOrder::DayFirst),
            (0, _) => Some(DateOrder::MonthFirst),
            // Both, which means the file disagrees with itself. Nothing to do
            // but say so and let a person look at it.
            _ => None,
        }
    }

    pub fn contradictory(self) -> bool {
        self.proves_day_first > 0 && self.proves_month_first > 0
    }
}

/// Split a value into its date part and a time of day, if it carries one.
fn split_time(value: &str) -> (String, Option<NaiveTime>) {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let Some(at) = tokens.iter().position(|token| token.contains(':')) else {
        return (value.trim().to_string(), None);
    };

    // "2:30 PM" is two tokens and "2:30PM" is one, and both turn up.
    let meridiem = tokens
        .get(at + 1)
        .filter(|token| matches!(token.to_ascii_lowercase().as_str(), "am" | "pm"));
    let clock = match meridiem {
        Some(word) => format!("{} {}", tokens[at], word),
        None => tokens[at].to_string(),
    };
    let time = ["%H:%M:%S", "%H:%M", "%I:%M:%S %p", "%I:%M %p", "%I:%M%p", "%I:%M:%S%p"]
        .iter()
        .find_map(|pattern| NaiveTime::parse_from_str(&clock, pattern).ok());
    if time.is_none() {
        return (value.trim().to_string(), None);
    }

    let used = if meridiem.is_some() { 2 } else { 1 };
    let rest: Vec<&str> = tokens
        .iter()
        .enumerate()
        .filter(|(index, _)| *index < at || *index >= at + used)
        .map(|(_, token)| *token)
        .collect();
    (rest.join(" "), time)
}

/// A date written as three numbers, in whichever order.
fn split_triple(text: &str) -> Option<([i64; 3], usize)> {
    let separator = ['/', '-', '.'];
    if !text.contains(separator) {
        return None;
    }
    let parts: Vec<&str> = text.split(separator).collect();
    if parts.len() != 3 {
        return None;
    }
    let mut numbers = [0i64; 3];
    for (slot, part) in numbers.iter_mut().zip(parts.iter()) {
        let part = part.trim();
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    Some((numbers, parts[0].trim().len()))
}

fn shape_of(parts: [i64; 3], first_digits: usize) -> Shape {
    let [a, b, c] = parts;
    if first_digits == 4 || a > 31 {
        return if (1..=12).contains(&b) && (1..=31).contains(&c) {
            Shape::YearFirst
        } else {
            Shape::NotADate
        };
    }
    if a < 1 || b < 1 || a > 31 || b > 31 || c < 0 {
        return Shape::NotADate;
    }
    match (a > 12, b > 12) {
        (true, true) => Shape::NotADate,
        (true, false) => Shape::DayFirst,
        (false, true) => Shape::MonthFirst,
        (false, false) => Shape::Ambiguous,
    }
}

/// Two digit years, the way every spreadsheet resolves them: 69 is 1969 and 68
/// is 2068. Guessing differently would silently move a plan by a century.
fn full_year(value: i64, digits: usize) -> i32 {
    if digits > 2 {
        return value as i32;
    }
    if value <= 68 {
        2000 + value as i32
    } else {
        1900 + value as i32
    }
}

/// Read a date out of a cell.
///
/// A cell the workbook types as a date is taken as it stands. A bare number in
/// a date column is a serial, which is what an unformatted date cell holds.
/// Text is tried in the unambiguous shapes first, and only a bare three number
/// date whose own digits settle nothing falls back on the chosen order.
pub fn read_date(cell: &Cell, order: DateOrder) -> DateRead {
    match cell {
        Cell::Empty => return DateRead::Blank,
        Cell::Stamp { at, date_only } => {
            return DateRead::Certain {
                at: *at,
                date_only: *date_only,
            };
        }
        Cell::Number(serial) => {
            // The same range `excel.rs` accepts, which reaches to 2064 and
            // keeps a duration or a percentage from being read as a date.
            if !(1.0..60_000.0).contains(serial) {
                return DateRead::Unreadable;
            }
            let Some(epoch) =
                NaiveDate::from_ymd_opt(SERIAL_EPOCH.0, SERIAL_EPOCH.1, SERIAL_EPOCH.2)
            else {
                return DateRead::Unreadable;
            };
            let whole = serial.trunc();
            let minutes = ((serial - whole) * 24.0 * 60.0).round() as i64;
            let date = epoch + chrono::Duration::days(whole as i64);
            let Some(midnight) = date.and_hms_opt(0, 0, 0) else {
                return DateRead::Unreadable;
            };
            return DateRead::Certain {
                at: midnight + chrono::Duration::minutes(minutes),
                date_only: minutes == 0,
            };
        }
        Cell::Elapsed(_) => return DateRead::Unreadable,
        Cell::Text(_) => {}
    }

    let value = cell.text();
    if value.is_empty() {
        return DateRead::Blank;
    }
    let (head, time) = split_time(&value);
    let finish = |date: NaiveDate, assumed: bool| -> DateRead {
        let Some(at) = date.and_hms_opt(0, 0, 0) else {
            return DateRead::Unreadable;
        };
        let at = match time {
            Some(clock) => at.date().and_time(clock),
            None => at,
        };
        let date_only = time.is_none();
        if assumed {
            DateRead::Assumed { at, date_only }
        } else {
            DateRead::Certain { at, date_only }
        }
    };

    if let Some((parts, first_digits)) = split_triple(&head) {
        let [a, b, c] = parts;
        let shape = shape_of(parts, first_digits);
        return match shape {
            Shape::YearFirst => {
                match NaiveDate::from_ymd_opt(full_year(a, first_digits), b as u32, c as u32) {
                    Some(date) => finish(date, false),
                    None => DateRead::Unreadable,
                }
            }
            Shape::DayFirst | Shape::MonthFirst | Shape::Ambiguous => {
                let day_first = match shape {
                    Shape::DayFirst => true,
                    Shape::MonthFirst => false,
                    // Nothing in the value settles it, so the setting does,
                    // and the reading is marked as having leaned on it.
                    _ => order == DateOrder::DayFirst,
                };
                let (day, month) = if day_first { (a, b) } else { (b, a) };
                let year_digits = head
                    .split(['/', '-', '.'])
                    .nth(2)
                    .map(|part| part.trim().len())
                    .unwrap_or(4);
                match NaiveDate::from_ymd_opt(full_year(c, year_digits), month as u32, day as u32) {
                    Some(date) => finish(date, shape == Shape::Ambiguous),
                    None => DateRead::Unreadable,
                }
            }
            Shape::NotADate => DateRead::Unreadable,
        };
    }

    // A written out date, which is never ambiguous. `%e` takes a day with no
    // leading zero, which is how a person writes one.
    for pattern in [
        "%Y-%m-%d", "%e %b %Y", "%d %b %Y", "%e %B %Y", "%d %B %Y", "%b %e, %Y", "%B %e, %Y",
        "%b %e %Y", "%B %e %Y", "%e-%b-%Y", "%e-%b-%y",
    ] {
        if let Ok(date) = NaiveDate::parse_from_str(head.trim(), pattern) {
            return finish(date, false);
        }
    }
    DateRead::Unreadable
}

// ---------------------------------------------------------------- mapping

/// Everything the reader needs that the file cannot tell it.
#[derive(Debug, Clone, PartialEq)]
pub struct Mapping {
    /// Which row holds the headings. Very often not the first.
    pub heading_row: usize,
    /// What each column is, one entry per column of the sheet. `None` means
    /// the column is left where it is, which is the right answer for most of
    /// the columns in most sheets.
    pub columns: Vec<Option<Field>>,
    pub date_order: DateOrder,
    /// What a bare number in the Duration column means. Project reads one as
    /// days; a column headed Hours means hours, and the person importing gets
    /// the last word either way.
    pub duration_unit: DurationUnit,
}

impl Mapping {
    /// The best opening offer this can make for a sheet.
    pub fn guess(sheet: &Sheet) -> Mapping {
        let heading_row = guess_heading_row(sheet);
        let columns = guess_columns(sheet, heading_row);
        let mut mapping = Mapping {
            heading_row,
            columns,
            date_order: DateOrder::DayFirst,
            duration_unit: DurationUnit::Days,
        };
        mapping.duration_unit = guess_duration_unit(sheet, &mapping);
        // Only when the sheet proves it. An unproven guess would be a coin
        // toss dressed up as a finding.
        if let Some(order) = mapping.evidence(sheet).settled() {
            mapping.date_order = order;
        }
        mapping
    }

    pub fn column_of(&self, field: Field) -> Option<usize> {
        self.columns
            .iter()
            .position(|held| *held == Some(field))
    }

    /// Point a column at a field, taking it off whichever column had it.
    ///
    /// One field cannot come from two columns: reading both and letting the
    /// last win is exactly the kind of quiet nonsense this page exists to
    /// prevent.
    pub fn assign(&mut self, column: usize, field: Option<Field>) {
        if let Some(field) = field {
            for held in self.columns.iter_mut() {
                if *held == Some(field) {
                    *held = None;
                }
            }
        }
        if let Some(slot) = self.columns.get_mut(column) {
            *slot = field;
        }
    }

    /// What the sheet's own dates say about the order it was written in.
    pub fn evidence(&self, sheet: &Sheet) -> DateEvidence {
        let mut evidence = DateEvidence::default();
        for field in [Field::Start, Field::Finish] {
            let Some(column) = self.column_of(field) else {
                continue;
            };
            for row in sheet.rows.iter().skip(self.heading_row + 1) {
                let Some(cell) = row.get(column) else { continue };
                let Cell::Text(text) = cell else { continue };
                let (head, _) = split_time(text);
                let Some((parts, first_digits)) = split_triple(&head) else {
                    continue;
                };
                match shape_of(parts, first_digits) {
                    Shape::DayFirst => evidence.proves_day_first += 1,
                    Shape::MonthFirst => evidence.proves_month_first += 1,
                    Shape::Ambiguous => evidence.ambiguous += 1,
                    Shape::YearFirst | Shape::NotADate => {}
                }
            }
        }
        evidence
    }
}

/// Guess which row holds the headings.
///
/// A heading row is mostly words, and a plan's heading row usually names at
/// least one thing this recognises. Both matter: a title row and a heading row
/// are both text, and only one of them says "Start". Rows of data lose on the
/// same test, since a task row carries dates and numbers beside its name.
pub fn guess_heading_row(sheet: &Sheet) -> usize {
    let mut best = (0usize, i64::MIN);
    for (index, row) in sheet.rows.iter().enumerate().take(30) {
        let filled = row.iter().filter(|cell| !cell.is_empty()).count();
        if filled == 0 {
            continue;
        }
        let words = row
            .iter()
            .filter(|cell| matches!(cell, Cell::Text(_)) && !cell.is_empty())
            .count();
        let known = row
            .iter()
            .filter(|cell| Field::for_heading(&cell.text()).is_some())
            .count();
        let score = known as i64 * 8 + words as i64 * 2 - (filled - words) as i64 * 2;
        if score > best.1 {
            best = (index, score);
        }
    }
    best.0
}

/// Guess what each column is from its heading.
pub fn guess_columns(sheet: &Sheet, heading_row: usize) -> Vec<Option<Field>> {
    let mut columns: Vec<Option<Field>> = vec![None; sheet.width];
    let mut taken: Vec<Field> = Vec::new();
    for (index, slot) in columns.iter_mut().enumerate() {
        let heading = sheet.cell(heading_row, index).text();
        if let Some(field) = Field::for_heading(&heading)
            && !taken.contains(&field)
        {
            taken.push(field);
            *slot = Some(field);
        }
    }

    // A sheet whose name column is headed something nobody has thought of
    // still has a name column, and it is nearly always the leftmost column of
    // words. Guessing it beats opening the page with nothing mapped and no
    // hint of what to do; it is shown like every other guess and can be moved.
    if !taken.contains(&Field::Name) {
        let mut best: Option<(usize, usize)> = None;
        for column in 0..sheet.width {
            if columns.get(column).copied().flatten().is_some() {
                continue;
            }
            let values: Vec<&Cell> = sheet
                .rows
                .iter()
                .skip(heading_row + 1)
                .filter_map(|row| row.get(column))
                .filter(|cell| !cell.is_empty())
                .take(12)
                .collect();
            let words = values
                .iter()
                .filter(|cell| matches!(cell, Cell::Text(_)))
                .count();
            if values.is_empty() || words * 2 <= values.len() {
                continue;
            }
            // The longest words win. A name is a sentence and a reference is a
            // code, so length tells them apart where the heading does not.
            let length: usize = values.iter().map(|cell| cell.text().len()).sum::<usize>()
                / values.len().max(1);
            if best.is_none_or(|(_, held)| length > held) {
                best = Some((column, length));
            }
        }
        if let Some((column, _)) = best
            && let Some(slot) = columns.get_mut(column)
        {
            *slot = Some(Field::Name);
        }
    }
    columns
}

/// What a bare number in the Duration column most likely means.
fn guess_duration_unit(sheet: &Sheet, mapping: &Mapping) -> DurationUnit {
    let Some(column) = mapping.column_of(Field::Duration) else {
        return DurationUnit::Days;
    };
    let heading = crate::excel::key(&sheet.cell(mapping.heading_row, column).text());
    for (needle, unit) in [
        ("hour", DurationUnit::Hours),
        ("hr", DurationUnit::Hours),
        ("week", DurationUnit::Weeks),
        ("wk", DurationUnit::Weeks),
        ("month", DurationUnit::Months),
        ("min", DurationUnit::Minutes),
    ] {
        if heading.contains(needle) {
            return unit;
        }
    }
    DurationUnit::Days
}

// ----------------------------------------------------------------- report

/// One thing that could not be read, named precisely enough to go and look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// The row number the spreadsheet itself shows down its left edge.
    pub row: usize,
    pub heading: String,
    pub value: String,
    pub why: String,
}

/// Where the outline came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structure {
    /// Nothing said otherwise, so the plan comes in flat. That is a real
    /// answer, not a failure: plenty of lists are lists.
    Flat,
    FromColumn,
    FromIndent,
}

impl Structure {
    pub fn label(self) -> &'static str {
        match self {
            Structure::Flat => "Flat. Nothing in the sheet describes a hierarchy",
            Structure::FromColumn => "From the column mapped to Outline Level",
            Structure::FromIndent => "From the indentation in the name column",
        }
    }
}

/// Beyond this the list stops being a list and starts being the sheet again.
const MAX_NOTICES: usize = 200;

/// What an import will do, worked out before it does any of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub sheet: String,
    pub tasks: usize,
    /// Rows with nothing in them at all, which are formatting.
    pub blank_rows: usize,
    /// Rows that hold something but have no name: totals, banners, a stray
    /// note in the margin. Not tasks, and worth saying so.
    pub skipped_rows: usize,
    pub ignored: Vec<String>,
    pub notices: Vec<Notice>,
    pub unlisted_notices: usize,
    /// Dates that only the chosen day and month order settles.
    pub assumed_dates: usize,
    pub links: usize,
    pub dropped_links: usize,
    pub resources: usize,
    /// Work figures with nobody to carry them.
    pub work_unplaced: usize,
    pub structure: Structure,
    pub deepest: u16,
}

impl Report {
    fn new(sheet: String) -> Report {
        Report {
            sheet,
            tasks: 0,
            blank_rows: 0,
            skipped_rows: 0,
            ignored: Vec::new(),
            notices: Vec::new(),
            unlisted_notices: 0,
            assumed_dates: 0,
            links: 0,
            dropped_links: 0,
            resources: 0,
            work_unplaced: 0,
            structure: Structure::Flat,
            deepest: 0,
        }
    }

    fn note(&mut self, row: usize, heading: &str, value: &str, why: impl Into<String>) {
        if self.notices.len() >= MAX_NOTICES {
            self.unlisted_notices += 1;
            return;
        }
        self.notices.push(Notice {
            row,
            heading: heading.to_string(),
            value: value.to_string(),
            why: why.into(),
        });
    }
}

/// A plan read out of a sheet, and the account of how it was read.
pub struct Import {
    pub project: Project,
    pub report: Report,
}

// ---------------------------------------------------------------- reading

/// One row, read but not yet turned into a task.
///
/// Held back because the outline cannot be settled row by row: the level a row
/// gets depends on the shallowest row in the sheet and on the row above it, and
/// neither is known until every row has been looked at.
struct Draft {
    row: usize,
    id: String,
    name: String,
    indent: usize,
    level: Option<i64>,
    duration: Option<(i64, bool)>,
    start: Option<(NaiveDateTime, bool)>,
    finish: Option<(NaiveDateTime, bool)>,
    percent: Option<u8>,
    predecessors: String,
    resources: String,
    notes: String,
    cost: Option<f64>,
    work: Option<i64>,
}

fn read_percent(cell: &Cell) -> Option<f64> {
    match cell {
        // A spreadsheet stores 50% as 0.5 with a percent format on the cell,
        // and calamine hands back the 0.5. Anything at or under one is read as
        // that fraction, which does mean a literal "1" is read as finished
        // rather than as one percent. Of the two readings, that is the one
        // people mean.
        Cell::Number(value) => Some(if (0.0..=1.0).contains(value) {
            value * 100.0
        } else {
            *value
        }),
        Cell::Text(text) => {
            let trimmed = text.trim();
            let marked = trimmed.ends_with('%');
            let number: f64 = trimmed.trim_end_matches('%').trim().replace(',', "").parse().ok()?;
            Some(if marked || number > 1.0 {
                number
            } else {
                number * 100.0
            })
        }
        _ => None,
    }
}

fn read_money(cell: &Cell) -> Option<f64> {
    match cell {
        Cell::Number(value) => Some(*value),
        Cell::Text(text) => {
            let cleaned: String = text
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();
            cleaned.parse().ok()
        }
        _ => None,
    }
}

/// A duration cell, in whatever shape the sheet holds it.
fn read_span(cell: &Cell, default_unit: DurationUnit) -> Option<(i64, bool)> {
    match cell {
        Cell::Number(value) => {
            Some(((value * default_unit.minutes() as f64).round() as i64, false))
        }
        // `[hh]:mm` is real elapsed time, so it needs no unit guessed for it.
        Cell::Elapsed(minutes) => Some((*minutes, false)),
        Cell::Text(text) => crate::parse_duration_in(text, default_unit),
        _ => None,
    }
}

/// An outline level, as a number or as the depth of a WBS code.
fn read_level(cell: &Cell) -> Option<i64> {
    match cell {
        Cell::Number(value) => Some(*value as i64),
        Cell::Text(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.contains('.')
                && trimmed.chars().all(|c| c.is_ascii_digit() || c == '.')
            {
                // 1.2.3 is three deep. A WBS code says the same thing an
                // outline level does, in a different alphabet.
                return Some(trimmed.split('.').filter(|part| !part.is_empty()).count() as i64);
            }
            trimmed.parse().ok()
        }
        _ => None,
    }
}

/// How far a name is indented, counting a tab as four spaces because that is
/// what the export writes for one level.
fn indent_of(raw: &str) -> usize {
    raw.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Turn a date-only value into a working morning, the way `excel.rs` does.
fn at_morning(at: NaiveDateTime, date_only: bool) -> NaiveDateTime {
    if date_only {
        at.date().and_hms_opt(8, 0, 0).unwrap_or(at)
    } else {
        at
    }
}

/// Read a plan out of one sheet, following a mapping exactly.
///
/// Builds a whole plan of its own and hands it back. Nothing here touches the
/// plan that is open: a failed import has to leave that untouched, and the
/// surest way to manage that is to have no way of reaching it.
pub fn read(sheet: &Sheet, mapping: &Mapping, plan_name: &str) -> Result<Import, SheetError> {
    let name_at = mapping.column_of(Field::Name).ok_or(SheetError::NoName)?;
    let first_row = mapping.heading_row + 1;
    let mut report = Report::new(sheet.name.clone());

    // Which columns are being left behind, said out loud: a column quietly
    // dropped is the failure this page exists to prevent.
    for column in 0..sheet.width {
        if mapping.columns.get(column).copied().flatten().is_some() {
            continue;
        }
        let unheaded = sheet.cell(mapping.heading_row, column).text().is_empty();
        let no_data = sheet
            .rows
            .iter()
            .skip(first_row)
            .all(|row| row.get(column).is_none_or(Cell::is_empty));
        // A column with neither a heading nor a value is not being ignored. It
        // is not there.
        if unheaded && no_data {
            continue;
        }
        report.ignored.push(sheet.heading(mapping.heading_row, column));
    }

    let column_of = |field: Field| mapping.column_of(field);
    let heading_of = |column: usize| sheet.heading(mapping.heading_row, column);

    let mut drafts: Vec<Draft> = Vec::new();
    for (index, row) in sheet.rows.iter().enumerate().skip(first_row) {
        let number = index + 1;
        let raw_name = row.get(name_at).map(Cell::raw).unwrap_or_default();
        if raw_name.trim().is_empty() {
            if row.iter().all(Cell::is_empty) {
                report.blank_rows += 1;
            } else {
                report.skipped_rows += 1;
                let value = row
                    .iter()
                    .find(|cell| !cell.is_empty())
                    .map(Cell::text)
                    .unwrap_or_default();
                report.note(
                    number,
                    &heading_of(name_at),
                    &value,
                    "No name in this row, so it is not a task. Totals and banners land here.",
                );
            }
            continue;
        }

        let cell_of = |field: Field| -> &Cell {
            column_of(field)
                .and_then(|column| row.get(column))
                .unwrap_or(&Cell::Empty)
        };

        let mut draft = Draft {
            row: number,
            id: cell_of(Field::Id).text(),
            name: raw_name.trim().to_string(),
            indent: indent_of(&raw_name),
            level: None,
            duration: None,
            start: None,
            finish: None,
            percent: None,
            predecessors: cell_of(Field::Predecessors).text(),
            resources: cell_of(Field::Resources).text(),
            notes: cell_of(Field::Notes).text(),
            cost: None,
            work: None,
        };

        if let Some(column) = column_of(Field::OutlineLevel) {
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            if !cell.is_empty() {
                draft.level = read_level(cell);
                if draft.level.is_none() {
                    report.note(number, &heading_of(column), &cell.text(), "Not a level or a WBS code, so this row keeps the level of the row above it.");
                }
            }
        }

        if let Some(column) = column_of(Field::Duration) {
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            if !cell.is_empty() {
                draft.duration = read_span(cell, mapping.duration_unit);
                if draft.duration.is_none() {
                    report.note(
                        number,
                        &heading_of(column),
                        &cell.text(),
                        "Not a duration. The task comes in at one day.",
                    );
                }
            }
        }

        for field in [Field::Start, Field::Finish] {
            let Some(column) = column_of(field) else { continue };
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            match read_date(cell, mapping.date_order) {
                DateRead::Blank => {}
                DateRead::Certain { at, date_only } => {
                    if field == Field::Start {
                        draft.start = Some((at, date_only));
                    } else {
                        draft.finish = Some((at, date_only));
                    }
                }
                DateRead::Assumed { at, date_only } => {
                    report.assumed_dates += 1;
                    if field == Field::Start {
                        draft.start = Some((at, date_only));
                    } else {
                        draft.finish = Some((at, date_only));
                    }
                }
                DateRead::Unreadable => report.note(
                    number,
                    &heading_of(column),
                    &cell.text(),
                    "Not a date. The task is left for the scheduler to place.",
                ),
            }
        }

        if let Some(column) = column_of(Field::PercentComplete) {
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            if !cell.is_empty() {
                match read_percent(cell) {
                    Some(value) => draft.percent = Some(value.clamp(0.0, 100.0) as u8),
                    None => report.note(
                        number,
                        &heading_of(column),
                        &cell.text(),
                        "Not a percentage, so the task comes in as not started.",
                    ),
                }
            }
        }

        if let Some(column) = column_of(Field::Cost) {
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            if !cell.is_empty() {
                match read_money(cell) {
                    Some(value) => draft.cost = Some(value),
                    None => report.note(
                        number,
                        &heading_of(column),
                        &cell.text(),
                        "Not an amount of money, so no cost is booked.",
                    ),
                }
            }
        }

        if let Some(column) = column_of(Field::Work) {
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            if !cell.is_empty() {
                // Work is entered in hours everywhere else in the plan, so a
                // bare number here is hours rather than days.
                match read_span(cell, DurationUnit::Hours) {
                    Some((minutes, _)) => draft.work = Some(minutes),
                    None => report.note(
                        number,
                        &heading_of(column),
                        &cell.text(),
                        "Not an amount of work.",
                    ),
                }
            }
        }

        drafts.push(draft);
    }

    if drafts.is_empty() {
        return Err(SheetError::NoRows);
    }

    build(sheet, mapping, plan_name, drafts, report)
}

/// Turn the drafts into a plan, once every row has been seen.
fn build(
    sheet: &Sheet,
    mapping: &Mapping,
    plan_name: &str,
    drafts: Vec<Draft>,
    mut report: Report,
) -> Result<Import, SheetError> {
    let levels = outline_levels(&drafts, mapping, &mut report);

    let start = drafts
        .iter()
        .filter_map(|draft| draft.start.map(|(at, only)| at_morning(at, only)))
        .min()
        .unwrap_or_else(|| {
            NaiveDate::from_ymd_opt(2026, 1, 1)
                .and_then(|date| date.and_hms_opt(8, 0, 0))
                .unwrap_or_default()
        });

    let mut project = Project::blank(start);
    project.tasks.clear();
    project.name = plan_name.to_string();

    for (draft, level) in drafts.iter().zip(levels.iter()) {
        let mut duration = draft.duration.map(|(minutes, _)| minutes);
        let estimated = draft.duration.is_some_and(|(_, flag)| flag);

        // A pair of dates with no duration column is a duration: it is the
        // working time between them, which is the only reading that gets the
        // finish date back out again.
        if duration.is_none()
            && let (Some((from, from_only)), Some((to, only))) = (draft.start, draft.finish)
        {
            let from = at_morning(from, from_only);
            let span = project.calendar.work_minutes_between(from, to)
                + if only {
                    project.calendar.minutes_in_day(to.date())
                } else {
                    0
                };
            if span > 0 {
                duration = Some(span);
            } else {
                report.note(
                    draft.row,
                    Field::Finish.label(),
                    &to.format("%Y-%m-%d").to_string(),
                    "Finishes before it starts, so the dates cannot say how long it takes.",
                );
            }
        }

        let id = project.allocate_task_id();
        let mut task = Task::new(id, draft.name.clone(), duration.unwrap_or(480));
        task.estimated = estimated;
        task.outline_level = *level;
        task.notes = draft.notes.clone();
        if let Some(percent) = draft.percent {
            task.percent_complete = percent;
        }
        if let Some(cost) = draft.cost {
            task.fixed_cost = cost;
        }

        // A date somebody wrote down is a date they meant, so it becomes a
        // constraint rather than a suggestion. A finish with no start pins the
        // other end; a finish beside a duration column has nowhere to go but
        // the deadline, where it shows as a marker instead of being dropped.
        match (draft.start, draft.finish) {
            (Some((at, only)), _) => {
                task.mode = TaskMode::Auto;
                task.constraint = ConstraintType::StartNoEarlierThan;
                task.constraint_date = Some(at_morning(at, only));
            }
            (None, Some((at, only))) => {
                task.mode = TaskMode::Auto;
                task.constraint = ConstraintType::FinishNoLaterThan;
                task.constraint_date = Some(at_morning(at, only));
            }
            (None, None) => {}
        }
        if draft.duration.is_some()
            && let (Some(_), Some((at, only))) = (draft.start, draft.finish)
        {
            task.deadline = Some(at_morning(at, only));
        }

        project.tasks.push(task);
    }

    report.tasks = project.tasks.len();
    report.deepest = project.tasks.iter().map(|task| task.outline_level).max().unwrap_or(0);

    // People, once every row exists so a name used twice is one resource.
    for (index, draft) in drafts.iter().enumerate() {
        if !draft.resources.is_empty() {
            project.set_resource_text(index, &draft.resources);
        }
    }
    report.resources = project.resources.len();

    apply_work(&mut project, &drafts, &mut report);
    link(&mut project, &drafts, mapping, sheet, &mut report);

    Ok(Import { project, report })
}

/// Work the sheet gives, carried on whoever is doing the task.
///
/// Work in this plan is duration times units, so the only place a figure of
/// hours can live is on the assignments. A row with a figure and nobody on it
/// has nowhere to put it, and that is said in the report rather than swallowed.
fn apply_work(project: &mut Project, drafts: &[Draft], report: &mut Report) {
    for (index, draft) in drafts.iter().enumerate() {
        let Some(work) = draft.work else { continue };
        // Units written into the resource cell were typed on purpose, and
        // rewriting them from a work column would be this page overruling the
        // sheet rather than reading it.
        let stated = draft.resources.contains('[');
        let Some(task) = project.tasks.get_mut(index) else {
            continue;
        };
        let duration = task.duration_minutes;
        let people = task.assignments.len();
        if stated || people == 0 || duration <= 0 {
            report.work_unplaced += 1;
            continue;
        }
        let units = work as f64 / (duration as f64 * people as f64);
        for assignment in task.assignments.iter_mut() {
            assignment.units = units;
        }
    }
}

/// Turn the Predecessors column into links.
///
/// The numbers in that column count rows in the sheet, and the sheet's rows
/// are not the plan's: blank rows and banners were dropped on the way in. When
/// an ID column is mapped the numbers are looked up in it, which is exact.
/// Without one they are read as positions, which is what the exporter writes
/// and what the numbers usually are.
fn link(
    project: &mut Project,
    drafts: &[Draft],
    mapping: &Mapping,
    sheet: &Sheet,
    report: &mut Report,
) {
    let heading = mapping
        .column_of(Field::Predecessors)
        .map(|column| sheet.heading(mapping.heading_row, column))
        .unwrap_or_else(|| Field::Predecessors.label().to_string());

    let by_id: std::collections::HashMap<String, usize> = drafts
        .iter()
        .enumerate()
        .filter(|(_, draft)| !draft.id.is_empty())
        .map(|(index, draft)| (draft.id.clone(), index + 1))
        .collect();
    let use_ids = mapping.column_of(Field::Id).is_some() && !by_id.is_empty();

    for (index, draft) in drafts.iter().enumerate() {
        if draft.predecessors.is_empty() {
            continue;
        }
        let mut parts: Vec<String> = Vec::new();
        for token in draft.predecessors.split([',', ';']) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let digits: String = token.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() {
                report.dropped_links += 1;
                report.note(
                    draft.row,
                    &heading,
                    token,
                    "Not a row number, so this dependency was left out.",
                );
                continue;
            }
            let position = if use_ids {
                by_id.get(&digits).copied()
            } else {
                digits
                    .parse::<usize>()
                    .ok()
                    .filter(|row| *row >= 1 && *row <= drafts.len())
            };
            let Some(position) = position else {
                report.dropped_links += 1;
                report.note(
                    draft.row,
                    &heading,
                    token,
                    "No row in this import answers to that number, so the dependency was left out.",
                );
                continue;
            };
            // The tail carries the link type and any lag, which the plan
            // already knows how to read.
            parts.push(format!("{position}{}", &token[digits.len()..]));
        }
        if parts.is_empty() {
            continue;
        }
        let Some(id) = project.tasks.get(index).map(|task| task.id) else {
            continue;
        };
        project.set_predecessor_text(id, &parts.join(","));
    }
    report.links = project.links.len();
}

/// Work out the outline level of every row.
fn outline_levels(drafts: &[Draft], mapping: &Mapping, report: &mut Report) -> Vec<u16> {
    let has_column = mapping.column_of(Field::OutlineLevel).is_some()
        && drafts.iter().any(|draft| draft.level.is_some());
    let indents: Vec<usize> = drafts
        .iter()
        .map(|draft| draft.indent)
        .filter(|indent| *indent > 0)
        .collect();

    let mut raw: Vec<i64> = if has_column {
        report.structure = Structure::FromColumn;
        let mut last = 0i64;
        drafts
            .iter()
            .map(|draft| {
                // A row the column says nothing about sits where the row above
                // it sits. Inventing a level for it would invent structure.
                last = draft.level.unwrap_or(last);
                last
            })
            .collect()
    } else if !indents.is_empty() {
        report.structure = Structure::FromIndent;
        // One level is the smallest step the sheet actually uses, so four
        // spaces per level and two per level both come out right.
        let step = indents.iter().copied().fold(0usize, gcd).max(1);
        drafts
            .iter()
            .map(|draft| (draft.indent / step) as i64)
            .collect()
    } else {
        report.structure = Structure::Flat;
        vec![0; drafts.len()]
    };

    // The shallowest row in the sheet is the top of this plan, whether the
    // sheet counted from zero, from one, or from an indent nothing outdents.
    let floor = raw.iter().copied().min().unwrap_or(0);
    let mut previous = 0i64;
    for level in raw.iter_mut() {
        // A jump of more than one level has no parent to hang from, so it is
        // pulled in to the deepest level that does.
        let value = (*level - floor).max(0).min(previous + 1);
        previous = value;
        *level = value;
    }
    raw.into_iter().map(|level| level.clamp(0, u16::MAX as i64) as u16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Cell {
        if value.is_empty() {
            Cell::Empty
        } else {
            Cell::Text(value.to_string())
        }
    }

    /// A sheet of text cells, which is what a spreadsheet mostly is.
    fn sheet(rows: &[&[&str]]) -> Sheet {
        let rows: Vec<Vec<Cell>> = rows
            .iter()
            .map(|row| row.iter().map(|value| text(value)).collect())
            .collect();
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        Sheet {
            name: "Sheet1".into(),
            rows,
            width,
        }
    }

    fn import(sheet: &Sheet) -> Import {
        let mapping = Mapping::guess(sheet);
        read(sheet, &mapping, "Test plan").expect("read")
    }

    #[test]
    fn the_heading_row_is_found_under_a_block_of_title_and_logo() {
        // The shape of every plan anybody keeps in a spreadsheet.
        let book = sheet(&[
            &["Northwind Refit"],
            &[""],
            &["Prepared by Ana Reyes, March 2026"],
            &["Activity", "Owner", "From", "To"],
            &["Survey the site", "Ana", "02/03/2026", "06/03/2026"],
        ]);
        assert_eq!(guess_heading_row(&book), 3);
    }

    #[test]
    fn columns_are_guessed_from_somebody_elses_words_for_them() {
        let book = sheet(&[
            &["Activity", "Owner", "From", "To", "Notes", "Region"],
            &["Survey", "Ana", "02/03/2026", "06/03/2026", "east wall", "North"],
        ]);
        let mapping = Mapping::guess(&book);
        assert_eq!(mapping.column_of(Field::Name), Some(0));
        assert_eq!(mapping.column_of(Field::Resources), Some(1));
        assert_eq!(mapping.column_of(Field::Start), Some(2));
        assert_eq!(mapping.column_of(Field::Finish), Some(3));
        assert_eq!(mapping.column_of(Field::Notes), Some(4));
        assert_eq!(mapping.columns[5], None, "Region is nobody's field");
    }

    #[test]
    fn every_heading_the_exporter_writes_is_recognised_on_the_way_back() {
        // The two readers have to agree about what a heading means, or a
        // workbook this application wrote would come back through this page
        // mapped differently from the way it went out.
        for heading in crate::excel::COLUMNS {
            assert!(
                Field::for_heading(heading).is_some(),
                "the export writes {heading} and this cannot place it"
            );
        }
    }

    #[test]
    fn a_qualified_heading_still_names_its_field_and_a_lookalike_does_not() {
        assert_eq!(Field::for_heading("Duration (hours)"), Some(Field::Duration));
        assert_eq!(Field::for_heading("Start Date"), Some(Field::Start));
        assert_eq!(Field::for_heading("Planned Finish"), Some(Field::Finish));
        // The trap: a heading that begins with the name of a field and means
        // something else entirely.
        assert_eq!(Field::for_heading("Cost Centre"), None);
        assert_eq!(Field::for_heading("Start Location"), None);
        assert_eq!(Field::for_heading(""), None);
    }

    #[test]
    fn a_column_of_words_is_taken_for_the_name_when_no_heading_says_so() {
        let book = sheet(&[
            &["Ref", "What", "Days"],
            &["A-1", "Survey the site", "3"],
            &["A-2", "Pour the slab", "5"],
        ]);
        let mapping = Mapping::guess(&book);
        assert_eq!(mapping.column_of(Field::Name), Some(1));
    }

    // ---- dates -------------------------------------------------------

    #[test]
    fn an_ambiguous_date_follows_the_chosen_order_and_admits_it() {
        let cell = text("12/03/2026");
        let day = read_date(&cell, DateOrder::DayFirst);
        let month = read_date(&cell, DateOrder::MonthFirst);
        match (day, month) {
            (
                DateRead::Assumed { at: first, .. },
                DateRead::Assumed { at: second, .. },
            ) => {
                assert_eq!(first.date(), NaiveDate::from_ymd_opt(2026, 3, 12).unwrap());
                assert_eq!(second.date(), NaiveDate::from_ymd_opt(2026, 12, 3).unwrap());
            }
            other => panic!("an ambiguous date must say so: {other:?}"),
        }
    }

    #[test]
    fn a_date_that_settles_itself_ignores_the_setting() {
        // 25 cannot be a month, so the sheet has answered the question and the
        // setting has no business overruling it.
        for order in DateOrder::ALL {
            match read_date(&text("25/03/2026"), order) {
                DateRead::Certain { at, .. } => {
                    assert_eq!(at.date(), NaiveDate::from_ymd_opt(2026, 3, 25).unwrap());
                }
                other => panic!("{order:?} should not have been consulted: {other:?}"),
            }
        }
    }

    #[test]
    fn iso_and_written_out_dates_need_no_setting() {
        for value in ["2026-03-12", "12 Mar 2026", "Mar 12, 2026", "2026/03/12"] {
            match read_date(&text(value), DateOrder::MonthFirst) {
                DateRead::Certain { at, .. } => assert_eq!(
                    at.date(),
                    NaiveDate::from_ymd_opt(2026, 3, 12).unwrap(),
                    "{value}"
                ),
                other => panic!("{value} read as {other:?}"),
            }
        }
    }

    #[test]
    fn a_bare_number_in_a_date_column_is_a_serial() {
        // What an unformatted date cell holds. Counted from 1899-12-30.
        match read_date(&Cell::Number(46082.0), DateOrder::DayFirst) {
            DateRead::Certain { at, date_only } => {
                assert_eq!(at.date(), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
                assert!(date_only);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            read_date(&text("not a date"), DateOrder::DayFirst),
            DateRead::Unreadable
        ));
        assert!(matches!(
            read_date(&Cell::Empty, DateOrder::DayFirst),
            DateRead::Blank
        ));
    }

    #[test]
    fn a_time_of_day_survives_the_date_it_is_written_beside() {
        match read_date(&text("2026-03-12 14:30"), DateOrder::DayFirst) {
            DateRead::Certain { at, date_only } => {
                assert_eq!(at.time(), NaiveTime::from_hms_opt(14, 30, 0).unwrap());
                assert!(!date_only);
            }
            other => panic!("{other:?}"),
        }
        match read_date(&text("12/03/2026 2:30 PM"), DateOrder::DayFirst) {
            DateRead::Assumed { at, .. } => {
                assert_eq!(at.time(), NaiveTime::from_hms_opt(14, 30, 0).unwrap());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_sheet_can_prove_which_order_it_was_written_in() {
        let book = sheet(&[
            &["Task", "Start"],
            &["One", "25/03/2026"],
            &["Two", "12/03/2026"],
        ]);
        let mapping = Mapping::guess(&book);
        let evidence = mapping.evidence(&book);
        assert_eq!(evidence.proves_day_first, 1);
        assert_eq!(evidence.ambiguous, 1);
        assert_eq!(evidence.settled(), Some(DateOrder::DayFirst));
        assert_eq!(mapping.date_order, DateOrder::DayFirst);
    }

    #[test]
    fn a_sheet_that_contradicts_itself_settles_nothing() {
        let book = sheet(&[
            &["Task", "Start"],
            &["One", "25/03/2026"],
            &["Two", "03/25/2026"],
        ]);
        let evidence = Mapping::guess(&book).evidence(&book);
        assert!(evidence.contradictory());
        assert_eq!(evidence.settled(), None);
    }

    // ---- rows --------------------------------------------------------

    #[test]
    fn a_row_with_no_name_is_not_a_task() {
        let book = sheet(&[
            &["Task Name", "Duration"],
            &["Survey", "3d"],
            &["", ""],
            &["", "8d"],
            &["Pour", "5d"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.tasks, 2);
        assert_eq!(outcome.report.blank_rows, 1);
        assert_eq!(outcome.report.skipped_rows, 1, "the totals row");
        assert_eq!(outcome.report.notices.len(), 1);
        assert_eq!(outcome.report.notices[0].row, 4, "the sheet's own numbering");
    }

    #[test]
    fn indentation_becomes_the_outline() {
        let book = sheet(&[
            &["Task Name"],
            &["Phase one"],
            &["  Survey"],
            &["  Pour"],
            &["Phase two"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.structure, Structure::FromIndent);
        let levels: Vec<u16> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();
        assert_eq!(levels, vec![0, 1, 1, 0]);
    }

    #[test]
    fn an_outline_level_column_is_read_and_counted_from_the_top() {
        // A sheet that counts its own levels from one still comes in with a
        // root at zero, or the whole plan would sit under a level nobody wrote.
        let book = sheet(&[
            &["Task Name", "Level"],
            &["Phase one", "1"],
            &["Survey", "2"],
            &["Pour", "2"],
            &["Phase two", "1"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.structure, Structure::FromColumn);
        let levels: Vec<u16> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();
        assert_eq!(levels, vec![0, 1, 1, 0]);
    }

    #[test]
    fn a_wbs_column_gives_its_depth() {
        let book = sheet(&[
            &["Task Name", "WBS"],
            &["Phase one", "1"],
            &["Survey", "1.1"],
            &["Detail", "1.1.1"],
        ]);
        let outcome = import(&book);
        let levels: Vec<u16> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();
        assert_eq!(levels, vec![0, 1, 2]);
    }

    #[test]
    fn a_level_that_jumps_is_pulled_back_to_a_level_that_has_a_parent() {
        let book = sheet(&[
            &["Task Name", "Level"],
            &["Phase one", "1"],
            &["Buried", "4"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.project.tasks[1].outline_level, 1);
    }

    #[test]
    fn a_sheet_that_describes_no_hierarchy_comes_in_flat() {
        // Absent is a real answer. Inventing structure here would be inventing
        // a plan the sheet does not contain.
        let book = sheet(&[&["Task Name"], &["One"], &["Two"]]);
        let outcome = import(&book);
        assert_eq!(outcome.report.structure, Structure::Flat);
        assert!(
            outcome
                .project
                .tasks
                .iter()
                .all(|task| task.outline_level == 0)
        );
    }

    // ---- fields ------------------------------------------------------

    #[test]
    fn durations_arrive_in_every_shape_somebody_writes_them() {
        let book = sheet(&[
            &["Task Name", "Duration"],
            &["Bare", "5"],
            &["Suffixed", "5d"],
            &["Spelled", "5 days"],
            &["Weekly", "1w"],
            &["Hourly", "8 hrs"],
        ]);
        let outcome = import(&book);
        let minutes: Vec<i64> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.duration_minutes)
            .collect();
        assert_eq!(minutes, vec![2400, 2400, 2400, 2400, 480]);
    }

    #[test]
    fn a_duration_column_headed_hours_means_hours() {
        let book = sheet(&[&["Task Name", "Duration (hours)"], &["Survey", "8"]]);
        let mut mapping = Mapping::guess(&book);
        // The heading names a unit, so a bare 8 is a day rather than eight.
        assert_eq!(mapping.duration_unit, DurationUnit::Hours);
        let outcome = read(&book, &mapping, "Test").expect("read");
        assert_eq!(outcome.project.tasks[0].duration_minutes, 480);
        // And the person importing has the last word.
        mapping.duration_unit = DurationUnit::Days;
        let outcome = read(&book, &mapping, "Test").expect("read");
        assert_eq!(outcome.project.tasks[0].duration_minutes, 3840);
    }

    #[test]
    fn two_dates_and_no_duration_column_give_the_duration() {
        // Monday to Friday is five days, not four: a finish date names a day
        // of work, not the moment work stops.
        let book = sheet(&[
            &["Task Name", "Start", "Finish"],
            &["Survey", "2026-03-02", "2026-03-06"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.project.tasks[0].duration_minutes, 2400);
        assert_eq!(
            outcome.project.tasks[0].constraint,
            ConstraintType::StartNoEarlierThan
        );
    }

    #[test]
    fn a_finish_beside_a_duration_becomes_a_deadline_rather_than_being_dropped() {
        let book = sheet(&[
            &["Task Name", "Duration", "Start", "Finish"],
            &["Survey", "3d", "2026-03-02", "2026-03-06"],
        ]);
        let outcome = import(&book);
        let task = &outcome.project.tasks[0];
        assert_eq!(task.duration_minutes, 1440);
        assert!(task.deadline.is_some());
    }

    #[test]
    fn a_finish_with_no_start_pins_the_other_end() {
        let book = sheet(&[&["Task Name", "Finish"], &["Survey", "2026-03-06"]]);
        let outcome = import(&book);
        assert_eq!(
            outcome.project.tasks[0].constraint,
            ConstraintType::FinishNoLaterThan
        );
    }

    #[test]
    fn percentages_arrive_as_fractions_and_as_percentages() {
        let book = sheet(&[
            &["Task Name", "% Complete"],
            &["Half by fraction", ""],
            &["Half by percent", "50%"],
            &["Whole", "100"],
        ]);
        let mut rows = book.rows.clone();
        // A percent formatted cell comes back as the fraction it stores.
        rows[1][1] = Cell::Number(0.5);
        let book = Sheet { rows, ..book };
        let outcome = import(&book);
        let done: Vec<u8> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.percent_complete)
            .collect();
        assert_eq!(done, vec![50, 50, 100]);
    }

    #[test]
    fn money_arrives_with_its_symbols_stripped() {
        let book = sheet(&[&["Task Name", "Cost"], &["Survey", "$1,250.50"]]);
        let outcome = import(&book);
        assert!((outcome.project.tasks[0].fixed_cost - 1250.50).abs() < 0.01);
    }

    #[test]
    fn work_lands_on_whoever_is_doing_the_task() {
        // Work in this plan is duration times units, so the only place hours
        // can live is on the assignment.
        let book = sheet(&[
            &["Task Name", "Duration", "Resources", "Work"],
            &["Survey", "5d", "Ana", "20 hrs"],
            &["Pour", "5d", "", "40 hrs"],
        ]);
        let outcome = import(&book);
        let assignment = outcome.project.tasks[0].assignments[0];
        assert!((assignment.units - 0.5).abs() < 0.001, "half time for a week");
        assert_eq!(outcome.report.work_unplaced, 1, "nobody to carry the second");
    }

    #[test]
    fn resources_named_twice_are_one_person() {
        let book = sheet(&[
            &["Task Name", "Assigned To"],
            &["Survey", "Ana Reyes"],
            &["Pour", "ana reyes"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.resources, 1);
    }

    #[test]
    fn notes_come_across() {
        let book = sheet(&[
            &["Task Name", "Comments"],
            &["Survey", "Access from the east gate only"],
        ]);
        let outcome = import(&book);
        assert_eq!(
            outcome.project.tasks[0].notes,
            "Access from the east gate only"
        );
    }

    // ---- links -------------------------------------------------------

    #[test]
    fn predecessors_follow_the_sheets_own_numbering_when_it_is_mapped() {
        // The sheet numbers 10, 20, 30 and has a banner row in the middle, so
        // reading those numbers as positions would link the wrong rows or
        // nothing at all.
        let book = sheet(&[
            &["ID", "Task Name", "Predecessors"],
            &["10", "Survey", ""],
            &["", "Phase two", ""],
            &["20", "Pour", "10"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 1);
        let link = outcome.project.links[0];
        assert_eq!(link.predecessor, outcome.project.tasks[0].id);
        assert_eq!(link.successor, outcome.project.tasks[2].id);
    }

    #[test]
    fn a_predecessor_that_points_nowhere_is_left_out_and_said_so() {
        let book = sheet(&[
            &["Task Name", "Predecessors"],
            &["Survey", ""],
            &["Pour", "9"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 0);
        assert_eq!(outcome.report.dropped_links, 1);
        assert!(!outcome.report.notices.is_empty());
    }

    #[test]
    fn a_link_keeps_its_type_and_its_lag() {
        let book = sheet(&[
            &["Task Name", "Predecessors"],
            &["Survey", ""],
            &["Pour", "1SS+2d"],
        ]);
        let outcome = import(&book);
        let link = outcome.project.links[0];
        assert_eq!(link.kind, crate::model::LinkType::SS);
        assert_eq!(link.lag_minutes, 960);
    }

    // ---- the account of it -------------------------------------------

    #[test]
    fn columns_left_behind_are_named() {
        let book = sheet(&[
            &["Task Name", "Region", "Cost Centre"],
            &["Survey", "North", "4402"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.ignored, vec!["Region", "Cost Centre"]);
    }

    #[test]
    fn an_unreadable_date_is_reported_and_the_task_still_arrives() {
        let book = sheet(&[
            &["Task Name", "Start"],
            &["Survey", "whenever they let us in"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.tasks, 1);
        assert_eq!(outcome.report.notices.len(), 1);
        assert!(outcome.project.tasks[0].constraint_date.is_none());
    }

    #[test]
    fn assumed_dates_are_counted_so_the_page_can_own_up_to_them() {
        let book = sheet(&[
            &["Task Name", "Start"],
            &["One", "01/03/2026"],
            &["Two", "2026-03-02"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.assumed_dates, 1);
    }

    #[test]
    fn nothing_mapped_to_the_name_is_an_error_rather_than_an_empty_plan() {
        let book = sheet(&[&["Fruit"], &["Apple"]]);
        let mut mapping = Mapping::guess(&book);
        mapping.assign(0, None);
        assert!(matches!(
            read(&book, &mapping, "Test"),
            Err(SheetError::NoName)
        ));
    }

    #[test]
    fn a_sheet_of_headings_and_nothing_else_says_so() {
        let book = sheet(&[&["Task Name", "Duration"]]);
        let mapping = Mapping::guess(&book);
        assert!(matches!(
            read(&book, &mapping, "Test"),
            Err(SheetError::NoRows)
        ));
    }

    #[test]
    fn one_field_cannot_come_from_two_columns() {
        let book = sheet(&[&["Task Name", "Activity"], &["a", "b"]]);
        let mut mapping = Mapping::guess(&book);
        mapping.assign(1, Some(Field::Name));
        assert_eq!(mapping.columns[0], None);
        assert_eq!(mapping.column_of(Field::Name), Some(1));
    }

    #[test]
    fn columns_are_named_the_way_the_spreadsheet_names_them() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(5), "F");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(27), "AB");
    }

    // ---- the file itself ---------------------------------------------

    #[test]
    fn a_real_workbook_is_surveyed_sheet_by_sheet() {
        use rust_xlsxwriter::Workbook;

        let dir = std::env::temp_dir().join(format!("aop-sheet-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join("foreign.xlsx");

        let mut book = Workbook::new();
        let first = book.add_worksheet().set_name("Cover").expect("named");
        first.write_string(0, 0, "Northwind Refit").expect("written");
        let second = book.add_worksheet().set_name("Programme").expect("named");
        for (column, value) in ["Activity", "From", "To"].iter().enumerate() {
            second
                .write_string(0, column as u16, *value)
                .expect("written");
        }
        second.write_string(1, 0, "Survey the site").expect("written");
        second.write_string(1, 1, "25/03/2026").expect("written");
        second.write_string(1, 2, "27/03/2026").expect("written");
        book.save(&path).expect("saved");

        let sheets = survey(&path).expect("surveyed");
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[1].name, "Programme");

        let outcome = import(&sheets[1]);
        assert_eq!(outcome.report.tasks, 1);
        assert_eq!(outcome.project.tasks[0].name, "Survey the site");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_is_not_a_workbook_is_refused_rather_than_read() {
        let dir = std::env::temp_dir().join(format!("aop-sheet-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let path = dir.join("notes.txt");
        std::fs::write(&path, "this is not a workbook").expect("written");
        assert!(matches!(survey(&path), Err(SheetError::NotAWorkbook(_))));
        let _ = std::fs::remove_file(&path);
    }
}
