//! A check somebody can run when collaborating is not working.
//!
//! Five questions, asked in order and answered separately, because which one
//! fails is the whole diagnosis:
//!
//! ```text
//!   1  is a server address set          ->  nothing to check without one
//!   2  does that server answer          ->  address, DNS, firewall, TLS
//!   3  what does it say about itself    ->  its own database and issuer
//!   4  does the identity provider answer->  a separate machine, separately down
//!   5  is there a token, and is it good ->  signing in, rather than the servers
//! ```
//!
//! One combined verdict would throw all of that away. "Server reachable,
//! identity provider unreachable" points at a name or a firewall; "could not
//! connect" points at nothing.
//!
//! The health endpoint is unauthenticated on purpose, which is exactly why it
//! is worth asking: it still answers when signing in is the thing that is
//! broken.
//!
//! Nothing here prints a token, or any part of one, or its length. A length is
//! not nothing: it says which of two providers issued it.

use std::time::Duration;

use serde::Deserialize;

use crate::cloud::collab::{self, CollabError};
use crate::cloud::{oauth, tokens};

/// Health is asked when something is already wrong, so it gives up sooner than
/// the calls that carry work: a planner waiting a minute to be told nothing is
/// answering has learned the same thing more slowly.
const REQUEST_TIMEOUT_SECONDS: u64 = 10;

/// How one check came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Good,
    /// Working, with something worth knowing about it.
    Warning,
    Bad,
    /// Not asked, because an earlier answer made it meaningless. Said out
    /// loud rather than left blank: a check that did not run is not a check
    /// that passed.
    NotChecked,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Good => "OK",
            Outcome::Warning => "Check",
            Outcome::Bad => "Failed",
            Outcome::NotChecked => "Not checked",
        }
    }
}

/// One question and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub asked: &'static str,
    pub outcome: Outcome,
    /// What was found, and what to do about it when the news is bad.
    pub detail: String,
}

impl Check {
    fn new(asked: &'static str, outcome: Outcome, detail: impl Into<String>) -> Check {
        Check {
            asked,
            outcome,
            detail: detail.into(),
        }
    }
}

/// The name the server gives the message that carries work over the live
/// socket.
///
/// The one thing a client has to be able to ask about before it starts
/// streaming. It is a name and not a number on purpose: this is a question
/// about which messages are understood, and messages are added and answered
/// independently of each other, so an ordering would only be a guess about
/// which build gained which one.
pub const LIVE_CHANGES: &str = "live-changes";

/// What a server said when asked whether it understands streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaks {
    /// It named the streaming message, so work may be offered over the socket.
    Streaming,
    /// It answered, and did not name it. Older than streaming, so the socket
    /// is good for watching and this copy's own work has to go the other way.
    NotStreaming,
    /// It could not be asked at all. Not the same as a no, and must not be
    /// treated as one: a health endpoint that is briefly unreachable is not a
    /// reason to turn a working session into a degraded one.
    Unknown,
}

/// What the server says about itself.
#[derive(Debug, Clone, Default, Deserialize)]
struct Health {
    #[serde(default)]
    status: String,
    #[serde(default)]
    service: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    database: bool,
    #[serde(default)]
    issuer: String,
    /// The messages this server understands, by name.
    ///
    /// An option rather than a defaulted list, and the difference carries the
    /// whole design. A server that lists nothing has answered the question and
    /// the answer is no. A server that has no such field has not been asked a
    /// question it knows about: it is older than the field itself, and it may
    /// well speak everything this copy does. Collapsing the two would turn
    /// every deployment that has not been updated yet into a watching-only
    /// session, which is a regression dressed up as caution. Those are left to
    /// find out by trying, and a batch nobody answers is given up on and
    /// reported.
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

/// Run the lot. Blocking, so it belongs on a worker like every other call
/// that touches a network.
pub fn run(server: &str, issuer: &str) -> Vec<Check> {
    let mut checks = Vec::with_capacity(5);

    let base = match collab::base(server) {
        Ok(base) => {
            checks.push(Check::new(
                "Collaborate server address",
                Outcome::Good,
                format!("Set to {base}."),
            ));
            Some(base)
        }
        Err(error) => {
            checks.push(Check::new(
                "Collaborate server address",
                Outcome::Bad,
                error.to_string(),
            ));
            None
        }
    };

    let answered = match &base {
        Some(base) => {
            let reached = ask(base);
            checks.push(match &reached {
                Ok(_) => Check::new(
                    "Server answers",
                    Outcome::Good,
                    format!("{base}/api/health answered."),
                ),
                Err(error) => Check::new("Server answers", Outcome::Bad, error.to_string()),
            });
            reached.ok()
        }
        None => {
            checks.push(Check::new(
                "Server answers",
                Outcome::NotChecked,
                "There is no address to try.",
            ));
            None
        }
    };

    checks.push(match answered {
        Some(health) => describe_health(&health),
        None => Check::new(
            "What the server reports",
            Outcome::NotChecked,
            "The server did not answer, so it said nothing about itself.",
        ),
    });

    checks.push(check_issuer(issuer));
    checks.push(check_token());
    checks
}

/// Ask the health endpoint. No token: it is unauthenticated on purpose.
fn ask(base: &str) -> Result<Health, CollabError> {
    let mut response = ureq::get(format!("{base}/api/health"))
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .call()
        .map_err(|error| CollabError::NotReached {
            server: base.to_string(),
            why: oauth::describe(&error),
        })?;

    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|_| CollabError::NotReached {
            server: base.to_string(),
            why: "the reply did not finish arriving".into(),
        })?;

    // A degraded server answers 503 with a body worth reading, so the body is
    // parsed before the status is judged.
    match serde_json::from_str::<Health>(&body) {
        Ok(health) if !health.service.is_empty() => Ok(health),
        _ => Err(CollabError::NotReached {
            server: base.to_string(),
            why: format!(
                "something answered with status {status}, but it is not a Collaborate server"
            ),
        }),
    }
}

/// Ask a server whether it understands work offered over the live socket.
///
/// Blocking, like everything else here, so it belongs on a worker. Asked once
/// when a live session starts rather than on every batch: the answer is a
/// property of the build on the other end, and a server that changed under a
/// running session has dropped the socket anyway.
///
/// Unauthenticated, which is what makes it cheap enough to ask at all: it
/// costs one request that needs no token and no database of its own.
pub fn speaks_streaming(server: &str) -> Speaks {
    let Ok(base) = collab::base(server) else {
        return Speaks::Unknown;
    };
    match ask(&base) {
        Ok(health) => health.speaks_streaming(),
        Err(_) => Speaks::Unknown,
    }
}

impl Health {
    /// Whether this server named the streaming message.
    ///
    /// A plain search for the name rather than a comparison of versions. A
    /// newer server naming messages this build has never heard of is not a
    /// mismatch, and neither is an older one naming fewer: the only question
    /// is whether the one message about to be sent is understood.
    ///
    /// Saying nothing at all is not saying no. See [`Health::capabilities`].
    fn speaks_streaming(&self) -> Speaks {
        match &self.capabilities {
            None => Speaks::Unknown,
            Some(named) if named.iter().any(|name| name == LIVE_CHANGES) => Speaks::Streaming,
            Some(_) => Speaks::NotStreaming,
        }
    }
}

/// Turn the health body into something worth reading.
fn describe_health(health: &Health) -> Check {
    let mut detail = format!("{} version {}", health.service, health.version);
    if !health.issuer.is_empty() {
        // Worth showing beside the app's own issuer: a server signing people
        // in against a different provider refuses every token and says only
        // that the token is not active.
        detail.push_str(&format!(", signing in against {}", health.issuer));
    }
    detail.push('.');

    if health.database && health.status == "ok" {
        // Healthy and older than streaming is a real state and worth saying
        // out loud, because everything else about such a server looks fine:
        // it answers, it signs people in, and it takes a Sync. The only thing
        // it does not do is understand edits sent over the live socket, and
        // without this line the symptom is live editing that half works.
        match health.speaks_streaming() {
            Speaks::Streaming => {}
            Speaks::NotStreaming => {
                detail.push_str(
                    " It does not understand edits sent over the live connection, so it is \
                     older than this copy. Live editing still shows you other people's work \
                     as it happens; your own goes to the server when you save this plan or \
                     press Sync. Updating the server has it go as you type.",
                );
                return Check::new("What the server reports", Outcome::Warning, detail);
            }
            Speaks::Unknown => detail.push_str(
                " It does not say which messages it understands, so it is older than this \
                 copy in at least that respect. Live editing finds out by trying, and says \
                 so if nothing comes back.",
            ),
        }
        return Check::new("What the server reports", Outcome::Good, detail);
    }
    detail.push_str(
        " It reports that it cannot reach its own database, so it can answer this check \
         and nothing else. Whoever runs the server has to look at it.",
    );
    Check::new("What the server reports", Outcome::Bad, detail)
}

/// Is the identity provider there, and is it an identity provider.
///
/// Separate from the sync server on purpose: they are two machines and go
/// wrong one at a time.
fn check_issuer(issuer: &str) -> Check {
    if issuer.trim().is_empty() {
        return Check::new(
            "Identity provider",
            Outcome::Bad,
            "No identity provider address is set, so there is nothing to sign in against. \
             Fill it in in Options, under Alterion Collaborate.",
        );
    }
    match oauth::discover(issuer) {
        Ok(endpoints) => Check::new(
            "Identity provider",
            Outcome::Good,
            format!("{} answered with its sign in details.", endpoints.issuer),
        ),
        Err(error) => Check::new("Identity provider", Outcome::Bad, error.to_string()),
    }
}

/// Is there a session on this machine, and is it still usable.
///
/// Reads the store rather than the running session, because the question being
/// asked is what a fresh start would find.
fn check_token() -> Check {
    let Some(stored) = tokens::load_session() else {
        return Check::new(
            "Stored sign in",
            Outcome::Warning,
            "Nobody is signed in on this machine. Sign in from Options, under \
             Alterion Collaborate.",
        );
    };

    let expires_at = chrono::DateTime::from_timestamp(stored.expires_at, 0);
    let Some(expires_at) = expires_at else {
        return Check::new(
            "Stored sign in",
            Outcome::Bad,
            "There is a stored sign in, but its expiry makes no sense. Sign out and sign \
             in again from Options.",
        );
    };

    // Whether it can be renewed is the question, not whether the current
    // access token is still good: a session with a refresh token renews itself
    // and a planner never sees the difference.
    let renewable = !stored.refresh_token.is_empty();
    let expired = tokens::needs_refresh(chrono::Utc::now(), expires_at);
    let who = if stored.email.is_empty() {
        stored.name.clone()
    } else {
        stored.email.clone()
    };

    match (expired, renewable) {
        (false, _) => Check::new(
            "Stored sign in",
            Outcome::Good,
            format!("Signed in as {who}, good until {}.", expires_at.naive_local().format("%Y-%m-%d %H:%M")),
        ),
        (true, true) => Check::new(
            "Stored sign in",
            Outcome::Warning,
            format!("Signed in as {who}. The current pass has run out and will be renewed \
                     on the next sync. If that fails, sign out and sign in again."),
        ),
        (true, false) => Check::new(
            "Stored sign in",
            Outcome::Bad,
            format!("The sign in for {who} has run out and cannot be renewed. \
                     Sign in again from Options, under Alterion Collaborate."),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_is_reported_separately() {
        // One combined verdict is the thing this exists instead of: which
        // check failed is the diagnosis.
        let checks = run("", "");
        assert_eq!(checks.len(), 5);
        assert!(checks.iter().all(|check| !check.detail.is_empty()));
    }

    #[test]
    fn with_no_address_the_server_checks_say_they_did_not_run() {
        // Rather than passing, or failing for a reason that is not the reason.
        let checks = run("", "");
        assert_eq!(checks[0].outcome, Outcome::Bad, "no address is the fault");
        assert_eq!(checks[1].outcome, Outcome::NotChecked);
        assert_eq!(checks[2].outcome, Outcome::NotChecked);
    }

    /// A healthy server that names whatever it is given.
    fn reporting(capabilities: Option<&[&str]>) -> Health {
        Health {
            status: "ok".into(),
            service: "aop-collaborate".into(),
            version: "1.0.0-beta".into(),
            database: true,
            issuer: "https://auth.example.org".into(),
            capabilities: capabilities
                .map(|named| named.iter().map(|name| name.to_string()).collect()),
        }
    }

    #[test]
    fn a_server_that_cannot_reach_its_database_is_a_failure_not_a_pass() {
        let health = Health {
            status: "degraded".into(),
            database: false,
            ..reporting(Some(&[LIVE_CHANGES]))
        };
        let check = describe_health(&health);
        assert_eq!(check.outcome, Outcome::Bad);
        assert!(check.detail.contains("database"), "got {}", check.detail);
    }

    #[test]
    fn the_version_and_the_issuer_are_shown_because_bug_reports_need_them() {
        let check = describe_health(&reporting(Some(&[LIVE_CHANGES])));
        assert_eq!(check.outcome, Outcome::Good);
        assert!(check.detail.contains("1.0.0-beta"));
        assert!(check.detail.contains("https://auth.example.org"));
    }

    #[test]
    fn a_server_that_does_not_name_the_streaming_message_is_said_to_be_older() {
        // This is the whole point of publishing a name. Such a server answers
        // every other check perfectly: it is up, it has its database, and it
        // signs people in. Without this it looks like nothing is wrong and
        // live editing simply half works.
        let check = describe_health(&reporting(Some(&[])));
        assert_eq!(check.outcome, Outcome::Warning, "got {}", check.detail);
        assert!(check.detail.contains("older than this copy"), "got {}", check.detail);
    }

    #[test]
    fn a_server_that_says_nothing_about_what_it_speaks_has_not_said_no() {
        // The distinction the whole design turns on. A server older than the
        // field itself may well speak everything this copy does, and demoting
        // every deployment that has not been updated yet to a watching-only
        // session would be a regression dressed up as caution. It finds out by
        // trying, and an unanswered batch is what catches it.
        assert_eq!(reporting(None).speaks_streaming(), Speaks::Unknown);
        assert_eq!(reporting(Some(&[])).speaks_streaming(), Speaks::NotStreaming);
        assert_eq!(
            reporting(Some(&[LIVE_CHANGES])).speaks_streaming(),
            Speaks::Streaming,
        );

        let check = describe_health(&reporting(None));
        assert_eq!(check.outcome, Outcome::Good, "got {}", check.detail);
        assert!(check.detail.contains("does not say"), "got {}", check.detail);
    }

    #[test]
    fn a_name_this_copy_has_never_heard_of_is_not_a_mismatch() {
        // A newer server naming messages this build knows nothing about is
        // the ordinary way a protocol grows. The only question asked is
        // whether the one message about to be sent is understood.
        let check = describe_health(&reporting(Some(&[LIVE_CHANGES, "something-later"])));
        assert_eq!(check.outcome, Outcome::Good, "got {}", check.detail);
    }

    #[test]
    fn nothing_in_a_check_could_carry_a_token() {
        // Not the token, not a piece of it, and not its length: a length says
        // which of two providers issued it.
        let checks = run("", "");
        for check in checks {
            assert!(!check.detail.contains("Bearer"), "got {}", check.detail);
            assert!(!check.detail.contains("access_token"), "got {}", check.detail);
        }
    }
}
