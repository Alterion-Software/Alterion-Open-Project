# Getting the webview-free build to parity

The goal is one sentence: **the native build does everything the webview build
does.** This is how, in the order the dependencies actually run, with a gate
after the first step because the first step can say no.

Written 2026-08-20, after a day of finding out what breaks by running it.

## The finding that shapes all of this

The native build cannot scroll, and no amount of work in this repository
changes that.

```
  blitz-traits 0.2 can produce exactly nine events

    MouseMove  MouseDown  MouseUp  Click
    KeyPress   KeyDown    KeyUp
    Input      Ime

  no wheel        no scroll       no resize       no mounted
```

`dioxus-native-dom` 0.7.10 leaves seventeen event converters as
`unimplemented!`, and five of them (mounted, pointer, scroll, touch, wheel) are
implemented in 0.8.0-alpha.1. Those five are not a patch anybody can backport:
they exist because blitz 0.3 added the event types underneath them. On blitz
0.2 there is nothing to convert.

So everything else here is downstream of one decision: **move to Dioxus 0.8, or
stop.**

## What that decision costs

`dioxus` is one dependency for both builds. There is no way to hold the webview
build on 0.7 while the native build moves to 0.8, so this moves the **shipping**
product onto an alpha of its framework, while a customer is evaluating it.

That is the risk of this whole plan, and it is why the first phase exists.

## Phase 0. Cost the upgrade

Half a day, on a branch, and it is allowed to end the plan.

Bump `dioxus` to 0.8.0-alpha.1 and the blitz crates to 0.3.0-beta.1. Drop
`vendor/dioxus-native-dom` (0.8 implements `element_coordinates`), and check
whether `vendor/dioxus-native` is still needed for the bundled font or whether
0.8 accepts a `FontContext`.

Then compile both builds and run the suite.

**What comes out of it is a number**, not an opinion:

- compile errors in the webview build
- failures out of 1066 tests
- whether the webview build still starts, opens a plan and prints

**The gate.** If the webview build needs more than about a day to bring back,
stop and reconsider. The shipping product with a customer waiting outranks the
port. Reconsidering means: wait for 0.8 stable, and in the meantime accept a
native build that cannot scroll and is therefore not a build anybody can use.

## Phase 1. Move to 0.8

Fix what Phase 0 found. Done when both builds compile, 1066 tests pass, clippy
is silent, and the **webview** build has been driven by hand through the parity
checklist in Phase 5. The webview build is the one with users; it does not
regress.

Re-examine both vendored crates here. If 0.8 makes either unnecessary, delete
it. Carrying a patched copy of somebody else's crate is a debt and it should be
paid off the moment it stops earning.

## Phase 2. Close what 0.8 still leaves

Still `unimplemented!` in 0.8.0-alpha.1:

```
  resize   drag   clipboard   selection   composition
  animation   cancel   image   media   toggle   transition   visible
```

The first five matter here. The rest do not appear in this application.

**The viewport.** `onresize` is how the window size is learned today, and it is
stubbed in both versions. `onmounted` is implemented in 0.8, so measure the
root element instead. Until then, `crate::placement` treats an unknown viewport
as unknown, which keeps panels where they were asked for rather than in a
corner, but no panel will flip away from an edge.

**Scroll.** Two things depend on it. The grid's virtualisation reads scroll
position, and the pane sync is currently one of only two pieces of JavaScript
in the application, at `main.rs:704`, which cannot run natively at all. Both
want rewriting against the scroll events 0.8 provides. Removing that `eval` is
worth doing on its own account.

**Clipboard.** The other `eval`, at `controls.rs:344`. Replace it with a real
clipboard crate. That also fixes a webview bug: WebView2 gives no secure
context to a custom protocol, so `navigator.clipboard` silently does nothing on
Windows today, and the `execCommand` fallback exists only because of it.

**Drag.** Audit first. If row dragging and column resizing are built on
`onmousedown`/`onmousemove`, they already work and `convert_drag_data` being
stubbed is irrelevant. If they use HTML5 drag events, they have to be rewritten
against mouse events, because drag is stubbed in both versions.

**Composition.** IME, which is how anybody types a language that is not Latin.
Test it. If it is broken, decide whether it blocks a release rather than
discovering that from a user.

## Phase 3. The rendering gaps

Independent of the upgrade and can run beside it.

| | |
|---|---|
| `position: fixed` | Taffy has no `fixed`, and its `absolute` is measured from the **parent**, not the nearest positioned ancestor. Every menu, dropdown and picker depends on it. Either blitz grows containing blocks, or the panels move to the root and their coordinates become parent-relative. Decide which before writing anything. |
| CSS inside SVG | A stylesheet does not reach into an inline SVG here. Colours and fonts have to be on the element. Mostly done; the logo (#6) is what is left. |
| `writing-mode` | The vertical view bar (#8). Currently clipped so it cannot paint across the window. Rebuild it without vertical writing rather than wait for support. |
| `repeat(auto-fill, minmax())` | Collapses to no tracks. Four rules converted to wrapping flex already; check no new ones appear. |
| Layout differences | #12, including the table clipping its last columns. Collect them, look for the shared cause, and resist fixing each with a magic number. |

## Phase 4. What the webview simply gave for free

**Print preview** (#10). `<object type="application/pdf">` with no renderer
behind it. Do not rasterise the PDF: `aop_core::pdf` writes the document, so
draw the preview from the plan directly. Whatever replaces it must keep the
property that what is previewed is what is printed, which is currently
guaranteed by them being the same bytes.

**Animated avatars** (#9). Needs a decoder that advances frames and a way to
drive redraws from it.

**Accessibility.** Not yet looked at, and it should be before anybody promises
this to a client with requirements. The webview gave screen reader support
essentially free and the native stack does not.

## Phase 5. Parity, and only then cutover

A checklist, feature by feature, ticked in **both** builds by hand. Not a
summary, a list: every ribbon tab, every dialog, every view, open, save,
import, export, print, sync, live collaboration, the update flow.

The webview build stays the shipping build until every line ticks. Nothing
about this plan requires it to be turned off early, and a customer evaluating
the product is a reason to keep the boring option available.

## How to work on it

Three things went wrong today, in the same way each time, and they cost more
than any of the bugs did.

**Instrument before investigating.** Live collaboration took days of reading
and was answered in one run once there was a log. The segfault took four
crashes and was answered by the first core file. Reach for the instrument
first, not third.

**Check the claim before acting on it.** `position: fixed` was swapped for
`absolute` without checking what Taffy's `absolute` means. Seventeen stubbed
converters were reported as three because of a grep that missed every one
carrying a message, and recommendations were made on that number. Both checks
were one command.

**One gap wears several costumes.** `var()` in SVG attributes was the chart and
the cursors and the logo. A missing `viewBox` was the row pitch and the
oversized text. A viewport of zero was the picker and the context menu. When
several things break at once, look for the one cause before filing several
bugs.
