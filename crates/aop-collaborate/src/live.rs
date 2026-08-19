//! Live editing: who is connected to a project, and what to send them.
//!
//! The rule is small on purpose. When a change is appended, by anybody and
//! through any route, every other connection on that project is sent it. That
//! is all the liveness there is: no operational transform, no CRDT, no shared
//! mutable document. The command log is what makes that enough, because a
//! client that misses a message is not corrupt, it is behind, and being
//! behind is a thing this protocol already knows how to fix.
//!
//! Presence rides on the same socket because it is the same question: who
//! else is here, and where are they looking.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use aop_core::history::Change;
use serde::{Deserialize, Serialize};

use crate::sync::Assigned;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

/// One connection. Not one person: the same account with the plan open on a
/// laptop and a desktop is two of these.
pub type ConnId = u64;

/// What a client says.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// First message. The cursor is how a reconnecting client says what it
    /// already has, so it can be given what it missed before the live stream
    /// starts. Without it, a client that dropped for ten seconds silently
    /// loses whatever was pushed in them.
    Hello {
        #[serde(default)]
        after: Option<i64>,
        #[serde(default)]
        name: Option<String>,
        /// Where this planner's face is, if their provider serves one. A URL
        /// the identity provider hosts, never image data: a socket that
        /// carried pictures would carry megabytes to say who is here.
        #[serde(default)]
        picture: Option<String>,
    },
    /// Where this planner is looking, so others can show it.
    ///
    /// The ephemeral half of the protocol. Nothing here is stored, nothing
    /// gets a seq, and nothing is replayed on a reconnect: it is where
    /// somebody is right now, and when they go it goes with them. Putting any
    /// of it in the log would give "moved the mouse" a sequence number for a
    /// rebase to resolve.
    Presence {
        #[serde(default)]
        row: Option<i64>,
        /// Where the pointer is. Absent means unchanged, so a client sending
        /// only a row selection does not blank everybody else's view of it.
        #[serde(default)]
        at: Option<Pointer>,
        /// The cell they have open for editing, so two people do not both
        /// start on one without knowing. Absent means unchanged and `null`
        /// means they have closed it, which are different things.
        #[serde(default, deserialize_with = "some_option")]
        editing: Option<Option<Cell>>,
        /// What they have typed into it and not committed yet. Never goes
        /// near the log: an abandoned edit would otherwise become a permanent
        /// record of something that never happened.
        #[serde(default, deserialize_with = "some_option")]
        draft: Option<Option<String>>,
    },
    /// Work this planner has done, offered over the socket instead of over
    /// the REST push.
    ///
    /// The durable half. It is answered by exactly the same four decisions a
    /// REST push is answered by, because it is the same protocol: two
    /// transports with two implementations means the one used less is the one
    /// that is quietly wrong, and the day you find out is a conflict.
    Changes {
        #[serde(default)]
        after: Option<i64>,
        changes: Vec<Change>,
    },
    /// Keepalive, for clients that would rather not use websocket pings.
    Ping,
}

/// Read a field that may be absent, null, or a value, keeping all three apart.
///
/// `Option<Option<T>>` with plain `#[serde(default)]` collapses null and
/// absent into the same `None`, and on this message they mean opposite things:
/// absent is "nothing new to say" and null is "it has gone". Without this, a
/// client reporting a pointer move would close everybody else's view of the
/// cell it has open.
fn some_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// What the server says.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The answer to `hello`, once the catch-up has been decided.
    Welcome { head: i64, peers: Vec<Presence> },
    /// Everything the client missed while it was away. Sent before any live
    /// change, so the order a client applies things in is the log's order.
    Catchup { head: i64, changes: Vec<Change> },
    /// The client's cursor is not replayable. It has to take a snapshot from
    /// the REST endpoint before this stream means anything.
    Gap { head: i64, oldest: Option<i64> },
    /// One appended change, as it happened.
    Change { seq: i64, change: Change },
    /// The answer to a batch of changes that went in: which seq each one was
    /// given, so the client can mark its own work as sent.
    Applied {
        head: i64,
        applied: Vec<Assigned>,
        /// The server asking for a fresh whole plan. It stores commands and
        /// has no engine to replay them with, so it cannot fold its own log
        /// into a plan and has to ask whoever it is talking to. Carried here
        /// as well as on the REST answer, because with streaming nobody
        /// presses Sync for hours and the ask would never be heard.
        snapshot_wanted: bool,
    },
    /// Somebody else got there first. Carries what was missed, so the client
    /// can replay its own work on top and offer it again, which is the same
    /// answer and the same rebase the REST push gets.
    Behind {
        head: i64,
        after: i64,
        changes: Vec<Change>,
        /// Whether more is waiting beyond this page.
        more: bool,
    },
    /// This client's cursor is past the end of the log, so the two are not
    /// the same log and appending would interleave two histories.
    Ahead { head: i64, cursor: i64 },
    Presence(Presence),
    Joined { subject: String, name: String },
    Left { subject: String },
    Pong,
    Error { message: String },
}

impl ServerMessage {
    /// JSON, or nothing. A message that will not serialise is a bug in this
    /// file, and dropping it is better than tearing down a planner's socket.
    pub fn encode(&self) -> Option<String> {
        match serde_json::to_string(self) {
            Ok(text) => Some(text),
            Err(err) => {
                log::error!("could not encode a live message: {err}");
                None
            }
        }
    }
}

/// Where a planner's pointer is, expressed in the plan rather than on a
/// screen. A pixel position is meaningless to anybody else: they have a
/// different window size, a different zoom and a different scroll offset, so
/// the only thing that survives the trip is what the pointer is *over*.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(tag = "pane", rename_all = "snake_case")]
pub enum Pointer {
    /// Over the table: a row, and which column it is in.
    Table { row: i64, column: u16 },
    /// Over the chart: a row, and how far along the timescale in minutes from
    /// the plan's start. Minutes rather than a date so it is cheap to send,
    /// and relative to the plan so it survives a reschedule.
    Chart { row: i64, minutes: i64 },
}

/// A cell somebody has open for editing, in the plan rather than on a screen.
///
/// The same reasoning as [`Pointer`]: a pixel rectangle means nothing on
/// somebody else's window, so what travels is which row and which column.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cell {
    pub row: i64,
    pub column: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct Presence {
    pub subject: String,
    pub name: String,
    /// The row this planner has selected, if the client bothered to say.
    pub row: Option<i64>,
    /// Where their pointer is. `None` means they have not moved it, or their
    /// client does not send one: an older client stays a peer, it simply has
    /// no pointer drawn for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<Pointer>,
    /// Where their face is, if their provider serves one. Absent is the
    /// ordinary case rather than a fault: most accounts have no picture, and
    /// whatever draws one falls back to initials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// The cell they have open, if any.
    ///
    /// Always stated, unlike the pointer. A client sends only what changed,
    /// because it knows what it last said; this direction states the whole
    /// answer, because the copy receiving it has no way to tell an absent
    /// field from a cell that has just been closed and would have to guess.
    /// Guessing wrong leaves somebody's abandoned half word on screen, or
    /// clears a cell they are still typing into, and the few bytes this costs
    /// on a message that already carries a pointer are not worth that.
    pub editing: Option<Cell>,
    /// What they have typed into it and not committed. Stated whole for the
    /// same reason.
    pub draft: Option<String>,
}

struct Peer {
    subject: String,
    name: String,
    picture: Option<String>,
    row: Option<i64>,
    at: Option<Pointer>,
    editing: Option<Cell>,
    draft: Option<String>,
    outbox: UnboundedSender<String>,
}

/// Every live connection, grouped by project.
#[derive(Default)]
pub struct Hub {
    rooms: Mutex<HashMap<Uuid, HashMap<ConnId, Peer>>>,
    next_id: AtomicU64,
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a connection and get the handle used to address it.
    pub fn join(
        &self,
        project: Uuid,
        subject: String,
        name: String,
        outbox: UnboundedSender<String>,
    ) -> ConnId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        rooms.entry(project).or_default().insert(
            id,
            Peer {
                subject,
                name,
                picture: None,
                row: None,
                at: None,
                editing: None,
                draft: None,
                outbox,
            },
        );
        id
    }

    pub fn leave(&self, project: Uuid, id: ConnId) {
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(room) = rooms.get_mut(&project) {
            room.remove(&id);
            // An empty room is a leak that would grow with every project ever
            // opened, so the last one out takes the key with them.
            if room.is_empty() {
                rooms.remove(&project);
            }
        }
    }

    /// The name and picture a client asked to be known by. The token carries
    /// neither, so this is the only place either comes from.
    pub fn set_identity(
        &self,
        project: Uuid,
        id: ConnId,
        name: Option<String>,
        picture: Option<String>,
    ) {
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(peer) = rooms.get_mut(&project).and_then(|room| room.get_mut(&id)) {
            if let Some(name) = name {
                peer.name = name;
            }
            if picture.is_some() {
                peer.picture = picture;
            }
        }
    }

    /// This connection's name and picture, for the messages that echo them
    /// back to everybody else.
    pub fn describe(&self, project: Uuid, id: ConnId) -> (String, Option<String>) {
        let rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        rooms
            .get(&project)
            .and_then(|room| room.get(&id))
            .map(|peer| (peer.name.clone(), peer.picture.clone()))
            .unwrap_or_default()
    }

    /// Record where a pointer is. Pointer moves are frequent, so this is
    /// deliberately in-memory only and never written to the database: losing
    /// them on a restart costs nothing, and persisting them would turn a
    /// mouse into write traffic.
    pub fn set_pointer(&self, project: Uuid, id: ConnId, at: Option<Pointer>) {
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(peer) = rooms.get_mut(&project).and_then(|room| room.get_mut(&id)) {
            peer.at = at;
        }
    }

    pub fn set_row(&self, project: Uuid, id: ConnId, row: Option<i64>) {
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(peer) = rooms.get_mut(&project).and_then(|room| room.get_mut(&id)) {
            peer.row = row;
        }
    }

    /// Record the cell somebody has open and what they have typed into it.
    ///
    /// In memory only, like the pointer and for the same reason: an
    /// uncommitted value is not work, it is somebody mid-keystroke, and
    /// writing it anywhere would turn an abandoned edit into a record of
    /// something that never happened. The outer option is "nothing new to
    /// say", the inner one is "it has gone".
    pub fn set_editing(
        &self,
        project: Uuid,
        id: ConnId,
        editing: Option<Option<Cell>>,
        draft: Option<Option<String>>,
    ) {
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(peer) = rooms.get_mut(&project).and_then(|room| room.get_mut(&id)) {
            if let Some(editing) = editing {
                peer.editing = editing;
            }
            if let Some(draft) = draft {
                peer.draft = draft.map(|text| tidy(&text, MAX_DRAFT));
            }
        }
    }

    /// The cell and draft this connection has, as stored.
    ///
    /// Read back rather than taken from the message being answered, because
    /// what goes out is the whole answer rather than the difference: a client
    /// sends only what changed and the room is what knows the rest.
    pub fn what_is_open(&self, project: Uuid, id: ConnId) -> (Option<Cell>, Option<String>) {
        let rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        rooms
            .get(&project)
            .and_then(|room| room.get(&id))
            .map(|peer| (peer.editing, peer.draft.clone()))
            .unwrap_or_default()
    }

    /// Who else is in this project, not counting the connection asking.
    pub fn peers(&self, project: Uuid, except: Option<ConnId>) -> Vec<Presence> {
        let rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        rooms
            .get(&project)
            .map(|room| {
                room.iter()
                    .filter(|(id, _)| Some(**id) != except)
                    .map(|(_, peer)| Presence {
                        subject: peer.subject.clone(),
                        name: peer.name.clone(),
                        row: peer.row,
                        at: peer.at,
                        picture: peer.picture.clone(),
                        editing: peer.editing,
                        draft: peer.draft.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Send to everyone in a project, optionally skipping the one who caused
    /// it. Encoding happens once rather than per connection.
    pub fn broadcast(&self, project: Uuid, message: &ServerMessage, except: Option<ConnId>) {
        let Some(text) = message.encode() else {
            return;
        };
        let rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(room) = rooms.get(&project) else {
            return;
        };
        for (id, peer) in room {
            if Some(*id) == except {
                continue;
            }
            // A closed outbox means the connection's own task has already
            // gone, and it removes itself from the room when it does. Nothing
            // to do here but skip it.
            let _ = peer.outbox.send(text.clone());
        }
    }

    /// Send to one connection.
    pub fn send(&self, project: Uuid, id: ConnId, message: &ServerMessage) {
        let Some(text) = message.encode() else {
            return;
        };
        let rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(peer) = rooms.get(&project).and_then(|room| room.get(&id)) {
            let _ = peer.outbox.send(text);
        }
    }

    /// How many connections a project has, for the health endpoint and tests.
    pub fn connected(&self, project: Uuid) -> usize {
        self.rooms
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&project)
            .map(HashMap::len)
            .unwrap_or(0)
    }
}

/// How much of a name, a picture address or a draft is carried.
///
/// The frame limit already stops anything absurd, but these are echoed to
/// every other connection on the project, so one client sending a megabyte of
/// "draft" would cost every peer in the room a megabyte. A cell holds a task
/// name or a date, and a picture is a URL, so both caps are generous.
const MAX_PICTURE: usize = 2048;
const MAX_DRAFT: usize = 512;

/// Trim a piece of client text to something worth relaying.
fn tidy(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// A picture address worth keeping, or nothing.
///
/// The value is not trusted here beyond its size: what a URL is allowed to be
/// is a question about the window that would load it, and it is answered on
/// the client that draws it rather than guessed at here.
pub fn picture_worth_keeping(picture: Option<String>) -> Option<String> {
    picture
        .map(|url| tidy(url.trim(), MAX_PICTURE))
        .filter(|url| !url.is_empty())
}
