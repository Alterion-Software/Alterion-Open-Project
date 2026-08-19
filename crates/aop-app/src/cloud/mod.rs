//! Signing in to an Alterion account, and staying signed in.
//!
//! The account is what a planner uses to reach anything kept away from this
//! machine. Nothing here knows what that is: this module's whole job is to turn
//! a person at a keyboard into an access token that other code can present, and
//! to keep that token good without anybody having to think about it.
//!
//! The identity provider is self hosted and self deployable, so the only thing
//! written down is the address of the default one. Every endpoint underneath it
//! is read from that server's discovery document. Someone running their own
//! copy changes the issuer in Options and the rest follows: no build, no patch,
//! nothing hardcoded to a hostname.
//!
//! ```text
//!   sign_in(issuer, client_id)
//!        |
//!        |  1. GET {issuer}/.well-known/openid-configuration
//!        |  2. make a verifier, keep it here; send only its SHA-256
//!        |  3. listen on 127.0.0.1:0, whichever port is free
//!        |  4. open the system browser at the authorize endpoint
//!        |  5. wait for the browser, with a deadline
//!        |  6. check the state matches before looking at the code
//!        |  7. POST the code and the verifier to the token endpoint
//!        |  8. GET the userinfo endpoint to find out who that was
//!        v
//!     Session ---- access_token() renews itself a little early
//! ```
//!
//! Two things in here are not conveniences and should not be softened. The
//! loopback port is whatever the operating system hands out, never a fixed one,
//! because a program already holding a fixed port would be handed the
//! authorization code. And the `state` is checked before the code is touched,
//! because without that check anyone who can get the browser to visit a URL can
//! sign this application in as themselves.

pub mod collab;
pub mod device;
pub mod health;
pub mod link;
pub mod live;
pub mod oauth;
pub mod share;
pub mod tokens;
pub mod work;

use std::time::Duration;

use chrono::{DateTime, Utc};

pub use oauth::Endpoints;
pub use tokens::Stored;

/// How often a waiting task looks to see whether its worker has finished.
///
/// Short enough that a sign in landing feels immediate, long enough that
/// waiting five minutes for a browser costs a few thousand wake ups rather
/// than a core.
const LOOK_EVERY: Duration = Duration::from_millis(90);

/// Run blocking work on a thread of its own, and take the answer on the
/// interface's thread when it lands.
///
/// Every call in this module blocks, several of them for as long as a person
/// takes to finish a page in a browser. None of them may run where the
/// interface does, and none of them may be awaited by holding the interface
/// still, so the work goes to a thread and the wait becomes a poll.
///
/// `landed` is given `None` when the worker did not come back at all. That is
/// a panic, and the caller has to hear about it: the alternative is a button
/// that says it is still working for the rest of the session.
pub fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    landed: impl FnOnce(Option<T>) + 'static,
) {
    dioxus::prelude::spawn(async move {
        let worker = std::thread::spawn(work);
        while !worker.is_finished() {
            tokio::time::sleep(LOOK_EVERY).await;
        }
        landed(worker.join().ok());
    });
}

/// Whoever signed in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Account {
    /// The identifier the server uses for this person, which never changes.
    /// Names and addresses do, so anything remembering an account remembers
    /// this.
    pub subject: String,
    /// What to show. Never empty: an account with no name to give falls back to
    /// the part of the address before the at sign.
    pub name: String,
    pub email: String,
    /// Where the account's picture is, when the provider serves one.
    ///
    /// `None` is the ordinary case, not a fault: the claim is optional in
    /// OIDC and this deployment's provider does not send it yet. Whatever
    /// draws an account falls back to [`Account::initials`] rather than
    /// leaving a hole where a face would be.
    ///
    /// Only ever an address tokens would be safe on, checked where the claim
    /// is read: this is a string the provider chooses and the webview would
    /// otherwise be asked to fetch.
    pub picture: Option<String>,
}

impl Account {
    /// The letters to draw when there is no picture.
    ///
    /// Taken from the display name rather than the address, so the initials
    /// and the name under them agree. First and last word, which is how a
    /// person writes their own: "Ada Lovelace King" is AK, not AL.
    pub fn initials(&self) -> String {
        let letters: Vec<char> = self
            .name
            .split_whitespace()
            .filter_map(|word| word.chars().find(|letter| letter.is_alphanumeric()))
            .collect();
        let picked: String = match letters.as_slice() {
            // Nothing in the name to draw with. The address is the only other
            // thing an account is sure to have, so it stands in.
            [] => self
                .email
                .chars()
                .find(|letter| letter.is_alphanumeric())
                .into_iter()
                .collect(),
            [only] => only.to_string(),
            [first, .., last] => format!("{first}{last}"),
        };
        picked.to_uppercase()
    }
}

/// Where the provider keeps the page a person manages their own account on.
///
/// Nothing about a password, an address or a picture happens in this
/// application. The browser already has a session with the provider, the
/// provider owns those flows, and a desktop application that asks for a
/// password is a place credentials can be collected for no reason. So this
/// resolves to an address to open, and that is all it does.
///
/// Derived from the issuer rather than written down, because the provider is
/// self hosted: there is no host to hardcode. `configured` is the settings
/// key, for a deployment whose account page is not under its issuer, and it
/// wins when it is set.
///
/// `None` when there is nothing worth opening, which is a button that is not
/// drawn rather than one that opens a browser at nowhere.
pub fn account_page(issuer: &str, configured: &str) -> Option<String> {
    let configured = configured.trim().trim_end_matches('/');
    if !configured.is_empty() {
        return oauth::transport_is_safe(configured).then(|| configured.to_string());
    }
    let issuer = issuer.trim().trim_end_matches('/');
    if issuer.is_empty() || !oauth::transport_is_safe(issuer) {
        return None;
    }
    Some(format!("{issuer}{ACCOUNT_PATH}"))
}

/// What is put after the issuer when no account page has been set.
///
/// A guess, and knowingly one: only the provider knows where its own account
/// page lives, and the discovery document has no field that says. It is the
/// path the Alterion provider uses, and the settings key exists for every
/// deployment where it is wrong.
const ACCOUNT_PATH: &str = "/account";

/// Everything that can stop a sign in, phrased as the thing to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInError {
    /// The issuer could not be read, or is not an identity provider.
    NoDiscovery { issuer: String, why: String },
    /// This machine cannot say which machine it is, so a session cannot be
    /// bound to it and nothing can be sealed to it. Fatal on purpose: an empty
    /// identity is one every machine shares, and a made up one changes every
    /// launch.
    NoDeviceIdentity(String),
    /// The system random generator could not be read.
    NoRandomness,
    /// Nothing could listen for the browser's reply.
    NoLoopback(String),
    /// Nothing opened the browser. Carries the address, so it can be opened by
    /// hand rather than the sign in being simply impossible.
    NoBrowser(String),
    /// The browser never came back.
    Abandoned,
    /// The server said no, in the words of RFC 6749.
    Refused {
        code: String,
        description: Option<String>,
    },
    /// The reply did not belong to this attempt. The CSRF defence.
    StateMismatch,
    /// The browser came back agreeing to nothing in particular.
    NoCode,
    /// The token endpoint would not trade the code.
    CodeRefused,
    /// The session is over and cannot be renewed.
    SessionEnded,
    /// Something else went wrong talking to the token endpoint.
    NoTokens(String),
    /// Signed in, but the server would not say who.
    NoAccount(String),
    /// Signed out here, but the server was not told.
    NotRevoked(String),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignInError::NoDiscovery { issuer, why } => write!(
                f,
                "The sign in page could not be reached: {why}. \
                 Check the server address in Options; it is currently {issuer}."
            ),
            // The one from `device`, which already names what it looked at.
            SignInError::NoDeviceIdentity(why) => write!(f, "{why}"),
            SignInError::NoRandomness => write!(
                f,
                "Sign in needs the system random number generator and could not read it, \
                 so it has stopped rather than start an attempt that would not be secure. \
                 Restarting the machine is the usual fix."
            ),
            SignInError::NoLoopback(why) => write!(
                f,
                "Sign in listens on this machine for the browser's reply, and could not: {why}. \
                 Check any firewall or security tool that blocks connections within the \
                 machine, then try again."
            ),
            SignInError::NoBrowser(url) => write!(
                f,
                "The browser could not be opened. Paste this address into a browser to finish \
                 signing in:\n\n{url}"
            ),
            SignInError::Abandoned => write!(
                f,
                "Sign in was not finished in time and has been stopped. \
                 Start it again and complete the page in the browser within five minutes."
            ),
            SignInError::Refused { code, description } => match code.as_str() {
                // The ordinary path, not a malfunction: somebody pressed
                // Cancel, or declined the permissions.
                "access_denied" => write!(
                    f,
                    "Sign in was declined in the browser. Nothing has changed. \
                     Try again when you are ready."
                ),
                "invalid_scope" => write!(
                    f,
                    "This account is not allowed to sign in to Alterion Open Project. \
                     Ask whoever administers the sign in server to grant it."
                ),
                other => {
                    write!(f, "The sign in server refused the request ({other}).")?;
                    if let Some(description) = description {
                        write!(f, " It said: {description}.")?;
                    }
                    write!(f, " Check the server address in Options and try again.")
                }
            },
            SignInError::StateMismatch => write!(
                f,
                "The reply from the browser did not belong to this sign in, so it has been \
                 discarded and nothing was signed in. Start sign in again. If it keeps \
                 happening, close any other copies of Alterion Open Project first."
            ),
            SignInError::NoCode => write!(
                f,
                "The browser came back without finishing the sign in. Start it again and let \
                 the page in the browser run to the end."
            ),
            SignInError::CodeRefused => write!(
                f,
                "The sign in server would not complete the sign in. The usual cause is that \
                 this application's redirect address is not registered against its client id: \
                 a desktop application listens on whichever port is free, so the address has to \
                 be registered as a loopback one rather than with a fixed port. \
                 Whoever administers the sign in server can check that."
            ),
            SignInError::SessionEnded => write!(
                f,
                "This account is no longer signed in. Sign in again. \
                 This happens after a password change, or when the session was ended \
                 from another device."
            ),
            SignInError::NoTokens(why) => write!(
                f,
                "The sign in could not be completed: {why}. Try again, and check the server \
                 address in Options if it keeps happening."
            ),
            SignInError::NoAccount(why) => write!(
                f,
                "Sign in worked, but the account details could not be read: {why}. \
                 Sign in again."
            ),
            SignInError::NotRevoked(why) => write!(
                f,
                "Signed out on this machine, but the sign in server could not be told: {why}. \
                 If this machine is shared, sign out from your account page as well."
            ),
        }
    }
}

impl std::error::Error for SignInError {}

/// A signed in account and the tokens that go with it.
///
/// Held by whatever needs to reach the server. It renews itself, so a caller
/// asks for a token and gets one that works rather than having to watch a
/// clock.
///
/// One session per stored account, and no copies. The server spends a refresh
/// token the moment it is used and treats a second use of the same one as a
/// stolen token, revoking everything. Two sessions renewing from one stored
/// record would do exactly that to themselves.
pub struct Session {
    /// As it was set in Options, which is what the endpoints are rediscovered
    /// from if they are ever needed again.
    issuer: String,
    client_id: String,
    /// Discovered at sign in, and looked up again after a restore. Kept out of
    /// storage on purpose: a deployment that moves its endpoints should be
    /// followed, not remembered wrongly.
    endpoints: Option<Endpoints>,
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
    account: Account,
}

/// As with the stored record, the tokens are left out of the printed form so
/// that a debug print somewhere cannot put one in a log.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("account", &self.account)
            .field("expires_at", &self.expires_at)
            .field("access_token", &"(withheld)")
            .field("refresh_token", &"(withheld)")
            .finish()
    }
}

impl Session {
    /// Who this is.
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// Which server signed them in.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// When the current access token stops being accepted, for showing rather
    /// than for deciding anything: [`Session::access_token`] already handles it.
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// An access token that works.
    ///
    /// Renews first if the current one is close to running out, so the caller
    /// never presents a token that expires half way through what it was doing.
    /// Needs `&mut` for exactly that reason: asking for a token can change the
    /// session.
    pub fn access_token(&mut self) -> Result<&str, SignInError> {
        if tokens::needs_refresh(Utc::now(), self.expires_at) {
            self.renew()?;
        }
        Ok(&self.access_token)
    }

    /// Trade the refresh token for a fresh pair.
    fn renew(&mut self) -> Result<(), SignInError> {
        if self.refresh_token.is_empty() {
            return Err(SignInError::SessionEnded);
        }

        let endpoints = self.resolve_endpoints()?;
        let fresh = oauth::refresh(&endpoints, &self.client_id, &self.refresh_token)?;
        self.take(fresh);

        // The old refresh token has just been spent, and the server treats a
        // second use of it as theft against the whole account. So the
        // replacement is written down straight away, and if it cannot be, the
        // stale record is thrown away rather than left to present a spent token
        // on the next start.
        if tokens::save_session(&self.stored()).is_err() {
            let _ = tokens::clear_session();
        }
        Ok(())
    }

    /// The endpoints, discovering them if this session came back from storage.
    fn resolve_endpoints(&mut self) -> Result<Endpoints, SignInError> {
        if let Some(endpoints) = &self.endpoints {
            return Ok(endpoints.clone());
        }
        let endpoints = oauth::discover(&self.issuer)?;
        self.endpoints = Some(endpoints.clone());
        Ok(endpoints)
    }

    /// Take a token response into the session.
    fn take(&mut self, fresh: oauth::Tokens) {
        self.expires_at = tokens::expires_at(Utc::now(), fresh.expires_in);
        self.access_token = fresh.access_token;
        // A server that sends no new refresh token means the old one stands.
        if !fresh.refresh_token.is_empty() {
            self.refresh_token = fresh.refresh_token;
        }
    }

    /// Ask the provider who this is, again.
    ///
    /// The account page lives at the provider, so a name, an address or a
    /// picture changes there and nothing here is told. Reading the claims
    /// again is the only way to find out, and it is worth doing at the one
    /// moment a change is likely rather than on a timer: this happens a
    /// handful of times in an account's life, and a repeating call for it
    /// would be waste that also keeps waking the interface.
    ///
    /// The refreshed claims go through exactly the same reading as the ones at
    /// sign in, address check included. A claim is no more trustworthy for
    /// having arrived second.
    pub fn refresh_account(&mut self) -> Result<Account, SignInError> {
        // Before the endpoints, because asking for a token can renew the
        // session, and a renewal is a better moment to fail than half way
        // through a request.
        self.access_token()?;
        let endpoints = self.resolve_endpoints()?;
        let claims = oauth::userinfo(&endpoints, &self.access_token)?;
        let account = account_from(&claims);
        self.account = account.clone();
        // Written down so the card says the same thing on the next start
        // without asking again. Quiet on failure: an unwritable store is not
        // a reason to throw away a session that works.
        let _ = tokens::save_session(&self.stored());
        Ok(account)
    }

    /// The session as it is kept between runs.
    fn stored(&self) -> Stored {
        Stored {
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
            access_token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
            expires_at: self.expires_at.timestamp(),
            subject: self.account.subject.clone(),
            name: self.account.name.clone(),
            email: self.account.email.clone(),
            picture: self.account.picture.clone(),
        }
    }
}

/// Sign in, from the beginning.
///
/// Blocking, and slow by nature: it opens a browser and waits for a person. The
/// caller runs it off the interface thread and shows what came back.
pub fn sign_in(issuer: &str, client_id: &str) -> Result<Session, SignInError> {
    // Before anything else, and before a browser is opened at anybody. A
    // session is bound to the machine it was issued to, and a machine that
    // cannot say which one it is has nothing to bind to. Stopping here costs a
    // message; carrying on would mean either an empty identity, which every
    // machine shares, or an invented one, which changes every launch and signs
    // the user out on every start.
    device::components().map_err(|absent| SignInError::NoDeviceIdentity(absent.to_string()))?;

    let endpoints = oauth::discover(issuer)?;

    // The verifier stays in this process. Only its digest goes through the
    // browser, so a code lifted out of the browser's history or off the address
    // bar cannot be traded for anything.
    let key = oauth::proof_key()?;
    let state = oauth::fresh_state()?;

    // Bound before the browser is opened, so the port in the redirect address
    // is one that is already being listened on.
    let loopback = oauth::Loopback::open()?;
    let redirect_uri = loopback.redirect_uri();

    let url = oauth::authorize_url(&endpoints, client_id, &redirect_uri, &state, &key.challenge);
    oauth::open_in_browser(&url)?;

    let callback = loopback.wait(oauth::callback_timeout())?;
    let code = code_from(callback, &state)?;

    let tokens = oauth::exchange_code(
        &endpoints,
        client_id,
        &code,
        &redirect_uri,
        &key.verifier,
    )?;

    let claims = oauth::userinfo(&endpoints, &tokens.access_token)?;
    let account = account_from(&claims);

    let mut session = Session {
        issuer: endpoints.issuer.clone(),
        client_id: client_id.to_string(),
        endpoints: Some(endpoints),
        access_token: String::new(),
        refresh_token: String::new(),
        expires_at: Utc::now(),
        account,
    };
    session.take(tokens);

    // A sign in that is not written down is one the user has to do again on the
    // next start. A failure to write it is not a failure to sign in, though, so
    // the session is returned either way.
    let _ = tokens::save_session(&session.stored());

    Ok(session)
}

/// Make sense of what the browser came back with.
///
/// Split out from the flow so the decision can be tested without a browser, a
/// socket or a server, which is the only way the refusal paths ever get
/// exercised.
fn code_from(callback: oauth::Callback, expected_state: &str) -> Result<String, SignInError> {
    // A refusal carries no code, so there is nothing here that could be spent
    // even if it were somebody else's. Reporting what the server actually said
    // beats reporting a state mismatch for a server that echoed no state.
    if let Some(code) = callback.error {
        return Err(SignInError::Refused {
            code,
            description: callback.error_description,
        });
    }

    // Everything below this line depends on the reply belonging to this
    // attempt. Without the check, anyone who can make the browser visit a URL
    // can sign this application in as themselves and have the planner's work
    // land in their account.
    let matched = callback
        .state
        .as_deref()
        .is_some_and(|returned| oauth::same_value(returned, expected_state));
    if !matched {
        return Err(SignInError::StateMismatch);
    }

    callback
        .code
        .filter(|code| !code.is_empty())
        .ok_or(SignInError::NoCode)
}

/// Turn the server's claims into something to put on screen.
///
/// The name is whichever of these the server had. A blank where a name should
/// be looks like a fault, so there is always something.
fn account_from(claims: &oauth::Claims) -> Account {
    let email = claims.email.clone().unwrap_or_default();
    let name = claims
        .name
        .clone()
        .or_else(|| claims.preferred_username.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| match email.split_once('@') {
            Some((local, _)) if !local.is_empty() => local.to_string(),
            _ => claims.sub.clone(),
        });

    Account {
        subject: claims.sub.clone(),
        name,
        email,
        // Checked here rather than where it is drawn, so that having one at
        // all means having one that is safe to fetch. The claim is a string
        // the provider chooses, and it ends up as the source of an image the
        // webview loads.
        picture: claims
            .picture
            .clone()
            .map(|url| url.trim().to_string())
            .filter(|url| oauth::transport_is_safe(url)),
    }
}

/// Sign out.
///
/// Tells the server to forget both tokens, then forgets them here. The local
/// half happens whatever the server says: a person who pressed Sign out is
/// signed out, and being told the server could not be reached while still
/// appearing to be signed in would be the worst of both.
pub fn sign_out(session: &Session) -> Result<(), SignInError> {
    let endpoints = match &session.endpoints {
        Some(endpoints) => Some(endpoints.clone()),
        None => oauth::discover(&session.issuer).ok(),
    };

    let mut outcome = Ok(());
    if let Some(endpoints) = endpoints {
        // The refresh token first: it is the standing permission, and the one
        // that matters if only one of the two calls gets through.
        for token in [&session.refresh_token, &session.access_token] {
            if token.is_empty() {
                continue;
            }
            if let Err(error) = oauth::revoke(&endpoints, token) {
                outcome = Err(error);
            }
        }
    } else {
        outcome = Err(SignInError::NotRevoked(
            "the server address could not be read".into(),
        ));
    }

    let _ = tokens::clear_session();
    outcome
}

/// Pick up where the last run left off.
///
/// Reads the store and no more: no network, so start up is not held up by a
/// server that is slow or absent. The token is renewed on first use, which is
/// where a session that has since been ended shows up as needing a fresh sign
/// in.
///
/// `None` for every way this can come to nothing, and they all mean the same
/// thing to whoever draws the interface: nobody is signed in. A machine that
/// changed, a store with nothing in it, a blob somebody edited. None of them is
/// worth a dialog, and a person whose motherboard was replaced simply signs in
/// again.
pub fn restore() -> Option<Session> {
    let stored = tokens::load_session()?;
    let expires_at = DateTime::from_timestamp(stored.expires_at, 0).unwrap_or_else(Utc::now);

    // Nothing to renew with and nothing left to use: this is not a session, it
    // is a leftover, and offering it would show somebody as signed in until the
    // first thing they tried failed.
    if stored.refresh_token.is_empty() && tokens::needs_refresh(Utc::now(), expires_at) {
        let _ = tokens::clear_session();
        return None;
    }

    Some(Session {
        issuer: stored.issuer,
        client_id: stored.client_id,
        endpoints: None,
        access_token: stored.access_token,
        refresh_token: stored.refresh_token,
        expires_at,
        account: Account {
            subject: stored.subject,
            name: stored.name,
            email: stored.email,
            // Checked again on the way out: the record is written by this
            // application but read from a file, and a file can be edited.
            picture: stored.picture.filter(|url| oauth::transport_is_safe(url)),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callback(query: &str) -> oauth::Callback {
        oauth::parse_callback(&format!("GET /callback?{query} HTTP/1.1"))
    }

    // ------------------------------------------------------- the state check

    #[test]
    fn a_matching_state_lets_the_code_through() {
        let outcome = code_from(callback("code=the-code&state=the-state"), "the-state");
        assert_eq!(outcome, Ok("the-code".to_string()));
    }

    #[test]
    fn a_mismatched_state_is_refused_and_the_code_is_not_used() {
        // This is the CSRF defence. Without it, anyone able to make the browser
        // visit a URL can sign this application in as themselves.
        let outcome = code_from(callback("code=someone-elses&state=not-mine"), "the-state");
        assert_eq!(outcome, Err(SignInError::StateMismatch));
    }

    #[test]
    fn a_reply_with_no_state_at_all_is_refused() {
        assert_eq!(
            code_from(callback("code=the-code"), "the-state"),
            Err(SignInError::StateMismatch)
        );
    }

    #[test]
    fn a_state_that_is_only_a_prefix_of_the_real_one_is_refused() {
        assert_eq!(
            code_from(callback("code=x&state=the-stat"), "the-state"),
            Err(SignInError::StateMismatch)
        );
        assert_eq!(
            code_from(callback("code=x&state=the-state-and-more"), "the-state"),
            Err(SignInError::StateMismatch)
        );
    }

    #[test]
    fn a_reply_with_a_state_but_no_code_is_not_treated_as_a_sign_in() {
        assert_eq!(
            code_from(callback("state=the-state"), "the-state"),
            Err(SignInError::NoCode)
        );
        assert_eq!(
            code_from(callback("code=&state=the-state"), "the-state"),
            Err(SignInError::NoCode)
        );
    }

    // ---------------------------------------------------------- refusals

    #[test]
    fn a_declined_sign_in_is_reported_as_a_refusal() {
        let outcome = code_from(callback("error=access_denied&state=the-state"), "the-state");
        assert_eq!(
            outcome,
            Err(SignInError::Refused {
                code: "access_denied".into(),
                description: None
            })
        );
    }

    #[test]
    fn a_refusal_is_reported_even_when_the_server_echoed_no_state() {
        // There is no code in a refusal, so nothing can be spent; saying "that
        // reply was not yours" instead of what the server said would only
        // confuse whoever pressed Cancel.
        let outcome = code_from(callback("error=access_denied"), "the-state");
        assert!(matches!(outcome, Err(SignInError::Refused { .. })));
    }

    #[test]
    fn a_refusal_keeps_the_servers_own_words() {
        let outcome = code_from(
            callback("error=invalid_request&error_description=Missing%20scope&state=the-state"),
            "the-state",
        );
        assert_eq!(
            outcome,
            Err(SignInError::Refused {
                code: "invalid_request".into(),
                description: Some("Missing scope".into())
            })
        );
    }

    #[test]
    fn declining_reads_as_declining_rather_than_as_a_fault() {
        let message = SignInError::Refused {
            code: "access_denied".into(),
            description: None,
        }
        .to_string();
        assert!(message.contains("declined"), "got {message}");
        assert!(message.contains("Nothing has changed"), "got {message}");
    }

    // ------------------------------------------------------ error messages

    #[test]
    fn every_message_says_what_the_person_can_do() {
        // An error a planner cannot act on is an error that becomes a support
        // request, so each one names the thing to try.
        let errors = [
            SignInError::NoDiscovery {
                issuer: "https://auth.example.org".into(),
                why: "nothing is answering at that address".into(),
            },
            SignInError::NoDeviceIdentity(
                device::NoAnchor {
                    looked_at: vec!["/etc/machine-id".into()],
                }
                .to_string(),
            ),
            SignInError::NoRandomness,
            SignInError::NoLoopback("permission denied".into()),
            SignInError::NoBrowser("https://auth.example.org/authorize?x=1".into()),
            SignInError::Abandoned,
            SignInError::Refused {
                code: "server_error".into(),
                description: None,
            },
            SignInError::StateMismatch,
            SignInError::NoCode,
            SignInError::CodeRefused,
            SignInError::SessionEnded,
            SignInError::NoTokens("the server did not answer in time".into()),
            SignInError::NoAccount("the server answered with status 500".into()),
            SignInError::NotRevoked("nothing is answering at that address".into()),
        ];

        for error in errors {
            let message = error.to_string();
            assert!(message.len() > 40, "too terse to act on: {message}");
            assert!(
                message.contains("Check")
                    || message.contains("Try")
                    || message.contains("try")
                    || message.contains("Sign in again")
                    || message.contains("Start")
                    || message.contains("Paste")
                    || message.contains("sign out")
                    || message.contains("Ask")
                    || message.contains("Restarting")
                    || message.contains("can check"),
                "nothing to do about it: {message}"
            );
        }
    }

    #[test]
    fn an_unreachable_server_points_at_the_setting_that_names_it() {
        let message = SignInError::NoDiscovery {
            issuer: "https://auth.example.org".into(),
            why: "nothing is answering at that address".into(),
        }
        .to_string();
        assert!(message.contains("Options"), "got {message}");
        assert!(message.contains("https://auth.example.org"), "got {message}");
    }

    #[test]
    fn a_browser_that_will_not_open_hands_the_address_over_instead() {
        // Being unable to launch a browser should not make signing in
        // impossible, only manual.
        let url = "https://auth.example.org/authorize?client_id=x&state=y";
        let message = SignInError::NoBrowser(url.into()).to_string();
        assert!(message.contains(url), "got {message}");
    }

    // ------------------------------------------------------------ account

    #[test]
    fn a_name_is_used_when_the_server_gives_one() {
        let claims = oauth::Claims {
            sub: "0198f0c2".into(),
            name: Some("Ada Lovelace".into()),
            preferred_username: Some("ada".into()),
            email: Some("ada@example.org".into()),
            picture: None,
        };
        let account = account_from(&claims);
        assert_eq!(account.name, "Ada Lovelace");
        assert_eq!(account.email, "ada@example.org");
        assert_eq!(account.subject, "0198f0c2");
    }

    #[test]
    fn the_username_stands_in_when_there_is_no_name() {
        // Which is the ordinary case against this server: it sends a preferred
        // username and no name at all.
        let claims = oauth::Claims {
            sub: "0198f0c2".into(),
            name: None,
            preferred_username: Some("ada".into()),
            email: Some("ada@example.org".into()),
            picture: None,
        };
        assert_eq!(account_from(&claims).name, "ada");
    }

    #[test]
    fn the_address_stands_in_when_there_is_neither() {
        let claims = oauth::Claims {
            sub: "0198f0c2".into(),
            name: None,
            preferred_username: None,
            email: Some("ada@example.org".into()),
            picture: None,
        };
        assert_eq!(account_from(&claims).name, "ada");
    }

    #[test]
    fn an_account_is_never_shown_with_a_blank_name() {
        // A blank where a name belongs reads as something having gone wrong.
        let claims = oauth::Claims {
            sub: "0198f0c2".into(),
            name: Some("   ".into()),
            preferred_username: None,
            email: None,
            picture: None,
        };
        assert_eq!(account_from(&claims).name, "0198f0c2");
        assert!(!account_from(&claims).name.is_empty());
    }

    // ------------------------------------------------------------ session

    fn session_expiring_in(seconds: i64) -> Session {
        Session {
            issuer: "https://auth.example.org".into(),
            client_id: "a-client".into(),
            endpoints: None,
            access_token: "an-access-token".into(),
            refresh_token: String::new(),
            expires_at: tokens::expires_at(Utc::now(), seconds),
            account: Account::default(),
        }
    }

    #[test]
    fn a_token_with_plenty_of_life_left_is_handed_straight_back() {
        let mut session = session_expiring_in(3600);
        assert_eq!(session.access_token(), Ok("an-access-token"));
    }

    #[test]
    fn a_token_about_to_expire_is_renewed_rather_than_handed_out() {
        // With no refresh token there is nothing to renew with, so this surfaces
        // as the session being over. What matters is that the stale token was
        // not returned as though it were good.
        let mut session = session_expiring_in(tokens::EARLY_REFRESH_SECONDS - 1);
        assert_eq!(session.access_token(), Err(SignInError::SessionEnded));
    }

    #[test]
    fn a_session_does_not_print_its_tokens() {
        let printed = format!("{:?}", session_expiring_in(3600));
        assert!(!printed.contains("an-access-token"), "got {printed}");
    }

    #[test]
    fn nothing_here_names_a_server() {
        // The provider is self hosted and self deployable, so an address
        // compiled in here would be one deployment signing in everybody who
        // never looked at the setting. A session carries the issuer it was
        // made against, and there is nowhere else for one to come from.
        let session = session_expiring_in(3600);
        assert_eq!(session.issuer(), "https://auth.example.org");
    }

    // ------------------------------------------------- the account card

    fn named(name: &str) -> Account {
        Account {
            name: name.into(),
            ..Account::default()
        }
    }

    #[test]
    fn an_account_with_no_picture_falls_back_to_its_initials() {
        // The provider serves no picture claim yet, so this is what every card
        // draws today. It has to read as a monogram rather than as a face that
        // failed to arrive.
        let account = Account {
            name: "Ada Lovelace".into(),
            email: "ada@example.org".into(),
            ..Account::default()
        };
        assert_eq!(account.picture, None);
        assert_eq!(account.initials(), "AL");
    }

    #[test]
    fn initials_are_the_first_and_last_name_however_many_are_between() {
        assert_eq!(named("Ada Lovelace King").initials(), "AK");
        assert_eq!(named("ada lovelace").initials(), "AL");
        assert_eq!(named("Ada").initials(), "A");
    }

    #[test]
    fn a_name_that_gives_no_letters_falls_back_to_the_address() {
        // An account always has something to draw with, so the circle is never
        // simply blank.
        let account = Account {
            name: "  ".into(),
            email: "ada@example.org".into(),
            ..Account::default()
        };
        assert_eq!(account.initials(), "A");
    }

    #[test]
    fn a_picture_the_provider_serves_is_kept_and_one_it_could_not_be_is_not() {
        let with = account_from(&oauth::Claims {
            sub: "a-subject".into(),
            name: Some("Ada Lovelace".into()),
            picture: Some("https://auth.example.org/faces/ada.png".into()),
            ..oauth::Claims::default()
        });
        assert_eq!(
            with.picture.as_deref(),
            Some("https://auth.example.org/faces/ada.png")
        );

        // The claim is a string the provider chooses and it ends up as the
        // source of an image the webview loads, so anything that is not an
        // address a token would be safe on is dropped rather than drawn.
        for unusable in ["javascript:alert(1)", "http://elsewhere.example/face.png", "  "] {
            let account = account_from(&oauth::Claims {
                sub: "a-subject".into(),
                picture: Some(unusable.into()),
                ..oauth::Claims::default()
            });
            assert_eq!(account.picture, None, "{unusable} should not be drawn");
        }
    }

    #[test]
    fn a_claim_with_no_picture_at_all_is_ordinary_rather_than_a_failure() {
        let account = account_from(&oauth::Claims {
            sub: "a-subject".into(),
            name: Some("Ada Lovelace".into()),
            ..oauth::Claims::default()
        });
        assert_eq!(account.picture, None);
        assert_eq!(account.initials(), "AL");
    }

    // ------------------------------------------------- the account page

    #[test]
    fn the_account_page_is_derived_from_whichever_issuer_signed_this_copy_in() {
        // No host is written down anywhere: the provider is self hosted, so
        // the only address this application has is the one it was given.
        assert_eq!(
            account_page("https://auth.example.org", ""),
            Some("https://auth.example.org/account".to_string())
        );
        assert_eq!(
            account_page("https://auth.example.org/", ""),
            Some("https://auth.example.org/account".to_string())
        );
    }

    #[test]
    fn a_deployment_that_keeps_its_account_page_elsewhere_is_followed() {
        assert_eq!(
            account_page("https://auth.example.org", "https://id.example.org/profile/"),
            Some("https://id.example.org/profile".to_string())
        );
    }

    #[test]
    fn there_is_no_button_when_there_is_nowhere_to_open() {
        assert_eq!(account_page("", ""), None);
        // Same rule as everything else pointed at a provider: not encrypted,
        // not opened.
        assert_eq!(account_page("http://auth.example.org", ""), None);
        assert_eq!(account_page("https://auth.example.org", "ftp://elsewhere"), None);
    }
}
