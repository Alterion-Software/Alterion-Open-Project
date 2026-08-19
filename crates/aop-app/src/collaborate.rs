//! Starting work that needs a network, and taking the answer back.
//!
//! Everything collaborating does blocks: signing in waits for a person in a
//! browser, a sync waits for a server, and a first snapshot of a large plan is
//! a real amount of JSON. None of it may happen where the interface runs, so
//! each of these does the same three things:
//!
//! ```text
//!   1  gather what the work needs, and say what is being waited for
//!   2  hand it to a thread
//!   3  take the answer back where the plan can be written
//! ```
//!
//! The session goes with the work rather than being borrowed by it. There is
//! only ever one, in one place: the server spends a refresh token the moment
//! it is used and treats a second use of a spent one as a stolen token, so two
//! copies renewing at once would revoke the account they share.

use dioxus::prelude::*;

use crate::cloud;
use crate::state::{AppState, Dialog, Working};

pub fn sign_in(mut state: Signal<AppState>) {
    let Some((issuer, client_id)) = state.write().start_sign_in() else {
        return;
    };
    cloud::off_thread(
        move || cloud::sign_in(&issuer, &client_id),
        move |outcome| match outcome {
            Some(outcome) => state.write().sign_in_landed(outcome),
            None => state.write().worker_lost("Signing in"),
        },
    );
}

pub fn sign_out(mut state: Signal<AppState>) {
    let Some(session) = state.write().hand_over(Working::SigningOut) else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::sign_out(session),
        move |outcome| match outcome {
            Some(outcome) => state.write().sign_out_landed(outcome),
            // The session went with the worker, so signing out is what this
            // amounts to anyway. Saying so beats leaving a button spinning.
            None => state.write().sign_out_landed(Err(
                "Signing out stopped unexpectedly. This machine is signed out; \
                 sign out from your account page as well if it is shared."
                    .into(),
            )),
        },
    );
}

pub fn sync(mut state: Signal<AppState>) {
    let Some((session, offer)) = state.write().start_sync() else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::sync(session, offer),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.sync_landed(outcome);
            }
            None => state.write().worker_lost("The sync"),
        },
    );
}

/// Ask for what is on the server, see what it would do, and decide.
///
/// A pull rather than a sync: nothing here is offered, and that is the
/// difference. It reuses the push with an empty batch, because an empty push
/// is already how a client asks "am I still current?", and the answer to a
/// client that is not carries exactly what it missed. So the preview it opens
/// is [`Dialog::SyncBehind`], which is the same preview and the same rebase
/// used everywhere else.
///
/// **Why this always shows the preview**, when a rebase that arrives on the
/// live socket does not: asking for it is asking to see it. Somebody who has
/// had live editing off, or who has been away, presses this precisely because
/// they want to know what happened before it lands on their plan. A change
/// arriving on a socket was not asked for and interrupting typing to announce
/// it, several times a minute, is worse than not showing it at all.
pub fn pull(mut state: Signal<AppState>) {
    let Some((session, offer)) = state.write().start_pull() else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::sync(session, offer),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.sync_landed(outcome);
            }
            None => state.write().worker_lost("The pull"),
        },
    );
}

/// Give the server the fresh whole plan it asked for.
///
/// Housekeeping, so nothing waits on it and nothing is said about it. The
/// server stores commands and cannot fold its own log into a plan, so it asks
/// whoever it is talking to; before edits streamed, that ask was answered by
/// whoever next pressed Sync, and with streaming nobody presses Sync for
/// hours.
pub fn send_snapshot(mut state: Signal<AppState>) {
    let Some((session, server, project, head, plan)) = state.write().start_snapshot() else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::snapshot(session, server, project, head, plan),
        move |done| match done {
            Some((session, _)) => {
                let mut writer = state.write();
                writer.working = None;
                writer.hand_back(session);
            }
            None => state.write().worker_lost("Sending a copy of the plan"),
        },
    );
}

/// Ask the server where this plan has got to, rather than assuming.
pub fn check(mut state: Signal<AppState>) {
    let (server, project) = {
        let s = state.read();
        match (&s.link, s.sync_blocked()) {
            (Some(link), None) => (s.collaborate_server.trim().to_string(), link.project.clone()),
            _ => return,
        }
    };
    let Some(session) = state.write().hand_over(Working::Checking) else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::standing(session, server, project),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.standing_landed(outcome);
            }
            None => state.write().worker_lost("The check"),
        },
    );
}

/// Put this plan on the server for the first time.
pub fn publish(mut state: Signal<AppState>) {
    let (server, name, plan) = {
        let s = state.read();
        if s.publish_blocked().is_some() {
            return;
        }
        (
            s.collaborate_server.trim().to_string(),
            s.project.name.clone(),
            s.project.clone(),
        )
    };
    let Some(session) = state.write().hand_over(Working::Publishing) else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::publish(session, server, name, plan),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.publish_landed(outcome);
            }
            None => state.write().worker_lost("Putting this plan on the server"),
        },
    );
}

/// Take a whole plan from the server, for when replaying is no longer possible.
pub fn fresh_copy(mut state: Signal<AppState>) {
    let (server, project) = {
        let s = state.read();
        match &s.link {
            Some(link) => (s.collaborate_server.trim().to_string(), link.project.clone()),
            None => return,
        }
    };
    let Some(session) = state.write().hand_over(Working::Fetching) else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::fetch(session, server, project),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.fresh_copy_landed(outcome);
            }
            None => state.write().worker_lost("Fetching a fresh copy"),
        },
    );
}

/// Turn live editing on, or off.
///
/// Turning it on needs a token, and asking for one can renew the session, so
/// even this goes through a worker. The socket then runs on a thread of its
/// own; the session never goes near it.
pub fn live(mut state: Signal<AppState>, on: bool) {
    if !on {
        state.write().stop_live(Some("Live editing is off.".into()));
        return;
    }
    if state.read().sync_blocked().is_some() {
        return;
    }
    let Some(session) = state.write().hand_over(Working::Checking) else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::live_token(session),
        move |done| match done {
            Some((session, Ok(token))) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.start_live(token);
            }
            Some((session, Err(why))) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.cloud_message = Some(why);
            }
            None => state.write().worker_lost("Starting live editing"),
        },
    );
}

/// Fetch the plan a link names, and open it.
///
/// The server comes from the link rather than from the settings, and that is
/// the point of the feature: this is how a plan reaches a second machine. It
/// is also why nothing gets here without somebody having read the address on
/// the dialog first.
pub fn open_link(mut state: Signal<AppState>, share: cloud::share::Share) {
    // The copy on this machine first. It carries the plan, the change log and
    // anything that never reached the server, and the cursor beside it turns
    // what happened since into a handful of entries rather than a download.
    // That is what the cursor is for, and it is the difference between
    // opening instantly and waiting on a whole plan.
    if state.write().open_local_copy(share.server.clone(), share.project.clone()) {
        pull(state);
        return;
    }
    let Some(session) = state.write().hand_over(Working::Fetching) else {
        return;
    };
    let (server, project) = (share.server.clone(), share.project.clone());
    let asked = (server.clone(), project.clone());
    cloud::off_thread(
        move || cloud::work::open(session, server, project),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.open_link_landed(asked.0, asked.1, outcome);
            }
            None => state.write().worker_lost("Opening the plan from that link"),
        },
    );
}

/// Read the account's details again, quietly.
///
/// Called when this window gets the focus back after somebody was sent to the
/// provider's account page, which is the only moment a change is likely.
/// Nothing waits for it and nothing is said if it fails: the card goes on
/// showing what it already had, which is better than a dialog about a picture.
pub fn refresh_account(mut state: Signal<AppState>) {
    let Some(session) = state.write().hand_over(Working::ReadingAccount) else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::refresh_account(session),
        move |done| match done {
            // `hand_back` puts the account on the card, so a refreshed name or
            // a new picture lands with the session and there is nothing else
            // to do about it here.
            Some((session, _)) => state.write().hand_back(session),
            None => state.write().worker_lost("Reading your account details"),
        },
    );
}

/// Start live editing and put the link to this plan on the clipboard.
///
/// One action rather than three, because inviting somebody is one intention:
/// turn it on, find the link, copy the link. Pressed again while a session is
/// already running it copies the link again rather than tearing the session
/// down, since a control that starts something on the first press and stops it
/// on the second, while also being how a link is copied, is one nobody can
/// predict. What it will do next is written on the button.
///
/// **What the link can and cannot do.** It names a server and a plan, and it
/// admits nobody by itself. What lets somebody in is an invitation to their
/// email address, which the owner sends from Options; the link is then how
/// their copy finds the plan the invitation is for. Both halves are needed and
/// neither is enough, so the message says to do the other one.
pub fn share(mut state: Signal<AppState>) {
    let (link, already) = {
        let s = state.read();
        let link = s
            .link
            .as_ref()
            .and_then(|link| cloud::share::write(s.collaborate_server.trim(), &link.project));
        (link, s.live.is_some())
    };

    let Some(link) = link else {
        // Nothing is started and nothing is copied. A link to a plan that is
        // not on a server is a string that looks like it works.
        state.write().cloud_message = Some(
            "This plan is not on a server yet, so there is no link to it. Put it on the \
             server first, from File and then Options, under Alterion Collaborate."
                .into(),
        );
        return;
    };

    crate::controls::copy_to_clipboard(&link);
    {
        let mut writer = state.write();
        writer.status = if already {
            "Link copied".into()
        } else {
            "Link copied, and live editing is starting".into()
        };
        writer.cloud_message = Some(format!(
            "{link}\n\nThe link is on the clipboard. It works straight away for you, on your \
             other machines, and for anybody already in the plan. For anybody else, invite \
             their email address first: File, then Options, then Alterion Collaborate, under \
             Shared with. Their copy claims the invitation the first time they open the link, \
             so send both and they need do nothing else."
        ));
    }

    if !already {
        live(state, true);
    }
}

/// Take somebody else's work, then offer this planner's again.
///
/// The second push is not optional: the whole point of a rebase is that the
/// work sitting here still gets to the server, and leaving it for somebody to
/// press Sync a second time is how it gets forgotten.
pub fn accept_incoming(
    mut state: Signal<AppState>,
    head: i64,
    differences: Vec<aop_core::compare::Difference>,
    changes: Vec<aop_core::history::Change>,
    replayed: usize,
    asked: usize,
) {
    let brought = state
        .write()
        .accept_incoming(head, &differences, changes, replayed, asked);
    if brought.is_clean() {
        sync(state);
    }
}

/// Ask the server who this plan is shared with.
///
/// Read on demand and not kept fresh. Somebody else can be added or removed by
/// the owner on another machine, and there is nothing here that would hear
/// about it, so a list that refreshed itself would only be wrong less
/// obviously.
pub fn sharing(mut state: Signal<AppState>) {
    let Some((session, server, project)) = state.write().start_sharing(Working::ReadingSharing)
    else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::sharing(session, server, project),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.sharing_landed(outcome);
            }
            None => state.write().worker_lost("Reading who this plan is shared with"),
        },
    );
}

/// Make one change to who a plan is shared with, and read the list back.
///
/// One function for the three verbs because they differ only in what is sent:
/// each of them ends by asking the server what the sharing is now, rather than
/// by assuming that what was asked for is what happened.
fn adjust(mut state: Signal<AppState>, adjust: cloud::work::Adjust, said: String) {
    let Some((session, server, project)) = state.write().start_sharing(Working::ChangingSharing)
    else {
        return;
    };
    cloud::off_thread(
        move || cloud::work::adjust(session, server, project, adjust),
        move |done| match done {
            Some((session, outcome)) => {
                let mut writer = state.write();
                writer.hand_back(session);
                writer.sharing_changed(said, outcome);
            }
            None => state.write().worker_lost("Changing who this plan is shared with"),
        },
    );
}

/// Invite an address to this plan.
///
/// Nothing here or on the server looks the address up. Nobody is told whether
/// it belongs to an account, because nobody asked: an invitation is a note
/// left for whoever proves that address is theirs, and until somebody does,
/// this application knows no more about them than what was typed.
pub fn invite(mut state: Signal<AppState>) {
    let ready = state.read().invite_ready();
    let (email, role) = match ready {
        Ok(ready) => ready,
        Err(why) => {
            state.write().sharing_message = Some(why);
            return;
        }
    };
    let said = format!(
        "Invited {email} as a {role}. They join the plan the first time they open it while \
         signed in with that address."
    );
    adjust(state, cloud::work::Adjust::Invite { email, role }, said);
}

/// Withdraw an invitation that has not been taken up.
pub fn cancel_invite(state: Signal<AppState>, email: String) {
    let said = format!("The invitation to {email} has been withdrawn.");
    adjust(state, cloud::work::Adjust::CancelInvite { email }, said);
}

/// Take somebody out of this plan.
///
/// Nothing is done to the copy on their machine, and nothing could be: they
/// hold a whole plan and there is no reach into it from here. What changes is
/// that their next sync is answered the way any plan that is not theirs is,
/// and their copy already says exactly that.
pub fn remove_member(state: Signal<AppState>, subject: String, who: String) {
    let said = format!(
        "{who} has been taken out of this plan. The copy on their machine is untouched and \
         still theirs to open; it simply stops syncing with this one."
    );
    adjust(state, cloud::work::Adjust::Remove { subject }, said);
}

/// Check the server and the sign in, one question at a time.
///
/// Unauthenticated where it can be, which is the point: the health endpoint
/// still answers when signing in is the thing that is broken.
pub fn health(mut state: Signal<AppState>) {
    if state.read().working.is_some() {
        return;
    }
    let (server, issuer) = {
        let s = state.read();
        (s.collaborate_server.clone(), s.idp_issuer.clone())
    };
    {
        let mut writer = state.write();
        writer.health.clear();
        writer.working = Some(Working::RunningHealthCheck);
        writer.dialog = Some(Dialog::HealthCheck);
    }
    cloud::off_thread(
        move || cloud::health::run(&server, &issuer),
        move |checks| {
            let mut writer = state.write();
            writer.working = None;
            writer.health = checks.unwrap_or_default();
        },
    );
}
