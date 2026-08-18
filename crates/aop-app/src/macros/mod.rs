//! Macros: recording what a planner did and doing it again.
//!
//! A macro here is a short script of commands from a fixed vocabulary. It is
//! not a scripting language: there are no variables, no arithmetic and no way
//! to reach the disk, the network or the process. That is a deliberate ceiling
//! rather than an unfinished floor. A plan file arrives by email as often as it
//! arrives from a colleague's hands, and a plan that can carry a macro has to
//! be a plan that is safe to open. Everything a macro can do is something the
//! planner could have done from the ribbon, and Undo takes all of it back.
//!
//! The three pieces:
//!
//! * [`Cmd`] is the vocabulary, the replay engine and the recording format at
//!   once, generated from one table in `cmd.rs`.
//! * [`script`] turns a `Vec<Cmd>` into the text that gets stored, and back.
//! * [`MacroDef`] is one named macro: what it is called, what it says it does,
//!   what runs it, and its body as text.
//!
//! The change log is the first thing to call in here: every edit is written
//! down as the command that made it. The recorder and the Developer tab are
//! still to come, which is why the module allows dead code; the allow comes
//! off when the last of the vocabulary has a caller.
#![allow(dead_code)]

pub mod cmd;
pub mod script;

// The re-exports are the module's front door. Some of it is still only walked
// through by the tests, hence the allow alongside the one on the module itself.
#[allow(unused_imports)]
pub use cmd::{Cmd, ResourceField, Row, ViewOption};

use serde::{Deserialize, Serialize};

use aop_core::Field;

use crate::state::AppState;

/// The version stamped into a script's header.
///
/// Bumped only when an old script would be read wrongly rather than merely
/// incompletely, because a reader that refuses a file it could have understood
/// is worse than one that skips a line it cannot.
pub const FORMAT_VERSION: u32 = 1;

/// The file extension a macro gets when it is saved on its own.
pub const MACRO_EXTENSION: &str = "aopm";

/// How long a macro name may be. Long enough for a sentence in Snake_Case,
/// short enough to sit in a menu.
const MAX_NAME_LENGTH: usize = 64;

/// One named macro.
///
/// Serde because this rides in two places: inside the plan file, so a plan can
/// carry the macros that belong to it, and in a standalone `.aopm` file that
/// can be shared on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroDef {
    /// A letter, then letters, digits and underscores. Validated the way
    /// Project validates a macro name, so a plan written here opens there.
    pub name: String,
    /// What it does, in the planner's words. Shown in the macro list and
    /// written into the script header.
    pub description: String,
    /// Written the way `keymap` writes a binding, such as `Ctrl+Shift+M`.
    /// `None` means it is only reachable from the macro list.
    pub shortcut: Option<String>,
    /// The script itself, as text.
    ///
    /// Text and not a `Vec<Cmd>` on purpose. What the planner reads and edits
    /// has to be the thing that runs, or the editor is showing a translation.
    pub body: String,
    /// True when this came from the recorder rather than from somebody typing.
    /// Worth keeping: recording over a hand-written macro loses work, and this
    /// is what lets the interface ask first.
    pub recorded: bool,
}

impl MacroDef {
    /// A new, empty macro under a checked name.
    pub fn new(name: &str, description: &str) -> Result<MacroDef, MacroError> {
        check_name(name)?;
        Ok(MacroDef {
            name: name.to_string(),
            description: description.to_string(),
            shortcut: None,
            body: script::to_script(name, description, &[]),
            recorded: false,
        })
    }

    /// Turn what the recorder collected into a macro.
    pub fn from_recording(
        name: &str,
        description: &str,
        body: &[Cmd],
    ) -> Result<MacroDef, MacroError> {
        check_name(name)?;
        Ok(MacroDef {
            name: name.to_string(),
            description: description.to_string(),
            shortcut: None,
            body: script::to_script(name, description, body),
            recorded: true,
        })
    }

    /// The commands this macro asks for, or where the text stops making sense.
    pub fn commands(&self) -> Result<Vec<Cmd>, MacroError> {
        script::parse(&self.body)
    }

    /// Run the macro against a plan.
    ///
    /// Every command is read before any of them runs, so a script with a typo
    /// on its last line changes nothing at all. Once running, a command that
    /// fails stops the rest: carrying on past a failure would leave the plan
    /// halfway through something nobody asked for.
    pub fn run(&self, state: &mut AppState) -> Result<(), MacroError> {
        let steps = script::parse_lines(&self.body)?;
        for (line, command) in steps {
            command.apply(state).map_err(|why| MacroError::Failed {
                line,
                command: command.fn_name(),
                why: why.to_string(),
            })?;
        }
        Ok(())
    }
}

/// Whether a name is one a macro may be called.
///
/// The same rule Project uses: a letter first, then letters, digits and
/// underscores. It rules out spaces and punctuation, which matters because the
/// name is written into the script header as a bare word and read back off it.
/// Run a script against the plan, as a single step.
///
/// The whole run is one snapshot and one schedule, however many commands it
/// holds. Letting each command take its own would clone the entire project
/// per command and push the planner's real history off the undo stack, and
/// would run the critical path pass to the same wasted count.
///
/// A command that fails stops the run. What ran before it stays: the planner
/// can read how far it got from the count and undo the lot in one keystroke.
///
/// The whole run is one entry in the change log, naming the macro. The
/// commands inside it are not written down one by one: they were recorded when
/// somebody first did them by hand, and a log that counted them again would
/// say the work happened twice.
pub fn run(state: &mut crate::state::AppState, script: &str) -> Result<usize, MacroError> {
    let commands = script::parse(script)?;
    let name = script::read_header(script).name;
    let outcome = state.unrecorded(|state| {
        state.as_one_step(|state| {
            let mut done = 0usize;
            for command in &commands {
                match command.apply(state) {
                    Ok(()) => done += 1,
                    Err(error) => return Err((done, error)),
                }
            }
            Ok(done)
        })
    });

    // What actually ran, which is not always what was asked for. A run that
    // stopped part way leaves its earlier commands in the plan, so the entry
    // holds those and only those: the log has to replay to the plan that is
    // there, not to the one the script hoped for.
    let done = match &outcome {
        Ok(done) => *done,
        Err((done, _)) => *done,
    };
    if done > 0 {
        state.write_change(
            commands[..done]
                .iter()
                .map(Cmd::to_script)
                .collect::<Vec<String>>()
                .join("\n"),
            describe_run(name.as_deref(), done, commands.len()),
        );
    }

    outcome.map_err(|(done, error)| {
        // Saying where it stopped is the difference between a usable message
        // and one that just says something went wrong.
        MacroError::Stopped {
            after: done,
            reason: Box::new(error),
        }
    })
}

/// What the change log says about a run.
///
/// A run that stopped part way says so, because an entry that named the macro
/// alone would read as if the whole thing had been carried out.
fn describe_run(name: Option<&str>, done: usize, asked: usize) -> String {
    let named = match name {
        Some(name) if !name.trim().is_empty() => format!("the macro {}", name.trim()),
        _ => "a macro".to_string(),
    };
    if done == asked {
        format!("Ran {named}")
    } else {
        format!("Ran {done} of the {asked} commands in {named}")
    }
}

pub fn check_name(name: &str) -> Result<(), MacroError> {
    let bad = |why: &str| {
        Err(MacroError::BadName {
            name: name.to_string(),
            why: why.to_string(),
        })
    };
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return bad("a macro needs a name");
    };
    if !first.is_alphabetic() {
        return bad("a macro name has to start with a letter");
    }
    if !characters.all(|character| character.is_alphanumeric() || character == '_') {
        return bad("a macro name can only hold letters, digits and underscores");
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return bad("that name is too long to show in a menu");
    }
    Ok(())
}

/// Everything that can go wrong reading or running a macro.
///
/// One type rather than a parse error and a run error, because the planner sees
/// one thing go wrong and wants one message about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacroError {
    /// A run stopped part way. `after` is how many commands had already been
    /// carried out, so the planner can be told where it got to.
    Stopped {
        after: usize,
        reason: Box<MacroError>,
    },
    /// The script text stopped making sense on this line.
    Syntax { line: usize, message: String },
    /// A command was read and then refused to run.
    Failed {
        line: usize,
        command: &'static str,
        why: String,
    },
    BadName {
        name: String,
        why: String,
    },
    /// Rows are numbered from 1 the way the ID column shows them.
    RowsCountFromOne,
    NoSuchRow {
        row: u32,
        rows: usize,
    },
    NoSuchResource {
        name: String,
    },
    NothingSelected,
    NeedsTwoRows,
    ClipboardEmpty,
    NothingToUndo,
    NothingToRedo,
    SelfLink {
        row: u32,
    },
    /// The scheduler would not accept the link, so it was rolled back.
    LinkRefused {
        predecessor: u32,
        successor: u32,
    },
    /// The state undid the change itself and explained why.
    Refused {
        what: String,
        why: String,
    },
    /// The field exists but no cell writes it.
    FieldNotWritable {
        field: Field,
    },
    FieldNotFillable {
        field: Field,
    },
    ValueNotUnderstood {
        field: Field,
        value: String,
    },
    PercentOutOfRange {
        percent: u8,
    },
    CannotGroupBy {
        field: Field,
    },
    CannotSortBy {
        field: Field,
    },
    ColumnAlreadyShown {
        field: Field,
    },
    ColumnNotShown {
        field: Field,
    },
    /// The table always shows something.
    LastColumn,
}

impl MacroError {
    pub(crate) fn syntax(line: usize, message: impl Into<String>) -> MacroError {
        MacroError::Syntax {
            line,
            message: message.into(),
        }
    }

    /// The line of the script this is about, when it is about one.
    pub fn line(&self) -> Option<usize> {
        match self {
            MacroError::Syntax { line, .. } | MacroError::Failed { line, .. } => Some(*line),
            _ => None,
        }
    }
}

impl std::fmt::Display for MacroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MacroError::Stopped { after, reason } => {
                let done = if *after == 1 {
                    "1 command ran".to_string()
                } else {
                    format!("{after} commands ran")
                };
                write!(
                    f,
                    "The macro stopped part way: {done} before it did. {reason}\n\n\
                     Undo puts the whole run back in one step."
                )
            }
            MacroError::Syntax { line, message } => write!(f, "Line {line}: {message}"),
            MacroError::Failed { line, command, why } => {
                write!(f, "Line {line}: {command} could not run. {why}")
            }
            MacroError::BadName { name, why } => write!(f, "{name} will not do as a name: {why}"),
            MacroError::RowsCountFromOne => {
                write!(f, "Rows are numbered from 1, the way the ID column shows them")
            }
            MacroError::NoSuchRow { row, rows } => {
                write!(f, "There is no row {row}; the plan has {rows} row(s)")
            }
            MacroError::NoSuchResource { name } => write!(f, "There is no resource called {name}"),
            MacroError::NothingSelected => write!(f, "Nothing is selected"),
            MacroError::NeedsTwoRows => write!(f, "This needs two or more rows selected"),
            MacroError::ClipboardEmpty => write!(f, "There is nothing to paste"),
            MacroError::NothingToUndo => write!(f, "There is nothing left to undo"),
            MacroError::NothingToRedo => write!(f, "There is nothing to redo"),
            MacroError::SelfLink { row } => write!(f, "Row {row} cannot wait on itself"),
            MacroError::LinkRefused {
                predecessor,
                successor,
            } => write!(
                f,
                "Linking row {predecessor} to row {successor} would make a loop"
            ),
            MacroError::Refused { what, why } => write!(f, "{what}: {why}"),
            MacroError::FieldNotWritable { field } => {
                write!(f, "{} is worked out rather than typed in", field.label())
            }
            MacroError::FieldNotFillable { field } => {
                write!(f, "{} cannot be filled down", field.label())
            }
            MacroError::ValueNotUnderstood { field, value } => {
                write!(f, "{value} is not a {}", field.label())
            }
            MacroError::PercentOutOfRange { percent } => {
                write!(f, "{percent}% is more than complete")
            }
            MacroError::CannotGroupBy { field } => {
                write!(f, "Rows cannot be grouped by {}", field.label())
            }
            MacroError::CannotSortBy { field } => {
                write!(f, "Rows cannot be sorted by {}", field.label())
            }
            MacroError::ColumnAlreadyShown { field } => {
                write!(f, "The {} column is already shown", field.label())
            }
            MacroError::ColumnNotShown { field } => {
                write!(f, "The {} column is not shown", field.label())
            }
            MacroError::LastColumn => write!(f, "The table has to show at least one column"),
        }
    }
}

impl std::error::Error for MacroError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::cmd::Row;

    fn plan_of(names: &[&str]) -> AppState {
        let mut state = AppState::new();
        for name in names {
            state.append_task(name);
        }
        state.reschedule();
        state
    }

    #[test]
    fn a_name_has_to_start_with_a_letter_and_then_behave() {
        for good in ["Indent_And_Half_Done", "M", "a1_2", "Rollup2"] {
            assert!(check_name(good).is_ok(), "{good} should be allowed");
        }
        for bad in ["", "1st", "_leading", "has space", "has-dash", "quote\"d"] {
            assert!(check_name(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn a_name_too_long_for_a_menu_is_refused() {
        let long = "A".repeat(MAX_NAME_LENGTH + 1);
        assert!(check_name(&long).is_err());
        assert!(check_name(&"A".repeat(MAX_NAME_LENGTH)).is_ok());
    }

    #[test]
    fn a_recording_writes_a_body_that_reads_back_as_what_was_recorded() {
        let recorded = vec![
            Cmd::SelectRows {
                from: Row(3),
                to: Row(7),
            },
            Cmd::Indent {},
            Cmd::SetPercentComplete { percent: 50 },
        ];
        let macro_def = MacroDef::from_recording(
            "Indent_And_Half_Done",
            "Indents tasks 3 to 7 and marks them half complete.",
            &recorded,
        )
        .expect("a good name");

        assert!(macro_def.recorded);
        assert_eq!(macro_def.commands().expect("it wrote it"), recorded);
        let header = script::read_header(&macro_def.body);
        assert_eq!(header.name.as_deref(), Some("Indent_And_Half_Done"));
        assert_eq!(header.format, Some(FORMAT_VERSION));
    }

    #[test]
    fn a_macro_is_refused_a_name_the_header_could_not_hold() {
        assert!(MacroDef::new("Two Words", "").is_err());
        assert!(MacroDef::from_recording("Two Words", "", &[]).is_err());
    }

    #[test]
    fn a_macro_survives_the_round_trip_through_serde() {
        let mut macro_def = MacroDef::from_recording("Roll_Up", "Rolls up.", &[Cmd::Indent {}])
            .expect("a good name");
        macro_def.shortcut = Some("Ctrl+Shift+M".to_string());
        let json = serde_json::to_string(&macro_def).expect("plain data");
        let back: MacroDef = serde_json::from_str(&json).expect("plain data");
        assert_eq!(back, macro_def);
    }

    #[test]
    fn running_a_macro_does_what_the_script_says() {
        let mut state = plan_of(&["Phase", "Design", "Build", "Ship"]);
        let macro_def = MacroDef::from_recording(
            "Indent_The_Middle",
            "Nests the middle two rows under the first.",
            &[
                Cmd::SelectRows {
                    from: Row(2),
                    to: Row(3),
                },
                Cmd::Indent {},
                Cmd::SetPercentComplete { percent: 50 },
            ],
        )
        .expect("a good name");

        macro_def.run(&mut state).expect("every line should run");

        assert_eq!(state.project.tasks[1].outline_level, 1);
        assert_eq!(state.project.tasks[2].outline_level, 1);
        assert_eq!(state.project.tasks[1].percent_complete, 50);
        // Row 4 was never selected, so it is untouched.
        assert_eq!(state.project.tasks[3].outline_level, 0);
        assert_eq!(state.project.tasks[3].percent_complete, 0);
    }

    #[test]
    fn a_replayed_script_is_one_entry_naming_the_macro() {
        // Every command in a macro was recorded when somebody first did it by
        // hand. Writing them down again on replay would say the plan was
        // edited twice as often as it was.
        let mut state = plan_of(&["Phase", "Design", "Build"]);
        state.user_name = "Ada".to_string();
        let before = state.project.history.len();
        let body = script::to_script(
            "Indent_The_Middle",
            "Nests the middle rows.",
            &[
                Cmd::SelectRows {
                    from: Row(2),
                    to: Row(3),
                },
                Cmd::Indent {},
            ],
        );

        let done = run(&mut state, &body).expect("both lines run");

        assert_eq!(done, 2);
        let log = state.project.history.changes();
        assert_eq!(
            log.len(),
            before + 1,
            "a replay is one entry, not one for every command it carried out"
        );
        let entry = log.last().expect("the run recorded one");
        assert_eq!(entry.summary, "Ran the macro Indent_The_Middle");
        assert_eq!(entry.author, "Ada");
        assert_eq!(
            entry.command_count(),
            2,
            "and it keeps what it ran, so the entry replays"
        );
    }

    #[test]
    fn a_run_that_stops_part_way_records_only_what_ran() {
        // What is in the plan is the first command, so that is what the entry
        // has to hold: a log that replayed to a plan nobody ever had is worse
        // than one that is short.
        let mut state = plan_of(&["Phase", "Design"]);
        let before = state.project.history.len();

        let error = run(&mut state, "select_row(1);\nselect_row(9);\n")
            .expect_err("there is no row 9");
        assert!(matches!(error, MacroError::Stopped { after: 1, .. }), "{error}");

        let entry = state
            .project
            .history
            .changes()
            .last()
            .expect("one command ran, so there is an entry");
        assert_eq!(state.project.history.len(), before + 1);
        assert_eq!(entry.command_count(), 1);
        assert_eq!(entry.summary, "Ran 1 of the 2 commands in a macro");
    }

    #[test]
    fn a_script_with_a_typo_anywhere_in_it_changes_nothing() {
        // Everything is read before anything runs, so the plan is not left
        // halfway through a macro that was never going to finish.
        let mut state = plan_of(&["Phase", "Design", "Build"]);
        let before = state.project.tasks[1].outline_level;
        let macro_def = MacroDef {
            name: "Half_Written".to_string(),
            description: String::new(),
            shortcut: None,
            body: "select_rows(2, 3);\nindent();\nsummon_the_moon();\n".to_string(),
            recorded: false,
        };

        let error = macro_def.run(&mut state).expect_err("the third line is wrong");
        assert_eq!(error.line(), Some(3));
        assert_eq!(state.project.tasks[1].outline_level, before);
    }

    #[test]
    fn a_command_that_will_not_run_names_its_line_and_itself() {
        let mut state = plan_of(&["Phase"]);
        let macro_def = MacroDef {
            name: "Off_The_End".to_string(),
            description: String::new(),
            shortcut: None,
            body: "select_row(1);\nselect_row(9);\n".to_string(),
            recorded: false,
        };

        let error = macro_def.run(&mut state).expect_err("there is no row 9");
        match error {
            MacroError::Failed { line, command, .. } => {
                assert_eq!(line, 2);
                assert_eq!(command, "select_row");
            }
            other => panic!("expected a run failure, got {other}"),
        }
        assert!(error.to_string().contains("row 9"), "{error}");
    }

    #[test]
    fn everything_a_macro_did_comes_back_with_one_undo_per_command() {
        let mut state = plan_of(&["Phase", "Design", "Build"]);
        let levels: Vec<u16> = state
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();

        Cmd::SelectRows {
            from: Row(2),
            to: Row(3),
        }
        .apply(&mut state)
        .expect("rows 2 and 3 exist");
        Cmd::Indent {}.apply(&mut state).expect("both can indent");

        Cmd::Undo {}.apply(&mut state).expect("indent checkpointed");
        let after: Vec<u16> = state
            .project
            .tasks
            .iter()
            .map(|task| task.outline_level)
            .collect();
        assert_eq!(after, levels, "one undo should take the whole command back");
    }

    #[test]
    fn a_row_a_planner_can_see_is_the_row_the_macro_changes() {
        // Row 3 on screen is `tasks[2]`. Getting this wrong once would put the
        // whole vocabulary one out.
        let mut state = plan_of(&["One", "Two", "Three", "Four"]);
        Cmd::SelectRow { row: Row(3) }
            .apply(&mut state)
            .expect("row 3 exists");
        assert_eq!(state.selection, vec![2]);
        assert_eq!(state.project.tasks[2].name, "Three");
        assert_eq!(Field::Id.value(&state.project, 2, "%Y-%m-%d"), "3");
    }

    #[test]
    fn row_zero_is_a_mistake_rather_than_the_first_row() {
        let mut state = plan_of(&["One", "Two"]);
        let error = Cmd::SelectRow { row: Row(0) }
            .apply(&mut state)
            .expect_err("there is no row 0");
        assert_eq!(error, MacroError::RowsCountFromOne);
    }

    #[test]
    fn a_command_that_needs_a_selection_says_so_rather_than_doing_nothing() {
        let mut state = plan_of(&["One", "Two"]);
        state.selection.clear();
        assert_eq!(
            Cmd::Indent {}.apply(&mut state),
            Err(MacroError::NothingSelected)
        );
    }

    #[test]
    fn linking_a_row_to_itself_is_refused_before_the_scheduler_sees_it() {
        let mut state = plan_of(&["One", "Two"]);
        let error = Cmd::SetLink {
            row: Row(1),
            predecessor: Row(1),
            kind: aop_core::LinkType::FS,
            lag_minutes: 0,
        }
        .apply(&mut state)
        .expect_err("a task cannot wait on itself");
        assert_eq!(error, MacroError::SelfLink { row: 1 });
    }

    #[test]
    fn a_link_that_would_make_a_loop_comes_back_as_an_error_not_a_dialog() {
        let mut state = plan_of(&["One", "Two"]);
        Cmd::SetLink {
            row: Row(2),
            predecessor: Row(1),
            kind: aop_core::LinkType::FS,
            lag_minutes: 0,
        }
        .apply(&mut state)
        .expect("one then two is fine");

        let error = Cmd::SetLink {
            row: Row(1),
            predecessor: Row(2),
            kind: aop_core::LinkType::FS,
            lag_minutes: 0,
        }
        .apply(&mut state)
        .expect_err("that closes the loop");

        // A script has nobody to click OK, so the message box must not be left
        // standing where the next command would trip over it.
        assert!(state.dialog.is_none(), "a macro left a dialog on screen");
        assert!(matches!(
            error,
            MacroError::Refused { .. } | MacroError::LinkRefused { .. }
        ));
    }

    #[test]
    fn a_field_the_grid_cannot_type_into_is_refused_by_name() {
        let mut state = plan_of(&["One"]);
        let error = Cmd::SetField {
            row: Row(1),
            field: Field::TotalSlack,
            value: "3 days".to_string(),
        }
        .apply(&mut state)
        .expect_err("slack is worked out");
        assert_eq!(
            error,
            MacroError::FieldNotWritable {
                field: Field::TotalSlack
            }
        );
    }

    #[test]
    fn a_value_the_cell_would_throw_away_is_refused_rather_than_ignored() {
        let mut state = plan_of(&["One"]);
        let error = Cmd::SetField {
            row: Row(1),
            field: Field::Duration,
            value: "a fortnight".to_string(),
        }
        .apply(&mut state)
        .expect_err("that is not a duration");
        assert!(matches!(error, MacroError::ValueNotUnderstood { .. }), "{error}");
    }

    #[test]
    fn a_field_that_can_be_typed_into_is_typed_into() {
        let mut state = plan_of(&["One"]);
        Cmd::SetField {
            row: Row(1),
            field: Field::Name,
            value: "Renamed".to_string(),
        }
        .apply(&mut state)
        .expect("names are typed in");
        assert_eq!(state.project.tasks[0].name, "Renamed");
    }

    #[test]
    fn booking_somebody_who_is_not_on_the_sheet_is_an_error() {
        let mut state = plan_of(&["One"]);
        let error = Cmd::AssignResource {
            row: Row(1),
            name: "Nobody".to_string(),
            units_percent: 100.0,
        }
        .apply(&mut state)
        .expect_err("nobody by that name");
        assert!(matches!(error, MacroError::NoSuchResource { .. }), "{error}");
    }

    #[test]
    fn units_are_written_as_a_percentage_and_stored_as_a_fraction() {
        let mut state = plan_of(&["One"]);
        Cmd::AddResource {
            name: "Ada".to_string(),
        }
        .apply(&mut state)
        .expect("a new resource");
        Cmd::AssignResource {
            row: Row(1),
            name: "Ada".to_string(),
            units_percent: 50.0,
        }
        .apply(&mut state)
        .expect("Ada is on the sheet");

        let units = state.project.tasks[0].assignments[0].units;
        assert!((units - 0.5).abs() < f64::EPSILON, "{units} should be 0.5");
    }

    #[test]
    fn a_field_nothing_can_be_grouped_by_is_refused() {
        let mut state = plan_of(&["One"]);
        let error = Cmd::GroupBy {
            field: Some(Field::Notes),
        }
        .apply(&mut state)
        .expect_err("no banding by notes");
        assert_eq!(error, MacroError::CannotGroupBy { field: Field::Notes });
    }

    #[test]
    fn group_by_none_takes_the_banding_off_again() {
        let mut state = plan_of(&["One"]);
        Cmd::GroupBy {
            field: Some(Field::Duration),
        }
        .apply(&mut state)
        .expect("duration bands");
        assert!(state.group_by.is_some());
        Cmd::GroupBy { field: None }
            .apply(&mut state)
            .expect("and off again");
        assert!(state.group_by.is_none());
    }

    #[test]
    fn a_column_is_inserted_at_the_position_the_planner_counted_to() {
        let mut state = plan_of(&["One"]);
        Cmd::ShowColumn {
            field: Field::Cost,
            at: Row(1),
        }
        .apply(&mut state)
        .expect("first position");
        assert_eq!(state.columns[0].field, Field::Cost);

        let error = Cmd::ShowColumn {
            field: Field::Cost,
            at: Row(1),
        }
        .apply(&mut state)
        .expect_err("it is already there");
        assert_eq!(error, MacroError::ColumnAlreadyShown { field: Field::Cost });

        Cmd::HideColumn { field: Field::Cost }
            .apply(&mut state)
            .expect("and off again");
        assert!(!state.columns.iter().any(|c| c.field == Field::Cost));
    }

    #[test]
    fn a_view_setting_is_a_view_setting_and_leaves_the_plan_alone() {
        // A fresh plan has nothing behind it, which is what makes the last
        // assertion mean anything.
        let mut state = AppState::new();
        Cmd::SetViewOption {
            option: ViewOption::CriticalPath,
            on: true,
        }
        .apply(&mut state)
        .expect("a flag");
        assert!(state.show_critical);
        assert!(!state.can_undo(), "a view setting is not an undo step");
        assert!(!state.dirty, "a view setting does not make the plan unsaved");
    }
}
