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

    #[test]
    fn a_server_that_cannot_reach_its_database_is_a_failure_not_a_pass() {
        let health = Health {
            status: "degraded".into(),
            service: "aop-collaborate".into(),
            version: "1.0.0-beta".into(),
            database: false,
            issuer: "https://auth.example.org".into(),
        };
        let check = describe_health(&health);
        assert_eq!(check.outcome, Outcome::Bad);
        assert!(check.detail.contains("database"), "got {}", check.detail);
    }

    #[test]
    fn the_version_and_the_issuer_are_shown_because_bug_reports_need_them() {
        let health = Health {
            status: "ok".into(),
            service: "aop-collaborate".into(),
            version: "1.0.0-beta".into(),
            database: true,
            issuer: "https://auth.example.org".into(),
        };
        let check = describe_health(&health);
        assert_eq!(check.outcome, Outcome::Good);
        assert!(check.detail.contains("1.0.0-beta"));
        assert!(check.detail.contains("https://auth.example.org"));
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
