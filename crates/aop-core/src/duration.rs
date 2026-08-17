//! Parsing and formatting of duration strings such as `5d`, `2 wks`, `4h`.

use crate::{MINUTES_PER_DAY, MINUTES_PER_MONTH, MINUTES_PER_WEEK};

/// The duration unit a value is entered and displayed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
}

impl DurationUnit {
    pub fn minutes(self) -> i64 {
        match self {
            DurationUnit::Minutes => 1,
            DurationUnit::Hours => 60,
            DurationUnit::Days => MINUTES_PER_DAY,
            DurationUnit::Weeks => MINUTES_PER_WEEK,
            DurationUnit::Months => MINUTES_PER_MONTH,
        }
    }

    fn singular(self) -> &'static str {
        match self {
            DurationUnit::Minutes => "min",
            DurationUnit::Hours => "hr",
            DurationUnit::Days => "day",
            DurationUnit::Weeks => "wk",
            DurationUnit::Months => "mon",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            DurationUnit::Minutes => "mins",
            DurationUnit::Hours => "hrs",
            DurationUnit::Days => "days",
            DurationUnit::Weeks => "wks",
            DurationUnit::Months => "mons",
        }
    }
}

fn unit_from_suffix(suffix: &str) -> Option<DurationUnit> {
    match suffix.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "m" | "min" | "mins" | "minute" | "minutes" => Some(DurationUnit::Minutes),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(DurationUnit::Hours),
        "d" | "dy" | "day" | "days" => Some(DurationUnit::Days),
        "w" | "wk" | "wks" | "week" | "weeks" => Some(DurationUnit::Weeks),
        "mo" | "mon" | "mons" | "month" | "months" => Some(DurationUnit::Months),
        _ => None,
    }
}

/// Parse a duration into working minutes. A bare number is read as days, which
/// matches the default unit in Microsoft Project.
///
/// Accepts `5`, `5d`, `5 days`, `1.5w`, `30 mins`, and a trailing `?` for an
/// estimated duration (the flag is returned separately).
pub fn parse_duration(input: &str) -> Option<(i64, bool)> {
    let raw = input.trim();
    if raw.is_empty() {
        return None;
    }
    let estimated = raw.ends_with('?');
    let raw = raw.trim_end_matches('?').trim();

    let split_at = raw
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-'))
        .unwrap_or(raw.len());
    let (number, suffix) = raw.split_at(split_at);

    let value: f64 = number.replace(',', "").parse().ok()?;
    if value < 0.0 {
        return None;
    }
    let unit = unit_from_suffix(suffix).unwrap_or(DurationUnit::Days);

    Some(((value * unit.minutes() as f64).round() as i64, estimated))
}

/// Render working minutes the way the duration column does: the largest unit
/// that divides cleanly, so 2400 minutes reads as `1 wk` rather than `5 days`.
pub fn format_duration(minutes: i64) -> String {
    format_duration_flagged(minutes, false)
}

pub fn format_duration_flagged(minutes: i64, estimated: bool) -> String {
    let mark = if estimated { "?" } else { "" };
    if minutes == 0 {
        return format!("0 days{mark}");
    }

    let unit = if minutes % MINUTES_PER_WEEK == 0 {
        DurationUnit::Weeks
    } else if minutes % MINUTES_PER_DAY == 0 {
        DurationUnit::Days
    } else if minutes % 60 == 0 {
        DurationUnit::Hours
    } else {
        DurationUnit::Minutes
    };

    let whole = minutes / unit.minutes();
    let remainder = minutes % unit.minutes();

    if remainder == 0 {
        let label = if whole == 1 { unit.singular() } else { unit.plural() };
        format!("{whole} {label}{mark}")
    } else {
        let value = minutes as f64 / unit.minutes() as f64;
        format!("{value:.2} {}{mark}", unit.plural())
    }
}

/// Render minutes as a work amount, which is always shown in hours.
pub fn format_work(minutes: i64) -> String {
    if minutes % 60 == 0 {
        format!("{} hrs", minutes / 60)
    } else {
        format!("{:.1} hrs", minutes as f64 / 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_numbers_are_days() {
        assert_eq!(parse_duration("5"), Some((2400, false)));
    }

    #[test]
    fn suffixes_are_understood() {
        assert_eq!(parse_duration("5d"), Some((2400, false)));
        assert_eq!(parse_duration("1w"), Some((2400, false)));
        assert_eq!(parse_duration("4 hrs"), Some((240, false)));
        assert_eq!(parse_duration("90 mins"), Some((90, false)));
    }

    #[test]
    fn estimated_flag_round_trips() {
        assert_eq!(parse_duration("3d?"), Some((1440, true)));
        assert_eq!(format_duration_flagged(1440, true), "3 days?");
    }

    #[test]
    fn formatting_picks_the_largest_clean_unit() {
        assert_eq!(format_duration(0), "0 days");
        assert_eq!(format_duration(480), "1 day");
        assert_eq!(format_duration(1440), "3 days");
        assert_eq!(format_duration(2400), "1 wk");
        assert_eq!(format_duration(240), "4 hrs");
        assert_eq!(format_duration(90), "90 mins");
    }
}
