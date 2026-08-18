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
[![Rust](https://img.shields.io/badge/Rust-2021-orange?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Dioxus](https://img.shields.io/badge/Dioxus-0.7-blueviolet?style=flat)](https://dioxuslabs.com/)
[![Status](https://img.shields.io/badge/status-Open--Beta-cyan)](#not-implemented)

_A better free project scheduler. A real critical path engine behind a Microsoft Project style ribbon, reading Project's own files._

</div>

---

## What it does

A desktop project scheduler: forward and backward pass, slack, critical path,
summary rollup, resource booking and overallocation, drawn behind a ribbon that
follows Microsoft Project's layout.

Plans are saved as `.aprj`, a binary container (`APRJ` magic, versioned header,
deflated MessagePack). Microsoft Project plans open directly, both binary `.mpp`
and XML Format.

```
cargo run -p aop-app        # launch
cargo test                  # 146 tests: engine, import, geometry, settings, interactions
```

## Layout

```
crates/aop-core     the plan and the maths, no UI, fully unit tested
  calendar.rs       working-time arithmetic (shifts, weekends, holidays)
  duration.rs       "5d" / "2w" / "4h" parsing and display
  model.rs          Project, Task outline, Link, Resource, Constraint, Baseline
  schedule.rs       topological sort -> forward pass -> backward pass ->
                    slack -> critical path -> summary rollup -> cost -> overallocation
  templates.rs      eight starter plans, which double as test fixtures
  persist.rs        .aprj binary container, CSV export, print-ready HTML
  mspdi.rs          Microsoft Project XML (MSPDI) import
  mpp.rs            maps a parsed .mpp onto the plan

crates/aop-app      the desktop shell
  theme.rs          the whole stylesheet, inline (no asset pipeline needed)
  brand.rs          the Alterion wordmark, inlined as SVG
  ribbon.rs         custom title bar, QAT, tab strip, seven ribbon tabs
  backstage.rs      File menu: dashboard, templates, browser, print, export,
                    About, and a Project-style Options screen
  grid.rs           the Entry table, editable, drag to reorder
  gantt.rs          timescale, bars, arrows, and bar dragging
  views.rs          resource sheet, task usage, network, calendar, team planner
  dialogs.rs        task information, project information, working time,
                    assign resources, customize Quick Access Toolbar
  contextmenu.rs    right-click menus
  popups.rs         predecessor and resource pickers
  controls.rs       themed Dropdown, ComboBox and ribbon MenuBtn
  preview.rs        the mini Gantt used by thumbnails and previews
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

`packaging/` holds both routes. Linux has no installer: the AUR `PKGBUILD`
builds from source and hands the files to pacman, which is what Arch expects.
Windows gets an NSIS installer that cross-builds from Linux.

```
cd packaging/aur && makepkg -si          # Linux
packaging/windows/build-installer.sh     # Windows setup.exe
```

Both share `packaging/linux/`: the desktop entry, the scalable icon (which the
Windows `.ico` is generated from), and the MIME definition that registers
`.aprj` against its `APRJ` magic bytes rather than just the extension.

## Not implemented

The ribbon is drawn in full, but these report on the status bar rather than
doing anything: Format Painter, font and colour controls, Text Styles,
Gridlines, Layout, Insert Column, Custom Fields, resource levelling, Group by,
Macros, Spelling and Subproject.

Print writes a print-ready HTML page rather than talking to a printer.
Resource Usage shows the Resource Sheet.

## Settings and recovery

Preferences live in `~/.config/alterion-open-project/config.cfg`, a plain
`key = value` file meant to be edited by hand. They are deliberately kept out of
the `.aprj`, so a plan sent to someone else does not carry your name and palette
over theirs.

The plan is snapshotted every thirty seconds while there are unsaved changes, to
`~/.config/alterion-open-project/recovery/`. A session that ends without saving,
including a crash or a kill, is offered back on the next start. Snapshots are
named after the process that owns them, so two open windows never offer each
other's live work back as though it were lost.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Open an issue before writing code.

## Security

See [SECURITY.md](SECURITY.md). Report vulnerabilities privately, never in a
public issue or merge request.

## License

[Apache-2.0](LICENSE).
