//! Giving a link to the copy that is already running.
//!
//! Clicking an `aop://` link while the application is open must open the plan
//! in that window, not start a second one. Two copies of the same plan, each
//! with its own change log and its own sync cursor, is exactly the situation
//! the sync protocol's `ahead` case exists to detect, and starting one on
//! purpose would be manufacturing it.
//!
//! ```text
//!   second launch                     first copy
//!   -------------                     ----------
//!   read the port file
//!   connect 127.0.0.1:<port>  ----->  listener thread
//!   send  AOP1 <link>         ----->  parse, queue
//!                             <-----  one byte, meaning "it is mine now"
//!   exit
//! ```
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
//! **What may be sent.** Only a link, only from this machine, and only up to a
//! sensible length. The listener binds the loopback interface, so nothing off
//! the machine can reach it. What arrives is still not acted on: it is put in
//! front of the person, who sees which server it names before anything is
//! asked of that server.

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

/// The longest line the listener will read.
///
/// A link is a scheme, a host and a UUID. Anything much longer is not one, and
/// reading it would be reading whatever somebody felt like sending.
const LONGEST: u64 = 2048;

/// Links handed over by later launches, waiting to be looked at.
///
/// A queue rather than a callback because the plan may only be written where
/// the interface runs, and the listener is on a thread of its own. Exactly the
/// arrangement the live socket uses, for exactly the same reason.
type Inbox = Arc<Mutex<Vec<String>>>;
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

/// Offer a link to whichever copy is already running, and take charge if none
/// is.
///
/// `link` is what a later launch was asked to open. A launch with nothing to
/// hand over never tries: opening a second window on purpose is a thing people
/// do, and it is only a link that must not be allowed to start one.
pub fn claim(link: Option<&str>) -> Claim {
    if let Some(link) = link
        && offer(link)
    {
        return Claim::HandedOver;
    }
    listen();
    Claim::Run
}

/// Everything handed over since the last look. Never blocks.
pub fn arrivals() -> Vec<String> {
    let Some(inbox) = ARRIVALS.get() else {
        return Vec::new();
    };
    let mut held = inbox.lock().unwrap_or_else(PoisonError::into_inner);
    std::mem::take(&mut *held)
}

/// Try to give the link to a copy that is already running.
///
/// False for every way that can fail, and they all mean the same thing to the
/// caller: nobody took it, so this launch is the one that opens a window.
fn offer(link: &str) -> bool {
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
        || writeln!(stream, "{GREETING}{link}").is_err()
        || stream.flush().is_err()
    {
        return false;
    }

    // The answer is the whole point of waiting. Without it a launch cannot
    // tell a copy that took the link from a port the operating system has
    // since given to something else, and the difference is whether the link
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

/// Read one offered link, answer for it, and queue it.
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

    let Some(link) = read_offer(&line) else {
        return;
    };
    inbox
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(link);
    // Only now, because this is a promise: a launch that hears it exits, and
    // saying it before the link is queued would lose the link.
    let mut stream = reader.into_inner().into_inner();
    let _ = stream.write_all(&[TAKEN]);
    let _ = stream.flush();
}

/// The link inside an offered line, if the line is one.
///
/// Split out so the checking can be tested without a socket. It refuses
/// anything that is not this application's greeting followed by something that
/// reads as a link, which keeps whatever else may be on that port out of the
/// interface entirely.
fn read_offer(line: &str) -> Option<String> {
    let rest = line.trim_end_matches(['\r', '\n']).strip_prefix(GREETING)?;
    crate::cloud::share::read(rest).map(|_| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "aop://sync.example.org/plan/0198f0c2-1111-4222-8333-444455556666";

    #[test]
    fn a_greeting_carrying_a_link_is_taken() {
        assert_eq!(read_offer(&format!("{GREETING}{LINK}\n")), Some(LINK.to_string()));
        assert_eq!(read_offer(&format!("{GREETING}{LINK}\r\n")), Some(LINK.to_string()));
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
    fn a_greeting_carrying_something_other_than_a_link_is_refused() {
        // Nothing is opened from this that was not a link, whatever a
        // greeting was put in front of it.
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
    fn a_link_offered_to_a_listening_copy_is_queued_and_answered_for() {
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
        assert_eq!(held.as_slice(), [LINK.to_string()]);
    }
}
