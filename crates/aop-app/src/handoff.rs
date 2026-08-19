//! Giving a plan to the copy that is already running.
//!
//! Clicking an `aop://` link, or double clicking a plan file, while the
//! application is open must open that plan in the window that is already open
//! and not start a second one. Two copies of the same plan, each with its own
//! change log and its own sync cursor, is exactly the situation the sync
//! protocol's `ahead` case exists to detect, and starting one from the file
//! manager would be manufacturing it.
//!
//! ```text
//!   second launch                     first copy
//!   -------------                     ----------
//!   read the port file
//!   connect 127.0.0.1:<port>  ----->  listener thread
//!   send  AOP1 <what>         ----->  parse, queue
//!                             <-----  one byte, meaning "it is mine now"
//!   exit
//! ```
//!
//! **A link and a path go the same way, told apart by scheme.** A link is sent
//! as it stands, and a path is sent as `file://` and then the path, percent
//! encoded. One channel, and the greeting says which of the two arrived, so
//! the window can ask the right question about it. A bare path with no scheme
//! is still refused: whatever else may be listening on a port the operating
//! system has since handed to somebody else, this only ever acts on something
//! that says what it is.
//!
//! **Why a socket and not a lock file.** A lock file left behind by a process
//! that was killed is a lock file that makes the application unstartable, and
//! every scheme for detecting a stale one is a guess about process ids that
//! another program may since have been given. There is nothing to go stale
//! here: the file holds a port number and nothing else, and a port nobody
//! answers on is simply a failed connection. The launch that finds one binds a
//! fresh port, writes it down over the old one, and carries on as the copy in
//! charge. The worst a stale file costs is one refused connection.
//!
//! The greeting is checked in both directions for the same reason. The
//! operating system may have given that port to something else entirely, and a
//! link should not be posted into an unrelated program's socket.
//!
//! **What may be sent.** Only a link or a path, only from this machine, and
//! only up to a sensible length. The listener binds the loopback interface, so
//! nothing off the machine can reach it. What arrives is still not acted on by
//! itself: a link is put in front of the person, who sees which server it
//! names before anything is asked of that server, and a path goes through the
//! same question about unsaved work that opening a file has always asked.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

/// What both ends say first, so neither talks to the wrong program.
const GREETING: &str = "AOP1 ";

/// What the running copy answers with once it has the link.
const TAKEN: u8 = b'y';

/// How long a launch waits on the copy that may be running.
///
/// This is a connection to this machine, so it is either answered at once or
/// not answered at all. Waiting longer would only make a stale port file into
/// a pause before the window opens.
const PATIENCE: Duration = Duration::from_millis(400);

/// The scheme a handed over file path is sent under.
///
/// Its own scheme rather than a bare path, so that the greeting still says
/// what the rest of the line is. Percent encoded after it, because this is a
/// line based protocol and a file name is allowed to contain a line break.
const FILE: &str = "file://";

/// The longest line the listener will read.
///
/// A link is a scheme, a host and a UUID, and a path is a path. Anything much
/// longer is neither, and reading it would be reading whatever somebody felt
/// like sending.
const LONGEST: u64 = 2048;

/// What one launch can hand to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handed {
    /// A plan on a server somewhere, which has to be fetched.
    Link(String),
    /// A plan file on this machine, already resolved to an absolute path.
    Path(PathBuf),
}

impl Handed {
    /// What a launch was asked to open, if it was asked to open anything.
    ///
    /// A link is told from a path by its scheme and by nothing else, because
    /// guessing would be guessing about whether a network request gets made,
    /// and the desktop hands over a URL and a path through the same argument.
    ///
    /// A path is resolved here, in the process that was given it. A relative
    /// path means nothing to the copy that is already running: it has its own
    /// working directory, very likely somebody's home rather than the folder
    /// the file manager was looking at, and sending one would open a different
    /// file or none at all.
    pub fn from_argument(argument: &str) -> Option<Handed> {
        if crate::cloud::share::looks_like_a_link(argument) {
            return Some(Handed::Link(argument.to_string()));
        }
        // The desktop entry is registered with `%u`, and a file manager is
        // free to hand a local file over as a URL rather than as a path.
        // Reading only one of the two shapes means the association fires and
        // nothing opens, which looks exactly like it did not fire.
        let argument = match argument.strip_prefix(FILE) {
            Some(encoded) => std::borrow::Cow::Owned(decode(encoded)?),
            None => std::borrow::Cow::Borrowed(argument),
        };
        let path = std::fs::canonicalize(argument.as_ref()).ok()?;
        path.is_file().then_some(Handed::Path(path))
    }

    /// How this goes over the wire.
    ///
    /// `None` for a path that is not valid text, which no encoding here can
    /// fix. Nothing is lost by it: the launch that has the path simply opens
    /// its own window, which is what happened before any of this existed.
    fn wire(&self) -> Option<String> {
        match self {
            Handed::Link(link) => Some(link.clone()),
            Handed::Path(path) => path.to_str().map(|text| format!("{FILE}{}", encode(text))),
        }
    }
}

/// Percent encode everything that is not plainly safe in one line of text.
///
/// Deliberately more than a URL would need. The point is not to be a correct
/// file URL, which nothing else reads: it is that whatever a file is called,
/// including a name with a line break or a space in it, survives a line based
/// protocol and comes back out byte for byte.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The other half of [`encode`], which has to be exact or it names another
/// file. `None` for anything that is not something this wrote.
fn decode(text: &str) -> Option<String> {
    let raw = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%' {
            let digits = text.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(digits, 16).ok()?);
            at += 3;
        } else {
            out.push(raw[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What later launches have handed over, waiting to be looked at.
///
/// A queue rather than a callback because the plan may only be written where
/// the interface runs, and the listener is on a thread of its own. Exactly the
/// arrangement the live socket uses, for exactly the same reason.
type Inbox = Arc<Mutex<Vec<Handed>>>;
static ARRIVALS: OnceLock<Inbox> = OnceLock::new();

/// Where the port of the copy in charge is written down.
fn port_path() -> Option<PathBuf> {
    crate::settings::config_root().map(|dir| dir.join("running.port"))
}

/// What a launch should do next.
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// Carry on and open a window. Anything a later launch hands over will
    /// arrive through [`arrivals`].
    Run,
    /// A copy already running has taken the link, so this one has no work.
    HandedOver,
}

/// Offer a plan to whichever copy is already running, and take charge if none
/// is.
///
/// `wanted` is what a later launch was asked to open. A launch with nothing to
/// hand over never tries: opening a second empty window on purpose is a thing
/// people do, and it is only a plan that must not be allowed to start one.
pub fn claim(wanted: Option<&Handed>) -> Claim {
    if let Some(wanted) = wanted
        && offer(wanted)
    {
        return Claim::HandedOver;
    }
    listen();
    Claim::Run
}

/// Everything handed over since the last look. Never blocks.
pub fn arrivals() -> Vec<Handed> {
    let Some(inbox) = ARRIVALS.get() else {
        return Vec::new();
    };
    let mut held = inbox.lock().unwrap_or_else(PoisonError::into_inner);
    std::mem::take(&mut *held)
}

/// Try to give it to a copy that is already running.
///
/// False for every way that can fail, and they all mean the same thing to the
/// caller: nobody took it, so this launch is the one that opens a window.
fn offer(wanted: &Handed) -> bool {
    let Some(wire) = wanted.wire() else {
        return false;
    };
    let Some(port) = port_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| *port != 0)
    else {
        return false;
    };

    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, PATIENCE) else {
        return false;
    };
    if stream.set_read_timeout(Some(PATIENCE)).is_err()
        || stream.set_write_timeout(Some(PATIENCE)).is_err()
        || writeln!(stream, "{GREETING}{wire}").is_err()
        || stream.flush().is_err()
    {
        return false;
    }

    // The answer is the whole point of waiting. Without it a launch cannot
    // tell a copy that took the plan from a port the operating system has
    // since given to something else, and the difference is whether the plan
    // was opened or quietly went nowhere.
    let mut answer = [0u8; 1];
    stream.read_exact(&mut answer).is_ok() && answer[0] == TAKEN
}

/// Take charge: bind a port, write it down, and read what arrives on it.
///
/// Quiet on failure, and deliberately. Not being reachable means later
/// launches open their own window, which is what happened before any of this
/// existed. It is not worth refusing to start over.
fn listen() {
    let Ok(server) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
        return;
    };
    let Ok(address) = server.local_addr() else {
        return;
    };

    if let Some(path) = port_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Over whatever was there. A port file naming a copy that has gone is
        // the ordinary case after a crash, and this is where it is put right.
        if std::fs::write(&path, address.port().to_string()).is_err() {
            return;
        }
    }

    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
    // Only the first claim in a process is the one in charge. A second would
    // leave the first listener bound with nobody draining it.
    if ARRIVALS.set(Arc::clone(&inbox)).is_err() {
        return;
    }

    let started = std::thread::Builder::new()
        .name("aop-handoff".into())
        .spawn(move || {
            for stream in server.incoming().flatten() {
                take(stream, &inbox);
            }
        });
    let _ = started;
}

/// Read one offer, answer for it, and queue it.
fn take(stream: TcpStream, inbox: &Inbox) {
    // A caller that connects and then says nothing must not hold the thread
    // that every other launch is waiting on.
    if stream.set_read_timeout(Some(PATIENCE)).is_err() {
        return;
    }
    let mut line = String::new();
    let mut reader = BufReader::new(stream).take(LONGEST);
    if reader.read_line(&mut line).is_err() {
        return;
    }

    let Some(handed) = read_offer(&line) else {
        return;
    };
    inbox
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(handed);
    // Only now, because this is a promise: a launch that hears it exits, and
    // saying it before the plan is queued would lose the plan.
    let mut stream = reader.into_inner().into_inner();
    let _ = stream.write_all(&[TAKEN]);
    let _ = stream.flush();
}

/// What an offered line is offering, if the line is one of ours at all.
///
/// Split out so the checking can be tested without a socket. It refuses
/// anything that is not this application's greeting followed by something that
/// says which of the two things it is, which keeps whatever else may be on
/// that port out of the interface entirely.
fn read_offer(line: &str) -> Option<Handed> {
    let rest = line.trim_end_matches(['\r', '\n']).strip_prefix(GREETING)?;
    if let Some(encoded) = rest.strip_prefix(FILE) {
        let path = PathBuf::from(decode(encoded)?);
        // Absolute or nothing. A relative path here would be resolved against
        // this process's working directory rather than the one that sent it,
        // which is how a handed over file becomes a different file.
        return path.is_absolute().then_some(Handed::Path(path));
    }
    crate::cloud::share::read(rest).map(|_| Handed::Link(rest.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "aop://sync.example.org/plan/0198f0c2-1111-4222-8333-444455556666";

    #[test]
    fn a_greeting_carrying_a_link_is_taken() {
        let taken = Some(Handed::Link(LINK.to_string()));
        assert_eq!(read_offer(&format!("{GREETING}{LINK}\n")), taken);
        assert_eq!(read_offer(&format!("{GREETING}{LINK}\r\n")), taken);
    }

    #[test]
    fn a_greeting_carrying_a_path_is_taken_as_a_path() {
        // The whole point of the change. A file has to travel the same way a
        // link does, or double clicking one while this is open starts a second
        // copy of the application on the same plan.
        let offered = Handed::Path(PathBuf::from("/home/ada/plans/bridge.aprj"));
        let wire = offered.wire().expect("a path that is text");
        assert_eq!(read_offer(&format!("{GREETING}{wire}\n")), Some(offered));
    }

    #[test]
    fn a_file_name_survives_whatever_it_is_called() {
        // The reason a path is encoded rather than sent as it stands: this is
        // a line based protocol, and a file name may contain a line break, a
        // space, a percent sign or a hash. Any of those arriving as itself
        // opens a different file, or none.
        for awkward in [
            "/home/ada/plans/bridge.aprj",
            "/home/ada/Plans and Notes/site survey #2 (50% done).aprj",
            "/home/ada/pl\nans/odd.aprj",
            "/home/ada/planer/br\u{fc}cke \u{2013} entwurf.aprj",
        ] {
            let offered = Handed::Path(PathBuf::from(awkward));
            let wire = offered.wire().expect("a path that is text");
            assert!(!wire.contains('\n'), "a newline would end the line early");
            assert_eq!(read_offer(&format!("{GREETING}{wire}\n")), Some(offered));
        }
    }

    #[test]
    fn a_path_is_resolved_where_it_was_given_and_a_file_url_is_read_as_a_path() {
        // Two things at once, because they are the same claim: whatever shape
        // the desktop hands a local file over in, and whichever directory it
        // was looking at, what goes on the wire is one absolute path.
        let dir = std::env::temp_dir().join(format!("aop-handoff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory to work in");
        let file = dir.join("bridge.aprj");
        std::fs::write(&file, b"not really a plan").expect("a file to point at");
        let real = std::fs::canonicalize(&file).expect("it is there");

        let plain = Handed::from_argument(&file.to_string_lossy());
        assert_eq!(plain, Some(Handed::Path(real.clone())));

        let url = format!("{FILE}{}", encode(&file.to_string_lossy()));
        assert_eq!(Handed::from_argument(&url), Some(Handed::Path(real)));

        // And a name that points at nothing is nobody's to hand over.
        assert_eq!(
            Handed::from_argument(&dir.join("missing.aprj").to_string_lossy()),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_path_is_refused_by_the_copy_that_receives_it() {
        // It would be resolved against this process's working directory, which
        // is not the one the launch that sent it was looking at. Resolving
        // happens where the path came from, and anything that arrives here
        // unresolved is not something this wrote.
        let wire = format!("{FILE}{}", encode("plans/bridge.aprj"));
        assert_eq!(read_offer(&format!("{GREETING}{wire}\n")), None);
    }

    #[test]
    fn anything_that_is_not_this_application_talking_is_refused() {
        // The operating system may have given that port to something else
        // entirely since it was written down, so what arrives on it is not
        // assumed to be a launch of this application.
        assert_eq!(read_offer(LINK), None);
        assert_eq!(read_offer("GET / HTTP/1.1\r\n"), None);
        assert_eq!(read_offer(""), None);
    }

    #[test]
    fn a_greeting_carrying_something_that_says_nothing_about_itself_is_refused() {
        // Both things this opens say which they are. A bare path does not, and
        // is refused for that reason rather than because it is a path: the
        // port may since have been given to another program, and what arrives
        // on it has to identify itself before anything acts on it.
        assert_eq!(read_offer(&format!("{GREETING}/home/ada/plans/bridge.aprj\n")), None);
        assert_eq!(read_offer(&format!("{GREETING}https://example.org/\n")), None);
        assert_eq!(read_offer(GREETING), None);
    }

    #[test]
    fn a_port_nobody_answers_on_is_simply_nobody_to_hand_over_to() {
        // The stale lock problem, and its whole answer: the file names a port,
        // a port nobody answers on refuses the connection, and the launch that
        // found it carries on and takes charge itself.
        let dead = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port");
        let port = dead.local_addr().expect("its own address").port();
        drop(dead);
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        assert!(TcpStream::connect_timeout(&address, PATIENCE).is_err());
    }

    #[test]
    fn something_offered_to_a_listening_copy_is_queued_and_answered_for() {
        let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port");
        let port = server.local_addr().expect("its own address").port();
        let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
        let held = Arc::clone(&inbox);
        let waiter = std::thread::spawn(move || {
            let stream = server.incoming().flatten().next().expect("one caller");
            take(stream, &held);
        });

        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&address, PATIENCE).expect("it is listening");
        writeln!(stream, "{GREETING}{LINK}").expect("the greeting goes");
        let mut answer = [0u8; 1];
        stream.read_exact(&mut answer).expect("an answer comes back");

        waiter.join().expect("the reader finishes");
        assert_eq!(answer[0], TAKEN);
        let held = inbox.lock().expect("nothing panicked holding it");
        assert_eq!(held.as_slice(), [Handed::Link(LINK.to_string())]);
    }
}
