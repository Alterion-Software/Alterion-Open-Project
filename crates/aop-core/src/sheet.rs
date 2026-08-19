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

/// The earliest date this will believe somebody typed.
///
/// An empty or zeroed date cell comes out of a workbook as a serial at or near
/// zero, which lands in the last two days of 1899. Nobody plans work then, and
/// one of them getting through is not a wrong date on one task: the plan's own
/// start is the earliest start in the sheet, so a single zero drags the whole
/// timescale back a century and every report with it.
const EARLIEST_REAL_DATE: (i32, u32, u32) = (1900, 1, 2);

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
/// outline level, which it writes as indentation, and notes. WBS is not a
/// field of its own because a WBS code is a rendering of the outline level
/// rather than a second fact, so a column of them maps to Outline Level and is
/// read for its depth. It is read for its identity as well: the codes in that
/// column are what a Predecessors column full of `4.2.31.1` is citing, and a
/// sheet that cites them has no other identifier to offer.
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
    /// The other end of the same relationship, which some sheets keep beside
    /// the first. Importing one means creating the link on the other task.
    Successors,
    Resources,
    Work,
    Cost,
    Notes,
}

impl Field {
    pub const ALL: [Field; 13] = [
        Field::Id,
        Field::Name,
        Field::OutlineLevel,
        Field::Duration,
        Field::Start,
        Field::Finish,
        Field::PercentComplete,
        Field::Predecessors,
        Field::Successors,
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
            Field::Successors => "Successors",
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
            Field::Predecessors => "What it waits for, by row number or WBS code",
            Field::Successors => "What waits for it, by row number or WBS code",
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
            Field::Successors => "successors",
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
                // A whole heading, so it beats the trimming pass below, which
                // would otherwise drop "Days" and call this column Work. A
                // sheet that counts a task in work days is counting how long
                // it takes, not how much effort it carries.
                "Work Days",
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
            Field::Successors => &[
                "Successors",
                "Successor",
                "Followed By",
                "Feeds",
                "Blocks",
                "Leads To",
                "Precedes",
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
    /// A heading is tried whole, then as the shorter headings it is a longer
    /// way of writing: the words before a bracket, the words inside it, and
    /// each side of a slash. Every one of those is matched whole against the
    /// alias lists first, and only then with a trailing unit or qualifier
    /// taken off.
    ///
    /// The order is the whole point, because the useful word is on either side
    /// of the bracket depending on the sheet. "Duration (hours)" is settled by
    /// the words outside, and has to be, or the word inside would make it
    /// Work. "In Dependencies (Predecessors)" is settled only by the words
    /// inside, because nothing outside names a field.
    ///
    /// What none of this does is match a word anywhere in a heading. That is
    /// what would make a Cost Centre column a Cost column, and a wrong guess
    /// that looks deliberate is worse than no guess at all.
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

        // The heading itself, then the ways it can be read as a longer way of
        // naming one column. Outside the bracket before inside it, for the
        // reason above.
        let mut readings: Vec<&str> = vec![heading];
        if let Some(open) = heading.find('(') {
            readings.push(&heading[..open]);
            let inside = &heading[open + 1..];
            let close = inside.find(')').unwrap_or(inside.len());
            readings.push(&inside[..close]);
        }
        // A heading that offers two words for one column names it twice, and
        // either word will do: "Notes / Comments" is the Notes column.
        if heading.contains('/') {
            readings.extend(heading.split('/'));
        }

        for reading in &readings {
            if let Some(field) = named(&crate::excel::key(reading)) {
                return Some(field);
            }
        }

        // Only now the trailing units and qualifiers, so an exact name always
        // beats a trimmed one.
        for reading in &readings {
            let mut words: Vec<String> = reading
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(|word| word.to_lowercase())
                .collect();
            while words.len() > 1 {
                if !QUALIFIERS.contains(&words[words.len() - 1].as_str()) {
                    break;
                }
                words.pop();
                if let Some(field) = named(&words.concat()) {
                    return Some(field);
                }
            }
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
    match read_any_date(cell, order) {
        DateRead::Certain { at, .. } | DateRead::Assumed { at, .. } if !is_real_date(at) => {
            // Absent rather than unreadable: there was nothing in the cell to
            // read, only a zero the spreadsheet stored for an empty one.
            DateRead::Blank
        }
        other => other,
    }
}

/// Whether a date is one somebody could have meant.
fn is_real_date(at: NaiveDateTime) -> bool {
    NaiveDate::from_ymd_opt(
        EARLIEST_REAL_DATE.0,
        EARLIEST_REAL_DATE.1,
        EARLIEST_REAL_DATE.2,
    )
    .is_none_or(|floor| at.date() >= floor)
}

fn read_any_date(cell: &Cell, order: DateOrder) -> DateRead {
    match cell {
        Cell::Empty => return DateRead::Blank,
        Cell::Stamp { at, date_only } => {
            return DateRead::Certain {
                at: *at,
                date_only: *date_only,
            };
        }
        Cell::Number(serial) => {
            // A serial at or before the spreadsheet's own day zero is what an
            // empty or zeroed cell holds, so it is absent rather than wrong.
            if *serial < 1.0 {
                return DateRead::Blank;
            }
            // The same upper bound `excel.rs` uses, which reaches to 2064 and
            // keeps a duration from being read as a date.
            if *serial >= 60_000.0 {
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

/// The deepest outline a plan can really have.
///
/// Twenty is already deeper than anybody reads, and the cap is here for a
/// reason beyond taste: a plan cannot be indented past what its own grid can
/// show, and a depth that runs away is how an import turns a hundred rows into
/// a hundred levels with one task each.
pub const MAX_OUTLINE_DEPTH: u16 = 20;

/// The largest number that can plausibly be a level, or a top level WBS
/// branch.
///
/// This is the guard that matters. `1230258` in the outline column is an issue
/// number somebody kept there, not a level and not a branch, and reading it as
/// one is what used to set every following row climbing a level at a time.
const IMPLAUSIBLE_ABOVE: i64 = 999;

/// How many of a column's values are worth reading before its shape is clear.
const SHAPE_SAMPLE: usize = 400;

/// What a column's own values say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shaped {
    /// Dotted codes at more than one depth. That is an outline, whatever the
    /// heading over it happens to say.
    Wbs,
    /// Plain numbers that climb. That is an identifier, and nothing else.
    Counter,
    /// Neither shape is clear, so the heading is left to decide.
    Unsettled,
}

/// How deep a WBS code is, if the text is one at all.
///
/// `1.2.3` is three deep, `DM33.1.1` is three deep, and a bare `4` is a top
/// level branch, so one. A value with a space in it is a sentence rather than
/// a code, and a bare number too large to be a branch is a reference number
/// somebody kept in the same column.
fn wbs_depth(text: &str) -> Option<i64> {
    let trimmed = text.trim().trim_end_matches('.');
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    let coded = |part: &&str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    if !parts.iter().all(coded) {
        return None;
    }
    if parts.len() == 1 {
        let only = parts[0];
        if only.chars().all(|c| c.is_ascii_digit()) {
            return only
                .parse::<i64>()
                .ok()
                .filter(|value| *value <= IMPLAUSIBLE_ABOVE)
                .map(|_| 1);
        }
    }
    Some(parts.len() as i64)
}

/// Read a column's values to work out whether it is a WBS or an identifier.
///
/// A heading cannot answer this and should not be asked to. "No." heads a
/// column of `1`, `1.1`, `1.1.1` in one sheet and a column of `1`, `2`, `3` in
/// the next, and the difference between those two readings is the difference
/// between a plan with an outline and a flat list of fifteen hundred rows.
///
/// So the values decide. Dotted codes at more than one depth are an outline:
/// nothing else varies its own depth down a column. Plain numbers that only
/// climb are an identifier. Anything else is left unsettled, which means the
/// heading keeps whatever it guessed and the person importing can say
/// otherwise on the page.
fn column_shape(sheet: &Sheet, heading_row: usize, column: usize) -> Shaped {
    let mut depths: Vec<i64> = Vec::new();
    let mut dotted = 0usize;
    let mut plain = 0usize;
    let mut climbing = true;
    let mut highest: Option<i64> = None;
    let mut looked = 0usize;

    for row in sheet.rows.iter().skip(heading_row + 1) {
        let Some(cell) = row.get(column) else { continue };
        if cell.is_empty() {
            continue;
        }
        looked += 1;
        if looked > SHAPE_SAMPLE {
            break;
        }
        let value = cell.text();
        let digits = !value.is_empty() && value.chars().all(|c| c.is_ascii_digit());
        match wbs_depth(&value) {
            Some(depth) if depth > 1 => {
                dotted += 1;
                if !depths.contains(&depth) {
                    depths.push(depth);
                }
            }
            Some(_) if digits => {
                plain += 1;
                if !depths.contains(&1) {
                    depths.push(1);
                }
            }
            _ => {}
        }
        if digits {
            let value: i64 = value.parse().unwrap_or(0);
            if highest.is_some_and(|held| value <= held) {
                climbing = false;
            }
            highest = Some(value);
        } else {
            // A column of identifiers is numbers all the way down. One value
            // that is not a number is enough to say it is not counting.
            climbing = false;
        }
    }

    // A quarter of the numbers being dotted is enough: a plan's top level is
    // undotted by definition, and a deep plan has plenty of both.
    if dotted >= 3 && depths.len() >= 2 && dotted * 4 >= dotted + plain {
        return Shaped::Wbs;
    }
    if dotted == 0 && plain >= 3 && climbing {
        return Shaped::Counter;
    }
    Shaped::Unsettled
}

/// The column that holds the sheet's WBS codes, if one does.
///
/// Worked out from the values every time rather than remembered on the
/// mapping, so a column chosen by hand on the Import page reads exactly the
/// same way as one this guessed.
fn wbs_column(sheet: &Sheet, mapping: &Mapping) -> Option<usize> {
    let column = mapping.column_of(Field::OutlineLevel)?;
    match column_shape(sheet, mapping.heading_row, column) {
        Shaped::Wbs => Some(column),
        Shaped::Counter | Shaped::Unsettled => None,
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

    // Now the values, for the one question a heading cannot answer. A column
    // of dotted codes at varying depth is the outline, whatever it is called,
    // and the cost of missing it is the whole hierarchy: a sheet whose "No."
    // column is read as an identifier imports as fifteen hundred rows at level
    // zero.
    if !taken.contains(&Field::OutlineLevel) {
        let found = (0..sheet.width).find(|column| {
            // Only a column nothing else wants, or one the heading placed as
            // an identifier, which is the mistake being corrected. A column
            // already holding names or dates is not up for reconsideration.
            matches!(columns.get(*column).copied().flatten(), None | Some(Field::Id))
                && column_shape(sheet, heading_row, *column) == Shaped::Wbs
        });
        if let Some(column) = found {
            // Nothing is lost by taking it off Id. A WBS column is also the
            // identity a Predecessors column cites, and the reader looks it up
            // there, so the codes still resolve.
            if columns.get(column).copied().flatten() == Some(Field::Id) {
                taken.retain(|field| *field != Field::Id);
            }
            taken.push(Field::OutlineLevel);
            if let Some(slot) = columns.get_mut(column) {
                *slot = Some(Field::OutlineLevel);
            }
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
    /// References that name nothing this import holds, or that the two
    /// dependency columns disagree about.
    pub dropped_links: usize,
    /// Links refused because taking them would have made a plan that cannot be
    /// scheduled at all.
    pub looped_links: usize,
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
            looped_links: 0,
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
    /// The row's WBS code, when the outline column holds codes rather than
    /// numbers. Kept beside the id because it is an identity as much as a
    /// depth: it is what a Predecessors column full of `4.2.31.1` is citing,
    /// and in a sheet like that it is the only identity there is.
    code: String,
    name: String,
    indent: usize,
    level: Option<i64>,
    duration: Option<(i64, bool)>,
    start: Option<(NaiveDateTime, bool)>,
    finish: Option<(NaiveDateTime, bool)>,
    percent: Option<u8>,
    predecessors: String,
    successors: String,
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

/// The longest a task can plausibly be, in working minutes: a hundred years.
///
/// Nothing in a plan lasts that long, and a number in a duration column that
/// says so is a date serial or a rolled up total that landed in the wrong
/// column. Believing one does not stretch a single bar: every summary above it
/// stretches too, and so does the plan's own finish, so one bad cell can put
/// the timescale a century out.
const IMPLAUSIBLE_MINUTES: i64 = 100 * 260 * 480;

/// A duration cell, in whatever shape the sheet holds it.
fn read_span(cell: &Cell, default_unit: DurationUnit) -> Option<(i64, bool)> {
    let read = match cell {
        Cell::Number(value) => {
            Some(((value * default_unit.minutes() as f64).round() as i64, false))
        }
        // `[hh]:mm` is real elapsed time, so it needs no unit guessed for it.
        Cell::Elapsed(minutes) => Some((*minutes, false)),
        Cell::Text(text) => crate::parse_duration_in(text, default_unit),
        _ => None,
    };
    read.filter(|(minutes, _)| minutes.abs() <= IMPLAUSIBLE_MINUTES)
}

/// An outline level, as a number or as the depth of a WBS code.
///
/// `wbs` says the column was read as a column of codes, in which case every
/// value in it is one and the answer is how deep it is. Otherwise the value is
/// a level as it stands, and a dotted one is still counted for its depth.
///
/// Either way a value too large to be a level is refused rather than believed.
/// The caller reads `None` as "this row says nothing", and a row that says
/// nothing keeps the level of the row above it, which is the only safe answer:
/// believing `1230258` is how a sheet ends up eighty levels deep.
fn read_level(cell: &Cell, wbs: bool) -> Option<i64> {
    if wbs {
        return wbs_depth(&cell.text());
    }
    match cell {
        Cell::Number(value) => {
            let value = *value as i64;
            (0..=IMPLAUSIBLE_ABOVE).contains(&value).then_some(value)
        }
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
            trimmed
                .parse::<i64>()
                .ok()
                .filter(|value| (0..=IMPLAUSIBLE_ABOVE).contains(value))
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
    // Settled once for the sheet rather than row by row, because it is a fact
    // about the column and reading it per row would be reading the same four
    // hundred cells for every one of fifteen hundred rows.
    let coded = wbs_column(sheet, mapping);

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
            code: coded
                .and_then(|column| row.get(column))
                .map(Cell::text)
                .unwrap_or_default(),
            name: raw_name.trim().to_string(),
            indent: indent_of(&raw_name),
            level: None,
            duration: None,
            start: None,
            finish: None,
            percent: None,
            predecessors: cell_of(Field::Predecessors).text(),
            successors: cell_of(Field::Successors).text(),
            resources: cell_of(Field::Resources).text(),
            notes: cell_of(Field::Notes).text(),
            cost: None,
            work: None,
        };

        if let Some(column) = column_of(Field::OutlineLevel) {
            let cell = row.get(column).unwrap_or(&Cell::Empty);
            if !cell.is_empty() {
                draft.level = read_level(cell, coded.is_some());
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

/// One reference out of a dependency cell.
///
/// `key` is the code or row number it names and `tail` is the link type and
/// lag in the shape `parse_predecessor_text` already reads, so whichever way
/// round the sheet wrote them, they leave here written one way.
struct Reference {
    key: String,
    tail: String,
}

/// Whether what follows a row number is a link type and a lag rather than more
/// of the reference.
///
/// This is the difference between `475SS`, which is row 475 start to start,
/// and `4.2.31.1`, which is a WBS code that happens to start with a digit.
/// Reading the second as row 4 is not a near miss: it lands every one of those
/// references on whatever task is fourth, which in a plan means one summary
/// task collecting hundreds of dependencies and a dependency loop by lunchtime.
fn is_a_tail(rest: &str) -> bool {
    if rest.is_empty() {
        return true;
    }
    let upper = rest.to_ascii_uppercase();
    let after = match upper.get(..2) {
        Some(head) if crate::model::LinkType::parse(head).is_some() => &upper[2..],
        _ => upper.as_str(),
    };
    after.is_empty() || after.starts_with('+') || after.starts_with('-')
}

/// Split a dependency cell into the references it names.
///
/// Sheets write these every way there is, and one sheet writes them several
/// ways: `12`, `12SS+2d`, `4.2.31.1 Complete Draft IDD [FS]`, and any number
/// of those to a cell separated by a comma, a semicolon or a line break. What
/// is constant is that the reference is the first word and the type is either
/// stuck to it or spelled out in brackets at the end.
fn references(text: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    for token in text.split([',', ';', '\n', '\r']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        // A type in brackets at the end, which is how a sheet that also writes
        // the task's name into the cell keeps the two apart.
        let (body, bracketed) = match (token.rfind('['), token.rfind(']')) {
            (Some(open), Some(close)) if close > open => {
                let inside = token[open + 1..close].trim().to_ascii_uppercase();
                match crate::model::LinkType::parse(&inside) {
                    Some(kind) => (token[..open].trim(), Some(kind)),
                    None => (token, None),
                }
            }
            _ => (token, None),
        };
        let Some(word) = body.split_whitespace().next() else {
            continue;
        };
        if let Some(kind) = bracketed {
            out.push(Reference {
                key: word.trim_end_matches('.').to_string(),
                tail: kind.code().to_string(),
            });
            continue;
        }
        let digits: String = word.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &word[digits.len()..];
        if !digits.is_empty() && is_a_tail(rest) {
            out.push(Reference {
                key: digits,
                tail: rest.to_string(),
            });
        } else {
            out.push(Reference {
                key: word.trim_end_matches('.').to_string(),
                tail: String::new(),
            });
        }
    }
    out
}

/// What the rows of a sheet answer to, so a reference can find one.
struct Identities {
    /// Code, upper cased, to the 1-based position of the row that carries it.
    by_code: std::collections::HashMap<String, usize>,
    /// The row number the spreadsheet shows down its left edge, to the same.
    by_row: std::collections::HashMap<usize, usize>,
    /// Whether the sheet has any identity of its own at all.
    named: bool,
    rows: usize,
}

impl Identities {
    fn of(drafts: &[Draft], mapping: &Mapping) -> Identities {
        let mut by_code: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut named = false;
        // The id column first, so it wins where a sheet has both and they
        // disagree: it is the column somebody put there to be cited.
        if mapping.column_of(Field::Id).is_some() {
            for (index, draft) in drafts.iter().enumerate() {
                if draft.id.is_empty() {
                    continue;
                }
                named = true;
                by_code
                    .entry(draft.id.to_ascii_uppercase())
                    .or_insert(index + 1);
            }
        }
        for (index, draft) in drafts.iter().enumerate() {
            if draft.code.is_empty() {
                continue;
            }
            named = true;
            by_code
                .entry(draft.code.to_ascii_uppercase())
                .or_insert(index + 1);
        }
        let by_row = drafts
            .iter()
            .enumerate()
            .map(|(index, draft)| (draft.row, index + 1))
            .collect();
        Identities {
            by_code,
            by_row,
            named,
            rows: drafts.len(),
        }
    }

    /// The 1-based position a reference names, if any row answers to it.
    ///
    /// The sheet's own names first, then two readings of a bare number, and
    /// which of the two applies depends on whether the sheet names its rows at
    /// all.
    ///
    /// A sheet that names them and then writes a bare number is not naming a
    /// row, because its names are not numbers. It is pointing at the number
    /// down the left edge of the spreadsheet, which is what somebody reading
    /// the file sees. A sheet that names nothing is counting its own task rows
    /// from one, which is what this application's own export writes.
    ///
    /// A number that answers to neither reading is left out and counted.
    /// Counting it off as a position anyway would pick a task very nearly at
    /// random and call it a dependency, and enough of those in one plan is a
    /// dependency loop, which is a plan that cannot be scheduled at all.
    fn find(&self, key: &str) -> Option<usize> {
        if let Some(position) = self.by_code.get(&key.to_ascii_uppercase()) {
            return Some(*position);
        }
        if !key.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let number: usize = key.parse().ok()?;
        if self.named {
            return self.by_row.get(&number).copied();
        }
        (number >= 1 && number <= self.rows).then_some(number)
    }
}

/// The links being built, and whether one more would close a loop.
///
/// A plan whose links form a loop cannot be scheduled at all: every date in it
/// stays at zero and the window opens on a complaint instead of a plan. A
/// sheet is quite capable of describing one, especially a sheet whose two
/// dependency columns were kept by different people. So a loop is refused
/// here, where the rows that caused it can still be named, rather than carried
/// into a plan that will not run.
///
/// The awkward part is that the loop has to be looked for the way the
/// scheduler will look for it. A link naming a summary is not one link there:
/// it is expanded onto every leaf under that summary. So a link from a task to
/// its own parent is a loop even though the two are different rows, and
/// checking the links as written would miss it entirely.
struct Building {
    /// Links as written, from row position to row position.
    out: Vec<Vec<usize>>,
    /// The predecessor text each 0-based position has collected.
    incoming: Vec<Vec<String>>,
    /// Pairs already linked, in the direction they were linked.
    joined: std::collections::HashSet<(usize, usize)>,
    /// The leaf rows under each row, which is what a link really joins.
    leaves_of: Vec<Vec<usize>>,
    /// For each leaf row, itself and every summary above it: the rows whose
    /// links that leaf therefore carries.
    owners: Vec<Vec<usize>>,
}

impl Building {
    fn new(levels: &[u16]) -> Building {
        let rows = levels.len();
        let is_leaf = |row: usize| {
            levels
                .get(row + 1)
                .is_none_or(|next| *next <= levels[row])
        };
        let mut leaves_of: Vec<Vec<usize>> = Vec::with_capacity(rows);
        for row in 0..rows {
            let mut under = Vec::new();
            for below in row + 1..rows {
                if levels[below] <= levels[row] {
                    break;
                }
                if is_leaf(below) {
                    under.push(below);
                }
            }
            // A row with nothing under it stands for itself.
            if under.is_empty() {
                under.push(row);
            }
            leaves_of.push(under);
        }
        let mut owners: Vec<Vec<usize>> = vec![Vec::new(); rows];
        for (row, under) in leaves_of.iter().enumerate() {
            for leaf in under {
                owners[*leaf].push(row);
            }
        }
        Building {
            out: vec![Vec::new(); rows],
            incoming: vec![Vec::new(); rows],
            joined: std::collections::HashSet::new(),
            leaves_of,
            owners,
        }
    }

    fn already(&self, from: usize, to: usize) -> bool {
        self.joined.contains(&(from, to))
    }

    /// Whether a leaf under `from` can already be reached by following links
    /// out of `to`, which is what would make a link from `from` to `to` a loop.
    ///
    /// The walk is over leaves, because that is the graph the scheduler builds.
    /// A leaf carries the links of every summary it sits under, and a link
    /// arriving at a summary arrives at all of its leaves.
    fn reaches(&self, to: usize, from: usize) -> bool {
        let target: std::collections::HashSet<usize> =
            self.leaves_of[from].iter().copied().collect();
        let mut seen = vec![false; self.out.len()];
        let mut stack: Vec<usize> = self.leaves_of[to].clone();
        while let Some(leaf) = stack.pop() {
            if target.contains(&leaf) {
                return true;
            }
            if std::mem::replace(&mut seen[leaf], true) {
                continue;
            }
            for owner in &self.owners[leaf] {
                for next in &self.out[*owner] {
                    stack.extend(self.leaves_of[*next].iter().copied());
                }
            }
        }
        false
    }

    fn add(&mut self, from: usize, to: usize, tail: &str) {
        self.out[from].push(to);
        self.joined.insert((from, to));
        self.incoming[to].push(format!("{}{tail}", from + 1));
    }
}

/// Turn the dependency columns into links.
///
/// The references in those columns are the sheet's own, not the plan's: they
/// name rows by whatever the sheet calls them, which is a WBS code in one sheet
/// and a row number in the next, and blank rows and banners mean the sheet's
/// numbering is not this plan's either way. So every reference is resolved to a
/// position here, and only then handed to the plan, which reads positions.
///
/// Successors are the same relationship written from the other end, so one is
/// imported by creating the link on the other task. Where a sheet has both
/// columns and they disagree about a pair, the predecessors column wins: it is
/// read first, and the successors column then finds the pair already joined.
/// Two columns that disagree are one relationship written twice, not two
/// relationships, and taking both would make a loop out of a typo.
fn link(
    project: &mut Project,
    drafts: &[Draft],
    mapping: &Mapping,
    sheet: &Sheet,
    report: &mut Report,
) {
    let heading_for = |field: Field| {
        mapping
            .column_of(field)
            .map(|column| sheet.heading(mapping.heading_row, column))
            .unwrap_or_else(|| field.label().to_string())
    };
    let known = Identities::of(drafts, mapping);
    let levels: Vec<u16> = project.tasks.iter().map(|task| task.outline_level).collect();
    let mut building = Building::new(&levels);

    // Predecessors first, then successors, so that the order in which a
    // disagreement is settled is the same every time this runs.
    for reading in [Field::Predecessors, Field::Successors] {
        let heading = heading_for(reading);
        for (index, draft) in drafts.iter().enumerate() {
            let cell = match reading {
                Field::Predecessors => &draft.predecessors,
                _ => &draft.successors,
            };
            if cell.is_empty() {
                continue;
            }
            for reference in references(cell) {
                let Some(position) = known.find(&reference.key) else {
                    report.dropped_links += 1;
                    report.note(
                        draft.row,
                        &heading,
                        &reference.key,
                        "No row in this import answers to that, so the dependency was left out.",
                    );
                    continue;
                };
                let other = position - 1;
                // Which end of the link this row is depends on which column
                // the reference came out of.
                let (from, to) = match reading {
                    Field::Predecessors => (other, index),
                    _ => (index, other),
                };
                if from == to {
                    report.dropped_links += 1;
                    report.note(
                        draft.row,
                        &heading,
                        &reference.key,
                        "A task cannot depend on itself, so this was left out.",
                    );
                    continue;
                }
                if building.already(from, to) {
                    // The two columns agreeing about a pair. Nothing to do and
                    // nothing worth saying.
                    continue;
                }
                if building.already(to, from) {
                    report.dropped_links += 1;
                    report.note(
                        draft.row,
                        &heading,
                        &reference.key,
                        "The other dependency column already has these two the other way round, so this one was left out.",
                    );
                    continue;
                }
                if building.reaches(to, from) {
                    report.looped_links += 1;
                    report.note(
                        draft.row,
                        &heading,
                        &reference.key,
                        "Following this dependency comes back to this row, and a plan whose \
                         dependencies form a loop cannot be scheduled at all, so it was left out.",
                    );
                    continue;
                }
                building.add(from, to, &reference.tail);
            }
        }
    }

    for (index, parts) in building.incoming.iter().enumerate() {
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

    // `None` means the row said nothing, which is a different thing from
    // saying zero and has to stay different all the way down: it is what makes
    // a row inherit rather than invent.
    let raw: Vec<Option<i64>> = if has_column {
        report.structure = Structure::FromColumn;
        drafts.iter().map(|draft| draft.level).collect()
    } else if !indents.is_empty() {
        report.structure = Structure::FromIndent;
        // One level is the smallest step the sheet actually uses, so four
        // spaces per level and two per level both come out right.
        let step = indents.iter().copied().fold(0usize, gcd).max(1);
        drafts
            .iter()
            .map(|draft| Some((draft.indent / step) as i64))
            .collect()
    } else {
        report.structure = Structure::Flat;
        vec![Some(0); drafts.len()]
    };

    // The shallowest row that said anything is the top of this plan, whether
    // the sheet counted from zero, from one, or from an indent nothing
    // outdents. Rows that said nothing are not in the reckoning: they have no
    // level of their own to be the shallowest.
    let floor = raw.iter().flatten().copied().min().unwrap_or(0);
    let mut previous = 0i64;
    let mut levels = Vec::with_capacity(raw.len());
    for value in raw {
        let level = match value {
            // A jump of more than one level has no parent to hang from, so it
            // is pulled in to the deepest level that does.
            Some(level) => (level - floor).max(0).min(previous + 1),
            // A row the column says nothing about sits where the row above it
            // sits. Note that it inherits the level as settled, not the raw
            // value: inheriting the raw one is what let a single absurd value
            // set every following row climbing a level at a time, for the rest
            // of the sheet.
            None => previous,
        };
        let level = level.clamp(0, MAX_OUTLINE_DEPTH as i64);
        previous = level;
        levels.push(level as u16);
    }
    levels
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
    fn a_heading_can_carry_its_useful_word_on_either_side_of_a_bracket() {
        // The two shapes, in one sheet: "Duration (hours)" says what it is
        // outside the bracket and qualifies it inside, and "In Dependencies
        // (Predecessors)" does the opposite. Reading either one from the wrong
        // side is silent: the first would come in as Work and the second as
        // nothing at all.
        assert_eq!(Field::for_heading("Duration (hours)"), Some(Field::Duration));
        assert_eq!(
            Field::for_heading("In Dependencies (Predecessors)"),
            Some(Field::Predecessors)
        );
        assert_eq!(
            Field::for_heading("Out Dependencies (Successors)"),
            Some(Field::Successors)
        );
        // Two words for one column, which is a heading naming it twice.
        assert_eq!(Field::for_heading("Notes / Comments"), Some(Field::Notes));
        // And none of that loosens the matcher. These are the headings that
        // must stay unplaced, whatever is bracketed onto them.
        assert_eq!(Field::for_heading("Cost Centre"), None);
        assert_eq!(Field::for_heading("Cost Centre (Total)"), None);
        assert_eq!(Field::for_heading("Realistic End Date"), None);
        assert_eq!(Field::for_heading("Start Location"), None);
    }

    #[test]
    fn work_days_is_how_long_a_task_takes_and_not_how_much_effort_it_carries() {
        // "Work" and "Days" separately name two different fields, and the
        // trimming pass would drop the second and answer Work. The whole
        // heading is a duration in days, which is what people write it for.
        assert_eq!(Field::for_heading("Work Days"), Some(Field::Duration));
        assert_eq!(Field::for_heading("Work"), Some(Field::Work));
    }

    // ---- what a column holds, when its heading will not say -----------

    /// A sheet of names beside one column of values, which is what the
    /// questions about a column's shape are all about.
    fn column(heading: &str, values: &[&str]) -> Sheet {
        let mut rows: Vec<Vec<Cell>> = vec![vec![text("Task Name"), text(heading)]];
        rows.extend(
            values
                .iter()
                .map(|value| vec![text("A task"), text(value)]),
        );
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        Sheet {
            name: "Sheet1".into(),
            rows,
            width,
        }
    }

    #[test]
    fn a_wbs_column_is_the_outline_however_it_is_headed() {
        // The defect this exists for. Column A of a real plan is headed "No."
        // and holds the whole outline, the heading matches the ID aliases, and
        // the plan imports flat: fifteen hundred rows at level zero.
        let book = column("No.", &["1", "1.1", "1.1.1", "1.2", "1.2.1", "1.2.2", "2"]);
        let mapping = Mapping::guess(&book);
        assert_eq!(mapping.column_of(Field::OutlineLevel), Some(1));
        assert_eq!(mapping.column_of(Field::Id), None, "it is not an identifier");
        let outcome = read(&book, &mapping, "Test plan").expect("read");
        let levels: Vec<u16> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();
        assert_eq!(levels, vec![0, 1, 2, 1, 2, 2, 0]);
    }

    #[test]
    fn a_column_of_counting_numbers_is_an_identifier_however_it_is_headed() {
        // The other half of the same question. Nothing here varies its depth,
        // so there is no outline in it to find.
        let book = column("No.", &["1", "2", "3", "4", "5", "6", "7"]);
        let mapping = Mapping::guess(&book);
        assert_eq!(mapping.column_of(Field::Id), Some(1));
        assert_eq!(mapping.column_of(Field::OutlineLevel), None);
    }

    #[test]
    fn a_column_of_codes_all_the_same_depth_is_left_to_its_heading() {
        // Version numbers, part numbers, clause numbers. Dotted, but the depth
        // never varies, so nothing in them describes a hierarchy and this
        // declines to invent one. The heading keeps the last word.
        let book = column("No.", &["1.1", "1.2", "1.3", "2.1", "2.2", "2.3"]);
        let mapping = Mapping::guess(&book);
        assert_eq!(mapping.column_of(Field::Id), Some(1));
        assert_eq!(mapping.column_of(Field::OutlineLevel), None);
    }

    #[test]
    fn a_wbs_column_is_found_even_where_no_heading_names_it() {
        let book = column("Ref", &["1", "1.1", "1.1.1", "1.2", "2", "2.1", "2.1.1"]);
        let mapping = Mapping::guess(&book);
        assert_eq!(mapping.column_of(Field::OutlineLevel), Some(1));
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
    fn a_zeroed_date_cell_is_absent_rather_than_a_century_out() {
        // A spreadsheet writes an empty or zeroed date cell as a serial at or
        // near zero, which reads back as the last days of 1899. Believing one
        // costs more than one wrong task: the plan's own start is the earliest
        // start in the sheet, so a single zero drags the whole timescale back
        // a hundred and twenty years.
        assert_eq!(read_date(&Cell::Number(0.0), DateOrder::DayFirst), DateRead::Blank);
        assert_eq!(read_date(&Cell::Number(1.0), DateOrder::DayFirst), DateRead::Blank);
        assert_eq!(read_date(&Cell::Number(2.0), DateOrder::DayFirst), DateRead::Blank);
        // And a real date is still a real date.
        assert!(matches!(
            read_date(&text("2026-03-02"), DateOrder::DayFirst),
            DateRead::Certain { .. }
        ));
    }

    #[test]
    fn a_date_serial_that_landed_in_the_duration_column_is_not_a_duration() {
        // 33000 working days is a hundred and twenty years. Nothing lasts that
        // long, and believing it stretches every summary above the task and
        // the plan's own finish with them.
        let book = sheet(&[
            &["Task Name", "Work Days"],
            &["Survey", "5"],
            &["Pasted", "33000"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.project.tasks[0].duration_minutes, 5 * 480);
        assert_eq!(
            outcome.project.tasks[1].duration_minutes,
            480,
            "one day, and said so, rather than a century"
        );
        assert!(
            outcome
                .report
                .notices
                .iter()
                .any(|notice| notice.value == "33000")
        );
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
    fn an_unreadable_outline_value_inherits_rather_than_burrowing() {
        // The runaway. One issue number in the outline column used to be
        // believed, every row after it inherited that number, and the level
        // then climbed by one a row for the rest of the sheet: levels eight to
        // eighty four with a single task in each.
        let book = sheet(&[
            &["Task Name", "No."],
            &["Phase one", "1"],
            &["Survey", "1.1"],
            &["Detail", "1.1.1"],
            &["Ticket", "1230258"],
            &["Also ticket", "1229221"],
            &["Still ticket", "1215785"],
            &["Phase two", "2"],
            &["Dig", "2.1"],
        ]);
        let outcome = import(&book);
        let levels: Vec<u16> = outcome
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();
        assert_eq!(
            levels,
            vec![0, 1, 2, 2, 2, 2, 0, 1],
            "the three rows that say nothing keep the level of the row above them"
        );
        assert_eq!(outcome.report.deepest, 2);
    }

    #[test]
    fn the_outline_is_capped_at_a_depth_a_plan_can_really_have() {
        let deep: String = std::iter::repeat_n("1", 40).collect::<Vec<_>>().join(".");
        let book = sheet(&[
            &["Task Name", "WBS"],
            &["Top", "1"],
            &["Under", "1.1"],
            &["Buried", &deep],
        ]);
        let outcome = import(&book);
        assert!(
            outcome.report.deepest <= MAX_OUTLINE_DEPTH,
            "got {}",
            outcome.report.deepest
        );
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

    #[test]
    fn a_dependency_naming_a_wbs_code_finds_the_row_that_carries_it() {
        // And, just as importantly, does not find row four. Reading the
        // leading digit of "4.2.31.1" as a row number is not a near miss: it
        // lands every reference in the sheet on one summary task, and a plan
        // of those is a plan that will not schedule.
        let book = sheet(&[
            &["No.", "Task Name", "In Dependencies (Predecessors)"],
            &["4", "Design", ""],
            &["4.1", "Draft", ""],
            &["4.2", "Build", ""],
            &["4.2.31", "Wire it up", ""],
            &["4.2.31.1", "Test it", ""],
            &["5", "Hand over", "4.2.31.1 Test it [FS]"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 1);
        let link = outcome.project.links[0];
        assert_eq!(link.predecessor, outcome.project.tasks[4].id, "Test it");
        assert_eq!(link.successor, outcome.project.tasks[5].id, "Hand over");
    }

    #[test]
    fn a_reference_that_names_nothing_is_left_out_and_counted() {
        let book = sheet(&[
            &["No.", "Task Name", "Predecessors"],
            &["1", "Survey", ""],
            &["1.1", "Dig", ""],
            &["1.2", "Pour", "9.9.9, \u{25c6}"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 0);
        assert_eq!(outcome.report.dropped_links, 2);
    }

    #[test]
    fn a_successor_column_creates_the_link_on_the_other_task() {
        // The reverse of a predecessor, so importing one means giving the link
        // to the row it names rather than to the row it is written on.
        let book = sheet(&[
            &["Task Name", "Out Dependencies (Successors)"],
            &["Survey", "3"],
            &["Dig", ""],
            &["Pour", ""],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 1);
        let link = outcome.project.links[0];
        assert_eq!(link.predecessor, outcome.project.tasks[0].id, "Survey");
        assert_eq!(link.successor, outcome.project.tasks[2].id, "Pour");
    }

    #[test]
    fn the_two_dependency_columns_agreeing_is_one_link_and_not_two() {
        let book = sheet(&[
            &["Task Name", "Predecessors", "Successors"],
            &["Survey", "", "2"],
            &["Dig", "1", ""],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 1);
        assert_eq!(outcome.report.dropped_links, 0, "agreement is not a problem");
    }

    #[test]
    fn the_two_dependency_columns_disagreeing_keeps_the_predecessor() {
        // One relationship written twice, the second time backwards. Taking
        // both would be a loop of two, which is a plan that cannot be
        // scheduled, so the predecessors column wins and the other is said.
        // The same two rows, joined both ways round by the same row's own two
        // cells: Survey waits for Dig, and Survey is followed by Dig.
        let book = sheet(&[
            &["Task Name", "Predecessors", "Successors"],
            &["Survey", "2", "2"],
            &["Dig", "", ""],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 1);
        let link = outcome.project.links[0];
        assert_eq!(link.predecessor, outcome.project.tasks[1].id, "Dig");
        assert_eq!(link.successor, outcome.project.tasks[0].id, "Survey");
        assert_eq!(outcome.report.dropped_links, 1);
        assert!(
            outcome
                .report
                .notices
                .iter()
                .any(|notice| notice.why.contains("other way round"))
        );
    }

    #[test]
    fn a_dependency_that_would_close_a_loop_is_refused_and_named() {
        let book = sheet(&[
            &["Task Name", "Predecessors"],
            &["Survey", "3"],
            &["Dig", "1"],
            &["Pour", "2"],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 2, "the loop is broken, not the plan");
        assert_eq!(outcome.report.looped_links, 1);
        assert!(
            crate::schedule::schedule(&mut outcome.project.clone()).is_ok(),
            "a plan that came out of this has to be schedulable"
        );
    }

    #[test]
    fn a_dependency_on_a_row_that_contains_it_is_a_loop_and_is_refused() {
        // The scheduler expands a link naming a summary onto every leaf under
        // it, so a task waiting for its own parent is waiting for itself. The
        // two are different rows, so nothing but the outline says otherwise.
        let book = sheet(&[
            &["Task Name", "Level", "Predecessors"],
            &["Phase one", "1", ""],
            &["Survey", "2", "1"],
            &["Dig", "2", ""],
        ]);
        let outcome = import(&book);
        assert_eq!(outcome.report.links, 0);
        assert_eq!(outcome.report.looped_links, 1);
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
