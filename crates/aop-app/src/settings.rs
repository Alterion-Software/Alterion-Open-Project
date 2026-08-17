//! Settings that outlive a session, in a file a person can read and edit.
//!
//! These are preferences about the application rather than anything belonging
//! to a plan: who you are, which palette to paint, how dates read. They are
//! deliberately kept out of the `.aprj` file, since a plan sent to someone else
//! should not carry the author's palette or their name over the top of theirs.
//!
//! The format is plain `key = value` under `[section]` headers, so it can be
//! read and corrected in any editor without the application. Anything
//! unrecognised is left alone rather than treated as an error: a settings file
//! from a newer build should not stop an older one from starting.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::theme::ThemeChoice;

/// Everything remembered between sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub user_name: String,
    pub user_initials: String,
    pub company: String,
    pub theme: ThemeChoice,
    pub date_format: usize,
    pub show_timeline: bool,
    pub show_outline_number: bool,
    pub show_critical: bool,
    /// Only the bindings the user has changed. Defaults are left out so a later
    /// release can improve one and have it reach anyone who never touched it.
    pub keys: crate::keymap::Keymap,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            user_name: String::new(),
            user_initials: String::new(),
            company: String::new(),
            theme: ThemeChoice::default(),
            date_format: 0,
            show_timeline: true,
            show_outline_number: false,
            // Marking the critical path everywhere by default turns the whole
            // plan red, which says nothing about which parts matter.
            show_critical: false,
            keys: crate::keymap::Keymap::default(),
        }
    }
}

/// Where the file lives.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("alterion-open-project").join("config.cfg"))
}

impl Settings {
    /// Render the file, comments and all.
    ///
    /// The comments are the point: this is a file people are invited to edit,
    /// so it says what each section is for rather than being a bare dump.
    pub fn to_text(&self) -> String {
        format!(
            "# Alterion Open Project settings\n\
             # Edited by hand is fine. Unknown keys are ignored.\n\
             \n\
             [general]\n\
             user_name = {}\n\
             user_initials = {}\n\
             company = {}\n\
             \n\
             [appearance]\n\
             # system, light or dark. system follows the desktop.\n\
             theme = {}\n\
             date_format = {}\n\
             \n\
             [view]\n\
             timeline = {}\n\
             outline_numbers = {}\n\
             critical_path = {}\n\
             {}",
            self.user_name,
            self.user_initials,
            self.company,
            self.theme.label().to_ascii_lowercase(),
            self.date_format,
            self.show_timeline,
            self.show_outline_number,
            self.show_critical,
            self.keyboard_section(),
        )
    }

    /// The keyboard section, listing only what has been changed.
    ///
    /// An action written with nothing after the equals sign is one the user
    /// deliberately unbound, which is different from one they never touched.
    fn keyboard_section(&self) -> String {
        let mut out = String::from(
            "\n[keyboard]\n\
             # Only bindings you have changed. Delete a line to go back to the\n\
             # default. An empty value means the command has no shortcut.\n\
             # Written as Ctrl+S, Alt+Shift+Right, F2.\n",
        );
        for (action, binding) in self.keys.customised() {
            out.push_str(&format!(
                "{} = {}\n",
                action.key(),
                binding.map(String::as_str).unwrap_or("")
            ));
        }
        out
    }

    /// Read the file, keeping the defaults for anything absent or unreadable.
    ///
    /// A malformed line is skipped rather than failing the parse. Refusing to
    /// start over one bad line in a preferences file would be a poor trade.
    pub fn from_text(text: &str) -> Self {
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            values.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }

        let mut settings = Settings::default();
        let text_of = |key: &str| values.get(key).cloned();
        let flag = |key: &str, fallback: bool| {
            values
                .get(key)
                .map(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1" | "on"))
                .unwrap_or(fallback)
        };

        if let Some(value) = text_of("user_name") {
            settings.user_name = value;
        }
        if let Some(value) = text_of("user_initials") {
            settings.user_initials = value;
        }
        if let Some(value) = text_of("company") {
            settings.company = value;
        }
        if let Some(value) = text_of("theme") {
            // Written lower case, matched however it comes back.
            let label = ThemeChoice::ORDER
                .into_iter()
                .find(|choice| choice.label().eq_ignore_ascii_case(&value));
            settings.theme = label.unwrap_or_default();
        }
        if let Some(value) = text_of("date_format").and_then(|v| v.parse().ok()) {
            settings.date_format = value;
        }
        // Anything named after an action is a binding; everything else has
        // already been taken above.
        for (key, value) in &values {
            settings.keys.set_from_config(key, value);
        }

        settings.show_timeline = flag("timeline", settings.show_timeline);
        settings.show_outline_number = flag("outline_numbers", settings.show_outline_number);
        settings.show_critical = flag("critical_path", settings.show_critical);

        settings
    }

    /// Read the settings, or the defaults if there are none yet.
    ///
    /// A first run writes the defaults out rather than waiting for something to
    /// change. The file is meant to be editable by hand, and a file that does
    /// not exist until you happen to change a setting is not discoverable.
    pub fn load() -> Self {
        let Some(path) = path() else {
            return Settings::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Settings::from_text(&text),
            Err(_) => {
                let settings = Settings::default();
                settings.save();
                settings
            }
        }
    }

    /// Write the settings out.
    ///
    /// Quiet on failure: this is called whenever a preference changes, and a
    /// dialog about an unwritable config directory in the middle of choosing a
    /// theme would be worse than the setting not sticking.
    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let _ = std::fs::write(path, self.to_text());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_round_trip() {
        let settings = Settings {
            user_name: "Chace Berry".into(),
            user_initials: "CB".into(),
            company: "Alterion".into(),
            theme: ThemeChoice::Light,
            date_format: 3,
            show_timeline: false,
            show_outline_number: true,
            show_critical: true,
            keys: {
                let mut keys = crate::keymap::Keymap::default();
                keys.bind(crate::keymap::Action::SetBaseline, "Ctrl+B");
                keys.clear(crate::keymap::Action::Print);
                keys
            },
        };
        assert_eq!(Settings::from_text(&settings.to_text()), settings);
    }

    #[test]
    fn an_absent_file_reads_as_the_defaults() {
        assert_eq!(Settings::from_text(""), Settings::default());
    }

    #[test]
    fn a_malformed_line_is_skipped_rather_than_failing_the_file() {
        // Refusing to start over one bad line in a preferences file would be a
        // poor trade for the user.
        let text = "[general]\nthis line has no equals sign\nuser_name = Ada\n";
        assert_eq!(Settings::from_text(text).user_name, "Ada");
    }

    #[test]
    fn a_key_from_a_newer_build_is_ignored_rather_than_fatal() {
        let text = "[general]\nuser_name = Ada\nsomething_new = 42\n";
        let settings = Settings::from_text(text);
        assert_eq!(settings.user_name, "Ada");
        assert_eq!(settings.theme, ThemeChoice::default());
    }

    #[test]
    fn a_name_with_spaces_and_symbols_comes_back_whole() {
        let text = "user_name = Ada Lovelace-King\ncompany = Bits & Bobs, Ltd\n";
        let settings = Settings::from_text(text);
        assert_eq!(settings.user_name, "Ada Lovelace-King");
        assert_eq!(settings.company, "Bits & Bobs, Ltd");
    }

    #[test]
    fn the_theme_reads_back_whatever_case_it_was_written_in() {
        for text in ["theme = light", "theme = Light", "theme = LIGHT"] {
            assert_eq!(Settings::from_text(text).theme, ThemeChoice::Light);
        }
    }

    #[test]
    fn an_unrecognised_theme_falls_back_to_following_the_desktop() {
        assert_eq!(Settings::from_text("theme = puce").theme, ThemeChoice::System);
    }

    #[test]
    fn flags_accept_the_spellings_a_person_would_actually_write() {
        for value in ["true", "yes", "1", "on"] {
            assert!(Settings::from_text(&format!("timeline = {value}")).show_timeline);
        }
        assert!(!Settings::from_text("timeline = false").show_timeline);
    }
}
