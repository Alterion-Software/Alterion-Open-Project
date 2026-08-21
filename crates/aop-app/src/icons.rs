//! Ribbon iconography.
//!
//! Traced from Lucide, which is ISC licensed and so compatible with this
//! project. Fetched from the icon set rather than drawn by hand: a home made
//! approximation of a well known icon reads as an approximation, and a hundred
//! of them read as a hundred.
//!
//! Lucide draws in one colour and leaves it to the caller, which is better than
//! baking colours into the paths: a glyph then takes the colour of whatever it
//! sits in, and dark and light themes need no separate artwork. Ribbon commands
//! that want a colour get it here as a tint, chosen by what the command does
//! rather than per icon, so a family of related commands looks like one.
//!
//! Single quotes are used inside the markup so the Rust literals stay free of
//! escapes.

use dioxus::prelude::*;

/// Render a named icon. Unknown names fall back to a neutral document glyph so
/// a typo shows up as a plain shape rather than an empty button.
pub fn icon(name: &str, size: u32) -> Element {
    let body = body_for(name);
    let tint = tint_for(name);
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            color: "{tint}",
            stroke: "currentColor",
            // Lucide draws at 2 against a 24 unit box. These render between 13
            // and 28 pixels, where a hairline goes muddy, so they sit just under
            // the source weight rather than well under it.
            stroke_width: "1.75",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            dangerous_inner_html: "{body}",
        }
    }
}

/// What colour a command's glyph takes.
///
/// By meaning rather than per icon, so a family of related commands reads as
/// one. Everything else inherits from whatever it sits in, which is what makes
/// the same glyph work in a dark ribbon, a light one, and a menu row.
fn tint_for(name: &str) -> &'static str {
    match name {
        // Removes or stops something.
        "cut" | "clear" | "inactivate" | "unlink" | "close-doc" | "critical-tasks"
        | "warning" => "var(--danger)",
        // Time, dates and the calendar.
        "calendar" | "working-time" | "timescale" | "status-date" | "auto-schedule"
        | "manual-schedule" | "update-project" | "add-to-timeline" | "timeline-band" => {
            "var(--contextual)"
        }
        // People.
        "assign-resources" | "team-planner" | "resource-pool" | "resource-sheet"
        | "resource-usage" | "add-resource" | "account" | "share" => "var(--accent)",
        // Confirms something.
        "mark-on-track" | "deliverable" | "respect-links" => "var(--bar-progress)",
        // Everything else takes the colour of what it sits in.
        _ => "currentColor",
    }
}

fn body_for(name: &str) -> String {
    match name {
        // ---- traced from Lucide, ISC licensed --------------------------
        //
        // Fetched from the icon set rather than drawn here: a hand made
        // approximation of a well known icon reads as an approximation.
        // Six shapes that used to be typed as characters and were not there.
        //
        // A caret, a tick, a flag, a pencil, a dot and a turning arrow are
        // affordances, not text, and typing them as codepoints made them
        // depend on the font having that glyph. The bundled font does not have
        // any of these six, and with no system fallback they simply were not
        // drawn: every dropdown lost its caret and nothing said so. Drawn
        // here, they are the same in every build on every machine, they take
        // their colour from `currentColor` like the rest of the set, and they
        // line up by geometry rather than by a font's baseline.
        //
        // lucide: chevron-down
        "caret-down" => String::from("<path d='m6 9 6 6 6-6' />"),
        // lucide: check
        "tick" => String::from("<path d='M20 6 9 17l-5-5' />"),
        // lucide: flag
        "flag" => String::from(
            "<path d='M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z' />\
             <line x1='4' x2='4' y1='22' y2='15' />",
        ),
        // lucide: pencil
        "pencil" => String::from(
            "<path d='M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 \
             4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z' />\
             <path d='m15 5 4 4' />",
        ),
        // lucide: circle-dot
        "fisheye" => String::from(
            "<circle cx='12' cy='12' r='10' /><circle cx='12' cy='12' r='2.5' />",
        ),
        // lucide: corner-down-right
        "turn-down-right" => String::from(
            "<path d='m15 10 5 5-5 5' /><path d='M4 4v7a4 4 0 0 0 4 4h12' />",
        ),

        // lucide: user
        "account" => String::from(
            "<path d='M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2' /><circle cx='12' cy='7' r='4' />",
        ),
        // lucide: user-plus
        "add-resource" => String::from(
            "<path d='M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2' /><circle cx='9' cy='7' r='4' /><line x1='19' x2='19' y1='8' y2='14' /><line x1='22' x2='16' y1='11' y2='11' />",
        ),
        // lucide: calendar-plus
        "add-to-timeline" => String::from(
            "<path d='M16 18h6' /><path d='M16 2v3' /><path d='M19 15v6' /><path d='M21 11.5V5a2 2 0 00-2-2H5a2 2 0 00-2 2v14a2 2 0 002 2h8.3' /><path d='M3 9h18' /><path d='M8 2v3' />",
        ),
        // lucide: layout-grid
        "apps" => String::from(
            "<rect width='7' height='7' x='3' y='3' rx='1' /><rect width='7' height='7' x='14' y='3' rx='1' /><rect width='7' height='7' x='14' y='14' rx='1' /><rect width='7' height='7' x='3' y='14' rx='1' />",
        ),
        // lucide: layout-grid
        "arrange-all" => String::from(
            "<rect width='7' height='7' x='3' y='3' rx='1' /><rect width='7' height='7' x='14' y='3' rx='1' /><rect width='7' height='7' x='14' y='14' rx='1' /><rect width='7' height='7' x='3' y='14' rx='1' />",
        ),
        // lucide: user-round-arrow-left
        "assign-resources" => String::from(
            "<path d='m19 16-3 3' /><path d='M2 21a8 8 0 0 1 12.664-6.5' /><path d='M22 19h-6l3 3' /><circle cx='10' cy='8' r='5' />",
        ),
        // lucide: calendar-cog
        "auto-schedule" => String::from(
            "<path d='m15.228 16.852-.923-.383' /><path d='m15.228 19.148-.923.383' /><path d='M16 2v3' /><path d='m16.47 14.305.382.923' /><path d='m16.852 20.772-.383.924' /><path d='m19.148 15.228.383-.923' /><path d='m19.53 21.696-.382-.924' /><path d='m20.773 16.852.924-.383' /><path d='m20.773 19.148.924.383' /><path d='M21 10.5V5a2 2 0 00-2-2H5a2 2 0 00-2 2v14a2 2 0 002 2h5.5' /><path d='M3 9h18' /><path d='M8 2v3' /><circle cx='18' cy='18' r='3' />",
        ),
        // lucide: arrow-left
        "back" => String::from(
            "<path d='m12 19-7-7 7-7' /><path d='M19 12H5' />",
        ),
        // lucide: git-compare-arrows
        "baseline" => String::from(
            "<circle cx='5' cy='6' r='3' /><path d='M12 6h5a2 2 0 0 1 2 2v7' /><path d='m15 9-3-3 3-3' /><circle cx='19' cy='18' r='3' /><path d='M12 18H7a2 2 0 0 1-2-2V9' /><path d='m9 15 3 3-3 3' />",
        ),
        // lucide: bold
        "bold" => String::from(
            "<path d='M6 12h9a4 4 0 0 1 0 8H7a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h7a4 4 0 0 1 0 8' />",
        ),
        // lucide: trending-down, work falling away as it is done
        "burndown" => String::from(
            "<path d='M16 17h6v-6' /><path d='m22 17-8.5-8.5-5 5L2 7' />",
        ),
        // lucide: trending-up, work accumulating towards the total
        "burnup" => String::from(
            "<path d='M16 7h6v6' /><path d='m22 7-8.5 8.5-5-5L2 17' />",
        ),
        // lucide: calculator
        "calculate" => String::from(
            "<rect width='16' height='20' x='4' y='2' rx='2' /><line x1='8' x2='16' y1='6' y2='6' /><line x1='16' x2='16' y1='14' y2='18' /><path d='M16 10h.01' /><path d='M12 10h.01' /><path d='M8 10h.01' /><path d='M12 14h.01' /><path d='M8 14h.01' /><path d='M12 18h.01' /><path d='M8 18h.01' />",
        ),
        // lucide: calendar
        "calendar" => String::from(
            "<path d='M8 2v3' /><path d='M16 2v3' /><rect x='3' y='3' width='18' height='18' rx='2' /><path d='M3 9h18' />",
        ),
        // lucide: eraser
        "clear" => String::from(
            "<path d='M21 21H8a2 2 0 0 1-1.42-.587l-3.994-3.999a2 2 0 0 1 0-2.828l10-10a2 2 0 0 1 2.829 0l5.999 6a2 2 0 0 1 0 2.828L12.834 21' /><path d='m5.082 11.09 8.828 8.828' />",
        ),
        // lucide: file-x
        "close-doc" => String::from(
            "<path d='M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z' /><path d='M14 2v5a1 1 0 0 0 1 1h5' /><path d='m14.5 12.5-5 5' /><path d='m9.5 12.5 5 5' />",
        ),
        // lucide: info
        "col-indicators" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M12 16v-4' /><path d='M12 8h.01' />",
        ),
        // lucide: toggle-left
        // lucide: clock. The Task Mode column asks whether a task is
        // scheduled by hand or by the plan, which is a question about time.
        "col-mode" => String::from(
            "<path d='M12 6v6l4 2' /><circle cx='12' cy='12' r='10' />",
        ),
        // lucide: columns-3-cog
        "column-settings" => String::from(
            "<path d='M10.6 21H5a2 2 0 01-2-2V5a2 2 0 012-2h14a2 2 0 012 2v5.6' /><path d='m14.305 19.53.923-.382' /><path d='M15 3v7.6' /><path d='m15.229 16.852-.924-.383' /><path d='m16.852 15.228-.383-.923' /><path d='m16.852 20.772-.383.924' /><path d='m19.148 15.228.383-.923' /><path d='m19.53 21.696-.382-.924' /><path d='m20.773 16.852.922-.383' /><path d='m20.773 19.148.922.383' /><path d='M9 3v18' /><circle cx='18' cy='18' r='3' />",
        ),
        // lucide: git-compare
        "compare" => String::from(
            "<circle cx='18' cy='18' r='3' /><circle cx='6' cy='6' r='3' /><path d='M13 6h3a2 2 0 0 1 2 2v7' /><path d='M11 18H8a2 2 0 0 1-2-2V9' />",
        ),
        // lucide: copy
        "copy" => String::from(
            "<rect width='14' height='14' x='8' y='8' rx='2' ry='2' /><path d='M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2' />",
        ),
        // lucide: route, a path through the plan, literally
        "critical-path" => String::from(
            "<circle cx='6' cy='19' r='3' /><path d='M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15' /><circle cx='18' cy='5' r='3' />",
        ),
        // lucide: triangle-alert
        "critical-tasks" => String::from(
            "<path d='m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3' /><path d='M12 9v4' /><path d='M12 17h.01' />",
        ),
        // lucide: square-pen
        "custom-fields" => String::from(
            "<path d='M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7' /><path d='M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z' />",
        ),
        // lucide: scissors
        "cut" => String::from(
            "<circle cx='6' cy='6' r='3' /><path d='M8.12 8.12 12 12' /><path d='M20 4 8.12 15.88' /><circle cx='6' cy='18' r='3' /><path d='M14.8 14.8 20 20' />",
        ),
        // lucide: layout-dashboard
        "dashboard" => String::from(
            "<rect width='7' height='9' x='3' y='3' rx='1' /><rect width='7' height='5' x='14' y='3' rx='1' /><rect width='7' height='9' x='14' y='12' rx='1' /><rect width='7' height='5' x='3' y='16' rx='1' />",
        ),
        // lucide: package-check
        "deliverable" => String::from(
            "<path d='M12 22V12' /><path d='m16 17 2 2 4-4' /><path d='M21 11.127V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.729l7 4a2 2 0 0 0 2 .001l1.32-.753' /><path d='M3.29 7 12 12l8.71-5' /><path d='m7.5 4.27 8.997 5.148' />",
        ),
        // lucide: panel-bottom
        "details" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='M3 15h18' />",
        ),
        // lucide: panel-bottom-open
        "details-pane" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='M3 15h18' /><path d='m9 10 3-3 3 3' />",
        ),
        // lucide: pen-tool
        "drawing" => String::from(
            "<path d='M15.707 21.293a1 1 0 0 1-1.414 0l-1.586-1.586a1 1 0 0 1 0-1.414l5.586-5.586a1 1 0 0 1 1.414 0l1.586 1.586a1 1 0 0 1 0 1.414z' /><path d='m18 13-1.375-6.874a1 1 0 0 0-.746-.776L3.235 2.028a1 1 0 0 0-1.207 1.207L5.35 15.879a1 1 0 0 0 .776.746L13 18' /><path d='m2.3 2.3 7.286 7.286' /><circle cx='11' cy='11' r='2' />",
        ),
        // lucide: maximize
        "entire-project" => String::from(
            "<path d='M8 3H5a2 2 0 0 0-2 2v3' /><path d='M21 8V5a2 2 0 0 0-2-2h-3' /><path d='M3 16v3a2 2 0 0 0 2 2h3' /><path d='M16 21h3a2 2 0 0 0 2-2v-3' />",
        ),
        // lucide: file-up
        "export" => String::from(
            "<path d='M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z' /><path d='M14 2v5a1 1 0 0 0 1 1h5' /><path d='M12 12v6' /><path d='m15 15-3-3-3 3' />",
        ),
        // lucide: message-square
        "feedback" => String::from(
            "<path d='M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z' />",
        ),
        // lucide: paint-bucket
        "fill-color" => String::from(
            "<path d='M11 7 6 2' /><path d='M18.992 12H2.041' /><path d='M21.145 18.38A3.34 3.34 0 0 1 20 16.5a3.3 3.3 0 0 1-1.145 1.88c-.575.46-.855 1.02-.855 1.595A2 2 0 0 0 20 22a2 2 0 0 0 2-2.025c0-.58-.285-1.13-.855-1.595' /><path d='m8.5 4.5 2.148-2.148a1.205 1.205 0 0 1 1.704 0l7.296 7.296a1.205 1.205 0 0 1 0 1.704l-7.592 7.592a3.615 3.615 0 0 1-5.112 0l-3.888-3.888a3.615 3.615 0 0 1 0-5.112L5.67 7.33' />",
        ),
        // lucide: arrow-down-to-line
        "fill-down" => String::from(
            "<path d='M12 17V3' /><path d='m6 11 6 6 6-6' /><path d='M19 21H5' />",
        ),
        // lucide: funnel
        "filter" => String::from(
            "<path d='M10 20a1 1 0 0 0 .553.895l2 1A1 1 0 0 0 14 21v-7a2 2 0 0 1 .517-1.341L21.74 4.67A1 1 0 0 0 21 3H3a1 1 0 0 0-.742 1.67l7.225 7.989A2 2 0 0 1 10 14z' />",
        ),
        // lucide: search
        "find" => String::from(
            "<path d='m21 21-4.34-4.34' /><circle cx='11' cy='11' r='8' />",
        ),
        // lucide: folder
        "folder" => String::from(
            "<path d='M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z' />",
        ),
        // lucide: baseline
        "font-color" => String::from(
            "<path d='M4 20h16' /><path d='m6 16 6-12 6 12' /><path d='M8 12h8' />",
        ),
        // lucide: paintbrush
        "format-painter" => String::from(
            "<path d='m14.622 17.897-10.68-2.913' /><path d='M18.376 2.622a1 1 0 1 1 3.002 3.002L17.36 9.643a.5.5 0 0 0 0 .707l.944.944a2.41 2.41 0 0 1 0 3.408l-.944.944a.5.5 0 0 1-.707 0L8.354 7.348a.5.5 0 0 1 0-.707l.944-.944a2.41 2.41 0 0 1 3.408 0l.944.944a.5.5 0 0 0 .707 0z' /><path d='M9 8c-1.804 2.71-3.97 3.46-6.583 3.948a.507.507 0 0 0-.302.819l7.32 8.883a1 1 0 0 0 1.185.204C12.735 20.405 16 16.792 16 15' />",
        ),
        // lucide: chart-gantt
        "gantt" => String::from(
            "<path d='M10 6h8' /><path d='M12 16h6' /><path d='M3 3v16a2 2 0 0 0 2 2h16' /><path d='M8 11h7' />",
        ),
        // lucide: grid-3x3
        "gridlines" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='M3 9h18' /><path d='M3 15h18' /><path d='M9 3v18' /><path d='M15 3v18' />",
        ),
        // lucide: group
        "group-by" => String::from(
            "<path d='M3 7V5c0-1.1.9-2 2-2h2' /><path d='M17 3h2c1.1 0 2 .9 2 2v2' /><path d='M21 17v2c0 1.1-.9 2-2 2h-2' /><path d='M7 21H5c-1.1 0-2-.9-2-2v-2' /><rect width='7' height='5' x='7' y='7' rx='1' /><rect width='7' height='5' x='10' y='12' rx='1' />",
        ),
        // lucide: circle-question-mark
        "help" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3' /><path d='M12 17h.01' />",
        ),
        // lucide: eye-off
        "hide" => String::from(
            "<path d='M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49' /><path d='M14.084 14.158a3 3 0 0 1-4.242-4.242' /><path d='M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143' /><path d='m2 2 20 20' />",
        ),
        // lucide: highlighter
        "highlight" => String::from(
            "<path d='m9 11-6 6v3h9l3-3' /><path d='m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4' />",
        ),
        // lucide: ban
        "inactivate" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M4.929 4.929 19.07 19.071' />",
        ),
        // lucide: info
        "info" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M12 16v-4' /><path d='M12 8h.01' />",
        ),
        // lucide: info
        "information" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M12 16v-4' /><path d='M12 8h.01' />",
        ),
        // lucide: between-horizontal-start
        "insert-column" => String::from(
            "<rect width='13' height='7' x='8' y='3' rx='1' /><path d='m2 9 3 3-3 3' /><rect width='13' height='7' x='8' y='14' rx='1' />",
        ),
        // lucide: search-check
        "inspect" => String::from(
            "<path d='m8 11 2 2 4-4' /><circle cx='11' cy='11' r='8' /><path d='m21 21-4.3-4.3' />",
        ),
        // lucide: italic
        "italic" => String::from(
            "<line x1='19' x2='10' y1='4' y2='4' /><line x1='14' x2='5' y1='20' y2='20' /><line x1='15' x2='9' y1='4' y2='20' />",
        ),
        // lucide: layout-template
        "layout" => String::from(
            "<rect width='18' height='7' x='3' y='3' rx='1' /><rect width='9' height='7' x='3' y='14' rx='1' /><rect width='5' height='7' x='16' y='14' rx='1' />",
        ),
        // lucide: panel-left
        "layout-left" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='M9 3v18' />",
        ),
        // lucide: panel-right
        "layout-right" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='M15 3v18' />",
        ),
        // lucide: columns-2
        "layout-split" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='M12 3v18' />",
        ),
        // lucide: align-vertical-justify-center
        "level" => String::from(
            "<rect width='14' height='6' x='5' y='16' rx='2' /><rect width='10' height='6' x='7' y='2' rx='2' /><path d='M2 12h20' />",
        ),
        // lucide: settings-2
        "level-options" => String::from(
            "<path d='M14 17H5' /><path d='M19 7h-9' /><circle cx='17' cy='17' r='3' /><circle cx='7' cy='7' r='3' />",
        ),
        // lucide: link
        "link" => String::from(
            "<path d='M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71' /><path d='M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71' />",
        ),
        // lucide: link-2
        "links-between" => String::from(
            "<path d='M9 17H7A5 5 0 0 1 7 7h2' /><path d='M15 7h2a5 5 0 1 1 0 10h-2' /><line x1='8' x2='16' y1='12' y2='12' />",
        ),
        // lucide: terminal
        "macros" => String::from(
            "<path d='M12 19h8' /><path d='m4 17 6-6-6-6' />",
        ),
        // lucide: pin
        "manual-schedule" => String::from(
            "<path d='M12 17v5' /><path d='M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z' />",
        ),
        // lucide: circle-check
        "mark-on-track" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='m9 12 2 2 4-4' />",
        ),
        // lucide: diamond
        "milestone" => String::from(
            "<path d='M2.7 10.3a2.41 2.41 0 0 0 0 3.41l7.59 7.59a2.41 2.41 0 0 0 3.41 0l7.59-7.59a2.41 2.41 0 0 0 0-3.41l-7.59-7.59a2.41 2.41 0 0 0-3.41 0Z' />",
        ),
        // lucide: clock
        "mode" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M12 6v6l4 2' />",
        ),
        // lucide: zap, placed by the scheduler
        "mode-auto" => String::from(
            "<path d='M15.914 4a1.5 1.5 0 00-2.474-1.561l-9 9A1.5 1.5 0 005.5 14h4.002a.5.5 0 01.471.666L8.086 20a1.5 1.5 0 002.475 1.56l9-9A1.5 1.5 0 0018.5 10h-3.997a.5.5 0 01-.472-.667z' />",
        ),
        // lucide: pin, a row the scheduler is not allowed to move
        "mode-manual" => String::from(
            "<path d='M12 17v5' /><path d='M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z' />",
        ),
        // lucide: move
        "move" => String::from(
            "<path d='M12 2v20' /><path d='m15 19-3 3-3-3' /><path d='m19 9 3 3-3 3' /><path d='M2 12h20' /><path d='m5 9-3 3 3 3' /><path d='m9 5 3-3 3 3' />",
        ),
        // lucide: arrow-right-left
        "move-project" => String::from(
            "<path d='m16 3 4 4-4 4' /><path d='M20 7H4' /><path d='m8 21-4-4 4-4' /><path d='M4 17h16' />",
        ),
        // lucide: workflow
        "network" => String::from(
            "<rect width='8' height='8' x='3' y='3' rx='2' /><path d='M7 11v4a2 2 0 0 0 2 2h4' /><rect width='8' height='8' x='13' y='13' rx='2' />",
        ),
        // lucide: file-plus
        "new" => String::from(
            "<path d='M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z' /><path d='M14 2v5a1 1 0 0 0 1 1h5' /><path d='M9 15h6' /><path d='M12 18v-6' />",
        ),
        // lucide: app-window
        "new-window" => String::from(
            "<rect x='2' y='4' width='20' height='16' rx='2' /><path d='M10 4v4' /><path d='M2 8h20' /><path d='M6 4v4' />",
        ),
        // lucide: chevrons-right
        "next-over" => String::from(
            "<path d='m6 17 5-5-5-5' /><path d='m13 17 5-5-5-5' />",
        ),
        // lucide: notebook-pen
        "notes" => String::from(
            "<path d='M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4' /><path d='M2 6h4' /><path d='M2 10h4' /><path d='M2 14h4' /><path d='M2 18h4' /><path d='M21.378 5.626a1 1 0 1 0-3.004-3.004l-5.01 5.012a2 2 0 0 0-.506.854l-.837 2.87a.5.5 0 0 0 .62.62l2.87-.837a2 2 0 0 0 .854-.506z' />",
        ),
        // lucide: folder-open
        "open" => String::from(
            "<path d='m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2' />",
        ),
        // lucide: settings
        "options" => String::from(
            "<path d='M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915' /><circle cx='12' cy='12' r='3' />",
        ),
        // lucide: layout-grid
        "other-views" => String::from(
            "<rect width='7' height='7' x='3' y='3' rx='1' /><rect width='7' height='7' x='14' y='3' rx='1' /><rect width='7' height='7' x='14' y='14' rx='1' /><rect width='7' height='7' x='3' y='14' rx='1' />",
        ),
        // lucide: list-tree
        "outline" => String::from(
            "<path d='M8 5h13' /><path d='M13 12h8' /><path d='M13 19h8' /><path d='M3 10a2 2 0 0 0 2 2h3' /><path d='M3 5v12a2 2 0 0 0 2 2h3' />",
        ),
        // lucide: list-ordered
        "outline-number" => String::from(
            "<path d='M11 5h10' /><path d='M11 12h10' /><path d='M11 19h10' /><path d='M4 4h1v5' /><path d='M4 9h2' /><path d='M6.5 20H3.4c0-1 2.6-1.925 2.6-3.5a1.5 1.5 0 0 0-2.6-1.02' />",
        ),
        // lucide: package
        "package" => String::from(
            "<path d='M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z' /><path d='M12 22V12' /><polyline points='3.29 7 12 12 20.71 7' /><path d='m7.5 4.27 9 5.15' />",
        ),
        // lucide: clipboard-paste
        "paste" => String::from(
            "<path d='M11 14h10' /><path d='M16 4h2a2 2 0 0 1 2 2v1.344' /><path d='m17 18 4-4-4-4' /><path d='M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 1.793-1.113' /><rect x='8' y='2' width='8' height='4' rx='1' />",
        ),
        // lucide: percent
        "percent" => String::from(
            "<line x1='19' x2='5' y1='5' y2='19' /><circle cx='6.5' cy='6.5' r='2.5' /><circle cx='17.5' cy='17.5' r='2.5' />",
        ),
        // lucide: printer
        "print" => String::from(
            "<path d='M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2' /><path d='M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6' /><rect x='6' y='14' width='12' height='8' rx='1' />",
        ),
        // lucide: info
        "project-info" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M12 16v-4' /><path d='M12 8h.01' />",
        ),
        // lucide: file-text
        "project-summary" => String::from(
            "<path d='M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z' /><path d='M14 2v5a1 1 0 0 0 1 1h5' /><path d='M10 9H8' /><path d='M16 13H8' /><path d='M16 17H8' />",
        ),
        // lucide: redo-2
        "redo" => String::from(
            "<path d='m15 14 5-5-5-5' /><path d='M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13' />",
        ),
        // lucide: receipt
        "report-costs" => String::from(
            "<path d='M12 17V7' /><path d='M16 8h-6a2 2 0 0 0 0 4h4a2 2 0 0 1 0 4H8' /><path d='M4 3a1 1 0 0 1 1-1 1.3 1.3 0 0 1 .7.2l.933.6a1.3 1.3 0 0 0 1.4 0l.934-.6a1.3 1.3 0 0 1 1.4 0l.933.6a1.3 1.3 0 0 0 1.4 0l.933-.6a1.3 1.3 0 0 1 1.4 0l.934.6a1.3 1.3 0 0 0 1.4 0l.933-.6A1.3 1.3 0 0 1 19 2a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1 1.3 1.3 0 0 1-.7-.2l-.933-.6a1.3 1.3 0 0 0-1.4 0l-.934.6a1.3 1.3 0 0 1-1.4 0l-.933-.6a1.3 1.3 0 0 0-1.4 0l-.933.6a1.3 1.3 0 0 1-1.4 0l-.934-.6a1.3 1.3 0 0 0-1.4 0l-.933.6a1.3 1.3 0 0 1-.7.2 1 1 0 0 1-1-1z' />",
        ),
        // lucide: file-chart-column
        "report-custom" => String::from(
            "<path d='M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z' /><path d='M14 2v5a1 1 0 0 0 1 1h5' /><path d='M8 18v-1' /><path d='M12 18v-6' /><path d='M16 18v-3' />",
        ),
        // lucide: chart-line
        "report-progress" => String::from(
            "<path d='M3 3v16a2 2 0 0 0 2 2h16' /><path d='m19 9-5 5-4-4-3 3' />",
        ),
        // lucide: square-user
        "resource-pool" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><circle cx='12' cy='10' r='3' /><path d='M7 21v-2a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v2' />",
        ),
        // lucide: table-2
        "resource-sheet" => String::from(
            "<path d='M9 3H5a2 2 0 0 0-2 2v4m6-6h10a2 2 0 0 1 2 2v4M9 3v18m0 0h10a2 2 0 0 0 2-2V9M9 21H5a2 2 0 0 1-2-2V9m0 0h18' />",
        ),
        // lucide: chart-column
        "resource-usage" => String::from(
            "<path d='M3 3v16a2 2 0 0 0 2 2h16' /><path d='M18 17V9' /><path d='M13 17V5' /><path d='M8 17v-3' />",
        ),
        // lucide: link-2
        "respect-links" => String::from(
            "<path d='M9 17H7A5 5 0 0 1 7 7h2' /><path d='M15 7h2a5 5 0 1 1 0 10h-2' /><line x1='8' x2='16' y1='12' y2='12' />",
        ),
        // lucide: save
        "save" => String::from(
            "<path d='M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z' /><path d='M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7' /><path d='M7 3v4a1 1 0 0 0 1 1h7' />",
        ),
        // lucide: scale
        "scale" => String::from(
            "<path d='M12 3v18' /><path d='m19 8 3 8a5 5 0 0 1-6 0zV7' /><path d='M3 7h1a17 17 0 0 0 8-2 17 17 0 0 0 8 2h1' /><path d='m5 8 3 8a5 5 0 0 1-6 0zV7' /><path d='M7 21h10' />",
        ),
        // lucide: locate-fixed
        "scroll-to-task" => String::from(
            "<line x1='2' x2='5' y1='12' y2='12' /><line x1='19' x2='22' y1='12' y2='12' /><line x1='12' x2='12' y1='2' y2='5' /><line x1='12' x2='12' y1='19' y2='22' /><circle cx='12' cy='12' r='7' /><circle cx='12' cy='12' r='3' />",
        ),
        // lucide: search
        "search" => String::from(
            "<path d='m21 21-4.34-4.34' /><circle cx='11' cy='11' r='8' />",
        ),
        // lucide: square-check
        "selected-tasks" => String::from(
            "<rect width='18' height='18' x='3' y='3' rx='2' /><path d='m9 12 2 2 4-4' />",
        ),
        // lucide: settings
        "settings" => String::from(
            "<path d='M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915' /><circle cx='12' cy='12' r='3' />",
        ),
        // The drawing tools, one glyph per shape so the menu reads at a glance.
        // lucide: move-up-right
        "shape-arrow" => String::from(
            "<path d='M13 5H19V11' /><path d='M19 5 5 19' />",
        ),
        // lucide: slash
        "shape-line" => String::from("<path d='M22 2 2 22' />"),
        // lucide: circle
        "shape-oval" => String::from("<circle cx='12' cy='12' r='10' />"),
        // lucide: rectangle-horizontal
        "shape-rectangle" => String::from(
            "<rect width='20' height='12' x='2' y='6' rx='2' />",
        ),
        // lucide: type
        "shape-text" => String::from(
            "<path d='M12 4v16' /><path d='M4 7V4h16v3' /><path d='M9 20h6' />",
        ),
        // lucide: move-horizontal
        "slack" => String::from(
            "<path d='m18 8 4 4-4 4' /><path d='M2 12h20' /><path d='m6 8-4 4 4 4' />",
        ),
        // lucide: arrow-up-down
        "sort" => String::from(
            "<path d='m21 16-4 4-4-4' /><path d='M17 20V4' /><path d='m3 8 4-4 4 4' /><path d='M7 4v16' />",
        ),
        // lucide: spell-check
        "spelling" => String::from(
            "<path d='m6 16 6-12 6 12' /><path d='M8 12h8' /><path d='m16 20 2 2 4-4' />",
        ),
        // lucide: calendar-check
        "status-date" => String::from(
            "<path d='M8 2v3' /><path d='M16 2v3' /><rect x='3' y='3' width='18' height='18' rx='2' /><path d='M3 9h18' /><path d='m9 15 2 2 4-4' />",
        ),
        // lucide: folder-tree
        "subproject" => String::from(
            "<path d='M20 10a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-2.5a1 1 0 0 1-.8-.4l-.9-1.2A1 1 0 0 0 15 3h-2a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1Z' /><path d='M20 21a1 1 0 0 0 1-1v-3a1 1 0 0 0-1-1h-2.9a1 1 0 0 1-.88-.55l-.42-.85a1 1 0 0 0-.92-.6H13a1 1 0 0 0-1 1v5a1 1 0 0 0 1 1Z' /><path d='M3 5a2 2 0 0 0 2 2h3' /><path d='M3 3v13a2 2 0 0 0 2 2h3' />",
        ),
        // lucide: replace
        "substitute" => String::from(
            "<path d='M14 4a1 1 0 0 1 1-1' /><path d='M15 10a1 1 0 0 1-1-1' /><path d='M21 4a1 1 0 0 0-1-1' /><path d='M21 9a1 1 0 0 1-1 1' /><path d='m3 7 3 3 3-3' /><path d='M6 10V5a2 2 0 0 1 2-2h2' /><rect x='3' y='14' width='7' height='7' rx='1' />",
        ),
        // lucide: brackets
        "summary" => String::from(
            "<path d='M16 3h3a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1h-3' /><path d='M8 21H5a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h3' />",
        ),
        // lucide: life-buoy
        "support" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='m4.93 4.93 4.24 4.24' /><path d='m14.83 9.17 4.24-4.24' /><path d='m14.83 14.83 4.24 4.24' /><path d='m9.17 14.83-4.24 4.24' /><circle cx='12' cy='12' r='4' />",
        ),
        // lucide: layers
        "switch-windows" => String::from(
            "<path d='M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z' /><path d='M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12' /><path d='M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17' />",
        ),
        // lucide: cloud
        "cloud" => String::from(
            "<path d='M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z' />",
        ),
        // lucide: share-2
        "share" => String::from(
            "<circle cx='18' cy='5' r='3' /><circle cx='6' cy='12' r='3' /><circle cx='18' cy='19' r='3' /><path d='M8.59 13.51l6.83 3.98' /><path d='M15.41 6.51l-6.82 3.98' />",
        ),
        // lucide: refresh-cw
        "sync" => String::from(
            "<path d='M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8' /><path d='M21 3v5h-5' /><path d='M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16' /><path d='M8 16H3v5' />",
        ),
        // lucide: table
        "tables" => String::from(
            "<path d='M12 3v18' /><rect width='18' height='18' x='3' y='3' rx='2' /><path d='M3 9h18' /><path d='M3 15h18' />",
        ),
        // lucide: list-plus
        "task-add" => String::from(
            "<path d='M16 5H3' /><path d='M11 12H3' /><path d='M16 19H3' /><path d='M18 9v6' /><path d='M21 12h-6' />",
        ),
        // lucide: table-2
        "task-sheet" => String::from(
            "<path d='M9 3H5a2 2 0 0 0-2 2v4m6-6h10a2 2 0 0 1 2 2v4M9 3v18m0 0h10a2 2 0 0 0 2-2V9M9 21H5a2 2 0 0 1-2-2V9m0 0h18' />",
        ),
        // lucide: table-properties
        "task-usage" => String::from(
            "<path d='M15 3v18' /><rect width='18' height='18' x='3' y='3' rx='2' /><path d='M21 9H3' /><path d='M21 15H3' />",
        ),
        // lucide: users-round
        "team-planner" => String::from(
            "<path d='M18 21a8 8 0 0 0-16 0' /><circle cx='10' cy='8' r='5' /><path d='M22 20c0-3.37-2-6.5-4-8a5 5 0 0 0-.45-8.3' />",
        ),
        // lucide: type
        "text-styles" => String::from(
            "<path d='M12 4v16' /><path d='M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2' /><path d='M9 20h6' />",
        ),
        // lucide: chart-no-axes-gantt
        "timeline-band" => String::from(
            "<path d='M6 5h12' /><path d='M4 12h10' /><path d='M12 19h8' />",
        ),
        // lucide: ruler
        "timescale" => String::from(
            "<path d='M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z' /><path d='m14.5 12.5 2-2' /><path d='m11.5 9.5 2-2' /><path d='m8.5 6.5 2-2' /><path d='m17.5 15.5 2-2' />",
        ),
        // lucide: chart-gantt
        "tracking-gantt" => String::from(
            "<path d='M10 6h8' /><path d='M12 16h6' /><path d='M3 3v16a2 2 0 0 0 2 2h16' /><path d='M8 11h7' />",
        ),
        // lucide: graduation-cap
        "training" => String::from(
            "<path d='M21.42 10.922a1 1 0 0 0-.019-1.838L12.83 5.18a2 2 0 0 0-1.66 0L2.6 9.08a1 1 0 0 0 0 1.832l8.57 3.908a2 2 0 0 0 1.66 0z' /><path d='M22 10v6' /><path d='M6 12.5V16a6 3 0 0 0 12 0v-3.5' />",
        ),
        // lucide: underline
        "underline" => String::from(
            "<path d='M6 4v6a6 6 0 0 0 12 0V4' /><line x1='4' x2='20' y1='20' y2='20' />",
        ),
        // lucide: undo-2
        "undo" => String::from(
            "<path d='M9 14 4 9l5-5' /><path d='M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11' />",
        ),
        // lucide: unlink
        "unlink" => String::from(
            "<path d='m18.84 12.25 1.72-1.71h-.02a5.004 5.004 0 0 0-.12-7.07 5.006 5.006 0 0 0-6.95 0l-1.72 1.71' /><path d='m5.17 11.75-1.71 1.71a5.004 5.004 0 0 0 .12 7.07 5.006 5.006 0 0 0 6.95 0l1.71-1.71' /><line x1='8' x2='8' y1='2' y2='5' /><line x1='2' x2='5' y1='8' y2='8' /><line x1='16' x2='16' y1='19' y2='22' /><line x1='19' x2='22' y1='16' y2='16' />",
        ),
        // lucide: calendar-sync
        "update-project" => String::from(
            "<path d='M11 10v4h4' /><path d='m11 14 1.535-1.605a5 5 0 018 1.5' /><path d='M16 2v3' /><path d='m21 18-1.535 1.605a5 5 0 01-8-1.5' /><path d='M21 22v-4h-4' /><path d='M21 8.517V5a2 2 0 00-2-2H5a2 2 0 00-2 2v14a2 2 0 002 2h3.517' /><path d='M3 9h4' /><path d='M8 2v3' />",
        ),
        // lucide: gauge, a rate, which is what velocity is
        "velocity" => String::from(
            "<path d='m12 14 4-4' /><path d='M3.34 19a10 10 0 1 1 17.32 0' />",
        ),
        // lucide: chart-pie
        "visual-report" => String::from(
            "<path d='M21 12c.552 0 1.005-.449.95-.998a10 10 0 0 0-8.953-8.951c-.55-.055-.998.398-.998.95v8a1 1 0 0 0 1 1z' /><path d='M21.21 15.89A10 10 0 1 1 8 2.83' />",
        ),
        // lucide: triangle-alert
        "warning" => String::from(
            "<path d='m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3' /><path d='M12 9v4' /><path d='M12 17h.01' />",
        ),
        // lucide: network
        "wbs" => String::from(
            "<rect x='16' y='16' width='6' height='6' rx='1' /><rect x='2' y='16' width='6' height='6' rx='1' /><rect x='9' y='2' width='6' height='6' rx='1' /><path d='M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3' /><path d='M12 12V8' />",
        ),
        // lucide: sparkles
        "whats-new" => String::from(
            "<path d='M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z' /><path d='M20 2v4' /><path d='M22 4h-4' /><circle cx='4' cy='20' r='2' />",
        ),
        // lucide: clock
        "working-time" => String::from(
            "<circle cx='12' cy='12' r='10' /><path d='M12 6v6l4 2' />",
        ),
        // lucide: zoom-in
        "zoom-in" => String::from(
            "<circle cx='11' cy='11' r='8' /><line x1='21' x2='16.65' y1='21' y2='16.65' /><line x1='11' x2='11' y1='8' y2='14' /><line x1='8' x2='14' y1='11' y2='11' />",
        ),
        // lucide: zoom-out
        "zoom-out" => String::from(
            "<circle cx='11' cy='11' r='8' /><line x1='21' x2='16.65' y1='21' y2='16.65' /><line x1='8' x2='14' y1='11' y2='11' />",
        ),

        // lucide: coins, cost carried by the work itself
        "cost-task" => String::from(
            "<path d='M13.744 17.736a6 6 0 1 1-7.48-7.48' /><path d='M15 6h1v4' /><path d='m6.134 14.768.866-.5 2 3.464' /><circle cx='16' cy='8' r='6' />",
        ),
        // lucide: banknote, what a person costs
        "cost-resource" => String::from(
            "<rect width='20' height='12' x='2' y='6' rx='2' /><circle cx='12' cy='12' r='2' /><path d='M6 12h.01M18 12h.01' />",
        ),
        // ---- ours, where Lucide has no equivalent ----------------------
        "win-min" => "<path d='M4 12h16' stroke-width='1.1'/>".into(),
        "win-max" => "<rect x='4.5' y='4.5' width='15' height='15' rx='0.5' stroke-width='1.1'/>".into(),
        "win-restore" => "<rect x='4.5' y='7.5' width='12' height='12' rx='0.5' stroke-width='1.1'/><path d='M7.5 4.5h12v12' stroke-width='1.1'/>".into(),
        "win-close" => "<path d='M5 5l14 14M19 5L5 19' stroke-width='1.1'/>".into(),

        // ---- chrome ----------------------------------------------------
        // A clock with its hand turned back: work being taken back from a
        // session that did not finish. Monochrome, since it sits beside text.
        "history" => String::from(
            "<path d='M3.5 12a8.5 8.5 0 1 0 2.6-6.1'/>\
             <path d='M3 4.5V9h4.5'/>\
             <path d='M12 7.5V12l3 1.8'/>",
        ),
        // File menu chrome: monochrome, since these sit beside plain text
        // rather than in the ribbon where the Office palette applies.
        // ---- File menu and chrome -------------------------------------
        //
        // Traced from Lucide, monochrome so they take the colour of the text
        // they sit beside. The ribbon keeps the Office palette; a lone blue or
        // gold glyph in a column of chrome reads as a mistake rather than a
        // highlight.
        // A neutral cross. The ribbon's "clear" is red because it means undoing
        // work; removing a shortcut is not that.
        "x" => String::from("<path d='M18 6 6 18'/><path d='m6 6 12 12'/>"),
        "close" => String::from(
            "<path d='M13 4h3a2 2 0 0 1 2 2v14'/><path d='M2 20h3'/><path d='M13 20h9'/>\
             <path d='M10 12v.01'/>\
             <path d='M13 4.562v16.157a1 1 0 0 1-1.242.97L5.242 20.07a1 1 0 0 1-.742-.97V5.562a1 1 0 0 1 .78-.976l6-1.6a1 1 0 0 1 1.22.976z'/>",
        ),
        "home" => String::from(
            "<path d='M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8'/>\
             <path d='M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z'/>",
        ),
        "file-new" => String::from(
            "<path d='M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z'/>\
             <path d='M14 2v4a2 2 0 0 0 2 2h4'/><path d='M9 15h6'/><path d='M12 18v-6'/>",
        ),
        "folder-open" => String::from(
            "<path d='m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2'/>",
        ),
        "save-mono" => String::from(
            "<path d='M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z'/>\
             <path d='M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7'/>\
             <path d='M7 3v4a1 1 0 0 0 1 1h7'/>",
        ),
        "save-as" => String::from(
            "<path d='M10 2v3a1 1 0 0 0 1 1h5'/>\
             <path d='M18 18v-6a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6'/>\
             <path d='M18 22H4a2 2 0 0 1-2-2V6'/>\
             <path d='M8 18a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10l4 4v12a2 2 0 0 1-2 2Z'/>",
        ),
        "printer" => String::from(
            "<path d='M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2'/>\
             <path d='M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6'/>\
             <rect x='6' y='14' width='12' height='8' rx='1'/>",
        ),
        "file-output" => String::from(
            "<path d='M14 2v4a2 2 0 0 0 2 2h4'/>\
             <path d='M4 7V4a2 2 0 0 1 2-2h9l5 5v13a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-3'/>\
             <path d='M2 15h10'/><path d='m9 18 3-3-3-3'/>",
        ),
        // The mirror of file-output, so Import and Export read as one pair.
        "file-input" => String::from(
            "<path d='M4 22h14a2 2 0 0 0 2-2V7l-5-5H6a2 2 0 0 0-2 2v4'/>\
             <path d='M14 2v4a2 2 0 0 0 2 2h4'/>\
             <path d='M2 15h10'/><path d='m9 18 3-3-3-3'/>",
        ),
        "info-circle" => String::from(
            "<circle cx='12' cy='12' r='10'/><path d='M12 16v-4'/><path d='M12 8h.01'/>",
        ),
        // About: a badge rather than a plain circle, so it does not read as the
        // same thing as Info sitting a few rows above it.
        "badge-info" => String::from(
            "<path d='M3.85 8.62a4 4 0 0 1 4.78-4.77 4 4 0 0 1 6.74 0 4 4 0 0 1 4.78 4.78 4 4 0 0 1 0 6.74 4 4 0 0 1-4.77 4.78 4 4 0 0 1-6.75 0 4 4 0 0 1-4.78-4.77 4 4 0 0 1 0-6.76Z'/>\
             <path d='M12 16v-4'/><path d='M12 8h.01'/>",
        ),
        "package-mono" => String::from(
            "<path d='m7.5 4.27 9 5.15'/>\
             <path d='M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z'/>\
             <path d='m3.3 7 8.7 5 8.7-5'/><path d='M12 22V12'/>",
        ),
        // lucide: file-up
        _ => String::from(
            "<path d='M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z' /><path d='M14 2v4a2 2 0 0 0 2 2h4' /><path d='M9 13h6' /><path d='M9 17h4' />",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon the interface asks for, gathered from the source itself so a
    /// name added later is covered without anyone updating this list.
    fn referenced() -> Vec<String> {
        let mut names = Vec::new();
        for file in [
            include_str!("ribbon.rs"),
            include_str!("grid.rs"),
            include_str!("views.rs"),
            include_str!("dialogs.rs"),
            include_str!("backstage.rs"),
            include_str!("contextmenu.rs"),
            include_str!("popups.rs"),
        ] {
            for chunk in file.split("icon(\"").skip(1) {
                if let Some(name) = chunk.split('"').next() {
                    names.push(name.to_string());
                }
            }
            // Ribbon buttons name their glyph rather than calling icon().
            for chunk in file.split("glyph: \"").skip(1) {
                if let Some(name) = chunk.split('"').next() {
                    names.push(name.to_string());
                }
            }
        }
        // Only things shaped like an icon name. A `glyph:` field is also used
        // for a literal character elsewhere, and that is not an icon.
        names.retain(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn every_icon_the_interface_asks_for_actually_exists() {
        // An unknown name falls back to a plain document, so a missing icon
        // compiles and ships as a wrong picture rather than an error. This is
        // the only thing that catches it.
        let fallback = body_for("definitely-not-an-icon-name");
        let mut wrong = Vec::new();
        for name in referenced() {
            if name.is_empty() {
                continue;
            }
            if body_for(&name) == fallback {
                wrong.push(name);
            }
        }
        assert!(wrong.is_empty(), "these fall back to a blank document: {wrong:?}");
    }

    #[test]
    fn the_fallback_is_still_a_shape_rather_than_nothing() {
        // A typo should show as a plain document, not an empty button.
        assert!(body_for("nonsense").contains("<path"));
    }

    #[test]
    fn a_tint_is_only_given_where_it_means_something() {
        // Colour by meaning, so a family of commands reads as one. Everything
        // else has to inherit, or a glyph in a menu row fights its label.
        assert_eq!(tint_for("cut"), "var(--danger)");
        assert_eq!(tint_for("team-planner"), "var(--accent)");
        assert_eq!(tint_for("copy"), "currentColor");
    }
}
