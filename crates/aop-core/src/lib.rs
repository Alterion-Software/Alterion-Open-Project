//! Scheduling core for Alterion Open Project.
//!
//! This crate holds the plan and the maths and knows nothing about the user
//! interface, so the critical path engine can be exercised entirely from tests.

pub mod calendar;
pub mod duration;
pub mod fields;
pub mod model;
pub mod mpp;
pub mod mspdi;
pub mod persist;
pub mod schedule;
pub mod templates;

pub use calendar::{CalendarException, DayShifts, Shift, WorkCalendar};
pub use fields::{Align, Field, FieldGroup};
pub use duration::{
    format_duration, format_duration_flagged, format_work, parse_duration, DurationUnit,
};
pub use model::{
    Assignment, BarStyles, Baseline, ConstraintType, Link, LinkType, Project, Resource, ResourceId,
    ResourceKind, ScheduleFrom, Scheduled, Task, TaskId, TaskMode,
};
pub use mspdi::ImportError;
pub use persist::{FileError, FILE_EXTENSION, FILE_TYPE_NAME};
pub use schedule::{
    apply_remedy, critical_reason, diagnose, schedule, Overallocation, Remedy, ScheduleError,
    ScheduleReport,
};

/// Working minutes in one hour, day, week and month.
///
/// These mirror the Microsoft Project defaults, where a "day" of duration means
/// eight hours of work rather than twenty-four hours of wall clock.
pub const MINUTES_PER_HOUR: i64 = 60;
pub const MINUTES_PER_DAY: i64 = 8 * MINUTES_PER_HOUR;
pub const MINUTES_PER_WEEK: i64 = 5 * MINUTES_PER_DAY;
pub const MINUTES_PER_MONTH: i64 = 20 * MINUTES_PER_DAY;

/// Product name shown in the title bar and the Backstage view.
pub const APP_NAME: &str = "Alterion Open Project";
