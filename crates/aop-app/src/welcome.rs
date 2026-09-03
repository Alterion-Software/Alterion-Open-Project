//! The pages shown before the plan is: the licence, what changed, and the ask.
//!
//! Three separate things that share one rule, which is why they share a file:
//! each is shown at most once, for a reason that can be pointed at, and each
//! can be refused for good.
//!
//! ```text
//!   first run          ->  the licence, once, then never again
//!   version changed    ->  what changed, then the support page
//!   version unchanged  ->  nothing at all
//! ```
//!
//! A first run is deliberately not treated as an update. Somebody opening this
//! for the first time has not been given a new version of anything, so release
//! notes for a release they never had would be noise, and an ask for money
//! before they have opened a plan would be worse than noise.
//!
//! The licence text is compiled in rather than read from disk. A licence that
//! can go missing at runtime is one the application can end up unable to show,
//! and "the file was not there" is not an answer to "what am I agreeing to".
//! The same goes for the changelog: release notes copied into Rust by hand
//! drift from the changelog people actually update, so there is one copy and
//! the section for the running version is cut out of it.

use dioxus::prelude::*;

use crate::icons::icon;
use crate::settings::Settings;
use crate::state::AppState;

/// The licence itself, verbatim. Not paraphrased, not summarised, not
/// rewritten: the text that governs is the text that is shown.
pub const LICENCE: &str = include_str!("../../../LICENSE");

/// The changelog, so release notes have one home rather than two.
const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// The version this build is.
pub const RUNNING: &str = env!("CARGO_PKG_VERSION");

// ------------------------------------------------------------ the ask
//
// FILL THESE IN BEFORE RELEASE.
//
// Every detail below starts as UNSET, and an option whose details are still
// UNSET is not drawn at all. That is the point: a dead link is merely
// embarrassing, but an account number that is not the right account number
// sends somebody's money to a stranger, so the failure has to be "the option
// was not offered" rather than "the option was wrong".

/// What an unfilled detail reads as. Compared against, never shown.
const UNSET: &str = "UNSET";

/// The Buy Me a Coffee page, opened in the system browser.
pub const COFFEE_URL: &str = "https://buymeacoffee.com/ChaceBerry";

/// The Ko-fi page, opened the same way.
pub const KOFI_URL: &str = "https://ko-fi.com/chaceberry";

/// Bank transfer details, in the order they are drawn.
pub const BANK_DETAILS: [(&str, &str); 5] = [
    ("Account name", UNSET),
    ("Bank", UNSET),
    ("Account number", UNSET),
    ("Branch code", UNSET),
    ("Reference", UNSET),
];

/// How many of the details above a transfer cannot be made without. Anything
/// after them is useful but optional, so a detail that does not apply to a
/// particular bank can be left unset without hiding the whole option.
const BANK_ESSENTIAL: usize = 3;

/// Whether a detail has been filled in.
fn is_set(value: &str) -> bool {
    !value.trim().is_empty() && value.trim() != UNSET
}

/// Whether there is a coffee link worth showing.
///
/// The scheme is checked as well as the value: `open_in_browser` hands the
/// string to the desktop, and a constant filled in as something other than a
/// web address should not be the thing that finds out what it does with it.
pub fn coffee_offered() -> bool {
    is_set(COFFEE_URL) && COFFEE_URL.starts_with("https://")
}

/// Whether there is a Ko-fi page to send anybody to. Judged the same way the
/// other link is: an address that is not a real one is not offered.
pub fn kofi_offered() -> bool {
    is_set(KOFI_URL) && KOFI_URL.starts_with("https://")
}

/// Whether there are enough bank details to pay into.
pub fn bank_offered() -> bool {
    BANK_DETAILS[..BANK_ESSENTIAL]
        .iter()
        .all(|(_, value)| is_set(value))
}

/// The bank rows that have actually been filled in.
pub fn bank_rows() -> Vec<(&'static str, &'static str)> {
    BANK_DETAILS
        .into_iter()
        .filter(|(_, value)| is_set(value))
        .collect()
}

// ------------------------------------------------------- what to show when

/// A page shown before the application is got on with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Greeting {
    /// The licence, on the very first run.
    Licence,
    /// What changed, after the version moved.
    PatchNotes,
    /// The two ways to help, both optional and both refusable.
    ///
    /// `after_update` says who raised it. When the application did, there is
    /// something to silence and the checkbox is offered; when the planner
    /// asked for it from About there is not, because nothing showed it to
    /// them uninvited.
    Support { after_update: bool },
}

/// Which pages this start owes the person in front of it.
///
/// Pure, and deliberately so: the rule about when each of these appears is the
/// whole feature, and a rule that can only be exercised by launching the
/// application is a rule nobody can check.
pub fn on_start(settings: &Settings, running: &str) -> Vec<Greeting> {
    let mut queue = Vec::new();

    // The record is the only thing suppressing the licence. Absent means it
    // has not been acknowledged, whatever else the file says.
    if !is_acknowledged(settings) {
        queue.push(Greeting::Licence);
    }

    // An empty last version is a first run, not an update. Nothing changed for
    // somebody who never had the previous release.
    let previous = settings.last_version.trim();
    if previous.is_empty() || previous == running.trim() {
        return queue;
    }

    // Nothing to show is not the same as choosing not to show it, but the
    // result is the same and an empty page is worse than no page.
    if settings.patch_notes && notes_for(running).is_some() {
        queue.push(Greeting::PatchNotes);
    }
    if settings.support_page {
        queue.push(Greeting::Support {
            after_update: true,
        });
    }
    queue
}

/// Whether the licence has been acknowledged at all.
pub fn is_acknowledged(settings: &Settings) -> bool {
    !settings.licence_acknowledged.trim().is_empty()
}

/// The changelog section for one version, or nothing if it has none.
///
/// Matched on the whole heading line rather than by searching for the version
/// string, so `1.0.0` never matches the section belonging to `1.0.0-beta`.
pub fn notes_for(version: &str) -> Option<&'static str> {
    let heading = format!("## {}", version.trim());
    let mut body: Option<usize> = None;
    let mut offset = 0usize;

    for line in CHANGELOG.split_inclusive('\n') {
        let starts_here = offset;
        offset += line.len();
        match body {
            None if line.trim_end() == heading => body = Some(offset),
            // The next release's heading ends this one. `### ` is a subheading
            // within the section and does not, which the trailing space in the
            // pattern is what distinguishes.
            Some(from) if line.starts_with("## ") => {
                return Some(CHANGELOG[from..starts_here].trim()).filter(|text| !text.is_empty());
            }
            _ => {}
        }
    }

    body.map(|from| CHANGELOG[from..].trim())
        .filter(|text| !text.is_empty())
}

// ------------------------------------------------------ reading a section

/// One line of a changelog section, once its markup has been read.
///
/// Enough of Markdown to render the changelog and no more. A full parser would
/// be a dependency and a surface; what is here covers what the file actually
/// contains, and anything it does not understand falls through as plain text
/// rather than disappearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Heading(String),
    /// A bullet. The bold lead-in the changelog opens most of them with is
    /// kept apart, since that phrase is the point of the bullet and the rest
    /// is the explanation.
    Bullet {
        lead: String,
        rest: String,
        nested: bool,
    },
    Text(String),
    Blank,
}

/// Strip the inline markup that would otherwise be read out loud as asterisks.
fn plain(text: &str) -> String {
    text.replace("**", "").replace('`', "")
}

/// Split a bullet into its bold lead-in and the rest, when it has one.
fn lead_of(text: &str) -> (String, String) {
    let Some(rest) = text.strip_prefix("**") else {
        return (String::new(), plain(text));
    };
    match rest.split_once("**") {
        Some((lead, tail)) => (plain(lead), plain(tail.trim_start())),
        None => (String::new(), plain(text)),
    }
}

/// Read a changelog section into something that can be drawn.
pub fn read(section: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in section.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            lines.push(Line::Blank);
        } else if let Some(heading) = trimmed.strip_prefix("### ") {
            lines.push(Line::Heading(plain(heading)));
        } else if let Some(bullet) = trimmed.strip_prefix("- ") {
            let (lead, rest) = lead_of(bullet);
            // Indented in the source means indented on screen: the changelog
            // uses it for the detail under a point, and flattening it would
            // promote an aside to a point of its own.
            let nested = raw.starts_with("  ") && !raw.starts_with("- ");
            lines.push(Line::Bullet { lead, rest, nested });
        } else if matches!(lines.last(), Some(Line::Bullet { .. } | Line::Text(_))) {
            // A wrapped continuation of whatever came before, joined back on
            // rather than becoming a paragraph of its own.
            match lines.last_mut() {
                Some(Line::Bullet { rest, .. }) | Some(Line::Text(rest)) => {
                    rest.push(' ');
                    rest.push_str(&plain(trimmed));
                }
                _ => {}
            }
        } else {
            lines.push(Line::Text(plain(trimmed)));
        }
    }
    lines
}

// ------------------------------------------------------------ the pages

/// Whichever page is at the front of the queue.
///
/// Its own scrim rather than the dialog one, because this is not a dialog: a
/// click beside it does not dismiss it, and nothing behind it is meant to be
/// reachable until it has been answered.
#[component]
pub fn Welcome(greeting: Greeting) -> Element {
    // How wide the window is, used only as a key. See below.
    let (across, _) = use_context::<Signal<crate::state::Viewport>>()();

    rsx! {
        div {
            class: "welcome-scrim",
            oncontextmenu: move |event| event.prevent_default(),
            // Keyed by the width of the window, so that changing the width
            // builds this again rather than stretching what is already here.
            //
            // The renderer breaks a paragraph into lines once and keeps the
            // answer, and it only throws that answer away when the display's
            // *scale* changes, never when the window's *size* does. A window
            // that has been widened since is then drawing text broken for the
            // width it used to be, in a box only tall enough for the lines it
            // used to need, so the last lines of a paragraph fall behind
            // whatever comes after it. Which is what this page looked like:
            // the lead sentence cut off at "help it keep", with a card sitting
            // over the rest of it.
            //
            // A key that moves with the width forces the subtree to be built
            // again, and a fresh subtree gets fresh line breaking. It is a
            // heavier answer than the fault deserves, and it is here rather
            // than at the root because this is a page of wrapped prose, which
            // is where the fault can be seen; a row of buttons and labels does
            // not wrap and never showed it.
            div { key: "w{across:.0}", class: "welcome",
                match greeting {
                    Greeting::Licence => rsx! { LicencePage {} },
                    Greeting::PatchNotes => rsx! { NotesPage {} },
                    Greeting::Support { after_update } => rsx! { SupportPage { after_update } },
                }
            }
        }
    }
}

/// The header every one of these pages wears.
#[component]
fn WelcomeHead(title: String, subtitle: String) -> Element {
    let (logo_w, logo_h) = crate::brand::LOGO_VIEWBOX;
    let palette = use_context::<Signal<AppState>>().read().theme.palette();
    rsx! {
        div { class: "welcome-head",
            div {
                class: "welcome-mark",
                style: "width: 148px; height: {148.0 * logo_h / logo_w}px;",
                dangerous_inner_html: crate::brand::logo(148.0, palette.paint("--ink")),
            }
            div { class: "welcome-heading",
                div { class: "welcome-title", "{title}" }
                div { class: "welcome-sub", "{subtitle}" }
            }
        }
    }
}

/// The licence, in full, once.
///
/// The wording is careful on purpose. Apache-2.0 grants rights; it is not a
/// contract under which the user gives any up, so there is nothing here to
/// agree to and the button does not pretend otherwise.
#[component]
fn LicencePage() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    rsx! {
        WelcomeHead {
            title: "Alterion Open Project".to_string(),
            subtitle: "This software is provided under the Apache License 2.0.".to_string(),
        }
        div { class: "welcome-body",
            p { class: "welcome-lead",
                "The licence below grants you the right to use this software, change it, and pass it on. \
                 It states the conditions attached to doing those things. You are not signing anything \
                 and you are not giving up any rights of your own."
            }
            pre { class: "licence-text", "{LICENCE}" }
        }
        div { class: "welcome-foot",
            span { class: "welcome-note", "Version {RUNNING}" }
            div { class: "grow" }
            button {
                class: "btn primary",
                onclick: move |_| state.write().acknowledge_licence(),
                "I understand"
            }
        }
    }
}

/// What changed since the version this copy last ran as.
#[component]
fn NotesPage() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let quiet = !state.read().patch_notes;
    let lines = notes_for(RUNNING).map(read).unwrap_or_default();

    rsx! {
        WelcomeHead {
            title: format!("What is new in {RUNNING}"),
            subtitle: "Everything that changed in this version.".to_string(),
        }
        div { class: "welcome-body",
            div { class: "notes",
                for (index, line) in lines.into_iter().enumerate() {
                    match line {
                        Line::Heading(text) => rsx! {
                            h3 { key: "note{index}", class: "notes-head", "{text}" }
                        },
                        Line::Bullet { lead, rest, nested } => rsx! {
                            div {
                                key: "note{index}",
                                class: if nested { "notes-bullet nested" } else { "notes-bullet" },
                                span { class: "notes-dot", "\u{2022}" }
                                span {
                                    if !lead.is_empty() {
                                        strong { "{lead} " }
                                    }
                                    "{rest}"
                                }
                            }
                        },
                        Line::Text(text) => rsx! {
                            p { key: "note{index}", class: "notes-text", "{text}" }
                        },
                        Line::Blank => rsx! {},
                    }
                }
            }
        }
        div { class: "welcome-foot",
            crate::backstage::OptCheck {
                label: "Don't show this again".to_string(),
                on_state: quiet,
                on: move |_| {
                    let on = state.read().patch_notes;
                    state.write().patch_notes = !on;
                },
            }
            div { class: "grow" }
            button {
                class: "btn primary",
                onclick: move |_| state.write().greeting_answered(),
                "Continue"
            }
        }
    }
}

/// Two ways to help, both optional, neither of which changes what the
/// application does.
///
/// Nothing here is a trial, a limit or a nag. There is no countdown on the
/// dismissal, the dismissal is a button of the same size as everything else,
/// and no copy on this page suggests that anything works better for somebody
/// who gives than for somebody who does not, because nothing does.
#[component]
fn SupportPage(after_update: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let quiet = !state.read().support_page;
    let coffee = coffee_offered();
    let kofi = kofi_offered();
    let bank = bank_offered();

    rsx! {
        WelcomeHead {
            title: "Supporting development".to_string(),
            subtitle: "Entirely optional, and nothing changes either way.".to_string(),
        }
        div { class: "welcome-body",
            p { class: "welcome-lead",
                "Alterion Open Project is free, and every part of it stays free. Nothing is locked \
                 behind a payment, nothing is limited or slowed down, and there is nothing to buy. \
                 If it has been useful and you would like to help it keep being built, the ways to \
                 do that are below."
            }

            if coffee {
                div { class: "give",
                    div { class: "give-head",
                        {icon("support", 16)}
                        span { "Buy Me a Coffee" }
                    }
                    p { class: "give-note",
                        "Opens in your browser. One off or recurring, whichever suits."
                    }
                    button {
                        class: "btn",
                        onclick: move |_| {
                            // Quiet on failure: a machine with no browser
                            // handler is not something to interrupt anyone
                            // with, and the address is on screen anyway.
                            let _ = crate::cloud::oauth::open_in_browser(COFFEE_URL);
                        },
                        "Open {COFFEE_URL}"
                    }
                }
            }

            if kofi {
                div { class: "give",
                    div { class: "give-head",
                        {icon("report-costs", 16)}
                        span { "Ko-fi" }
                    }
                    p { class: "give-note",
                        "Also opens in your browser. One off, or monthly if you would rather."
                    }
                    button {
                        class: "btn",
                        onclick: move |_| {
                            // Quiet on failure, for the reason the button
                            // above gives.
                            let _ = crate::cloud::oauth::open_in_browser(KOFI_URL);
                        },
                        "Open {KOFI_URL}"
                    }
                }
            }

            if bank {
                div { class: "give",
                    div { class: "give-head",
                        {icon("cost-resource", 16)}
                        span { "Bank transfer" }
                    }
                    p { class: "give-note",
                        "The details below are plain text. Select them, or use Copy."
                    }
                    div { class: "give-details",
                        for (label, value) in bank_rows() {
                            div { key: "{label}", class: "give-row",
                                span { class: "k", "{label}" }
                                span { class: "v", "{value}" }
                                button {
                                    class: "iconbtn",
                                    title: "Copy",
                                    onclick: move |_| crate::controls::copy_to_clipboard(value),
                                    {icon("copy", 14)}
                                }
                            }
                        }
                    }
                }
            }

            if !coffee && !bank {
                // Both constants are still placeholders. Saying so beats an
                // empty panel, and it only ever reaches a build nobody has
                // filled the details into.
                p { class: "welcome-lead",
                    "No donation details have been set for this build, so there is nothing to show here."
                }
            }
        }
        div { class: "welcome-foot",
            if after_update {
                crate::backstage::OptCheck {
                    label: "Don't show this again".to_string(),
                    on_state: quiet,
                    on: move |_| {
                        let on = state.read().support_page;
                        state.write().support_page = !on;
                    },
                }
            }
            div { class: "grow" }
            button {
                class: "btn primary",
                onclick: move |_| state.write().greeting_answered(),
                if after_update { "Continue" } else { "Close" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings file as it stands after a given version has been run.
    fn after_running(version: &str) -> Settings {
        Settings {
            licence_acknowledged: RUNNING.into(),
            licence_acknowledged_at: "2026-08-18T09:14:00Z".into(),
            last_version: version.into(),
            ..Settings::default()
        }
    }

    #[test]
    fn the_licence_is_shown_on_a_first_run_and_not_afterwards() {
        let fresh = Settings::default();
        assert!(on_start(&fresh, RUNNING).contains(&Greeting::Licence));

        let acknowledged = after_running(RUNNING);
        assert!(!on_start(&acknowledged, RUNNING).contains(&Greeting::Licence));
    }

    #[test]
    fn a_first_run_is_not_treated_as_an_update() {
        // Nothing changed for somebody who never had the previous release, so
        // the licence is the only thing they are shown.
        assert_eq!(on_start(&Settings::default(), RUNNING), vec![Greeting::Licence]);
    }

    #[test]
    fn patch_notes_appear_when_the_version_moved_and_not_when_it_did_not() {
        // The section has to exist for there to be anything to show, so the
        // running version's own notes are what this is checked against.
        assert!(notes_for(RUNNING).is_some(), "the changelog has no section for {RUNNING}");

        let updated = on_start(&after_running("0.0.1-nonesuch"), RUNNING);
        assert!(updated.contains(&Greeting::PatchNotes));

        let unchanged = on_start(&after_running(RUNNING), RUNNING);
        assert!(!unchanged.contains(&Greeting::PatchNotes));
        assert!(unchanged.is_empty(), "an ordinary launch shows nothing at all");
    }

    #[test]
    fn the_support_page_appears_once_per_update_and_never_on_an_ordinary_launch() {
        let updated = on_start(&after_running("0.0.1-nonesuch"), RUNNING);
        assert!(updated.contains(&Greeting::Support { after_update: true }));

        // Once the version has been recorded, the same launch repeated shows
        // nothing. That recording is what makes it once per update rather than
        // once per start.
        let again = on_start(&after_running(RUNNING), RUNNING);
        assert!(!again.contains(&Greeting::Support { after_update: true }));
    }

    #[test]
    fn each_dont_show_again_is_honoured_on_its_own() {
        let mut quiet_notes = after_running("0.0.1-nonesuch");
        quiet_notes.patch_notes = false;
        let queue = on_start(&quiet_notes, RUNNING);
        assert!(!queue.contains(&Greeting::PatchNotes));
        assert!(
            queue.contains(&Greeting::Support { after_update: true }),
            "silencing the notes must not silence the ask"
        );

        let mut quiet_support = after_running("0.0.1-nonesuch");
        quiet_support.support_page = false;
        let queue = on_start(&quiet_support, RUNNING);
        assert!(queue.contains(&Greeting::PatchNotes));
        assert!(
            !queue.contains(&Greeting::Support { after_update: true }),
            "silencing the ask must not silence the notes"
        );

        let mut quiet_both = after_running("0.0.1-nonesuch");
        quiet_both.patch_notes = false;
        quiet_both.support_page = false;
        assert!(on_start(&quiet_both, RUNNING).is_empty());
    }

    #[test]
    fn the_licence_shipped_is_the_apache_licence_and_not_a_summary_of_one() {
        assert!(LICENCE.contains("Apache License"));
        assert!(LICENCE.contains("Version 2.0, January 2004"));
        assert!(LICENCE.contains("TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION"));
        // A paraphrase would be a fraction of the length of the real thing.
        assert!(LICENCE.len() > 10_000, "that is not the whole licence");
    }

    #[test]
    fn a_version_that_is_a_prefix_of_another_takes_its_own_section() {
        // "1.0.0" must not be handed the section belonging to "1.0.0-beta".
        assert!(notes_for("1.0.0").is_none());
        assert!(notes_for("1.0.0-beta").is_some());
    }

    #[test]
    fn a_section_stops_at_the_next_release() {
        let notes = notes_for("1.0.0-beta").expect("a section");
        assert!(!notes.contains("## 0.1.0-beta"), "it ran into the next release");
    }

    #[test]
    fn an_unknown_version_has_no_notes_rather_than_all_of_them() {
        assert!(notes_for("9.9.9").is_none());
        assert!(notes_for("").is_none());
    }

    #[test]
    fn a_section_reads_as_headings_and_bullets() {
        let lines = read("### Sharing a plan\n\n- **A log inside the plan.** Every edit.\n  Wrapped on.\nplain words\n");
        assert_eq!(lines[0], Line::Heading("Sharing a plan".into()));
        assert_eq!(lines[1], Line::Blank);
        match &lines[2] {
            Line::Bullet { lead, rest, .. } => {
                assert_eq!(lead, "A log inside the plan.");
                assert_eq!(rest, "Every edit. Wrapped on. plain words");
            }
            other => panic!("expected a bullet, got {other:?}"),
        }
    }

    #[test]
    fn markup_that_would_be_read_out_as_punctuation_is_stripped() {
        assert_eq!(plain("a **bold** and a `code` word"), "a bold and a code word");
    }

    #[test]
    fn an_option_whose_details_are_placeholders_is_not_offered() {
        // The whole reason the constants start as UNSET: an unfilled build
        // shows no option rather than a dead link or a wrong account number.
        assert!(!is_set(UNSET));
        assert!(!is_set("  "));
        assert!(is_set("https://example.test/coffee"));
    }

    #[test]
    fn a_coffee_link_that_is_not_a_web_address_is_not_offered() {
        // `open_in_browser` hands whatever it is given to the desktop, so the
        // check happens here rather than there.
        assert!(!coffee_offered() || COFFEE_URL.starts_with("https://"));
    }

    #[test]
    fn every_bank_detail_shown_has_actually_been_filled_in() {
        for (label, value) in bank_rows() {
            assert!(is_set(value), "{label} is still a placeholder");
        }
    }
}
