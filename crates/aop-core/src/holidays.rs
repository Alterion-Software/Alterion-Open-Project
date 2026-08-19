//! Public holidays, read out of an iCalendar file.
//!
//! A plan that schedules work on Christmas Day is wrong, and typing every
//! holiday by hand for every year is how that goes on being wrong. The
//! calendar already has the shape for it in `CalendarException`, and `mspdi`
//! already fills those in from a Project file, so this is a new source for a
//! structure that exists rather than anything new in the model.
//!
//! `.ics` because that is what governments, Google and Outlook publish
//! holidays as, so somebody can download their own country's file. A table of
//! holidays shipped in the binary would be wrong for most people and stale for
//! everybody inside a year, and fetching from an API would put somebody else's
//! uptime between a planner and their plan.
//!
//! Nothing here decides *whose* days these are. It reads a file into
//! `CalendarException` values and adds the ones a calendar has not already got;
//! where they land is the caller's choice, and Change Working Time makes it an
//! explicit one. Aimed at the project calendar or a base they are public
//! holidays and apply to everybody. Aimed at a person they are that person's
//! leave, which is what an `.ics` exported from somebody's own calendar
//! application actually contains.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use crate::calendar::{CalendarException, DayShifts, WorkCalendar};

#[derive(Debug)]
pub enum IcsError {
    Io(std::io::Error),
    /// The file opened and is not an iCalendar file.
    NotACalendar,
}

impl std::fmt::Display for IcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IcsError::Io(error) => write!(f, "{error}"),
            IcsError::NotACalendar => write!(
                f,
                "This is not an iCalendar file. Holiday calendars are published as .ics files, which begin with BEGIN:VCALENDAR."
            ),
        }
    }
}

impl std::error::Error for IcsError {}

impl From<std::io::Error> for IcsError {
    fn from(error: std::io::Error) -> Self {
        IcsError::Io(error)
    }
}

/// A yearly repeat, in the parts of `RRULE` that a holiday actually uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Yearly {
    pub interval: i32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
    /// From `BYMONTH`, when the rule names a month other than the event's own.
    pub month: Option<u32>,
    pub day: Option<u32>,
    /// From `BYDAY` with an ordinal: the fourth Thursday, the last Monday.
    pub weekday: Option<(i32, Weekday)>,
}

/// One event out of the file, before it is spread over any years.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occasion {
    pub name: String,
    pub from: NaiveDate,
    /// Inclusive. An all-day event's `DTEND` is the morning after, which is
    /// the single most common way to read one of these files wrongly: every
    /// holiday comes out a day long when it should not be and two days long
    /// when it should.
    pub to: NaiveDate,
    pub rule: Option<Yearly>,
    /// There is a repeat rule here that this does not know how to spread out.
    /// Said plainly rather than quietly importing the one occurrence, because
    /// a holiday that lands once and never again is a trap.
    pub unhandled_rule: bool,
    pub excluded: Vec<NaiveDate>,
}

/// What the file turned out to hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// `X-WR-CALNAME`, which is what the publisher calls the calendar.
    pub name: Option<String>,
    pub occasions: Vec<Occasion>,
    /// Events with a time of day. A public holiday is a whole day off, so an
    /// event that starts at half past nine is a meeting somebody left in the
    /// file, and it is counted here rather than turned into a day of shutdown.
    pub timed: usize,
}

/// One holiday on one date, ready to become a calendar exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    pub name: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Worked out from a repeat rule rather than written out in the file.
    pub repeating: bool,
}

impl Found {
    /// Every holiday that falls in the years given, rules spread out.
    ///
    /// A range because a downloaded file covers ten years and a plan covers
    /// one, and an exception for 2034 in a plan that finishes in 2027 is
    /// clutter somebody has to scroll past forever.
    pub fn between(&self, first_year: i32, last_year: i32) -> Vec<Holiday> {
        let mut out: Vec<Holiday> = Vec::new();
        for occasion in &self.occasions {
            let span = (occasion.to - occasion.from).num_days().max(0);
            match &occasion.rule {
                None => {
                    if occasion.from.year() >= first_year && occasion.from.year() <= last_year {
                        out.push(Holiday {
                            name: occasion.name.clone(),
                            from: occasion.from,
                            to: occasion.to,
                            repeating: false,
                        });
                    }
                }
                Some(rule) => {
                    let mut seen = 0u32;
                    // From the event's own year, not the window's: COUNT and
                    // INTERVAL are counted from the rule's first occurrence,
                    // so starting anywhere else would land on the wrong years.
                    for year in occasion.from.year()..=last_year {
                        if let Some(count) = rule.count
                            && seen >= count
                        {
                            break;
                        }
                        let Some(date) = occurrence(occasion, rule, year) else {
                            continue;
                        };
                        if date < occasion.from {
                            continue;
                        }
                        if rule.until.is_some_and(|until| date > until) {
                            break;
                        }
                        seen += 1;
                        if occasion.excluded.contains(&date) {
                            continue;
                        }
                        if date.year() < first_year || date.year() > last_year {
                            continue;
                        }
                        out.push(Holiday {
                            name: occasion.name.clone(),
                            from: date,
                            to: date + Duration::days(span),
                            repeating: true,
                        });
                    }
                }
            }
        }
        out.sort_by_key(|holiday| (holiday.from, holiday.name.clone()));
        out.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.name == b.name);
        out
    }

    /// The years the file has anything to say about, for the range to start at.
    pub fn span(&self) -> Option<(i32, i32)> {
        let first = self.occasions.iter().map(|o| o.from.year()).min()?;
        let last = self
            .occasions
            .iter()
            .map(|occasion| match &occasion.rule {
                // A rule with no end goes on forever, and a range that runs to
                // forever is no range, so an open ended rule is shown as
                // reaching the end of the written out dates instead.
                Some(rule) => rule.until.map(|until| until.year()).unwrap_or(occasion.to.year()),
                None => occasion.to.year(),
            })
            .max()?;
        Some((first, last.max(first)))
    }

    /// Events whose repeat rule this does not spread out, by name.
    pub fn unhandled(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .occasions
            .iter()
            .filter(|occasion| occasion.unhandled_rule)
            .map(|occasion| occasion.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

/// Where a yearly rule lands in one year.
fn occurrence(occasion: &Occasion, rule: &Yearly, year: i32) -> Option<NaiveDate> {
    let interval = rule.interval.max(1);
    if (year - occasion.from.year()) % interval != 0 {
        return None;
    }
    let month = rule.month.unwrap_or_else(|| occasion.from.month());
    match rule.weekday {
        Some((ordinal, weekday)) => nth_weekday(year, month, ordinal, weekday),
        // 29 February simply does not happen in most years, and inventing the
        // 28th for it would be inventing a holiday.
        None => NaiveDate::from_ymd_opt(year, month, rule.day.unwrap_or_else(|| occasion.from.day())),
    }
}

/// The nth weekday of a month, counting from the end when n is negative.
fn nth_weekday(year: i32, month: u32, ordinal: i32, weekday: Weekday) -> Option<NaiveDate> {
    if ordinal > 0 {
        let first = NaiveDate::from_ymd_opt(year, month, 1)?;
        let shift = (7 + weekday.num_days_from_monday() as i32
            - first.weekday().num_days_from_monday() as i32)
            % 7;
        let day = 1 + shift + (ordinal - 1) * 7;
        NaiveDate::from_ymd_opt(year, month, u32::try_from(day).ok()?)
    } else if ordinal < 0 {
        let last = last_day_of(year, month)?;
        let shift = (7 + last.weekday().num_days_from_monday() as i32
            - weekday.num_days_from_monday() as i32)
            % 7;
        let day = last.day() as i32 - shift + (ordinal + 1) * 7;
        if day < 1 {
            return None;
        }
        NaiveDate::from_ymd_opt(year, month, u32::try_from(day).ok()?)
    } else {
        None
    }
}

fn last_day_of(year: i32, month: u32) -> Option<NaiveDate> {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.map(|date| date - Duration::days(1))
}

// ---------------------------------------------------------------- reading

pub fn read(path: &std::path::Path) -> Result<Found, IcsError> {
    let bytes = std::fs::read(path)?;
    // These files are UTF-8 by the specification and are not always. A
    // holiday whose name has one bad byte in it is still a holiday.
    let text = String::from_utf8_lossy(&bytes);
    parse(&text)
}

/// Undo line folding: a long line is continued on the next one with a leading
/// space or tab. Real files fold constantly, and a parser that reads lines
/// straight off the file loses the end of every long name.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        lines.push(line.to_string());
    }
    lines
}

/// Split a content line into its property name, its parameters and its value.
fn split_line(line: &str) -> Option<(String, Vec<String>, String)> {
    let mut quoted = false;
    let mut colon = None;
    for (index, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ':' if !quoted => {
                colon = Some(index);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = line.split_at(colon);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params: Vec<String> = parts.map(|part| part.trim().to_ascii_uppercase()).collect();
    Some((name, params, value[1..].to_string()))
}

/// Text values escape their commas, semicolons and newlines.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n') | Some('N') => out.push(' '),
            Some(escaped) => out.push(escaped),
            None => {}
        }
    }
    out.trim().to_string()
}

/// A date value, which is either eight digits or a stamp.
fn parse_date(value: &str) -> Option<(NaiveDate, bool)> {
    let value = value.trim();
    let head = value.split(['T', 't']).next().unwrap_or(value);
    let date = NaiveDate::parse_from_str(head, "%Y%m%d").ok()?;
    let timed = match value.split(['T', 't']).nth(1) {
        // Midnight to midnight is a whole day however the file spells it.
        Some(clock) => !clock.trim_end_matches(['Z', 'z']).starts_with("000000"),
        None => false,
    };
    Some((date, timed))
}

fn parse_weekday(code: &str) -> Option<Weekday> {
    match code {
        "MO" => Some(Weekday::Mon),
        "TU" => Some(Weekday::Tue),
        "WE" => Some(Weekday::Wed),
        "TH" => Some(Weekday::Thu),
        "FR" => Some(Weekday::Fri),
        "SA" => Some(Weekday::Sat),
        "SU" => Some(Weekday::Sun),
        _ => None,
    }
}

/// Read an `RRULE`.
///
/// `Ok(Some)` is a yearly rule this can spread out, `Ok(None)` is no rule at
/// all, and `Err` is a rule this cannot spread out and will say so about.
fn parse_rule(value: &str) -> Result<Yearly, ()> {
    let mut rule = Yearly {
        interval: 1,
        count: None,
        until: None,
        month: None,
        day: None,
        weekday: None,
    };
    let mut yearly = false;
    for part in value.split(';') {
        let Some((key, setting)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_uppercase();
        let setting = setting.trim();
        match key.as_str() {
            "FREQ" => yearly = setting.eq_ignore_ascii_case("YEARLY"),
            "INTERVAL" => rule.interval = setting.parse().unwrap_or(1),
            "COUNT" => rule.count = setting.parse().ok(),
            "UNTIL" => rule.until = parse_date(setting).map(|(date, _)| date),
            "BYMONTH" => rule.month = setting.split(',').next().and_then(|m| m.parse().ok()),
            "BYMONTHDAY" => rule.day = setting.split(',').next().and_then(|d| d.parse().ok()),
            "BYDAY" => {
                let token = setting.split(',').next().unwrap_or(setting).to_ascii_uppercase();
                let split = token.len().saturating_sub(2);
                let (ordinal, code) = token.split_at(split);
                let Some(weekday) = parse_weekday(code) else {
                    return Err(());
                };
                // A BYDAY with no ordinal means every one of that weekday,
                // which is a working pattern rather than a holiday.
                let Ok(ordinal) = ordinal.parse::<i32>() else {
                    return Err(());
                };
                rule.weekday = Some((ordinal, weekday));
            }
            // A rule this does not implement must not be half applied.
            "BYSETPOS" | "BYWEEKNO" | "BYYEARDAY" => return Err(()),
            _ => {}
        }
    }
    if yearly { Ok(rule) } else { Err(()) }
}

/// Read the events out of an iCalendar file.
///
/// Anything that is not a `VEVENT` is skipped without complaint: these files
/// carry timezone definitions, alarms and to-do items, and none of them are
/// holidays.
pub fn parse(text: &str) -> Result<Found, IcsError> {
    let lines = unfold(text);
    if !lines
        .iter()
        .any(|line| line.trim().eq_ignore_ascii_case("BEGIN:VCALENDAR"))
    {
        return Err(IcsError::NotACalendar);
    }

    let mut found = Found::default();
    // What we are inside of. Only the innermost matters, which is how an
    // alarm buried in an event keeps its own SUMMARY out of the holiday list.
    let mut stack: Vec<String> = Vec::new();
    let mut current: Option<Draft> = None;

    for line in &lines {
        let Some((name, params, value)) = split_line(line) else {
            continue;
        };
        match name.as_str() {
            "BEGIN" => {
                let component = value.trim().to_ascii_uppercase();
                if component == "VEVENT" && stack.last().is_none_or(|top| top == "VCALENDAR") {
                    current = Some(Draft::default());
                }
                stack.push(component);
                continue;
            }
            "END" => {
                let component = value.trim().to_ascii_uppercase();
                stack.pop();
                if component == "VEVENT"
                    && let Some(draft) = current.take()
                {
                    match draft.finish() {
                        Some(occasion) => found.occasions.push(occasion),
                        None if draft.timed => found.timed += 1,
                        None => {}
                    }
                }
                continue;
            }
            "X-WR-CALNAME" => {
                if found.name.is_none() {
                    found.name = Some(unescape(&value));
                }
                continue;
            }
            _ => {}
        }

        // Only properties of the event itself, not of an alarm inside it.
        if stack.last().map(String::as_str) != Some("VEVENT") {
            continue;
        }
        let Some(draft) = current.as_mut() else {
            continue;
        };
        draft.take(&name, &params, &value);
    }

    Ok(found)
}

#[derive(Debug, Default)]
struct Draft {
    summary: Option<String>,
    from: Option<NaiveDate>,
    end: Option<NaiveDate>,
    timed: bool,
    rule: Option<Yearly>,
    unhandled_rule: bool,
    excluded: Vec<NaiveDate>,
}

impl Draft {
    fn take(&mut self, name: &str, params: &[String], value: &str) {
        match name {
            "SUMMARY" => self.summary = Some(unescape(value)),
            "DTSTART" => {
                if let Some((date, timed)) = parse_date(value) {
                    self.from = Some(date);
                    // The parameter is the file saying so outright; the stamp
                    // is the file saying so by having a clock in it.
                    self.timed |= timed && !params.iter().any(|p| p == "VALUE=DATE");
                }
            }
            "DTEND" => {
                if let Some((date, timed)) = parse_date(value) {
                    self.end = Some(date);
                    self.timed |= timed && !params.iter().any(|p| p == "VALUE=DATE");
                }
            }
            "RRULE" => match parse_rule(value) {
                Ok(rule) => self.rule = Some(rule),
                Err(()) => self.unhandled_rule = true,
            },
            "EXDATE" => {
                for part in value.split(',') {
                    if let Some((date, _)) = parse_date(part) {
                        self.excluded.push(date);
                    }
                }
            }
            _ => {}
        }
    }

    fn finish(&self) -> Option<Occasion> {
        if self.timed {
            return None;
        }
        let from = self.from?;
        // DTEND is the morning after the last day, so the last day is the day
        // before it. A missing or backwards end means the event is one day.
        let to = match self.end {
            Some(end) if end > from => end - Duration::days(1),
            _ => from,
        };
        Some(Occasion {
            name: match &self.summary {
                Some(text) if !text.is_empty() => text.clone(),
                _ => "Holiday".to_string(),
            },
            from,
            to,
            rule: self.rule.clone(),
            unhandled_rule: self.unhandled_rule,
            excluded: self.excluded.clone(),
        })
    }
}

// --------------------------------------------------------- into a calendar

/// Whether the calendar already has this day off.
///
/// By date rather than by name: importing a second file that calls the same
/// day something else must not book the day off twice.
pub fn already_held(calendar: &WorkCalendar, holiday: &Holiday) -> bool {
    calendar
        .exceptions
        .iter()
        .any(|held| held.from == holiday.from && held.to == holiday.to)
}

/// Add the holidays the calendar does not already have. Returns how many.
pub fn add(calendar: &mut WorkCalendar, holidays: &[Holiday]) -> usize {
    let mut added = 0;
    for holiday in holidays {
        if already_held(calendar, holiday) {
            continue;
        }
        calendar.exceptions.push(CalendarException {
            name: holiday.name.clone(),
            from: holiday.from,
            to: holiday.to,
            shifts: DayShifts::nonworking(),
        });
        added += 1;
    }
    calendar.exceptions.sort_by_key(|exception| exception.from);
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like the files people actually download: a folded line, a
    /// timezone block, an alarm inside an event, and an exclusive DTEND.
    const REAL: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Somebody//Holidays//EN\r\n\
X-WR-CALNAME:United Kingdom Holidays\r\n\
BEGIN:VTIMEZONE\r\n\
TZID:Europe/London\r\n\
BEGIN:DAYLIGHT\r\n\
TZNAME:BST\r\n\
END:DAYLIGHT\r\n\
END:VTIMEZONE\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20261225\r\n\
DTEND;VALUE=DATE:20261226\r\n\
SUMMARY:Christmas Day\r\n\
BEGIN:VALARM\r\n\
ACTION:DISPLAY\r\n\
SUMMARY:Reminder\r\n\
END:VALARM\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20261228\r\n\
DTEND;VALUE=DATE:20261231\r\n\
SUMMARY:Winter shutdown\\, works closed\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
DTSTART;VALUE=DATE:20260501\r\n\
DTEND;VALUE=DATE:20260502\r\n\
SUMMARY:A holiday with a name long enough that the publisher folded the l\r\n\
\x20ine in the middle of it\r\n\
END:VEVENT\r\n\
BEGIN:VEVENT\r\n\
DTSTART;TZID=Europe/London:20260610T093000\r\n\
DTEND;TZID=Europe/London:20260610T103000\r\n\
SUMMARY:Site meeting\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    fn found() -> Found {
        parse(REAL).expect("a calendar")
    }

    #[test]
    fn the_publishers_name_for_the_calendar_comes_through() {
        assert_eq!(found().name.as_deref(), Some("United Kingdom Holidays"));
    }

    #[test]
    fn an_exclusive_end_date_does_not_stretch_a_holiday() {
        // The trap: DTEND is the morning after. Reading it as the last day
        // makes every one day holiday two days long.
        let christmas = &found().occasions[0];
        assert_eq!(christmas.name, "Christmas Day");
        assert_eq!(christmas.from, NaiveDate::from_ymd_opt(2026, 12, 25).unwrap());
        assert_eq!(christmas.to, NaiveDate::from_ymd_opt(2026, 12, 25).unwrap());
    }

    #[test]
    fn a_multi_day_shutdown_keeps_all_of_its_days() {
        let shutdown = &found().occasions[1];
        assert_eq!(shutdown.from, NaiveDate::from_ymd_opt(2026, 12, 28).unwrap());
        assert_eq!(shutdown.to, NaiveDate::from_ymd_opt(2026, 12, 30).unwrap());
        assert_eq!(shutdown.name, "Winter shutdown, works closed");
    }

    #[test]
    fn a_folded_line_is_put_back_together() {
        // Real files fold constantly, and a parser that reads lines straight
        // off the file loses the end of every long name.
        let name = &found().occasions[2].name;
        assert!(name.ends_with("in the middle of it"), "{name}");
        assert!(!name.contains("  "), "the fold must not leave a gap: {name}");
    }

    #[test]
    fn an_event_with_a_time_of_day_is_not_a_holiday() {
        // A whole office is not off work because somebody left a site meeting
        // in the file. Counted rather than silently dropped.
        let found = found();
        assert_eq!(found.timed, 1);
        assert!(found.occasions.iter().all(|o| o.name != "Site meeting"));
    }

    #[test]
    fn timezones_and_alarms_are_skipped_without_complaint() {
        assert_eq!(found().occasions.len(), 3);
        assert!(found().occasions.iter().all(|o| o.name != "Reminder"));
    }

    #[test]
    fn a_yearly_rule_lands_on_every_year_of_the_range() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20200101\n\
DTEND;VALUE=DATE:20200102\nSUMMARY:New Year's Day\nRRULE:FREQ=YEARLY\n\
END:VEVENT\nEND:VCALENDAR\n";
        let holidays = parse(text).expect("a calendar").between(2026, 2028);
        assert_eq!(holidays.len(), 3);
        assert_eq!(holidays[0].from, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(holidays[2].from, NaiveDate::from_ymd_opt(2028, 1, 1).unwrap());
        assert!(holidays[0].repeating);
    }

    #[test]
    fn a_rule_that_names_a_weekday_of_a_month_is_worked_out() {
        // The fourth Thursday of November, which is how a real file writes a
        // holiday that moves every year.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20201126\n\
SUMMARY:Thanksgiving\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=4TH\nEND:VEVENT\nEND:VCALENDAR\n";
        let holidays = parse(text).expect("a calendar").between(2026, 2026);
        assert_eq!(
            holidays[0].from,
            NaiveDate::from_ymd_opt(2026, 11, 26).unwrap()
        );
    }

    #[test]
    fn a_rule_counted_from_the_end_of_the_month_is_worked_out() {
        // The last Monday in May.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20200525\n\
SUMMARY:Spring bank holiday\nRRULE:FREQ=YEARLY;BYMONTH=5;BYDAY=-1MO\nEND:VEVENT\nEND:VCALENDAR\n";
        let holidays = parse(text).expect("a calendar").between(2026, 2026);
        assert_eq!(holidays[0].from, NaiveDate::from_ymd_opt(2026, 5, 25).unwrap());
    }

    #[test]
    fn until_and_count_stop_a_rule() {
        let until = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260101\n\
SUMMARY:Ends\nRRULE:FREQ=YEARLY;UNTIL=20270101\nEND:VEVENT\nEND:VCALENDAR\n";
        assert_eq!(parse(until).expect("a calendar").between(2026, 2030).len(), 2);

        let count = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260101\n\
SUMMARY:Three times\nRRULE:FREQ=YEARLY;COUNT=3\nEND:VEVENT\nEND:VCALENDAR\n";
        assert_eq!(parse(count).expect("a calendar").between(2026, 2030).len(), 3);
    }

    #[test]
    fn an_excluded_date_is_left_out() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260101\n\
SUMMARY:Yearly\nRRULE:FREQ=YEARLY\nEXDATE;VALUE=DATE:20270101\nEND:VEVENT\nEND:VCALENDAR\n";
        let holidays = parse(text).expect("a calendar").between(2026, 2028);
        assert_eq!(holidays.len(), 2);
        assert!(holidays.iter().all(|h| h.from.year() != 2027));
    }

    #[test]
    fn a_rule_this_cannot_spread_out_is_owned_up_to() {
        // Silently importing one occurrence of a monthly rule would leave a
        // plan with eleven months of holidays it does not know about.
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20260101\n\
SUMMARY:First of the month\nRRULE:FREQ=MONTHLY\nEND:VEVENT\nEND:VCALENDAR\n";
        let found = parse(text).expect("a calendar");
        assert_eq!(found.unhandled(), vec!["First of the month".to_string()]);
        // The one occurrence written in the file still comes in.
        assert_eq!(found.between(2026, 2026).len(), 1);
    }

    #[test]
    fn a_leap_day_holiday_skips_the_years_that_do_not_have_one() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20240229\n\
SUMMARY:Leap day\nRRULE:FREQ=YEARLY\nEND:VEVENT\nEND:VCALENDAR\n";
        let holidays = parse(text).expect("a calendar").between(2025, 2028);
        assert_eq!(holidays.len(), 1);
        assert_eq!(holidays[0].from, NaiveDate::from_ymd_opt(2028, 2, 29).unwrap());
    }

    #[test]
    fn a_range_keeps_the_years_a_plan_cares_about() {
        let holidays = found().between(2027, 2030);
        assert!(holidays.is_empty(), "the file is all 2026");
        assert_eq!(found().between(2026, 2026).len(), 3);
    }

    #[test]
    fn holidays_go_into_the_calendar_once() {
        let mut calendar = WorkCalendar::standard();
        let holidays = found().between(2026, 2026);
        assert_eq!(add(&mut calendar, &holidays), 3);
        // A second import of the same file must not book the days off twice.
        assert_eq!(add(&mut calendar, &holidays), 0);
        assert_eq!(calendar.exceptions.len(), 3);
        assert!(!calendar.is_working_day(
            NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()
        ));
    }

    #[test]
    fn a_file_that_is_not_a_calendar_is_refused() {
        assert!(matches!(parse("hello"), Err(IcsError::NotACalendar)));
    }

    #[test]
    fn an_event_with_no_summary_still_books_the_day_off() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART;VALUE=DATE:20261225\n\
END:VEVENT\nEND:VCALENDAR\n";
        let holidays = parse(text).expect("a calendar").between(2026, 2026);
        assert_eq!(holidays[0].name, "Holiday");
    }
}
