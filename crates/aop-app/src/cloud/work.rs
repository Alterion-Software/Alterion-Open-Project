//! The blocking half of collaborating, gathered where it can be run on a
//! worker thread.
//!
//! Everything in here talks to a network, and none of it may be called from
//! the thread drawing the interface. The shape is the same in each case: the
//! session goes in, the answer and the session come back out.
//!
//! The session is moved rather than borrowed on purpose. Asking it for a token
//! can renew it, renewing spends the refresh token, and the server treats a
//! second use of a spent one as a stolen token and revokes the whole account.
//! One session, in one place, doing one thing at a time is what makes that
//! impossible rather than unlikely.

use aop_core::Project;
use aop_core::history::Change;

use crate::cloud::collab::{self, Created, Fetched, Pushed, Standing};
use crate::cloud::{Session, SignInError};

/// Everything a push needs, gathered on the interface thread before the work
/// is handed over.
#[derive(Debug, Clone)]
pub struct Offer {
    pub server: String,
    pub project: String,
    /// The server's cursor, not the plan's own change id. The two are
    /// different numbers and confusing them is how a sync asks for the wrong
    /// changes.
    pub after: i64,
    pub changes: Vec<Change>,
    /// The plan as it stands, for the case where the server asks for a fresh
    /// whole copy of it.
    pub plan: Project,
}

/// A session, and whatever the work came back with.
///
/// The session is first because it always comes back, whether or not the work
/// did: losing it would sign the planner out over a server that was briefly
/// unreachable.
pub type Handed<T> = (Session, Result<T, String>);

/// Get a token that will still be good when the call lands.
///
/// Renewing here rather than at the call site is the whole reason these
/// functions take the session: `access_token` can go to the network, so it
/// belongs on this side of the thread boundary too.
fn token_for(session: &mut Session) -> Result<String, String> {
    session
        .access_token()
        .map(str::to_string)
        .map_err(|error: SignInError| error.to_string())
}

/// Offer this plan's unsent work to the server.
pub fn sync(mut session: Session, offer: Offer) -> Handed<Pushed> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };

    let pushed = collab::push(
        &offer.server,
        &token,
        &offer.project,
        offer.after,
        &offer.changes,
    );

    // The server asking for a fresh whole plan is housekeeping: it stores
    // commands and has no engine to replay them with, so it cannot fold its
    // own log into a plan and asks whoever pushes next. There is no decision
    // in it for a planner, so it is answered here and not carried back.
    if let Ok(Pushed::Applied {
        head,
        snapshot_wanted: true,
        ..
    }) = &pushed
    {
        let _ = collab::put_snapshot(&offer.server, &token, &offer.project, *head, &offer.plan);
    }

    let outcome = pushed.map_err(|error| error.to_string());
    (session, outcome)
}

/// Fetch a whole plan, for when replaying is no longer possible.
pub fn fetch(mut session: Session, server: String, project: String) -> Handed<Fetched> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };
    let outcome = collab::snapshot(&server, &token, &project).map_err(|error| error.to_string());
    (session, outcome)
}

/// Fetch the plan a link names, claiming an invitation if that is what is in
/// the way.
///
/// A link admits nobody by itself, and it never did: it names a server and a
/// plan, and the server decides. What has changed is that there is now
/// something the server can decide in the opener's favour. So this is two
/// calls where it used to be one:
///
/// ```text
///   GET  snapshot          -> the plan, if this account already has it
///        |
///        +-- not found     -> POST claim, in case an invitation is waiting
///                             for the address this account signs in with
///                  |
///                  +-- joined  -> GET snapshot again, and this time it works
///                  +-- nothing -> say which address would have to be invited
///                  +-- outage  -> say that nobody could check, which is not
///                                 the same as saying no
/// ```
///
/// The claim is made only after the plain fetch has failed. Making it first
/// would put a round trip to the identity provider in front of every link
/// opened by somebody who was already a member, for no gain to anybody.
///
/// The server answers a non-member exactly as it answers a plan that is not
/// there, and that is on purpose: telling them apart would confirm which ids
/// are real to anybody who tried a few. So the message covers both, because
/// both are true as far as anybody here can tell.
pub fn open(mut session: Session, server: String, project: String) -> Handed<Fetched> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };
    // The address the invitation would have to name. It is this account's own,
    // read from the sign in rather than asked for, and saying it is the whole
    // difference between "you are not a member" and a message somebody can act
    // on without a second conversation.
    let signs_in_as = session.account().email.clone();

    let outcome = match collab::snapshot(&server, &token, &project) {
        Ok(plan) => Ok(plan),
        Err(collab::CollabError::NoSuchProject | collab::CollabError::NotAllowed) => {
            claim_then_open(&server, &token, &project, &signs_in_as)
        }
        Err(other) => Err(other.to_string()),
    };
    (session, outcome)
}

/// Present this account for a plan it could not open, and try again if that
/// changed anything.
fn claim_then_open(
    server: &str,
    token: &str,
    project: &str,
    signs_in_as: &str,
) -> Result<Fetched, String> {
    let address = match signs_in_as.trim() {
        "" => "the address this account signs in with".to_string(),
        address => address.to_string(),
    };

    match collab::claim(server, token, project) {
        Ok(collab::Claimed::Joined(_) | collab::Claimed::Already(_)) => {
            collab::snapshot(server, token, project).map_err(|error| error.to_string())
        }
        Ok(collab::Claimed::NoInvitation) => Err(format!(
            "The server would not give you that plan, and there is no invitation waiting \
             for {address}. Either the plan is not there or it has not been shared with \
             you: the server answers the same way for both, on purpose, so that plan ids \
             cannot be guessed at. Ask whoever sent you the link to invite {address}, \
             then open the link again."
        )),
        Ok(collab::Claimed::CouldNotCheck(why)) => Err(format!(
            "The server could not ask your identity provider which address this account \
             uses, so it cannot tell whether you have been invited. This is not a refusal \
             and it is not an answer either: try again in a moment. If it keeps happening, \
             whoever runs the server needs to see this: {why}"
        )),
        Ok(collab::Claimed::NotConfirmed(why)) => Err(format!(
            "The server would not give you that plan, and it will not check for an \
             invitation either: {why}. Nothing about this is to do with the plan or the \
             server address, and there is nothing to change here. Confirm {address} with \
             whoever runs your sign in, then open the link again."
        )),
        Err(error) => Err(error.to_string()),
    }
}

/// Who a plan is shared with.
pub fn sharing(mut session: Session, server: String, project: String) -> Handed<collab::Sharing> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };
    let outcome = collab::sharing(&server, &token, &project).map_err(|error| error.to_string());
    (session, outcome)
}

/// Everything a change to the sharing needs, gathered before the work is
/// handed over.
///
/// One type for four verbs, because the four differ only in which call is made
/// and every one of them ends the same way: read the list back, so that what
/// is on screen is what the server holds rather than what this copy assumed it
/// would now hold.
#[derive(Debug, Clone)]
pub enum Adjust {
    Invite { email: String, role: String },
    CancelInvite { email: String },
    Remove { subject: String },
}

/// Make one change to who a plan is shared with, then read the list back.
///
/// The read is part of the same job rather than a second press. A membership
/// list that shows the change this copy asked for, rather than the change the
/// server made, is one that disagrees with the server the first time two
/// people manage a plan at once.
pub fn adjust(
    mut session: Session,
    server: String,
    project: String,
    adjust: Adjust,
) -> Handed<collab::Sharing> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };

    let made = match &adjust {
        Adjust::Invite { email, role } => collab::invite(&server, &token, &project, email, role),
        Adjust::CancelInvite { email } => collab::cancel_invite(&server, &token, &project, email),
        Adjust::Remove { subject } => collab::remove_member(&server, &token, &project, subject),
    };
    if let Err(error) = made {
        return (session, Err(error.to_string()));
    }

    let outcome = collab::sharing(&server, &token, &project).map_err(|error| error.to_string());
    (session, outcome)
}

/// Put this plan on the server for the first time.
pub fn publish(
    mut session: Session,
    server: String,
    name: String,
    plan: Project,
) -> Handed<Created> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };
    let outcome = collab::create(&server, &token, &name, &plan).map_err(|error| error.to_string());
    (session, outcome)
}

/// Ask the server where this plan has got to.
///
/// Asked rather than assumed: "nothing has changed here since I last pushed"
/// is a different claim from "the server agrees this is the latest version",
/// and only one of them is worth showing a planner.
pub fn standing(mut session: Session, server: String, project: String) -> Handed<Standing> {
    let token = match token_for(&mut session) {
        Ok(token) => token,
        Err(why) => return (session, Err(why)),
    };
    let outcome = collab::about(&server, &token, &project).map_err(|error| error.to_string());
    (session, outcome)
}

/// A token to open the live socket with.
///
/// Its own job because opening the socket happens on yet another thread, and
/// the session must not travel there: it would be two places at once, each
/// able to spend the other's refresh token.
pub fn live_token(mut session: Session) -> Handed<String> {
    let outcome = token_for(&mut session);
    (session, outcome)
}

/// Read the account's details again, for when they may have been changed.
///
/// Its own job rather than part of something else, because it is the one call
/// here that nobody is waiting for: it happens after somebody comes back from
/// the provider's account page, and if it fails the card goes on saying what
/// it already said.
pub fn refresh_account(mut session: Session) -> Handed<crate::cloud::Account> {
    let outcome = session.refresh_account().map_err(|error| error.to_string());
    (session, outcome)
}

/// Sign out, and forget the session either way.
///
/// Returns only what to say, because there is nothing to hand back: a person
/// who pressed Sign out is signed out whatever the server managed.
pub fn sign_out(session: Session) -> Result<(), String> {
    crate::cloud::sign_out(&session).map_err(|error| error.to_string())
}
