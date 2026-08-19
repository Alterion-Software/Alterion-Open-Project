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
    /// Which rules are drawn behind the rows. Off means the pane relies on the
    /// row banding alone, which some planners prefer for a printed look.
    pub grid_rows: bool,
    pub grid_columns: bool,
    /// The vertical rule marking today in the chart.
    pub grid_status_date: bool,
    /// Bars drawn to whole days rather than to the hour, so a half day of work
    /// still reads as a day wide.
    pub round_bars: bool,
    /// Dependency arrows between bars.
    pub show_links: bool,
    /// The task name written beside its bar.
    pub bar_text: bool,
    /// Whether annotation shapes are drawn over the chart. A per view choice
    /// in Project too, kept with the planner rather than in the plan.
    pub show_drawings: bool,

    // ---- Alterion Collaborate -------------------------------------------
    /// Where the identity provider lives. Everything else about it is read
    /// from its own discovery document, so somebody running their own only
    /// changes this one line.
    ///
    /// Empty until somebody fills it in, and deliberately so. The provider is
    /// self hosted and self deployable, so any address shipped here would be
    /// somebody else's server signing in every planner who never looked at the
    /// setting. There is no address that is right for everyone, so there is
    /// none.
    pub idp_issuer: String,
    /// How this application identifies itself to that provider. Not a secret:
    /// a desktop application is a public client and proves itself with PKCE
    /// rather than with something it would have to hide on disk. Issued by
    /// whoever runs the provider, so it is empty until they say what it is.
    pub idp_client_id: String,
    /// The page a person manages their own account on, at the provider.
    ///
    /// Empty means "under the issuer", which is what nearly every deployment
    /// wants and what nobody should have to fill in. It is here at all because
    /// the provider is self hosted: whoever runs one can put its account page
    /// wherever they like, and nothing in the discovery document says where.
    pub idp_account_url: String,
    /// Where the sync server lives. A different address from the provider:
    /// one signs people in, the other keeps plans, and a deployment can put
    /// them on different machines.
    pub collaborate_server: String,
    /// Whether to offer signing in and syncing at all.
    pub collaborate: bool,

    // ---- the licence, and what changed ----------------------------------
    /// The version whose licence was acknowledged. Empty until somebody has,
    /// and that emptiness is the whole record: absent means show it, present
    /// means never again.
    pub licence_acknowledged: String,
    /// When that happened, as RFC 3339. Kept beside the version rather than
    /// instead of it, since "which licence" and "when" are two different facts
    /// and a record holding one of them answers half the question.
    pub licence_acknowledged_at: String,
    /// The version this copy last finished starting as. Empty on a first run,
    /// which is how a first run is told from an update: nothing changed for
    /// somebody who never had the previous one.
    pub last_version: String,
    /// Whether what changed is shown after an update.
    pub patch_notes: bool,
    /// Whether the support page is offered after an update. Its own key, and
    /// deliberately so: silencing one must never silence the other.
    pub support_page: bool,
    /// Whether to look for a newer release at all. Honoured everywhere,
    /// including the check made at start up.
    pub update_check: bool,
    /// The one version somebody asked never to be offered again. Empty until
    /// they do, and that emptiness is the whole record.
    ///
    /// The version rather than a flag, and that is the point of the key. A
    /// flag would either silence every release after it or need clearing at
    /// some moment nobody could name, whereas a version answers exactly one
    /// question: is the release that was just found this one. Anything newer
    /// is a different release and is still offered.
    pub skip_version: String,
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
            grid_rows: true,
            grid_columns: true,
            grid_status_date: true,
            round_bars: false,
            show_links: true,
            bar_text: true,
            show_drawings: true,
            idp_issuer: String::new(),
            idp_client_id: String::new(),
            idp_account_url: String::new(),
            collaborate_server: String::new(),
            // Off until somebody asks for it. A planner who never signs in
            // should never be shown a sign in button.
            collaborate: false,
            licence_acknowledged: String::new(),
            licence_acknowledged_at: String::new(),
            last_version: String::new(),
            // Both on, and both refusable. What changed is worth reading once
            // per release; the ask for help is worth making once and then
            // never again if the answer was no.
            patch_notes: true,
            support_page: true,
            update_check: true,
            // Nothing skipped until somebody skips something.
            skip_version: String::new(),
            // Marking the critical path everywhere by default turns the whole
            // plan red, which says nothing about which parts matter.
            show_critical: false,
            keys: crate::keymap::Keymap::default(),
        }
    }
}

/// The folder this application keeps everything of its own in.
///
/// One answer, because there were six and every one of them was Unix only:
/// `XDG_CONFIG_HOME`, or `HOME/.config`. Windows sets neither, so all six
/// returned `None` there and the settings, the recent list, the added
/// dictionary words, the crash recovery snapshots and the port file the
/// single instance check depends on were silently never written. Nothing
/// failed; everything simply forgot itself on every launch.
///
/// `APPDATA` is where Windows keeps exactly this kind of file, and it roams
/// with the account. The Unix answer is unchanged, so nobody's existing
/// settings move.
pub fn config_root() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let appdata = PathBuf::from(appdata);
            if !appdata.as_os_str().is_empty() {
                return Some(appdata.join("Alterion Open Project"));
            }
        }
        // Last resort, and better than forgetting everything: a folder beside
        // the profile rather than none at all.
        return crate::state::home_dir()
            .map(|home| home.join("AppData").join("Roaming").join("Alterion Open Project"));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| crate::state::home_dir().map(|home| home.join(".config")))?;
    Some(base.join("alterion-open-project"))
}

/// Where the file lives.
pub fn path() -> Option<PathBuf> {
    config_root().map(|dir| dir.join("config.cfg"))
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
             \n\
             [gridlines]\n\
             grid_rows = {}\n\
             grid_columns = {}\n\
             grid_status_date = {}\n\
             \n\
             [layout]\n\
             round_bars = {}\n\
             show_links = {}\n\
             bar_text = {}\n\
             show_drawings = {}\n\
             \n\
             [collaborate]\n\
             # The identity provider to sign in against. Self hosted providers\n\
             # work by changing this alone: everything else is read from its\n\
             # own discovery document. There is no default, because there is no\n\
             # address that would be right for everybody.\n\
             collaborate = {}\n\
             idp_issuer = {}\n\
             idp_client_id = {}\n\
             # Where Manage account opens. Left empty it is the issuer's own\n\
             # account page, which is where it lives unless a deployment has\n\
             # moved it.\n\
             idp_account_url = {}\n\
             # The sync server that keeps the plans. Its own address, since it\n\
             # need not live on the same machine as the provider.\n\
             collaborate_server = {}\n\
             \n\
             [licence]\n\
             # Which version's licence was acknowledged, and when. Delete both\n\
             # lines to be shown it again on the next start.\n\
             acknowledged = {}\n\
             acknowledged_at = {}\n\
             \n\
             [updates]\n\
             # The version this copy last started as. What changed is shown\n\
             # when this and the running version differ, and only then.\n\
             last_version = {}\n\
             patch_notes = {}\n\
             # Whether to offer the support page after an update. Separate\n\
             # from patch_notes on purpose: turning one off leaves the other.\n\
             support_page = {}\n\
             # Whether to look for a newer release at all.\n\
             update_check = {}\n\
             # One version you chose to skip, and only that one. A release\n\
             # newer than it is still offered. Delete this line to be offered\n\
             # it again.\n\
             skip_version = {}\n\
             {}",
            self.user_name,
            self.user_initials,
            self.company,
            self.theme.label().to_ascii_lowercase(),
            self.date_format,
            self.show_timeline,
            self.show_outline_number,
            self.show_critical,
            self.grid_rows,
            self.grid_columns,
            self.grid_status_date,
            self.round_bars,
            self.show_links,
            self.bar_text,
            self.show_drawings,
            self.collaborate,
            self.idp_issuer,
            self.idp_client_id,
            self.idp_account_url,
            self.collaborate_server,
            self.licence_acknowledged,
            self.licence_acknowledged_at,
            self.last_version,
            self.patch_notes,
            self.support_page,
            self.update_check,
            self.skip_version,
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
        settings.grid_rows = flag("grid_rows", settings.grid_rows);
        settings.grid_columns = flag("grid_columns", settings.grid_columns);
        settings.grid_status_date = flag("grid_status_date", settings.grid_status_date);
        settings.round_bars = flag("round_bars", settings.round_bars);
        settings.show_links = flag("show_links", settings.show_links);
        settings.bar_text = flag("bar_text", settings.bar_text);
        settings.show_drawings = flag("show_drawings", settings.show_drawings);
        settings.collaborate = flag("collaborate", settings.collaborate);
        if let Some(value) = text_of("idp_issuer").filter(|v| !v.trim().is_empty()) {
            settings.idp_issuer = value;
        }
        if let Some(value) = text_of("idp_client_id").filter(|v| !v.trim().is_empty()) {
            settings.idp_client_id = value;
        }
        if let Some(value) = text_of("idp_account_url").filter(|v| !v.trim().is_empty()) {
            settings.idp_account_url = value;
        }
        if let Some(value) = text_of("collaborate_server").filter(|v| !v.trim().is_empty()) {
            settings.collaborate_server = value;
        }

        // Read whole rather than filtered for emptiness: an empty
        // acknowledgement is the same as none, and writing one back empty is
        // how somebody asks to be shown the licence again.
        if let Some(value) = text_of("acknowledged") {
            settings.licence_acknowledged = value;
        }
        if let Some(value) = text_of("acknowledged_at") {
            settings.licence_acknowledged_at = value;
        }
        if let Some(value) = text_of("last_version") {
            settings.last_version = value;
        }
        settings.patch_notes = flag("patch_notes", settings.patch_notes);
        settings.support_page = flag("support_page", settings.support_page);
        settings.update_check = flag("update_check", settings.update_check);
        // Read whole rather than filtered for emptiness, for the same reason
        // the acknowledgement above is: an empty value is a real answer, and
        // writing one back empty is how a skip is withdrawn by hand.
        if let Some(value) = text_of("skip_version") {
            settings.skip_version = value;
        }

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
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err() {
                return;
            }
        let _ = std::fs::write(path, self.to_text());
    }

    /// Forget a skip the running copy has already gone past.
    ///
    /// A skip only ever means "not this one, I am staying where I am". Once
    /// the running version is that one or later, whatever it took to get here
    /// (the installer run by hand, a package manager, a rebuild), the record
    /// describes a release nobody can be offered any more. Left alone it would
    /// sit in the file for good, be shown in Options as though it still meant
    /// something, and have to be reasoned about every time this is read.
    ///
    /// It also happens to be where a value that is not a version at all gets
    /// dropped. Comparison treats anything unparseable as equal to everything,
    /// so `skip_version = yes` typed into the file by hand would otherwise
    /// silence every release. Equal is not newer, so it is cleared here on the
    /// next start rather than quietly suppressing updates for ever.
    pub fn forget_a_spent_skip(&mut self, running: &str) {
        if !self.skip_version.is_empty()
            && !crate::updates::is_newer(&self.skip_version, running)
        {
            self.skip_version.clear();
        }
    }
}

/// Whether a release that has been found is the one that was skipped.
///
/// Equality, and deliberately nothing wider. "Skip this version" is an answer
/// about one release, not a ceiling: a person who skips `1.0.2` because it
/// broke something they rely on still wants to hear about `1.0.3`, which may
/// well be the release that fixes it. Suppressing anything at or below the
/// skipped version would turn one refusal into a permanent silence.
///
/// Compared as versions rather than as text, since `1.0.10` and `1.0.9` are
/// not ordered the way their strings are, and `1.0.0-beta` is not `1.0.0`.
pub fn is_skipped(skipped: &str, version: &str) -> bool {
    !skipped.is_empty()
        && !crate::updates::is_newer(version, skipped)
        && !crate::updates::is_newer(skipped, version)
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
            // Each set away from its default, so the round trip has to carry
            // the value rather than land on it by luck.
            grid_rows: false,
            grid_columns: false,
            grid_status_date: false,
            round_bars: true,
            show_links: false,
            bar_text: false,
            show_drawings: false,
            idp_issuer: "https://auth.example.test".into(),
            idp_client_id: "a-client".into(),
            idp_account_url: "https://auth.example.test/account".into(),
            collaborate_server: "https://sync.example.test".into(),
            collaborate: true,
            licence_acknowledged: "1.0.0-beta".into(),
            licence_acknowledged_at: "2026-08-18T09:14:00Z".into(),
            last_version: "1.0.0-beta".into(),
            patch_notes: false,
            support_page: false,
            update_check: false,
            skip_version: "1.0.2".into(),
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
    fn silencing_one_popup_leaves_the_other_alone() {
        // Two keys rather than one, because "stop telling me what changed" and
        // "stop asking me for money" are different requests.
        let quiet_notes = Settings::from_text("patch_notes = false");
        assert!(!quiet_notes.patch_notes);
        assert!(quiet_notes.support_page);

        let quiet_support = Settings::from_text("support_page = false");
        assert!(quiet_support.patch_notes);
        assert!(!quiet_support.support_page);
    }

    #[test]
    fn deleting_the_acknowledgement_asks_for_the_licence_again() {
        // The record is the only thing suppressing it, so a settings file
        // without one has to read as never acknowledged.
        assert!(Settings::from_text("").licence_acknowledged.is_empty());
        assert_eq!(
            Settings::from_text("acknowledged = 1.0.0-beta").licence_acknowledged,
            "1.0.0-beta"
        );
    }

    #[test]
    fn the_account_page_is_left_to_the_issuer_until_a_deployment_names_one() {
        // Empty is the answer, not a field waiting to be filled in: nearly
        // every provider keeps its account page under its own issuer, and
        // deriving it means nobody has to know that.
        assert!(Settings::default().idp_account_url.is_empty());
        assert_eq!(
            Settings::from_text("idp_account_url = https://id.example.test/profile")
                .idp_account_url,
            "https://id.example.test/profile"
        );
    }

    #[test]
    fn update_checks_are_on_until_they_are_turned_off() {
        assert!(Settings::default().update_check);
        assert!(!Settings::from_text("update_check = false").update_check);
    }

    #[test]
    fn nothing_is_skipped_until_somebody_skips_something() {
        assert!(Settings::default().skip_version.is_empty());
        assert!(!is_skipped("", "1.0.2"));
    }

    #[test]
    fn the_skipped_version_travels_through_the_file_with_everything_else() {
        // Its own assertion as well as the round trip above, because this is
        // the key whose loss would silently start offering a version somebody
        // has already refused.
        let settings = Settings {
            skip_version: "1.0.2".into(),
            ..Settings::default()
        };
        assert_eq!(Settings::from_text(&settings.to_text()).skip_version, "1.0.2");
        assert_eq!(
            Settings::from_text("skip_version = 1.0.2-beta.2").skip_version,
            "1.0.2-beta.2"
        );
    }

    #[test]
    fn deleting_the_line_by_hand_offers_the_version_again() {
        // The record is the only thing suppressing the offer, so a file
        // without one has to read as nothing skipped.
        assert!(Settings::from_text("update_check = true").skip_version.is_empty());
        assert!(!is_skipped(&Settings::from_text("").skip_version, "1.0.2"));
    }

    #[test]
    fn only_the_skipped_version_itself_is_suppressed() {
        assert!(is_skipped("1.0.2", "1.0.2"));
        // The whole feature: newer releases keep coming.
        assert!(!is_skipped("1.0.2", "1.0.3"));
        assert!(!is_skipped("1.0.9", "1.0.10"));
        // And an older one is not resurrected by the skip either. It would
        // never be offered anyway, since only something newer than the running
        // copy is ever found, but the rule must not read as a ceiling.
        assert!(!is_skipped("1.0.2", "1.0.1"));
    }

    #[test]
    fn a_skip_is_matched_as_a_version_rather_than_as_text() {
        // Written 1.0.02 by hand, or padded by a release script, is the same
        // release and must stay skipped.
        assert!(is_skipped("1.0.2", "1.0.2+build.7"));
        // A pre-release is a different release from the version it leads to,
        // which comparing text would get wrong in both directions.
        assert!(!is_skipped("1.0.2-beta", "1.0.2"));
        assert!(!is_skipped("1.0.2", "1.0.2-beta"));
    }

    #[test]
    fn a_skip_the_running_copy_has_passed_is_forgotten() {
        // Somebody who ran the installer by hand is now on the version they
        // skipped, or past it, and the record describes a release nobody can
        // be offered any more.
        let mut caught_up = Settings {
            skip_version: "1.0.2".into(),
            ..Settings::default()
        };
        caught_up.forget_a_spent_skip("1.0.2");
        assert!(caught_up.skip_version.is_empty());

        let mut overtaken = Settings {
            skip_version: "1.0.2".into(),
            ..Settings::default()
        };
        overtaken.forget_a_spent_skip("1.0.3");
        assert!(overtaken.skip_version.is_empty());

        // Still ahead, so still worth honouring.
        let mut still_ahead = Settings {
            skip_version: "1.0.2".into(),
            ..Settings::default()
        };
        still_ahead.forget_a_spent_skip("1.0.1");
        assert_eq!(still_ahead.skip_version, "1.0.2");
    }

    #[test]
    fn a_skip_that_is_not_a_version_is_dropped_rather_than_silencing_everything() {
        // Comparison treats anything unparseable as equal to everything, so a
        // hand typed `skip_version = yes` would suppress every release. It is
        // not newer than the running version, so it goes on the next start.
        let mut nonsense = Settings {
            skip_version: "yes".into(),
            ..Settings::default()
        };
        nonsense.forget_a_spent_skip("1.0.1");
        assert!(nonsense.skip_version.is_empty());
    }

    #[test]
    fn flags_accept_the_spellings_a_person_would_actually_write() {
        for value in ["true", "yes", "1", "on"] {
            assert!(Settings::from_text(&format!("timeline = {value}")).show_timeline);
        }
        assert!(!Settings::from_text("timeline = false").show_timeline);
    }
}
