//! Talking to an Alterion Collaborate server.
//!
//! The server keeps the authored change log and hands out what a client has
//! not seen. It never merges and never replays, so everything interesting
//! happens on this side of the wire; what is here is the four answers a push
//! can get and the two ways of getting a whole plan.
//!
//! ```text
//!   POST /api/projects/{id}/changes   after = N, here is my unsent work
//!        |
//!        +-- 200 applied   the log was at N, mine are N+1 onwards
//!        +-- 409 behind    the log has moved, and what I missed came back
//!        |                 in the same response
//!        +-- 409 gap       what I missed is no longer kept, take a snapshot
//!        +-- 409 ahead     my cursor is past the server's head, so this is
//!                          not the same log and pushing would interleave two
//! ```
//!
//! Every call here blocks. They are called from a worker, never from the
//! thread drawing the interface.

use std::time::Duration;

use aop_core::Project;
use aop_core::history::Change;
use serde::{Deserialize, Serialize};

use crate::cloud::oauth::describe;

/// How long to wait on the server before giving up.
///
/// Longer than the identity provider's, because a first snapshot of a large
/// plan is a real amount of JSON and a planner would rather wait for it than
/// be told it timed out.
const REQUEST_TIMEOUT_SECONDS: u64 = 60;

/// Everything that can stop a call to the server, phrased as the thing to do
/// about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabError {
    /// No server address is set.
    NoServer,
    /// The address is not one this application will send a token to.
    NotEncrypted(String),
    /// Nothing usable answered.
    NotReached { server: String, why: String },
    /// The server would not accept the token.
    NotSignedIn,
    /// A real account that is not allowed to touch this plan.
    NotAllowed,
    /// The plan is not on this server, under this account, any more.
    NoSuchProject,
    /// The server refused the request and said why.
    Refused(String),
    /// The server answered with something this build cannot read.
    Unreadable(String),
}

impl std::fmt::Display for CollabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollabError::NoServer => write!(
                f,
                "No Collaborate server address is set, so there is nowhere to sync to. \
                 Fill in the server address in Options, under Alterion Collaborate."
            ),
            CollabError::NotEncrypted(server) => write!(
                f,
                "The server address {server} is not an encrypted one, and this would send \
                 your sign in token over it. Use an https address, or run the server on \
                 this machine at localhost. Check the address in Options."
            ),
            CollabError::NotReached { server, why } => write!(
                f,
                "The Collaborate server could not be reached: {why}. \
                 Check the server address in Options; it is currently {server}."
            ),
            CollabError::NotSignedIn => write!(
                f,
                "The server would not accept this sign in. Sign out and sign in again \
                 from Options, under Alterion Collaborate."
            ),
            CollabError::NotAllowed => write!(
                f,
                "This account is not allowed to change this plan. \
                 Ask whoever shared it with you for permission to edit."
            ),
            CollabError::NoSuchProject => write!(
                f,
                "This plan is no longer on that server, or is no longer shared with this \
                 account. Ask whoever owns it to share it again, or unlink this plan and \
                 put a fresh copy on the server."
            ),
            CollabError::Refused(why) => write!(
                f,
                "The server refused the request: {why}. Try again, and check the server \
                 address in Options if it keeps happening."
            ),
            CollabError::Unreadable(why) => write!(
                f,
                "The server's answer could not be read: {why}. \
                 The server is probably a newer version than this copy. Check for an update."
            ),
        }
    }
}

impl std::error::Error for CollabError {}

/// What a push was told.
#[derive(Debug, Clone, PartialEq)]
pub enum Pushed {
    /// The work went. Each local change id was given a seq.
    Applied {
        head: i64,
        applied: Vec<(u64, i64)>,
        /// The server asking for a fresh whole plan, because its log has run
        /// far enough past the newest stored one that a first sync would mean
        /// replaying thousands of commands.
        snapshot_wanted: bool,
    },
    /// Somebody pushed first, and what was missed came back with the refusal.
    Behind {
        head: i64,
        after: i64,
        changes: Vec<Change>,
        /// Whether more is waiting beyond this page.
        more: bool,
    },
    /// What was missed is no longer kept, so there is nothing to replay onto.
    Gap { head: i64, oldest: Option<i64> },
    /// The cursor is past the server's head, so the two are not the same log.
    Ahead { head: i64, cursor: i64 },
}

/// A whole plan as of some seq, and everything appended since.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub seq: i64,
    pub plan: Project,
    pub head: i64,
    pub changes: Vec<Change>,
}

/// What the server currently holds for a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub name: String,
    pub head: i64,
    /// How many other connections have this plan open.
    pub connected: usize,
}

/// Where a newly created plan landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub id: String,
    pub head: i64,
}

// ------------------------------------------------------------------ the wire

/// Whether a token may be sent to this address.
///
/// The same rule the sign in uses: plain HTTP is allowed only to this machine,
/// where there is no network to read it off. A bearer token sent in the clear
/// to anywhere else is a token somebody else has.
fn transport_is_safe(server: &str) -> bool {
    if let Some(rest) = server.strip_prefix("https://") {
        return !rest.is_empty();
    }
    let Some(rest) = server.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', ':']).next().unwrap_or_default();
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// The base address, checked and tidied, or the reason it cannot be used.
pub fn base(server: &str) -> Result<String, CollabError> {
    let server = server.trim().trim_end_matches('/');
    if server.is_empty() {
        return Err(CollabError::NoServer);
    }
    if !transport_is_safe(server) {
        return Err(CollabError::NotEncrypted(server.to_string()));
    }
    Ok(server.to_string())
}

/// Turn a status and a body into either a value or a message worth showing.
///
/// Separated from the calls so every refusal reads the same way whichever
/// endpoint produced it, and so the mapping can be tested without a server.
fn read_status(status: u16, body: &str) -> Result<(), CollabError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    match status {
        401 => Err(CollabError::NotSignedIn),
        403 => Err(CollabError::NotAllowed),
        404 => Err(CollabError::NoSuchProject),
        _ => Err(CollabError::Refused(message_in(body).unwrap_or_else(
            || format!("it answered with status {status}"),
        ))),
    }
}

/// The `message` field an error body carries, if it is one.
fn message_in(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

/// Make one call and hand back the status and the body.
///
/// Status codes are read here rather than turned into errors by the client,
/// because a push's refusals carry the whole answer in their bodies.
fn call(request: ureq::RequestBuilder<ureq::typestate::WithoutBody>, server: &str)
-> Result<(u16, String), CollabError> {
    let mut response = request
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .call()
        .map_err(|error| CollabError::NotReached {
            server: server.to_string(),
            why: describe(&error),
        })?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|_| {
        CollabError::NotReached {
            server: server.to_string(),
            why: "the reply did not finish arriving".into(),
        }
    })?;
    Ok((status, body))
}

/// The same, for the calls that carry a body.
///
/// The body is rendered here rather than by the client, because `ureq` is
/// built without its JSON feature: one serialiser in the tree, and one place
/// where a value that will not serialise is an error rather than a panic.
fn send(
    request: ureq::RequestBuilder<ureq::typestate::WithBody>,
    server: &str,
    payload: &impl Serialize,
) -> Result<(u16, String), CollabError> {
    let body = serde_json::to_string(payload)
        .map_err(|error| CollabError::Unreadable(error.to_string()))?;

    let mut response = request
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .send(body)
        .map_err(|error| CollabError::NotReached {
            server: server.to_string(),
            why: describe(&error),
        })?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(|_| {
        CollabError::NotReached {
            server: server.to_string(),
            why: "the reply did not finish arriving".into(),
        }
    })?;
    Ok((status, body))
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

// ------------------------------------------------------------------ the push

/// The shape every push answer has, whichever of the four it is.
#[derive(Debug, Deserialize)]
struct PushAnswer {
    #[serde(default)]
    status: String,
    #[serde(default)]
    head: i64,
    #[serde(default)]
    after: i64,
    #[serde(default)]
    cursor: i64,
    #[serde(default)]
    oldest: Option<i64>,
    #[serde(default)]
    changes: Vec<Change>,
    #[serde(default)]
    more: bool,
    #[serde(default)]
    applied: Vec<AppliedOne>,
    #[serde(default)]
    snapshot_wanted: bool,
}

#[derive(Debug, Deserialize)]
struct AppliedOne {
    local_id: u64,
    seq: i64,
}

/// Read a push answer, whatever the status was.
///
/// Split out from the call so the four outcomes can be exercised without a
/// server, which is the only way three of them ever get tested: two of them
/// need another client, and one needs a trimmed log.
pub fn read_push(status: u16, body: &str) -> Result<Pushed, CollabError> {
    // A refusal here is a conflict, not a failure, and the body is the answer.
    // Anything that is not a 200 or a 409 is a real error.
    if status != 200 && status != 409 {
        read_status(status, body)?;
    }

    let answer: PushAnswer = serde_json::from_str(body)
        .map_err(|error| CollabError::Unreadable(error.to_string()))?;

    match answer.status.as_str() {
        "applied" => Ok(Pushed::Applied {
            head: answer.head,
            applied: answer
                .applied
                .into_iter()
                .map(|one| (one.local_id, one.seq))
                .collect(),
            snapshot_wanted: answer.snapshot_wanted,
        }),
        "behind" => Ok(Pushed::Behind {
            head: answer.head,
            after: answer.after,
            changes: answer.changes,
            more: answer.more,
        }),
        "gap" => Ok(Pushed::Gap {
            head: answer.head,
            oldest: answer.oldest,
        }),
        "ahead" => Ok(Pushed::Ahead {
            head: answer.head,
            cursor: answer.cursor,
        }),
        other => Err(CollabError::Unreadable(format!(
            "it called the answer \"{other}\", which this copy does not know"
        ))),
    }
}

#[derive(Serialize)]
struct PushBody<'a> {
    after: i64,
    changes: &'a [Change],
    /// This client's live socket, when it has one open.
    ///
    /// The server broadcasts an appended change to every connection on the
    /// project except this one. Leaving it out means a client that is holding
    /// a socket is sent its own push straight back down it, and by then the
    /// entries carry the seqs the server assigned rather than the ids this
    /// copy chose, so they arrive looking like somebody else's work and are
    /// applied a second time. `append_task` is not idempotent, so that is a
    /// duplicated task and not a wasted round trip.
    ///
    /// Omitted entirely rather than sent as null when there is no socket, so
    /// the body a server sees is byte for byte the one it saw before this
    /// field existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    connection: Option<u64>,
}

/// Offer work to the server.
///
/// An empty `changes` is how a client asks "am I still current?", and it gets
/// the same four answers a real push does. That is what the sync view uses to
/// check rather than assume.
///
/// `connection` is this copy's live socket if it has one, so the append does
/// not send the work back to the copy that just offered it.
pub fn push(
    server: &str,
    token: &str,
    project: &str,
    after: i64,
    changes: &[Change],
    connection: Option<u64>,
) -> Result<Pushed, CollabError> {
    let base = base(server)?;
    let (status, body) = send(
        ureq::post(format!("{base}/api/projects/{project}/changes"))
            .header("Authorization", bearer(token)),
        &base,
        &PushBody { after, changes, connection },
    )?;
    read_push(status, &body)
}

/// The body a push sends, as JSON, so its shape can be checked without a
/// server.
///
/// Its own function because this is a field the client simply never filled in
/// and nothing noticed: the server read it, defaulted it to `None`, and
/// broadcast every synced change back to the client that made it. A shape
/// nothing asserts on is a shape that drifts.
#[cfg(test)]
fn push_body(after: i64, changes: &[Change], connection: Option<u64>) -> String {
    serde_json::to_string(&PushBody { after, changes, connection }).unwrap_or_default()
}

// ------------------------------------------------------------- whole plans

#[derive(Debug, Deserialize)]
struct SnapshotAnswer {
    seq: i64,
    plan: Project,
    #[serde(default)]
    head: i64,
    #[serde(default)]
    changes: Vec<Change>,
}

/// Fetch a whole plan, and everything appended after it.
///
/// The two come together on purpose: a snapshot alone is a plan as of some
/// seq, and asking for the tail separately leaves a window in which a change
/// appended between the two calls belongs to neither answer.
pub fn snapshot(server: &str, token: &str, project: &str) -> Result<Fetched, CollabError> {
    let base = base(server)?;
    let (status, body) = call(
        ureq::get(format!("{base}/api/projects/{project}/snapshot"))
            .header("Authorization", bearer(token)),
        &base,
    )?;
    read_status(status, &body)?;

    let answer: SnapshotAnswer = serde_json::from_str(&body)
        .map_err(|error| CollabError::Unreadable(error.to_string()))?;
    Ok(Fetched {
        seq: answer.seq,
        plan: answer.plan,
        head: answer.head,
        changes: answer.changes,
    })
}

#[derive(Serialize)]
struct SnapshotBody<'a> {
    seq: i64,
    plan: &'a Project,
}

/// Store a fresh whole plan, when the server has asked for one.
pub fn put_snapshot(
    server: &str,
    token: &str,
    project: &str,
    seq: i64,
    plan: &Project,
) -> Result<(), CollabError> {
    let base = base(server)?;
    let (status, body) = send(
        ureq::put(format!("{base}/api/projects/{project}/snapshot"))
            .header("Authorization", bearer(token)),
        &base,
        &SnapshotBody { seq, plan },
    )?;
    read_status(status, &body)
}

#[derive(Debug, Deserialize)]
struct CreatedAnswer {
    id: String,
    #[serde(default)]
    head: i64,
}

#[derive(Serialize)]
struct CreateBody<'a> {
    name: &'a str,
    plan: &'a Project,
}

/// Put a plan on the server for the first time.
pub fn create(
    server: &str,
    token: &str,
    name: &str,
    plan: &Project,
) -> Result<Created, CollabError> {
    let base = base(server)?;
    let (status, body) = send(
        ureq::post(format!("{base}/api/projects")).header("Authorization", bearer(token)),
        &base,
        &CreateBody { name, plan },
    )?;
    read_status(status, &body)?;

    let answer: CreatedAnswer = serde_json::from_str(&body)
        .map_err(|error| CollabError::Unreadable(error.to_string()))?;
    Ok(Created {
        id: answer.id,
        head: answer.head,
    })
}

#[derive(Debug, Deserialize)]
struct AboutAnswer {
    #[serde(default)]
    name: String,
    #[serde(default)]
    head: i64,
    #[serde(default)]
    connected: usize,
}

/// Ask the server where a plan has got to.
pub fn about(server: &str, token: &str, project: &str) -> Result<Standing, CollabError> {
    let base = base(server)?;
    let (status, body) = call(
        ureq::get(format!("{base}/api/projects/{project}"))
            .header("Authorization", bearer(token)),
        &base,
    )?;
    read_status(status, &body)?;

    let answer: AboutAnswer = serde_json::from_str(&body)
        .map_err(|error| CollabError::Unreadable(error.to_string()))?;
    Ok(Standing {
        name: answer.name,
        head: answer.head,
        connected: answer.connected,
    })
}

// -------------------------------------------------------------- sharing

/// One person who can reach a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The identifier the identity provider gave them. Meaningless to read,
    /// and the only thing a removal can be addressed to.
    pub subject: String,
    pub role: String,
    /// The address they came in by, which the server sends to the owner and to
    /// nobody else. `None` therefore means one of two things: this copy is not
    /// the owner, or this is the person who made the plan and was never
    /// invited. Either way there is no address to show.
    pub email: Option<String>,
}

/// An invitation that has been sent and not yet taken up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invitation {
    pub email: String,
    pub role: String,
    /// The date it was sent, as far as the day. An invitation nobody has
    /// claimed in a month is usually an address with a typo in it.
    pub sent_on: String,
}

/// Who a plan is shared with, as the server tells it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sharing {
    /// This copy's own subject, so its own row can be marked rather than
    /// offered a Remove button that would refuse.
    pub you: String,
    pub role: String,
    pub owner: String,
    pub members: Vec<Member>,
    /// `None` when this copy is not the owner. Not an empty list: "there are
    /// none" and "these are not yours to see" are different, and showing the
    /// second as the first would be a quiet lie to an editor.
    pub invites: Option<Vec<Invitation>>,
}

impl Sharing {
    /// Whether this copy may change any of it.
    pub fn you_own_it(&self) -> bool {
        self.you == self.owner
    }
}

#[derive(Debug, Deserialize)]
struct SharingAnswer {
    #[serde(default)]
    you: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    owner: String,
    #[serde(default)]
    members: Vec<MemberRow>,
    #[serde(default)]
    invites: Option<Vec<InviteRow>>,
}

#[derive(Debug, Deserialize)]
struct MemberRow {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InviteRow {
    #[serde(default)]
    email: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    invited_at: String,
}

/// The day out of a timestamp, for showing.
///
/// The minute an invitation was sent answers no question anybody asks about
/// one. A malformed timestamp is shown as it arrived rather than dropped: it
/// is a label, and a wrong label beats a missing row.
fn day_of(stamp: &str) -> String {
    match stamp.split_once('T') {
        Some((day, _)) if day.len() == 10 => day.to_string(),
        _ => stamp.to_string(),
    }
}

/// Who this plan is shared with.
pub fn sharing(server: &str, token: &str, project: &str) -> Result<Sharing, CollabError> {
    let base = base(server)?;
    let (status, body) = call(
        ureq::get(format!("{base}/api/projects/{project}/members"))
            .header("Authorization", bearer(token)),
        &base,
    )?;
    read_status(status, &body)?;
    read_sharing(&body)
}

/// Split from the call so the shape can be exercised without a server.
pub fn read_sharing(body: &str) -> Result<Sharing, CollabError> {
    let answer: SharingAnswer =
        serde_json::from_str(body).map_err(|error| CollabError::Unreadable(error.to_string()))?;
    Ok(Sharing {
        you: answer.you,
        role: answer.role,
        owner: answer.owner,
        members: answer
            .members
            .into_iter()
            .map(|row| Member {
                subject: row.subject,
                role: row.role,
                // An address the server sent as an empty string is no address.
                email: row.email.filter(|email| !email.trim().is_empty()),
            })
            .collect(),
        invites: answer.invites.map(|rows| {
            rows.into_iter()
                .map(|row| Invitation {
                    email: row.email,
                    role: row.role,
                    sent_on: day_of(&row.invited_at),
                })
                .collect()
        }),
    })
}

#[derive(Serialize)]
struct InviteBody<'a> {
    email: &'a str,
    role: &'a str,
}

/// Invite an address to a plan.
///
/// The address is not looked up anywhere, here or on the server. What comes
/// back says nothing about whether anybody holds it, because nothing asked:
/// an endpoint that answered that would tell whoever called it which addresses
/// have accounts behind them.
pub fn invite(
    server: &str,
    token: &str,
    project: &str,
    email: &str,
    role: &str,
) -> Result<(), CollabError> {
    let base = base(server)?;
    let (status, body) = send(
        ureq::post(format!("{base}/api/projects/{project}/invites"))
            .header("Authorization", bearer(token)),
        &base,
        &InviteBody { email, role },
    )?;
    read_status(status, &body)
}

/// Withdraw an invitation that has not been taken up.
pub fn cancel_invite(
    server: &str,
    token: &str,
    project: &str,
    email: &str,
) -> Result<(), CollabError> {
    let base = base(server)?;
    // Percent encoded, because an address carries an at sign and may carry a
    // plus, and a plus in a raw query string is a space.
    let email = crate::cloud::oauth::encode(email);
    let (status, body) = call(
        ureq::delete(format!("{base}/api/projects/{project}/invites?email={email}"))
            .header("Authorization", bearer(token)),
        &base,
    )?;
    read_status(status, &body)
}

/// Take somebody out of a plan.
///
/// Nothing happens to the copy on their machine, and nothing could: they hold
/// a whole plan and there is no reach into it from here. What changes is that
/// their next sync is answered the way any plan that is not theirs is.
pub fn remove_member(
    server: &str,
    token: &str,
    project: &str,
    subject: &str,
) -> Result<(), CollabError> {
    let base = base(server)?;
    let subject = crate::cloud::oauth::encode(subject);
    let (status, body) = call(
        ureq::delete(format!("{base}/api/projects/{project}/members?subject={subject}"))
            .header("Authorization", bearer(token)),
        &base,
    )?;
    read_status(status, &body)
}

/// What claiming an invitation came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claimed {
    /// In, with this role.
    Joined(String),
    /// Already in, and nothing was changed.
    Already(String),
    /// There is no invitation here for this account's address, or there is no
    /// such plan. The server answers both the same way on purpose, so that a
    /// plan id cannot be checked for existence by trying to claim it.
    NoInvitation,
    /// The identity provider could not be asked which address this account
    /// uses, so nobody knows whether there was an invitation or not. Kept
    /// apart from [`Claimed::NoInvitation`] because one of them is worth
    /// trying again and the other is not.
    CouldNotCheck(String),
    /// The provider does not vouch for this account's address. A third answer
    /// again, and not a refusal about this plan: it is about the account, it
    /// would be the same for every plan, and what to do about it is somewhere
    /// this application cannot reach.
    NotConfirmed(String),
}

#[derive(Debug, Deserialize)]
struct ClaimAnswer {
    #[serde(default)]
    status: String,
    #[serde(default)]
    role: String,
}

/// Present this account for a plan, in case it has been invited.
///
/// The one call that can turn somebody who cannot open a plan into somebody
/// who can. It proves nothing about them by itself: the server asks the
/// identity provider which address the token belongs to, and matches that
/// against what the owner typed. Whoever holds the token is the only person it
/// can ever admit.
pub fn claim(server: &str, token: &str, project: &str) -> Result<Claimed, CollabError> {
    let base = base(server)?;
    let (status, body) = send(
        ureq::post(format!("{base}/api/projects/{project}/claim"))
            .header("Authorization", bearer(token)),
        &base,
        &serde_json::json!({}),
    )?;
    read_claim(status, &body)
}

/// Read a claim answer, whatever the status was.
///
/// Split from the call because the three refusals are the whole point of the
/// endpoint and none of them can be produced without a server, an identity
/// provider, and somebody else's invitation.
pub fn read_claim(status: u16, body: &str) -> Result<Claimed, CollabError> {
    match status {
        // Not an error. It is the ordinary answer to opening a link somebody
        // sent to the wrong address, and it has to be told apart from the
        // outage below.
        404 => return Ok(Claimed::NoInvitation),
        // The server could not ask the identity provider. Its own words, which
        // name the endpoint it could not reach.
        502 => {
            return Ok(Claimed::CouldNotCheck(message_in(body).unwrap_or_else(|| {
                "the server could not reach the identity provider".to_string()
            })));
        }
        // The only thing this endpoint calls a bad request is an address the
        // provider has not confirmed. Read as an answer rather than as a
        // failure, so that what comes back is advice about an account rather
        // than the general "check the server address" a refusal carries.
        400 => {
            return Ok(Claimed::NotConfirmed(message_in(body).unwrap_or_else(|| {
                "your identity provider has not confirmed this account's address".to_string()
            })));
        }
        _ => read_status(status, body)?,
    }

    let answer: ClaimAnswer =
        serde_json::from_str(body).map_err(|error| CollabError::Unreadable(error.to_string()))?;
    match answer.status.as_str() {
        "joined" => Ok(Claimed::Joined(answer.role)),
        "already" => Ok(Claimed::Already(answer.role)),
        other => Err(CollabError::Unreadable(format!(
            "it called the answer \"{other}\", which this copy does not know"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------- the four answers

    #[test]
    fn an_accepted_push_says_which_seq_each_change_was_given() {
        let body = r#"{ "status": "applied", "head": 45,
                        "applied": [ { "local_id": 7, "seq": 43 },
                                     { "local_id": 8, "seq": 44 } ],
                        "snapshot_wanted": false }"#;
        assert_eq!(
            read_push(200, body),
            Ok(Pushed::Applied {
                head: 45,
                applied: vec![(7, 43), (8, 44)],
                snapshot_wanted: false,
            })
        );
    }

    #[test]
    fn a_behind_answer_carries_what_was_missed_rather_than_needing_a_second_call() {
        let body = r#"{ "status": "behind", "head": 45, "after": 42, "more": false,
                        "changes": [ { "id": 43, "at": "2026-08-18T09:00:00",
                                       "author": "Grace", "script": "indent();",
                                       "summary": "Indented a task" } ] }"#;
        let Ok(Pushed::Behind { head, after, changes, more }) = read_push(409, body) else {
            panic!("a 409 saying behind is not an error");
        };
        assert_eq!((head, after, more), (45, 42, false));
        assert_eq!(changes.len(), 1, "the rebase needs no second round trip");
        assert_eq!(changes[0].author, "Grace");
    }

    #[test]
    fn a_push_says_which_socket_it_must_not_be_echoed_to() {
        // Asserted on the wire shape, because this is a field that existed on
        // the server, was documented there, was read there, and was never once
        // sent. Nothing noticed, and every sync made during a live session was
        // broadcast straight back to the copy that made it.
        let body = push_body(42, &[], Some(11));
        assert!(body.contains("\"connection\":11"), "got {body}");
    }

    #[test]
    fn a_push_with_no_socket_leaves_the_field_out_rather_than_sending_nothing() {
        // Not `null`. A server built before the field existed should see the
        // body it has always seen, byte for byte.
        let body = push_body(42, &[], None);
        assert!(!body.contains("connection"), "got {body}");
        assert!(body.contains("\"after\":42"), "got {body}");
    }

    #[test]
    fn a_gap_is_told_apart_from_being_behind() {
        // Rebasing on an incomplete answer is exactly the silent data loss the
        // gap answer exists to prevent, so the two must not be conflated.
        let body = r#"{ "status": "gap", "head": 45, "oldest": 38,
                        "message": "the log this cursor needs has been trimmed" }"#;
        assert_eq!(
            read_push(409, body),
            Ok(Pushed::Gap { head: 45, oldest: Some(38) })
        );
    }

    #[test]
    fn a_cursor_past_the_head_comes_back_as_ahead() {
        let body = r#"{ "status": "ahead", "head": 12, "cursor": 60,
                        "message": "this cursor is past the server's head" }"#;
        assert_eq!(
            read_push(409, body),
            Ok(Pushed::Ahead { head: 12, cursor: 60 })
        );
    }

    #[test]
    fn an_answer_this_copy_does_not_know_is_reported_rather_than_guessed_at() {
        let body = r#"{ "status": "rearranged", "head": 4 }"#;
        assert!(matches!(
            read_push(409, body),
            Err(CollabError::Unreadable(_))
        ));
    }

    // ------------------------------------------------------------- refusals

    #[test]
    fn a_real_failure_is_not_read_as_a_push_answer() {
        let body = r#"{ "error": "unauthenticated", "message": "not authenticated" }"#;
        assert_eq!(read_push(401, body), Err(CollabError::NotSignedIn));
        assert_eq!(read_push(403, body), Err(CollabError::NotAllowed));
        assert_eq!(read_push(404, body), Err(CollabError::NoSuchProject));
    }

    #[test]
    fn a_refusal_keeps_the_servers_own_words() {
        let body = r#"{ "error": "bad_request", "message": "a push carries at most 1000 changes" }"#;
        assert_eq!(
            read_push(400, body),
            Err(CollabError::Refused(
                "a push carries at most 1000 changes".into()
            ))
        );
    }

    #[test]
    fn every_message_says_what_the_person_can_do() {
        let errors = [
            CollabError::NoServer,
            CollabError::NotEncrypted("http://sync.example.org".into()),
            CollabError::NotReached {
                server: "https://sync.example.org".into(),
                why: "nothing is answering at that address".into(),
            },
            CollabError::NotSignedIn,
            CollabError::NotAllowed,
            CollabError::NoSuchProject,
            CollabError::Refused("the plan is not a project".into()),
            CollabError::Unreadable("missing field `seq`".into()),
        ];
        for error in errors {
            let message = error.to_string();
            assert!(message.len() > 40, "too terse to act on: {message}");
            assert!(
                message.contains("Options")
                    || message.contains("Ask")
                    || message.contains("Check")
                    || message.contains("Try"),
                "nothing to do about it: {message}"
            );
        }
    }

    // --------------------------------------------------------------- sharing

    #[test]
    fn the_owner_is_sent_the_addresses_and_the_invitations() {
        let body = r#"{ "you": "owner-sub", "role": "owner", "owner": "owner-sub",
                        "members": [
                          { "subject": "owner-sub", "role": "owner", "email": null },
                          { "subject": "ada-sub", "role": "editor", "email": "ada@example.com" } ],
                        "invites": [
                          { "email": "grace@example.com", "role": "viewer",
                            "invited_at": "2026-08-19T09:12:44Z" } ] }"#;
        let sharing = read_sharing(body).expect("the owner's view");

        assert!(sharing.you_own_it());
        assert_eq!(sharing.members.len(), 2);
        // Null for whoever made the plan: they were never invited, so the
        // server was never given an address for them.
        assert_eq!(sharing.members[0].email, None);
        assert_eq!(sharing.members[1].email.as_deref(), Some("ada@example.com"));

        let waiting = sharing.invites.expect("the owner sees what is pending");
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].email, "grace@example.com");
        // The minute an invitation was sent answers no question anybody asks.
        assert_eq!(waiting[0].sent_on, "2026-08-19");
    }

    #[test]
    fn a_member_who_is_not_the_owner_is_sent_no_addresses_at_all() {
        // Null and not an empty list, and the difference matters: an editor
        // shown "no invitations are waiting" would be told something the
        // server never said.
        let body = r#"{ "you": "ada-sub", "role": "editor", "owner": "owner-sub",
                        "members": [
                          { "subject": "owner-sub", "role": "owner", "email": null },
                          { "subject": "ada-sub", "role": "editor", "email": null } ],
                        "invites": null }"#;
        let sharing = read_sharing(body).expect("an editor's view");

        assert!(!sharing.you_own_it());
        assert!(sharing.invites.is_none(), "these are not hers to see");
        assert!(
            sharing.members.iter().all(|member| member.email.is_none()),
            "an editor is sent no addresses, so none can be drawn",
        );
    }

    // ---------------------------------------------------------- claiming

    #[test]
    fn a_claim_that_worked_says_what_it_came_to() {
        assert_eq!(
            read_claim(200, r#"{ "status": "joined", "role": "editor" }"#),
            Ok(Claimed::Joined("editor".into()))
        );
        assert_eq!(
            read_claim(200, r#"{ "status": "already", "role": "viewer" }"#),
            Ok(Claimed::Already("viewer".into()))
        );
    }

    #[test]
    fn being_uninvited_is_an_answer_and_not_a_failure() {
        // The ordinary result of opening a link sent to the wrong address. It
        // has to come back as something to say, not as an error that reads
        // like the server is broken.
        let body = r#"{ "error": "not_found", "message": "not found" }"#;
        assert_eq!(read_claim(404, body), Ok(Claimed::NoInvitation));
    }

    #[test]
    fn a_provider_that_could_not_be_asked_is_told_apart_from_a_refusal() {
        // The distinction the whole endpoint turns on. One of these is worth
        // trying again in a minute and the other is not, and a client that
        // showed them the same way would send somebody to ask for an
        // invitation they already have.
        let body = r#"{ "error": "idp_unavailable",
                        "message": "identity provider: userinfo: connection refused" }"#;
        let answer = read_claim(502, body);
        assert_eq!(
            answer,
            Ok(Claimed::CouldNotCheck(
                "identity provider: userinfo: connection refused".into()
            ))
        );
        assert_ne!(answer, Ok(Claimed::NoInvitation));
    }

    #[test]
    fn an_unconfirmed_address_comes_back_in_the_servers_own_words() {
        // A 400, because it is about the caller's own account rather than
        // about this plan, so the server can afford to say what is wrong. It
        // is not read as a refusal: a refusal's advice is to check the server
        // address, and the server address has nothing to do with it.
        let body = r#"{ "error": "bad_request",
                        "message": "your identity provider has not confirmed that this address belongs to this account" }"#;
        assert_eq!(
            read_claim(400, body),
            Ok(Claimed::NotConfirmed(
                "your identity provider has not confirmed that this address belongs to this account".into()
            ))
        );
    }

    // ------------------------------------------------------------- addresses

    #[test]
    fn a_token_is_only_sent_in_the_clear_to_this_machine() {
        assert!(transport_is_safe("https://sync.example.org"));
        assert!(transport_is_safe("http://localhost:8090"));
        assert!(transport_is_safe("http://127.0.0.1:8090"));
        // Anywhere else and there is a network between here and there.
        assert!(!transport_is_safe("http://sync.example.org"));
        assert!(!transport_is_safe("http://192.168.1.4:8090"));
        assert!(!transport_is_safe("sync.example.org"));
    }

    #[test]
    fn a_trailing_slash_does_not_produce_a_double_one_in_every_url() {
        assert_eq!(base("https://sync.example.org/"), Ok("https://sync.example.org".into()));
        assert_eq!(base("  "), Err(CollabError::NoServer));
    }
}
