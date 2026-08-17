//! Working-time calendar.
//!
//! Every date in the scheduler is expressed in *working* time, not wall time.
//! A "day" of duration is 8 working hours, and adding one day to Friday 08:00
//! lands on Friday 17:00, while adding two days lands on Monday 17:00.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

/// Upper bound on day-by-day iteration so a malformed calendar (for example one
/// with no working days at all) can never spin forever.
const MAX_DAY_SCAN: i64 = 365 * 50;

/// The last representable instant in a day. chrono keeps `end_of_day()`
/// private, so it is rebuilt here.
fn end_of_day() -> NaiveTime {
    NaiveTime::from_hms_nano_opt(23, 59, 59, 999_999_999).expect("valid end of day")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shift {
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl Shift {
    pub fn new(sh: u32, sm: u32, eh: u32, em: u32) -> Self {
        Self {
            start: NaiveTime::from_hms_opt(sh, sm, 0).expect("valid shift start"),
            end: NaiveTime::from_hms_opt(eh, em, 0).expect("valid shift end"),
        }
    }

    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes().max(0)
    }
}

/// The working shifts for a single day. An empty shift list means non-working.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayShifts {
    pub shifts: Vec<Shift>,
}

impl DayShifts {
    pub fn nonworking() -> Self {
        Self { shifts: Vec::new() }
    }

    /// 08:00-12:00 and 13:00-17:00, the Microsoft Project "Standard" day.
    pub fn standard() -> Self {
        Self {
            shifts: vec![Shift::new(8, 0, 12, 0), Shift::new(13, 0, 17, 0)],
        }
    }

    pub fn night() -> Self {
        Self {
            shifts: vec![Shift::new(23, 0, 23, 59), Shift::new(0, 0, 3, 0), Shift::new(4, 0, 8, 0)],
        }
    }

    pub fn minutes(&self) -> i64 {
        self.shifts.iter().map(Shift::minutes).sum()
    }

    pub fn is_working(&self) -> bool {
        self.minutes() > 0
    }
}

/// A named date range that overrides the normal weekly pattern: holidays,
/// shutdowns, or extra working weekends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarException {
    pub name: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub shifts: DayShifts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkCalendar {
    pub name: String,
    /// Seven entries, Monday through Sunday.
    pub week: Vec<DayShifts>,
    pub exceptions: Vec<CalendarException>,
}

impl Default for WorkCalendar {
    fn default() -> Self {
        Self::standard()
    }
}

impl WorkCalendar {
    /// Mon-Fri 08:00-12:00, 13:00-17:00. Saturday and Sunday non-working.
    pub fn standard() -> Self {
        Self {
            name: "Standard".into(),
            week: vec![
                DayShifts::standard(),
                DayShifts::standard(),
                DayShifts::standard(),
                DayShifts::standard(),
                DayShifts::standard(),
                DayShifts::nonworking(),
                DayShifts::nonworking(),
            ],
            exceptions: Vec::new(),
        }
    }

    /// Round the clock, every day.
    pub fn twenty_four_hour() -> Self {
        let all_day = DayShifts {
            shifts: vec![Shift::new(0, 0, 23, 59)],
        };
        Self {
            name: "24 Hours".into(),
            week: vec![all_day; 7],
            exceptions: Vec::new(),
        }
    }

    /// A single 23:00-08:00 graveyard shift, Mon-Fri.
    pub fn night_shift() -> Self {
        Self {
            name: "Night Shift".into(),
            week: vec![
                DayShifts::night(),
                DayShifts::night(),
                DayShifts::night(),
                DayShifts::night(),
                DayShifts::night(),
                DayShifts::nonworking(),
                DayShifts::nonworking(),
            ],
            exceptions: Vec::new(),
        }
    }

    fn weekday_index(date: NaiveDate) -> usize {
        date.weekday().num_days_from_monday() as usize
    }

    /// The shifts that actually apply on `date`, exceptions taking priority.
    pub fn shifts_on(&self, date: NaiveDate) -> &DayShifts {
        for ex in &self.exceptions {
            if date >= ex.from && date <= ex.to {
                return &ex.shifts;
            }
        }
        self.week
            .get(Self::weekday_index(date))
            .unwrap_or(&self.week[0])
    }

    pub fn exception_on(&self, date: NaiveDate) -> Option<&CalendarException> {
        self.exceptions
            .iter()
            .find(|ex| date >= ex.from && date <= ex.to)
    }

    pub fn is_working_day(&self, date: NaiveDate) -> bool {
        self.shifts_on(date).is_working()
    }

    pub fn minutes_in_day(&self, date: NaiveDate) -> i64 {
        self.shifts_on(date).minutes()
    }

    /// The first working instant at or after `dt`.
    pub fn next_working_instant(&self, dt: NaiveDateTime) -> NaiveDateTime {
        let mut date = dt.date();
        let mut time = dt.time();
        for _ in 0..MAX_DAY_SCAN {
            for shift in &self.shifts_on(date).shifts {
                if time < shift.end {
                    return date.and_time(time.max(shift.start));
                }
            }
            date += Duration::days(1);
            time = NaiveTime::MIN;
        }
        dt
    }

    /// The last working instant at or before `dt`.
    pub fn prev_working_instant(&self, dt: NaiveDateTime) -> NaiveDateTime {
        let mut date = dt.date();
        let mut time = dt.time();
        for _ in 0..MAX_DAY_SCAN {
            for shift in self.shifts_on(date).shifts.iter().rev() {
                if time > shift.start {
                    return date.and_time(time.min(shift.end));
                }
            }
            date -= Duration::days(1);
            time = end_of_day();
        }
        dt
    }

    /// Advance `from` by `minutes` of working time.
    ///
    /// A zero-minute advance snaps forward onto the next working instant, which
    /// is what a milestone needs.
    pub fn add_minutes(&self, from: NaiveDateTime, minutes: i64) -> NaiveDateTime {
        let mut cursor = self.next_working_instant(from);
        if minutes <= 0 {
            return cursor;
        }
        let mut remaining = minutes;

        for _ in 0..MAX_DAY_SCAN {
            let date = cursor.date();
            let shifts = self.shifts_on(date).shifts.clone();
            for shift in shifts {
                let segment_start = cursor.time().max(shift.start);
                if segment_start >= shift.end {
                    continue;
                }
                let available = (shift.end - segment_start).num_minutes();
                if available >= remaining {
                    return date.and_time(segment_start + Duration::minutes(remaining));
                }
                remaining -= available;
                cursor = date.and_time(shift.end);
            }
            cursor = self.next_working_instant((date + Duration::days(1)).and_time(NaiveTime::MIN));
        }
        cursor
    }

    /// Walk `minutes` of working time backwards from `from`.
    pub fn sub_minutes(&self, from: NaiveDateTime, minutes: i64) -> NaiveDateTime {
        let mut cursor = self.prev_working_instant(from);
        if minutes <= 0 {
            return cursor;
        }
        let mut remaining = minutes;

        for _ in 0..MAX_DAY_SCAN {
            let date = cursor.date();
            let shifts = self.shifts_on(date).shifts.clone();
            for shift in shifts.iter().rev() {
                let segment_end = cursor.time().min(shift.end);
                if segment_end <= shift.start {
                    continue;
                }
                let available = (segment_end - shift.start).num_minutes();
                if available >= remaining {
                    return date.and_time(segment_end - Duration::minutes(remaining));
                }
                remaining -= available;
                cursor = date.and_time(shift.start);
            }
            cursor = self.prev_working_instant((date - Duration::days(1)).and_time(end_of_day()));
        }
        cursor
    }

    /// Working minutes in the half-open interval `[a, b)`. Negative when `b < a`.
    pub fn work_minutes_between(&self, a: NaiveDateTime, b: NaiveDateTime) -> i64 {
        if b < a {
            return -self.work_minutes_between(b, a);
        }
        let mut total = 0i64;
        let mut date = a.date();
        let last = b.date();
        let mut scanned = 0i64;

        while date <= last && scanned < MAX_DAY_SCAN {
            for shift in &self.shifts_on(date).shifts {
                let lo = date.and_time(shift.start).max(a);
                let hi = date.and_time(shift.end).min(b);
                if hi > lo {
                    total += (hi - lo).num_minutes();
                }
            }
            date += Duration::days(1);
            scanned += 1;
        }
        total
    }

    /// Whether an instant is a working moment, counting the exact end of a
    /// shift as one.
    ///
    /// 17:00 is not a moment you can *start* work, but it is a real point on
    /// the clock, and a milestone is allowed to sit there.
    pub fn is_working_boundary(&self, at: NaiveDateTime) -> bool {
        let time = at.time();
        self.shifts_on(at.date())
            .shifts
            .iter()
            .any(|shift| time >= shift.start && time <= shift.end)
    }

    /// Place a zero-duration marker.
    ///
    /// Unlike the start of a task, this must not roll forward off the end of a
    /// working day: a milestone that lands at 17:00 on Wednesday belongs on
    /// Wednesday, not at 08:00 on Thursday.
    pub fn snap_marker(&self, at: NaiveDateTime) -> NaiveDateTime {
        if self.is_working_boundary(at) {
            at
        } else {
            self.next_working_instant(at)
        }
    }

    /// How far through a day's working time an instant sits, from 0.0 to 1.0.
    ///
    /// This is what lets a chart draw bars against day columns: the end of the
    /// working day is 1.0, so a task finishing at 17:00 reaches the right edge
    /// of its column and a task starting at 08:00 the next morning begins
    /// exactly there, with no false gap for the night in between.
    pub fn day_fraction(&self, at: NaiveDateTime) -> f64 {
        let date = at.date();
        let minutes = self.minutes_in_day(date);
        if minutes <= 0 {
            return 0.0;
        }
        let midnight = date.and_hms_opt(0, 0, 0).expect("valid midnight");
        (self.work_minutes_between(midnight, at) as f64 / minutes as f64).clamp(0.0, 1.0)
    }

    /// The position of an instant measured in day columns from `origin`.
    pub fn day_offset(&self, origin: NaiveDate, at: NaiveDateTime) -> f64 {
        (at.date() - origin).num_days() as f64 + self.day_fraction(at)
    }

    /// Whole working days between two dates, used for timescale drawing.
    pub fn working_days_between(&self, a: NaiveDate, b: NaiveDate) -> i64 {
        let (lo, hi, sign) = if b >= a { (a, b, 1) } else { (b, a, -1) };
        let mut count = 0i64;
        let mut date = lo;
        while date < hi {
            if self.is_working_day(date) {
                count += 1;
            }
            date += Duration::days(1);
        }
        count * sign
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn one_day_of_work_ends_at_five() {
        let cal = WorkCalendar::standard();
        // Monday 2026-08-17 08:00 + 1 day (480 min) => same day 17:00
        assert_eq!(cal.add_minutes(dt(2026, 8, 17, 8, 0), 480), dt(2026, 8, 17, 17, 0));
    }

    #[test]
    fn lunch_break_is_skipped() {
        let cal = WorkCalendar::standard();
        // 11:00 + 2h spans the 12:00-13:00 break and lands at 14:00
        assert_eq!(cal.add_minutes(dt(2026, 8, 17, 11, 0), 120), dt(2026, 8, 17, 14, 0));
    }

    #[test]
    fn weekend_is_skipped() {
        let cal = WorkCalendar::standard();
        // Friday 2026-08-21 08:00 + 2 days => Monday 2026-08-24 17:00
        assert_eq!(cal.add_minutes(dt(2026, 8, 21, 8, 0), 960), dt(2026, 8, 24, 17, 0));
    }

    #[test]
    fn finish_snaps_to_next_working_start() {
        let cal = WorkCalendar::standard();
        // A successor starting the instant a Friday-17:00 task ends begins Monday 08:00
        assert_eq!(
            cal.next_working_instant(dt(2026, 8, 21, 17, 0)),
            dt(2026, 8, 24, 8, 0)
        );
    }

    #[test]
    fn subtraction_is_the_inverse_of_addition() {
        let cal = WorkCalendar::standard();
        let start = dt(2026, 8, 17, 8, 0);
        for minutes in [30, 480, 960, 2400, 5000] {
            let end = cal.add_minutes(start, minutes);
            assert_eq!(cal.sub_minutes(end, minutes), start, "roundtrip {minutes}m");
        }
    }

    #[test]
    fn span_measures_working_minutes_only() {
        let cal = WorkCalendar::standard();
        // Friday 08:00 to Monday 08:00 is exactly one working day
        assert_eq!(
            cal.work_minutes_between(dt(2026, 8, 21, 8, 0), dt(2026, 8, 24, 8, 0)),
            480
        );
    }

    #[test]
    fn a_marker_stays_where_it_lands() {
        let cal = WorkCalendar::standard();
        // The end of Wednesday is a real instant, so a marker stays there
        // rather than rolling on to Thursday morning.
        assert_eq!(cal.snap_marker(dt(2026, 9, 2, 17, 0)), dt(2026, 9, 2, 17, 0));
        // Mid-morning is fine too.
        assert_eq!(cal.snap_marker(dt(2026, 9, 2, 10, 0)), dt(2026, 9, 2, 10, 0));
        // But a marker cannot sit in the middle of the night or a weekend.
        assert_eq!(cal.snap_marker(dt(2026, 9, 2, 21, 0)), dt(2026, 9, 3, 8, 0));
        assert_eq!(cal.snap_marker(dt(2026, 8, 22, 10, 0)), dt(2026, 8, 24, 8, 0));
    }

    #[test]
    fn the_end_of_a_working_day_is_the_end_of_its_column() {
        let cal = WorkCalendar::standard();
        // Wednesday 17:00 is the far edge of Wednesday.
        assert_eq!(cal.day_fraction(dt(2026, 9, 2, 17, 0)), 1.0);
        // Thursday 08:00 is the near edge of Thursday.
        assert_eq!(cal.day_fraction(dt(2026, 9, 3, 8, 0)), 0.0);
        // Lunchtime is halfway through the eight hours.
        assert_eq!(cal.day_fraction(dt(2026, 9, 2, 12, 0)), 0.5);
    }

    #[test]
    fn consecutive_tasks_meet_with_no_gap() {
        let cal = WorkCalendar::standard();
        let origin = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap();
        // One task finishing Wednesday 17:00 and the next starting Thursday
        // 08:00 must land on exactly the same column position.
        let finish = cal.day_offset(origin, dt(2026, 9, 2, 17, 0));
        let next_start = cal.day_offset(origin, dt(2026, 9, 3, 8, 0));
        assert_eq!(
            finish, next_start,
            "back to back tasks must not leave a gap for the night"
        );
    }

    #[test]
    fn a_weekend_gap_is_still_shown() {
        let cal = WorkCalendar::standard();
        let origin = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap();
        // Friday 17:00 to Monday 08:00 should still span the weekend.
        let friday = cal.day_offset(origin, dt(2026, 8, 21, 17, 0));
        let monday = cal.day_offset(origin, dt(2026, 8, 24, 8, 0));
        assert!((monday - friday - 2.0).abs() < 1e-9, "expected two days of weekend");
    }

    #[test]
    fn holidays_are_not_working_time() {
        let mut cal = WorkCalendar::standard();
        cal.exceptions.push(CalendarException {
            name: "Company shutdown".into(),
            from: NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 8, 18).unwrap(),
            shifts: DayShifts::nonworking(),
        });
        // Monday 08:00 + 2 days now skips Tuesday and ends Wednesday 17:00
        assert_eq!(cal.add_minutes(dt(2026, 8, 17, 8, 0), 960), dt(2026, 8, 19, 17, 0));
    }
}
