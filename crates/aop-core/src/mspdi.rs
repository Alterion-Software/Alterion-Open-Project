//! Importing Microsoft Project plans.
//!
//! Project's own `.mpp` is an undocumented OLE compound document and is not
//! read here. What is read is **MSPDI**, the documented XML that Project writes
//! from Save As -> XML Format (`*.xml`), which carries the whole plan: the task
//! outline, links with lag, constraints, calendars, resources and assignments.
//!
//! The mapping is deliberately forgiving. Unknown elements are skipped rather
//! than rejected, because real files carry a great deal this app has no use for.

use std::collections::HashMap;

use chrono::{NaiveDateTime, Timelike};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::calendar::{CalendarException, DayShifts, WorkCalendar};
use crate::model::{
    Assignment, ConstraintType, Link, LinkType, Project, Resource, ResourceKind, Task, TaskMode,
};
use crate::MINUTES_PER_DAY;

#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    /// The file parsed as XML but is not a Project plan.
    NotMspdi,
    /// The binary `.mpp` reader could not make sense of the file.
    Mpp(String),
    Xml(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "{e}"),
            ImportError::NotMspdi => write!(
                f,
                "This XML file is not a Microsoft Project plan (no <Project> element was found)."
            ),
            ImportError::Mpp(detail) => write!(f, "{detail}"),
            ImportError::Xml(detail) => write!(f, "The file could not be read: {detail}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        ImportError::Io(e)
    }
}

/// Every OLE compound document, and therefore every `.mpp`, starts with this.
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Open a Microsoft Project plan, picking the reader from the file's own
/// contents rather than its name: a compound document is a binary `.mpp`,
/// anything else is treated as MSPDI XML.
pub fn open(path: &std::path::Path) -> Result<Project, ImportError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() >= 8 && bytes[..8] == OLE_MAGIC {
        return crate::mpp::open(path);
    }
    let text = String::from_utf8_lossy(&bytes);
    from_xml(&text)
}

// ---------------------------------------------------------------- raw rows

#[derive(Default)]
struct RawTask {
    uid: u32,
    name: String,
    outline_level: u16,
    duration_minutes: i64,
    start: Option<NaiveDateTime>,
    finish: Option<NaiveDateTime>,
    percent_complete: u8,
    milestone: bool,
    summary: bool,
    manual: bool,
    active: bool,
    notes: String,
    fixed_cost: f64,
    constraint_type: u8,
    constraint_date: Option<NaiveDateTime>,
    deadline: Option<NaiveDateTime>,
    predecessors: Vec<(u32, u8, i64, u8)>,
}

#[derive(Default)]
struct RawResource {
    uid: u32,
    name: String,
    initials: String,
    group: String,
    kind: u8,
    max_units: f64,
    standard_rate: f64,
    overtime_rate: f64,
    cost_per_use: f64,
}

/// Project's ConstraintType codes.
fn constraint_from(code: u8) -> ConstraintType {
    match code {
        1 => ConstraintType::AsLateAsPossible,
        2 => ConstraintType::MustStartOn,
        3 => ConstraintType::MustFinishOn,
        4 => ConstraintType::StartNoEarlierThan,
        5 => ConstraintType::StartNoLaterThan,
        6 => ConstraintType::FinishNoEarlierThan,
        7 => ConstraintType::FinishNoLaterThan,
        _ => ConstraintType::AsSoonAsPossible,
    }
}

/// Project's PredecessorLink Type codes.
fn link_from(code: u8) -> LinkType {
    match code {
        0 => LinkType::FF,
        2 => LinkType::SF,
        3 => LinkType::SS,
        _ => LinkType::FS,
    }
}

/// Project writes durations as an ISO 8601 period, for example `PT40H0M0S`.
fn parse_iso_duration(text: &str) -> Option<i64> {
    let text = text.trim();
    let rest = text.strip_prefix('P')?;
    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };

    let mut minutes = 0i64;
    let mut number = String::new();
    for c in date_part.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
        } else {
            let value: f64 = number.parse().unwrap_or(0.0);
            number.clear();
            // Calendar periods are read against an 8 hour working day.
            minutes += match c {
                'Y' => (value * 12.0 * 20.0 * MINUTES_PER_DAY as f64) as i64,
                'M' => (value * 20.0 * MINUTES_PER_DAY as f64) as i64,
                'W' => (value * 5.0 * MINUTES_PER_DAY as f64) as i64,
                'D' => (value * MINUTES_PER_DAY as f64) as i64,
                _ => 0,
            };
        }
    }

    number.clear();
    for c in time_part.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
        } else {
            let value: f64 = number.parse().unwrap_or(0.0);
            number.clear();
            minutes += match c {
                'H' => (value * 60.0) as i64,
                'M' => value as i64,
                'S' => (value / 60.0) as i64,
                _ => 0,
            };
        }
    }
    Some(minutes)
}

fn parse_datetime(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M").ok())
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(8, 0, 0))
        })
}

fn parse_bool(text: &str) -> bool {
    matches!(text.trim(), "1" | "true" | "True")
}

// ------------------------------------------------------------------ parser

/// Parse an MSPDI document into a plan.
pub fn from_xml(text: &str) -> Result<Project, ImportError> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);

    let mut seen_project = false;
    let mut project_name = String::new();
    let mut project_start: Option<NaiveDateTime> = None;
    let mut author = String::new();
    let mut company = String::new();
    let mut currency = String::new();

    let mut tasks: Vec<RawTask> = Vec::new();
    let mut resources: Vec<RawResource> = Vec::new();
    let mut assignments: Vec<(u32, u32, f64)> = Vec::new();
    let mut holidays: Vec<(String, chrono::NaiveDate, chrono::NaiveDate)> = Vec::new();

    // Where the parser currently is. MSPDI nests shallowly, so a small stack of
    // element names is enough to tell a Task's <Name> from a Resource's.
    let mut path: Vec<String> = Vec::new();
    let mut text_buffer = String::new();

    // Scratch rows for whichever element is open.
    let mut task = RawTask::default();
    let mut resource = RawResource::default();
    let mut predecessor: (u32, u8, i64, u8) = (0, 1, 0, 7);
    let mut assignment: (u32, u32, f64) = (0, 0, 1.0);
    let mut exception_name = String::new();
    let mut exception_from: Option<chrono::NaiveDate> = None;
    let mut exception_to: Option<chrono::NaiveDate> = None;
    let mut exception_working = true;

    loop {
        match reader.read_event() {
            Err(e) => return Err(ImportError::Xml(e.to_string())),
            Ok(Event::Eof) => break,

            Ok(Event::Start(element)) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                match name.as_str() {
                    "Project" => seen_project = true,
                    "Task" if path.last().map(String::as_str) == Some("Tasks") => {
                        // MSPDI omits fields that hold their default, so a
                        // fresh row starts at Project's defaults, not zero.
                        task = RawTask {
                            active: true,
                            ..RawTask::default()
                        };
                    }
                    "Resource" if path.last().map(String::as_str) == Some("Resources") => {
                        resource = RawResource {
                            max_units: 1.0,
                            ..RawResource::default()
                        };
                    }
                    "PredecessorLink" => predecessor = (0, 1, 0, 7),
                    "Assignment" => assignment = (0, 0, 1.0),
                    "Exception" => {
                        exception_name = String::from("Holiday");
                        exception_from = None;
                        exception_to = None;
                        exception_working = true;
                    }
                    _ => {}
                }
                path.push(name);
                text_buffer.clear();
            }

            Ok(Event::Text(bytes)) => {
                text_buffer = bytes.decode().map(|c| c.to_string()).unwrap_or_default();
            }

            Ok(Event::End(element)) => {
                let name = String::from_utf8_lossy(element.local_name().as_ref()).to_string();
                let value = std::mem::take(&mut text_buffer);
                path.pop();
                let parent = path.last().map(String::as_str).unwrap_or("");

                match (parent, name.as_str()) {
                    // ---- project header ---------------------------------
                    ("Project", "Name") | ("Project", "Title") => {
                        if project_name.is_empty() {
                            project_name = value;
                        }
                    }
                    ("Project", "StartDate") => project_start = parse_datetime(&value),
                    ("Project", "Author") => author = value,
                    ("Project", "Company") => company = value,
                    ("Project", "CurrencySymbol") => currency = value,

                    // ---- tasks ------------------------------------------
                    ("Task", "UID") => task.uid = value.trim().parse().unwrap_or(0),
                    ("Task", "Name") => task.name = value,
                    ("Task", "OutlineLevel") => {
                        // MSPDI counts from one, this model counts from zero.
                        let level: u16 = value.trim().parse().unwrap_or(1);
                        task.outline_level = level.saturating_sub(1);
                    }
                    ("Task", "Duration") => {
                        task.duration_minutes = parse_iso_duration(&value).unwrap_or(0)
                    }
                    ("Task", "Start") => task.start = parse_datetime(&value),
                    ("Task", "Finish") => task.finish = parse_datetime(&value),
                    ("Task", "PercentComplete") => {
                        task.percent_complete = value.trim().parse::<u8>().unwrap_or(0).min(100)
                    }
                    ("Task", "Milestone") => task.milestone = parse_bool(&value),
                    ("Task", "Summary") => task.summary = parse_bool(&value),
                    ("Task", "Manual") => task.manual = parse_bool(&value),
                    ("Task", "Active") => task.active = parse_bool(&value),
                    ("Task", "Notes") => task.notes = value,
                    ("Task", "FixedCost") => task.fixed_cost = value.trim().parse().unwrap_or(0.0),
                    ("Task", "ConstraintType") => {
                        task.constraint_type = value.trim().parse().unwrap_or(0)
                    }
                    ("Task", "ConstraintDate") => task.constraint_date = parse_datetime(&value),
                    ("Task", "Deadline") => task.deadline = parse_datetime(&value),
                    ("Tasks", "Task") => tasks.push(std::mem::take(&mut task)),

                    // ---- links ------------------------------------------
                    ("PredecessorLink", "PredecessorUID") => {
                        predecessor.0 = value.trim().parse().unwrap_or(0)
                    }
                    ("PredecessorLink", "Type") => predecessor.1 = value.trim().parse().unwrap_or(1),
                    ("PredecessorLink", "LinkLag") => {
                        predecessor.2 = value.trim().parse().unwrap_or(0)
                    }
                    ("PredecessorLink", "LagFormat") => {
                        predecessor.3 = value.trim().parse().unwrap_or(7)
                    }
                    ("Task", "PredecessorLink") => task.predecessors.push(predecessor),

                    // ---- resources --------------------------------------
                    ("Resource", "UID") => resource.uid = value.trim().parse().unwrap_or(0),
                    ("Resource", "Name") => resource.name = value,
                    ("Resource", "Initials") => resource.initials = value,
                    ("Resource", "Group") => resource.group = value,
                    ("Resource", "Type") => resource.kind = value.trim().parse().unwrap_or(1),
                    ("Resource", "MaxUnits") => {
                        resource.max_units = value.trim().parse().unwrap_or(1.0)
                    }
                    ("Resource", "StandardRate") => {
                        resource.standard_rate = value.trim().parse().unwrap_or(0.0)
                    }
                    ("Resource", "OvertimeRate") => {
                        resource.overtime_rate = value.trim().parse().unwrap_or(0.0)
                    }
                    ("Resource", "CostPerUse") => {
                        resource.cost_per_use = value.trim().parse().unwrap_or(0.0)
                    }
                    ("Resources", "Resource") => resources.push(std::mem::take(&mut resource)),

                    // ---- assignments ------------------------------------
                    ("Assignment", "TaskUID") => assignment.0 = value.trim().parse().unwrap_or(0),
                    ("Assignment", "ResourceUID") => {
                        assignment.1 = value.trim().parse().unwrap_or(0)
                    }
                    ("Assignment", "Units") => assignment.2 = value.trim().parse().unwrap_or(1.0),
                    ("Assignments", "Assignment") => assignments.push(assignment),

                    // ---- calendar exceptions ----------------------------
                    ("Exception", "Name") => exception_name = value,
                    ("Exception", "DayWorking") => exception_working = parse_bool(&value),
                    ("TimePeriod", "FromDate") => {
                        exception_from = parse_datetime(&value).map(|d| d.date())
                    }
                    ("TimePeriod", "ToDate") => {
                        exception_to = parse_datetime(&value).map(|d| d.date())
                    }
                    ("Exceptions", "Exception") => {
                        if !exception_working
                            && let Some(from) = exception_from {
                                holidays.push((
                                    std::mem::take(&mut exception_name),
                                    from,
                                    exception_to.unwrap_or(from),
                                ));
                            }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if !seen_project {
        return Err(ImportError::NotMspdi);
    }

    Ok(assemble(
        project_name,
        project_start,
        author,
        company,
        currency,
        tasks,
        resources,
        assignments,
        holidays,
    ))
}

#[allow(clippy::too_many_arguments)]
fn assemble(
    name: String,
    start: Option<NaiveDateTime>,
    author: String,
    company: String,
    currency: String,
    raw_tasks: Vec<RawTask>,
    raw_resources: Vec<RawResource>,
    assignments: Vec<(u32, u32, f64)>,
    holidays: Vec<(String, chrono::NaiveDate, chrono::NaiveDate)>,
) -> Project {
    let fallback_start = start
        .or_else(|| raw_tasks.iter().filter_map(|t| t.start).min())
        .unwrap_or_else(|| {
            chrono::NaiveDate::from_ymd_opt(2026, 1, 5)
                .unwrap()
                .and_hms_opt(8, 0, 0)
                .unwrap()
        });

    let mut project = Project::blank(fallback_start);
    project.name = if name.trim().is_empty() {
        "Imported Project".into()
    } else {
        name
    };
    project.author = author;
    project.company = company;
    if !currency.trim().is_empty() {
        project.currency_symbol = currency;
    }

    // Project's own calendar is not read; its holidays are, which is what
    // actually moves dates around.
    project.calendar = WorkCalendar::standard();
    for (label, from, to) in holidays {
        project.calendar.exceptions.push(CalendarException {
            name: if label.trim().is_empty() {
                "Holiday".into()
            } else {
                label
            },
            from,
            to,
            shifts: DayShifts::nonworking(),
        });
    }

    // MSPDI keeps a UID 0 row for the project summary; it is not a real task.
    let rows: Vec<&RawTask> = raw_tasks.iter().filter(|t| t.uid != 0).collect();

    let mut uid_to_id: HashMap<u32, u32> = HashMap::new();
    for raw in &rows {
        let id = project.allocate_task_id();
        uid_to_id.insert(raw.uid, id);

        let duration = if raw.milestone { 0 } else { raw.duration_minutes.max(0) };
        let mut task = Task::new(id, raw.name.clone(), duration);
        task.outline_level = raw.outline_level;
        task.percent_complete = raw.percent_complete;
        task.notes = raw.notes.clone();
        task.fixed_cost = raw.fixed_cost;
        task.active = raw.active;
        task.mode = if raw.manual { TaskMode::Manual } else { TaskMode::Auto };
        task.manual_start = if raw.manual { raw.start } else { None };
        task.constraint = constraint_from(raw.constraint_type);
        task.constraint_date = if task.constraint.needs_date() {
            raw.constraint_date
        } else {
            None
        };
        task.deadline = raw.deadline;
        project.tasks.push(task);
    }

    // Links come second so every UID already has an id.
    for raw in &rows {
        let Some(&successor) = uid_to_id.get(&raw.uid) else {
            continue;
        };
        for (pred_uid, kind, lag, lag_format) in &raw.predecessors {
            let Some(&predecessor) = uid_to_id.get(pred_uid) else {
                continue;
            };
            project.add_link(Link {
                predecessor,
                successor,
                kind: link_from(*kind),
                lag_minutes: lag_minutes(*lag, *lag_format),
            });
        }
    }

    let mut resource_ids: HashMap<u32, u32> = HashMap::new();
    for raw in raw_resources.iter().filter(|r| !r.name.trim().is_empty()) {
        let id = project.allocate_resource_id();
        resource_ids.insert(raw.uid, id);
        let mut resource = Resource::new(id, raw.name.clone());
        if !raw.initials.trim().is_empty() {
            resource.initials = raw.initials.clone();
        }
        resource.group = raw.group.clone();
        resource.kind = match raw.kind {
            0 => ResourceKind::Material,
            2 => ResourceKind::Cost,
            _ => ResourceKind::Work,
        };
        resource.max_units = if raw.max_units > 0.0 { raw.max_units } else { 1.0 };
        resource.standard_rate = raw.standard_rate;
        resource.overtime_rate = raw.overtime_rate;
        resource.cost_per_use = raw.cost_per_use;
        project.resources.push(resource);
    }

    for (task_uid, resource_uid, units) in assignments {
        let (Some(&task_id), Some(&resource_id)) =
            (uid_to_id.get(&task_uid), resource_ids.get(&resource_uid))
        else {
            continue;
        };
        if let Some(task) = project.task_mut(task_id)
            && task.assignments.iter().all(|a| a.resource != resource_id) {
                task.assignments.push(Assignment {
                    resource: resource_id,
                    units: if units > 0.0 { units } else { 1.0 },
                });
            }
    }

    // Keep the plan's own start honest for the scheduler.
    if let Some(earliest) = start {
        project.start_date = earliest;
    }
    project.start_date = project
        .calendar
        .next_working_instant(project.start_date.with_second(0).unwrap_or(project.start_date));

    project
}

/// MSPDI writes LinkLag in tenths of a minute, with LagFormat naming the unit
/// it should be displayed in. Only the amount matters here.
fn lag_minutes(lag: i64, _format: u8) -> i64 {
    lag / 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::schedule;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Project xmlns="http://schemas.microsoft.com/project">
  <Name>Bridge Refit</Name>
  <Author>A Planner</Author>
  <CurrencySymbol>£</CurrencySymbol>
  <StartDate>2026-08-17T08:00:00</StartDate>
  <Calendars><Calendar><UID>1</UID><Name>Standard</Name>
    <Exceptions><Exception><EnteredByOccurrences>0</EnteredByOccurrences>
      <TimePeriod><FromDate>2026-08-19T00:00:00</FromDate><ToDate>2026-08-19T23:59:00</ToDate></TimePeriod>
      <Occurrences>1</Occurrences><Name>Site closed</Name><Type>1</Type><DayWorking>0</DayWorking>
    </Exception></Exceptions>
  </Calendar></Calendars>
  <Tasks>
    <Task><UID>0</UID><ID>0</ID><Name>Bridge Refit</Name><OutlineLevel>0</OutlineLevel><Summary>1</Summary></Task>
    <Task><UID>1</UID><ID>1</ID><Name>Survey</Name><OutlineLevel>1</OutlineLevel>
      <Duration>PT16H0M0S</Duration><Start>2026-08-17T08:00:00</Start><PercentComplete>50</PercentComplete>
      <Milestone>0</Milestone><Summary>0</Summary><Manual>0</Manual><Active>1</Active>
    </Task>
    <Task><UID>2</UID><ID>2</ID><Name>Strip deck</Name><OutlineLevel>1</OutlineLevel>
      <Duration>PT40H0M0S</Duration><Milestone>0</Milestone><Summary>0</Summary><Active>1</Active>
      <ConstraintType>4</ConstraintType><ConstraintDate>2026-08-24T08:00:00</ConstraintDate>
      <PredecessorLink><PredecessorUID>1</PredecessorUID><Type>1</Type><LinkLag>4800</LinkLag><LagFormat>7</LagFormat></PredecessorLink>
    </Task>
    <Task><UID>3</UID><ID>3</ID><Name>Deck clear</Name><OutlineLevel>1</OutlineLevel>
      <Duration>PT0H0M0S</Duration><Milestone>1</Milestone><Summary>0</Summary><Active>1</Active>
      <PredecessorLink><PredecessorUID>2</PredecessorUID><Type>1</Type><LinkLag>0</LinkLag></PredecessorLink>
    </Task>
  </Tasks>
  <Resources>
    <Resource><UID>0</UID><Name></Name></Resource>
    <Resource><UID>1</UID><Name>Ana Reyes</Name><Initials>AR</Initials><Group>Survey</Group>
      <Type>1</Type><MaxUnits>1</MaxUnits><StandardRate>75</StandardRate></Resource>
  </Resources>
  <Assignments>
    <Assignment><UID>1</UID><TaskUID>1</TaskUID><ResourceUID>1</ResourceUID><Units>0.5</Units></Assignment>
  </Assignments>
</Project>
"#;

    #[test]
    fn iso_durations_are_read_in_working_time() {
        assert_eq!(parse_iso_duration("PT8H0M0S"), Some(480));
        assert_eq!(parse_iso_duration("PT40H0M0S"), Some(2400));
        assert_eq!(parse_iso_duration("PT0H0M0S"), Some(0));
        assert_eq!(parse_iso_duration("P2D"), Some(960));
    }

    #[test]
    fn the_project_header_is_read() {
        let project = from_xml(SAMPLE).unwrap();
        assert_eq!(project.name, "Bridge Refit");
        assert_eq!(project.author, "A Planner");
        assert_eq!(project.currency_symbol, "£");
    }

    #[test]
    fn the_outline_is_rebuilt_without_the_project_summary_row() {
        let project = from_xml(SAMPLE).unwrap();
        // UID 0 is Project's own summary row and must not become a task.
        assert_eq!(project.tasks.len(), 3);
        assert_eq!(project.tasks[0].name, "Survey");
        // MSPDI levels count from one, this model counts from zero.
        assert_eq!(project.tasks[0].outline_level, 0);
    }

    #[test]
    fn durations_milestones_and_progress_survive() {
        let project = from_xml(SAMPLE).unwrap();
        assert_eq!(project.tasks[0].duration_minutes, 960);
        assert_eq!(project.tasks[0].percent_complete, 50);
        assert_eq!(project.tasks[1].duration_minutes, 2400);
        assert!(project.tasks[2].is_milestone());
    }

    #[test]
    fn links_carry_their_type_and_lag() {
        let project = from_xml(SAMPLE).unwrap();
        assert_eq!(project.links.len(), 2);
        let first = project.links[0];
        assert_eq!(first.kind, LinkType::FS);
        // 4800 tenths of a minute is 480 minutes, which is one working day.
        assert_eq!(first.lag_minutes, 480);
    }

    #[test]
    fn constraints_map_onto_the_right_kind() {
        let project = from_xml(SAMPLE).unwrap();
        assert_eq!(
            project.tasks[1].constraint,
            ConstraintType::StartNoEarlierThan
        );
        assert!(project.tasks[1].constraint_date.is_some());
        // An As Soon As Possible task must not keep a stray constraint date.
        assert_eq!(project.tasks[0].constraint, ConstraintType::AsSoonAsPossible);
        assert!(project.tasks[0].constraint_date.is_none());
    }

    #[test]
    fn resources_and_their_bookings_come_across() {
        let project = from_xml(SAMPLE).unwrap();
        assert_eq!(project.resources.len(), 1, "the blank UID 0 row is skipped");
        assert_eq!(project.resources[0].name, "Ana Reyes");
        assert_eq!(project.resources[0].standard_rate, 75.0);
        assert_eq!(project.tasks[0].assignments.len(), 1);
        assert_eq!(project.tasks[0].assignments[0].units, 0.5);
    }

    #[test]
    fn calendar_exceptions_become_non_working_days() {
        let project = from_xml(SAMPLE).unwrap();
        let closed = chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap();
        assert!(!project.calendar.is_working_day(closed));
    }

    #[test]
    fn an_imported_plan_schedules_cleanly() {
        let mut project = from_xml(SAMPLE).unwrap();
        let report = schedule(&mut project).unwrap();
        assert!(report.finish > report.start);
        assert!(report.critical_task_count > 0);
    }

    #[test]
    fn a_compound_document_is_handed_to_the_binary_reader() {
        let dir = std::env::temp_dir().join("aop-mspdi-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plan.mpp");
        // Only the magic, so the binary reader will reject it, but it proves
        // the routing: this must not be treated as XML.
        std::fs::write(&path, OLE_MAGIC).unwrap();

        let error = open(&path).unwrap_err();
        assert!(
            matches!(error, ImportError::Mpp(_)),
            "a compound document must go to the mpp reader, got {error:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_non_project_xml_file_is_refused() {
        let error = from_xml("<?xml version=\"1.0\"?><catalogue><book/></catalogue>").unwrap_err();
        assert!(matches!(error, ImportError::NotMspdi));
    }
}
