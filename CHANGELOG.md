# Changelog

Notable changes, newest first. Dates are the day a version was tagged.

## 1.0.5-beta

**Print preview shows the whole document.** It paged one sheet at a time
behind four buttons, which meant scrolling a printed document was something you
could only do after saving it. It is now the document, scrolled. It is also the
same bytes that get printed rather than a second render of a one page range, so
the two cannot disagree, and turning a page no longer costs a full render of the
plan.

**Live collaborate actually carries what you do.** Everything in 1.0.4 was
correct about what to send and when; the socket could not be written to
between frames from the server.

`set_read_timeout` matched on the transport and did nothing for anything it
did not recognise. This crate asks tungstenite for rustls, `dioxus-devtools`
asks the same tungstenite for native-tls, cargo unifies the features, and
tungstenite prefers native-tls when both are on. So the socket was a variant
with no arm, no read timeout was ever set, and the read blocked until the
server happened to say something, which is every twenty five to thirty
seconds. Everything waiting went out in a clump at that moment, which is why
one pointer position was recorded and then nothing ever moved again.

Measured against an independent client watching the room: nothing but the
greeting in thirty seconds before, a hundred and twenty four pointer updates
in twenty five seconds after. Typing into a cell now appears on the other
screen letter by letter, and the committed change follows within the second.

The connection is made directly now and the timeout set on a duplicate of the
socket, which holds whichever TLS wrapper is chosen, so no dependency's
feature selection can take it away again.

Also: a log at `<config root>/log.log`, truncated at each start, recording
start up, signing in, and every live decision **including the decisions not to
send**. This failure worked once and then stopped silently, which a log of
successes alone cannot show.

## 1.0.4-beta

Live editing that carries what you do, and a way out to Microsoft Project.

### Live collaborate, which was wrong in both directions at once

The server renumbers every entry as it lands it, so a change sent as 6 comes
back to everybody, its author included, as 43. Comparing an id the server
chose against one this copy chose is comparing two numbering schemes that
share a type, and it failed opposite ways on the same line.

- **Fields duplicated on every sync.** Your own work came back over the
  socket wearing a number you did not recognise and was applied twice. The
  client also never sent the field the server reads to avoid echoing to the
  sender, and could not: nothing told it its own connection id.
- **Somebody else's change never appeared.** Local ids and server sequence
  numbers share a numbering space, so a copy holding unsent work holds an
  entry whose id will also be issued as a sequence number. The next change
  anybody made collided with it and was dropped as a duplicate, and the
  cursor advanced regardless, so it was never asked for again. The table was
  not failing to redraw; the change never arrived.
- **Your copy offered other people's work back to the server**, on every
  incoming change, writing a duplicate into the shared log for everybody.

The cursor decides now, because the cursor is the record of how far down the
shared log this copy has read.

- Saving a shared plan reaches the server, and can never fail the save for it.
- A batch nobody answers times out after twenty seconds instead of leaving a
  session silently mute, which is what a mismatched server did.
- `/api/health` publishes what protocol it speaks, so a mismatched pair says
  so instead of half working.

### Microsoft Project

- **Export as Microsoft Project XML.** Project opens it with File and Open and
  can save it as `.mpp` itself. Tasks, outline, links with type and lag,
  constraints, deadlines, baselines, actuals, resources, assignments and
  calendars, including per resource calendars and leave, which are written as
  a derived calendar per person. The 1504 task plan we test with round trips
  with no field mismatches and identical dates on both sides.
- **Importing a Project file was silently truncating text.** A name, note or
  author containing `&` or `<` arrived cut short: "Survey & report" imported
  as "report", because the parser kept only the last fragment of a run split
  at each entity reference. Every such name in every file ever imported was
  affected.
- Also now read, having been dropped before: baselines, task calendars,
  estimated durations, the actuals block, resource notes, email and code, and
  cost resources, which were arriving as work resources.

### Spreadsheets

- **A plan that would not schedule.** Dependency references written as WBS
  codes were parsed as row numbers, so `5.2.1` resolved to whichever task is
  fifth, and the resulting cycle meant nothing scheduled and every date read
  as zero. A WBS column is now read as both the outline and the identity its
  own dependency column cites.
- Cycles are refused at import with a notice naming the rows, rather than
  producing a plan that cannot be scheduled and does not say why.
- An outline value that cannot be a level, such as an issue number, is
  inherited from the row above rather than starting a ratchet that deepened
  every following row.
- A duration of a hundred years and a date at Excel's epoch zero are refused.
- The mapping page shows the columns it could place, with the rest a button
  away, rather than drawing a card for all ninety nine.

### Successors

A Successors tab in Task Information and a Successors column in the grid,
both the same picker as Predecessors pointed the other way. A successor is
not stored: it is the same link read from the other end, so editing either
end changes one thing.

### Also

- Opening a plan file no longer starts a second copy of the application. Two
  copies of one shared plan, each with its own log and cursor, is the state
  the sync protocol exists to detect.
- The About page names the engine rather than the toolkit it is built with.

## 1.0.3-beta

Live editing that is actually live, and Windows keeping what it is told.

### Windows

- **Nothing was remembered between launches.** Every path this application
  keeps a file at was worked out from `XDG_CONFIG_HOME` or `HOME/.config`,
  both Unix variables that Windows does not set, so all eight copies of that
  guess returned nothing there. The settings, the recent list, the added
  dictionary words, the crash recovery snapshots, the saved versions and the
  port file the single instance check depends on were silently never written.
  Nothing failed; everything simply forgot itself.
- **A console window flashed up** during printing and during sign in. A
  graphical application that starts a console program is given a window for
  it, and listing printers, sending a job, reading a stored token and probing
  the hardware are all console programs.
- **Updating no longer needs administrator rights.** It installs per user, to
  where the account can already write, which is what lets an application
  update itself at all. A machine wide option is still there and switches the
  install location, registry hive, shortcut scope and uninstaller together.
- The Open pane started in the directory the application happened to be
  launched from, and had no way off the drive it started on.

### Live collaborate

- **Changes stream.** Nothing pushed automatically before: a live session
  received other people's work and sent none of its own until somebody
  pressed Sync. Both transports now call one decision function, so the
  protocol cannot mean two things.
- **Two channels over one socket.** A change is durable: logged, sequenced,
  authored, replayable. A pointer, a selection, an open cell and a half typed
  word are ephemeral: broadcast and forgotten. Putting an interaction in the
  log would pollute the audit trail and make undo ambiguous.
- **Cursors glide** rather than jumping three times a second, and carry the
  person's name and picture. Moving a mouse cannot redraw the window.
- **A clean rebase happens quietly**, because with streaming "behind" is the
  ordinary state rather than an event. A batch that would touch the cell
  somebody has open is held until they close it, so the ground never moves
  under their fingers.
- **Pull Changes**, which always shows what it would do before it does it.
- **A plan opened from a server has a file.** It existed only in memory, so
  closing the application lost the plan, its log and any unsent work. It now
  opens at file speed and catches up from its cursor, and Save As keeps it
  syncing rather than silently unlinking it.

### Also

- **Save As asked nothing before writing over an existing file.** It offers
  to replace, to keep both with a numbered name, or to stop.
- Previewing somebody else's change replayed it for real, which closed the
  open cell editor and threw away a half typed word.
- Per resource calendars, so leave has somewhere to live, with public
  holidays importable from an iCalendar file into whichever calendar you
  choose.
- An Import page for spreadsheets nobody here wrote: pick the sheet, the
  heading row and what each column means, with real data shown under each.
- The Team Planner drew every bar in a lane at the same height, so a plan
  with unassigned work was one unreadable smear.
- Sharing a plan means something: an owner invites an address and whoever
  opens the link claims it by proving that address.

## 1.0.2-beta

Windows fixes, found by running it on Windows.

- **The Open and Save As panes started nowhere.** The folder they begin in was
  read from `HOME`, which is a Unix variable that Windows does not set, so
  every Windows copy fell through to `.`, the directory the application
  happened to be launched from. It asks for `USERPROFILE` first there, falls
  back to `HOMEDRIVE` and `HOMEPATH` for the case where Windows splits it in
  two, and checks the Documents folder exists before starting in it, since it
  can be renamed, redirected to a network share, or simply absent.
- **The Open pane had no way off the drive it started on.** Going up walks to
  the parent, and the parent of `C:\` is nothing. It has drive buttons and a
  Home button now, the same as the browser in the dialogs, which got them
  first.

## 1.0.1-beta

Everything here was found by running the thing against a real deployment
rather than a test, which is a fair description of what the tests were
missing.

### Files that could not be found

- **A workbook was invisible in the Open page**, on every platform. Three
  copies of "which files can this application open" had drifted apart: the
  Open page listed no spreadsheet at all, the browser dialog listed `.xlsx`
  but not `.xls`, `.xlsm` or `.ods`, and the opener reads all of them. One
  function answers that now, and a test asserts the browser offers everything
  the opener can read, because a file the application can read but will not
  show is, to the person looking for it, a file it cannot read.
- **On Windows you could not leave the drive you started on.** Going up walks
  to the parent, and the parent of `C:\` is nothing, so a plan on another
  drive or a network share was unreachable. There are drive buttons now.

### Copying

- **Every copy in the application silently did nothing on Windows.**
  `navigator.clipboard` exists only in a secure context. WebKitGTK trusts the
  application's own protocol and hands the clipboard over; WebView2 does not,
  so the object was undefined and the call short circuited to nothing at all,
  with no error anywhere. There is a fallback for the plain context now.

### Signing in and accounts

- An **account card**: avatar, name, address, and a button that opens your
  account page in the browser. The five stacked reassurance boxes it replaces
  said the same thing twice and buried the one line anybody reads.
- The **name other people see** in a shared plan now comes from the account
  rather than from whatever was typed locally. A locally typed name is kept
  and comes back on sign out.
- Account details are re-read when the Collaborate page is opened, when the
  window comes back, and on demand. Relying on the window regaining focus
  alone was a mistake: a compositor is under no obligation to report it.

### Sharing

- **`aop://` links.** Clicking one opens the plan in the copy already running
  rather than starting a second, which matters more than it sounds: two copies
  of one plan, each with its own change log and cursor, is precisely the state
  the sync protocol's `ahead` answer exists to detect.
- A **Cloud** button and a **Collaborate** button in the Quick Access Toolbar.
  Collaborate starts live editing and copies the link in one press, and copies
  again rather than tearing the session down.
- **History and Sync is a sidebar**, not a view. Checking whether you are
  current, reading what came in and picking a version to return to are all
  things done while looking at the plan.

### Live collaborate

- **Pointers**, carried in plan coordinates rather than pixels. Everyone has a
  different window size, zoom and scroll offset, so a pixel position draws
  somebody's pointer where they are not. A pointer whose row is not on screen
  is not drawn at all rather than pinned to an edge, because an edge marker
  would claim they are somewhere they are not.

### Corrections

- **A real token was refused while a fake one was not.** The identity provider
  returned `scope` as a JSON array; RFC 7662 defines it as one space separated
  string. An inactive answer carries no scope, so bogus tokens parsed and were
  correctly refused, while every genuine token failed to parse and answered
  502. Fixed on both sides.
- **The editing box and picker on Predecessors and Resources were dead.** A
  panel positioned by hand had lost its positioning rule, so it laid out in
  normal flow and painted below the window, while the scrim over it took every
  click and closed the editor. A test now asserts that anything positioned by
  an inline offset carries a rule that takes it out of the flow.
- A failed identity provider call **says why in the log**. It returned the
  reason to the caller and recorded nothing, so the server knew exactly what
  was wrong and told only the party least able to act on it.

## 1.0.0-beta

The first release with collaboration, and a great many corrections to work that
looked finished and was not.

### Sharing a plan

**Alterion Collaborate.** A plan can now live on a server, carry a record of
who changed what, and be worked on by more than one person.

- **A change log inside the plan.** Every edit is recorded as the command that
  made it, with an author and a moment. That one choice makes it an audit
  trail, the unit a sync exchanges, and something a macro can replay, all from
  one thing. It travels with the file.
- **Sync by difference, not by file.** A client pushes against the point it
  last saw. If the server has moved on it is handed what it missed in the same
  answer and can replay its own work on top. If its cursor is older than the
  log still held, it is told so distinctly rather than being given a subtly
  incomplete answer. Nothing is written on a refusal.
- **Live collaborate** over a websocket, with presence, and a reconnecting
  client catching up from its cursor before any live message reaches it.
- **Comparing two plans**, which also answers the Compare Projects gap. Tasks
  are matched by identity rather than row, so a reordered plan reports moves
  instead of looking wholly rewritten, and only edits are reported, not
  everything the scheduler recomputed as a consequence.
- **Signing in** with the authorization code flow and PKCE against a
  self-hostable identity provider. Endpoints come from its discovery document,
  so pointing at your own means changing one address.
- **Tokens bound to the machine.** Sealed with a key derived from hardware that
  can be read without administrator rights, then kept in a file of their own at
  `0600` on Linux, the Keychain on macOS, and the registry under DPAPI on
  Windows. A copied file is no use on another machine. Never in `config.cfg`.

### Earned value

Absent entirely before this. Actuals on the model, a timephasing primitive that
spreads a quantity across working time, then PV, EV and AC and the ten measures
derived from them, exposed as columns.

Earned value here is not percent complete times the budget. It walks that
percentage along the baseline's own duration axis and reads the cost curve
there, so a front-loaded task earns what it actually earned. Microsoft's worked
example is a test, and so is the naive formula being wrong.

### Corrections

Several of these were wrong in ways that looked right.

- **The critical path is now a backward trace of driving relationships** from
  the tasks that finish last, which is what the standards define. The previous
  walk summed durations over links between zero-slack tasks and truncated
  silently in four ways: a milestone-to-milestone link, two tasks tying on
  start date, a link attached to a summary, or a switched-off task each
  returned a fragment of the chain with no sign that it had. Parallel critical
  paths were dropped entirely.
- **Path length was the sum of task durations and ignored lag**, so a chain
  running 5 to 20 January reported two days. It is the elapsed working span.
- **Total slack was the finish slip only.** It is the smaller of the start and
  finish slips, as Microsoft defines it, which matters because criticality
  hangs off that field.
- **A dismissed warning no longer changes the critical path.** Acknowledging a
  warning is a statement about the warning list, not the schedule; it was
  breaking the chain in half.
- **A switched-off task no longer holds up the ones after it**, nor sets the
  project finish. The logic either side of it is bridged, as Project has done
  since 2013.
- **Opening a plan now schedules it.** Everything derived was left at zero
  until something else asked for a reschedule, which the window did and an
  export did not: 189 of 242 tasks exported a duration of zero to Excel.
- **Dependency loops name the loop**, not the two hundred tasks leading into
  it.

### Reports

The burn charts stopped claiming things the data cannot support.

- The burnup's scope line never moved and never could, since no record is kept
  of when scope changed. It says so, and shows the movement that is supported.
- The chart and the figures beside it no longer disagree about what remains.
- The actual line stops at the status date instead of running flat into the
  future.
- A plan three months late no longer reports itself on plan.
- Duration as a basis is gone in favour of counting tasks. Summing durations
  across work that overlaps gives a number that is neither effort nor elapsed
  time.
- Velocity counts finished tasks only, over iterations the plan declares rather
  than windows guessed from a calendar.
- Resource Usage exists, rather than quietly showing the Resource Sheet under a
  tab bearing its name.

### New

- **Drawing** on the Gantt: line, arrow, rectangle, oval and text box, pinned
  either to a date or to a task bar, storing intent rather than pixels so they
  survive a zoom and a reschedule.
- **Macros**: a command vocabulary that records, replays and reads as script.
- **Resource levelling**, **Group by**, **Fill Down**, **subprojects**, **text
  styles** and the **Format Painter**, all of which the ribbon offered and none
  of which did anything.
- **Update Project**: mark work complete through a date, or reschedule what has
  not happened past it.
- **Excel import and export**, round-tripping tasks, links and resources.
- **Printing** to a real printer through CUPS on Linux and macOS and through
  the print handler on Windows, with copies, a page range and a paged preview.
  It previously wrote an HTML page.
- **Spelling** with downloadable dictionaries, **custom fields**, and
  **external dependencies**.

### Packaging

- A **Windows installer** that no longer opens a console window behind the app,
  and that carries `WebView2Loader.dll`, without which it did not start at all.
  It installs the WebView2 runtime only when it is genuinely absent.
- A **macOS** universal build and disk image script.
- `install.sh` for Linux, and an AUR PKGBUILD.

## 0.1.0-beta

First public build. Reads and writes `.aprj`, imports `.mpp` and MSPDI,
schedules with a critical path engine, and draws a Gantt chart.
