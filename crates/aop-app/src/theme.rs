//! The stylesheet, kept inline so the binary runs from `cargo run` with no
//! asset pipeline.
//!
//! The layout follows Microsoft Project (green-bar chrome, ribbon groups, a
//! grid beside a timescale) but the palette is Alterion's: near-black surfaces,
//! a muted teal accent and pale teal text, taken from the Alterion website.

pub const CSS: &str = r##"
:root {
  /* Alterion palette */
  --bg: #0d0f10;
  --surface: #131718;
  --surface-2: #0d1a1a;
  --surface-3: #171d1e;
  --surface-4: #1b2223;

  --accent: #81b5b5;
  --accent-bright: #a5d3d3;
  /* Ink for text sitting on the accent or the contextual colour. Those are the
     pale elements on this palette, so it is the dark one. It flips with the
     palette; anything hardcoding a colour here is a bug waiting for a theme. */
  --on-accent: #0b1414;
  /* Ink for text sitting on a chart bar. The bars are a mid tone in both
     palettes, so unlike --on-accent this does not flip. */
  --on-bar: #f2f7f7;
  --accent-dim: rgba(129, 181, 181, 0.14);
  --accent-line: rgba(129, 181, 181, 0.42);
  --contextual: #8aa2c4;

  --line: #27302f;
  --line-soft: #1d2425;
  --ink: #d8e7e8;
  --ink-soft: #8fafaf;
  --ink-faint: #5f7676;

  --hover: rgba(216, 231, 232, 0.065);
  --pressed: rgba(216, 231, 232, 0.12);
  --selection: rgba(129, 181, 181, 0.17);
  --selection-line: rgba(129, 181, 181, 0.55);
  --focus: var(--accent);

  --grid-line: #222a2b;
  --grid-header: #171d1e;
  --nonworking: rgba(216, 231, 232, 0.032);

  /* chart */
  --bar: #3f7d7d;
  --bar-edge: #6aadad;
  --bar-progress: #a5d3d3;
  --bar-critical: #9d474d;
  --bar-critical-edge: #d9636a;
  --bar-progress-critical: #e79aa0;
  --bar-summary: #cfe3e3;
  --bar-inactive: #414c4c;
  --baseline: #6b7f7f;
  --slack: #4d6060;
  --today: #d9636a;
  --link-arrow: #7e9a9a;

  --danger: #d9636a;
  --danger-bg: rgba(217, 99, 106, 0.12);
  --warn: #d9b06a;

  --shadow: 0 12px 34px rgba(0, 0, 0, 0.55);

  /* Families are listed most-wanted first, but every name here must be one
     that actually exists somewhere, because a matcher that falls back by
     substring can land on an unrelated font whose name merely contains the
     word. "Inter" matching a symbol font called CustomTkinter_shapes_font is
     exactly that, and it renders text as arbitrary shapes. */
  --font: "Inter", "InterVariable", "Segoe UI", "Noto Sans", "DejaVu Sans", "Liberation Sans", sans-serif;
  --mono: ui-monospace, "Cascadia Mono", "JetBrains Mono", Consolas, monospace;
}

* { box-sizing: border-box; }

html, body, #main {
  height: 100%;
  margin: 0;
  padding: 0;
  overflow: hidden;
}

body {
  font-family: var(--font);
  font-size: 12px;
  color: var(--ink);
  background: var(--bg);
  -webkit-user-select: none;
  user-select: none;
  cursor: default;
  -webkit-font-smoothing: antialiased;
  text-rendering: optimizeLegibility;
}

.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
  /* No OS decorations, so the app draws its own edge. */
  border: 1px solid #202a2a;
}

button { font: inherit; color: inherit; }

/* ---------- scrollbars ---------- */

::-webkit-scrollbar { width: 13px; height: 13px; }
::-webkit-scrollbar-track { background: var(--bg); }
::-webkit-scrollbar-thumb { background: #2c3737; border: 3px solid var(--bg); border-radius: 8px; }
::-webkit-scrollbar-thumb:hover { background: #3d4c4c; }
::-webkit-scrollbar-corner { background: var(--bg); }

/* ---------- title bar ---------- */

.titlebar {
  display: flex;
  align-items: center;
  height: 30px;
  background: var(--surface-2);
  color: var(--ink);
  padding: 0 6px;
  flex: none;
  border-bottom: 1px solid var(--line-soft);
}

.qat { display: flex; align-items: center; gap: 1px; padding-left: 2px; }

.qat-btn {
  width: 24px;
  height: 24px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
}

.qat-btn:hover:not(:disabled) { background: var(--hover); color: var(--accent-bright); }
.qat-btn:active:not(:disabled) { background: var(--pressed); }
.qat-btn:disabled { opacity: 0.32; }

.qat-sep { width: 1px; height: 15px; margin: 0 4px; background: var(--line); }

.drag-region {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  min-width: 0;
}

.wincontrols { display: flex; flex: none; margin-right: -6px; }

.wc {
  width: 44px;
  height: 30px;
  display: grid;
  place-items: center;
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
}

.wc:hover { background: var(--hover); color: var(--ink); }
.wc.close:hover { background: var(--danger); color: #fff; }

.title-text {
  text-align: center;
  font-size: 12px;
  color: var(--ink-soft);
  letter-spacing: 0.2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  padding: 0 12px;
}

.title-text b { color: var(--ink); font-weight: 500; }

/* ---------- contextual tools banner ---------- */

.tools-banner {
  display: flex;
  align-items: stretch;
  height: 16px;
  background: var(--surface-2);
  flex: none;
  overflow: hidden;
}

/* Hidden copies of the tabs, purely to reserve the same widths. */
.tools-banner .ghost {
  visibility: hidden;
  height: 16px;
  border-top: 0;
  pointer-events: none;
  flex: none;
}

.tools-label {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--contextual);
  color: var(--on-accent);
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.7px;
  text-transform: uppercase;
  padding: 0 14px;
  border-radius: 4px 4px 0 0;
  white-space: nowrap;
  flex: none;
}

/* ---------- ribbon tab strip ---------- */

.tabstrip {
  display: flex;
  align-items: stretch;
  height: 27px;
  background: var(--surface-2);
  flex: none;
}

.tab {
  display: flex;
  align-items: center;
  padding: 0 14px;
  color: var(--ink-soft);
  font-size: 12px;
  border: 0;
  border-top: 2px solid transparent;
  background: transparent;
  cursor: default;
  white-space: nowrap;
}

.tab:hover { color: var(--ink); background: var(--hover); }

.tab.active {
  background: var(--surface);
  color: var(--accent-bright);
  border-top-color: var(--accent);
}

.tab.file {
  background: var(--accent);
  color: var(--on-accent);
  font-weight: 600;
  padding: 0 17px;
  border-top-color: var(--accent);
}

.tab.file:hover { background: var(--accent-bright); color: var(--on-accent); }

.tab.contextual { background: transparent; color: var(--contextual); }
.tab.contextual:hover { background: var(--hover); }
.tab.contextual.active { background: var(--surface); border-top-color: var(--contextual); color: var(--contextual); }

.tabstrip .filler { flex: 1; }

/* ---------- ribbon ---------- */

.ribbon {
  display: flex;
  align-items: stretch;
  background: var(--surface);
  border-bottom: 1px solid var(--line);
  height: 94px;
  flex: none;
  overflow: hidden;
}

.ribbon.collapsed { height: 0; }

.ribbon-scroll {
  display: flex;
  align-items: stretch;
  overflow-x: auto;
  overflow-y: hidden;
  flex: 1;
}

.ribbon-scroll::-webkit-scrollbar { height: 5px; }

.rgroup {
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--line-soft);
  padding: 3px 6px 0;
  flex: none;
}

.rgroup-body {
  display: flex;
  /* Stretch, so a button with a long caption sets the height and the rest come
     up to meet it rather than one spilling past a fixed box. */
  align-items: stretch;
  gap: 2px;
  flex: 1;
  padding-bottom: 2px;
}

.rgroup-title {
  text-align: center;
  font-size: 9.5px;
  color: var(--ink-faint);
  letter-spacing: 0.3px;
  padding: 1px 0 3px;
  white-space: nowrap;
}

.rgroup-title .launcher { display: inline-block; margin-left: 4px; opacity: 0.6; }

.rcol { display: flex; flex-direction: column; gap: 1px; }
.rrow { display: flex; align-items: center; gap: 2px; }

.rbtn-lg {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  min-width: 48px;
  max-width: 76px;
  /* A minimum rather than a fixed height. A caption like "Change Working Time"
     wraps to three lines, and against a fixed height it simply spilled out of
     the button. The row stretches its buttons to match, so they stay level. */
  min-height: 66px;
  padding: 4px 5px 2px;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  cursor: default;
  text-align: center;
  line-height: 1.15;
  font-size: 11px;
}

.rbtn-lg .glyph { height: 32px; flex: none; display: grid; place-items: center; color: var(--accent); }
.rbtn-lg .caption { white-space: normal; overflow-wrap: anywhere; }

.rbtn-sm {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 21px;
  padding: 0 6px;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  cursor: default;
  white-space: nowrap;
  font-size: 11px;
  max-width: 178px;
}

.rbtn-sm .glyph { display: grid; place-items: center; width: 16px; flex: none; color: var(--accent); }
.rbtn-sm .caption { overflow: hidden; text-overflow: ellipsis; }

.rbtn-icon {
  width: 22px;
  height: 21px;
  display: grid;
  place-items: center;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
}

.rbtn-lg:hover:not(.disabled), .rbtn-sm:hover:not(.disabled), .rbtn-icon:hover:not(.disabled) {
  background: var(--hover);
  border-color: var(--line);
}

.rbtn-lg:active:not(.disabled), .rbtn-sm:active:not(.disabled), .rbtn-icon:active:not(.disabled) {
  background: var(--pressed);
}

.rbtn-lg.on, .rbtn-sm.on, .rbtn-icon.on {
  background: var(--accent-dim);
  border-color: var(--accent-line);
}

.disabled { opacity: 0.32; }

.caret { font-size: 8px; opacity: 0.65; line-height: 1; }

.rcheck {
  display: flex;
  /* Centred on the box rather than the text baseline: the label is 11px and
     the box 12px, so aligning by baseline leaves the tick sitting low. */
  align-items: center;
  gap: 7px;
  height: 20px;
  padding: 0 5px;
  font-size: 11px;
  line-height: 1;
  border-radius: 3px;
  cursor: default;
  white-space: nowrap;
  text-align: left;
}

.rcheck:hover { background: var(--hover); }

.rcheck .box {
  width: 12px;
  height: 12px;
  border: 1px solid var(--ink-faint);
  border-radius: 2px;
  background: transparent;
  display: grid;
  place-items: center;
  /* The tick is drawn from the font, so it needs its own line box or it sits
     a pixel low inside the square. */
  font-size: 9px;
  line-height: 1;
  flex: none;
}

.rcheck .box.on { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }

.font-row { display: flex; gap: 2px; align-items: center; }

.rselect {
  height: 20px;
  border: 1px solid var(--line);
  border-radius: 3px;
  background: var(--surface-3);
  font-size: 11px;
  padding: 0 5px;
  color: var(--ink);
  -webkit-user-select: text;
  user-select: text;
}

.rselect:focus { outline: none; border-color: var(--accent); }

/* ---------- dropdown ---------- */

.dd {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 7px;
  height: 20px;
  padding: 0 7px;
  border: 1px solid var(--line);
  border-radius: 3px;
  background: var(--surface-3);
  color: var(--ink);
  font-size: 11px;
  cursor: default;
  text-align: left;
  min-width: 0;
}

.dd.lg { height: 30px; font-size: 12px; padding: 0 10px; border-radius: 4px; }
.dd:hover:not(.disabled) { border-color: var(--accent-line); background: var(--surface-4); }
.dd.disabled { opacity: 0.38; }
.dd .dd-value { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dd .dd-caret { font-size: 8px; color: var(--ink-soft); flex: none; }

.dd-list {
  position: fixed;
  z-index: 90;
  background: var(--surface-4);
  border: 1px solid var(--line);
  border-radius: 5px;
  box-shadow: var(--shadow);
  padding: 4px;
  max-height: 320px;
  overflow-y: auto;
}

.dd-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 6px 9px;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  font-size: 12px;
  text-align: left;
  cursor: default;
  white-space: nowrap;
}

.dd-item:hover { background: var(--accent-dim); color: var(--accent-bright); }
.dd-item .tick svg { color: var(--accent); }

/* ---------- combo box ---------- */

.combo {
  display: flex;
  align-items: stretch;
  height: 20px;
  border: 1px solid var(--line);
  border-radius: 3px;
  background: var(--surface-3);
  overflow: hidden;
}

.combo:focus-within { border-color: var(--accent); }

.combo-input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--ink);
  font: inherit;
  font-size: 11px;
  padding: 0 6px;
  -webkit-user-select: text;
  user-select: text;
}

.combo-caret {
  width: 18px;
  border: 0;
  border-left: 1px solid var(--line);
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  display: grid;
  place-items: center;
}

.combo-caret:hover { background: var(--hover); color: var(--accent-bright); }
.dd-item.on { color: var(--accent-bright); }
.dd-item .tick { width: 12px; flex: none; color: var(--accent); }

.gallery { display: flex; gap: 5px; align-items: center; padding: 2px 0; }

.gallery-item {
  width: 60px;
  height: 46px;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: var(--surface-2);
  padding: 6px 5px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  justify-content: center;
  cursor: default;
  flex: none;
  overflow: hidden;
}

.gallery-item:hover { border-color: var(--accent-line); background: var(--surface-3); }

.gallery-item.on {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-dim);
}

.gallery-item .g-bar { height: 4px; border-radius: 2px; flex: none; }

/* ---------- backstage ---------- */

.backstage {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  background: var(--bg);
}

.bs-nav {
  width: 196px;
  background: var(--surface-2);
  border-right: 1px solid var(--line);
  display: flex;
  flex-direction: column;
  padding: 6px 0 10px;
  flex: none;
}

.bs-back {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 40px;
  padding: 0 14px;
  border: 0;
  background: transparent;
  color: var(--accent);
  cursor: default;
  font-size: 13px;
}

.bs-back:hover { background: var(--hover); }

.bs-item {
  display: flex;
  align-items: center;
  gap: 11px;
  text-align: left;
  padding: 8px 18px;
  border: 0;
  border-left: 2px solid transparent;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  font-size: 13px;
}

/* The glyph column is a fixed width so the labels line up whatever each icon
   happens to measure. */
.bs-item .glyph {
  display: grid;
  place-items: center;
  width: 16px;
  flex: none;
  color: var(--ink-faint);
}
.bs-item.active .glyph, .bs-item:hover .glyph { color: var(--accent); }

.bs-item:hover { background: var(--hover); color: var(--ink); }
.bs-item.active { background: var(--accent-dim); border-left-color: var(--accent); color: var(--accent-bright); }
.bs-sep { height: 1px; background: var(--line); margin: 7px 14px; }
.bs-spacer { flex: 1; min-height: 12px; }

.bs-body {
  flex: 1;
  min-width: 0;
  padding: 26px 40px 40px;
  overflow-y: auto;
}

.bs-title {
  font-size: 30px;
  font-weight: 300;
  letter-spacing: -0.6px;
  color: var(--ink);
  margin: 0 0 20px;
}

.bs-sub {
  font-size: 14px;
  font-weight: 600;
  margin: 24px 0 10px;
  color: var(--ink);
  letter-spacing: 0.1px;
}

/* ---------- splash ---------- */

.splash {
  position: fixed;
  inset: 0;
  z-index: 200;
  display: flex;
  align-items: stretch;
  background: var(--bg);
  animation: splash-in 260ms ease-out;
}

@keyframes splash-in { from { opacity: 0; } to { opacity: 1; } }

.splash-left {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 10px;
  padding: 0 56px;
  min-width: 0;
}

.splash-logo { color: var(--ink); }
.splash-logo svg { width: 100%; height: 100%; display: block; }

.splash-product {
  font-size: 15px;
  letter-spacing: 4.4px;
  text-transform: uppercase;
  color: var(--ink-faint);
  padding-left: 3px;
}

.splash-version { font-size: 11.5px; color: var(--ink-faint); padding-left: 3px; margin-top: 8px; }

.splash-bar {
  width: 240px;
  height: 3px;
  border-radius: 2px;
  background: rgba(216, 231, 232, 0.09);
  overflow: hidden;
  margin: 12px 0 4px 3px;
}

.splash-fill {
  height: 100%;
  width: 40%;
  border-radius: 2px;
  background: var(--accent);
  animation: splash-sweep 1.7s ease-in-out forwards;
}

@keyframes splash-sweep {
  from { width: 6%; }
  to { width: 100%; }
}

.splash-note { font-size: 11px; color: var(--ink-faint); padding-left: 3px; }

.splash-art {
  width: 42%;
  max-width: 420px;
  flex: none;
  display: grid;
  place-items: center;
  background: var(--surface-2);
  border-left: 1px solid var(--line);
}

/* ---------- info ---------- */

.info-head { margin-bottom: 20px; }
.info-head .recent-path { margin-top: 5px; }

.info-alert {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  padding: 11px 14px;
  margin-bottom: 18px;
  max-width: 740px;
  background: var(--danger-bg);
  border: 1px solid rgba(217, 99, 106, 0.4);
  border-radius: 6px;
  font-size: 12px;
  line-height: 1.5;
}

.stat-row {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(112px, 1fr));
  gap: 10px;
  max-width: 740px;
  margin-bottom: 20px;
}

.stat-tile {
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 12px 14px;
}

.stat-value {
  font-size: 17px;
  font-weight: 600;
  letter-spacing: -0.3px;
  color: var(--ink);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.stat-label {
  font-size: 9.5px;
  letter-spacing: 1px;
  text-transform: uppercase;
  color: var(--ink-faint);
  margin-top: 4px;
}

.info-chart {
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  max-width: 740px;
  margin-bottom: 20px;
  overflow: hidden;
}

.info-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 14px;
  max-width: 740px;
}

.info-card {
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 14px 16px 6px;
}

.info-card h3 {
  font-size: 11px;
  letter-spacing: 0.9px;
  text-transform: uppercase;
  color: var(--accent);
  margin: 0 0 10px;
  font-weight: 600;
}

.info-line {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  padding: 7px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.info-card .info-line:last-child { border-bottom: 0; }
.info-line .k { color: var(--ink-soft); }
.info-line .v { color: var(--ink); text-align: right; }

/* ---------- home ---------- */

.home-section {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  width: 100%;
  margin-top: 26px;
}

.home-section .bs-sub { margin-bottom: 10px; }

.bs-link {
  border: 0;
  background: transparent;
  color: var(--accent);
  font-size: 12px;
  cursor: default;
  padding: 2px 4px;
  border-radius: 4px;
}

.bs-link:hover { background: var(--accent-dim); color: var(--accent-bright); }

.home-empty {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 22px;
  border: 1px dashed var(--line);
  border-radius: 8px;
  color: var(--ink-soft);
  font-size: 12px;
  max-width: 520px;
}

.home-empty > :first-child { color: var(--accent); }

/* template gallery */
.tpl-grid {
  display: grid;
  /* auto-fill so the cards spread across whatever width there is, rather than
     huddling on the left of a wide window. */
  grid-template-columns: repeat(auto-fill, minmax(212px, 1fr));
  gap: 16px;
  width: 100%;
}

.tpl-card {
  border: 1px solid var(--line);
  border-radius: 5px;
  background: var(--surface);
  cursor: default;
  padding: 0;
  text-align: left;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  transition: border-color 0.15s, transform 0.15s;
}

.tpl-card:hover { border-color: var(--accent-line); transform: translateY(-2px); }

.tpl-thumb {
  height: 128px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
  position: relative;
  overflow: hidden;
}

.tpl-thumb svg { display: block; width: 100%; height: 100%; }

.tpl-meta { padding: 9px 11px 11px; }
.tpl-name { font-size: 12.5px; font-weight: 600; color: var(--ink); }
.tpl-desc { font-size: 11px; color: var(--ink-soft); margin-top: 4px; line-height: 1.4; }
.tpl-count { font-size: 10.5px; color: var(--accent); margin-top: 6px; letter-spacing: 0.2px; }

/* recent list */
.recent-list { max-width: 680px; }

.recent-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 9px 10px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--ink);
  width: 100%;
  text-align: left;
  cursor: default;
}

.recent-row:hover { background: var(--hover); }
.recent-row .glyph { color: var(--accent); display: grid; place-items: center; }
.recent-name { font-size: 13px; }
.recent-path { font-size: 11px; color: var(--ink-faint); }

.bs-field { display: flex; align-items: center; gap: 10px; margin: 10px 0; max-width: 680px; }
.bs-field label { width: 120px; color: var(--ink-soft); flex: none; }

.bs-input, .dlg input, .dlg select, .dlg textarea {
  border: 1px solid var(--line);
  border-radius: 4px;
  padding: 6px 8px;
  /* Matches .dd.lg, so a dropdown beside a text box lines up. A textarea
     overrides this with its own height, since it is meant to be tall. */
  height: 30px;
  box-sizing: border-box;
  /* WebKit paints a native select with the platform look and ignores the
     background above unless the appearance is cleared first. */
  appearance: none;
  font: inherit;
  color: var(--ink);
  background: var(--surface-3);
  flex: 1;
  -webkit-user-select: text;
  user-select: text;
}

.dlg textarea { height: auto; }
.bs-input::placeholder, .dlg input::placeholder { color: var(--ink-faint); }
.bs-input:focus, .dlg input:focus, .dlg select:focus, .dlg textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-dim);
}

.btn {
  border: 1px solid var(--line);
  background: var(--surface-3);
  color: var(--ink);
  border-radius: 4px;
  padding: 6px 16px;
  cursor: default;
  font: inherit;
}

.btn:hover { background: var(--surface-4); border-color: var(--accent-line); }

.btn.primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
.btn.primary:hover { background: var(--accent-bright); }

.btn.danger { color: var(--danger); border-color: rgba(217, 99, 106, 0.4); }
.btn.danger:hover { background: var(--danger-bg); }

.info-grid {
  display: grid;
  grid-template-columns: 180px 1fr;
  gap: 9px 18px;
  max-width: 680px;
  font-size: 12px;
}

.info-grid .k { color: var(--ink-soft); }

.ok-banner {
  background: var(--accent-dim);
  border: 1px solid var(--accent-line);
  color: var(--accent-bright);
  border-radius: 4px;
  padding: 8px 12px;
  font-size: 12px;
  max-width: 680px;
  margin: 12px 0;
}

/* ---------- options ---------- */

.opt-layout { display: flex; gap: 26px; align-items: flex-start; max-width: 940px; }

.opt-nav {
  width: 200px;
  flex: none;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line);
  border-radius: 6px;
  overflow: hidden;
}

.opt-nav-item {
  text-align: left;
  padding: 9px 14px;
  border: 0;
  border-left: 2px solid transparent;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  font-size: 12.5px;
}

.opt-nav-item:hover { background: var(--hover); color: var(--ink); }
.opt-nav-item.active { background: var(--accent-dim); border-left-color: var(--accent); color: var(--accent-bright); }

.opt-body { flex: 1; min-width: 0; }

.opt-head {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--ink);
  margin: 0 0 12px;
  padding-bottom: 7px;
  border-bottom: 1px solid var(--line);
}

.opt-head + .opt-head, .opt-row + .opt-head, .rcheck + .opt-head, .opt-static + .opt-head { margin-top: 26px; }

.opt-row { display: flex; align-items: flex-start; gap: 16px; margin-bottom: 12px; }

.opt-label { width: 210px; flex: none; display: flex; flex-direction: column; gap: 2px; padding-top: 6px; }
.opt-label span { color: var(--ink); font-size: 12px; }
.opt-label .opt-hint { color: var(--ink-faint); font-size: 10.5px; line-height: 1.35; }

.opt-control { flex: 1; min-width: 0; display: flex; }
.opt-control > * { flex: 1; min-width: 0; }

.opt-static { border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }

/* ---------- keyboard shortcuts ---------- */

.opt-note {
  border: 1px solid var(--accent-line);
  background: var(--accent-dim);
  border-radius: 6px;
  padding: 8px 12px;
  font-size: 11.5px;
  color: var(--ink);
  margin-bottom: 14px;
}

/* Each group is a heading and its list. The gap between groups has to be
   clearly larger than the gap between rows, or the headings read as belonging
   to the list above them rather than the one below. */
.key-group + .key-group { margin-top: 26px; }
.key-group .opt-head { margin: 0 0 9px; }

.key-list { border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }

.key-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 12px;
  border-bottom: 1px solid var(--line-soft);
}
.key-row:last-child { border-bottom: none; }
.key-row:hover { background: var(--hover); }
.key-row.recording { background: var(--accent-dim); }

.key-name { flex: 1; min-width: 0; font-size: 12px; color: var(--ink); }

/* Marks a binding that has been moved off its default, so Reset means something
   visible rather than being a button that might do nothing. */
.key-changed {
  font-size: 9.5px;
  letter-spacing: 0.5px;
  text-transform: uppercase;
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  padding: 1px 7px;
  flex: none;
}

/* The binding is the control: click it, then press the keys. */
.key-bind {
  min-width: 190px;
  text-align: left;
  padding: 4px 9px;
  border: 1px solid var(--line);
  border-radius: 5px;
  background: var(--surface-3);
  cursor: default;
  font-size: 11.5px;
}
.key-bind:hover { border-color: var(--accent-line); }
.key-row.recording .key-bind { border-color: var(--accent); }

.key-combo { color: var(--ink); font-family: var(--mono); font-size: 11px; }
.key-none { color: var(--ink-faint); font-style: italic; }
.key-listening { color: var(--accent-bright); }

.key-clear {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  flex: none;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--ink-faint);
  cursor: default;
}
.key-clear:hover { background: var(--hover); color: var(--ink); }

.opt-static-row {
  display: flex;
  justify-content: space-between;
  gap: 14px;
  padding: 8px 12px;
  font-size: 12px;
  border-bottom: 1px solid var(--line-soft);
}

.opt-static-row:last-child { border-bottom: 0; }
.opt-static-row .v { color: var(--ink-soft); }

/* ---------- fix issue ---------- */

.error-banner .grow { flex: 1; }

.fix-problem {
  display: flex;
  gap: 12px;
  align-items: flex-start;
  padding: 12px 14px;
  background: var(--danger-bg);
  border: 1px solid rgba(217, 99, 106, 0.4);
  border-radius: 6px;
  line-height: 1.55;
  font-size: 12.5px;
}

.fix-icon { color: var(--danger); flex: none; display: grid; place-items: center; }

.fix-head {
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-soft);
  margin: 20px 0 8px;
  letter-spacing: 0.2px;
}

.fix-action { font-size: 12.5px; line-height: 1.55; color: var(--ink); }

.fix-changes { border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }

.fix-change {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.fix-change:last-child { border-bottom: 0; }
.fix-bullet { color: var(--danger); display: grid; place-items: center; flex: none; }

/* ---------- field picker ---------- */

.field-list {
  border: 1px solid var(--line);
  border-radius: 5px;
  max-height: 380px;
  overflow-y: auto;
}

.field-group {
  position: sticky;
  top: 0;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
  padding: 7px 12px;
  font-size: 10px;
  letter-spacing: 0.9px;
  text-transform: uppercase;
  color: var(--accent);
  font-weight: 600;
  z-index: 1;
}

.field-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--line-soft);
  cursor: default;
}

.field-row:hover:not(.shown) { background: var(--accent-dim); }
.field-row.shown { opacity: 0.5; }
.field-text { flex: 1; min-width: 0; }
.field-name { font-size: 12.5px; color: var(--ink); }
.field-desc { font-size: 11px; color: var(--ink-faint); margin-top: 2px; }

.field-badge {
  flex: none;
  font-size: 10px;
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  padding: 2px 9px;
}

/* ---------- predecessor picker ---------- */

.pred-list {
  border: 1px solid var(--line);
  border-radius: 5px;
  max-height: 320px;
  overflow-y: auto;
  margin: 6px 0 4px;
}

.pred-row { border-bottom: 1px solid var(--line-soft); }
.pred-row:last-child { border-bottom: 0; }
.pred-row.on { background: var(--accent-dim); }

.pred-pick {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 28px;
  padding-right: 10px;
  cursor: default;
}

.pred-row:not(.on) .pred-pick:hover { background: var(--hover); }

.pred-id {
  min-width: 22px;
  text-align: right;
  color: var(--ink-faint);
  font-size: 11px;
  flex: none;
}

.pred-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.pred-row.summary .pred-name { font-weight: 600; }
.pred-row.on .pred-name { color: var(--accent-bright); }

.pred-detail {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 0 10px 8px 46px;
}

.pred-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 10px;
}

/* ---------- colour rows ---------- */

.colour-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 7px 0;
  border-bottom: 1px solid var(--line-soft);
}

.colour-swatch {
  width: 30px;
  height: 14px;
  border-radius: 3px;
  border: 1px solid var(--line);
  flex: none;
}

.colour-name { flex: 1; font-size: 12px; }
.colour-hex { font-family: var(--mono); font-size: 11px; color: var(--ink-faint); width: 74px; text-align: right; }

.colour-picker {
  width: 44px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: transparent;
  cursor: default;
  flex: none;
}

.colour-picker::-webkit-color-swatch-wrapper { padding: 2px; }
.colour-picker::-webkit-color-swatch { border: 0; border-radius: 2px; }

/* ---------- quick access editor ---------- */

.qat-list { border: 1px solid var(--line); border-radius: 5px; overflow: hidden; }

.qat-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 34px;
  padding: 0 6px 0 11px;
  border-bottom: 1px solid var(--line-soft);
}

.qat-item:last-child { border-bottom: 0; }
.qat-item:hover { background: var(--hover); }
.qat-item .qat-glyph { display: grid; place-items: center; width: 18px; flex: none; color: var(--accent); }
.qat-item .qat-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }

/* ---------- about ---------- */

.about-wrap {
  display: flex;
  justify-content: center;
  padding: 8px 0 32px;
}

.about-card {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 14px;
  padding: 36px 40px 32px;
  width: 100%;
  max-width: 620px;
}

.about-brand {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  text-align: center;
  padding-bottom: 26px;
  border-bottom: 1px solid var(--line);
}

.about-logo { color: var(--ink); }
.about-logo svg { width: 100%; height: 100%; display: block; }

.about-name {
  font-size: 27px;
  font-weight: 600;
  letter-spacing: -0.4px;
  color: var(--ink);
}

.about-pills { display: flex; gap: 8px; flex-wrap: wrap; justify-content: center; }

.pill {
  border: 1px solid var(--line);
  border-radius: 999px;
  padding: 3px 12px;
  font-size: 11px;
  color: var(--ink-soft);
}

.pill.accent { border-color: var(--accent-line); color: var(--accent-bright); background: var(--accent-dim); }

.about-rows { margin-top: 22px; }

.about-row {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 9px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12.5px;
}

.about-row .k { color: var(--ink-soft); }
.about-row .v { color: var(--ink); text-align: right; }

.about-attr-btn {
  margin: 24px auto 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  background: transparent;
  border: 1px solid var(--accent-line);
  border-radius: 9px;
  color: var(--accent-bright);
  font-size: 12.5px;
  font-weight: 600;
  cursor: default;
}

.about-attr-btn:hover { background: var(--accent-dim); border-color: var(--accent); }

.attr-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  padding: 6px 0;
  border-bottom: 1px solid var(--line-soft);
  font-size: 12px;
}

.attr-name { flex: none; width: 130px; color: var(--ink); }
.attr-license { flex: none; width: 130px; color: var(--ink-soft); font-size: 11px; }
.attr-url { flex: 1; color: var(--ink-faint); font-size: 11px; overflow: hidden; text-overflow: ellipsis; }

/* print preview */
/* The document on the left, where it is going on the right. The preview takes
   the room because it is the thing being judged. */
.print-layout { display: flex; gap: 22px; align-items: stretch; min-height: 0; }
.print-preview { flex: 1 1 auto; min-width: 0; display: flex; }
.print-settings {
  width: 300px;
  flex: none;
  overflow-y: auto;
  max-height: calc(100vh - 210px);
  padding-right: 4px;
}

/* Shown when the engine has no PDF viewer of its own to hand. */
.print-fallback {
  display: grid;
  place-items: center;
  height: 100%;
  padding: 24px;
  text-align: center;
  color: var(--ink-soft);
  font-size: 12px;
}

/* ---------- print queues ---------- */

.queue-list { display: flex; flex-direction: column; gap: 4px; margin-bottom: 12px; }

.queue {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-3);
  cursor: default;
  text-align: left;
}
.queue:hover { border-color: var(--accent-line); }
.queue.on { border-color: var(--accent); background: var(--accent-dim); }
.queue .glyph { display: grid; place-items: center; flex: none; color: var(--ink-faint); }
.queue.on .glyph { color: var(--accent); }

.queue-text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.queue-name { display: flex; align-items: center; gap: 7px; font-size: 12px; color: var(--ink); }
.queue-status { font-size: 10.5px; color: var(--ink-faint); }

.queue-default {
  font-size: 9px;
  letter-spacing: 0.5px;
  text-transform: uppercase;
  color: var(--accent);
  border: 1px solid var(--accent-line);
  border-radius: 999px;
  padding: 0 6px;
}

/* Buttons on the Print page carry a glyph, so they lay out as a row rather
   than as a bare label. */
.print-settings .btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.print-settings .btn .glyph { display: grid; place-items: center; }
.print-go { width: 100%; padding: 10px 14px; font-size: 13px; }

/* What the printed document will look like, stated rather than guessed at. */
.print-fact {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 0;
  border-bottom: 1px solid var(--line);
  font-size: 11.5px;
}
.print-fact:last-child { border-bottom: none; }
.pf-label { color: var(--ink-faint); }
.pf-value { color: var(--ink); text-align: right; }

.print-frame {
  flex: 1 1 auto;
  width: 100%;
  min-width: 0;
  height: calc(100vh - 210px);
  border: 1px solid var(--line);
  border-radius: 6px;
  background: #f2f5f5;
  box-shadow: var(--shadow);
}

/* ---------- timeline band ---------- */

.timeline {
  background: var(--surface);
  border-bottom: 1px solid var(--line);
  flex: none;
  position: relative;
  overflow: hidden;
  padding: 4px 10px;
}

/* The contextual banner with nothing to announce: still there, still the same
   height, just not saying anything. */
.tools-banner.empty { visibility: hidden; }

.timeline-caption {
  position: absolute;
  left: 9px;
  top: 3px;
  font-size: 9px;
  color: var(--ink-faint);
  letter-spacing: 0.7px;
  text-transform: uppercase;
}

/* A transparent sheet over everything, so a drag keeps receiving events
   however fast the pointer moves or wherever it wanders. */
.drag-shield {
  position: fixed;
  inset: 0;
  z-index: 150;
  background: transparent;
}

.drag-shield.col-resize { cursor: col-resize; }
.drag-shield.grabbing { cursor: grabbing; }

/* ---------- icon buttons ---------- */

.iconbtn {
  width: 22px;
  height: 22px;
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 4px;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  line-height: 1;
  font-size: 12px;
}

.iconbtn svg { display: block; }
.iconbtn:hover:not(:disabled) { background: var(--hover); border-color: var(--line); color: var(--accent-bright); }
.iconbtn:active:not(:disabled) { background: var(--pressed); }
.iconbtn:disabled { opacity: 0.3; }
.iconbtn.danger:hover:not(:disabled) { background: var(--danger-bg); border-color: var(--danger); color: var(--danger); }

/* a row of icon buttons that reads as one control */
.btn-group { display: inline-flex; align-items: center; gap: 2px; }

/* ---------- internal window panes ---------- */

.panes {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  padding: 6px 6px 0;
  gap: 0;
}

.pane-bar { display: flex; flex: none; gap: 6px; min-width: 0; }

.pane-tab {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 25px;
  padding: 0 10px;
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-bottom: 0;
  border-radius: 6px 6px 0 0;
  font-size: 11px;
  color: var(--ink-soft);
  flex: none;
  min-width: 0;
}

.pane-tab.grow { flex: 1; }
.pane-tab.active { color: var(--accent-bright); background: var(--surface-3); }
/* The name takes the slack, so the button lands on the tab's right edge
   whether or not there is a subtitle to sit beside it. */
.pane-tab .pane-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pane-tab .pane-sub {
  color: var(--ink-faint);
  font-size: 10px;
  flex: none;
  padding-right: 4px;
  white-space: nowrap;
}
.pane-tab .pane-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); flex: none; opacity: 0.75; }

/* The tab's own button sits flush with the right edge of the tab. */
.pane-tab .iconbtn {
  width: 20px;
  height: 19px;
  flex: none;
  margin-left: 4px;
  margin-right: -2px;
}
.pane-tab .iconbtn:not(:hover) { color: var(--ink-faint); }

.pane-frame {
  flex: 1 1 auto;
  min-height: 0;
  min-width: 0;
  display: flex;
  border: 1px solid var(--line);
  border-radius: 0 6px 0 0;
  background: var(--surface);
  overflow: hidden;
}

/* ---------- workspace ---------- */

.workspace { flex: 1; display: flex; min-height: 0; background: var(--bg); }

.viewbar {
  width: 22px;
  background: var(--surface-2);
  color: var(--ink-soft);
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border-right: 1px solid var(--line);
}

.viewbar span {
  writing-mode: vertical-rl;
  transform: rotate(180deg);
  font-size: 10.5px;
  letter-spacing: 0.8px;
  white-space: nowrap;
}

.split {
  flex: 1 1 auto;
  display: flex;
  align-items: stretch;
  min-width: 0;
  min-height: 0;
  /* Each pane scrolls itself, so this only frames them. */
  overflow: hidden;
  background: var(--surface);
}

.split.hide-table .pane-left { display: none; }
.split.hide-chart .chart-pane { display: none; }

.pane-left {
  display: flex;
  align-items: stretch;
  flex: none;
  min-width: 0;
  background: var(--surface);
}

.splitter {
  position: relative;
  width: 5px;
  background: var(--line);
  cursor: col-resize;
  flex: none;
  align-self: stretch;
  min-height: 100%;
}

/* A wider invisible grip, so the splitter is easy to catch. */
.splitter::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: -4px;
  right: -4px;
}

.splitter:hover { background: var(--accent); }

/* While resizing, nothing under the pointer may react. */
.split.resizing { cursor: col-resize; }
.split.resizing .grid,
.split.resizing .chart-svg { pointer-events: none; }
.split.resizing .splitter { background: var(--accent); }

/* While a row is being dragged the cursor says so throughout. */
.split.row-dragging { cursor: grabbing; }
.split.row-dragging .chart-svg { pointer-events: none; }

/* ---------- task grid ---------- */

/* Stands in for the rows outside the viewport, so the pane scrolls its full
   height without those rows existing. It must not take any styling of its own,
   or the striping and borders would show a seam where the drawn rows end. */
.row-spacer { border: none; background: none; }
.row-spacer td { padding: 0; border: none; }

/* Wide tables scroll sideways on their own, without moving the chart. */
.grid-pane {
  flex: none;
  background: var(--surface);
  overflow: auto;
  min-width: 0;
}

/* The table pane is sized to its columns exactly. A vertical scrollbar here
   would eat into that width and force a pointless horizontal scrollbar, so it
   is hidden: vertical scrolling is driven from the chart's bar and the wheel,
   and the two panes are kept in step anyway. */
.grid-pane::-webkit-scrollbar:vertical { width: 0; }

.grid { border-collapse: collapse; table-layout: fixed; font-size: 12px; width: 100%; }

.grid th {
  position: sticky;
  top: 0;
  z-index: 3;
  overflow: visible;
  background: var(--grid-header);
  border: 1px solid var(--grid-line);
  border-top: 0;
  height: 38px;
  font-weight: 500;
  font-size: 11px;
  color: var(--ink-soft);
  padding: 2px 6px;
  text-align: left;
  white-space: nowrap;
}

.grid th.num { text-align: center; }

.grid td {
  border: 1px solid var(--grid-line);
  height: 22px;
  padding: 0 6px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--ink);
}

/* The gridline choices, as classes on the table so one toggle repaints
   without every cell carrying its own style. */
.grid.no-rows td { border-top-color: transparent; border-bottom-color: transparent; }
.grid.no-columns td { border-left-color: transparent; border-right-color: transparent; }

.grid tr.row:hover td { background: var(--hover); }
.grid tr.row.selected td { background: var(--selection); }
.grid tr.row.summary td { font-weight: 600; color: var(--ink); }
.grid tr.row.inactive td { color: var(--ink-faint); text-decoration: line-through; }
/* Criticality reads as a quiet marker on the row number, not red text. */
.grid tr.row.critical td.rownum {
  box-shadow: inset 2px 0 0 var(--bar-critical-edge);
}

.grid tr.row.dragging td { opacity: 0.45; }
.grid tr.row.drop-above td { box-shadow: inset 0 2px 0 var(--accent); }
.grid tr.row.drop-below td { box-shadow: inset 0 -2px 0 var(--accent); }
.grid tr.row.drop-into td { box-shadow: inset 0 0 0 1px var(--accent); }

/* A grouping band: a heading over the rows beneath it, spanning every column.
   It is not a task, so none of the row states above can reach it, and it takes
   the header's surface to read as a divider rather than as an empty row. */
.grid tr.row.band td {
  background: var(--grid-header);
  border-left: 0;
  border-right: 0;
  color: var(--ink-soft);
  font-size: 11px;
  cursor: default;
}

.grid tr.row.band:hover td { background: var(--grid-header); }
.grid tr.row.band .band-label { font-weight: 600; color: var(--ink); }
.grid tr.row.band .band-totals { margin-left: 10px; }

.grid td.rownum {
  background: var(--grid-header);
  text-align: center;
  color: var(--ink-faint);
  font-size: 11px;
  position: sticky;
  left: 0;
  z-index: 2;
  cursor: grab;
  user-select: none;
}

.grid td.rownum:hover { color: var(--accent-bright); background: var(--surface-4); }
.grid td.rownum:active { cursor: grabbing; }

.grid tr.row.selected td.rownum { background: var(--accent-dim); color: var(--accent-bright); }

.grid td.c-num { text-align: right; }
.grid td.c-mid { text-align: center; }

/* Symbols in a cell are centred on the row, not sat on the text baseline,
   which is what made them look a pixel or two high. */
.grid td.c-mid > span,
.grid td.rownum {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  line-height: 1;
}

.grid .ind-critical { color: var(--warn); font-size: 12px; line-height: 1; }
.grid .mode-glyph { display: grid; place-items: center; line-height: 1; }

/* The grip that resizes a column, straddling its right-hand border. */
/* Sits centred on the 1px divider between this column and the next, so the
   line the planner is aiming at is the line they grab. The cell's padding box
   ends at right: 0, the border occupies the next pixel, so a 9px grab area
   pulled 5px past that edge is centred on it. */
.col-grip {
  position: absolute;
  top: 0;
  right: -5px;
  width: 9px;
  height: 100%;
  cursor: col-resize;
  z-index: 4;
}

.col-grip::after {
  content: "";
  position: absolute;
  top: 5px;
  bottom: 5px;
  left: 4px;
  width: 1px;
  background: transparent;
}

.col-grip:hover::after { background: var(--accent); }

/* The typed alternative under the predecessor picker's list. */
/* The pickers appear both in a floating popup and inside a dialog tab. Inside
   a dialog the popup's own header is redundant, and the list wants to use the
   height the tab already has. */
.dlg .picker .ctxheader { display: none; }
.dlg .picker .pred-list { max-height: 300px; }

.picker-cell { display: flex; align-items: center; width: 100%; height: 100%; }
.picker-cell .cell-input { flex: 1; min-width: 0; }
/* The cell looks like a plain text box otherwise, and nothing says a list is
   behind it. */
.picker-caret {
  flex: none;
  width: 16px;
  height: 100%;
  border: 0;
  padding: 0;
  cursor: pointer;
  font-size: 9px;
  color: var(--ink-soft);
  background: transparent;
}
.picker-caret:hover { color: var(--accent-bright); }

/* ---------- critical path chart ---------- */

.cp-chart {
  border: 1px solid var(--grid-line);
  border-radius: 4px;
  overflow-x: auto;
  margin-bottom: 10px;
  background: var(--surface);
}
.cp-chart svg { display: block; min-width: 720px; }
.cp-name  { font-size: 11px; fill: var(--ink); }
.cp-span  { font-size: 10px; fill: var(--ink-soft); }
.cp-tick  { font-size: 10px; fill: var(--ink-soft); }
.cp-joint { font-size: 9px;  fill: var(--danger); }

.cp-legend {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--ink-soft);
  font-size: 11px;
  margin-bottom: 18px;
  line-height: 1.5;
}
.cp-swatch {
  flex: none;
  width: 20px;
  height: 10px;
  border-radius: 2px;
  background: var(--danger);
}

.pred-type { display: flex; align-items: center; gap: 8px; padding: 8px 10px 0; }
.pred-type label { color: var(--ink-soft); font-size: 11px; flex: none; }
.pred-type .bs-input { flex: 1; min-width: 0; }

/* Sticky already makes the header a containing block, which is what lets the
   resize grip position itself against the cell's own edge. */
.grid th { position: sticky; }
.grid th .th-inner {
  position: relative;
  display: flex;
  align-items: center;
  height: 100%;
}

.grid th.num .th-inner,
.grid th.c-mid .th-inner { justify-content: center; }

.grid th .th-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--ink-soft);
}

.cell-name { display: flex; align-items: center; gap: 3px; }

.twisty {
  width: 12px;
  flex: none;
  text-align: center;
  font-size: 8px;
  color: var(--ink-faint);
  cursor: default;
}

.cell-input {
  width: 100%;
  height: 20px;
  border: 1px solid var(--accent);
  outline: none;
  font: inherit;
  padding: 0 4px;
  background: var(--surface-4);
  color: var(--ink);
  -webkit-user-select: text;
  user-select: text;
}

.add-row td { color: var(--ink-faint); font-style: italic; }

/* Manual is the state worth noticing: it means the scheduler is not moving
   this row, which is why a date can look wrong. Auto is the norm, so it stays
   quiet. */
.mode-glyph { display: grid; place-items: center; color: var(--ink-faint); }
.mode-glyph.manual { color: var(--contextual); }
.mode-glyph.auto { color: var(--ink-faint); }

/* ---------- gantt chart ---------- */

.chart-pane {
  flex: 1 1 auto;
  position: relative;
  background: var(--surface);
  overflow: auto;
  min-width: 0;
}

/* Holds the chart's own width so the pane above can scroll to it. */
.chart-canvas { display: block; }
.chart-head { position: sticky; top: 0; z-index: 4; background: var(--grid-header); }
.chart-svg { display: block; }

.tl-major, .tl-minor { font-size: 10px; fill: var(--ink-soft); }
.tl-major { fill: var(--ink); }
.tl-minor.weekend { fill: var(--ink-faint); }

.bar-label { font-size: 10px; fill: var(--ink-soft); dominant-baseline: middle; }

/* Annotation shapes. The group is inert as a whole and each shape opts back
   in, so an unfilled outline is still clickable while the empty space between
   two shapes lets the pointer through to the bars underneath. */
.drawings { pointer-events: none; }
.draw-text { dominant-baseline: middle; user-select: none; }

/* Timeline band labels. One beside its bar reads as ordinary text; one within
   a bar sits on the bar's own colour, so it takes the dark ink instead. */
.band-label { font-size: 10px; fill: var(--ink-soft); }
.band-label.in { fill: var(--on-accent); font-weight: 600; }

/* ---------- reports ---------- */

.reports-pane {
  flex: 1 1 auto;
  overflow: auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  background: var(--surface);
}
.report-row { display: flex; gap: 14px; align-items: stretch; flex-wrap: wrap; }

.report-card {
  flex: 1 1 380px;
  min-width: 320px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-3);
  padding: 12px 14px 10px;
}
.report-head { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
.report-title { font-size: 13px; font-weight: 650; color: var(--ink); }
.report-note { font-size: 11px; color: var(--ink-faint); }
.report-chart { display: block; margin: 10px 0 6px; overflow: visible; }
.axis-label { font-size: 9.5px; fill: var(--ink-faint); }
.axis-title { font-size: 9.5px; fill: var(--ink-soft); letter-spacing: 0.4px; }

.report-legend { display: flex; gap: 14px; font-size: 10.5px; color: var(--ink-soft); }
.report-legend span { display: inline-flex; align-items: center; gap: 6px; }
.report-legend .sw { width: 14px; height: 3px; border-radius: 2px; display: inline-block; }
.report-legend .sw.ideal { background: var(--ink-faint); }
.report-legend .sw.actual { background: var(--accent-bright); }
.report-legend .sw.scope { background: var(--ink-faint); }
.report-legend .sw.done { background: var(--bar-progress); }
.report-legend .sw.planned { background: var(--bar); }

/* Velocity: planned behind, completed in front, so the gap between them is
   the thing you read rather than two bars to compare by eye. */
.velocity {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  height: 170px;
  margin: 10px 0 6px;
  padding-bottom: 16px;
  padding-left: 62px;
  position: relative;
}

/* The scale the bars are read against. Bars with no axis show which iteration
   was busiest but not by how much. */
.vel-axis { position: absolute; left: 0; top: 0; bottom: 16px; width: 56px; }
.vel-tick {
  position: absolute;
  right: 6px;
  transform: translateY(50%);
  font-size: 9.5px;
  color: var(--ink-faint);
  white-space: nowrap;
}
.vel-grid {
  position: absolute;
  left: 62px;
  right: 0;
  border-top: 1px solid var(--grid-line);
}
.vel-col { flex: 1 1 0; min-width: 10px; height: 100%; position: relative; }
.vel-stack { position: relative; height: 100%; }
.vel-planned, .vel-done {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  border-radius: 3px 3px 0 0;
}
.vel-planned { background: var(--bar); opacity: 0.35; }
.vel-done { background: var(--bar-progress); }
.vel-label {
  position: absolute;
  bottom: -15px;
  left: 0;
  right: 0;
  text-align: center;
  font-size: 9.5px;
  color: var(--ink-faint);
}

/* The path reads as a chain: each step, then the link that carries it onward.
   A flat list would say which tasks are critical but not in what order, which
   is the part that actually matters. */
.crit-list { max-height: 170px; overflow-y: auto; margin-top: 8px; }
.crit-step { display: flex; flex-direction: column; }
.crit-joint {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 1px 0 1px 30px;
  font-size: 9.5px;
  color: var(--ink-faint);
}
.crit-arrow { color: var(--accent); }
.crit-dur { color: var(--ink-faint); font-size: 10.5px; white-space: nowrap; }
.crit-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding: 4px 0;
  font-size: 11.5px;
}
.crit-id { color: var(--ink-faint); min-width: 26px; }
.crit-name { flex: 1; min-width: 0; color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.crit-dates { color: var(--ink-soft); font-size: 10.5px; white-space: nowrap; }

/* ---------- report pages ---------- */

.rep-head { margin-bottom: 16px; }
.rep-title { font-size: 22px; font-weight: 650; color: var(--ink); margin: 0 0 6px; letter-spacing: -0.3px; }
.rep-sub { margin: 0; font-size: 12px; color: var(--ink-soft); line-height: 1.6; max-width: 760px; }

/* A chart says a shape but not a number, so the figures come first. */
.rep-figures { display: flex; gap: 10px; flex-wrap: wrap; margin-bottom: 16px; }
.rep-figure {
  flex: 1 1 150px;
  background: var(--surface-2);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.rep-value { font-size: 19px; font-weight: 650; color: var(--ink); letter-spacing: -0.3px; }
.rep-label { font-size: 9.5px; letter-spacing: 0.9px; text-transform: uppercase; color: var(--ink-faint); }

.rep-chart-box {
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--surface-3);
  padding: 14px;
  margin-bottom: 18px;
}
.velocity.tall { height: 260px; }

.rep-section {
  font-size: 13px;
  font-weight: 650;
  color: var(--ink);
  margin: 0 0 8px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--line);
}
.rep-table { width: 100%; border-collapse: collapse; font-size: 11.5px; }
.rep-table thead th {
  text-align: left;
  font-size: 9.5px;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--ink-faint);
  font-weight: 600;
  padding: 6px 8px;
  border-bottom: 1px solid var(--line);
}
.rep-table td { padding: 6px 8px; border-bottom: 1px solid var(--line-soft); color: var(--ink); }
.rep-table tbody tr:hover { background: var(--hover); }
.rep-table .n { text-align: right; }
.rep-table .muted { color: var(--ink-soft); }

/* ---------- colour commands ---------- */

/* A row command that also carries a swatch: glyph, label, then the colour. */
.swatch-btn { display: flex; align-items: center; gap: 6px; }
.swatch-btn .colour-bar { width: 14px; height: 10px; border-radius: 2px; margin-left: 2px; }

.colour-btn-wrap { position: relative; display: inline-flex; }

/* Glyph above, the colour it will apply below, the way Office does it. */
.colour-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1px;
  padding: 3px 4px 2px;
}
.colour-bar {
  display: block;
  width: 15px;
  height: 3px;
  border-radius: 1px;
  border: 1px solid var(--line);
}

.colour-pop {
  position: absolute;
  top: 100%;
  left: 0;
  z-index: 60;
  margin-top: 4px;
  padding: 8px;
  border: 1px solid var(--line);
  border-radius: 7px;
  background: var(--surface-4);
  box-shadow: var(--shadow);
}
.colour-grid {
  display: grid;
  grid-template-columns: repeat(4, 20px);
  gap: 5px;
  margin-bottom: 7px;
}
.colour-chip {
  width: 20px;
  height: 20px;
  border-radius: 4px;
  border: 1px solid var(--line);
  cursor: default;
  padding: 0;
}
.colour-chip:hover { outline: 2px solid var(--accent); outline-offset: 1px; }
.colour-clear {
  width: 100%;
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  font-size: 10.5px;
  cursor: default;
  padding: 3px;
  border-radius: 4px;
  white-space: nowrap;
}
.colour-clear:hover { background: var(--hover); color: var(--ink); }

/* ---------- external dependencies ---------- */

.ext-add { display: flex; gap: 8px; margin-bottom: 14px; }
.ext-add .bs-input { flex: 1; min-width: 0; }

.ext-list { display: flex; flex-direction: column; gap: 4px; }
.ext-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: var(--surface-3);
}
.ext-main { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
.ext-ref { font-family: var(--mono); font-size: 11.5px; color: var(--accent); }
.ext-label { font-size: 12px; color: var(--ink); }
.ext-date { max-width: 132px; font-size: 11px; padding: 3px 8px; }
.ext-users { font-size: 10.5px; color: var(--ink-faint); }
.ext-acts { display: flex; align-items: center; gap: 6px; flex: none; }

/* ---------- custom fields ---------- */

.cf-pick { display: flex; gap: 14px; margin-bottom: 6px; }
.cf-pick .bs-field { flex: 1; }

.cf-indicator {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border: 1px solid var(--line);
  border-radius: 6px;
  margin-bottom: 5px;
  background: var(--surface-3);
}
.cf-glyph { font-size: 14px; color: var(--accent); width: 18px; text-align: center; }
.cf-rule { font-size: 11.5px; color: var(--ink); }
.cf-meaning { flex: 1; font-size: 10.5px; color: var(--ink-faint); }

/* The fields this plan already uses, as a way back to one. */
.cf-inuse { display: flex; flex-wrap: wrap; gap: 6px; }
.cf-chip {
  display: flex;
  align-items: baseline;
  gap: 7px;
  padding: 4px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: var(--surface-3);
  cursor: default;
}
.cf-chip:hover { border-color: var(--accent-line); }
.cf-chip-name { font-size: 11.5px; color: var(--ink); }
.cf-chip-slot { font-size: 9.5px; color: var(--ink-faint); font-family: var(--mono); }

/* ---------- dictionaries ---------- */

.dict-list { margin-top: 8px; }
.dict-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 7px 2px;
  border-bottom: 1px solid var(--line-soft);
}
.dict-row:last-child { border-bottom: none; }
.dict-name { flex: 1; min-width: 0; font-size: 12px; color: var(--ink); }
.dict-code { font-family: var(--mono); font-size: 10.5px; color: var(--ink-faint); }
.dict-size { font-size: 10.5px; color: var(--ink-faint); min-width: 58px; text-align: right; }
.dict-state { font-size: 11px; color: var(--accent-bright); }

.dict-note {
  margin: 10px 0 4px;
  padding: 8px 12px;
  border-radius: 6px;
  font-size: 11.5px;
  border: 1px solid var(--line);
}
.dict-note.ok { background: var(--accent-dim); border-color: var(--accent-line); color: var(--ink); }
.dict-note.bad { background: var(--danger-bg); border-color: var(--danger); color: var(--ink); }

/* ---------- spelling panel ---------- */

/* Floats over the right of the workspace: the plan stays visible, because a
   correction only makes sense next to the row it belongs to. */
.spell-panel {
  position: fixed;
  top: 108px;
  right: 12px;
  bottom: 34px;
  width: 460px;
  max-width: calc(100vw - 24px);
  z-index: 55;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--surface-4);
  box-shadow: var(--shadow);
  overflow: hidden;
}
.spell-panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 10px 9px 14px;
  border-bottom: 1px solid var(--line);
  background: var(--surface-3);
}
.spell-panel-title { font-size: 12.5px; font-weight: 650; color: var(--ink); }
.spell-panel-body { flex: 1; overflow-y: auto; padding: 12px 14px 14px; }
.spell-panel-body .report-card { border: 0; background: transparent; padding: 0; margin-bottom: 14px; }

/* ---------- spelling ---------- */

.spell-hint {
  margin: 8px 0 0;
  padding: 10px 12px;
  border-radius: 6px;
  background: var(--surface-2);
  border: 1px solid var(--line);
  font-family: var(--mono);
  font-size: 11.5px;
  color: var(--ink);
  white-space: pre;
}

.spell-list { margin-top: 10px; max-height: calc(100vh - 300px); overflow-y: auto; }

.spell-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 8px 4px;
  border-bottom: 1px solid var(--line-soft);
}
.spell-row:last-child { border-bottom: none; }

.spell-main { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 10px; }
.spell-word {
  font-weight: 650;
  color: var(--warn);
  font-size: 12.5px;
  text-decoration: underline wavy var(--warn);
  text-underline-offset: 3px;
}
.spell-where { font-size: 10px; color: var(--ink-faint); white-space: nowrap; }
.spell-context {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--ink-soft);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.spell-acts { display: flex; gap: 6px; flex: none; align-items: center; }
.spell-fix, .spell-skip {
  border-radius: 4px;
  padding: 3px 10px;
  font-size: 11px;
  cursor: default;
  border: 1px solid var(--line);
  background: var(--surface-3);
  color: var(--ink);
}
.spell-fix {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--on-accent);
  font-weight: 600;
}
.spell-fix:hover { background: var(--accent-bright); }
.spell-skip:hover { background: var(--hover); }
.spell-none { font-size: 10.5px; color: var(--ink-faint); font-style: italic; }

.spell-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 14px;
  padding-top: 10px;
  border-top: 1px solid var(--line);
  font-size: 10.5px;
  color: var(--ink-faint);
}

/* ---------- sheets ---------- */

.sheet-pane { flex: 1; overflow: auto; background: var(--surface); }

.sheet { border-collapse: collapse; font-size: 12px; width: 100%; table-layout: fixed; }

.sheet th {
  position: sticky;
  top: 0;
  background: var(--grid-header);
  border: 1px solid var(--grid-line);
  height: 34px;
  font-weight: 500;
  font-size: 11px;
  color: var(--ink-soft);
  padding: 2px 7px;
  text-align: left;
  white-space: nowrap;
}

.sheet td {
  border: 1px solid var(--grid-line);
  height: 22px;
  padding: 0 7px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sheet tr:hover td { background: var(--hover); }
.sheet tr.selected td { background: var(--selection); }
.sheet tr.over td { color: var(--warn); font-weight: 600; }
.sheet tr.add-row td { color: var(--ink-faint); font-style: italic; }

/* ---------- network diagram ---------- */

.network-pane { flex: 1; overflow: auto; background: var(--bg); padding: 22px; }

.node {
  position: absolute;
  width: 168px;
  border: 1px solid var(--bar-edge);
  border-left: 3px solid var(--bar-edge);
  border-radius: 3px;
  background: var(--surface);
  font-size: 10px;
  padding: 4px 6px;
  line-height: 1.4;
}

.node.critical { border-color: var(--bar-critical-edge); border-left-color: var(--bar-critical-edge); }
.node .n-name { font-weight: 600; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.node .n-row { display: flex; justify-content: space-between; color: var(--ink-soft); }

/* ---------- calendar view ---------- */

.calendar-pane { flex: 1; overflow: auto; background: var(--surface); padding: 12px; }

.cal-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  border-left: 1px solid var(--grid-line);
  border-top: 1px solid var(--grid-line);
}

.cal-dow {
  background: var(--grid-header);
  border-right: 1px solid var(--grid-line);
  border-bottom: 1px solid var(--grid-line);
  padding: 5px 6px;
  font-size: 11px;
  color: var(--ink-soft);
  text-align: center;
}

.cal-cell {
  min-height: 86px;
  border-right: 1px solid var(--grid-line);
  border-bottom: 1px solid var(--grid-line);
  padding: 3px;
  font-size: 10px;
  overflow: hidden;
}

.cal-cell.nonworking { background: var(--nonworking); }
.cal-cell .d { font-size: 11px; color: var(--ink-faint); text-align: right; }

.cal-chip {
  background: var(--bar);
  color: var(--ink);
  border-radius: 2px;
  padding: 1px 4px;
  margin-bottom: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cal-chip.critical { background: var(--bar-critical); }
.cal-chip.summary { background: #3a4747; }

/* ---------- status bar ---------- */

.statusbar {
  height: 24px;
  background: var(--surface-2);
  border-top: 1px solid var(--line);
  color: var(--ink-soft);
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 0 10px;
  font-size: 11px;
  flex: none;
}

.statusbar .grow { flex: 1; }
.statusbar .chip { white-space: nowrap; }
.statusbar .chip b { color: var(--ink); font-weight: 500; }
.statusbar .warn { color: var(--warn); font-weight: 600; }

.zoom-slider {
  display: flex;
  align-items: stretch;
  height: 19px;
  border: 1px solid var(--line);
  border-radius: 4px;
  overflow: hidden;
  background: rgba(216, 231, 232, 0.04);
}

.zoom-btn {
  width: 24px;
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  line-height: 1;
  padding: 0 0 1px;
}

.zoom-btn:hover { background: var(--hover); color: var(--accent-bright); }

.zoom-label {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 62px;
  padding: 0 6px;
  font-size: 11px;
  color: var(--ink);
  border-left: 1px solid var(--line);
  border-right: 1px solid var(--line);
}

/* ---------- context menu ---------- */

/* Office puts a floating mini toolbar above its context menu. */
/* The toolbar and the menu, anchored as one so the toolbar is above the menu
   whichever way the pair opened. */
.ctx-stack {
  position: fixed;
  z-index: 81;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  max-height: 80vh;
}
.ctx-minibar-wrap { flex: none; }

.ctx-minibar {
  display: flex;
  align-items: center;
  gap: 1px;
  padding: 3px;
  background: var(--surface-4);
  border: 1px solid var(--line);
  border-radius: 6px;
  box-shadow: var(--shadow);
}

.minibtn {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  height: 26px;
  min-width: 28px;
  padding: 0 5px;
  border: 1px solid transparent;
  border-radius: 4px;
  background: transparent;
  color: var(--ink);
  cursor: default;
}

.minibtn svg { color: var(--accent); }
.minibtn:hover:not(.disabled) { background: var(--accent-dim); border-color: var(--accent-line); }
.minibtn:active:not(.disabled) { background: var(--pressed); }
.minibtn.disabled { opacity: 0.32; }
.minibtn.disabled svg { color: var(--ink-faint); }

.minisep { width: 1px; height: 18px; margin: 0 3px; background: var(--line); }

.ctx-scrim { position: fixed; inset: 0; z-index: 80; }

.ctxmenu {
  /* Sits inside the stack, so it is the menu that scrolls when the pair is
     taller than the room available, never the toolbar above it. */
  min-height: 0;
  overflow-y: auto;
  min-width: 226px;
  background: var(--surface-4);
  border: 1px solid var(--line);
  border-radius: 5px;
  box-shadow: var(--shadow);
  padding: 4px;
}

.ctxitem {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 5px 9px;
  border: 0;
  border-radius: 3px;
  background: transparent;
  color: var(--ink);
  font-size: 12px;
  text-align: left;
  cursor: default;
}

.ctxitem:hover:not(.disabled) { background: var(--accent-dim); color: var(--accent-bright); }
.ctxitem.disabled { opacity: 0.34; }
.ctxitem .glyph { width: 16px; display: grid; place-items: center; color: var(--accent); flex: none; }
.ctxitem .label { flex: 1; }
.ctxitem .shortcut { color: var(--ink-faint); font-size: 10.5px; }
.ctxitem .tick { width: 12px; color: var(--accent); }

/* A flagged issue inside the context menu: what is wrong, then what can be
   done about it. Wider than a menu row because it carries a sentence, not a
   command name. */
.ctx-issue {
  max-width: 340px;
  padding: 8px 10px;
  margin: 2px 0;
  border-radius: 5px;
  background: var(--accent-dim);
  border: 1px solid var(--accent-line);
}
.ctx-issue-text {
  display: block;
  font-size: 11.5px;
  line-height: 1.45;
  color: var(--ink);
}
.ctx-issue-acts { display: flex; gap: 6px; margin-top: 7px; }
.ctx-issue-fix, .ctx-issue-ignore {
  border-radius: 4px;
  padding: 3px 10px;
  font-size: 11px;
  cursor: default;
  border: 1px solid var(--line);
  background: var(--surface-3);
  color: var(--ink);
}
.ctx-issue-fix { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
.ctx-issue-fix:hover { background: var(--accent-bright); }
.ctx-issue-ignore:hover { background: var(--hover); }

/* A warning the planner has said they know about. Still there, so the row does
   not silently stop mentioning it, but no longer competing for attention. */
.ind-critical.ignored { color: var(--ink-faint); opacity: 0.55; }

/* An issue in the menu that has been dismissed reads the same way. */
.ctx-issue.ignored { background: transparent; border-color: var(--line); }
.ctx-issue.ignored .ctx-issue-text { color: var(--ink-faint); }

.ctxsep { height: 1px; background: var(--line); margin: 4px 6px; }

.ctxheader {
  padding: 5px 9px 6px;
  font-size: 10.5px;
  color: var(--ink-faint);
  letter-spacing: 0.4px;
  text-transform: uppercase;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---------- dialogs ---------- */

.scrim {
  position: fixed;
  inset: 0;
  background: rgba(4, 8, 8, 0.62);
  z-index: 70;
  display: grid;
  place-items: center;
}

.dlg {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 7px;
  box-shadow: var(--shadow);
  min-width: 470px;
  max-width: 860px;
  max-height: 86vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.dlg-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
  font-weight: 600;
  color: var(--ink);
}

.dlg-close {
  border: 0;
  background: transparent;
  color: var(--ink-soft);
  cursor: default;
  width: 24px;
  height: 24px;
  border-radius: 3px;
}

.dlg-close:hover { background: var(--danger); color: #fff; }

.dlg-body { padding: 16px; overflow-y: auto; }

.dlg-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 11px 14px;
  border-top: 1px solid var(--line);
  background: var(--surface-2);
}

.dlg-tabs {
  display: flex;
  gap: 3px;
  padding: 9px 14px 0;
  background: var(--surface-2);
  border-bottom: 1px solid var(--line);
}

.dlg-tab {
  border: 1px solid transparent;
  border-bottom: 0;
  background: transparent;
  color: var(--ink-soft);
  padding: 5px 14px;
  border-radius: 4px 4px 0 0;
  cursor: default;
  font-size: 11px;
}

.dlg-tab:hover { color: var(--ink); background: var(--hover); }
.dlg-tab.active { background: var(--surface); border-color: var(--line); color: var(--accent-bright); position: relative; top: 1px; }

.form-row { display: flex; align-items: center; gap: 10px; margin-bottom: 11px; }
.form-row label { width: 132px; flex: none; color: var(--ink-soft); }
.form-row .grow { flex: 1; }

.assign-table { width: 100%; border-collapse: collapse; font-size: 12px; }
.assign-table th, .assign-table td { border: 1px solid var(--grid-line); padding: 4px 7px; text-align: left; }
.assign-table th { background: var(--grid-header); color: var(--ink-soft); font-weight: 500; }
.assign-table tr.on td { background: var(--selection); }
.assign-table tr:hover td { background: var(--hover); }

.hint { color: var(--ink-soft); font-size: 11px; margin-top: 10px; line-height: 1.5; }

/* Trailing unit beside a rate box, so the number does not have to say it. */
.unit { color: var(--ink-soft); font-size: 11px; }
.dlg-sub { font-size: 12px; font-weight: 600; color: var(--ink); margin: 0 0 8px; }
.sep { height: 1px; background: var(--grid-line); margin: 14px 0; }
.dlg-list { max-height: 150px; overflow-y: auto; border: 1px solid var(--grid-line); border-radius: 3px; margin-top: 8px; }
.dlg-list-row { display: flex; align-items: center; gap: 10px; padding: 4px 9px; font-size: 12px; }
.dlg-list-row:nth-child(even) { background: var(--hover); }
.dlg-list-row .grow { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.error-banner {
  background: var(--danger-bg);
  border-bottom: 1px solid var(--danger);
  color: var(--danger);
  padding: 6px 12px;
  font-size: 11px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
}

.empty-state {
  display: grid;
  place-items: center;
  height: 100%;
  color: var(--ink-faint);
  font-size: 13px;
  text-align: center;
  line-height: 1.7;
  white-space: pre-line;
}
"##;

/// The light palette, as rules that can stand alone or sit inside a query.
///
/// Emitted after the main stylesheet and nothing else, so it wins purely by
/// order and every rule written against the tokens follows it without knowing
/// a second theme exists. Only the tokens are restated; anything that reads a
/// raw colour rather than a token would have to be fixed where it does so, not
/// here.
///
/// The accent darkens rather than staying put: a mid teal that reads well on
/// near-black has too little contrast against white to carry text or a focus
/// ring.
const LIGHT_RULES: &str = r##"
:root {
  --bg: #eaefef;
  --surface: #ffffff;
  --surface-2: #f2f7f7;
  --surface-3: #f7fafa;
  --surface-4: #ffffff;

  --accent: #2f5f5e;
  --accent-bright: #1e4746;
  --on-accent: #f4f8f8;
  --accent-dim: rgba(47, 95, 94, 0.10);
  --accent-line: rgba(47, 95, 94, 0.38);
  --contextual: #45699b;

  --line: #d2dedd;
  --line-soft: #e6eded;
  --ink: #10201f;
  --ink-soft: #4a6362;
  --ink-faint: #7b9291;

  --hover: rgba(16, 32, 31, 0.055);
  --pressed: rgba(16, 32, 31, 0.10);
  --selection: rgba(47, 95, 94, 0.13);
  --selection-line: rgba(47, 95, 94, 0.5);
  --focus: var(--accent);

  --grid-line: #e3ebeb;
  --grid-header: #f2f7f7;
  --nonworking: rgba(16, 32, 31, 0.038);

  /* chart */
  --bar: #4b8b8b;
  --bar-edge: #2f5f5e;
  --bar-progress: #1e4746;
  --bar-critical: #b3565c;
  --bar-critical-edge: #8b393f;
  --bar-progress-critical: #7a2f34;
  --bar-summary: #20403f;
  --bar-inactive: #b9c6c6;
  --baseline: #8aa3a2;
  --slack: #a9bdbc;
  --today: #b3565c;
  --link-arrow: #6d8786;

  --danger: #ac5157;
  --danger-bg: rgba(172, 81, 87, 0.10);
  --warn: #9d6f16;

  --shadow: 0 12px 30px rgba(16, 32, 31, 0.18);
}
"##;

/// Which palette to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Follow whatever the desktop asks for, and change with it.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ORDER: [ThemeChoice; 3] =
        [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "System",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }

    pub fn from_label(label: &str) -> Self {
        Self::ORDER
            .into_iter()
            .find(|choice| choice.label() == label)
            .unwrap_or_default()
    }

    /// The stylesheet to emit after the main one.
    ///
    /// Following the desktop is a media query rather than something read over
    /// D-Bus and polled: the engine already knows the answer, and a query keeps
    /// up on its own when the desktop switches while the application is open.
    pub fn overlay(self) -> String {
        match self {
            ThemeChoice::Dark => String::new(),
            ThemeChoice::Light => LIGHT_RULES.to_string(),
            ThemeChoice::System => {
                format!("@media (prefers-color-scheme: light) {{\n{LIGHT_RULES}\n}}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn following_the_desktop_is_the_default() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }

    #[test]
    fn dark_needs_no_overlay_because_it_is_what_the_sheet_already_says() {
        assert!(ThemeChoice::Dark.overlay().is_empty());
    }

    #[test]
    fn choosing_light_applies_it_whatever_the_desktop_says() {
        let overlay = ThemeChoice::Light.overlay();
        assert!(overlay.contains("--surface: #ffffff"));
        assert!(
            !overlay.contains("prefers-color-scheme"),
            "an explicit choice is not conditional on the desktop"
        );
    }

    #[test]
    fn following_the_desktop_only_lightens_when_it_asks() {
        let overlay = ThemeChoice::System.overlay();
        assert!(overlay.starts_with("@media (prefers-color-scheme: light)"));
        assert!(overlay.contains("--surface: #ffffff"));
    }

    #[test]
    fn every_choice_survives_a_round_trip_through_its_label() {
        for choice in ThemeChoice::ORDER {
            assert_eq!(ThemeChoice::from_label(choice.label()), choice);
        }
    }
}
