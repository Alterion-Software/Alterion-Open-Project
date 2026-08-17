//! The catalogue of columns a table can show, matching the fields Microsoft
//! Project offers in Insert Column.
//!
//! A field knows its own heading, its natural width, how it aligns, and how to
//! read itself off a task. Keeping that in one place means the grid, the CSV
//! export and the printed sheet can all be driven by the same list.

use serde::{Deserialize, Serialize};

use crate::duration::{format_duration, format_duration_flagged, format_work};
use crate::model::{ConstraintType, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Field {
    // identity and outline
    Id,
    Wbs,
    OutlineLevel,
    Indicators,
    TaskMode,
    Name,

    // scheduling
    Duration,
    Start,
    Finish,
    LateStart,
    LateFinish,
    TotalSlack,
    FreeSlack,
    Critical,

    // links
    Predecessors,
    Successors,

    // constraints
    ConstraintType,
    ConstraintDate,
    Deadline,

    // progress
    PercentComplete,
    Milestone,
    Summary,
    Active,

    // effort and money
    Work,
    Cost,
    FixedCost,
    ResourceNames,
    ResourceInitials,

    // baseline
    BaselineStart,
    BaselineFinish,
    BaselineDuration,
    StartVariance,
    FinishVariance,

    // free text
    Notes,
}

/// How a column's text sits in its cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Centre,
}

/// The groups Insert Column lists fields under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldGroup {
    Outline,
    Schedule,
    Links,
    Constraints,
    Progress,
    Cost,
    Baseline,
    Text,
}

impl FieldGroup {
    pub const ORDER: [FieldGroup; 8] = [
        FieldGroup::Outline,
        FieldGroup::Schedule,
        FieldGroup::Links,
        FieldGroup::Constraints,
        FieldGroup::Progress,
        FieldGroup::Cost,
        FieldGroup::Baseline,
        FieldGroup::Text,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FieldGroup::Outline => "Outline",
            FieldGroup::Schedule => "Schedule",
            FieldGroup::Links => "Links",
            FieldGroup::Constraints => "Constraints",
            FieldGroup::Progress => "Progress",
            FieldGroup::Cost => "Work and cost",
            FieldGroup::Baseline => "Baseline",
            FieldGroup::Text => "Text",
        }
    }
}

impl Field {
    /// Every field, in the order Insert Column lists them.
    pub const ALL: [Field; 34] = [
        Field::Id,
        Field::Wbs,
        Field::OutlineLevel,
        Field::Indicators,
        Field::TaskMode,
        Field::Name,
        Field::Duration,
        Field::Start,
        Field::Finish,
        Field::LateStart,
        Field::LateFinish,
        Field::TotalSlack,
        Field::FreeSlack,
        Field::Critical,
        Field::Predecessors,
        Field::Successors,
        Field::ConstraintType,
        Field::ConstraintDate,
        Field::Deadline,
        Field::PercentComplete,
        Field::Milestone,
        Field::Summary,
        Field::Active,
        Field::Work,
        Field::Cost,
        Field::FixedCost,
        Field::ResourceNames,
        Field::ResourceInitials,
        Field::BaselineStart,
        Field::BaselineFinish,
        Field::BaselineDuration,
        Field::StartVariance,
        Field::FinishVariance,
        Field::Notes,
    ];

    /// The columns a new plan opens with, the Project Entry table.
    pub const ENTRY_TABLE: [Field; 9] = [
        Field::Id,
        Field::Indicators,
        Field::TaskMode,
        Field::Name,
        Field::Duration,
        Field::Start,
        Field::Finish,
        Field::Predecessors,
        Field::ResourceNames,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Id => "ID",
            Field::Wbs => "WBS",
            Field::OutlineLevel => "Outline Level",
            Field::Indicators => "Indicators",
            Field::TaskMode => "Task Mode",
            Field::Name => "Task Name",
            Field::Duration => "Duration",
            Field::Start => "Start",
            Field::Finish => "Finish",
            Field::LateStart => "Late Start",
            Field::LateFinish => "Late Finish",
            Field::TotalSlack => "Total Slack",
            Field::FreeSlack => "Free Slack",
            Field::Critical => "Critical",
            Field::Predecessors => "Predecessors",
            Field::Successors => "Successors",
            Field::ConstraintType => "Constraint Type",
            Field::ConstraintDate => "Constraint Date",
            Field::Deadline => "Deadline",
            Field::PercentComplete => "% Complete",
            Field::Milestone => "Milestone",
            Field::Summary => "Summary",
            Field::Active => "Active",
            Field::Work => "Work",
            Field::Cost => "Cost",
            Field::FixedCost => "Fixed Cost",
            Field::ResourceNames => "Resource Names",
            Field::ResourceInitials => "Resource Initials",
            Field::BaselineStart => "Baseline Start",
            Field::BaselineFinish => "Baseline Finish",
            Field::BaselineDuration => "Baseline Duration",
            Field::StartVariance => "Start Variance",
            Field::FinishVariance => "Finish Variance",
            Field::Notes => "Notes",
        }
    }

    /// A short heading for the columns whose full name will not fit.
    pub fn heading(self) -> &'static str {
        match self {
            Field::Id => "",
            Field::Indicators => "\u{2139}",
            Field::TaskMode => "\u{25f4}",
            other => other.label(),
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Field::Id => "Row number in the current order",
            Field::Wbs => "Dotted outline number, such as 2.1.3",
            Field::OutlineLevel => "How deeply the task is nested",
            Field::Indicators => "Warnings, notes, deadlines and constraints",
            Field::TaskMode => "Auto or manually scheduled",
            Field::Name => "The task name, indented by its outline level",
            Field::Duration => "How long the work takes, in working time",
            Field::Start => "Scheduled start",
            Field::Finish => "Scheduled finish",
            Field::LateStart => "The latest it can start without delaying the finish",
            Field::LateFinish => "The latest it can finish without delaying the finish",
            Field::TotalSlack => "How far it can slip before the project finish moves",
            Field::FreeSlack => "How far it can slip before anything else moves",
            Field::Critical => "Whether a slip moves the project finish",
            Field::Predecessors => "What has to happen first",
            Field::Successors => "What waits on this",
            Field::ConstraintType => "How the task is pinned to the calendar",
            Field::ConstraintDate => "The date the constraint refers to",
            Field::Deadline => "A date to flag against, which does not move the task",
            Field::PercentComplete => "How much of the work is done",
            Field::Milestone => "Whether it is a zero length marker",
            Field::Summary => "Whether it rolls up other tasks",
            Field::Active => "Inactive tasks are ignored by the scheduler",
            Field::Work => "Booked effort in hours",
            Field::Cost => "Resource cost plus fixed cost",
            Field::FixedCost => "A cost that does not come from a resource",
            Field::ResourceNames => "Who or what is booked, with units",
            Field::ResourceInitials => "The same, abbreviated",
            Field::BaselineStart => "Start when the baseline was taken",
            Field::BaselineFinish => "Finish when the baseline was taken",
            Field::BaselineDuration => "Duration when the baseline was taken",
            Field::StartVariance => "How far the start has moved from the baseline",
            Field::FinishVariance => "How far the finish has moved from the baseline",
            Field::Notes => "Free text kept with the task",
        }
    }

    pub fn group(self) -> FieldGroup {
        match self {
            Field::Id | Field::Wbs | Field::OutlineLevel | Field::Indicators
            | Field::TaskMode | Field::Name => FieldGroup::Outline,
            Field::Duration | Field::Start | Field::Finish | Field::LateStart
            | Field::LateFinish | Field::TotalSlack | Field::FreeSlack | Field::Critical => {
                FieldGroup::Schedule
            }
            Field::Predecessors | Field::Successors => FieldGroup::Links,
            Field::ConstraintType | Field::ConstraintDate | Field::Deadline => {
                FieldGroup::Constraints
            }
            Field::PercentComplete | Field::Milestone | Field::Summary | Field::Active => {
                FieldGroup::Progress
            }
            Field::Work | Field::Cost | Field::FixedCost | Field::ResourceNames
            | Field::ResourceInitials => FieldGroup::Cost,
            Field::BaselineStart | Field::BaselineFinish | Field::BaselineDuration
            | Field::StartVariance | Field::FinishVariance => FieldGroup::Baseline,
            Field::Notes => FieldGroup::Text,
        }
    }

    pub fn default_width(self) -> f64 {
        match self {
            Field::Id => 34.0,
            Field::Indicators | Field::TaskMode => 26.0,
            Field::Name => 210.0,
            Field::Notes => 220.0,
            Field::ResourceNames => 136.0,
            Field::Wbs | Field::OutlineLevel | Field::PercentComplete => 64.0,
            Field::Milestone | Field::Summary | Field::Active | Field::Critical => 62.0,
            Field::Duration | Field::BaselineDuration => 76.0,
            Field::Start | Field::Finish | Field::LateStart | Field::LateFinish
            | Field::ConstraintDate | Field::Deadline | Field::BaselineStart
            | Field::BaselineFinish => 96.0,
            Field::ConstraintType => 128.0,
            _ => 92.0,
        }
    }

    pub fn align(self) -> Align {
        match self {
            Field::Id => Align::Centre,
            Field::Indicators | Field::TaskMode | Field::Milestone | Field::Summary
            | Field::Active | Field::Critical => Align::Centre,
            Field::Duration | Field::PercentComplete | Field::Work | Field::Cost
            | Field::FixedCost | Field::TotalSlack | Field::FreeSlack | Field::OutlineLevel
            | Field::BaselineDuration | Field::StartVariance | Field::FinishVariance => {
                Align::Right
            }
            _ => Align::Left,
        }
    }

    /// Whether a cell of this field can be typed into.
    pub fn editable(self) -> bool {
        matches!(
            self,
            Field::Name
                | Field::Duration
                | Field::Start
                | Field::Finish
                | Field::Predecessors
                | Field::ResourceNames
                | Field::PercentComplete
                | Field::FixedCost
                | Field::Deadline
                | Field::Notes
        )
    }

    /// Read this field off a task, formatted the way the table shows it.
    pub fn value(self, project: &Project, index: usize, date_format: &str) -> String {
        let Some(task) = project.tasks.get(index) else {
            return String::new();
        };
        let date = |value: chrono::NaiveDateTime| value.format(date_format).to_string();
        let signed = |minutes: i64| {
            if minutes < 0 {
                format!("-{}", format_duration(-minutes))
            } else {
                format_duration(minutes)
            }
        };
        let yes_no = |flag: bool| if flag { "Yes" } else { "No" }.to_string();

        match self {
            Field::Id => (index + 1).to_string(),
            Field::Wbs => project.wbs(index),
            Field::OutlineLevel => (task.outline_level + 1).to_string(),
            Field::Indicators => String::new(),
            Field::TaskMode => task.mode.label().to_string(),
            Field::Name => task.name.clone(),
            Field::Duration => {
                format_duration_flagged(task.scheduled.duration_minutes, task.estimated)
            }
            Field::Start => date(task.scheduled.start),
            Field::Finish => date(task.scheduled.finish),
            Field::LateStart => date(task.scheduled.late_start),
            Field::LateFinish => date(task.scheduled.late_finish),
            Field::TotalSlack => signed(task.scheduled.total_slack_minutes),
            Field::FreeSlack => signed(task.scheduled.free_slack_minutes),
            Field::Critical => yes_no(task.scheduled.critical),
            Field::Predecessors => project.predecessor_text(task.id),
            Field::Successors => project
                .successors_of(task.id)
                .into_iter()
                .filter_map(|link| project.index_of(link.successor))
                .map(|i| (i + 1).to_string())
                .collect::<Vec<_>>()
                .join(","),
            Field::ConstraintType => task.constraint.label().to_string(),
            Field::ConstraintDate => task
                .constraint_date
                .filter(|_| task.constraint != ConstraintType::AsSoonAsPossible)
                .map(date)
                .unwrap_or_default(),
            Field::Deadline => task.deadline.map(date).unwrap_or_default(),
            Field::PercentComplete => format!("{}%", task.percent_complete),
            Field::Milestone => yes_no(task.is_milestone()),
            Field::Summary => yes_no(project.is_summary(index)),
            Field::Active => yes_no(task.active),
            Field::Work => format_work(task.scheduled.work_minutes),
            Field::Cost => format!("{}{:.2}", project.currency_symbol, task.scheduled.cost),
            Field::FixedCost => format!("{}{:.2}", project.currency_symbol, task.fixed_cost),
            Field::ResourceNames => project.resource_text(task),
            Field::ResourceInitials => task
                .assignments
                .iter()
                .filter_map(|a| project.resource(a.resource))
                .map(|r| r.initials.clone())
                .collect::<Vec<_>>()
                .join(","),
            Field::BaselineStart => task.baseline.map(|b| date(b.start)).unwrap_or_default(),
            Field::BaselineFinish => task.baseline.map(|b| date(b.finish)).unwrap_or_default(),
            Field::BaselineDuration => task
                .baseline
                .map(|b| format_duration(b.duration_minutes))
                .unwrap_or_default(),
            Field::StartVariance => task
                .start_variance_minutes(&project.calendar)
                .map(signed)
                .unwrap_or_default(),
            Field::FinishVariance => task
                .finish_variance_minutes(&project.calendar)
                .map(signed)
                .unwrap_or_default(),
            Field::Notes => task.notes.replace('\n', " "),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::schedule;
    use crate::templates;

    fn sample() -> Project {
        let start = chrono::NaiveDate::from_ymd_opt(2026, 8, 17)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap();
        let spec = templates::by_id("simple").unwrap();
        let mut project = templates::build(spec, start);
        project.set_baseline();
        schedule(&mut project).unwrap();
        project
    }

    #[test]
    fn every_field_reads_without_panicking() {
        let project = sample();
        for field in Field::ALL {
            for index in 0..project.tasks.len() {
                let _ = field.value(&project, index, "%d/%m/%y");
            }
        }
    }

    #[test]
    fn every_field_is_listed_exactly_once() {
        let mut seen = Field::ALL.to_vec();
        let total = seen.len();
        seen.sort_by_key(|f| format!("{f:?}"));
        seen.dedup();
        assert_eq!(seen.len(), total, "a field is listed twice");
    }

    #[test]
    fn every_field_belongs_to_a_group_that_lists_it() {
        for field in Field::ALL {
            let group = field.group();
            assert!(
                FieldGroup::ORDER.contains(&group),
                "{:?} is in a group the picker never shows",
                field
            );
        }
    }

    #[test]
    fn the_entry_table_is_made_of_real_fields() {
        for field in Field::ENTRY_TABLE {
            assert!(Field::ALL.contains(&field));
            assert!(field.default_width() > 0.0);
        }
    }

    #[test]
    fn values_read_what_they_claim_to() {
        let project = sample();
        // Row 2 of the simple template is the kickoff meeting, one day long.
        assert_eq!(Field::Id.value(&project, 1, "%d/%m/%y"), "2");
        assert_eq!(Field::Wbs.value(&project, 1, "%d/%m/%y"), "1.1");
        assert_eq!(Field::Duration.value(&project, 1, "%d/%m/%y"), "1 day");
        assert_eq!(Field::Summary.value(&project, 0, "%d/%m/%y"), "Yes");
        assert_eq!(Field::Summary.value(&project, 1, "%d/%m/%y"), "No");
        assert!(!Field::BaselineStart.value(&project, 1, "%d/%m/%y").is_empty());
    }

    #[test]
    fn a_task_without_a_baseline_shows_nothing_rather_than_a_wrong_date() {
        let mut project = sample();
        project.clear_baseline();
        assert_eq!(Field::BaselineStart.value(&project, 1, "%d/%m/%y"), "");
        assert_eq!(Field::FinishVariance.value(&project, 1, "%d/%m/%y"), "");
    }
}
