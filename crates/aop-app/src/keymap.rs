//! What the keyboard does, and letting people change it.
//!
//! Shortcuts are held as data rather than as a `match` over key events, so the
//! same table can drive the keyboard, be listed in the settings, be rebound,
//! and be written to the config file. A hard-coded match can do the first of
//! those and none of the rest.
//!
//! A binding is stored in the form it is shown in: `Ctrl+S`, `Alt+Shift+Right`,
//! `F2`. Matching a key press means rendering it the same way and comparing the
//! two strings. That keeps what the user sees, what the file holds, and what
//! the matcher compares all the same thing, so none of them can drift.

use std::collections::BTreeMap;

use dioxus::prelude::{Key, Modifiers};

/// Something the keyboard can be pointed at.
///
/// Everything the application can do from a key press is here, including the
/// things that have no binding by default: the settings list is generated from
/// this, and an action missing from it is an action nobody can bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    // File
    New,
    Open,
    Save,
    SaveAs,
    Print,
    Export,
    CloseProject,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    EditCell,
    // Task
    InsertTask,
    InsertMilestone,
    InsertSummary,
    Indent,
    Outdent,
    MoveUp,
    MoveDown,
    Link,
    Unlink,
    TaskInformation,
    ToggleActive,
    ManuallySchedule,
    AutoSchedule,
    RespectLinks,
    // Project
    ProjectInformation,
    AssignResources,
    SetBaseline,
    ScrollToTask,
    // View
    ZoomIn,
    ZoomOut,
    ToggleTimeline,
    ToggleCriticalPath,
    ToggleOutlineNumber,
    ExpandAll,
    CollapseAll,
    MaximiseTable,
    MaximiseChart,
    // Navigation
    SelectUp,
    SelectDown,
}

impl Action {
    pub const ALL: [Action; 43] = [
        Action::New,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::Print,
        Action::Export,
        Action::CloseProject,
        Action::Undo,
        Action::Redo,
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::Delete,
        Action::EditCell,
        Action::InsertTask,
        Action::InsertMilestone,
        Action::InsertSummary,
        Action::Indent,
        Action::Outdent,
        Action::MoveUp,
        Action::MoveDown,
        Action::Link,
        Action::Unlink,
        Action::TaskInformation,
        Action::ToggleActive,
        Action::ManuallySchedule,
        Action::AutoSchedule,
        Action::RespectLinks,
        Action::ProjectInformation,
        Action::AssignResources,
        Action::SetBaseline,
        Action::ScrollToTask,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ToggleTimeline,
        Action::ToggleCriticalPath,
        Action::ToggleOutlineNumber,
        Action::ExpandAll,
        Action::CollapseAll,
        Action::MaximiseTable,
        Action::MaximiseChart,
        Action::SelectUp,
        Action::SelectDown,
    ];

    /// Which part of the settings list the action belongs under.
    pub fn group(self) -> &'static str {
        match self {
            Action::New
            | Action::Open
            | Action::Save
            | Action::SaveAs
            | Action::Print
            | Action::Export
            | Action::CloseProject => "File",
            Action::Undo
            | Action::Redo
            | Action::Cut
            | Action::Copy
            | Action::Paste
            | Action::Delete
            | Action::EditCell => "Edit",
            Action::InsertTask
            | Action::InsertMilestone
            | Action::InsertSummary
            | Action::Indent
            | Action::Outdent
            | Action::MoveUp
            | Action::MoveDown
            | Action::Link
            | Action::Unlink
            | Action::TaskInformation
            | Action::ToggleActive
            | Action::ManuallySchedule
            | Action::AutoSchedule
            | Action::RespectLinks => "Task",
            Action::ProjectInformation
            | Action::AssignResources
            | Action::SetBaseline
            | Action::ScrollToTask => "Project",
            Action::ZoomIn
            | Action::ZoomOut
            | Action::ToggleTimeline
            | Action::ToggleCriticalPath
            | Action::ToggleOutlineNumber
            | Action::ExpandAll
            | Action::CollapseAll
            | Action::MaximiseTable
            | Action::MaximiseChart => "View",
            Action::SelectUp | Action::SelectDown => "Navigation",
        }
    }

    /// The groups, in the order the settings list shows them.
    pub const GROUPS: [&'static str; 6] =
        ["File", "Edit", "Task", "Project", "View", "Navigation"];

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New project",
            Action::Open => "Open",
            Action::Save => "Save",
            Action::SaveAs => "Save As",
            Action::Print => "Print",
            Action::Export => "Export",
            Action::CloseProject => "Close project",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Cut => "Cut",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::Delete => "Delete task",
            Action::EditCell => "Edit the selected cell",
            Action::InsertTask => "Insert task",
            Action::InsertMilestone => "Insert milestone",
            Action::InsertSummary => "Insert summary task",
            Action::Indent => "Indent",
            Action::Outdent => "Outdent",
            Action::MoveUp => "Move up",
            Action::MoveDown => "Move down",
            Action::Link => "Link tasks",
            Action::Unlink => "Unlink tasks",
            Action::TaskInformation => "Task Information",
            Action::ToggleActive => "Activate or deactivate",
            Action::ManuallySchedule => "Manually schedule",
            Action::AutoSchedule => "Auto schedule",
            Action::RespectLinks => "Respect links",
            Action::ProjectInformation => "Project Information",
            Action::AssignResources => "Assign Resources",
            Action::SetBaseline => "Set baseline",
            Action::ScrollToTask => "Scroll to task",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::ToggleTimeline => "Show or hide the timeline",
            Action::ToggleCriticalPath => "Show or hide the critical path",
            Action::ToggleOutlineNumber => "Show or hide outline numbers",
            Action::ExpandAll => "Expand all",
            Action::CollapseAll => "Collapse all",
            Action::MaximiseTable => "Maximise the table",
            Action::MaximiseChart => "Maximise the chart",
            Action::SelectUp => "Select the row above",
            Action::SelectDown => "Select the row below",
        }
    }

    /// The name used in the config file. Stable: renaming one loses the user's
    /// binding for it, so these do not follow the label.
    pub fn key(self) -> &'static str {
        match self {
            Action::New => "new",
            Action::Open => "open",
            Action::Save => "save",
            Action::SaveAs => "save_as",
            Action::Print => "print",
            Action::Export => "export",
            Action::CloseProject => "close_project",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::Cut => "cut",
            Action::Copy => "copy",
            Action::Paste => "paste",
            Action::Delete => "delete",
            Action::EditCell => "edit_cell",
            Action::InsertTask => "insert_task",
            Action::InsertMilestone => "insert_milestone",
            Action::InsertSummary => "insert_summary",
            Action::Indent => "indent",
            Action::Outdent => "outdent",
            Action::MoveUp => "move_up",
            Action::MoveDown => "move_down",
            Action::Link => "link",
            Action::Unlink => "unlink",
            Action::TaskInformation => "task_information",
            Action::ToggleActive => "toggle_active",
            Action::ManuallySchedule => "manually_schedule",
            Action::AutoSchedule => "auto_schedule",
            Action::RespectLinks => "respect_links",
            Action::ProjectInformation => "project_information",
            Action::AssignResources => "assign_resources",
            Action::SetBaseline => "set_baseline",
            Action::ScrollToTask => "scroll_to_task",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::ToggleTimeline => "toggle_timeline",
            Action::ToggleCriticalPath => "toggle_critical_path",
            Action::ToggleOutlineNumber => "toggle_outline_number",
            Action::ExpandAll => "expand_all",
            Action::CollapseAll => "collapse_all",
            Action::MaximiseTable => "maximise_table",
            Action::MaximiseChart => "maximise_chart",
            Action::SelectUp => "select_up",
            Action::SelectDown => "select_down",
        }
    }

    pub fn from_key(key: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|action| action.key() == key)
    }

    /// What the action is bound to out of the box, if anything.
    ///
    /// Several actions deliberately start unbound. A default for everything
    /// would mean inventing chords nobody expects, and colliding with the ones
    /// people do.
    pub fn default_binding(self) -> Option<&'static str> {
        Some(match self {
            Action::New => "Ctrl+N",
            Action::Open => "Ctrl+O",
            Action::Save => "Ctrl+S",
            Action::Undo => "Ctrl+Z",
            Action::Redo => "Ctrl+Y",
            Action::Cut => "Ctrl+X",
            Action::Copy => "Ctrl+C",
            Action::Paste => "Ctrl+V",
            Action::Delete => "Delete",
            Action::EditCell => "F2",
            Action::InsertTask => "Insert",
            Action::Indent => "Alt+Shift+Right",
            Action::Outdent => "Alt+Shift+Left",
            Action::MoveUp => "Alt+Shift+Up",
            Action::MoveDown => "Alt+Shift+Down",
            Action::SelectUp => "Up",
            Action::SelectDown => "Down",
            _ => return None,
        })
    }
}

/// Render a key press the way a binding is written.
///
/// Returns `None` for a press that is only a modifier, so holding Ctrl on its
/// own is not treated as a shortcut or recorded as one.
/// Whether the "this is a command" modifier is held.
///
/// The bindings are all written with `Ctrl`, because that is what they are
/// called in the keyboard page and on the two platforms where that is the key.
/// A Mac keyboard does not work that way: the command key is Cmd, which
/// arrives as `META`, and reading only `CONTROL` meant that on macOS every
/// shortcut with a modifier resolved to the bare letter and nothing matched.
/// Cmd+S saved nothing.
///
/// Ctrl is still accepted there as well as Cmd. A planner who came from
/// Windows and presses the key they are used to gets what they meant, and
/// there is no binding it could be confused with.
fn is_accelerator(modifiers: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.contains(Modifiers::META) || modifiers.contains(Modifiers::CONTROL)
    } else {
        // Not `META` here: that is the Windows key and the Super key, which
        // belong to the desktop rather than to this application.
        modifiers.contains(Modifiers::CONTROL)
    }
}

pub fn shortcut_for(key: &Key, modifiers: Modifiers) -> Option<String> {
    let name = match key {
        Key::Character(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            text.to_uppercase()
        }
        Key::ArrowUp => "Up".into(),
        Key::ArrowDown => "Down".into(),
        Key::ArrowLeft => "Left".into(),
        Key::ArrowRight => "Right".into(),
        Key::Enter => "Enter".into(),
        Key::Escape => "Escape".into(),
        Key::Tab => "Tab".into(),
        Key::Backspace => "Backspace".into(),
        Key::Delete => "Delete".into(),
        Key::Insert => "Insert".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::F1 => "F1".into(),
        Key::F2 => "F2".into(),
        Key::F3 => "F3".into(),
        Key::F4 => "F4".into(),
        Key::F5 => "F5".into(),
        Key::F6 => "F6".into(),
        Key::F7 => "F7".into(),
        Key::F8 => "F8".into(),
        Key::F9 => "F9".into(),
        Key::F10 => "F10".into(),
        Key::F11 => "F11".into(),
        Key::F12 => "F12".into(),
        // A modifier on its own is not a shortcut.
        _ => return None,
    };

    let mut parts = Vec::new();
    if is_accelerator(modifiers) {
        parts.push("Ctrl");
    }
    if modifiers.contains(Modifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(Modifiers::SHIFT) {
        parts.push("Shift");
    }
    parts.push(&name);
    Some(parts.join("+"))
}

/// Which key press runs which action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    /// Only the bindings that differ from the defaults are held here, so a
    /// later release can change a default and have it reach anyone who never
    /// touched that one.
    changed: BTreeMap<Action, Option<String>>,
}

impl Keymap {
    /// What an action is bound to, if anything.
    pub fn binding(&self, action: Action) -> Option<String> {
        match self.changed.get(&action) {
            Some(binding) => binding.clone(),
            None => action.default_binding().map(str::to_string),
        }
    }

    /// Whether the user has moved this one off its default.
    pub fn is_customised(&self, action: Action) -> bool {
        self.changed.contains_key(&action)
    }

    /// Bind an action, taking the key press off whatever else had it.
    ///
    /// Two actions on one key means one of them silently never runs, so the
    /// older binding is cleared rather than left to lose the race.
    pub fn bind(&mut self, action: Action, shortcut: &str) -> Option<Action> {
        let clash = self
            .action_for(shortcut)
            .filter(|existing| *existing != action);
        if let Some(existing) = clash {
            self.changed.insert(existing, None);
        }
        self.changed.insert(action, Some(shortcut.to_string()));
        clash
    }

    /// Leave an action with no binding at all.
    pub fn clear(&mut self, action: Action) {
        self.changed.insert(action, None);
    }

    /// Put an action back to how it started.
    pub fn reset(&mut self, action: Action) {
        self.changed.remove(&action);
    }

    /// Put every action back to how it started.
    pub fn reset_all(&mut self) {
        self.changed.clear();
    }

    /// Which action a key press runs.
    pub fn action_for(&self, shortcut: &str) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|action| self.binding(*action).as_deref() == Some(shortcut))
    }

    /// The customised bindings, for writing to the config file. Defaults are
    /// left out so that changing one later still reaches people.
    pub fn customised(&self) -> impl Iterator<Item = (Action, Option<&String>)> {
        self.changed
            .iter()
            .map(|(action, binding)| (*action, binding.as_ref()))
    }

    /// Read bindings back. An empty value means deliberately unbound.
    pub fn set_from_config(&mut self, key: &str, value: &str) {
        let Some(action) = Action::from_key(key) else {
            return;
        };
        let value = value.trim();
        if value.is_empty() {
            self.changed.insert(action, None);
        } else {
            self.changed.insert(action, Some(value.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_stable_key_and_survives_a_round_trip() {
        for action in Action::ALL {
            assert_eq!(Action::from_key(action.key()), Some(action));
        }
    }

    #[test]
    fn no_two_actions_share_a_config_key() {
        let mut keys: Vec<&str> = Action::ALL.iter().map(|a| a.key()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "a shared key would overwrite a binding");
    }

    #[test]
    fn no_two_actions_start_on_the_same_key_press() {
        let mut seen: Vec<String> = Action::ALL
            .iter()
            .filter_map(|a| a.default_binding().map(str::to_string))
            .collect();
        seen.sort();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "one of them would never run");
    }

    #[test]
    fn a_press_is_written_the_same_way_a_binding_is() {
        let ctrl = Modifiers::CONTROL;
        assert_eq!(
            shortcut_for(&Key::Character("s".into()), ctrl).as_deref(),
            Some("Ctrl+S")
        );
        assert_eq!(
            shortcut_for(&Key::ArrowRight, Modifiers::ALT | Modifiers::SHIFT).as_deref(),
            Some("Alt+Shift+Right")
        );
        assert_eq!(
            shortcut_for(&Key::F2, Modifiers::empty()).as_deref(),
            Some("F2")
        );
    }

    #[test]
    fn holding_a_modifier_on_its_own_is_not_a_shortcut() {
        // Otherwise reaching for Ctrl would be recorded as a binding.
        assert_eq!(shortcut_for(&Key::Control, Modifiers::CONTROL), None);

        // The command key. On macOS Cmd is what a planner presses and it
        // arrives as `META`; everywhere else `META` is the desktop's own key
        // and means nothing here.
        let meta = shortcut_for(&Key::Character("s".into()), Modifiers::META);
        if cfg!(target_os = "macos") {
            assert_eq!(meta.as_deref(), Some("Ctrl+S"), "Cmd+S has to be a shortcut on macOS");
        } else {
            assert_eq!(meta.as_deref(), Some("S"), "the Super key is not an accelerator here");
        }
        assert_eq!(shortcut_for(&Key::Shift, Modifiers::SHIFT), None);
    }

    #[test]
    fn the_defaults_are_what_the_map_reports_before_anything_changes() {
        let map = Keymap::default();
        assert_eq!(map.binding(Action::Save).as_deref(), Some("Ctrl+S"));
        assert_eq!(map.action_for("Ctrl+S"), Some(Action::Save));
        assert!(map.binding(Action::SetBaseline).is_none(), "unbound by default");
    }

    #[test]
    fn binding_a_key_press_takes_it_off_whatever_had_it() {
        // Two actions on one key means one of them silently never runs.
        let mut map = Keymap::default();
        let displaced = map.bind(Action::SetBaseline, "Ctrl+S");

        assert_eq!(displaced, Some(Action::Save));
        assert_eq!(map.action_for("Ctrl+S"), Some(Action::SetBaseline));
        assert!(map.binding(Action::Save).is_none(), "Save gave the key up");
    }

    #[test]
    fn rebinding_an_action_to_the_key_it_already_has_displaces_nothing() {
        let mut map = Keymap::default();
        assert_eq!(map.bind(Action::Save, "Ctrl+S"), None);
        assert_eq!(map.action_for("Ctrl+S"), Some(Action::Save));
    }

    #[test]
    fn an_action_can_be_left_with_no_binding() {
        let mut map = Keymap::default();
        map.clear(Action::Save);
        assert!(map.binding(Action::Save).is_none());
        assert_eq!(map.action_for("Ctrl+S"), None);
    }

    #[test]
    fn resetting_puts_the_default_back() {
        let mut map = Keymap::default();
        map.bind(Action::Save, "Ctrl+Shift+S");
        assert!(map.is_customised(Action::Save));
        map.reset(Action::Save);
        assert!(!map.is_customised(Action::Save));
        assert_eq!(map.binding(Action::Save).as_deref(), Some("Ctrl+S"));
    }

    #[test]
    fn only_the_changed_bindings_are_written_out() {
        // So that changing a default in a later release still reaches anyone
        // who never touched that one.
        let mut map = Keymap::default();
        map.bind(Action::SetBaseline, "Ctrl+B");
        let written: Vec<_> = map.customised().collect();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].0, Action::SetBaseline);
    }

    #[test]
    fn an_empty_value_in_the_file_means_deliberately_unbound() {
        let mut map = Keymap::default();
        map.set_from_config("save", "");
        assert!(map.binding(Action::Save).is_none());
        assert!(map.is_customised(Action::Save));
    }

    #[test]
    fn an_unknown_action_in_the_file_is_ignored() {
        let mut map = Keymap::default();
        map.set_from_config("summon_the_moon", "Ctrl+M");
        assert_eq!(map, Keymap::default());
    }
}
