//! The authorization code flow, as a desktop application has to run it.
//!
//! A native application has nowhere to keep a client secret: whatever is
//! compiled into it is compiled into everybody's copy. So this is a public
//! client, and the thing that proves the token request came from the same
//! program that started the sign in is PKCE (RFC 7636): a random verifier is
//! kept in this process, only its SHA-256 goes to the browser, and the token
//! endpoint will not trade the code for tokens without the original.
//!
//! The reply comes back to a loopback socket rather than a custom URL scheme,
//! which is what RFC 8252 asks of a native application. The port is whatever
//! the operating system hands out, never a fixed one: a fixed port collides
//! with anything else already listening, and a program that got there first
//! would be handed the authorization code.
//!
//! Nothing about the server is written down here beyond its address. Every
//! endpoint is read from the issuer's discovery document, so someone running
//! their own deployment of the identity provider changes one setting and the
//! rest follows.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::cloud::SignInError;
use crate::quiet::Quiet;

/// What is asked for. Only what the application actually reads: the subject to
/// tell accounts apart, and a name and address to show whose account it is.
pub const SCOPE: &str = "openid profile email";

/// How long to wait for the browser before giving up on the sign in.
///
/// Long enough to find a password and a second factor, short enough that a
/// person who wandered off does not leave a socket open for the rest of the
/// session. The wait ends the moment the browser answers, so this only ever
/// costs an abandoned attempt.
const CALLBACK_TIMEOUT_SECONDS: u64 = 300;

/// How often the loopback socket is looked at while waiting.
const POLL_INTERVAL_MILLIS: u64 = 100;

/// How long any one request to the identity provider may take.
const REQUEST_TIMEOUT_SECONDS: u64 = 20;

/// The most of a browser's request that is read before it is parsed. A request
/// line and its headers are far smaller; the cap is only so that something
/// which is not a browser cannot make this read forever.
const MAX_REQUEST_BYTES: usize = 16 * 1024;

// ----------------------------------------------------------------- discovery

/// Where the identity provider keeps each part of the flow.
///
/// Read rather than assumed, so a deployment that serves its endpoints from
/// different paths works without a change here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// The issuer as the server itself states it, which is not always exactly
    /// what was typed into the setting.
    pub issuer: String,
    pub authorize: String,
    pub token: String,
    pub userinfo: String,
    /// Optional: RFC 7009 is a separate specification and a deployment may not
    /// offer it. Signing out then forgets the tokens locally and no more.
    pub revoke: Option<String>,
}

/// The discovery document, as far as this application cares about it.
#[derive(Deserialize)]
struct DiscoveryDocument {
    issuer: Option<String>,
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
    userinfo_endpoint: Option<String>,
    revocation_endpoint: Option<String>,
    code_challenge_methods_supported: Option<Vec<String>>,
}

/// The well known location, from an issuer.
///
/// The path is appended to the issuer rather than replacing its path, which is
/// what RFC 8414 says to do and what lets an identity provider live under a
/// prefix such as `https://example.com/auth`.
pub fn discovery_url(issuer: &str) -> String {
    format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    )
}

/// Whether an address is one tokens may safely travel over.
///
/// Plain HTTP is refused except to the loopback interface, where it never
/// leaves the machine. Someone self hosting can develop against
/// `http://127.0.0.1:8080` and still not be able to point the application at a
/// public server that would put an access token on the wire in the clear.
///
/// Public within the crate because the same question is asked of anything else
/// this application hands to the desktop or to the webview on the provider's
/// say so, and there should be one answer to it rather than two.
pub fn transport_is_safe(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://[::1]")
        || url.starts_with("http://localhost")
}

/// Read a discovery document.
///
/// Kept apart from fetching it so the parsing and its checks can be tested
/// without a server.
pub fn parse_discovery(issuer: &str, body: &str) -> Result<Endpoints, SignInError> {
    let document: DiscoveryDocument = serde_json::from_str(body).map_err(|_| {
        SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: "the address answered with something that is not a discovery document".into(),
        }
    })?;

    let missing = |what: &str| SignInError::NoDiscovery {
        issuer: issuer.to_string(),
        why: format!("its discovery document names no {what}"),
    };

    let stated = document.issuer.unwrap_or_default();
    // RFC 8414 requires the issuer in the document to match the one asked for.
    // A mismatch means the address is answering for somebody else, which is how
    // a mix-up attack starts, so it is refused rather than followed.
    if stated.trim_end_matches('/') != issuer.trim_end_matches('/') {
        return Err(SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: format!("it answers for {stated} instead"),
        });
    }

    // Only S256 is used. A server that offers nothing but the plain method is
    // one where a stolen authorization code is enough on its own.
    let methods = document.code_challenge_methods_supported.unwrap_or_default();
    if !methods.iter().any(|m| m == "S256") {
        return Err(SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: "it does not offer the S256 proof key method this application requires".into(),
        });
    }

    let endpoints = Endpoints {
        issuer: stated,
        authorize: document.authorization_endpoint.ok_or_else(|| missing("sign in page"))?,
        token: document.token_endpoint.ok_or_else(|| missing("token endpoint"))?,
        userinfo: document.userinfo_endpoint.ok_or_else(|| missing("account endpoint"))?,
        revoke: document.revocation_endpoint,
    };

    for url in [&endpoints.authorize, &endpoints.token, &endpoints.userinfo] {
        if !transport_is_safe(url) {
            return Err(SignInError::NoDiscovery {
                issuer: issuer.to_string(),
                why: format!("it points at {url}, which is not encrypted"),
            });
        }
    }

    Ok(endpoints)
}

/// Ask an issuer where its endpoints are.
pub fn discover(issuer: &str) -> Result<Endpoints, SignInError> {
    let issuer = issuer.trim().trim_end_matches('/');
    if issuer.is_empty() {
        return Err(SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: "no server address is set".into(),
        });
    }
    if !transport_is_safe(issuer) {
        return Err(SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: "the address is not an encrypted one".into(),
        });
    }

    let body = ureq::get(discovery_url(issuer))
        .config()
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .call()
        .map_err(|error| SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: describe(&error),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|_| SignInError::NoDiscovery {
            issuer: issuer.to_string(),
            why: "the reply from the server did not finish arriving".into(),
        })?;

    parse_discovery(issuer, &body)
}

/// Put a request failure into words that mean something to a person.
pub(crate) fn describe(error: &ureq::Error) -> String {
    match error {
        ureq::Error::HostNotFound => "the address does not resolve".into(),
        ureq::Error::ConnectionFailed => "nothing is answering at that address".into(),
        ureq::Error::Timeout(_) => "the server did not answer in time".into(),
        ureq::Error::StatusCode(code) => format!("the server answered with status {code}"),
        ureq::Error::BadUri(_) => "the address is not a valid one".into(),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => {
            "the server's certificate could not be trusted".into()
        }
        other => format!("the request did not go through ({other})"),
    }
}

// --------------------------------------------------------------------- PKCE

/// A proof key: the secret this process keeps, and the digest the browser
/// carries in its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofKey {
    pub verifier: String,
    pub challenge: String,
}

/// The S256 challenge for a verifier: base64url of its SHA-256, unpadded.
///
/// Written out rather than taken on trust, because RFC 7636 publishes a worked
/// example and getting this wrong is only visible as the token endpoint saying
/// no.
pub fn challenge_for(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// A fresh proof key.
///
/// Thirty-two random bytes become forty-three base64url characters, which sits
/// inside RFC 7636's range of 43 to 128 and uses only its unreserved alphabet,
/// so nothing has to be escaped on the way through a query string.
pub fn proof_key() -> Result<ProofKey, SignInError> {
    let verifier = base64url(&random_bytes(32)?);
    let challenge = challenge_for(&verifier);
    Ok(ProofKey { verifier, challenge })
}

/// An opaque value tying the reply to this attempt.
pub fn fresh_state() -> Result<String, SignInError> {
    Ok(base64url(&random_bytes(16)?))
}

/// base64url without padding, which is the encoding every part of this flow
/// uses. Small enough that it is not worth a dependency.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let mut block = [0u8; 3];
        block[..group.len()].copy_from_slice(group);
        let packed = u32::from(block[0]) << 16 | u32::from(block[1]) << 8 | u32::from(block[2]);

        // Three bytes make four characters; a short final group makes fewer,
        // and the padding that would have followed is simply left off.
        let characters = group.len() + 1;
        for index in 0..characters {
            let shift = 18 - index * 6;
            out.push(ALPHABET[(packed >> shift) as usize & 0x3f] as char);
        }
    }
    out
}

/// Random bytes from the operating system.
///
/// The verifier and the state are the whole security of this flow, so they come
/// from the system generator and nowhere else. There is no fallback to a weaker
/// source: a sign in that cannot be made unguessable does not happen at all.
fn random_bytes(count: usize) -> Result<Vec<u8>, SignInError> {
    crate::cloud::tokens::random_bytes(count).ok_or(SignInError::NoRandomness)
}

// ------------------------------------------------------- percent encoding

/// Percent encode a query value, leaving only the unreserved set alone.
///
/// Written here rather than pulled in, because the only thing it has to be is
/// correct for the handful of values this module puts in a URL.
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Undo percent encoding.
///
/// A plus sign is left as itself rather than read as a space. The values that
/// come back here are opaque ones the server chose, and rewriting a character
/// inside one would corrupt it; the state this application sends is base64url,
/// which has no plus in its alphabet at all.
pub fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let pair = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = pair.and_then(|pair| u8::from_str_radix(pair, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Compare two opaque values without letting how long the comparison took say
/// how much of one matched.
pub fn same_value(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        difference |= a ^ b;
    }
    difference == 0
}

// ------------------------------------------------------------ the callback

/// What came back on the loopback socket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Callback {
    pub code: Option<String>,
    pub state: Option<String>,
    /// The error code from RFC 6749, such as `access_denied`.
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl Callback {
    /// Whether this request carries an answer at all, as opposed to being a
    /// browser asking for a favicon or opening a connection speculatively.
    fn is_an_answer(&self) -> bool {
        self.code.is_some() || self.error.is_some()
    }
}

/// Pull the answer out of a browser's request line.
///
/// Takes the whole first line, `GET /callback?... HTTP/1.1`, because that is
/// the shape of the thing actually read off the socket.
pub fn parse_callback(request_line: &str) -> Callback {
    let mut callback = Callback::default();

    let Some(target) = request_line.split_whitespace().nth(1) else {
        return callback;
    };
    let Some((_, query)) = target.split_once('?') else {
        return callback;
    };

    for pair in query.split('&') {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, decode(value)),
            None => (pair, String::new()),
        };
        match key {
            "code" => callback.code = Some(value),
            "state" => callback.state = Some(value),
            "error" => callback.error = Some(value),
            "error_description" => callback.error_description = Some(value),
            _ => {}
        }
    }

    callback
}

/// The page the browser is left showing.
///
/// Self contained on purpose: this is served from a socket that closes a moment
/// later, so anything fetched from elsewhere would arrive after the server had
/// gone, and anything fetched from the network would be a request the user
/// never asked this application to make.
fn finished_page(heading: &str, detail: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Alterion Open Project</title><style>\
         :root{{color-scheme:light dark}}\
         body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
         font:16px/1.5 system-ui,-apple-system,Segoe UI,sans-serif;background:#f6f6f8;color:#1b1b1f}}\
         @media(prefers-color-scheme:dark){{body{{background:#121216;color:#e8e8ec}}\
         .card{{background:#1c1c22 !important;border-color:#2c2c34 !important}}}}\
         .card{{max-width:26rem;padding:2rem 2.25rem;border:1px solid #e0e0e6;border-radius:12px;\
         background:#fff;text-align:center}}\
         h1{{margin:0 0 .5rem;font-size:1.25rem;font-weight:600}}\
         p{{margin:0;opacity:.75}}\
         </style></head><body><div class=\"card\"><h1>{heading}</h1><p>{detail}</p></div></body></html>"
    )
}

/// Answer a browser and close the connection.
fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let reply = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.flush();
}

/// Read the request line a browser sent.
fn request_line(stream: &mut TcpStream) -> Option<String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let Ok(read) = stream.read(&mut chunk) else {
            break;
        };
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        // The first line is all that is wanted, so there is no reason to keep
        // reading once a line break has arrived.
        if buffer.contains(&b'\n') || buffer.len() >= MAX_REQUEST_BYTES {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buffer);
    text.lines().next().map(|line| line.trim().to_string())
}

/// A loopback socket, and the port the operating system chose for it.
pub struct Loopback {
    listener: TcpListener,
    pub port: u16,
}

impl Loopback {
    /// Open one.
    ///
    /// Port zero means "whichever is free", and the port that comes back is
    /// what the redirect address is built from. Asking for a particular port
    /// would fail whenever something else held it, and worse, would let
    /// whatever held it first receive the authorization code.
    pub fn open() -> Result<Loopback, SignInError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| SignInError::NoLoopback(error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| SignInError::NoLoopback(error.to_string()))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| SignInError::NoLoopback(error.to_string()))?;
        Ok(Loopback { listener, port })
    }

    /// Where the identity provider is told to send the browser.
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Wait for the browser, and answer it.
    ///
    /// Anything that is not the answer, and browsers do send such things, gets
    /// a short refusal and the wait carries on. The wait itself is bounded, so
    /// a person who closed the tab and went to lunch does not leave this
    /// parked for the rest of the session.
    pub fn wait(&self, timeout: Duration) -> Result<Callback, SignInError> {
        let deadline = Instant::now() + timeout;

        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let Some(line) = request_line(&mut stream) else {
                        continue;
                    };
                    let callback = parse_callback(&line);
                    if !callback.is_an_answer() {
                        respond(&mut stream, "404 Not Found", "");
                        continue;
                    }

                    let page = match &callback.error {
                        None => finished_page(
                            "You are signed in",
                            "This tab can be closed. Alterion Open Project has the rest.",
                        ),
                        Some(_) => finished_page(
                            "Sign in did not finish",
                            "This tab can be closed. Alterion Open Project will say what happened.",
                        ),
                    };
                    respond(&mut stream, "200 OK", &page);
                    return Ok(callback);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(SignInError::Abandoned);
                    }
                    std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS));
                }
                Err(error) => return Err(SignInError::NoLoopback(error.to_string())),
            }
        }
    }
}

/// How long to wait for the browser, as a duration.
pub fn callback_timeout() -> Duration {
    Duration::from_secs(CALLBACK_TIMEOUT_SECONDS)
}

// ---------------------------------------------------------- the browser

/// The address the browser is sent to.
pub fn authorize_url(
    endpoints: &Endpoints,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    let separator = if endpoints.authorize.contains('?') { '&' } else { '?' };
    format!(
        "{}{separator}response_type=code&client_id={}&redirect_uri={}&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        endpoints.authorize,
        encode(client_id),
        encode(redirect_uri),
        encode(SCOPE),
        encode(state),
        encode(challenge),
    )
}

/// Hand a URL to whatever the desktop uses to open one.
///
/// The system browser, not an embedded one: RFC 8252 is emphatic about it, and
/// the reasons are practical as well. The user can see the address bar and the
/// certificate, their password manager works, and a session they already have
/// with the identity provider is reused rather than asked for again.
pub fn open_in_browser(url: &str) -> Result<(), SignInError> {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.quiet();
        command.arg(url);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.quiet();
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        // Through the URL handler rather than `cmd /c start`, which would take
        // the ampersands between the query parameters as its own punctuation
        // and open the browser at a truncated address.
        let mut command = Command::new("rundll32.exe");
        command.quiet();
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    };

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.quiet();
        command.arg(url);
        command
    };

    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|_| SignInError::NoBrowser(url.to_string()))
}

// ------------------------------------------------------------- the tokens

/// What the token endpoint hands back.
#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    /// Absent when a deployment issues access tokens only. Without one there is
    /// nothing to renew with and the session lasts until the token expires.
    #[serde(default)]
    pub refresh_token: String,
    /// Seconds from now. Defaulted rather than required, since a server is
    /// allowed to leave it out, in which case the token is treated as though it
    /// were about to expire and renewed on first use.
    #[serde(default)]
    pub expires_in: i64,
}

/// Trade an authorization code for tokens.
///
/// The verifier goes up here and nowhere else. It is what makes the code
/// useless to anything that intercepted it on the way back through the
/// browser.
pub fn exchange_code(
    endpoints: &Endpoints,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<Tokens, SignInError> {
    post_form(
        &endpoints.token,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", verifier),
        ],
        SignInError::CodeRefused,
    )
}

/// Renew with a refresh token.
///
/// The reply carries a new refresh token as well as a new access token: the
/// server spends the old one on use and treats a second attempt with it as a
/// stolen token, so whatever comes back has to be kept before anything else
/// happens.
pub fn refresh(
    endpoints: &Endpoints,
    client_id: &str,
    refresh_token: &str,
) -> Result<Tokens, SignInError> {
    post_form(
        &endpoints.token,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ],
        SignInError::SessionEnded,
    )
}

/// The header the sign in server binds a session to a machine with.
///
/// Hex, because that is what the server decodes, and a digest rather than the
/// components themselves, because the server only needs something that is the
/// same on this machine and different on another: there is no reason to put a
/// hardware inventory on the wire.
///
/// Left off entirely rather than sent empty when the machine cannot identify
/// itself. The server treats an empty fingerprint as a hard reject on purpose,
/// and sending one anyway would be asking for that answer while pretending to
/// have asked a different question. Nothing gets this far without an identity
/// in any case: [`crate::cloud::sign_in`] stops before the browser opens.
pub const FINGERPRINT_HEADER: &str = "X-Device-Fingerprint";

fn fingerprint() -> Option<String> {
    crate::cloud::device::fingerprint_hex().ok()
}

/// Post a form to the token endpoint and read what comes back.
///
/// Form encoded, not JSON: RFC 6749 says so and this server enforces it with a
/// 415 rather than a hint. `refused` is what a 400 or a 401 means for the grant
/// being asked for, which is not the same thing for a first sign in as it is
/// for a renewal.
fn post_form(
    url: &str,
    fields: &[(&str, &str)],
    refused: SignInError,
) -> Result<Tokens, SignInError> {
    let mut request = ureq::post(url);
    if let Some(fingerprint) = fingerprint() {
        request = request.header(FINGERPRINT_HEADER, fingerprint);
    }

    // Status codes are handled here rather than turned into errors by the
    // client, because a refusal and a server having fallen over need different
    // things said about them.
    let mut response = request
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .send_form(fields.iter().copied())
        .map_err(|error| SignInError::NoTokens(describe(&error)))?;

    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|_| SignInError::NoTokens("the reply did not finish arriving".into()))?;

    if status == 400 || status == 401 {
        return Err(refused);
    }
    if !(200..300).contains(&status) {
        return Err(SignInError::NoTokens(format!(
            "the server answered with status {status}"
        )));
    }

    let tokens: Tokens = serde_json::from_str(&body)
        .map_err(|_| SignInError::NoTokens("the server's reply made no sense".into()))?;
    if tokens.access_token.is_empty() {
        return Err(SignInError::NoTokens("the server sent an empty token".into()));
    }
    Ok(tokens)
}

/// Ask the identity provider to forget a token.
///
/// Best effort by design (RFC 7009 asks a server to answer the same way whether
/// or not the token was real), so this only reports a failure worth telling the
/// user about: signing out locally happens either way.
pub fn revoke(endpoints: &Endpoints, token: &str) -> Result<(), SignInError> {
    let Some(url) = &endpoints.revoke else {
        return Ok(());
    };

    let mut request = ureq::post(url);
    if let Some(fingerprint) = fingerprint() {
        request = request.header(FINGERPRINT_HEADER, fingerprint);
    }

    request
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .send_form([("token", token)])
        .map(|_| ())
        .map_err(|error| SignInError::NotRevoked(describe(&error)))
}

// ---------------------------------------------------------- who signed in

/// The claims this application reads about whoever signed in.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Claims {
    /// The stable identifier for the account. Everything else about a person
    /// can change; this is what two sign ins are compared by.
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub preferred_username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    /// Where the account's picture is, if the provider serves one. Standard
    /// OIDC, and absent from every reply until the provider starts sending it,
    /// which is why nothing downstream may assume there is one.
    #[serde(default)]
    pub picture: Option<String>,
}

/// Ask who the access token belongs to.
pub fn userinfo(endpoints: &Endpoints, access_token: &str) -> Result<Claims, SignInError> {
    let mut request = ureq::get(&endpoints.userinfo)
        .header("Authorization", format!("Bearer {access_token}"));
    if let Some(fingerprint) = fingerprint() {
        request = request.header(FINGERPRINT_HEADER, fingerprint);
    }

    let body = request
        .config()
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECONDS)))
        .build()
        .call()
        .map_err(|error| SignInError::NoAccount(describe(&error)))?
        .body_mut()
        .read_to_string()
        .map_err(|_| SignInError::NoAccount("the reply did not finish arriving".into()))?;

    let claims: Claims = serde_json::from_str(&body)
        .map_err(|_| SignInError::NoAccount("the server described the account in a way this build does not understand".into()))?;
    if claims.sub.is_empty() {
        return Err(SignInError::NoAccount(
            "the server named no account for that token".into(),
        ));
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --------------------------------------------------------------- PKCE

    #[test]
    fn the_challenge_matches_the_worked_example_in_rfc_7636() {
        // Appendix B of the specification. If this drifts, every sign in fails
        // at the token endpoint with nothing to say why.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn base64url_matches_the_rfc_4648_test_vectors() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64url_uses_the_url_alphabet_and_no_padding() {
        // Bytes chosen to land on the two characters that differ from plain
        // base64, which would otherwise have to be escaped in a query string.
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        assert!(!base64url(&[0u8; 32]).contains('='));
    }

    #[test]
    fn a_verifier_is_the_length_and_alphabet_the_rfc_allows() {
        let key = proof_key().expect("the system generator");
        assert_eq!(key.verifier.len(), 43, "inside the 43 to 128 range");
        assert!(
            key.verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')),
            "unreserved only: {}",
            key.verifier
        );
        assert_eq!(key.challenge, challenge_for(&key.verifier));
    }

    #[test]
    fn two_proof_keys_are_not_the_same_one() {
        // A verifier that repeated would make the proof no proof at all.
        let first = proof_key().expect("the system generator");
        let second = proof_key().expect("the system generator");
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(fresh_state().expect("state"), fresh_state().expect("state"));
    }

    // ---------------------------------------------------------- discovery

    const LIVE: &str = r#"{
        "issuer":"https://auth.example.org",
        "authorization_endpoint":"https://auth.example.org/api/oauth/authorize",
        "token_endpoint":"https://auth.example.org/api/oauth/token",
        "userinfo_endpoint":"https://auth.example.org/api/oauth/userinfo",
        "introspection_endpoint":"https://auth.example.org/api/oauth/introspect",
        "revocation_endpoint":"https://auth.example.org/api/oauth/revoke",
        "response_types_supported":["code"],
        "grant_types_supported":["authorization_code","refresh_token"],
        "code_challenge_methods_supported":["S256"],
        "token_endpoint_auth_methods_supported":["client_secret_post","none"],
        "scopes_supported":["openid","profile","email"]
    }"#;

    #[test]
    fn a_discovery_document_names_every_endpoint_the_flow_uses() {
        let endpoints =
            parse_discovery("https://auth.example.org", LIVE).expect("a valid document");
        assert_eq!(
            endpoints.authorize,
            "https://auth.example.org/api/oauth/authorize"
        );
        assert_eq!(endpoints.token, "https://auth.example.org/api/oauth/token");
        assert_eq!(
            endpoints.userinfo,
            "https://auth.example.org/api/oauth/userinfo"
        );
        assert_eq!(
            endpoints.revoke.as_deref(),
            Some("https://auth.example.org/api/oauth/revoke")
        );
    }

    #[test]
    fn a_trailing_slash_on_the_issuer_is_not_a_different_server() {
        // Someone typing the address into Options will put one there sooner or
        // later, and it must not read as a mix-up.
        assert!(parse_discovery("https://auth.example.org/", LIVE).is_ok());
    }

    #[test]
    fn nothing_is_hardcoded_about_where_the_endpoints_live() {
        // The whole point of discovery: another deployment, different paths.
        let elsewhere = LIVE
            .replace("https://auth.example.org", "https://id.example.org")
            .replace("/api/oauth/", "/oidc/v1/");
        let endpoints = parse_discovery("https://id.example.org", &elsewhere).expect("valid");
        assert_eq!(endpoints.token, "https://id.example.org/oidc/v1/token");
    }

    #[test]
    fn a_document_that_answers_for_another_issuer_is_refused() {
        // A server claiming to be somebody else is how a mix-up attack starts.
        let outcome = parse_discovery("https://id.example.org", LIVE);
        assert!(matches!(outcome, Err(SignInError::NoDiscovery { .. })));
    }

    #[test]
    fn a_server_without_s256_is_refused() {
        let plain = LIVE.replace(r#"["S256"]"#, r#"["plain"]"#);
        let outcome = parse_discovery("https://auth.example.org", &plain);
        assert!(matches!(outcome, Err(SignInError::NoDiscovery { .. })));
    }

    #[test]
    fn a_missing_endpoint_is_refused_rather_than_guessed_at() {
        let without = LIVE.replace("token_endpoint", "was_token_endpoint");
        assert!(parse_discovery("https://auth.example.org", &without).is_err());
    }

    #[test]
    fn an_unencrypted_endpoint_is_refused() {
        // An access token over plain HTTP is an access token anyone on the path
        // has.
        let plain = LIVE
            .replace(
                "\"token_endpoint\":\"https://",
                "\"token_endpoint\":\"http://",
            )
            .replace("http://auth.example.org/api/oauth/token", "http://tokens.example.org/t");
        let outcome = parse_discovery("https://auth.example.org", &plain);
        assert!(matches!(outcome, Err(SignInError::NoDiscovery { .. })));
    }

    #[test]
    fn a_self_hosted_deployment_on_loopback_is_allowed_over_plain_http() {
        // Developing against your own copy should not require a certificate.
        let local = LIVE.replace("https://auth.example.org", "http://127.0.0.1:8080");
        let endpoints = parse_discovery("http://127.0.0.1:8080", &local).expect("valid");
        assert_eq!(endpoints.token, "http://127.0.0.1:8080/api/oauth/token");
    }

    #[test]
    fn a_reply_that_is_not_a_discovery_document_says_so() {
        let outcome = parse_discovery("https://auth.example.org", "<html>not found</html>");
        assert!(matches!(outcome, Err(SignInError::NoDiscovery { .. })));
    }

    /// The issuer to reach in the tests that use a network.
    ///
    /// From the environment, because there is no address compiled into this
    /// application any more: the provider is self hosted, so whose copy to
    /// test against is the tester's answer to give.
    fn issuer_under_test() -> Option<String> {
        std::env::var("AOP_TEST_ISSUER").ok().filter(|v| !v.trim().is_empty())
    }

    /// Actually reaches the network, so it only runs when asked for:
    /// `AOP_TEST_ISSUER=https://auth.example.org cargo test -p aop-app -- --ignored discovers`.
    #[test]
    #[ignore = "reaches the network"]
    fn discovers_the_real_server() {
        // The one thing the offline tests cannot check: that the document this
        // parser was written against is the document the server sends.
        let Some(issuer) = issuer_under_test() else {
            return;
        };
        let endpoints = discover(&issuer).expect("the issuer under test");
        assert_eq!(endpoints.issuer, issuer);
        assert!(endpoints.authorize.starts_with(&issuer));
        assert!(endpoints.token.starts_with("https://"));
        assert!(endpoints.revoke.is_some(), "signing out needs this");
    }

    #[test]
    #[ignore = "reaches the network"]
    fn a_token_request_with_nothing_behind_it_is_refused_rather_than_accepted() {
        // Proves the request is shaped the way the server expects: a wrongly
        // encoded one comes back as an unsupported media type, not a refusal.
        let Some(issuer) = issuer_under_test() else {
            return;
        };
        let endpoints = discover(&issuer).expect("the issuer under test");
        let outcome = exchange_code(
            &endpoints,
            "not-a-real-client",
            "not-a-real-code",
            "http://127.0.0.1:49152/callback",
            "not-a-real-verifier",
        );
        assert!(
            matches!(outcome, Err(SignInError::CodeRefused)),
            "got {outcome:?}"
        );
    }

    #[test]
    fn the_well_known_path_is_appended_rather_than_replacing_one() {
        assert_eq!(
            discovery_url("https://example.com/auth"),
            "https://example.com/auth/.well-known/openid-configuration"
        );
        assert_eq!(
            discovery_url("https://example.com/auth/"),
            "https://example.com/auth/.well-known/openid-configuration"
        );
    }

    // ----------------------------------------------------------- callback

    #[test]
    fn a_successful_callback_carries_the_code_and_the_state() {
        let callback = parse_callback("GET /callback?code=abc123&state=xyz789 HTTP/1.1");
        assert_eq!(callback.code.as_deref(), Some("abc123"));
        assert_eq!(callback.state.as_deref(), Some("xyz789"));
        assert!(callback.error.is_none());
    }

    #[test]
    fn a_refusal_is_read_as_a_refusal_and_not_as_a_missing_code() {
        // The user pressing Cancel is the ordinary path, not a malfunction.
        let callback =
            parse_callback("GET /callback?error=access_denied&state=xyz789 HTTP/1.1");
        assert_eq!(callback.error.as_deref(), Some("access_denied"));
        assert!(callback.code.is_none());
        assert!(callback.is_an_answer(), "it is an answer, just not a yes");
    }

    #[test]
    fn an_error_description_is_decoded_back_into_a_sentence() {
        let callback = parse_callback(
            "GET /callback?error=invalid_scope&error_description=Scope%20not%20granted HTTP/1.1",
        );
        assert_eq!(
            callback.error_description.as_deref(),
            Some("Scope not granted")
        );
    }

    #[test]
    fn a_percent_encoded_state_comes_back_as_it_was_sent() {
        // The server percent encodes whatever it echoes, so anything outside
        // the unreserved set arrives escaped.
        let state = "a+b/c=d";
        let line = format!("GET /callback?code=x&state={} HTTP/1.1", encode(state));
        assert_eq!(parse_callback(&line).state.as_deref(), Some(state));
    }

    #[test]
    fn a_request_that_is_not_the_answer_is_not_mistaken_for_one() {
        // Browsers ask for a favicon the moment the page renders, and open
        // spare connections before that.
        for line in [
            "GET /favicon.ico HTTP/1.1",
            "GET /callback HTTP/1.1",
            "GET / HTTP/1.1",
            "",
            "nonsense",
        ] {
            assert!(!parse_callback(line).is_an_answer(), "line was {line:?}");
        }
    }

    #[test]
    fn percent_decoding_leaves_a_malformed_escape_alone() {
        // Better a value that fails the state check than a panic on the way to
        // making it.
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
        assert_eq!(decode("%2"), "%2");
    }

    #[test]
    fn encoding_leaves_the_unreserved_set_alone() {
        assert_eq!(encode("aZ09-._~"), "aZ09-._~");
        assert_eq!(encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(decode(&encode("a b&c=d")), "a b&c=d");
    }

    // ------------------------------------------------------------- state

    #[test]
    fn a_mismatched_state_is_not_the_same_value() {
        assert!(same_value("abc", "abc"));
        assert!(!same_value("abc", "abd"));
        assert!(!same_value("abc", "abcd"), "a prefix is not a match");
        assert!(!same_value("abc", ""), "nor is nothing");
    }

    // ---------------------------------------------------------- loopback

    #[test]
    fn the_port_is_whichever_one_was_free_and_never_a_fixed_one() {
        let first = Loopback::open().expect("a loopback socket");
        let second = Loopback::open().expect("a second loopback socket");
        assert_ne!(first.port, second.port, "a fixed port would collide");
        assert!(first.port > 0);
        assert_eq!(
            first.redirect_uri(),
            format!("http://127.0.0.1:{}/callback", first.port)
        );
    }

    #[test]
    fn waiting_for_a_browser_that_never_comes_gives_up() {
        let loopback = Loopback::open().expect("a loopback socket");
        let outcome = loopback.wait(Duration::from_millis(250));
        assert!(matches!(outcome, Err(SignInError::Abandoned)));
    }

    #[test]
    fn the_browser_is_answered_with_a_page_that_needs_nothing_else() {
        // Served from a socket that is about to close, so anything it asked for
        // afterwards would arrive at nothing.
        let page = finished_page("You are signed in", "This tab can be closed.");
        assert!(!page.contains("http://"), "no external resources");
        assert!(!page.contains("https://"));
        assert!(!page.contains("<script"));
        assert!(page.contains("You are signed in"));
    }

    #[test]
    fn the_callback_is_read_off_a_real_socket_and_answered() {
        let loopback = Loopback::open().expect("a loopback socket");
        let port = loopback.port;

        let browser = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            let _ = stream.write_all(
                b"GET /callback?code=the-code&state=the-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            );
            let mut reply = String::new();
            let _ = stream.read_to_string(&mut reply);
            reply
        });

        let callback = loopback
            .wait(Duration::from_secs(10))
            .expect("the browser answered");
        assert_eq!(callback.code.as_deref(), Some("the-code"));
        assert_eq!(callback.state.as_deref(), Some("the-state"));

        let reply = browser.join().expect("the browser thread");
        assert!(reply.starts_with("HTTP/1.1 200"), "got {reply:?}");
        assert!(reply.contains("close"), "the tab is told it can be closed");
    }

    // ------------------------------------------------------ authorize url

    fn sample_endpoints() -> Endpoints {
        parse_discovery("https://auth.example.org", LIVE).expect("valid")
    }

    #[test]
    fn the_authorize_url_carries_everything_the_server_needs() {
        let url = authorize_url(
            &sample_endpoints(),
            "alterion-open-project",
            "http://127.0.0.1:49152/callback",
            "the-state",
            "the-challenge",
        );
        assert!(url.starts_with("https://auth.example.org/api/oauth/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=alterion-open-project"));
        assert!(url.contains("code_challenge=the-challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=the-state"));
        assert!(
            url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A49152%2Fcallback"),
            "the redirect has to be escaped: {url}"
        );
        assert!(!url.contains("client_secret"), "this is a public client");
    }

    #[test]
    fn an_authorize_endpoint_that_already_has_a_query_is_extended_not_broken() {
        let mut endpoints = sample_endpoints();
        endpoints.authorize = "https://id.example.org/authorize?tenant=acme".into();
        let url = authorize_url(&endpoints, "app", "http://127.0.0.1:1/callback", "s", "c");
        assert!(url.contains("?tenant=acme&response_type=code"), "got {url}");
    }
}
