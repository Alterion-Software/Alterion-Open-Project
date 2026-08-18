//! The websocket: `GET /api/projects/{id}/live`.
//!
//! Authenticated exactly like the REST routes, by introspecting a bearer
//! token, with one concession: a browser cannot set headers on a websocket
//! handshake, so `?access_token=` is accepted here and nowhere else.
//!
//! The recovery story is the reason the first message is `hello`. A socket
//! that drops for ten seconds and comes back would otherwise resume a live
//! stream having missed whatever was appended in them, and nothing would ever
//! say so. Sending a cursor turns a reconnect into a small sync.

use std::time::Duration;

use actix_web::{HttpRequest, HttpResponse, get, web};
use actix_ws::AggregatedMessage;
use futures_util::StreamExt;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use uuid::Uuid;

use crate::auth::bearer_or_query;
use crate::entity::changes as change_rows;
use crate::error::SyncError;
use crate::live::{ClientMessage, ConnId, ServerMessage};
use crate::state::AppState;

/// How long a connection has to introduce itself before the live stream
/// starts without a catch-up. A client that never says hello is watching from
/// now on, which is a coherent thing to want.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// Continuation frames are capped: this protocol's largest message is a page
/// of the log, and anything bigger is either a bug or somebody probing.
const MAX_FRAME: usize = 1 << 20;

#[get("/api/projects/{id}/live")]
pub async fn live(
    req: HttpRequest,
    body: web::Payload,
    state: web::Data<AppState>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, SyncError> {
    let token = bearer_or_query(&req).ok_or(SyncError::Unauthenticated)?;
    let who = state.idp.verify(&token).await?;
    let id = path.into_inner();
    // Any member may watch. Writing still goes through the REST push, which
    // checks the role again, so a viewer cannot append by holding a socket.
    super::role_on(&state.db, id, &who.subject).await?;

    let (response, session, stream) =
        actix_ws::handle(&req, body).map_err(SyncError::internal)?;

    let (outbox, inbox) = mpsc::unbounded_channel();
    // Joining before the first message is read means nothing appended between
    // now and the catch-up is lost: it queues, and arrives after. It can
    // overlap with what the catch-up already carried, which is harmless,
    // because a change that is already in a client's history is ignored by
    // its merge rather than applied twice.
    let conn = state
        .hub
        .join(id, who.subject.clone(), who.subject.clone(), outbox);

    actix_web::rt::spawn(pump(
        state.clone(),
        id,
        conn,
        who.subject,
        session,
        stream,
        inbox,
    ));
    Ok(response)
}

/// One connection's whole life: greet, then relay in both directions.
#[allow(clippy::too_many_arguments)]
async fn pump(
    state: web::Data<AppState>,
    project: Uuid,
    conn: ConnId,
    subject: String,
    mut session: actix_ws::Session,
    stream: actix_ws::MessageStream,
    mut inbox: UnboundedReceiver<String>,
) {
    let mut stream = stream.aggregate_continuations().max_continuation_size(MAX_FRAME);

    // The greeting is handled before the relay loop starts so a catch-up can
    // never be interleaved with the live changes it is meant to precede.
    let greeting = tokio::time::timeout(HELLO_TIMEOUT, stream.next()).await;
    if let Ok(Some(Ok(AggregatedMessage::Text(text)))) = greeting {
        match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Hello { after, name }) => {
                if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
                    state.hub.set_name(project, conn, name);
                }
                for message in catch_up(&state, project, conn, after).await {
                    if send(&mut session, &message).await.is_err() {
                        return finish(&state, project, conn, &subject, session).await;
                    }
                }
                let (name, _) = state.hub.describe(project, conn);
                state.hub.broadcast(
                    project,
                    &ServerMessage::Joined { subject: subject.clone(), name },
                    Some(conn),
                );
            }
            Ok(_) | Err(_) => {
                let _ = send(
                    &mut session,
                    &ServerMessage::Error { message: "expected a hello message".into() },
                )
                .await;
            }
        }
    }

    loop {
        tokio::select! {
            outgoing = inbox.recv() => {
                let Some(text) = outgoing else { break };
                if session.text(text).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(AggregatedMessage::Text(text))) => {
                        if !on_client_message(&state, project, conn, &subject, &mut session, &text).await {
                            break;
                        }
                    }
                    Some(Ok(AggregatedMessage::Ping(bytes))) => {
                        if session.pong(&bytes).await.is_err() {
                            break;
                        }
                    }
                    // Binary frames and pongs carry nothing this protocol
                    // uses; ignoring them beats closing on a stray keepalive.
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        log::debug!("live socket error on {project}: {err}");
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    finish(&state, project, conn, &subject, session).await;
}

/// Returns false when the connection should close.
async fn on_client_message(
    state: &web::Data<AppState>,
    project: Uuid,
    conn: ConnId,
    subject: &str,
    session: &mut actix_ws::Session,
    text: &str,
) -> bool {
    match serde_json::from_str::<ClientMessage>(text) {
        Ok(ClientMessage::Presence { row }) => {
            state.hub.set_row(project, conn, row);
            let (name, _) = state.hub.describe(project, conn);
            state.hub.broadcast(
                project,
                &ServerMessage::Presence(crate::live::Presence {
                    subject: subject.to_string(),
                    name,
                    row,
                }),
                Some(conn),
            );
            true
        }
        Ok(ClientMessage::Ping) => send(session, &ServerMessage::Pong).await.is_ok(),
        // A second hello is a client that lost track of its own state. It is
        // not worth closing over, and re-sending a catch-up on demand would
        // let one client ask for the whole log as fast as it can type.
        Ok(ClientMessage::Hello { .. }) => true,
        Err(err) => send(
            session,
            &ServerMessage::Error { message: format!("could not read that message: {err}") },
        )
        .await
        .is_ok(),
    }
}

/// What a reconnecting client missed, or the refusal that says it cannot be
/// caught up this way.
async fn catch_up(
    state: &web::Data<AppState>,
    project: Uuid,
    conn: ConnId,
    after: Option<i64>,
) -> Vec<ServerMessage> {
    let peers = state.hub.peers(project, Some(conn));
    let Ok(row) = super::load_project(&state.db, project).await else {
        return vec![ServerMessage::Error { message: "project is gone".into() }];
    };
    let Ok(range) = super::log_range(&state.db, &row).await else {
        return vec![ServerMessage::Error { message: "could not read the log".into() }];
    };

    let mut messages = vec![ServerMessage::Welcome { head: range.head, peers }];
    let Some(after) = after else {
        // No cursor means a client that has no plan yet, and the whole plan
        // is a REST call, not a websocket message.
        return messages;
    };

    if range.has_gap_since(after) {
        messages.push(ServerMessage::Gap { head: range.head, oldest: range.oldest });
        return messages;
    }

    match super::changes::fetch_after(&state.db, project, after, super::changes::MAX_PAGE).await {
        Ok(rows) => messages.push(ServerMessage::Catchup {
            head: range.head,
            changes: rows.iter().map(change_rows::Model::to_change).collect(),
        }),
        Err(err) => {
            log::error!("catch-up failed for {project}: {err}");
            messages.push(ServerMessage::Error { message: "could not read the log".into() });
        }
    }
    messages
}

async fn send(session: &mut actix_ws::Session, message: &ServerMessage) -> Result<(), ()> {
    let Some(text) = message.encode() else {
        return Ok(());
    };
    session.text(text).await.map_err(|_| ())
}

async fn finish(
    state: &web::Data<AppState>,
    project: Uuid,
    conn: ConnId,
    subject: &str,
    session: actix_ws::Session,
) {
    state.hub.leave(project, conn);
    state
        .hub
        .broadcast(project, &ServerMessage::Left { subject: subject.to_string() }, None);
    let _ = session.close(None).await;
}
