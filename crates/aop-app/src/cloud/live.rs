//! The live socket: other people's edits as they happen.
//!
//! One websocket per open plan, run on a thread of its own because reading a
//! socket blocks and the interface thread must never wait on a network. What
//! comes off it is queued, and the interface drains the queue on a timer; what
//! goes onto it is queued the same way in the other direction.
//!
//! ```text
//!   interface        queue          worker thread        server
//!   ---------        -----          -------------        ------
//!   send(presence) -> outgoing --->  write frame  ------>
//!   drain()        <- incoming <---  read frame   <------  change, presence,
//!                                                          joined, left
//! ```
//!
//! The first thing sent is `hello`, carrying the cursor. That is what makes a
//! reconnect safe: without it a socket that dropped for ten seconds resumes
//! having silently lost the edits made in them, and nothing ever says so.
//!
//! The token goes in the query string, which this one route accepts because a
//! websocket handshake cannot carry headers everywhere. It is never written to
//! a log, never put in an error message, and never held anywhere but the
//! request that opens the socket.

use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use aop_core::history::Change;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, Utf8Bytes};

use crate::cloud::collab::{self, CollabError};

/// How long a read waits before the worker looks at its own queues again.
///
/// Short enough that closing a plan does not leave a thread parked on a socket
/// for a noticeable time, long enough that an idle connection is not spinning.
const POLL: Duration = Duration::from_millis(400);

/// Where a planner's pointer is, said in the plan rather than on a screen.
///
/// A pixel position means nothing to anybody else: they have a different
/// window, a different zoom and a different scroll offset, so the only thing
/// that survives the trip is what the pointer is *over*. Both ends convert at
/// the edge, and this is the only form that goes on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "pane", rename_all = "snake_case")]
pub enum Pointer {
    /// Over the table: a row, and which column of it.
    Table { row: i64, column: u16 },
    /// Over the chart: a row, and how far along the timescale in minutes from
    /// the plan's start. Minutes rather than a date so it is cheap to send and
    /// survives a reschedule.
    Chart { row: i64, minutes: i64 },
}


/// Somebody else with this plan open.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Peer {
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub name: String,
    /// The row they have selected, if their copy bothered to say.
    #[serde(default)]
    pub row: Option<i64>,
    /// Where their pointer is. `None` is the ordinary case rather than a
    /// fault: a copy older than this one is still a peer, it simply has no
    /// pointer to draw. Absent in a message means unchanged and never "gone",
    /// so whoever holds one of these replaces it only with another.
    #[serde(default)]
    pub at: Option<Pointer>,
}

/// What arrived on the socket.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The connection is up. Says who else is here.
    Welcome { head: i64, peers: Vec<Peer> },
    /// Everything missed while away, sent before any live change so the order
    /// things are applied in is the log's order.
    Catchup { head: i64, changes: Vec<Change> },
    /// The cursor is not replayable, so nothing on this socket means anything
    /// until a whole plan has been fetched.
    Gap { head: i64 },
    /// One appended change, as it happened.
    Change { seq: i64, change: Change },
    Presence(Peer),
    Joined { name: String },
    Left { subject: String },
    /// The socket has ended, with what to say about it. Always the last thing
    /// a connection produces, so the interface has one place to notice that
    /// live editing has stopped.
    Closed(String),
}

/// What the interface asks the socket to send.
enum Outgoing {
    /// The row is always stated, because the server takes whatever a presence
    /// says about it. The pointer is stated only when there is a new one, so a
    /// selection moving does not blank everybody else's view of it.
    Presence {
        row: Option<i64>,
        at: Option<Pointer>,
    },
    Close,
}

/// One live connection, from the interface's side.
///
/// Dropping it closes the socket: the worker watches a flag that this sets on
/// the way out, so a plan being closed does not leave a thread reading a
/// socket nobody is listening to.
pub struct Live {
    incoming: Receiver<Incoming>,
    outgoing: Sender<Outgoing>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    /// Whether the socket has already reported itself finished, so the
    /// interface can stop asking.
    finished: bool,
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live").field("finished", &self.finished).finish()
    }
}

impl Live {
    /// Open a connection, and start reading it.
    ///
    /// Returns as soon as the thread is running rather than when the socket is
    /// up: connecting means DNS and a TLS handshake, and neither belongs in
    /// front of a button press. A connection that cannot be made arrives as
    /// [`Incoming::Closed`] like any other ending.
    pub fn connect(
        server: &str,
        token: &str,
        project: &str,
        after: i64,
        name: &str,
    ) -> Result<Live, CollabError> {
        // Checked here, before a thread exists, because a bad address is the
        // one failure worth refusing outright rather than reporting as a
        // connection that ended.
        let url = socket_url(&collab::base(server)?, project, token);

        let (to_ui, incoming) = channel();
        let (to_socket, from_ui) = channel();
        let stop = Arc::new(AtomicBool::new(false));

        let flag = Arc::clone(&stop);
        let hello = json!({ "type": "hello", "after": after, "name": name }).to_string();
        let worker = std::thread::Builder::new()
            .name("aop-live".into())
            .spawn(move || {
                let ending = pump(&url, hello, &to_ui, &from_ui, &flag);
                // The last word, whatever happened. Sending on a channel whose
                // other end has gone is fine and is what happens when a plan is
                // closed while the socket is still up.
                let _ = to_ui.send(Incoming::Closed(ending));
            })
            .map_err(|error| CollabError::NotReached {
                server: server.to_string(),
                why: format!("a connection could not be started: {error}"),
            })?;

        Ok(Live {
            incoming,
            outgoing: to_socket,
            stop,
            worker: Some(worker),
            finished: false,
        })
    }

    /// Everything that has arrived since the last look. Never blocks.
    pub fn drain(&mut self) -> Vec<Incoming> {
        let mut batch = Vec::new();
        while let Ok(message) = self.incoming.try_recv() {
            if matches!(message, Incoming::Closed(_)) {
                self.finished = true;
            }
            batch.push(message);
        }
        batch
    }

    /// Say where this planner is looking, so the others can show it.
    ///
    /// `at` is what the pointer is over, and `None` means "nothing new to say
    /// about it" rather than "it has gone". The protocol reads an absent
    /// pointer the same way, so a selection moving never blanks the pointer
    /// everybody else is drawing.
    pub fn looking_at(&self, row: Option<i64>, at: Option<Pointer>) {
        let _ = self.outgoing.send(Outgoing::Presence { row, at });
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.outgoing.send(Outgoing::Close);
        // Not joined. The worker wakes within one poll interval and closes the
        // socket itself; waiting for it here would put a network timeout in
        // front of whoever closed the plan.
        self.worker.take();
    }
}

/// The address of the live socket, token and all.
///
/// `ws` follows `http` and `wss` follows `https`, so a server reached over TLS
/// keeps its socket over TLS too, rather than quietly dropping to plaintext
/// for the one connection that carries every edit.
fn socket_url(base: &str, project: &str, token: &str) -> String {
    let base = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some(("http", rest)) => format!("ws://{rest}"),
        _ => base.to_string(),
    };
    format!("{base}/api/projects/{project}/live?access_token={token}")
}

/// The worker's whole life: connect, greet, then relay in both directions.
///
/// Returns what to say about the ending, which is the only thing that comes
/// back: everything else has already gone down the channel.
fn pump(
    url: &str,
    hello: String,
    to_ui: &Sender<Incoming>,
    from_ui: &Receiver<Outgoing>,
    stop: &AtomicBool,
) -> String {
    let mut socket = match tungstenite::connect(url) {
        Ok((socket, _)) => socket,
        // The address carries the token, so it must not reach the message: a
        // connection error's text is the sort of thing that gets pasted into a
        // bug report.
        Err(error) => return format!("Live editing could not start: {}", plainly(&error)),
    };

    // A read that never returns is a thread that never notices the plan was
    // closed, so the socket is asked to give up regularly and be asked again.
    if let Err(error) = set_read_timeout(&mut socket, POLL) {
        return format!("Live editing could not start: {error}");
    }

    if socket.send(Message::Text(hello.into())).is_err() {
        return "Live editing stopped before it started: the server closed the connection.".into();
    }

    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return "Live editing stopped.".into();
        }

        match from_ui.try_recv() {
            Ok(Outgoing::Presence { row, at }) => {
                let mut message = json!({ "type": "presence", "row": row });
                // Only when there is one. The key being absent is what the
                // protocol reads as "unchanged", and writing `null` instead
                // would blank the pointer rather than leave it alone.
                if let Some(at) = at
                    && let Ok(at) = serde_json::to_value(at)
                    && let Some(fields) = message.as_object_mut()
                {
                    fields.insert("at".into(), at);
                }
                if socket.send(Message::Text(message.to_string().into())).is_err() {
                    return "Live editing stopped: the connection was lost.".into();
                }
            }
            Ok(Outgoing::Close) | Err(TryRecvError::Disconnected) => {
                let _ = socket.close(None);
                return "Live editing stopped.".into();
            }
            Err(TryRecvError::Empty) => {}
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(message) = read_message(&text)
                    && to_ui.send(message).is_err()
                {
                    // Nobody is listening any more, which means the plan was
                    // closed while this was mid-frame.
                    let _ = socket.close(None);
                    return "Live editing stopped.".into();
                }
            }
            Ok(Message::Close(_)) => {
                return "Live editing stopped: the server closed the connection.".into();
            }
            // Pings, pongs and binary frames are not part of this protocol.
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return "Live editing stopped: the connection was closed.".into();
            }
            Err(error) => {
                return format!("Live editing stopped: {}.", plainly(&error));
            }
        }
    }
}

/// Ask the socket to stop waiting after a while, whichever transport it is on.
fn set_read_timeout(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    timeout: Duration,
) -> Result<(), String> {
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(plain) => plain,
        MaybeTlsStream::Rustls(tls) => &mut tls.sock,
        // The enum is open ended, and a transport this build does not know
        // about is not a reason to refuse the connection: it only means the
        // worker parks on the read until the socket itself ends.
        _ => return Ok(()),
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| format!("the connection could not be set up: {error}"))
}

/// A socket failure in words, with nothing in them that came off the wire.
///
/// The address carries the access token, so anything that might quote it is
/// replaced rather than passed along.
fn plainly(error: &tungstenite::Error) -> String {
    match error {
        tungstenite::Error::Io(io) => match io.kind() {
            ErrorKind::ConnectionRefused => "nothing is answering at that address".into(),
            ErrorKind::TimedOut => "the server did not answer in time".into(),
            other => format!("the connection failed ({other})"),
        },
        tungstenite::Error::Http(response) => {
            format!("the server answered with status {}", response.status().as_u16())
        }
        tungstenite::Error::Tls(_) => "the server's certificate could not be trusted".into(),
        tungstenite::Error::Url(_) => "the server address is not a valid one".into(),
        _ => "the connection failed".into(),
    }
}

/// Everything the server can say, in one shape.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Welcome {
        head: i64,
        #[serde(default)]
        peers: Vec<Peer>,
    },
    Catchup {
        head: i64,
        #[serde(default)]
        changes: Vec<Change>,
    },
    Gap {
        head: i64,
    },
    Change {
        seq: i64,
        change: Change,
    },
    Presence(Peer),
    Joined {
        #[serde(default)]
        subject: String,
        #[serde(default)]
        name: String,
    },
    Left {
        #[serde(default)]
        subject: String,
    },
    Pong,
    /// A complaint from the server. Carried as a shape rather than as words:
    /// the socket carrying on is the report, and what a planner can do about
    /// it is nothing.
    Error,
}

/// Turn one message into something the interface can act on.
///
/// Anything unrecognised is dropped rather than reported. A newer server
/// sending a message this build has never heard of is not a fault, and tearing
/// down a working socket over it would be.
fn read_message(text: &Utf8Bytes) -> Option<Incoming> {
    match serde_json::from_str::<ServerMessage>(text.as_str()).ok()? {
        ServerMessage::Welcome { head, peers } => Some(Incoming::Welcome { head, peers }),
        ServerMessage::Catchup { head, changes } => Some(Incoming::Catchup { head, changes }),
        ServerMessage::Gap { head } => Some(Incoming::Gap { head }),
        ServerMessage::Change { seq, change } => Some(Incoming::Change { seq, change }),
        ServerMessage::Presence(peer) => Some(Incoming::Presence(peer)),
        ServerMessage::Joined { subject, name } => Some(Incoming::Joined {
            name: if name.trim().is_empty() { subject } else { name },
        }),
        ServerMessage::Left { subject } => Some(Incoming::Left { subject }),
        // A keepalive answer and a server side complaint are both things the
        // planner can do nothing about; the socket carrying on is the report.
        ServerMessage::Pong | ServerMessage::Error => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<Incoming> {
        read_message(&Utf8Bytes::from(text))
    }

    #[test]
    fn a_socket_over_tls_stays_over_tls() {
        // Dropping to plaintext for the one connection carrying every edit
        // would be the worst place to do it.
        let url = socket_url("https://sync.example.org", "a-project", "a-token");
        assert!(url.starts_with("wss://sync.example.org/api/projects/a-project/live"));
        assert!(socket_url("http://localhost:8090", "p", "t").starts_with("ws://localhost:8090/"));
    }

    #[test]
    fn a_welcome_says_who_else_is_here() {
        let message = parse(
            r#"{"type":"welcome","head":45,
                "peers":[{"subject":"0198","name":"Grace","row":12}]}"#,
        );
        let Some(Incoming::Welcome { head, peers }) = message else {
            panic!("a welcome is the answer to hello");
        };
        assert_eq!(head, 45);
        assert_eq!(peers[0].name, "Grace");
        assert_eq!(peers[0].row, Some(12));
    }

    #[test]
    fn a_catchup_carries_the_changes_missed_while_away() {
        let message = parse(
            r#"{"type":"catchup","head":44,
                "changes":[{"id":43,"at":"2026-08-18T09:00:00","author":"Grace",
                            "script":"indent();","summary":"Indented a task"}]}"#,
        );
        let Some(Incoming::Catchup { changes, .. }) = message else {
            panic!("a catchup precedes the live stream");
        };
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn a_gap_on_the_socket_is_told_apart_from_a_change() {
        assert_eq!(parse(r#"{"type":"gap","head":45}"#), Some(Incoming::Gap { head: 45 }));
    }

    #[test]
    fn somebody_joining_without_a_name_is_shown_by_their_account() {
        // A blank where a name belongs reads as something having gone wrong.
        let message = parse(r#"{"type":"joined","subject":"0198f0c2","name":""}"#);
        assert_eq!(message, Some(Incoming::Joined { name: "0198f0c2".into() }));
    }

    #[test]
    fn a_message_this_copy_has_never_heard_of_does_not_end_the_connection() {
        // A newer server saying something new is not a fault.
        assert_eq!(parse(r#"{"type":"rearranged","seq":9}"#), None);
        assert_eq!(parse("not json at all"), None);
    }

    #[test]
    fn a_keepalive_is_not_passed_on_as_something_to_show() {
        assert_eq!(parse(r#"{"type":"pong"}"#), None);
    }
}
