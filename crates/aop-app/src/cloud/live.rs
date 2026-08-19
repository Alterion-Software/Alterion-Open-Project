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

use std::cell::{Cell as MutCell, RefCell};
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aop_core::history::Change;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, Utf8Bytes};

use crate::cloud::collab::{self, CollabError};

/// How long a read waits before the worker looks at its own queues again.
///
/// This is also the longest anything waits to be *written*, which is what
/// sets it: the worker alternates between reading and draining what the
/// interface has queued, so a pointer position sitting behind a four hundred
/// millisecond read arrives too late to follow. Twenty wake ups a second on a
/// socket nobody is using is a cheaper thing to pay for.
const POLL: Duration = Duration::from_millis(50);

/// The shortest gap between two presence messages.
///
/// Mouse movement turned into network traffic is waste, and every message
/// costs the receiving copy a redraw, so this is a cap rather than a rate:
/// nothing is sent unless the position actually changed, and then no more
/// often than this. The receiving side glides between the positions it is
/// told about, so eight a second looks continuous rather than like eight
/// jumps. Anything that is not a pointer move, such as a cell being opened,
/// goes straight out: those are rare and waiting on them would be a delay
/// with nothing to show for it.
const PRESENCE_EVERY: Duration = Duration::from_millis(120);

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


/// A cell somebody has open for editing, in the plan rather than on a screen.
///
/// Travels as a row and a column for the same reason a pointer does: the
/// rectangle it occupies is a fact about one window and nobody else's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub row: i64,
    pub column: u16,
}

/// Everything ephemeral about one planner, in one shape.
///
/// The ephemeral channel: broadcast and forgotten, never stored, never given
/// a seq, and gone when its connection is. Keeping it in one struct is what
/// lets the socket work out for itself what has actually changed since it
/// last said anything, so no caller has to remember to compare.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presence {
    /// The row this planner has selected.
    pub row: Option<i64>,
    /// What their pointer is over. `None` means "nothing new to say", never
    /// "it has gone": the protocol has no way to say a pointer has gone.
    pub at: Option<Pointer>,
    /// The cell they have open, if any.
    pub editing: Option<Cell>,
    /// What they have typed into it and not committed. Never goes anywhere
    /// near the log: an abandoned edit must not become a permanent record of
    /// something that never happened.
    pub draft: Option<String>,
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
    /// Where their face is, if their provider serves one. Already checked by
    /// the time it is in here: [`read_message`] drops an address this copy
    /// would not load, so nothing downstream has to remember to.
    #[serde(default)]
    pub picture: Option<String>,
    /// The cell they have open, so two people do not both start on one
    /// without knowing.
    #[serde(default)]
    pub editing: Option<Cell>,
    /// What they have typed into it and not committed yet.
    #[serde(default)]
    pub draft: Option<String>,
}

/// What arrived on the socket.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// The connection is up. Says who else is here.
    ///
    /// `connection` is this socket's own handle on the server, which is sent
    /// back on a REST push so that the append does not broadcast this client's
    /// own work to this very socket. `None` is an older server that does not
    /// say, and the push then omits the field exactly as it always did.
    Welcome {
        head: i64,
        peers: Vec<Peer>,
        connection: Option<u64>,
    },
    /// Everything missed while away, sent before any live change so the order
    /// things are applied in is the log's order.
    Catchup { head: i64, changes: Vec<Change> },
    /// The cursor is not replayable, so nothing on this socket means anything
    /// until a whole plan has been fetched.
    Gap { head: i64 },
    /// One appended change, as it happened.
    Change { seq: i64, change: Change },
    /// Work offered over the socket went in, and each local change id was
    /// given a seq. `snapshot_wanted` is the server asking for a fresh whole
    /// plan, which is housekeeping rather than anything a planner decides.
    Applied {
        head: i64,
        applied: Vec<(u64, i64)>,
        snapshot_wanted: bool,
    },
    /// Somebody else got there first, and what was missed came back with the
    /// refusal so it can be replayed under this copy's own work.
    Behind {
        head: i64,
        changes: Vec<Change>,
        /// Whether more is waiting beyond this page.
        more: bool,
    },
    /// This copy's cursor is past the server's, so the two are not the same
    /// log and offering work would interleave two histories.
    Ahead { head: i64, cursor: i64 },
    /// The server would not take something, and said why. Nothing was
    /// written, so whatever was offered is still here to be offered again.
    Refused(String),
    Presence(Peer),
    Joined { name: String },
    Left { subject: String },
    /// The socket has ended, with what to say about it. Always the last thing
    /// a connection produces, so the interface has one place to notice that
    /// live editing has stopped.
    Closed(String),
}

/// What the interface asks the socket to send.
///
/// Already encoded. What goes in a message is decided where the difference
/// between one moment and the next is known, and the worker's job is to write
/// bytes rather than to know the protocol twice.
enum Outgoing {
    Say(String),
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
    /// Set by the worker whenever it queues something, cleared by `drain`.
    ///
    /// The interface polls on a timer, and taking a write handle to the plan
    /// is what marks it dirty, so a poll that finds nothing must be able to
    /// say so without one. Without this, an open socket with nobody doing
    /// anything redraws the window on every tick forever.
    waiting: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    /// What the others were last told about this planner, and when.
    ///
    /// Kept here rather than on the plan's state, and behind a cell rather
    /// than behind `&mut`, because that is what lets a pointer move be sent
    /// without taking a write handle to the plan. A mouse must never redraw
    /// the window.
    told: RefCell<Presence>,
    told_at: MutCell<Option<Instant>>,
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
        picture: Option<&str>,
    ) -> Result<Live, CollabError> {
        // Checked here, before a thread exists, because a bad address is the
        // one failure worth refusing outright rather than reporting as a
        // connection that ended.
        let url = socket_url(&collab::base(server)?, project, token);

        let (to_ui, incoming) = channel();
        let (to_socket, from_ui) = channel();
        let stop = Arc::new(AtomicBool::new(false));

        let waiting = Arc::new(AtomicBool::new(false));

        let flag = Arc::clone(&stop);
        let queued = Arc::clone(&waiting);
        let mut hello = json!({ "type": "hello", "after": after, "name": name });
        // Only when there is one. A `null` here would be a claim about the
        // account rather than the absence of one, and most accounts have no
        // picture at all.
        if let Some(picture) = picture
            && let Some(fields) = hello.as_object_mut()
        {
            fields.insert("picture".into(), json!(picture));
        }
        let hello = hello.to_string();
        let worker = std::thread::Builder::new()
            .name("aop-live".into())
            .spawn(move || {
                let ending = pump(&url, hello, &to_ui, &from_ui, &flag, &queued);
                // The last word, whatever happened. Sending on a channel whose
                // other end has gone is fine and is what happens when a plan is
                // closed while the socket is still up.
                queued.store(true, Ordering::Relaxed);
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
            waiting,
            worker: Some(worker),
            told: RefCell::new(Presence::default()),
            told_at: MutCell::new(None),
            finished: false,
        })
    }

    /// Whether anything is queued, without taking any of it.
    ///
    /// Asked before a write handle to the plan is taken, because taking one
    /// is what redraws the window: a live session where nobody is doing
    /// anything must cost nothing at all.
    pub fn has_incoming(&self) -> bool {
        self.waiting.load(Ordering::Relaxed)
    }

    /// Everything that has arrived since the last look. Never blocks.
    pub fn drain(&mut self) -> Vec<Incoming> {
        // Cleared first. Anything the worker queues between here and the end
        // of the loop sets it again, so the worst case is one empty poll and
        // never a message that sits unnoticed.
        self.waiting.store(false, Ordering::Relaxed);
        let mut batch = Vec::new();
        while let Ok(message) = self.incoming.try_recv() {
            if matches!(message, Incoming::Closed(_)) {
                self.finished = true;
            }
            batch.push(message);
        }
        batch
    }

    /// Say what this planner is doing, if any of it is news.
    ///
    /// Takes the whole ephemeral picture and works out the difference itself,
    /// so no caller has to remember what was last said. Nothing goes out when
    /// nothing has changed, which is what keeps a still session silent, and a
    /// pointer that is the only thing moving is capped rather than streamed.
    ///
    /// `&self` on purpose: this is called from a timer several times a second
    /// while a mouse is moving, and taking a write handle to the plan to say
    /// where a pointer is would redraw the whole window for it.
    pub fn looking_at(&self, now: &Presence) {
        let Ok(mut told) = self.told.try_borrow_mut() else {
            return;
        };
        if *told == *now {
            return;
        }
        // Everything but the pointer is rare and worth saying at once. A
        // pointer on its own is the flood, so it waits its turn.
        let only_the_pointer = told.row == now.row
            && told.editing == now.editing
            && told.draft == now.draft;
        let elapsed = self.told_at.get().map(|when| when.elapsed());
        if only_the_pointer && elapsed.is_some_and(|since| since < PRESENCE_EVERY) {
            return;
        }

        let mut message = json!({ "type": "presence", "row": now.row });
        let Some(fields) = message.as_object_mut() else {
            return;
        };
        // Absent is what the protocol reads as unchanged, so only what is
        // actually new is written. A pointer is never written as `null`:
        // there is no such thing as a pointer going away, and writing one
        // would blank everybody else's view of it.
        if now.at.is_some()
            && now.at != told.at
            && let Ok(at) = serde_json::to_value(now.at)
        {
            fields.insert("at".into(), at);
        }
        // These two do go out as `null` when they change to nothing, because
        // an edit being abandoned is news: it is what releases the cell.
        if now.editing != told.editing
            && let Ok(editing) = serde_json::to_value(now.editing)
        {
            fields.insert("editing".into(), editing);
        }
        if now.draft != told.draft
            && let Ok(draft) = serde_json::to_value(now.draft.clone())
        {
            fields.insert("draft".into(), draft);
        }

        if self.outgoing.send(Outgoing::Say(message.to_string())).is_ok() {
            *told = now.clone();
            self.told_at.set(Some(Instant::now()));
        }
    }

    /// Offer work to the log over the socket.
    ///
    /// The same question the REST push asks, over a connection that is
    /// already open. It is answered by the same four decisions, which is the
    /// point: this is a faster way to move entries, not a way around them.
    pub fn send_changes(&self, after: i64, changes: &[Change]) {
        let message = json!({ "type": "changes", "after": after, "changes": changes });
        let _ = self.outgoing.send(Outgoing::Say(message.to_string()));
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
    queued: &AtomicBool,
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

        // Everything waiting, not one a turn: a batch of changes queued
        // behind a pointer move would otherwise wait a whole read for its
        // turn, and the read is what the interval below is sized against.
        loop {
            match from_ui.try_recv() {
                Ok(Outgoing::Say(text)) => {
                    if socket.send(Message::Text(text.into())).is_err() {
                        return "Live editing stopped: the connection was lost.".into();
                    }
                }
                Ok(Outgoing::Close) | Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return "Live editing stopped.".into();
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(message) = read_message(&text)
                    && {
                        // Flagged before the send, so the interface never
                        // finds an empty queue while something is in it.
                        queued.store(true, Ordering::Relaxed);
                        to_ui.send(message).is_err()
                    }
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

/// One change that went in, under the name this copy gave it and the name the
/// log gave it. The two are different numbers and confusing them is how a
/// sync marks the wrong work as sent.
#[derive(Debug, Clone, Copy, Deserialize)]
struct Assigned {
    local_id: u64,
    seq: i64,
}

/// Everything the server can say, in one shape.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Welcome {
        head: i64,
        #[serde(default)]
        peers: Vec<Peer>,
        /// Absent on any server built before it started saying so. Absent is
        /// handled rather than assumed away: the push simply leaves the field
        /// out, which is the body such a server already expects.
        #[serde(default)]
        connection: Option<u64>,
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
    Applied {
        head: i64,
        #[serde(default)]
        applied: Vec<Assigned>,
        #[serde(default)]
        snapshot_wanted: bool,
    },
    Behind {
        head: i64,
        #[serde(default)]
        changes: Vec<Change>,
        #[serde(default)]
        more: bool,
    },
    Ahead {
        head: i64,
        cursor: i64,
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
    /// A complaint from the server.
    ///
    /// It carries words now because it has to: with work streaming over this
    /// socket, a refusal is the difference between a batch that is on its way
    /// and one that is sitting here unanswered forever. Nothing was written
    /// when this arrives, so the work is still unsent and still offerable.
    Error {
        #[serde(default)]
        message: String,
    },
}

/// A peer as it may be held, with anything unloadable already gone.
///
/// The check is here, at the edge, rather than only where a face is drawn.
/// A picture address arriving over a socket is a string another account chose
/// and this server merely relayed, and the label is the one place a peer
/// could otherwise put content of their choosing into somebody else's window.
/// A session's own picture is checked the same way when it is signed in with;
/// arriving over a websocket makes it no more trustworthy than that, so it
/// passes the same gate and an address that fails it simply is not there,
/// which renders as initials exactly as an account with no picture does.
fn vetted(mut peer: Peer) -> Peer {
    peer.picture = peer
        .picture
        .filter(|url| crate::cloud::oauth::transport_is_safe(url));
    peer
}

/// Turn one message into something the interface can act on.
///
/// Anything unrecognised is dropped rather than reported. A newer server
/// sending a message this build has never heard of is not a fault, and tearing
/// down a working socket over it would be.
fn read_message(text: &Utf8Bytes) -> Option<Incoming> {
    match serde_json::from_str::<ServerMessage>(text.as_str()).ok()? {
        ServerMessage::Welcome { head, peers, connection } => Some(Incoming::Welcome {
            head,
            peers: peers.into_iter().map(vetted).collect(),
            connection,
        }),
        ServerMessage::Catchup { head, changes } => Some(Incoming::Catchup { head, changes }),
        ServerMessage::Gap { head } => Some(Incoming::Gap { head }),
        ServerMessage::Change { seq, change } => Some(Incoming::Change { seq, change }),
        ServerMessage::Applied { head, applied, snapshot_wanted } => Some(Incoming::Applied {
            head,
            applied: applied.into_iter().map(|one| (one.local_id, one.seq)).collect(),
            snapshot_wanted,
        }),
        ServerMessage::Behind { head, changes, more } => {
            Some(Incoming::Behind { head, changes, more })
        }
        ServerMessage::Ahead { head, cursor } => Some(Incoming::Ahead { head, cursor }),
        ServerMessage::Presence(peer) => Some(Incoming::Presence(vetted(peer))),
        ServerMessage::Joined { subject, name } => Some(Incoming::Joined {
            name: if name.trim().is_empty() { subject } else { name },
        }),
        ServerMessage::Left { subject } => Some(Incoming::Left { subject }),
        // A refusal has to come through, or a batch that was refused sits in
        // flight forever and nothing this copy does reaches anybody again.
        ServerMessage::Error { message } => Some(Incoming::Refused(message)),
        // A keepalive is not something to show.
        ServerMessage::Pong => None,
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
    fn a_welcome_says_who_else_is_here_and_which_connection_this_is() {
        let message = parse(
            r#"{"type":"welcome","head":45,"connection":7,
                "peers":[{"subject":"0198","name":"Grace","row":12}]}"#,
        );
        let Some(Incoming::Welcome { head, peers, connection }) = message else {
            panic!("a welcome is the answer to hello");
        };
        assert_eq!(head, 45);
        assert_eq!(peers[0].name, "Grace");
        assert_eq!(peers[0].row, Some(12));
        assert_eq!(
            connection,
            Some(7),
            "the handle a REST push sends back so it is not echoed to this socket",
        );
    }

    #[test]
    fn a_server_that_does_not_say_which_connection_this_is_still_welcomes() {
        // An older server sends no such field. The push then omits it, which
        // is the body that server already expects, and the copy falls back to
        // recognising its own work by where the cursor has reached.
        let message = parse(r#"{"type":"welcome","head":45}"#);
        let Some(Incoming::Welcome { connection, .. }) = message else {
            panic!("a welcome without the field is still a welcome");
        };
        assert_eq!(connection, None);
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
    fn a_face_this_copy_will_not_load_is_gone_before_anything_holds_it() {
        // A picture address on a presence is a string another account chose
        // and a server relayed. It goes through the same gate a session's own
        // picture goes through, and one that fails it is simply not there,
        // which renders as initials exactly as an account with no picture
        // does. This is the one place a peer could otherwise put content of
        // their choosing into somebody else's window.
        let welcome = parse(
            r#"{"type":"welcome","head":1,"peers":[
                {"subject":"a","name":"Ada","picture":"https://idp.example.test/a.png"},
                {"subject":"b","name":"Bob","picture":"javascript:alert(1)"},
                {"subject":"c","name":"Cy","picture":"http://idp.example.test/c.png"}]}"#,
        );
        let Some(Incoming::Welcome { peers, .. }) = welcome else {
            panic!("a welcome says who else is here");
        };
        assert_eq!(peers[0].picture.as_deref(), Some("https://idp.example.test/a.png"));
        assert!(peers[1].picture.is_none(), "a script url is not a face");
        assert!(peers[2].picture.is_none(), "nor is a plain http one");

        let presence = parse(
            r#"{"type":"presence","subject":"b","name":"Bob","picture":"data:text/html,<script>"}"#,
        );
        let Some(Incoming::Presence(peer)) = presence else {
            panic!("a presence is where somebody is");
        };
        assert!(peer.picture.is_none());
    }

    #[test]
    fn a_copy_older_than_pictures_is_still_a_peer() {
        // The field was added after the first copies shipped, and a presence
        // without one has to keep working with initials drawn for it.
        let message = parse(r#"{"type":"presence","subject":"a","name":"Ada","row":3}"#);
        let Some(Incoming::Presence(peer)) = message else {
            panic!("a presence without a picture is still a presence");
        };
        assert_eq!(peer.row, Some(3));
        assert!(peer.picture.is_none());
        assert!(peer.editing.is_none());
        assert!(peer.draft.is_none());
    }

    #[test]
    fn a_cell_somebody_has_open_arrives_with_what_they_have_typed() {
        let message = parse(
            r#"{"type":"presence","subject":"a","name":"Ada",
                "editing":{"row":4,"column":1},"draft":"Pour the "}"#,
        );
        let Some(Incoming::Presence(peer)) = message else {
            panic!("wrong variant");
        };
        assert_eq!(peer.editing, Some(Cell { row: 4, column: 1 }));
        assert_eq!(peer.draft.as_deref(), Some("Pour the "));
    }

    #[test]
    fn the_four_answers_a_push_gets_are_the_four_the_socket_gets() {
        // The client already knows how to answer behind, gap and ahead,
        // because the REST push taught it. A second set of words for the same
        // four decisions is how one transport ends up quietly wrong.
        assert_eq!(
            parse(r#"{"type":"applied","head":45,"applied":[{"local_id":7,"seq":45}]}"#),
            Some(Incoming::Applied {
                head: 45,
                applied: vec![(7, 45)],
                snapshot_wanted: false,
            }),
        );
        assert_eq!(
            parse(r#"{"type":"behind","head":46,"after":44,"changes":[],"more":false}"#),
            Some(Incoming::Behind { head: 46, changes: Vec::new(), more: false }),
        );
        assert_eq!(
            parse(r#"{"type":"ahead","head":12,"cursor":60}"#),
            Some(Incoming::Ahead { head: 12, cursor: 60 }),
        );
        assert_eq!(
            parse(r#"{"type":"gap","head":45,"oldest":38}"#),
            Some(Incoming::Gap { head: 45 }),
        );
    }

    #[test]
    fn an_older_server_that_never_asks_for_a_snapshot_does_not_break_a_newer_copy() {
        // Backwards compatible the other way round as well: a field this copy
        // knows about and the server has never heard of must not turn a
        // working answer into an unreadable one.
        assert_eq!(
            parse(r#"{"type":"applied","head":45}"#),
            Some(Incoming::Applied { head: 45, applied: Vec::new(), snapshot_wanted: false }),
        );
    }

    #[test]
    fn a_keepalive_is_not_passed_on_as_something_to_show() {
        assert_eq!(parse(r#"{"type":"pong"}"#), None);
    }
}
