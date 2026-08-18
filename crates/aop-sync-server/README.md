# aop-sync-server

The self-hostable sync service for Alterion Open Project. It stores plans,
keeps the authored change log, syncs clients, and streams live edits.

It is a **resource server**, not an identity provider. The Alterion identity
provider already exists and is itself self-hostable, so this service validates
bearer tokens by introspecting them against whichever issuer you point it at.
There are no accounts, passwords or sessions in here.

## What it stores

A plan's history is an append-only log of the commands that made it, the same
`Change { id, at, author, script, summary }` the desktop app records. The
server orders them and hands out the ones a client has not seen. It never
merges, and it never replays: it has no scheduling engine, on purpose.

| table             | what it holds                                              |
|-------------------|------------------------------------------------------------|
| `aop.projects`    | one plan, its owner's subject, and `head_seq`               |
| `aop.project_members` | who can see it, and in what role                        |
| `aop.changes`     | the log, keyed by `(project_id, seq)`                       |
| `aop.snapshots`   | a whole plan as of some seq, for clients that cannot replay |

`seq` counts from 1 and is the sync cursor. A cursor of `0` means "I have
nothing"; a `head_seq` of `0` means the log is empty, which is a real state:
a project starts as a snapshot with nothing appended yet.

## Running it

You need PostgreSQL 13 or newer and a reachable Alterion identity provider.

```sh
cargo run -p aop-sync-server
```

The first run writes a `config.cfg` next to the binary and exits with whatever
the database complains about. Fill it in and run again. Migrations are applied
at startup, so there is no separate migrate step.

## Configuration

`config.cfg`, beside the binary. Every key can be overridden by an environment
variable named `AOP_SYNC_<SECTION>_<KEY>`, so `AOP_SYNC_DATABASE_URL` and
`AOP_SYNC_IDP_ISSUER` do what they look like. There is deliberately no `.env`
support: a database password does not belong in a file that every tool in the
tree knows how to source.

```ini
[server]
bind_addr = 127.0.0.1
bind_port = 8090

[database]
url = postgres://aop:aop@localhost:5432/aop_sync

[idp]
issuer = https://auth.coraldune.cloud
client_id =
client_secret =
token_cache_secs = 60

[cors]
allowed_origins = http://localhost:1420

[sync]
snapshot_every = 500

[logging]
level = info
```

### Pointing it at your own identity provider

Change `issuer`. That is the whole change.

Every endpoint this server talks to is read from
`{issuer}/.well-known/openid-configuration`, so the introspection URL follows
your deployment rather than being hardcoded here. The discovery document's own
`issuer` field is checked against the configured one, so a redirect cannot
quietly send bearer tokens somewhere nobody chose.

Set `client_id` and `client_secret` only if your IdP requires client
authentication on its introspection endpoint; they are sent as form fields
(`client_secret_post`), and are omitted entirely when unset.

Introspection answers are cached for `token_cache_secs`, keyed by a hash of
the token. Inactive answers are never cached, so a revoked token stops working
within one cache window and a freshly issued one works immediately.

## API

```
GET    /api/health
GET    /api/projects                       what this subject can see
POST   /api/projects                       create; body carries the initial plan
GET    /api/projects/{id}                  metadata plus the head seq
GET    /api/projects/{id}/snapshot         a whole plan plus the log after it
PUT    /api/projects/{id}/snapshot         store a fresh snapshot
GET    /api/projects/{id}/changes?after=N  everything after cursor N
POST   /api/projects/{id}/changes          push
DELETE /api/projects/{id}                  owner only
GET    /api/projects/{id}/live             websocket
```

All of them take `Authorization: Bearer <token>`.

### Push and rebase

A client pushes the work it made against a cursor:

```json
POST /api/projects/{id}/changes
{ "after": 42,
  "changes": [ { "id": 7, "at": "2026-08-18T09:00:00", "author": "Ada",
                 "script": "indent();", "summary": "Indented a task" } ],
  "connection": 3 }
```

`connection` is optional and is this client's live socket id, so it is not
sent back its own change over the websocket.

There are four answers, and `status` always says which:

| server state           | code | `status`  | what the client does                    |
|------------------------|------|-----------|------------------------------------------|
| head is still 42       | 200  | `applied` | records the seq each local id was given  |
| head has moved to 45   | 409  | `behind`  | applies 43..45, replays its own on top, pushes again |
| 43 has been trimmed    | 409  | `gap`     | fetches a snapshot instead               |
| cursor is past head    | 409  | `ahead`   | fetches a snapshot; the logs differ      |

A refusal writes nothing. The `behind` body carries the missed changes so the
rebase needs no second round trip:

```json
{ "status": "behind", "head": 45, "after": 42, "more": false,
  "changes": [ { "id": 43, "at": "...", "author": "Grace", "script": "...", "summary": "..." } ] }
```

The returned changes are numbered with the server's `seq`, not with whatever
the pusher called them locally, which is why the acknowledgement maps them:

```json
{ "status": "applied", "head": 45,
  "applied": [ { "local_id": 7, "seq": 43 } ],
  "snapshot_wanted": false }
```

After a sync, both logs use the same numbers, and one cursor means one thing
on both sides.

`snapshot_wanted` is the server asking for a fresh snapshot. It cannot make
one itself, having no engine to replay commands with, so once the log has run
`snapshot_every` past the newest stored snapshot it asks whoever pushes next
to `PUT /api/projects/{id}/snapshot` with `{ "seq": <head it includes>, "plan": {...} }`.

### Live editing

```
GET /api/projects/{id}/live
```

Authenticated the same way. A browser cannot set headers on a websocket
handshake, so this one route also accepts `?access_token=<token>`.

Messages are JSON with a `type`. The client speaks first:

```json
{ "type": "hello", "after": 42, "name": "Ada" }
```

and gets `welcome` (head plus who else is connected), then either `catchup`
with everything it missed or `gap` if its cursor is no longer replayable.
Sending the cursor is what makes a reconnect safe: without it, a socket that
dropped for ten seconds resumes having silently lost the edits made in them.

After that: every appended change arrives as `{"type":"change","seq":43,
"change":{...}}`, and `{"type":"presence","row":12}` from a client is echoed
to everybody else so the app can show where people are looking. `joined` and
`left` bracket a connection.

A change can arrive both in a `catchup` and again live, if it landed while the
catch-up was being read. That is harmless: a change already in a client's
history is ignored by its merge rather than applied twice.

## Limits worth knowing before you deploy

- The live hub is in process. Two instances behind a load balancer each only
  broadcast to their own clients. They still converge, because the log in
  Postgres is the truth and a reconnect catches up from it, but live editing
  across instances needs a shared bus and does not have one yet.
- Nothing trims the log yet, so `gap` cannot currently happen through normal
  use. The handling is there because trimming will come and because a restored
  backup produces the same situation.
- Sharing has a table but no endpoint. Only the creator is a member.

## Tests

```sh
cargo test -p aop-sync-server
```

The push decision, the cursor arithmetic and the introspection cache all run
without a database or an identity provider. The tests that need Postgres are
ignored by default; set `AOP_SYNC_TEST_DATABASE_URL` and run with
`--ignored` to include them.
