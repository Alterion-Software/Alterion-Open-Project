//! The versions a plan can be put back to, and the view that shows them.
//!
//! The store lives beside the settings rather than beside the user's plans,
//! for the same reason the recovery snapshots do: a folder of versions turning
//! up next to somebody's documents is a surprise, and a plan sent to someone
//! else should not carry twenty copies of itself.
//!
//! Named by a digest of where the plan lives, because a file name is not
//! unique and a full path is not a file name. A plan that has never been saved
//! has nowhere to key off, so its versions are kept for the session and no
//! longer; the view says so rather than letting somebody count on them.

use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use aop_core::compare::summarise;
use aop_core::versions::{KEEP, Versions};

use crate::icons::icon;
use crate::state::{AppState, CheckOutcome, Dialog};

/// Where the versions live.
fn directory() -> Option<PathBuf> {
    crate::settings::config_root().map(|dir| dir.join("versions"))
}

/// The file one plan's versions are kept in.
fn file_for(plan: &Path) -> Option<PathBuf> {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(plan.to_string_lossy().as_bytes());
    let name = crate::cloud::tokens::to_hex(&digest);
    Some(directory()?.join(format!("{name}.json")))
}

/// Read what is kept for this plan.
///
/// A store that will not parse is treated as no store rather than as an error.
/// It is a convenience built on top of the plan, and refusing to open a file
/// because its version history is unreadable would be a poor trade.
pub fn read(plan: Option<&Path>) -> Versions {
    let Some(path) = plan.and_then(file_for) else {
        return Versions::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Write what is kept for this plan.
///
/// Quiet on failure. This runs whenever a version is taken, which includes the
/// moment just before a rebase, and a dialog about an unwritable directory in
/// the middle of that would interrupt the thing it exists to protect.
pub fn write(plan: Option<&Path>, versions: &Versions) {
    let Some(path) = plan.and_then(file_for) else {
        return;
    };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(text) = serde_json::to_string(versions) {
        let _ = std::fs::write(path, text);
    }
}

// ------------------------------------------------------------------- the view

/// Where this plan has been, and where it is.
///
/// Two halves of one question: the versions it can be put back to, and what
/// the server says about the copy on screen. Somebody asking "is this the
/// current plan?" is asking both at once.
///
/// A panel beside the plan rather than a view instead of it. Every question it
/// answers is asked *about what is on screen*: whether this copy is current,
/// what somebody else changed, which version to go back to. A view would take
/// the thing being asked about off the screen to answer.
#[component]
pub fn HistoryAndSync() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let kept = state.read().versions.len();
    rsx! {
        aside { class: "spell-panel sync-side",
            div { class: "spell-panel-head",
                span { class: "spell-panel-title", "History and Sync" }
                span { class: "sync-side-count", "{kept} version(s) kept" }
                button {
                    class: "dlg-close",
                    title: "Close",
                    onclick: move |_| state.write().sync_open = false,
                    "\u{2715}"
                }
            }
            div { class: "sync-view",
                SyncStanding {}
                VersionList {}
            }
        }
    }
}

#[component]
fn SyncStanding() -> Element {
    let state = use_context::<Signal<AppState>>();

    let (server, link, checked, waiting, unsent, message, peers, live) = {
        let s = state.read();
        (
            s.collaborate_server.clone(),
            s.link.clone(),
            s.checked.clone(),
            s.working,
            s.project.history.unsent().len(),
            s.cloud_message.clone(),
            // Named rather than counted. "2 other people" says nothing about
            // whether the person whose row keeps moving is the one you were
            // expecting.
            s.peers
                .iter()
                .map(|peer| match peer.name.trim() {
                    "" => "Someone".to_string(),
                    name => name.to_string(),
                })
                .collect::<Vec<String>>(),
            s.live.is_some(),
        )
    };

    let blocked = state.read().sync_blocked();

    rsx! {
        div { class: "sync-panel",
            h2 { class: "opt-head", "Sync" }

            match &link {
                Some(link) => rsx! {
                    div { class: "sync-row",
                        span { class: "sync-key", "On the server" }
                        span { class: "sync-value", "{server}" }
                    }
                    div { class: "sync-row",
                        span { class: "sync-key", "As plan" }
                        span { class: "sync-value mono", "{link.project}" }
                    }
                    div { class: "sync-row",
                        span { class: "sync-key", "Read up to" }
                        span { class: "sync-value", "change {link.cursor} on the server" }
                    }
                },
                None => rsx! {
                    div { class: "sync-row",
                        span { class: "sync-key", "On the server" }
                        span { class: "sync-value",
                            "No. This plan is on this machine only, so nothing here is shared."
                        }
                    }
                },
            }

            div { class: "sync-row",
                span { class: "sync-key", "Waiting to go" }
                span { class: "sync-value",
                    match unsent {
                        0 => "Nothing. Every change here has been sent.".to_string(),
                        1 => "1 change.".to_string(),
                        many => format!("{many} changes."),
                    }
                }
            }

            // The distinction the whole panel turns on: what the server was
            // asked, and when. A tick that means "nothing has happened here
            // since I last pushed" is a different claim and is not made.
            div { class: "sync-row",
                span { class: "sync-key", "Checked with the server" }
                match &checked {
                    None => rsx! {
                        span { class: "sync-value warn",
                            "Not checked. Nothing here has asked the server, so whether this is \
                             the latest version is not known."
                        }
                    },
                    Some(checked) => {
                        let when = checked.at.format("%Y-%m-%d %H:%M").to_string();
                        match &checked.outcome {
                            CheckOutcome::Current => rsx! {
                                span { class: "sync-value good",
                                    "The server agreed this is the latest version, asked at {when}."
                                }
                            },
                            CheckOutcome::Behind { by } => rsx! {
                                span { class: "sync-value warn",
                                    "The server has {by} change(s) this copy has not read, as of {when}."
                                }
                            },
                            CheckOutcome::Failed(why) => rsx! {
                                span { class: "sync-value bad",
                                    "The check could not be made at {when}. {why}"
                                }
                            },
                        }
                    }
                }
            }

            if live {
                div { class: "sync-row",
                    span { class: "sync-key", "Live editing" }
                    span { class: "sync-value good",
                        match peers.len() {
                            0 => "On. Nobody else has this plan open.".to_string(),
                            _ => format!("On. Also here: {}.", peers.join(", ")),
                        }
                    }
                }
            }

            if let Some(waiting) = waiting {
                p { class: "opt-note", "{waiting.waiting()}" }
            } else if let Some(message) = message {
                p { class: "opt-note", "{message}" }
            }

            div { class: "sync-actions",
                match &blocked {
                    Some(why) => rsx! {
                        button { class: "btn", disabled: true, "Sync now" }
                        span { class: "sync-why", "{why}" }
                    },
                    None => rsx! {
                        button {
                            class: "btn primary",
                            onclick: move |_| crate::collaborate::sync(state),
                            "Sync now"
                        }
                        // Asking to see what is there, which is not the same
                        // as offering what is here. The preview it opens is
                        // the one a refused push opens, and so is the rebase
                        // behind it.
                        button {
                            class: "btn",
                            onclick: move |_| crate::collaborate::pull(state),
                            "Pull changes"
                        }
                        button {
                            class: "btn",
                            onclick: move |_| crate::collaborate::check(state),
                            "Check with the server"
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn VersionList() -> Element {
    let mut state = use_context::<Signal<AppState>>();

    let (rows, dropped, selected, kept_only_for_now) = {
        let s = state.read();
        let rows: Vec<(usize, String, String, &'static str, &'static str)> = s
            .versions
            .all()
            .iter()
            .enumerate()
            .map(|(index, snapshot)| {
                (
                    index,
                    snapshot.at.format("%Y-%m-%d").to_string(),
                    snapshot.at.format("%H:%M").to_string(),
                    snapshot.taken.label(),
                    snapshot.taken.describe(),
                )
            })
            .collect();
        (
            rows,
            s.versions.dropped(),
            s.version_selected,
            s.file_path.is_none(),
        )
    };

    // Worked out here rather than in the row, so the whole plan is compared
    // once per render instead of once per version on screen.
    let difference = selected.map(|index| {
        let s = state.read();
        (
            summarise(&s.versions.changed_after(index, &s.project)).sentence(),
            s.versions.compared_with(index),
        )
    });

    rsx! {
        div { class: "sync-panel",
            h2 { class: "opt-head", "History" }
            p { class: "hint",
                "A version is kept every time this plan is saved, and again just before other \
                 people's changes are brought in. The second one is the point: a sync is the \
                 only moment your own work is replayed on top of somebody else's, so there is \
                 always something to come back to."
            }

            if kept_only_for_now {
                p { class: "opt-note",
                    "This plan has never been saved to a file, so its versions are kept only \
                     while it is open. Save it and they will be kept between runs."
                }
            }

            if rows.is_empty() {
                p { class: "hint", "No versions yet. Saving this plan takes the first one." }
            } else {
                table { class: "assign-table", style: "margin-top: 12px;",
                    thead {
                        tr {
                            th { style: "width: 108px;", "Date" }
                            th { style: "width: 64px;", "Time" }
                            th { style: "width: 132px;", "Who" }
                            th { "Why it was kept" }
                        }
                    }
                    tbody {
                        // Newest first, which is the order somebody looking for
                        // "the one before this went wrong" reads in.
                        for (index, date, time, label, describe) in rows.iter().rev() {
                            {
                                let index = *index;
                                let author = state
                                    .read()
                                    .versions
                                    .get(index)
                                    .map(|snapshot| snapshot.author.clone())
                                    .unwrap_or_default();
                                let class = if selected == Some(index) {
                                    "ver-row selected"
                                } else {
                                    "ver-row"
                                };
                                rsx! {
                                    tr { key: "v{index}", class: "{class}",
                                        onclick: move |_| {
                                            let already = state.read().version_selected == Some(index);
                                            state.write().version_selected =
                                                if already { None } else { Some(index) };
                                        },
                                        td { "{date}" }
                                        td { "{time}" }
                                        td { "{author}" }
                                        td {
                                            span { class: "ver-why", "{label}" }
                                            span { class: "ver-note", " {describe}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if dropped > 0 {
                    p { class: "hint",
                        "{dropped} older version(s) have been dropped. A version is a whole copy \
                         of the plan, so at most {KEEP} are kept."
                    }
                }

                if let Some((sentence, against)) = difference {
                    div { class: "ver-diff",
                        div { class: "ver-diff-head",
                            {icon("compare", 15)}
                            span { "Compared with {against}" }
                        }
                        p { "{sentence}" }
                        button {
                            class: "btn",
                            onclick: move |_| {
                                let index = state.read().version_selected;
                                if let Some(index) = index {
                                    state.write().dialog = Some(Dialog::RestoreVersion(index));
                                }
                            },
                            "Go back to this version..."
                        }
                    }
                }
            }
        }
    }
}
