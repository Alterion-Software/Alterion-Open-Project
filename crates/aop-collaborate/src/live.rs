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
    },
    /// Where this planner is looking, so others can show it.
    Presence {
        #[serde(default)]
        row: Option<i64>,
        /// Where the pointer is. Absent means unchanged, so a client sending
        /// only a row selection does not blank everybody else's view of it.
        #[serde(default)]
        at: Option<Pointer>,
    },
    /// Keepalive, for clients that would rather not use websocket pings.
    Ping,
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
}

struct Peer {
    subject: String,
    name: String,
    row: Option<i64>,
    at: Option<Pointer>,
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
        rooms
            .entry(project)
            .or_default()
            .insert(id, Peer { subject, name, row: None, at: None, outbox });
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

    /// The display name a client asked to be known by. The token has no name
    /// in it, so this is the only place one comes from.
    pub fn set_name(&self, project: Uuid, id: ConnId, name: String) {
        let mut rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(peer) = rooms.get_mut(&project).and_then(|room| room.get_mut(&id)) {
            peer.name = name;
        }
    }

    /// This connection's name and selected row, for the messages that echo
    /// them back to everybody else.
    pub fn describe(&self, project: Uuid, id: ConnId) -> (String, Option<i64>) {
        let rooms = self.rooms.lock().unwrap_or_else(PoisonError::into_inner);
        rooms
            .get(&project)
            .and_then(|room| room.get(&id))
            .map(|peer| (peer.name.clone(), peer.row))
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
