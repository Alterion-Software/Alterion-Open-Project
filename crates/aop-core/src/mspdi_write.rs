//! Writing a plan as Microsoft Project XML.
//!
//! The other half of `mspdi`. Project's own `.mpp` is an undocumented binary,
//! so MSPDI is the only honest route into Project: it opens a `.xml` with
//! File -> Open, gets a real editable plan, and can save it as `.mpp` itself.
//!
//! Two things govern what is written here. The first is `mspdi`'s reader,
//! which is the working specification of what has to survive: anything it
//! parses is written, so a plan can go out and come back. The second is the
//! Project Data Interchange schema, whose element order is a `xsd:sequence`
//! rather than a set. Project reports nothing useful when it refuses a file,
//! so the order below follows the schema exactly and is not to be rearranged
//! for tidiness.

use std::collections::HashMap;
use std::fmt::Display;
use std::io;

use chrono::{NaiveDateTime, NaiveTime};
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

use crate::calendar::{CalendarException, DayShifts, WorkCalendar};
use crate::earned_value::EarnedValueMethod;
use crate::model::{
    ConstraintType, LinkType, Project, Resource, ResourceId, ResourceKind, ScheduleFrom, Task,
    TaskMode,
};
use crate::{MINUTES_PER_DAY, MINUTES_PER_WEEK};

#[derive(Debug)]
pub enum ExportError {
    Io(io::Error),
    /// The document was built but is not valid UTF-8, which can only happen if
    /// the writer itself is wrong.
    Encoding(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(error) => write!(f, "{error}"),
            ExportError::Encoding(detail) => {
                write!(f, "The Project XML could not be written: {detail}")
            }
        }
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(error: io::Error) -> Self {
        ExportError::Io(error)
    }
}

/// The namespace Project writes on the root element and looks for on the way
/// back in. Not the schema's own target namespace, which carries a year.
const NAMESPACE: &str = "http://schemas.microsoft.com/project";

/// Project 2010, the version this document claims to have been saved by.
///
/// Not 2007, because two of the things the plan has to carry, whether a task
/// is active and whether it is scheduled by hand, arrived in 2010 and are not
/// in the 2007 schema at all. A schema sequence only ever grows at the end, so
/// those two are written last in the task, after everything 2007 knows about.
const SAVE_VERSION: u32 = 14;

/// `DurationFormat` and `LagFormat` code for days, and for days-with-a-question
/// mark, which is how Project shows an estimated duration.
const FORMAT_DAYS: u32 = 7;
const FORMAT_DAYS_ESTIMATED: u32 = 39;

/// Project's `Type` code for a task whose duration is what the planner typed.
/// This engine has no notion of effort-driven work, so every task is one.
const TASK_TYPE_FIXED_DURATION: u32 = 1;

/// `FixedCostAccrual`: prorated, Project's own default.
const ACCRUE_PRORATED: u32 = 2;

/// `StandardRateFormat` and `OvertimeRateFormat`: per hour, which is what a
/// rate in this model means.
const RATE_PER_HOUR: u32 = 2;

/// `WorkFormat`: hours.
const WORK_FORMAT_HOURS: u32 = 2;

/// A calendar exception written as a single non-recurring run of days.
const EXCEPTION_TYPE_DAILY: u32 = 1;

/// The schema allows no more than five working times on one day.
const MAX_WORKING_TIMES: usize = 5;

/// Write a plan to a file as Project XML.
pub fn save(path: &std::path::Path, project: &Project) -> Result<(), ExportError> {
    let xml = to_xml(project)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, xml)?;
    Ok(())
}

/// Render a plan as a Project XML document.
///
/// Separate from `save` because it is the whole of the work and none of the
/// side effects, which is what the round trip tests exercise.
pub fn to_xml(project: &Project) -> Result<String, ExportError> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    write_document(&mut writer, project)?;
    String::from_utf8(writer.into_inner()).map_err(|e| ExportError::Encoding(e.to_string()))
}

// ------------------------------------------------------------- identity

/// Which UID everything is written under.
///
/// MSPDI refers to a task by `UID` rather than by row, and a link names both
/// ends, so the same answer has to come out everywhere. The plan's own ids are
/// unique and non-zero, so they are used directly; the map exists for the one
/// case they are not, since UID 0 is reserved for the project summary row and
/// a task written under it would be dropped on the way back in.
struct Ids {
    tasks: HashMap<crate::model::TaskId, u32>,
    resources: HashMap<ResourceId, u32>,
}

impl Ids {
    fn build(project: &Project) -> Self {
        let mut tasks = HashMap::new();
        let mut spare = project
            .tasks
            .iter()
            .map(|task| task.id)
            .max()
            .unwrap_or(0)
            .max(1);
        for task in &project.tasks {
            let uid = if task.id == 0 {
                spare += 1;
                spare
            } else {
                task.id
            };
            tasks.insert(task.id, uid);
        }

        let mut resources = HashMap::new();
        let mut spare = project
            .resources
            .iter()
            .map(|resource| resource.id)
            .max()
            .unwrap_or(0)
            .max(1);
        for resource in &project.resources {
            let uid = if resource.id == 0 {
                spare += 1;
                spare
            } else {
                resource.id
            };
            resources.insert(resource.id, uid);
        }

        Self { tasks, resources }
    }
}

/// Every calendar the file will hold, and the UID each one answers to.
///
/// This model keeps a person's own time off on the person rather than in the
/// calendar library, because leave is not something the organisation shares.
/// MSPDI has no such place, so one is made here: a derived calendar naming the
/// person, based on whichever library calendar they follow and listing nothing
/// but their exceptions. Leaving its `WeekDays` out is what makes it derived
/// rather than a base, and is what lets the reader unpick it again.
struct Calendars<'a> {
    /// In write order, so UIDs come out in the order they were handed out.
    entries: Vec<CalendarEntry<'a>>,
    project_uid: u32,
    by_name: HashMap<&'a str, u32>,
    personal: HashMap<ResourceId, u32>,
}

struct CalendarEntry<'a> {
    uid: u32,
    name: &'a str,
    /// `None` for a person's own calendar, which states only its exceptions.
    week: Option<&'a [DayShifts]>,
    exceptions: &'a [CalendarException],
    /// The calendar this one is derived from, or `None` for a base.
    base: Option<u32>,
}

impl<'a> Calendars<'a> {
    fn build(project: &'a Project) -> Self {
        let mut entries = Vec::new();
        let mut by_name: HashMap<&str, u32> = HashMap::new();
        let mut next = 1u32;

        let project_uid = next;
        next += 1;
        by_name.insert(project.calendar.name.as_str(), project_uid);
        entries.push(CalendarEntry {
            uid: project_uid,
            name: project.calendar.name.as_str(),
            week: Some(project.calendar.week.as_slice()),
            exceptions: project.calendar.exceptions.as_slice(),
            base: None,
        });

        for calendar in &project.calendars {
            let uid = next;
            next += 1;
            // Two library calendars cannot normally share a name, but a plan
            // that has been through a merge could; the first keeps the name so
            // anything pointing at it still resolves, and the second is still
            // written so its content is not lost.
            by_name.entry(calendar.name.as_str()).or_insert(uid);
            entries.push(CalendarEntry {
                uid,
                name: calendar.name.as_str(),
                week: Some(calendar.week.as_slice()),
                exceptions: calendar.exceptions.as_slice(),
                base: None,
            });
        }

        let mut personal = HashMap::new();
        for resource in &project.resources {
            if resource.calendar_exceptions.is_empty() {
                continue;
            }
            let uid = next;
            next += 1;
            personal.insert(resource.id, uid);
            entries.push(CalendarEntry {
                uid,
                name: resource.name.as_str(),
                week: None,
                exceptions: resource.calendar_exceptions.as_slice(),
                base: Some(
                    by_name
                        .get(resource.base_calendar.as_str())
                        .copied()
                        .unwrap_or(project_uid),
                ),
            });
        }

        Self {
            entries,
            project_uid,
            by_name,
            personal,
        }
    }

    /// The UID a named calendar answers to, falling back to the project's for
    /// a name the library has lost. That is the same fallback
    /// `Project::calendar_or_project` makes, so the file says what the plan
    /// meant rather than what it happened to have written down.
    fn named(&self, name: &str) -> u32 {
        if name.trim().is_empty() {
            return self.project_uid;
        }
        self.by_name
            .get(name)
            .copied()
            .unwrap_or(self.project_uid)
    }

    /// The calendar a person keeps to: their own if they have time off of
    /// their own, otherwise whichever base they follow.
    fn for_resource(&self, resource: &Resource) -> u32 {
        self.personal
            .get(&resource.id)
            .copied()
            .unwrap_or_else(|| self.named(&resource.base_calendar))
    }
}

// --------------------------------------------------------------- writing

type Xml = Writer<Vec<u8>>;

fn open(w: &mut Xml, name: &str) -> io::Result<()> {
    w.write_event(Event::Start(BytesStart::new(name)))
}

fn open_with(w: &mut Xml, name: &str, attribute: (&str, &str)) -> io::Result<()> {
    let mut tag = BytesStart::new(name);
    tag.push_attribute(attribute);
    w.write_event(Event::Start(tag))
}

fn close(w: &mut Xml, name: &str) -> io::Result<()> {
    w.write_event(Event::End(BytesEnd::new(name)))
}

/// A text element. `BytesText::new` escapes, which is the whole reason this
/// module builds XML rather than formatting strings: a task name is free text
/// and routinely holds `&`, `<`, quotes and newlines.
fn text(w: &mut Xml, name: &str, value: &str) -> io::Result<()> {
    w.create_element(name)
        .write_text_content(BytesText::new(value))?;
    Ok(())
}

/// A text element, written only when there is something to say. MSPDI omits
/// what holds its default, and so does this.
fn text_if(w: &mut Xml, name: &str, value: &str) -> io::Result<()> {
    if value.trim().is_empty() {
        return Ok(());
    }
    text(w, name, value)
}

fn number(w: &mut Xml, name: &str, value: impl Display) -> io::Result<()> {
    text(w, name, &value.to_string())
}

/// A decimal, written so it reads back as the same number.
fn decimal(w: &mut Xml, name: &str, value: f64) -> io::Result<()> {
    let value = if value.is_finite() { value } else { 0.0 };
    text(w, name, &format!("{value}"))
}

fn flag(w: &mut Xml, name: &str, value: bool) -> io::Result<()> {
    text(w, name, if value { "1" } else { "0" })
}

/// MSPDI writes a moment as local time with no zone and no fraction.
fn datetime(w: &mut Xml, name: &str, value: NaiveDateTime) -> io::Result<()> {
    text(w, name, &value.format("%Y-%m-%dT%H:%M:%S").to_string())
}

fn datetime_if(w: &mut Xml, name: &str, value: Option<NaiveDateTime>) -> io::Result<()> {
    match value {
        Some(value) => datetime(w, name, value),
        None => Ok(()),
    }
}

fn time(w: &mut Xml, name: &str, value: NaiveTime) -> io::Result<()> {
    text(w, name, &value.format("%H:%M:%S").to_string())
}

/// MSPDI writes a duration as an ISO 8601 period.
///
/// Always as hours and minutes, never as days: the reader has to guess how
/// long a day is when it sees `P1D`, and eight hours written as `PT8H0M0S`
/// leaves it nothing to guess at.
fn iso_duration(minutes: i64) -> String {
    let minutes = minutes.max(0);
    format!("PT{}H{}M0S", minutes / 60, minutes % 60)
}

fn duration(w: &mut Xml, name: &str, minutes: i64) -> io::Result<()> {
    text(w, name, &iso_duration(minutes))
}

// ------------------------------------------------------------- the plan

fn write_document(w: &mut Xml, project: &Project) -> io::Result<()> {
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), Some("yes"))))?;

    let ids = Ids::build(project);
    let calendars = Calendars::build(project);

    open_with(w, "Project", ("xmlns", NAMESPACE))?;
    write_header(w, project, &calendars)?;
    write_calendars(w, &calendars)?;
    write_tasks(w, project, &ids, &calendars)?;
    write_resources(w, project, &ids, &calendars)?;
    write_assignments(w, project, &ids)?;
    close(w, "Project")?;
    Ok(())
}

fn write_header(w: &mut Xml, project: &Project, calendars: &Calendars) -> io::Result<()> {
    number(w, "SaveVersion", SAVE_VERSION)?;
    text(w, "Name", &project.name)?;
    text(w, "Title", &project.name)?;
    text_if(w, "Company", &project.company)?;
    text_if(w, "Author", &project.author)?;
    flag(
        w,
        "ScheduleFromStart",
        project.schedule_from == ScheduleFrom::ProjectStartDate,
    )?;
    datetime(w, "StartDate", project.start_date)?;
    datetime(w, "FinishDate", project.finish_date)?;
    // Project states this limit in whole days.
    number(w, "CriticalSlackLimit", project.critical_slack_minutes / MINUTES_PER_DAY)?;
    number(w, "CurrencyDigits", 2)?;
    text_if(w, "CurrencySymbol", &project.currency_symbol)?;
    // Required by the schema even though this model has only a symbol, and a
    // symbol is not a code. Project shows the symbol, so the code costs
    // nothing but has to be there.
    text(w, "CurrencyCode", currency_code(&project.currency_symbol))?;
    // The app writes money as symbol then amount, with no space between.
    number(w, "CurrencySymbolPosition", 0)?;
    number(w, "CalendarUID", calendars.project_uid)?;

    // The day the project calendar starts and ends, so Project agrees with
    // this plan about what a date with no time on it means.
    let (start, finish) = default_day(&project.calendar);
    time(w, "DefaultStartTime", start)?;
    time(w, "DefaultFinishTime", finish)?;

    // Duration in this engine is working time, and these three are what turn
    // a period back into a number of days on the other side. Exporting a
    // duration without them produces a plan that says the wrong thing.
    number(w, "MinutesPerDay", MINUTES_PER_DAY)?;
    number(w, "MinutesPerWeek", MINUTES_PER_WEEK)?;
    number(w, "DaysPerMonth", crate::MINUTES_PER_MONTH / MINUTES_PER_DAY)?;

    number(w, "DefaultTaskType", TASK_TYPE_FIXED_DURATION)?;
    number(w, "DefaultFixedCostAccrual", ACCRUE_PRORATED)?;
    number(w, "DurationFormat", FORMAT_DAYS)?;
    number(w, "WorkFormat", WORK_FORMAT_HOURS)?;
    flag(w, "HonorConstraints", true)?;
    datetime_if(w, "StatusDate", project.status_date)?;
    datetime(w, "CurrentDate", project.current_date)?;
    Ok(())
}

/// The ISO 4217 code for a currency symbol.
///
/// `XXX` is the code for "no currency", and is the honest answer where the
/// symbol says nothing about which currency it is: `$` alone is a dozen of
/// them. Anything already written as a three letter code is taken at its word.
fn currency_code(symbol: &str) -> &str {
    let symbol = symbol.trim();
    match symbol {
        "£" => "GBP",
        "€" => "EUR",
        "¥" => "JPY",
        "₹" => "INR",
        "R" => "ZAR",
        "kr" => "SEK",
        "CHF" | "USD" | "GBP" | "EUR" | "JPY" | "AUD" | "CAD" | "NZD" | "INR" | "ZAR" => symbol,
        _ => "XXX",
    }
}

/// The first and last working moment of a normal day on this calendar, used
/// for the defaults Project applies to a date typed without a time.
fn default_day(calendar: &WorkCalendar) -> (NaiveTime, NaiveTime) {
    let fallback = (
        NaiveTime::from_hms_opt(8, 0, 0).unwrap_or_default(),
        NaiveTime::from_hms_opt(17, 0, 0).unwrap_or_default(),
    );
    let Some(day) = calendar.week.iter().find(|day| day.is_working()) else {
        return fallback;
    };
    let start = day.shifts.iter().map(|shift| shift.start).min();
    let finish = day.shifts.iter().map(|shift| shift.end).max();
    match (start, finish) {
        (Some(start), Some(finish)) => (start, finish),
        _ => fallback,
    }
}

// ------------------------------------------------------------ calendars

fn write_calendars(w: &mut Xml, calendars: &Calendars) -> io::Result<()> {
    open(w, "Calendars")?;
    for entry in &calendars.entries {
        write_calendar(w, entry)?;
    }
    close(w, "Calendars")?;
    Ok(())
}

fn write_calendar(w: &mut Xml, entry: &CalendarEntry) -> io::Result<()> {
    open(w, "Calendar")?;
    number(w, "UID", entry.uid)?;
    text(w, "Name", entry.name)?;
    flag(w, "IsBaseCalendar", entry.base.is_none())?;
    // Project writes -1 rather than omitting it when a calendar has no base.
    match entry.base {
        Some(base) => number(w, "BaseCalendarUID", base)?,
        None => number(w, "BaseCalendarUID", -1)?,
    }

    if let Some(week) = entry.week {
        open(w, "WeekDays")?;
        for (slot, day) in week.iter().enumerate().take(7) {
            write_weekday(w, slot, day)?;
        }
        close(w, "WeekDays")?;
    }

    if !entry.exceptions.is_empty() {
        open(w, "Exceptions")?;
        for exception in entry.exceptions {
            write_exception(w, exception)?;
        }
        close(w, "Exceptions")?;
    }

    close(w, "Calendar")?;
    Ok(())
}

/// MSPDI numbers weekdays from Sunday; `WorkCalendar` counts from Monday.
fn day_type(slot: usize) -> u32 {
    ((slot as u32 + 1) % 7) + 1
}

fn write_weekday(w: &mut Xml, slot: usize, day: &DayShifts) -> io::Result<()> {
    open(w, "WeekDay")?;
    number(w, "DayType", day_type(slot))?;
    let working = day.is_working();
    flag(w, "DayWorking", working)?;
    if working {
        write_working_times(w, day)?;
    }
    close(w, "WeekDay")?;
    Ok(())
}

fn write_working_times(w: &mut Xml, day: &DayShifts) -> io::Result<()> {
    open(w, "WorkingTimes")?;
    for shift in day
        .shifts
        .iter()
        .filter(|shift| shift.end > shift.start)
        .take(MAX_WORKING_TIMES)
    {
        open(w, "WorkingTime")?;
        time(w, "FromTime", shift.start)?;
        time(w, "ToTime", shift.end)?;
        close(w, "WorkingTime")?;
    }
    close(w, "WorkingTimes")?;
    Ok(())
}

fn write_exception(w: &mut Xml, exception: &CalendarException) -> io::Result<()> {
    open(w, "Exception")?;
    // A run of dates rather than a recurrence, stated once.
    flag(w, "EnteredByOccurrences", false)?;
    open(w, "TimePeriod")?;
    if let Some(from) = exception.from.and_hms_opt(0, 0, 0) {
        datetime(w, "FromDate", from)?;
    }
    // Project writes the last minute of the closing day, not midnight, so the
    // day itself is inside the range rather than just up against it.
    if let Some(to) = exception.to.and_hms_opt(23, 59, 0) {
        datetime(w, "ToDate", to)?;
    }
    close(w, "TimePeriod")?;
    number(w, "Occurrences", 1)?;
    text(w, "Name", &exception.name)?;
    number(w, "Type", EXCEPTION_TYPE_DAILY)?;
    let working = exception.shifts.is_working();
    flag(w, "DayWorking", working)?;
    if working {
        write_working_times(w, &exception.shifts)?;
    }
    close(w, "Exception")?;
    Ok(())
}

// ---------------------------------------------------------------- tasks

fn constraint_code(constraint: ConstraintType) -> u32 {
    match constraint {
        ConstraintType::AsSoonAsPossible => 0,
        ConstraintType::AsLateAsPossible => 1,
        ConstraintType::MustStartOn => 2,
        ConstraintType::MustFinishOn => 3,
        ConstraintType::StartNoEarlierThan => 4,
        ConstraintType::StartNoLaterThan => 5,
        ConstraintType::FinishNoEarlierThan => 6,
        ConstraintType::FinishNoLaterThan => 7,
    }
}

fn link_code(kind: LinkType) -> u32 {
    match kind {
        LinkType::FF => 0,
        LinkType::FS => 1,
        LinkType::SF => 2,
        LinkType::SS => 3,
    }
}

fn write_tasks(w: &mut Xml, project: &Project, ids: &Ids, calendars: &Calendars) -> io::Result<()> {
    // A link names its successor, and the file lists it under that successor,
    // so the links are bucketed once rather than scanned per task: a plan of
    // any size otherwise pays for every task against every link.
    let mut incoming: HashMap<crate::model::TaskId, Vec<&crate::model::Link>> = HashMap::new();
    for link in &project.links {
        incoming.entry(link.successor).or_default().push(link);
    }

    open(w, "Tasks")?;
    for (index, task) in project.tasks.iter().enumerate() {
        let uid = ids.tasks.get(&task.id).copied().unwrap_or(index as u32 + 1);
        write_task(
            w,
            project,
            index,
            task,
            uid,
            ids,
            calendars,
            incoming.get(&task.id).map(Vec::as_slice).unwrap_or(&[]),
        )?;
    }
    close(w, "Tasks")?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_task(
    w: &mut Xml,
    project: &Project,
    index: usize,
    task: &Task,
    uid: u32,
    ids: &Ids,
    calendars: &Calendars,
    incoming: &[&crate::model::Link],
) -> io::Result<()> {
    let summary = project.is_summary(index);
    let manual = task.mode == TaskMode::Manual;

    open(w, "Task")?;
    number(w, "UID", uid)?;
    // ID is the row, UID is the identity. Links use the latter.
    number(w, "ID", index + 1)?;
    text(w, "Name", &task.name)?;
    number(w, "Type", TASK_TYPE_FIXED_DURATION)?;
    // MSPDI counts outline levels from one, this model counts from zero.
    number(w, "OutlineLevel", task.outline_level as u32 + 1)?;

    // A manually scheduled task's start is the planner's answer rather than
    // the scheduler's, and is the one Project has to be given.
    let start = match (manual, task.manual_start) {
        (true, Some(start)) => start,
        _ => task.scheduled.start,
    };
    datetime(w, "Start", start)?;
    datetime(w, "Finish", task.scheduled.finish)?;
    duration(w, "Duration", task.duration_minutes)?;
    number(
        w,
        "DurationFormat",
        if task.estimated {
            FORMAT_DAYS_ESTIMATED
        } else {
            FORMAT_DAYS
        },
    )?;
    duration(w, "Work", task.scheduled.work_minutes)?;
    flag(w, "Estimated", task.estimated)?;
    // A summary row has no duration of its own, so it is never the marker a
    // milestone is even when its own duration reads as zero.
    flag(w, "Milestone", task.is_milestone() && !summary)?;
    flag(w, "Summary", summary)?;
    flag(w, "Critical", task.scheduled.critical)?;
    decimal(w, "FixedCost", task.fixed_cost)?;
    number(w, "FixedCostAccrual", ACCRUE_PRORATED)?;
    number(w, "PercentComplete", task.percent_complete)?;
    decimal(w, "Cost", task.scheduled.cost)?;

    datetime_if(w, "ActualStart", task.actual_start)?;
    datetime_if(w, "ActualFinish", task.actual_finish)?;
    if task.actual_cost != 0.0 {
        decimal(w, "ActualCost", task.actual_cost)?;
    }
    if task.actual_work_minutes != 0 {
        duration(w, "ActualWork", task.actual_work_minutes)?;
    }
    if task.remaining_work_minutes != 0 {
        duration(w, "RemainingWork", task.remaining_work_minutes)?;
    }

    number(w, "ConstraintType", constraint_code(task.constraint))?;
    if !task.calendar.trim().is_empty() {
        number(w, "CalendarUID", calendars.named(&task.calendar))?;
    }
    if task.constraint.needs_date() {
        datetime_if(w, "ConstraintDate", task.constraint_date)?;
    }
    datetime_if(w, "Deadline", task.deadline)?;
    flag(w, "IgnoreResourceCalendar", task.ignore_resource_calendars)?;
    text_if(w, "Notes", &task.notes)?;
    if let Some(physical) = task.physical_percent_complete {
        number(w, "PhysicalPercentComplete", physical)?;
    }
    number(
        w,
        "EarnedValueMethod",
        match task.earned_value_method {
            EarnedValueMethod::PercentComplete => 0,
            EarnedValueMethod::PhysicalPercentComplete => 1,
        },
    )?;

    for link in incoming {
        let Some(&predecessor) = ids.tasks.get(&link.predecessor) else {
            continue;
        };
        open(w, "PredecessorLink")?;
        number(w, "PredecessorUID", predecessor)?;
        number(w, "Type", link_code(link.kind))?;
        flag(w, "CrossProject", false)?;
        // Lag is tenths of a minute, and is negative for an overlap.
        number(w, "LinkLag", link.lag_minutes * 10)?;
        number(w, "LagFormat", FORMAT_DAYS)?;
        close(w, "PredecessorLink")?;
    }

    if let Some(baseline) = task.baseline {
        open(w, "Baseline")?;
        // Baseline zero. Project keeps eleven, this model keeps the one that
        // every variance in it is measured against.
        number(w, "Number", 0)?;
        datetime(w, "Start", baseline.start)?;
        datetime(w, "Finish", baseline.finish)?;
        duration(w, "Duration", baseline.duration_minutes)?;
        number(w, "DurationFormat", FORMAT_DAYS)?;
        duration(w, "Work", baseline.work_minutes)?;
        decimal(w, "Cost", baseline.cost)?;
        close(w, "Baseline")?;
    }

    // Last, and in this order, because they are the 2010 additions and a
    // schema sequence grows only at its end.
    flag(w, "Manual", manual)?;
    flag(w, "Active", task.active)?;

    close(w, "Task")?;
    Ok(())
}

// ------------------------------------------------------------ resources

fn write_resources(
    w: &mut Xml,
    project: &Project,
    ids: &Ids,
    calendars: &Calendars,
) -> io::Result<()> {
    open(w, "Resources")?;
    for (index, resource) in project.resources.iter().enumerate() {
        let uid = ids
            .resources
            .get(&resource.id)
            .copied()
            .unwrap_or(index as u32 + 1);
        open(w, "Resource")?;
        number(w, "UID", uid)?;
        number(w, "ID", index + 1)?;
        text(w, "Name", &resource.name)?;
        // The schema knows only material and work here. A cost resource is a
        // work resource with `IsCostResource` set, further down.
        number(
            w,
            "Type",
            match resource.kind {
                ResourceKind::Material => 0,
                _ => 1,
            },
        )?;
        text_if(w, "Initials", &resource.initials)?;
        text_if(w, "Code", &resource.code)?;
        text_if(w, "Group", &resource.group)?;
        text_if(w, "EmailAddress", &resource.email)?;
        decimal(w, "MaxUnits", resource.max_units)?;
        decimal(w, "StandardRate", resource.standard_rate)?;
        number(w, "StandardRateFormat", RATE_PER_HOUR)?;
        decimal(w, "OvertimeRate", resource.overtime_rate)?;
        number(w, "OvertimeRateFormat", RATE_PER_HOUR)?;
        decimal(w, "CostPerUse", resource.cost_per_use)?;
        number(w, "CalendarUID", calendars.for_resource(resource))?;
        text_if(w, "Notes", &resource.notes)?;
        // The schema puts this after the baselines and outline codes, neither
        // of which is written here, so it comes last.
        flag(w, "IsCostResource", resource.kind == ResourceKind::Cost)?;
        close(w, "Resource")?;
    }
    close(w, "Resources")?;
    Ok(())
}

// ---------------------------------------------------------- assignments

fn write_assignments(w: &mut Xml, project: &Project, ids: &Ids) -> io::Result<()> {
    open(w, "Assignments")?;
    let mut uid = 0u32;
    for task in &project.tasks {
        let Some(&task_uid) = ids.tasks.get(&task.id) else {
            continue;
        };
        for assignment in &task.assignments {
            let Some(&resource_uid) = ids.resources.get(&assignment.resource) else {
                continue;
            };
            uid += 1;
            open(w, "Assignment")?;
            number(w, "UID", uid)?;
            number(w, "TaskUID", task_uid)?;
            number(w, "ResourceUID", resource_uid)?;
            number(w, "PercentWorkComplete", task.percent_complete)?;
            decimal(w, "Units", assignment.units)?;
            close(w, "Assignment")?;
        }
    }
    close(w, "Assignments")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::Shift;
    use crate::model::{Assignment, Link, Task};
    use crate::mspdi::from_xml;
    use crate::schedule::schedule;
    use chrono::{NaiveDate, Timelike};

    /// What a moment looks like once it has been through the file: MSPDI
    /// carries whole seconds, which is all this engine ever schedules to.
    fn truncated(value: NaiveDateTime) -> NaiveDateTime {
        value.with_nanosecond(0).unwrap_or(value)
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("a real date")
    }

    fn moment(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        day(y, m, d).and_hms_opt(h, 0, 0).expect("a real time")
    }

    /// A plan with one of everything the writer claims to carry.
    fn sample() -> Project {
        let mut project = Project::blank(moment(2026, 8, 17, 8));
        project.name = "Bridge Refit".into();
        project.author = "A Planner".into();
        project.company = "Ironworks".into();
        project.currency_symbol = "£".into();
        project.calendar.exceptions.push(CalendarException {
            name: "Site closed".into(),
            from: day(2026, 8, 26),
            to: day(2026, 8, 26),
            shifts: DayShifts::nonworking(),
        });

        let survey = project.allocate_task_id();
        let mut task = Task::new(survey, "Survey & report <draft>", 960);
        task.percent_complete = 50;
        task.notes = "Watch the \"tide\"\nand the wind".into();
        project.tasks.push(task);

        let strip = project.allocate_task_id();
        let mut task = Task::new(strip, "Strip deck", 2400);
        task.constraint = ConstraintType::StartNoEarlierThan;
        task.constraint_date = Some(moment(2026, 8, 24, 8));
        task.deadline = Some(moment(2026, 9, 4, 17));
        task.fixed_cost = 1250.5;
        project.tasks.push(task);

        let clear = project.allocate_task_id();
        project.tasks.push(Task::milestone(clear, "Deck clear"));

        project.add_link(Link {
            predecessor: survey,
            successor: strip,
            kind: LinkType::SS,
            // A negative lag is an overlap, and is the one that gets lost.
            lag_minutes: -240,
        });
        project.add_link(Link {
            predecessor: strip,
            successor: clear,
            kind: LinkType::FS,
            lag_minutes: 480,
        });

        let ana = project.add_resource("Ana Reyes");
        if let Some(resource) = project.resources.iter_mut().find(|r| r.id == ana) {
            resource.group = "Survey".into();
            resource.standard_rate = 75.0;
            resource.overtime_rate = 112.5;
            resource.cost_per_use = 40.0;
            resource.max_units = 0.5;
        }
        if let Some(task) = project.task_mut(survey) {
            task.assignments.push(Assignment {
                resource: ana,
                units: 0.5,
            });
        }

        project
    }

    /// Write, read back, and hand back what came home.
    fn round_trip(project: &Project) -> Project {
        let xml = to_xml(project).expect("the writer produced a document");
        from_xml(&xml).expect("the reader accepted it")
    }

    #[test]
    fn iso_durations_match_what_the_reader_expects() {
        assert_eq!(iso_duration(960), "PT16H0M0S");
        assert_eq!(iso_duration(0), "PT0H0M0S");
        assert_eq!(iso_duration(90), "PT1H30M0S");
        assert_eq!(crate::mspdi::parse_iso_duration(&iso_duration(2400)), Some(2400));
    }

    #[test]
    fn the_weekday_numbering_is_the_readers_own() {
        // Monday is slot zero here and DayType 2 there; Sunday is slot six and
        // DayType 1.
        assert_eq!(day_type(0), 2);
        assert_eq!(day_type(5), 7);
        assert_eq!(day_type(6), 1);
    }

    #[test]
    fn the_header_survives() {
        let there = round_trip(&sample());
        assert_eq!(there.name, "Bridge Refit");
        assert_eq!(there.author, "A Planner");
        assert_eq!(there.company, "Ironworks");
        assert_eq!(there.currency_symbol, "£");
        assert_eq!(there.start_date, moment(2026, 8, 17, 8));
    }

    #[test]
    fn tasks_keep_their_names_outline_durations_and_progress() {
        let here = sample();
        let there = round_trip(&here);
        assert_eq!(there.tasks.len(), here.tasks.len());
        for (before, after) in here.tasks.iter().zip(&there.tasks) {
            assert_eq!(after.name, before.name);
            assert_eq!(after.outline_level, before.outline_level);
            assert_eq!(after.duration_minutes, before.duration_minutes);
            assert_eq!(after.percent_complete, before.percent_complete);
            assert_eq!(after.is_milestone(), before.is_milestone());
            assert_eq!(after.notes, before.notes);
            assert_eq!(after.fixed_cost, before.fixed_cost);
        }
    }

    #[test]
    fn a_name_full_of_xml_metacharacters_comes_back_whole() {
        let mut here = Project::blank(moment(2026, 8, 17, 8));
        let id = here.allocate_task_id();
        let awkward = "R&D <phase 1> \"final\" 'draft', plain\ttab & line\nbreak";
        here.tasks.push(Task::new(id, awkward, 480));
        let there = round_trip(&here);
        assert_eq!(there.tasks[0].name, awkward);
    }

    #[test]
    fn the_outline_keeps_its_shape() {
        let mut here = Project::blank(moment(2026, 8, 17, 8));
        for (name, level) in [
            ("Phase one", 0u16),
            ("Design", 1),
            ("Draw it", 2),
            ("Check it", 2),
            ("Phase two", 0),
        ] {
            let id = here.allocate_task_id();
            let mut task = Task::new(id, name, 480);
            task.outline_level = level;
            here.tasks.push(task);
        }
        let there = round_trip(&here);
        let levels: Vec<u16> = there.tasks.iter().map(|t| t.outline_level).collect();
        assert_eq!(levels, vec![0, 1, 2, 2, 0]);
        assert!(there.is_summary(0), "a parent is still a parent");
        assert!(!there.is_summary(3));
    }

    #[test]
    fn links_keep_their_type_and_their_lag_including_an_overlap() {
        let here = sample();
        let there = round_trip(&here);
        assert_eq!(there.links.len(), here.links.len());

        let by_shape: Vec<(LinkType, i64)> =
            there.links.iter().map(|l| (l.kind, l.lag_minutes)).collect();
        assert!(by_shape.contains(&(LinkType::SS, -240)), "an overlap is a negative lag");
        assert!(by_shape.contains(&(LinkType::FS, 480)));

        // Both ends of every link still name real tasks, and the same pair.
        let names = |plan: &Project, link: &Link| {
            (
                plan.task(link.predecessor).map(|t| t.name.clone()),
                plan.task(link.successor).map(|t| t.name.clone()),
            )
        };
        let mut before: Vec<_> = here.links.iter().map(|l| names(&here, l)).collect();
        let mut after: Vec<_> = there.links.iter().map(|l| names(&there, l)).collect();
        before.sort();
        after.sort();
        assert_eq!(after, before);
    }

    #[test]
    fn every_link_type_survives() {
        let mut here = Project::blank(moment(2026, 8, 17, 8));
        let first = here.allocate_task_id();
        here.tasks.push(Task::new(first, "First", 480));
        for kind in LinkType::ALL {
            let id = here.allocate_task_id();
            here.tasks.push(Task::new(id, kind.code(), 480));
            here.add_link(Link {
                predecessor: first,
                successor: id,
                kind,
                lag_minutes: 0,
            });
        }
        let there = round_trip(&here);
        let mut kinds: Vec<&str> = there.links.iter().map(|l| l.kind.code()).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["FF", "FS", "SF", "SS"]);
    }

    #[test]
    fn every_constraint_survives_with_its_date() {
        let mut here = Project::blank(moment(2026, 8, 17, 8));
        for constraint in ConstraintType::ALL {
            let id = here.allocate_task_id();
            let mut task = Task::new(id, constraint.label(), 480);
            task.constraint = constraint;
            if constraint.needs_date() {
                task.constraint_date = Some(moment(2026, 9, 1, 8));
            }
            task.deadline = Some(moment(2026, 10, 1, 17));
            here.tasks.push(task);
        }
        let there = round_trip(&here);
        for (before, after) in here.tasks.iter().zip(&there.tasks) {
            assert_eq!(after.constraint, before.constraint, "{}", before.name);
            assert_eq!(after.constraint_date, before.constraint_date, "{}", before.name);
            assert_eq!(after.deadline, before.deadline, "{}", before.name);
        }
    }

    #[test]
    fn resources_and_their_bookings_survive() {
        let here = sample();
        let there = round_trip(&here);
        assert_eq!(there.resources.len(), 1);
        let ana = &there.resources[0];
        assert_eq!(ana.name, "Ana Reyes");
        assert_eq!(ana.group, "Survey");
        assert_eq!(ana.standard_rate, 75.0);
        assert_eq!(ana.overtime_rate, 112.5);
        assert_eq!(ana.cost_per_use, 40.0);
        assert_eq!(ana.max_units, 0.5);

        let survey = &there.tasks[0];
        assert_eq!(survey.assignments.len(), 1);
        assert_eq!(survey.assignments[0].resource, ana.id);
        assert_eq!(survey.assignments[0].units, 0.5);
    }

    #[test]
    fn a_cost_resource_is_still_a_cost_resource() {
        let mut here = Project::blank(moment(2026, 8, 17, 8));
        for kind in ResourceKind::ALL {
            let id = here.add_resource(kind.label());
            if let Some(resource) = here.resources.iter_mut().find(|r| r.id == id) {
                resource.kind = kind;
            }
        }
        let there = round_trip(&here);
        let kinds: Vec<ResourceKind> = there.resources.iter().map(|r| r.kind).collect();
        assert_eq!(kinds, ResourceKind::ALL.to_vec());
    }

    #[test]
    fn the_project_calendar_and_its_exceptions_survive() {
        let here = sample();
        let there = round_trip(&here);
        assert_eq!(there.calendar.name, here.calendar.name);
        assert_eq!(there.calendar.week, here.calendar.week);
        assert_eq!(there.calendar.exceptions, here.calendar.exceptions);
        assert!(!there.calendar.is_working_day(day(2026, 8, 26)));
    }

    #[test]
    fn a_base_calendar_with_its_own_week_survives() {
        let mut here = sample();
        let mut nights = WorkCalendar::standard();
        nights.name = "Night Shift".into();
        for slot in 0..5 {
            nights.week[slot] = DayShifts::night();
        }
        // A four day week, so the Friday is the one that has to come back.
        nights.week[4] = DayShifts::nonworking();
        nights.exceptions.push(CalendarException {
            name: "Shutdown".into(),
            from: day(2026, 12, 24),
            to: day(2027, 1, 2),
            shifts: DayShifts::nonworking(),
        });
        here.add_base_calendar(nights.clone());

        let there = round_trip(&here);
        let back = there
            .calendar_named("Night Shift")
            .expect("a base calendar goes into the library");
        assert_eq!(back.week, nights.week);
        assert_eq!(back.exceptions, nights.exceptions);
        assert!(!back.is_working_day(day(2026, 8, 21)), "Friday is off");
    }

    #[test]
    fn a_working_exception_keeps_the_hours_it_names() {
        let mut here = sample();
        here.calendar.exceptions.push(CalendarException {
            name: "Weekend push".into(),
            from: day(2026, 8, 29),
            to: day(2026, 8, 30),
            shifts: DayShifts {
                shifts: vec![Shift::new(9, 0, 13, 30)],
            },
        });
        let there = round_trip(&here);
        let back = there
            .calendar
            .exceptions
            .iter()
            .find(|e| e.name == "Weekend push")
            .expect("kept");
        assert_eq!(back.from, day(2026, 8, 29));
        assert_eq!(back.to, day(2026, 8, 30));
        assert_eq!(back.shifts.shifts, vec![Shift::new(9, 0, 13, 30)]);
        assert!(there.calendar.is_working_day(day(2026, 8, 29)));
    }

    #[test]
    fn a_persons_own_calendar_and_their_leave_survive() {
        let mut here = sample();
        let mut four_day = WorkCalendar::standard();
        four_day.name = "Four Day Week".into();
        four_day.week[4] = DayShifts::nonworking();
        here.add_base_calendar(four_day);

        let ben = here.add_resource("Ben Okafor");
        let leave = CalendarException {
            name: "Leave".into(),
            from: day(2026, 8, 18),
            to: day(2026, 8, 19),
            shifts: DayShifts::nonworking(),
        };
        if let Some(resource) = here.resources.iter_mut().find(|r| r.id == ben) {
            resource.base_calendar = "Four Day Week".into();
            resource.calendar_exceptions.push(leave.clone());
        }
        // Somebody who names a base and has no time off of their own.
        let cara = here.add_resource("Cara Lin");
        if let Some(resource) = here.resources.iter_mut().find(|r| r.id == cara) {
            resource.base_calendar = "Four Day Week".into();
        }

        let there = round_trip(&here);
        let back = there
            .resources
            .iter()
            .find(|r| r.name == "Ben Okafor")
            .expect("kept");
        assert_eq!(back.base_calendar, "Four Day Week");
        assert_eq!(back.calendar_exceptions, vec![leave]);

        let cara = there
            .resources
            .iter()
            .find(|r| r.name == "Cara Lin")
            .expect("kept");
        assert_eq!(cara.base_calendar, "Four Day Week");
        assert!(
            cara.calendar_exceptions.is_empty(),
            "nobody gains time off they never had"
        );

        // And the person's calendar is not left lying about in the library.
        assert!(there.calendar_named("Ben Okafor").is_none());
    }

    #[test]
    fn a_persons_leave_still_moves_their_work() {
        let mut here = sample();
        let id = here.allocate_task_id();
        here.tasks.push(Task::new(id, "Write it", 960));
        let ada = here.add_resource("Ada");
        if let Some(resource) = here.resources.iter_mut().find(|r| r.id == ada) {
            resource.calendar_exceptions.push(CalendarException {
                name: "Leave".into(),
                from: day(2026, 8, 18),
                to: day(2026, 8, 19),
                shifts: DayShifts::nonworking(),
            });
        }
        if let Some(task) = here.task_mut(id) {
            task.assignments.push(Assignment {
                resource: ada,
                units: 1.0,
            });
        }

        let mut there = round_trip(&here);
        schedule(&mut there).expect("the imported plan schedules");
        let back = there.tasks.iter().find(|t| t.name == "Write it").expect("kept");
        // Two days of work from Monday, with Ada away Tuesday and Wednesday.
        assert_eq!(back.scheduled.finish, moment(2026, 8, 20, 17));
    }

    #[test]
    fn a_task_calendar_survives() {
        let mut here = sample();
        let mut seven = WorkCalendar::standard();
        seven.name = "Seven Day".into();
        seven.week[5] = DayShifts::standard();
        seven.week[6] = DayShifts::standard();
        here.add_base_calendar(seven);
        if let Some(task) = here.tasks.first_mut() {
            task.calendar = "Seven Day".into();
            task.ignore_resource_calendars = true;
        }

        let there = round_trip(&here);
        assert_eq!(there.tasks[0].calendar, "Seven Day");
        assert!(there.tasks[0].ignore_resource_calendars);
    }

    #[test]
    fn a_baseline_survives() {
        let mut here = sample();
        schedule(&mut here).expect("schedules");
        here.set_baseline();
        let there = round_trip(&here);
        assert!(there.has_baseline());
        for (before, after) in here.tasks.iter().zip(&there.tasks) {
            let (Some(before), Some(after)) = (before.baseline, after.baseline) else {
                panic!("every task was baselined");
            };
            assert_eq!(after.start, truncated(before.start));
            assert_eq!(after.finish, truncated(before.finish));
            assert_eq!(after.duration_minutes, before.duration_minutes);
            assert_eq!(after.work_minutes, before.work_minutes);
            assert_eq!(after.cost, before.cost);
        }
    }

    #[test]
    fn what_really_happened_survives() {
        let mut here = sample();
        if let Some(task) = here.tasks.first_mut() {
            task.actual_start = Some(moment(2026, 8, 17, 8));
            task.actual_finish = Some(moment(2026, 8, 18, 17));
            task.actual_work_minutes = 900;
            task.actual_cost = 675.0;
            task.remaining_work_minutes = 120;
            task.physical_percent_complete = Some(40);
            task.earned_value_method = EarnedValueMethod::PhysicalPercentComplete;
        }
        let there = round_trip(&here);
        let back = &there.tasks[0];
        assert_eq!(back.actual_start, Some(moment(2026, 8, 17, 8)));
        assert_eq!(back.actual_finish, Some(moment(2026, 8, 18, 17)));
        assert_eq!(back.actual_work_minutes, 900);
        assert_eq!(back.actual_cost, 675.0);
        assert_eq!(back.remaining_work_minutes, 120);
        assert_eq!(back.physical_percent_complete, Some(40));
        assert_eq!(
            back.earned_value_method,
            EarnedValueMethod::PhysicalPercentComplete
        );
    }

    #[test]
    fn a_manual_task_keeps_its_typed_start() {
        let mut here = sample();
        if let Some(task) = here.tasks.last_mut() {
            task.mode = TaskMode::Manual;
            task.manual_start = Some(moment(2026, 9, 7, 8));
        }
        let there = round_trip(&here);
        let back = there.tasks.last().expect("kept");
        assert_eq!(back.mode, TaskMode::Manual);
        assert_eq!(back.manual_start, Some(moment(2026, 9, 7, 8)));
    }

    #[test]
    fn an_inactive_task_stays_inactive() {
        let mut here = sample();
        if let Some(task) = here.tasks.first_mut() {
            task.active = false;
        }
        let there = round_trip(&here);
        assert!(!there.tasks[0].active);
        assert!(there.tasks[1].active);
    }

    #[test]
    fn an_empty_plan_writes_a_file_that_reads_back() {
        let here = Project::blank(moment(2026, 8, 17, 8));
        let xml = to_xml(&here).expect("written");
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>"));
        assert!(xml.contains(NAMESPACE));
        let there = from_xml(&xml).expect("read back");
        assert!(there.tasks.is_empty());
        assert!(there.resources.is_empty());
        assert_eq!(there.calendar.week, here.calendar.week);
    }

    #[test]
    fn the_dates_a_scheduled_plan_worked_out_come_back_the_same() {
        let mut here = sample();
        schedule(&mut here).expect("schedules");
        let mut there = round_trip(&here);
        schedule(&mut there).expect("schedules again");
        for (before, after) in here.tasks.iter().zip(&there.tasks) {
            assert_eq!(after.scheduled.start, before.scheduled.start, "{}", before.name);
            assert_eq!(after.scheduled.finish, before.scheduled.finish, "{}", before.name);
        }
    }

    /// The schema's own order for the elements this writer emits, taken from
    /// the Project Data Interchange schema. `Manual` and `Active` are the 2010
    /// additions and sit after everything the 2007 schema declares.
    const TASK_ORDER: [&str; 26] = [
        "UID", "ID", "Name", "Type", "OutlineLevel", "Start", "Finish", "Duration",
        "DurationFormat", "Work", "Estimated", "Milestone", "Summary", "Critical", "FixedCost",
        "FixedCostAccrual", "PercentComplete", "Cost", "ActualStart", "ActualFinish", "ActualCost",
        "ActualWork", "RemainingWork", "ConstraintType", "CalendarUID", "ConstraintDate",
    ];

    const TASK_TAIL: [&str; 9] = [
        "Deadline", "IgnoreResourceCalendar", "Notes", "PhysicalPercentComplete",
        "EarnedValueMethod", "PredecessorLink", "Baseline", "Manual", "Active",
    ];

    const PROJECT_ORDER: [&str; 30] = [
        "SaveVersion", "Name", "Title", "Company", "Author", "ScheduleFromStart", "StartDate",
        "FinishDate", "CriticalSlackLimit", "CurrencyDigits", "CurrencySymbol", "CurrencyCode",
        "CurrencySymbolPosition", "CalendarUID", "DefaultStartTime", "DefaultFinishTime",
        "MinutesPerDay", "MinutesPerWeek", "DaysPerMonth", "DefaultTaskType",
        "DefaultFixedCostAccrual", "DurationFormat", "WorkFormat", "HonorConstraints",
        "StatusDate", "CurrentDate", "Calendars", "Tasks", "Resources", "Assignments",
    ];

    const RESOURCE_ORDER: [&str; 17] = [
        "UID", "ID", "Name", "Type", "Initials", "Code", "Group", "EmailAddress", "MaxUnits",
        "StandardRate", "StandardRateFormat", "OvertimeRate", "OvertimeRateFormat", "CostPerUse",
        "CalendarUID", "Notes", "IsCostResource",
    ];

    const CALENDAR_ORDER: [&str; 6] = [
        "UID", "Name", "IsBaseCalendar", "BaseCalendarUID", "WeekDays", "Exceptions",
    ];

    const EXCEPTION_ORDER: [&str; 7] = [
        "EnteredByOccurrences", "TimePeriod", "Occurrences", "Name", "Type", "DayWorking",
        "WorkingTimes",
    ];

    /// The direct children of every `<parent>` in the document, in the order
    /// they were written.
    fn children_of(xml: &str, parent: &str) -> Vec<Vec<String>> {
        use quick_xml::Reader;
        use quick_xml::events::Event;

        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut path: Vec<String> = Vec::new();
        let mut found: Vec<Vec<String>> = Vec::new();
        loop {
            match reader.read_event() {
                Ok(Event::Eof) | Err(_) => break,
                Ok(Event::Start(element)) => {
                    let name = String::from_utf8_lossy(element.name().as_ref()).to_string();
                    if name == parent {
                        found.push(Vec::new());
                    } else if path.last().map(String::as_str) == Some(parent)
                        && let Some(row) = found.last_mut()
                    {
                        row.push(name.clone());
                    }
                    path.push(name);
                }
                Ok(Event::End(_)) => {
                    path.pop();
                }
                _ => {}
            }
        }
        found
    }

    /// Whether what was written keeps to the order the schema declares.
    fn in_schema_order(written: &[String], schema: &[&str]) -> bool {
        let mut at = 0usize;
        for name in written {
            let Some(found) = schema[at..].iter().position(|s| s == name) else {
                return false;
            };
            at += found + 1;
        }
        true
    }

    #[test]
    fn every_element_is_written_in_the_order_the_schema_declares() {
        // Project validates a file against a sequence, not a set, and says
        // nothing useful when it refuses one. This is the guard on that.
        let mut here = sample();
        schedule(&mut here).expect("schedules");
        here.set_baseline();
        if let Some(task) = here.tasks.first_mut() {
            task.actual_start = Some(moment(2026, 8, 17, 8));
            task.actual_work_minutes = 480;
            task.actual_cost = 300.0;
            task.remaining_work_minutes = 480;
            task.physical_percent_complete = Some(25);
            task.calendar = here_calendar_name();
        }
        let mut four_day = WorkCalendar::standard();
        four_day.name = here_calendar_name();
        four_day.week[4] = DayShifts::nonworking();
        here.add_base_calendar(four_day);
        if let Some(resource) = here.resources.first_mut() {
            resource.email = "ana@example.com".into();
            resource.code = "AR-1".into();
            resource.notes = "Away in August".into();
            resource.calendar_exceptions.push(CalendarException {
                name: "Leave".into(),
                from: day(2026, 9, 14),
                to: day(2026, 9, 18),
                shifts: DayShifts::nonworking(),
            });
        }

        let xml = to_xml(&here).expect("written");

        let full_task: Vec<&str> = TASK_ORDER.iter().chain(TASK_TAIL.iter()).copied().collect();
        for (which, rows, order) in [
            ("Project", children_of(&xml, "Project"), PROJECT_ORDER.to_vec()),
            ("Task", children_of(&xml, "Task"), full_task),
            ("Resource", children_of(&xml, "Resource"), RESOURCE_ORDER.to_vec()),
            ("Calendar", children_of(&xml, "Calendar"), CALENDAR_ORDER.to_vec()),
            ("Exception", children_of(&xml, "Exception"), EXCEPTION_ORDER.to_vec()),
        ] {
            assert!(!rows.is_empty(), "{which} was never written");
            for row in &rows {
                assert!(
                    in_schema_order(row, &order),
                    "{which} children are out of schema order: {row:?}"
                );
            }
        }

        // The three collections come last, and in this order.
        let root = children_of(&xml, "Project").pop().expect("a root");
        let tail: Vec<&String> = root
            .iter()
            .filter(|name| matches!(name.as_str(), "Calendars" | "Tasks" | "Resources" | "Assignments"))
            .collect();
        assert_eq!(tail, vec!["Calendars", "Tasks", "Resources", "Assignments"]);
    }

    fn here_calendar_name() -> String {
        "Four Day Week".into()
    }

    #[test]
    fn a_file_is_actually_written() {
        let dir = std::env::temp_dir().join("aop-mspdi-write-test");
        let path = dir.join("plan.xml");
        let _ = std::fs::remove_file(&path);
        save(&path, &sample()).expect("written");
        let text = std::fs::read_to_string(&path).expect("readable");
        assert!(text.contains("Bridge Refit"));
        let _ = std::fs::remove_file(&path);
    }
}
