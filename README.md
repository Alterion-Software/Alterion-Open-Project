<div align="center">
    <picture>
        <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
        <source media="(prefers-color-scheme: light)" srcset="assets/logo-light.png">
        <img alt="Alterion Logo" src="assets/logo-dark.png" width="400">
    </picture>
</div>

<div align="center">

# Alterion Open Project

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Dioxus](https://img.shields.io/badge/Dioxus-0.7-blueviolet?style=flat)](https://dioxuslabs.com/)
[![Status](https://img.shields.io/badge/status-Open--Beta-cyan)](#not-implemented)

_A better free project scheduler. A real critical path engine behind a Microsoft Project style ribbon, reading Project's own files._

</div>

---

## What it does

A desktop project scheduler: forward and backward pass, slack, critical path,
summary rollup, resource booking and overallocation, earned value, resource
levelling, drawn behind a ribbon that follows Microsoft Project's layout.

Plans are saved as `.aprj`, a binary container (`APRJ` magic, versioned header,
deflated MessagePack). Microsoft Project plans open directly, both binary `.mpp`
and XML Format, and Excel round-trips.

A plan can also live on a server, keep a record of who changed what, and be
edited by more than one person at once. See [Sharing a plan](#sharing-a-plan).

```
cargo run -p aop-app        # launch
cargo test --workspace      # engine, import, geometry, settings, interactions, sync
cargo run -p aop-collaborate  # the sync server
```

## Layout

```
crates/aop-core     the plan and the maths, no UI, fully unit tested
  calendar.rs       working-time arithmetic (shifts, weekends, holidays)
  duration.rs       "5d" / "2w" / "4h" parsing and display
  model.rs          Project, Task outline, Link, Resource, Constraint, Baseline
  schedule.rs       topological sort -> forward pass -> backward pass ->
                    slack -> critical path -> summary rollup -> cost -> overallocation
  leveling.rs       resource levelling, and undoing it
  earned_value.rs   timephasing, then PV, EV, AC and the measures from them
  agile.rs          burndown, burnup, velocity, sprint status
  history.rs        the authored change log every edit is recorded into
  compare.rs        the difference between two plans, and applying it
  update.rs         Update Project: complete through, or reschedule past
  grouping.rs       Group by, and the summary rows a grouping produces
  custom.rs         user defined fields
  fields.rs         every column the table can show, and how to read it
  textstyle.rs      per row and per category text styling
  draw.rs           annotations on the chart, pinned to a date or a bar
  spelling.rs       the checker and its downloadable dictionaries
  subproject.rs     inserted plans and external dependencies
  issues.rs         schedule warnings, and dismissing them
  templates.rs      eight starter plans, which double as test fixtures
  persist.rs        .aprj binary container, CSV export
  pdf.rs            page layout for print and PDF
  excel.rs          .xlsx import and export
  mspdi.rs          Microsoft Project XML (MSPDI) import
  mpp.rs            maps a parsed .mpp onto the plan

crates/aop-app      the desktop shell
  theme.rs          the whole stylesheet, inline (no asset pipeline needed)
  brand.rs          the Alterion wordmark, inlined as SVG
  ribbon.rs         custom title bar, QAT, tab strip, seven ribbon tabs
  backstage.rs      File menu: dashboard, templates, browser, print, export,
                    reports, About, and a Project-style Options screen
  grid.rs           the Entry table, editable, drag to reorder
  gantt.rs          timescale, bars, arrows, and bar dragging
  views.rs          resource sheet, task usage, resource usage, network,
                    calendar, team planner
  dialogs.rs        task information, project information, working time,
                    assign resources, customize Quick Access Toolbar
  contextmenu.rs    right-click menus
  popups.rs         predecessor and resource pickers
  controls.rs       themed Dropdown, ComboBox and ribbon MenuBtn
  preview.rs        the mini Gantt used by thumbnails and previews
  viewport.rs       the row window both panes scroll through
  spooler.rs        printers, page ranges and the paged preview
  keymap.rs         the rebindable commands
  recovery.rs       the thirty second snapshot, and offering it back
  settings.rs       config.cfg, read and written by hand
  macros/           recording, replaying and reading a macro as script
  cloud/            signing in, device bound tokens, sync and live collaborate

crates/aop-collaborate   the sync server (Linux)
  sync.rs           the push decision: applied, behind, ahead, or gap
  live.rs           the websocket room
  auth.rs           validating a token against the identity provider
  entity/ schema/   Postgres tables and their migrations

crates/aop-procname      one process name in the task manager, not two
```

## The scheduling engine

Leaf tasks are scheduled; summary rows are derived by rolling their children up.
Links may name a summary at either end and are expanded onto its leaves, so the
topological order always places a predecessor's leaves before its successor and
one forward pass is enough.

Supported: all four link types with lag and lead, eight constraint types,
deadlines, manual vs auto scheduling, milestones, baselines with variance,
resource booking with work and cost, overallocation detection, and circular
dependency detection that reports rather than hangs.

A "day" of duration means eight working hours, matching Project's defaults, so
adding one day to Friday 08:00 lands on Friday 17:00 and adding two lands on
Monday 17:00.

## File formats

`.aprj` is a binary container: a four byte `APRJ` magic, a container version, a
flags word and the payload length, then MessagePack (field names kept, so older
files still load as the model grows) run through deflate. Files written by the
earlier JSON container still open.

Microsoft Project plans import two ways. **XML Format (*.xml)** is read in full:
outline, links with lag, constraints, deadlines, calendars with their holidays,
resources and assignments.

Binary **`.mpp`** is read by [`alterion-mpp-parser`](../crates/alterion-mpp-parser),
a separate Apache-2.0 crate written from the on-disk format. It covers MPP14
(Project 2010 and later) and names earlier generations rather than failing
vaguely. That parser has not yet been validated against a corpus of real files,
so where accuracy matters prefer the XML export; see its README for what is
still provisional.

## Interactions

| Where | Action |
| --- | --- |
| Grid row | Drag to reorder; drop on the middle of a row to nest underneath it |
| Grid cell | Double-click to edit; Predecessors and Resources open pickers |
| Gantt bar | Drag the middle to move, the right edge to resize, the left edge to set progress |
| Gantt bar | Shift-drag onto another bar to link them |
| Gantt bar | Double-click for Task Information, right-click for the full menu |
| Pane tab | Maximize the table or the chart, then restore |
| Pane tab button | Shows the layout it produces: fill with this pane, or restore the split |
| Anywhere | Right-click for a context menu; the webview's own menu never appears |

Keyboard: 43 commands, every one rebindable from Options > Keyboard and saved
to `config.cfg`. Taking a key press that is already in use removes it from
whatever had it, so two commands can never race for one key.

The window has no OS decorations: the title bar, drag region and
minimise/maximise/close are drawn by the app.

## Appearance

Bar colours live on the plan, so a recoloured chart travels with the file. Six
palettes sit in the Gantt Chart Style gallery, and Bar Colors opens a picker for
each element. The critical path is off by default; switch it on from Format.

## Packaging

`packaging/` holds every route.

```
./install.sh                             # Linux, current user
./install.sh --system                    # Linux, everyone (asks for root once)
cd packaging/aur && makepkg -si          # Arch, so pacman owns the files
packaging/windows/build-installer.sh     # Windows setup.exe, cross-built from Linux
packaging/macos/build-dmg.sh             # macOS universal .app and .dmg
```

On Arch, `install.sh` offers to build the package instead of dropping loose
files, because a package manager that owns the files can also remove them.

Both share `packaging/linux/`: the desktop entry, the scalable icon (which the
Windows `.ico` is generated from), and the MIME definition that registers
`.aprj` against its `APRJ` magic bytes rather than just the extension.

## Sharing a plan

Every edit is recorded as the command that made it, with an author and a
moment. That one record is the audit trail, the unit a sync exchanges, and the
thing a macro replays. It travels inside the `.aprj`.

Sync sends differences, never the file. A client pushes against the point it
last saw, and the server answers one of four ways:

| Answer | What it means | What the app does |
| --- | --- | --- |
| applied | nobody moved underneath you | marks them sent |
| behind | somebody did, and here is what you missed | shows the diff, offers to replay your work on top |
| gap | your cursor is older than the log still kept | says a fresh copy is needed |
| ahead | your cursor is past the server's | refuses, because this is not the plan you think it is |

Nothing is written on a refusal. The rebase is the planner's decision, in front
of a diff, rather than something that happens to their schedule quietly.

**Live collaborate** streams those same differences over a websocket while
several people have the plan open, so a levelling run or a date change appears
as it happens. A dropped connection catches up from its cursor before any live
message reaches it. Everyone's pointer is drawn where they are looking, carried
as a row and a position along the timescale rather than as pixels: two windows
are different sizes at different zooms, and a pixel position would put somebody
else's pointer where they are not.

**Giving somebody access** is an invitation to an address, claimed by whoever
holds it. The owner names an email; the person who opens the link proves that
address with their own sign in, and their copy claims the invitation the first
time it asks for the plan. Nothing is ever looked up by email, so there is no
way to probe the provider for who exists.

Somebody who is not a member is told a plan does not exist, rather than that
they may not have it. The distinction is the whole point: a 403 confirms the id
is real, and plan ids would then be worth guessing at.

**Links** are `aop://` addresses carrying the server and the plan. Opening one
while the application is already running opens it there rather than starting a
second copy, which matters more than it sounds: two copies of one plan, each
with its own change log and its own cursor, is exactly the state the `ahead`
answer above exists to detect. A link names the server it points at before
anything is fetched, since a link is an instruction from a stranger to talk to
a host of their choosing.

Signing in uses the authorization code flow with PKCE against an identity
provider you host yourself. The app ships with no default address: point it at
your own from Options > Alterion Collaborate, and the endpoints come from that
server's discovery document.

The server is `aop-collaborate`, Linux only, actix-web over Postgres. Its
README has the deployment.

## Not implemented

Honest list, kept current.

**Macros** record and replay, and the command vocabulary is complete, but there
is no editor for a recorded macro and no way to bind one to a key yet.

**Drawing** offers Line, Arrow, Rectangle, Oval and Text Box. Polygon and Arc
are deliberately absent: twenty years of forum traffic turned up nobody using
either, so they were not worth the vertex editor they need.

**Earned value** computes every measure and exposes them as columns, but there
is no Earned Value report page yet, and no Cash Flow. The timephasing they need
is built.

**Live collaborate** streams over a websocket and recovers a dropped
connection, but the server keeps its room in memory, so two instances behind a
load balancer each only reach their own clients. They converge on reconnect
because the database is the truth, but real multi instance live editing wants a
shared bus.

**Sharing a plan with somebody** has a table and a role model, and no endpoint
to grant with yet. Only whoever created a plan can open it.

## Settings and recovery

Preferences live in `~/.config/alterion-open-project/config.cfg`, a plain
`key = value` file meant to be edited by hand. They are deliberately kept out of
the `.aprj`, so a plan sent to someone else does not carry your name and palette
over theirs.

Tokens are never in that file. They are sealed against the machine and kept
where the platform keeps such things: a file of their own at `0600` on Linux,
the Keychain on macOS, the registry under DPAPI on Windows.

The plan is snapshotted every thirty seconds while there are unsaved changes, to
`~/.config/alterion-open-project/recovery/`. A session that ends without saving,
including a crash or a kill, is offered back on the next start. Snapshots are
named after the process that owns them, so two open windows never offer each
other's live work back as though it were lost.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Open an issue before writing code.

## Security

See [SECURITY.md](SECURITY.md). Report vulnerabilities privately to
<chaceberry686@gmail.com>, never in a public issue or merge request.

## Support

Bugs and feature requests belong in the issue tracker, where somebody else with
the same problem can find them. For anything that should not be public, email
<chaceberry686@gmail.com>.

## License

[Apache-2.0](LICENSE).
