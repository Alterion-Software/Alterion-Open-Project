//! The command vocabulary: one enum holding everything a macro can do.
//!
//! `Cmd` is deliberately four things at once. It is what the recorder writes
//! down, what the player carries out, what the script text parses into, and the
//! unit an undo step corresponds to. Splitting those apart would mean four
//! lists of commands that have to be kept in step by hand, and they would not
//! stay in step.
//!
//! The variants, the dispatcher, the script names and the text renderer are all
//! generated from the single `commands!` table below, for the same reason.
//! Adding a command means adding one row.

use aop_core::{Field, LinkType, TaskMode};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::state::{AppState, Column, Dialog, TaskFilter, ViewKind, Zoom};

use super::MacroError;

/// A position as the planner sees it, counting from 1.
///
/// `Field::Id` renders `index + 1`, so a script that says `select_rows(3, 7)`
/// has to mean the two rows labelled 3 and 7 on screen. Every `Cmd` therefore
/// carries `Row`, and the methods below are the one place it turns into a
/// `usize` offset. Doing the subtraction anywhere else is how a macro system
/// ends up off by one in half its commands.
///
/// Used for anything the planner counts from 1: task rows, resource rows and
/// column positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row(pub u32);

impl Row {
    /// The offset this row points at, checked against how many there are.
    pub fn index(self, len: usize) -> Result<usize, MacroError> {
        let index = self.offset()?;
        if index >= len {
            return Err(MacroError::NoSuchRow {
                row: self.0,
                rows: len,
            });
        }
        Ok(index)
    }

    /// The offset to insert at, which may legitimately be one past the end.
    pub fn insert_index(self, len: usize) -> Result<usize, MacroError> {
        let index = self.offset()?;
        if index > len {
            return Err(MacroError::NoSuchRow {
                row: self.0,
                rows: len,
            });
        }
        Ok(index)
    }

    fn offset(self) -> Result<usize, MacroError> {
        match self.0.checked_sub(1) {
            Some(index) => Ok(index as usize),
            None => Err(MacroError::RowsCountFromOne),
        }
    }
}

/// A view switch that is a plain on or off rather than a choice.
///
/// Grouped into one command because these are all the same shape: a flag on
/// `AppState` that changes what is drawn and nothing about the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewOption {
    CriticalPath,
    Timeline,
    OutlineNumber,
    Slack,
    Baseline,
    Links,
    BarText,
    RoundBars,
}

/// A column of the resource sheet.
///
/// `commit_resource_cell` takes a string key. Naming the columns here instead
/// means a typo in a script is caught by the parser rather than silently
/// changing nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceField {
    Name,
    Initials,
    Group,
    MaxUnits,
    Rate,
    Kind,
}

impl ResourceField {
    /// The key `AppState::commit_resource_cell` matches on.
    fn key(self) -> &'static str {
        match self {
            ResourceField::Name => "name",
            ResourceField::Initials => "initials",
            ResourceField::Group => "group",
            ResourceField::MaxUnits => "max",
            ResourceField::Rate => "rate",
            ResourceField::Kind => "kind",
        }
    }
}

// ---- script arguments ---------------------------------------------------

/// A value that can be written into a script line and read back out of one.
///
/// `render` and `parse` are each other's inverse for every type here, which is
/// what lets the round trip test hold for every command without listing them.
pub trait ScriptArg: Sized {
    fn render(&self) -> String;
    /// The message is plain, without a line number: the caller knows the line.
    fn parse(text: &str) -> Result<Self, String>;
}

/// Give an enum a script spelling taken from its own variant names.
///
/// The `render` match is exhaustive on purpose. Adding a variant to one of
/// these enums stops the build here, rather than shipping something the ribbon
/// offers and no script can name.
macro_rules! enum_arg {
    ($ty:ident { $($variant:ident),* $(,)? }) => {
        impl ScriptArg for $ty {
            fn render(&self) -> String {
                match self {
                    $($ty::$variant => stringify!($variant).to_string(),)*
                }
            }

            fn parse(text: &str) -> Result<Self, String> {
                $(if text.eq_ignore_ascii_case(stringify!($variant)) {
                    return Ok($ty::$variant);
                })*
                Err(format!(
                    "expected one of {}, found {text}",
                    [$(stringify!($variant)),*].join(", ")
                ))
            }
        }
    };
}

enum_arg!(TaskMode { Manual, Auto });

enum_arg!(TaskFilter {
    All,
    Critical,
    Milestones,
    Incomplete,
});

enum_arg!(Zoom {
    Days,
    Weeks,
    Months,
    Quarters,
});

enum_arg!(ViewKind {
    GanttChart,
    TrackingGantt,
    TaskSheet,
    TaskUsage,
    NetworkDiagram,
    CalendarView,
    ResourceSheet,
    ResourceUsage,
    TeamPlanner,
    Burndown,
    Burnup,
    Velocity,
    CriticalPath,
});

enum_arg!(ViewOption {
    CriticalPath,
    Timeline,
    OutlineNumber,
    Slack,
    Baseline,
    Links,
    BarText,
    RoundBars,
});

enum_arg!(ResourceField {
    Name,
    Initials,
    Group,
    MaxUnits,
    Rate,
    Kind,
});

impl ScriptArg for LinkType {
    /// Written the way a Predecessors cell writes it, so `3FS+2d` in the grid
    /// and `FS` in a script mean the same thing to the same reader.
    fn render(&self) -> String {
        self.code().to_string()
    }

    fn parse(text: &str) -> Result<Self, String> {
        LinkType::parse(text).ok_or_else(|| format!("expected FS, SS, FF or SF, found {text}"))
    }
}

impl ScriptArg for Field {
    /// A field is named by its variant rather than its label. Labels have
    /// spaces and punctuation in them and get reworded; the variant name is the
    /// part that is safe to write into a file somebody keeps.
    fn render(&self) -> String {
        format!("{self:?}")
    }

    fn parse(text: &str) -> Result<Self, String> {
        Field::ALL
            .into_iter()
            .find(|field| format!("{field:?}").eq_ignore_ascii_case(text))
            .ok_or_else(|| format!("there is no field called {text}"))
    }
}

impl ScriptArg for Option<Field> {
    fn render(&self) -> String {
        match self {
            Some(field) => field.render(),
            None => "none".to_string(),
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        if text.eq_ignore_ascii_case("none") {
            return Ok(None);
        }
        Field::parse(text).map(Some)
    }
}

impl ScriptArg for Row {
    fn render(&self) -> String {
        self.0.to_string()
    }

    fn parse(text: &str) -> Result<Self, String> {
        text.parse::<u32>()
            .map(Row)
            .map_err(|_| format!("expected a row number, found {text}"))
    }
}

impl ScriptArg for u8 {
    fn render(&self) -> String {
        self.to_string()
    }

    fn parse(text: &str) -> Result<Self, String> {
        text.parse::<u8>()
            .map_err(|_| format!("expected a whole number from 0 to 255, found {text}"))
    }
}

impl ScriptArg for i64 {
    fn render(&self) -> String {
        self.to_string()
    }

    fn parse(text: &str) -> Result<Self, String> {
        text.parse::<i64>()
            .map_err(|_| format!("expected a whole number, found {text}"))
    }
}

impl ScriptArg for f64 {
    /// A round figure is written without a trailing `.0`, because `100` is what
    /// a planner typing units expects to see written back.
    fn render(&self) -> String {
        if self.fract() == 0.0 && self.is_finite() {
            format!("{self:.0}")
        } else {
            self.to_string()
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(value),
            _ => Err(format!("expected a number, found {text}")),
        }
    }
}

impl ScriptArg for bool {
    fn render(&self) -> String {
        if *self { "true" } else { "false" }.to_string()
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text.to_ascii_lowercase().as_str() {
            "true" | "on" | "yes" => Ok(true),
            "false" | "off" | "no" => Ok(false),
            _ => Err(format!("expected true or false, found {text}")),
        }
    }
}

impl ScriptArg for String {
    fn render(&self) -> String {
        let mut out = String::with_capacity(self.len() + 2);
        out.push('"');
        for character in self.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                other => out.push(other),
            }
        }
        out.push('"');
        out
    }

    fn parse(text: &str) -> Result<Self, String> {
        let inner = text
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(|| format!("expected text in double quotes, found {text}"))?;
        let mut out = String::with_capacity(inner.len());
        let mut characters = inner.chars();
        while let Some(character) = characters.next() {
            if character != '\\' {
                out.push(character);
                continue;
            }
            match characters.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => return Err(format!("\\{other} means nothing in a script")),
                None => return Err("a backslash at the end of the text".to_string()),
            }
        }
        Ok(out)
    }
}

/// A moment, written the way `state::parse_date` reads it.
///
/// Always with the time, even at midnight: a bare date is read back as eight in
/// the morning, so writing one would not round trip.
const DATE_IN_SCRIPT: &str = "%Y-%m-%d %H:%M";

impl ScriptArg for NaiveDateTime {
    fn render(&self) -> String {
        self.format(DATE_IN_SCRIPT).to_string().render()
    }

    fn parse(text: &str) -> Result<Self, String> {
        let quoted = String::parse(text)?;
        crate::state::parse_date(&quoted)
            .ok_or_else(|| format!("{quoted} is not a date this build understands"))
    }
}

// ---- the table ----------------------------------------------------------

/// Build `Cmd` and everything that has to agree with it from one list.
///
/// Each row gives the variant and its arguments, the name it is called by in a
/// script, and what carrying it out means. From that come the enum, the `apply`
/// dispatcher, `fn_name`, `to_script` and the parser's side of the round trip,
/// so none of the five can be updated without the others.
macro_rules! commands {
    ($(
        $(#[$about:meta])*
        $variant:ident { $($field:ident : $ty:ty),* $(,)? } = $name:literal => |$state:ident| $body:block
    );* $(;)?) => {
        /// Everything a macro can do.
        ///
        /// Note what is not here: `open`, `save`, `save_as`, `import`, `export`
        /// and `print`. A macro changes the plan that is in memory and nothing
        /// else. The planner opens and saves deliberately from the interface,
        /// which is what makes running somebody else's macro a reversible thing
        /// rather than a leap of faith: whatever it did, Undo takes it back and
        /// closing without saving loses it.
        ///
        /// It will be tempting to break this for "just export to CSV". Do not.
        /// The moment one command can write a file, every macro has to be
        /// trusted before it is run, and the whole safety story goes with it.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Cmd {
            $(
                $(#[$about])*
                $variant { $($field: $ty),* }
            ),*
        }

        impl Cmd {
            /// Every name a script may call, in table order.
            pub const NAMES: &'static [&'static str] = &[$($name),*];

            /// Carry the command out. The single place a macro can touch the plan.
            ///
            /// Takes a plain `&mut AppState` rather than a signal, so the whole
            /// vocabulary can be exercised in tests with no Dioxus runtime.
            pub fn apply(&self, state: &mut AppState) -> Result<(), MacroError> {
                match self {
                    $(
                        Cmd::$variant { $($field),* } => {
                            let $state: &mut AppState = state;
                            $body
                        }
                    )*
                }
            }

            /// Render as one line of script text, semicolon and all.
            pub fn to_script(&self) -> String {
                match self {
                    $(
                        Cmd::$variant { $($field),* } => {
                            let args: Vec<String> = vec![$(ScriptArg::render($field)),*];
                            format!("{}({});", $name, args.join(", "))
                        }
                    )*
                }
            }

            /// The name this command is called by in a script.
            pub fn fn_name(&self) -> &'static str {
                match self {
                    $(Cmd::$variant { .. } => $name),*
                }
            }

            /// Rebuild a command from a parsed call. The parser owns line
            /// numbers, so the message here is about the call alone.
            pub(super) fn from_call(name: &str, args: &[String]) -> Result<Cmd, String> {
                match name {
                    $(
                        $name => {
                            const ARGS: &[&str] = &[$(stringify!($field)),*];
                            if args.len() != ARGS.len() {
                                return Err(arity_message($name, ARGS, args.len()));
                            }
                            #[allow(unused_mut)]
                            let mut _taken = args.iter();
                            Ok(Cmd::$variant {
                                $($field: {
                                    let raw = _taken.next().map(String::as_str).unwrap_or("");
                                    <$ty as ScriptArg>::parse(raw).map_err(|why| {
                                        format!("{}: {why}", stringify!($field))
                                    })?
                                }),*
                            })
                        }
                    )*
                    other => Err(format!("there is no command called {other}")),
                }
            }
        }
    };
}

/// Say what a call should have looked like rather than just that it was wrong.
fn arity_message(name: &str, expected: &[&str], given: usize) -> String {
    if expected.is_empty() {
        return format!("{name}() takes no arguments, but {given} were given");
    }
    format!(
        "{name}({}) takes {} argument(s), but {given} were given",
        expected.join(", "),
        expected.len()
    )
}

commands! {
    // ---- selection ------------------------------------------------------

    /// Select one row on its own, dropping whatever else was selected.
    SelectRow { row: Row } = "select_row" => |state| {
        let index = row.index(state.project.tasks.len())?;
        state.select(index);
        Ok(())
    };

    /// Select a run of rows, ends included.
    SelectRows { from: Row, to: Row } = "select_rows" => |state| {
        let len = state.project.tasks.len();
        let first = from.index(len)?;
        let last = to.index(len)?;
        state.select(first);
        state.extend_selection(last);
        Ok(())
    };

    /// Add or remove one row without disturbing the rest, which is the only
    /// way a script can describe a selection with a gap in it.
    ToggleRow { row: Row } = "toggle_row" => |state| {
        let index = row.index(state.project.tasks.len())?;
        state.toggle_selection(index);
        Ok(())
    };

    /// Select every row in the plan.
    SelectAll {} = "select_all" => |state| {
        state.selection = (0..state.project.tasks.len()).collect();
        Ok(())
    };

    /// Select nothing.
    ClearSelection {} = "clear_selection" => |state| {
        state.selection.clear();
        Ok(())
    };

    // ---- outline and structure ------------------------------------------

    /// Insert a blank task above the selection.
    InsertTask {} = "insert_task" => |state| {
        state.insert_task();
        Ok(())
    };

    /// Insert a zero length marker above the selection.
    InsertMilestone {} = "insert_milestone" => |state| {
        state.insert_milestone();
        Ok(())
    };

    /// Insert a summary row with the selection nested under it.
    InsertSummary {} = "insert_summary" => |state| {
        state.insert_summary();
        Ok(())
    };

    /// Add a named task at the bottom and leave it selected.
    ///
    /// The only command that creates a task with a name already on it, so a
    /// script can build a plan rather than only rearrange one.
    AppendTask { name: String } = "append_task" => |state| {
        let at = state.append_task(name);
        state.select(at);
        Ok(())
    };

    /// Delete every selected row and its children.
    DeleteTasks {} = "delete_tasks" => |state| {
        require_selection(state)?;
        state.delete_selected();
        Ok(())
    };

    /// Push the selection one level deeper in the outline.
    Indent {} = "indent" => |state| {
        require_selection(state)?;
        state.indent_selected();
        Ok(())
    };

    /// Pull the selection one level back out.
    Outdent {} = "outdent" => |state| {
        require_selection(state)?;
        state.outdent_selected();
        Ok(())
    };

    /// Move the selected block up past its previous sibling.
    MoveUp {} = "move_up" => |state| {
        require_selection(state)?;
        state.move_selected(-1);
        Ok(())
    };

    /// Move the selected block down past its next sibling.
    MoveDown {} = "move_down" => |state| {
        require_selection(state)?;
        state.move_selected(1);
        Ok(())
    };

    /// Copy the selection to the clipboard the plan keeps for itself.
    CopyTasks {} = "copy_tasks" => |state| {
        require_selection(state)?;
        state.copy_selected();
        Ok(())
    };

    /// Copy the selection and delete it.
    CutTasks {} = "cut_tasks" => |state| {
        require_selection(state)?;
        state.cut_selected();
        Ok(())
    };

    /// Paste the clipboard in above the selection.
    PasteTasks {} = "paste_tasks" => |state| {
        // The clipboard is private to the state, so whether anything arrived is
        // the only thing a script can ask. `paste` returns quietly when there
        // is nothing to paste, which a macro should not do silently.
        let before = state.project.tasks.len();
        state.paste();
        if state.project.tasks.len() == before {
            return Err(MacroError::ClipboardEmpty);
        }
        Ok(())
    };

    /// Open every summary row.
    ExpandAll {} = "expand_all" => |state| {
        state.expand_all(false);
        Ok(())
    };

    /// Close every summary row.
    CollapseAll {} = "collapse_all" => |state| {
        state.expand_all(true);
        Ok(())
    };

    // ---- links ----------------------------------------------------------

    /// Chain the selection finish to start, in the order it was selected.
    Link {} = "link" => |state| {
        if state.selection.len() < 2 {
            return Err(MacroError::NeedsTwoRows);
        }
        state.link_selected();
        reject_if_refused(state)?;
        Ok(())
    };

    /// Take every link off the selected tasks, in both directions.
    Unlink {} = "unlink" => |state| {
        require_selection(state)?;
        state.unlink_selected();
        Ok(())
    };

    /// Link one named row to another, replacing any link already between them.
    ///
    /// Lag is in working minutes, negative for a lead, matching `Link`.
    SetLink {
        row: Row,
        predecessor: Row,
        kind: LinkType,
        lag_minutes: i64,
    } = "set_link" => |state| {
        let len = state.project.tasks.len();
        let successor_index = row.index(len)?;
        let predecessor_index = predecessor.index(len)?;
        if successor_index == predecessor_index {
            return Err(MacroError::SelfLink { row: row.0 });
        }
        let (Some(predecessor_id), Some(successor_id)) = (
            state.project.tasks.get(predecessor_index).map(|task| task.id),
            state.project.tasks.get(successor_index).map(|task| task.id),
        ) else {
            return Err(MacroError::NoSuchRow { row: row.0, rows: len });
        };
        state.set_link(successor_index, predecessor_id, *kind, *lag_minutes);
        // `set_link` rolls a cycle back by itself and reports it in a dialog.
        // Asking whether the link is actually there is how a script finds out.
        reject_if_refused(state)?;
        if !state.project.link_exists(predecessor_id, successor_id) {
            return Err(MacroError::LinkRefused {
                predecessor: predecessor.0,
                successor: row.0,
            });
        }
        Ok(())
    };

    /// Take one named link off, leaving the rest alone.
    RemoveLink { row: Row, predecessor: Row } = "remove_link" => |state| {
        let len = state.project.tasks.len();
        let successor_index = row.index(len)?;
        let predecessor_index = predecessor.index(len)?;
        let Some(predecessor_id) = state.project.tasks.get(predecessor_index).map(|t| t.id) else {
            return Err(MacroError::NoSuchRow { row: predecessor.0, rows: len });
        };
        state.remove_link(successor_index, predecessor_id);
        Ok(())
    };

    // ---- fields ---------------------------------------------------------

    /// Type a value into one cell, exactly as if it had been typed in the grid.
    ///
    /// Only the fields the grid can edit: a script that could write Finish
    /// directly would be writing an answer the scheduler is supposed to work
    /// out.
    SetField { row: Row, field: Field, value: String } = "set_field" => |state| {
        let index = row.index(state.project.tasks.len())?;
        let column = column_for(*field)?;
        check_value(*field, value)?;
        state.commit_cell(index, column, value);
        reject_if_refused(state)?;
        Ok(())
    };

    /// Mark the selection a percentage done. Summary rows roll up and are left.
    SetPercentComplete { percent: u8 } = "set_percent_complete" => |state| {
        require_selection(state)?;
        if *percent > 100 {
            return Err(MacroError::PercentOutOfRange { percent: *percent });
        }
        state.set_percent_complete(*percent);
        Ok(())
    };

    /// Put the selection under the scheduler's control, or take it out.
    SetTaskMode { mode: TaskMode } = "set_task_mode" => |state| {
        require_selection(state)?;
        state.set_task_mode(*mode);
        Ok(())
    };

    /// Flip the selection between active and ignored by the scheduler.
    ToggleActive {} = "toggle_active" => |state| {
        require_selection(state)?;
        state.toggle_active();
        Ok(())
    };

    /// Drop constraints and pinned dates so the selection follows its links.
    RespectLinks {} = "respect_links" => |state| {
        require_selection(state)?;
        state.respect_links();
        Ok(())
    };

    /// Copy the top selected row's value down the rest of the selection.
    ///
    /// Which column is filled comes from the cell that was last clicked, so a
    /// script has to say so first.
    FillDown { field: Field } = "fill_down" => |state| {
        if state.selection.len() < 2 {
            return Err(MacroError::NeedsTwoRows);
        }
        if !aop_core::grouping::is_fillable(*field) {
            return Err(MacroError::FieldNotFillable { field: *field });
        }
        state.fill_field = Some(*field);
        state.fill_down();
        Ok(())
    };

    // ---- resources ------------------------------------------------------

    /// Add somebody to the resource sheet.
    AddResource { name: String } = "add_resource" => |state| {
        state.add_resource(name);
        Ok(())
    };

    /// Take somebody off the resource sheet and off every task they were on.
    DeleteResource { resource_row: Row } = "delete_resource" => |state| {
        let index = resource_row.index(state.project.resources.len())?;
        state.delete_resource(index);
        Ok(())
    };

    /// Book a named person onto a task. Units are a percentage, so 100 is
    /// full time and 50 is half.
    AssignResource { row: Row, name: String, units_percent: f64 } = "assign_resource" => |state| {
        let index = row.index(state.project.tasks.len())?;
        require_resource(state, name)?;
        state.assign_resource_by_name(index, name, *units_percent / 100.0);
        Ok(())
    };

    /// Change how much of somebody a task books, leaving the booking in place.
    SetAssignmentUnits {
        row: Row,
        name: String,
        units_percent: f64,
    } = "set_assignment_units" => |state| {
        let index = row.index(state.project.tasks.len())?;
        require_resource(state, name)?;
        state.set_assignment_units(index, name, *units_percent / 100.0);
        Ok(())
    };

    /// Take a named person off a task.
    UnassignResource { row: Row, name: String } = "unassign_resource" => |state| {
        let index = row.index(state.project.tasks.len())?;
        require_resource(state, name)?;
        state.unassign_resource(index, name);
        Ok(())
    };

    /// Type a value into one cell of the resource sheet.
    SetResourceField {
        resource_row: Row,
        field: ResourceField,
        value: String,
    } = "set_resource_field" => |state| {
        let index = resource_row.index(state.project.resources.len())?;
        state.commit_resource_cell(index, field.key(), value);
        Ok(())
    };

    // ---- view, filter, sort and group ------------------------------------

    /// Switch which view is on screen.
    SetView { view: ViewKind } = "set_view" => |state| {
        state.view = *view;
        Ok(())
    };

    /// Set the chart timescale.
    SetZoom { zoom: Zoom } = "set_zoom" => |state| {
        state.zoom = *zoom;
        Ok(())
    };

    /// Pick the timescale that fits the whole plan on screen.
    ZoomToFit {} = "zoom_to_fit" => |state| {
        state.zoom_to_fit();
        Ok(())
    };

    /// Show only some of the rows.
    SetFilter { filter: TaskFilter } = "set_filter" => |state| {
        state.set_filter(filter_key(*filter));
        Ok(())
    };

    /// Band the rows by a field, or `none` to stop banding them.
    GroupBy { field: Option<Field> } = "group_by" => |state| {
        let key = match field {
            Some(field) => group_key(*field)?,
            None => "",
        };
        state.set_group_by(key);
        Ok(())
    };

    /// Sort sibling blocks by a field, keeping the outline intact.
    SortBy { field: Field } = "sort_by" => |state| {
        state.sort_tasks(sort_key(*field)?);
        Ok(())
    };

    /// Put a column into the table at a given position, counting from 1.
    ShowColumn { field: Field, at: Row } = "show_column" => |state| {
        if state.columns.iter().any(|column| column.field == *field) {
            return Err(MacroError::ColumnAlreadyShown { field: *field });
        }
        let index = at.insert_index(state.columns.len())?;
        state.insert_column(index, *field);
        Ok(())
    };

    /// Take a column back out of the table.
    HideColumn { field: Field } = "hide_column" => |state| {
        let Some(index) = state.columns.iter().position(|column| column.field == *field) else {
            return Err(MacroError::ColumnNotShown { field: *field });
        };
        if state.columns.len() <= 1 {
            return Err(MacroError::LastColumn);
        }
        state.remove_column(index);
        Ok(())
    };

    /// Put the table back to the Entry columns.
    ResetColumns {} = "reset_columns" => |state| {
        state.reset_columns();
        Ok(())
    };

    /// Turn one of the view's on-or-off settings on or off.
    SetViewOption { option: ViewOption, on: bool } = "set_view_option" => |state| {
        let on = *on;
        match option {
            ViewOption::CriticalPath => state.show_critical = on,
            ViewOption::Timeline => state.show_timeline = on,
            ViewOption::OutlineNumber => state.show_outline_number = on,
            ViewOption::Slack => state.show_slack = on,
            ViewOption::Baseline => state.show_baseline = on,
            ViewOption::Links => state.show_links = on,
            ViewOption::BarText => state.bar_text = on,
            ViewOption::RoundBars => state.round_bars = on,
        }
        Ok(())
    };

    // ---- the plan as a whole ---------------------------------------------

    /// Record today's dates as the baseline to measure against.
    SetBaseline {} = "set_baseline" => |state| {
        state.set_baseline();
        Ok(())
    };

    /// Throw the baseline away.
    ClearBaseline {} = "clear_baseline" => |state| {
        state.clear_baseline();
        Ok(())
    };

    /// Move the date the whole plan is scheduled from.
    SetProjectStart { date: NaiveDateTime } = "set_project_start" => |state| {
        state.set_project_start(*date);
        Ok(())
    };

    /// Push overbooked work later until nobody is asked for more hours than
    /// they have.
    Level {} = "level" => |state| {
        state.level(aop_core::leveling::LevelScope::EntireProject);
        Ok(())
    };

    /// Take back the delays levelling put in.
    ClearLeveling {} = "clear_leveling" => |state| {
        state.clear_leveling();
        Ok(())
    };

    /// Step back one change.
    Undo {} = "undo" => |state| {
        if !state.can_undo() {
            return Err(MacroError::NothingToUndo);
        }
        state.undo();
        Ok(())
    };

    /// Step forward again.
    Redo {} = "redo" => |state| {
        if !state.can_redo() {
            return Err(MacroError::NothingToRedo);
        }
        state.redo();
        Ok(())
    };

    /// Say something in the status bar. How a macro reports what it did.
    Note { message: String } = "note" => |state| {
        state.note(message.clone());
        Ok(())
    };
}

// ---- helpers the table leans on -----------------------------------------

/// Fail loudly when a command needs rows and none are selected.
///
/// The `AppState` methods return quietly in that case, which is right for a
/// ribbon button and wrong for a script: a macro that silently did nothing is
/// worse than one that says which line gave up.
fn require_selection(state: &AppState) -> Result<(), MacroError> {
    if state.selection.is_empty() {
        return Err(MacroError::NothingSelected);
    }
    Ok(())
}

fn require_resource(state: &AppState, name: &str) -> Result<(), MacroError> {
    let wanted = name.trim().to_lowercase();
    let known = state
        .project
        .resources
        .iter()
        .any(|resource| resource.name.trim().to_lowercase() == wanted);
    if known {
        Ok(())
    } else {
        Err(MacroError::NoSuchResource {
            name: name.to_string(),
        })
    }
}

/// Turn a refusal the state reported through a dialog into an error.
///
/// A few commands undo themselves and put the reason in a message box. A script
/// has nobody to click OK, so the message is taken off the screen and handed
/// back as the failure it is.
fn reject_if_refused(state: &mut AppState) -> Result<(), MacroError> {
    if let Some(Dialog::Message { title, body }) = state.dialog.clone() {
        state.dialog = None;
        return Err(MacroError::Refused {
            what: title,
            why: body,
        });
    }
    Ok(())
}

/// The grid column a field is typed into.
fn column_for(field: Field) -> Result<Column, MacroError> {
    match field {
        Field::Name => Ok(Column::Name),
        Field::Duration => Ok(Column::Duration),
        Field::Start => Ok(Column::Start),
        Field::Finish => Ok(Column::Finish),
        Field::Predecessors => Ok(Column::Predecessors),
        Field::Successors => Ok(Column::Successors),
        Field::ResourceNames => Ok(Column::Resources),
        other => Err(MacroError::FieldNotWritable { field: other }),
    }
}

/// Refuse a value the cell would quietly throw away.
///
/// `commit_cell` ignores a duration or a date it cannot read, which leaves a
/// script looking like it worked. Checking first means the line that is wrong
/// is the line that is named.
fn check_value(field: Field, value: &str) -> Result<(), MacroError> {
    let readable = match field {
        Field::Duration => aop_core::parse_duration(value).is_some(),
        Field::Start | Field::Finish => crate::state::parse_date(value).is_some(),
        _ => true,
    };
    if readable {
        Ok(())
    } else {
        Err(MacroError::ValueNotUnderstood {
            field,
            value: value.to_string(),
        })
    }
}

/// The key `AppState::set_filter` matches on.
fn filter_key(filter: TaskFilter) -> &'static str {
    match filter {
        TaskFilter::All => "all",
        TaskFilter::Critical => "critical",
        TaskFilter::Milestones => "milestones",
        TaskFilter::Incomplete => "incomplete",
    }
}

/// The key `AppState::set_group_by` matches on.
///
/// Only some fields can be banded on. Naming one that cannot is a mistake worth
/// reporting rather than quietly ungrouping the view.
fn group_key(field: Field) -> Result<&'static str, MacroError> {
    match field {
        Field::Duration => Ok("duration"),
        Field::Critical => Ok("critical"),
        Field::Milestone => Ok("milestone"),
        Field::ResourceNames => Ok("resources"),
        Field::Start => Ok("start"),
        Field::Finish => Ok("finish"),
        Field::PercentComplete => Ok("complete"),
        other => Err(MacroError::CannotGroupBy { field: other }),
    }
}

/// The key `AppState::sort_tasks` matches on.
fn sort_key(field: Field) -> Result<&'static str, MacroError> {
    match field {
        Field::Start => Ok("start"),
        Field::Finish => Ok("finish"),
        Field::Duration => Ok("duration"),
        Field::Cost => Ok("cost"),
        Field::Name => Ok("name"),
        other => Err(MacroError::CannotSortBy { field: other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macros::script;

    /// One of every command, so the round trip test cannot quietly skip any.
    ///
    /// The assertion below ties this list to `Cmd::NAMES`, which the table
    /// generates, so a new command with no example here fails the build's tests
    /// rather than going untested.
    fn every_command() -> Vec<Cmd> {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 8, 17)
            .and_then(|day| day.and_hms_opt(8, 0, 0))
            .expect("a date this test wrote itself");
        vec![
            Cmd::SelectRow { row: Row(3) },
            Cmd::SelectRows {
                from: Row(3),
                to: Row(7),
            },
            Cmd::ToggleRow { row: Row(9) },
            Cmd::SelectAll {},
            Cmd::ClearSelection {},
            Cmd::InsertTask {},
            Cmd::InsertMilestone {},
            Cmd::InsertSummary {},
            Cmd::AppendTask {
                name: "Draft the \"brief\"".to_string(),
            },
            Cmd::DeleteTasks {},
            Cmd::Indent {},
            Cmd::Outdent {},
            Cmd::MoveUp {},
            Cmd::MoveDown {},
            Cmd::CopyTasks {},
            Cmd::CutTasks {},
            Cmd::PasteTasks {},
            Cmd::ExpandAll {},
            Cmd::CollapseAll {},
            Cmd::Link {},
            Cmd::Unlink {},
            Cmd::SetLink {
                row: Row(5),
                predecessor: Row(3),
                kind: LinkType::SS,
                lag_minutes: -480,
            },
            Cmd::RemoveLink {
                row: Row(5),
                predecessor: Row(3),
            },
            Cmd::SetField {
                row: Row(2),
                field: Field::Duration,
                value: "5 days".to_string(),
            },
            Cmd::SetPercentComplete { percent: 50 },
            Cmd::SetTaskMode {
                mode: TaskMode::Auto,
            },
            Cmd::ToggleActive {},
            Cmd::RespectLinks {},
            Cmd::FillDown {
                field: Field::PercentComplete,
            },
            Cmd::AddResource {
                name: "Ada".to_string(),
            },
            Cmd::DeleteResource {
                resource_row: Row(2),
            },
            Cmd::AssignResource {
                row: Row(2),
                name: "Ada".to_string(),
                units_percent: 50.0,
            },
            Cmd::SetAssignmentUnits {
                row: Row(2),
                name: "Ada".to_string(),
                units_percent: 100.0,
            },
            Cmd::UnassignResource {
                row: Row(2),
                name: "Ada".to_string(),
            },
            Cmd::SetResourceField {
                resource_row: Row(1),
                field: ResourceField::Rate,
                value: "75".to_string(),
            },
            Cmd::SetView {
                view: ViewKind::TrackingGantt,
            },
            Cmd::SetZoom { zoom: Zoom::Weeks },
            Cmd::ZoomToFit {},
            Cmd::SetFilter {
                filter: TaskFilter::Critical,
            },
            Cmd::GroupBy {
                field: Some(Field::ResourceNames),
            },
            Cmd::SortBy {
                field: Field::Start,
            },
            Cmd::ShowColumn {
                field: Field::Cost,
                at: Row(5),
            },
            Cmd::HideColumn {
                field: Field::Cost,
            },
            Cmd::ResetColumns {},
            Cmd::SetViewOption {
                option: ViewOption::CriticalPath,
                on: true,
            },
            Cmd::SetBaseline {},
            Cmd::ClearBaseline {},
            Cmd::SetProjectStart { date },
            Cmd::Level {},
            Cmd::ClearLeveling {},
            Cmd::Undo {},
            Cmd::Redo {},
            Cmd::Note {
                message: "Half done".to_string(),
            },
        ]
    }

    #[test]
    fn the_examples_cover_every_command_in_the_table() {
        let mut named: Vec<&str> = every_command().iter().map(|cmd| cmd.fn_name()).collect();
        named.sort_unstable();
        let mut expected = Cmd::NAMES.to_vec();
        expected.sort_unstable();
        assert_eq!(named, expected, "a command has no example to test with");
    }

    #[test]
    fn every_command_survives_being_written_out_and_read_back() {
        for command in every_command() {
            let line = command.to_script();
            let parsed = script::parse(&line)
                .unwrap_or_else(|error| panic!("{line} would not parse: {error}"));
            assert_eq!(parsed, vec![command], "{line} came back as something else");
        }
    }

    #[test]
    fn a_line_is_named_by_the_command_that_wrote_it() {
        for command in every_command() {
            let line = command.to_script();
            assert!(
                line.starts_with(command.fn_name()),
                "{line} does not start with {}",
                command.fn_name()
            );
            assert!(line.ends_with(");"), "{line} is not a finished statement");
        }
    }

    #[test]
    fn no_two_commands_answer_to_the_same_name() {
        let mut names = Cmd::NAMES.to_vec();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "one of them would never be reachable");
    }

    #[test]
    fn the_commands_that_touch_a_file_are_deliberately_absent() {
        // The whole safety story rests on this: a macro changes the plan in
        // memory, and the planner saves from the interface. Adding any of these
        // means every macro has to be trusted before it is run.
        for banned in ["open", "save", "save_as", "import", "export", "print"] {
            assert!(
                !Cmd::NAMES.contains(&banned),
                "{banned} lets a macro reach the disk"
            );
        }
    }

    #[test]
    fn every_field_can_be_named_in_a_script() {
        for field in Field::ALL {
            let written = field.render();
            assert_eq!(Field::parse(&written), Ok(field), "{written} does not read back");
        }
    }

    #[test]
    fn text_arguments_survive_quotes_and_newlines() {
        let awkward = "a \"quoted\" \\ thing\nover two lines".to_string();
        let written = awkward.render();
        assert_eq!(String::parse(&written), Ok(awkward));
    }

    #[test]
    fn a_round_number_of_units_is_written_without_a_decimal_point() {
        assert_eq!(100.0_f64.render(), "100");
        assert_eq!(37.5_f64.render(), "37.5");
        assert_eq!(f64::parse("100"), Ok(100.0));
    }

    #[test]
    fn a_date_keeps_its_time_of_day_through_the_round_trip() {
        let noon = chrono::NaiveDate::from_ymd_opt(2026, 8, 17)
            .and_then(|day| day.and_hms_opt(12, 30, 0))
            .expect("a date this test wrote itself");
        assert_eq!(NaiveDateTime::parse(&noon.render()), Ok(noon));
    }

    #[test]
    fn rows_are_one_based_at_the_boundary_and_zero_based_after_it() {
        assert_eq!(Row(1).index(5), Ok(0));
        assert_eq!(Row(5).index(5), Ok(4));
        assert_eq!(Row(0).index(5), Err(MacroError::RowsCountFromOne));
        assert_eq!(
            Row(6).index(5),
            Err(MacroError::NoSuchRow { row: 6, rows: 5 })
        );
        // A position to insert at may sit one past the end; a row may not.
        assert_eq!(Row(6).insert_index(5), Ok(5));
        assert_eq!(
            Row(7).insert_index(5),
            Err(MacroError::NoSuchRow { row: 7, rows: 5 })
        );
    }

    #[test]
    fn a_call_with_the_wrong_number_of_arguments_says_what_it_wanted() {
        let error = Cmd::from_call("select_rows", &["3".to_string()])
            .expect_err("one argument is not two");
        assert!(error.contains("from, to"), "{error}");
        assert!(error.contains("2 argument"), "{error}");
    }

    #[test]
    fn a_command_nobody_has_written_is_reported_by_name() {
        let error = Cmd::from_call("summon_the_moon", &[]).expect_err("no such command");
        assert!(error.contains("summon_the_moon"), "{error}");
    }
}
