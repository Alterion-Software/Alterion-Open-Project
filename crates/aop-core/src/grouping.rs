//! Group By and Fill Down, the two table operations that work on whole
//! selections rather than one cell.
//!
//! **Group By** never touches the plan. It reads the rows and hands back a
//! view: banded rows, each holding the tasks whose value matches, with the
//! count, work and cost of what sits under it. The outline and the row order
//! survive untouched, so turning grouping off shows exactly the plan that was
//! there before. That is why `group` takes a `&Project` and returns a fresh
//! `Vec<GroupRow>` instead of rewriting anything, and why a `GroupRow::Task`
//! is only an index back into the plan.
//!
//! Summary rows are left out of a grouped view entirely. A summary's work and
//! cost are its children's rolled up, so a band holding both would report the
//! same effort twice; and a phase heading pulled away from the tasks it heads
//! has nothing left to say.
//!
//! **Fill Down** does touch the plan. It takes the value in the first selected
//! row and writes it into the rest of the selection, for one column, and
//! reports how many rows actually moved so the caller can decide whether an
//! undo step is worth recording.
//!
//! Honest limitations:
//!
//! * Bands are exact values, never intervals. Project can group durations into
//!   "1 day to 1 week" buckets; every distinct duration gets its own band here.
//! * Nesting stops after one `then_by`. Project allows several levels, but each
//!   extra level splits the plan finer than a person can read, and two covers
//!   the cases that come up.
//! * A band label is the value alone ("Alice"), not Project's "Resource Names:
//!   Alice". The caller knows which field it asked for and can prefix it.
//! * Dates in band labels use one fixed format, because a grouping has no view
//!   settings of its own to read a preference from.
//! * Filling `Duration` changes what the scheduler is given, not what it
//!   produced. The caller reschedules afterwards.

use std::cmp::Ordering;

use chrono::NaiveDateTime;

use crate::fields::Field;
use crate::model::{Assignment, Project, Task};

/// The label a band gets when the tasks under it have nothing in the column.
pub const NO_VALUE: &str = "No value";

/// How a date reads inside a band label.
pub const LABEL_DATE_FORMAT: &str = "%d/%m/%y";

/// What to group by, and in which direction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupBy {
    pub field: Field,
    pub ascending: bool,
    /// A second field to split each band by. One level only.
    pub then_by: Option<Field>,
}

impl GroupBy {
    /// Group by one field, smallest or earliest or first alphabetically at the
    /// top, which is what a planner reaching for Group By expects to see.
    pub fn new(field: Field) -> Self {
        GroupBy {
            field,
            ascending: true,
            then_by: None,
        }
    }
}

/// One row of a grouped view, in the order it should be drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupRow {
    /// A banner over the tasks that share a value, carrying their totals.
    Band {
        label: String,
        count: usize,
        work_minutes: i64,
        cost: f64,
        /// 0 for a `field` band, 1 for a `then_by` band nested inside one.
        depth: usize,
    },
    /// An index into `project.tasks`, so nothing about the plan is copied.
    Task(usize),
}

/// Build the grouped view of a plan.
pub fn group(project: &Project, spec: &GroupBy) -> Vec<GroupRow> {
    // Summaries are dropped here, once, so no band and no total downstream has
    // to remember to exclude them.
    let leaves: Vec<usize> = (0..project.tasks.len())
        .filter(|&index| !project.is_summary(index))
        .collect();

    let mut view = Vec::with_capacity(leaves.len() + 4);
    for band in bands_of(project, &leaves, spec.field, spec.ascending) {
        view.push(band_row(project, &band, 0));
        match spec.then_by {
            // The outer band already totalled everything beneath it, so the
            // inner bands only have to split the same rows again.
            Some(second) => {
                for inner in bands_of(project, &band.rows, second, spec.ascending) {
                    view.push(band_row(project, &inner, 1));
                    view.extend(inner.rows.iter().map(|&index| GroupRow::Task(index)));
                }
            }
            None => view.extend(band.rows.iter().map(|&index| GroupRow::Task(index))),
        }
    }
    view
}

/// A band under construction: its heading, what it sorts by, and its members.
struct Band {
    label: String,
    key: SortKey,
    rows: Vec<usize>,
}

/// What a band sorts by, kept as the value's own type rather than as the text
/// it prints as, so "9" cannot come out above "10" and a September date cannot
/// come out above an August one.
enum SortKey {
    Number(f64),
    Date(NaiveDateTime),
    /// Lowercased, so "beta" and "Alpha" sort as words rather than by byte.
    Text(String),
    /// The column is empty for this task.
    Absent,
}

fn bands_of(project: &Project, rows: &[usize], field: Field, ascending: bool) -> Vec<Band> {
    let mut bands: Vec<Band> = Vec::new();
    for &index in rows {
        let (label, key) = band_of(project, index, field);
        match bands.iter_mut().find(|band| band.label == label) {
            // Pushing keeps the members in plan order, so grouping rearranges
            // nothing below the band it created.
            Some(band) => band.rows.push(index),
            None => bands.push(Band {
                label,
                key,
                rows: vec![index],
            }),
        }
    }

    // An absent value is not a value, so it is held out of the ordering
    // entirely instead of counting as the smallest or the largest.
    let absent = bands
        .iter()
        .position(|band| matches!(band.key, SortKey::Absent))
        .map(|at| bands.remove(at));

    bands.sort_by(|a, b| {
        let order = compare(&a.key, &b.key);
        let order = if ascending { order } else { order.reverse() };
        // Values that print differently but sort the same still need a settled
        // order, or the view would shuffle between runs.
        order.then_with(|| a.label.cmp(&b.label))
    });
    bands.extend(absent);
    bands
}

fn compare(a: &SortKey, b: &SortKey) -> Ordering {
    match (a, b) {
        (SortKey::Number(a), SortKey::Number(b)) => a.total_cmp(b),
        (SortKey::Date(a), SortKey::Date(b)) => a.cmp(b),
        (SortKey::Text(a), SortKey::Text(b)) => a.cmp(b),
        // One field yields one kind of key, and the absent band never reaches
        // here, so a mismatch has no meaningful answer to give.
        _ => Ordering::Equal,
    }
}

fn band_of(project: &Project, index: usize, field: Field) -> (String, SortKey) {
    let text = field.value(project, index, LABEL_DATE_FORMAT);
    if text.trim().is_empty() {
        return (NO_VALUE.to_string(), SortKey::Absent);
    }
    let key = sort_key(project, index, field, &text);
    (text, key)
}

/// Read the value the band should sort on, from the model rather than from the
/// text, because the text is formatted for reading and not for ordering.
fn sort_key(project: &Project, index: usize, field: Field, text: &str) -> SortKey {
    let words = || SortKey::Text(text.to_lowercase());
    let Some(task) = project.tasks.get(index) else {
        return words();
    };
    let number = |value: i64| SortKey::Number(value as f64);
    let optional = |value: Option<NaiveDateTime>| match value {
        Some(when) => SortKey::Date(when),
        None => words(),
    };

    match field {
        Field::Id => number(index as i64),
        Field::OutlineLevel => number(task.outline_level as i64),
        Field::Duration => number(task.scheduled.duration_minutes),
        Field::TotalSlack => number(task.scheduled.total_slack_minutes),
        Field::FreeSlack => number(task.scheduled.free_slack_minutes),
        Field::PercentComplete => number(task.percent_complete as i64),
        Field::Work => number(task.scheduled.work_minutes),
        Field::Cost => SortKey::Number(task.scheduled.cost),
        Field::FixedCost => SortKey::Number(task.fixed_cost),
        Field::BaselineDuration => match task.baseline {
            Some(baseline) => number(baseline.duration_minutes),
            None => words(),
        },
        Field::StartVariance => match task.start_variance_minutes(&project.calendar) {
            Some(minutes) => number(minutes),
            None => words(),
        },
        Field::FinishVariance => match task.finish_variance_minutes(&project.calendar) {
            Some(minutes) => number(minutes),
            None => words(),
        },
        Field::Start => SortKey::Date(task.scheduled.start),
        Field::Finish => SortKey::Date(task.scheduled.finish),
        Field::LateStart => SortKey::Date(task.scheduled.late_start),
        Field::LateFinish => SortKey::Date(task.scheduled.late_finish),
        Field::ConstraintDate => optional(task.constraint_date),
        Field::Deadline => optional(task.deadline),
        Field::BaselineStart => optional(task.baseline.map(|baseline| baseline.start)),
        Field::BaselineFinish => optional(task.baseline.map(|baseline| baseline.finish)),
        // WBS, names, resources, flags and free text all sort as what they read
        // as, which is what a planner scanning the bands is comparing anyway.
        _ => words(),
    }
}

fn band_row(project: &Project, band: &Band, depth: usize) -> GroupRow {
    let mut work_minutes = 0;
    let mut cost = 0.0;
    for &index in &band.rows {
        if let Some(task) = project.tasks.get(index) {
            work_minutes += task.scheduled.work_minutes;
            cost += task.scheduled.cost;
        }
    }
    GroupRow::Band {
        label: band.label.clone(),
        count: band.rows.len(),
        work_minutes,
        cost,
        depth,
    }
}

/// Whether a column can be filled down.
///
/// The gate starts at `Field::editable`, since a column nobody can type into is
/// not a column a fill may write to either. Three editable fields are held back
/// on top of that:
///
/// * `Start` and `Finish` live in `Scheduled`, which the scheduler owns. Typing
///   a date there is really asking for a constraint, and a fill that wrote the
///   date straight in would be overwritten by the next reschedule anyway.
/// * `Predecessors` reads as row numbers, which mean a different task in every
///   row. Copying the cell down would point the whole selection at one task and
///   could make a row its own predecessor; building links is `add_link`'s job.
pub fn is_fillable(field: Field) -> bool {
    field.editable()
        && !matches!(
            field,
            Field::Start | Field::Finish | Field::Predecessors
        )
}

/// A value lifted off the source task, kept in the model's own types so that
/// filling never round trips through formatted text and cannot lose precision
/// or pick up a parse error on the way.
enum FillValue {
    Name(String),
    Duration { minutes: i64, estimated: bool },
    Assignments(Vec<Assignment>),
    PercentComplete(u8),
    FixedCost(f64),
    Deadline(Option<NaiveDateTime>),
    Notes(String),
}

/// Copy the first selected row's value into the rest of the selection, for one
/// column. Returns how many rows changed.
///
/// `rows` is the selection in the order the table drew it: Project fills from
/// the top of a selection, so the caller passes the topmost row first.
pub fn fill_down(project: &mut Project, field: Field, rows: &[usize]) -> usize {
    if !is_fillable(field) {
        return 0;
    }
    let Some(&source) = rows.first() else {
        return 0;
    };
    // A summary shows its children rolled up, so filling from one would spread
    // a figure that nobody typed and that belongs to other rows.
    if project.is_summary(source) {
        return 0;
    }
    let Some(value) = project.tasks.get(source).and_then(|task| read(task, field)) else {
        return 0;
    };

    let mut changed = 0;
    for &index in rows.iter().skip(1) {
        // A repeated index would otherwise be counted twice on the way past.
        if index == source || project.is_summary(index) {
            continue;
        }
        if let Some(task) = project.tasks.get_mut(index)
            && write(task, &value)
        {
            changed += 1;
        }
    }
    changed
}

fn read(task: &Task, field: Field) -> Option<FillValue> {
    match field {
        Field::Name => Some(FillValue::Name(task.name.clone())),
        // The estimate mark travels with the number, because "3 days?" is a
        // different claim from "3 days".
        Field::Duration => Some(FillValue::Duration {
            minutes: task.duration_minutes,
            estimated: task.estimated,
        }),
        Field::ResourceNames => Some(FillValue::Assignments(task.assignments.clone())),
        Field::PercentComplete => Some(FillValue::PercentComplete(task.percent_complete)),
        Field::FixedCost => Some(FillValue::FixedCost(task.fixed_cost)),
        Field::Deadline => Some(FillValue::Deadline(task.deadline)),
        Field::Notes => Some(FillValue::Notes(task.notes.clone())),
        _ => None,
    }
}

/// Write the value onto a task, reporting whether it was any different from
/// what was already there.
fn write(task: &mut Task, value: &FillValue) -> bool {
    match value {
        FillValue::Name(name) => {
            let changed = &task.name != name;
            task.name = name.clone();
            changed
        }
        FillValue::Duration { minutes, estimated } => {
            let changed = task.duration_minutes != *minutes || task.estimated != *estimated;
            task.duration_minutes = *minutes;
            task.estimated = *estimated;
            changed
        }
        FillValue::Assignments(assignments) => {
            let changed = &task.assignments != assignments;
            task.assignments = assignments.clone();
            changed
        }
        FillValue::PercentComplete(percent) => {
            let changed = task.percent_complete != *percent;
            task.percent_complete = *percent;
            changed
        }
        FillValue::FixedCost(cost) => {
            let changed = (task.fixed_cost - cost).abs() > f64::EPSILON;
            task.fixed_cost = *cost;
            changed
        }
        FillValue::Deadline(deadline) => {
            let changed = task.deadline != *deadline;
            task.deadline = *deadline;
            changed
        }
        FillValue::Notes(notes) => {
            let changed = &task.notes != notes;
            task.notes = notes.clone();
            changed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MINUTES_PER_DAY;
    use chrono::NaiveDate;

    fn moment(year: i32, month: u32, day: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap()
    }

    fn blank() -> Project {
        Project::blank(moment(2026, 8, 17))
    }

    /// Add a leaf row and give it the figures a scheduled plan would carry.
    fn leaf(project: &mut Project, name: &str, days: i64, work_hours: i64, cost: f64) -> usize {
        let minutes = days * MINUTES_PER_DAY;
        project.push_task(name, minutes);
        let index = project.tasks.len() - 1;
        if let Some(task) = project.tasks.get_mut(index) {
            task.scheduled.duration_minutes = minutes;
            task.scheduled.work_minutes = work_hours * 60;
            task.scheduled.cost = cost;
        }
        index
    }

    fn set_level(project: &mut Project, index: usize, level: u16) {
        if let Some(task) = project.tasks.get_mut(index) {
            task.outline_level = level;
        }
    }

    fn set_notes(project: &mut Project, index: usize, notes: &str) {
        if let Some(task) = project.tasks.get_mut(index) {
            task.notes = notes.into();
        }
    }

    /// A phase holding three tasks, then one loose task with an empty column.
    ///
    /// Row 0 is a summary, so nothing in the tests below should ever see it.
    fn plan() -> Project {
        let mut project = blank();
        leaf(&mut project, "Phase", 0, 0, 0.0);
        let design = leaf(&mut project, "Design", 2, 8, 100.0);
        let build = leaf(&mut project, "Build", 3, 16, 200.0);
        let test = leaf(&mut project, "Test", 1, 4, 50.0);
        leaf(&mut project, "Ship", 1, 2, 25.0);

        for index in [design, build, test] {
            set_level(&mut project, index, 1);
        }
        set_notes(&mut project, design, "Alice");
        set_notes(&mut project, build, "Bob");
        set_notes(&mut project, test, "Alice");
        project
    }

    fn labels(view: &[GroupRow]) -> Vec<String> {
        view.iter()
            .filter_map(|row| match row {
                GroupRow::Band { label, .. } => Some(label.clone()),
                GroupRow::Task(_) => None,
            })
            .collect()
    }

    fn tasks(view: &[GroupRow]) -> Vec<usize> {
        view.iter()
            .filter_map(|row| match row {
                GroupRow::Task(index) => Some(*index),
                GroupRow::Band { .. } => None,
            })
            .collect()
    }

    fn band(view: &[GroupRow], wanted: &str) -> (usize, i64, f64, usize) {
        for row in view {
            if let GroupRow::Band {
                label,
                count,
                work_minutes,
                cost,
                depth,
            } = row
                && label == wanted
            {
                return (*count, *work_minutes, *cost, *depth);
            }
        }
        panic!("no band called {wanted}");
    }

    #[test]
    fn every_leaf_appears_once_and_no_summary_appears_at_all() {
        // A summary's work and cost are its children's, so banding it beside
        // them would count the same effort twice.
        let project = plan();
        let view = group(&project, &GroupBy::new(Field::Notes));
        let mut seen = tasks(&view);
        seen.sort_unstable();
        assert_eq!(seen, vec![1, 2, 3, 4]);
        assert!(!seen.contains(&0), "row 0 is the phase summary");
    }

    #[test]
    fn a_task_with_no_value_collects_under_its_own_band_last() {
        let project = plan();
        let view = group(&project, &GroupBy::new(Field::Notes));
        assert_eq!(labels(&view), vec!["Alice", "Bob", NO_VALUE]);
    }

    #[test]
    fn the_no_value_band_stays_last_when_the_order_is_reversed() {
        // An absent value is not a value, so reversing the order of real values
        // must not promote emptiness to the top of the view.
        let project = plan();
        let spec = GroupBy {
            field: Field::Notes,
            ascending: false,
            then_by: None,
        };
        assert_eq!(labels(&group(&project, &spec)), vec!["Bob", "Alice", NO_VALUE]);
    }

    #[test]
    fn numbers_sort_as_numbers_so_eleven_days_comes_after_nine() {
        // As text "11 days" sorts above "9 days", which is wrong in exactly the
        // range plans live in.
        let mut project = blank();
        leaf(&mut project, "Long", 11, 0, 0.0);
        leaf(&mut project, "Short", 9, 0, 0.0);
        let view = group(&project, &GroupBy::new(Field::Duration));
        assert_eq!(labels(&view), vec!["9 days", "11 days"]);
    }

    #[test]
    fn dates_sort_as_dates_not_as_the_text_they_print_as() {
        // Printed day first, "01/09/26" sorts above "30/08/26" as text while
        // being the later date.
        let mut project = blank();
        let september = leaf(&mut project, "Later", 1, 0, 0.0);
        let august = leaf(&mut project, "Earlier", 1, 0, 0.0);
        if let Some(task) = project.tasks.get_mut(september) {
            task.deadline = Some(moment(2026, 9, 1));
        }
        if let Some(task) = project.tasks.get_mut(august) {
            task.deadline = Some(moment(2026, 8, 30));
        }
        let view = group(&project, &GroupBy::new(Field::Deadline));
        assert_eq!(labels(&view), vec!["30/08/26", "01/09/26"]);
    }

    #[test]
    fn a_band_totals_the_work_and_cost_of_the_tasks_under_it() {
        let project = plan();
        let view = group(&project, &GroupBy::new(Field::Notes));
        let (count, work, cost, depth) = band(&view, "Alice");
        assert_eq!(count, 2);
        assert_eq!(work, (8 + 4) * 60);
        assert!((cost - 150.0).abs() < f64::EPSILON);
        assert_eq!(depth, 0);
    }

    #[test]
    fn tasks_keep_their_plan_order_inside_a_band() {
        // Grouping is a view over the plan, not a resort of it.
        let project = plan();
        let view = group(&project, &GroupBy::new(Field::Notes));
        assert_eq!(tasks(&view), vec![1, 3, 2, 4]);
    }

    #[test]
    fn then_by_nests_a_second_level_of_bands_inside_the_first() {
        let mut project = plan();
        if let Some(task) = project.tasks.get_mut(1) {
            task.scheduled.critical = true;
        }
        let spec = GroupBy {
            field: Field::Notes,
            ascending: true,
            then_by: Some(Field::Critical),
        };
        let view = group(&project, &spec);
        assert_eq!(
            labels(&view),
            vec!["Alice", "No", "Yes", "Bob", "No", NO_VALUE, "No"]
        );

        // The outer band still totals everything beneath it, inner bands
        // included, so splitting a band further never loses its figures.
        let (count, work, _, depth) = band(&view, "Alice");
        assert_eq!(count, 2, "both of Alice's tasks, across two inner bands");
        assert_eq!(work, (8 + 4) * 60);
        assert_eq!(depth, 0);
    }

    #[test]
    fn filling_copies_the_first_selected_rows_value_into_the_rest() {
        let mut project = plan();
        assert_eq!(fill_down(&mut project, Field::Notes, &[1, 2, 4]), 2);
        assert_eq!(project.tasks[2].notes, "Alice");
        assert_eq!(project.tasks[4].notes, "Alice");
        assert_eq!(project.tasks[3].notes, "Alice", "row 3 was never selected");
    }

    #[test]
    fn a_column_that_cannot_be_typed_into_is_refused_rather_than_filled() {
        // Finish and Cost are the scheduler's answers, not anybody's input, so
        // a fill that wrote them would be quietly claiming a false plan.
        let mut project = plan();
        assert_eq!(fill_down(&mut project, Field::Cost, &[1, 2, 3]), 0);
        assert_eq!(fill_down(&mut project, Field::Finish, &[1, 2, 3]), 0);
        assert_eq!(fill_down(&mut project, Field::Predecessors, &[1, 2, 3]), 0);
        assert!((project.tasks[2].scheduled.cost - 200.0).abs() < f64::EPSILON);
    }

    #[test]
    fn every_fillable_column_is_one_a_planner_could_type_into() {
        for field in Field::ALL {
            if is_fillable(field) {
                assert!(field.editable(), "{field:?} is filled but not editable");
            }
        }
    }

    #[test]
    fn summary_rows_are_skipped_because_their_figures_are_their_childrens() {
        let mut project = plan();
        set_notes(&mut project, 0, "Phase note");
        // Row 0 is the summary, and it sits between the source and the target.
        assert_eq!(fill_down(&mut project, Field::Notes, &[1, 0, 2]), 1);
        assert_eq!(project.tasks[0].notes, "Phase note");
        assert_eq!(project.tasks[2].notes, "Alice");

        // Filling out of a summary spreads a value nobody typed.
        assert_eq!(fill_down(&mut project, Field::Notes, &[0, 4]), 0);
        assert_eq!(project.tasks[4].notes, "");
    }

    #[test]
    fn only_the_rows_that_actually_changed_are_counted() {
        // The count is what tells the caller whether an undo step is warranted,
        // so a row that already held the value must not inflate it.
        let mut project = plan();
        assert_eq!(fill_down(&mut project, Field::Notes, &[1, 3, 2]), 1);
        // Row 3 already held the value, and row 1 is the source repeated.
        assert_eq!(fill_down(&mut project, Field::Notes, &[1, 1, 3]), 0);
        // Nothing to fill into is not a fill at all.
        assert_eq!(fill_down(&mut project, Field::Notes, &[1]), 0);
        assert_eq!(fill_down(&mut project, Field::Notes, &[]), 0);
    }

    #[test]
    fn a_filled_duration_keeps_its_estimate_mark() {
        // "3 days?" and "3 days" are different claims about how well the work
        // is understood, so the mark travels with the number.
        let mut project = plan();
        if let Some(task) = project.tasks.get_mut(1) {
            task.estimated = true;
        }
        assert_eq!(fill_down(&mut project, Field::Duration, &[1, 2]), 1);
        assert_eq!(project.tasks[2].duration_minutes, 2 * MINUTES_PER_DAY);
        assert!(project.tasks[2].estimated);
    }
}
