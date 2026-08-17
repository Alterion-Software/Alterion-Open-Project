//! Ribbon iconography.
//!
//! Each icon is raw SVG on a 24x24 grid, drawn in line art with accent fills
//! the way Office glyphs are. Single quotes are used inside the markup so the
//! Rust literals stay free of escapes.

use dioxus::prelude::*;

/// Render a named icon. Unknown names fall back to a neutral document glyph so
/// a typo shows up as a plain shape rather than an empty button.
pub fn icon(name: &str, size: u32) -> Element {
    let body = body_for(name);
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
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

const BLUE: &str = "#2f7bd0";
const RED: &str = "#d13438";
const GREEN: &str = "#31752f";
const GOLD: &str = "#c9930a";
const PURPLE: &str = "#7b4f9d";
const GREY: &str = "#605e5c";

fn body_for(name: &str) -> String {
    match name {
        // ---- views -----------------------------------------------------
        "gantt" => format!(
            "<rect x='3' y='5' width='7' height='3' rx='1' fill='{BLUE}' stroke='none'/>\
             <rect x='7' y='10.5' width='9' height='3' rx='1' fill='{BLUE}' stroke='none'/>\
             <rect x='12' y='16' width='8' height='3' rx='1' fill='{BLUE}' stroke='none'/>\
             <path d='M3 3v18' stroke='{GREY}'/>"
        ),
        "tracking-gantt" => format!(
            "<rect x='3' y='5' width='10' height='3' rx='1' fill='{BLUE}' stroke='none'/>\
             <rect x='3' y='5' width='6' height='3' rx='1' fill='{GREEN}' stroke='none'/>\
             <rect x='8' y='11' width='11' height='3' rx='1' fill='{BLUE}' stroke='none'/>\
             <rect x='8' y='11' width='4' height='3' rx='1' fill='{GREEN}' stroke='none'/>\
             <path d='M3 3v18' stroke='{GREY}'/>"
        ),
        "task-sheet" => format!(
            "<rect x='3' y='4' width='18' height='16' rx='1'/>\
             <path d='M3 9h18M3 14h18M9 4v16' stroke='{GREY}'/>"
        ),
        "task-usage" => format!(
            "<rect x='3' y='4' width='18' height='16' rx='1'/>\
             <path d='M3 9h18M11 4v16' stroke='{GREY}'/>\
             <rect x='13' y='11' width='2.5' height='6' fill='{BLUE}' stroke='none'/>\
             <rect x='17' y='13' width='2.5' height='4' fill='{BLUE}' stroke='none'/>"
        ),
        "network" => format!(
            "<rect x='2' y='4' width='7' height='5' rx='1' stroke='{BLUE}'/>\
             <rect x='15' y='4' width='7' height='5' rx='1'/>\
             <rect x='8.5' y='15' width='7' height='5' rx='1' stroke='{RED}'/>\
             <path d='M9 6.5h6M5.5 9v3.5h6.5V15M18.5 9v3.5H12'/>"
        ),
        "calendar" => format!(
            "<rect x='3' y='5' width='18' height='16' rx='1.5'/>\
             <path d='M3 10h18M8 3v4M16 3v4' stroke='{GREY}'/>\
             <rect x='6' y='13' width='3' height='3' fill='{BLUE}' stroke='none'/>"
        ),
        "team-planner" => format!(
            "<circle cx='6' cy='7' r='2.4' stroke='{BLUE}'/>\
             <circle cx='6' cy='16' r='2.4' stroke='{GREEN}'/>\
             <rect x='11' y='5' width='9' height='4' rx='1' fill='{BLUE}' stroke='none'/>\
             <rect x='11' y='14' width='6' height='4' rx='1' fill='{GREEN}' stroke='none'/>"
        ),
        "resource-sheet" => format!(
            "<rect x='3' y='4' width='18' height='16' rx='1'/>\
             <path d='M3 9h18' stroke='{GREY}'/>\
             <circle cx='7.5' cy='14' r='2' stroke='{BLUE}'/>\
             <path d='M12 13h7M12 16h5' stroke='{GREY}'/>"
        ),
        "resource-usage" => format!(
            "<circle cx='6' cy='7' r='2.4' stroke='{BLUE}'/>\
             <path d='M2.5 20c0-2.6 1.6-4.2 3.5-4.2s3.5 1.6 3.5 4.2' stroke='{BLUE}'/>\
             <rect x='13' y='5' width='2.5' height='14' fill='{GREY}' stroke='none' opacity='0.35'/>\
             <rect x='17.5' y='9' width='2.5' height='10' fill='{BLUE}' stroke='none'/>"
        ),
        "other-views" => format!("<rect x='3' y='4' width='18' height='16' rx='1'/><path d='M7 9h10M7 13h10M7 17h6' stroke='{GREY}'/>"),

        // ---- clipboard -------------------------------------------------
        "paste" => format!(
            "<rect x='6' y='4' width='12' height='17' rx='1.5'/>\
             <rect x='9' y='2.5' width='6' height='3.5' rx='1' fill='{GOLD}' stroke='none'/>\
             <path d='M9 12h6M9 16h4' stroke='{GREY}'/>"
        ),
        "cut" => format!("<circle cx='6.5' cy='18' r='2.4'/><circle cx='17.5' cy='18' r='2.4'/><path d='M8.4 16.2L18 4M15.6 16.2L6 4' stroke='{GREY}'/>"),
        "copy" => format!("<rect x='3.5' y='3.5' width='12' height='14' rx='1.5' stroke='{GREY}'/><rect x='8.5' y='7.5' width='12' height='14' rx='1.5'/>"),
        "format-painter" => format!("<rect x='5' y='3' width='12' height='6' rx='1' fill='{GOLD}' stroke='none'/><path d='M11 9v4M9.5 13h3v8h-3z'/>"),

        // ---- font ------------------------------------------------------
        "bold" => "<path d='M7 4h6a4 4 0 010 8H7zM7 12h7a4 4 0 010 8H7z' stroke-width='1.7'/>".into(),
        "italic" => "<path d='M10 4h7M7 20h7M14.5 4l-5 16' stroke-width='1.7'/>".into(),
        "underline" => "<path d='M7 3v7a5 5 0 0010 0V3M5 21h14' stroke-width='1.6'/>".into(),
        "font-color" => format!("<path d='M6 16L12 4l6 12M8.4 12h7.2'/><rect x='4' y='19' width='16' height='3' rx='1' fill='{RED}' stroke='none'/>"),
        // Lucide's paint-bucket outline (ISC), with the tipped bucket filled
        // so it reads as paint rather than an empty vessel, and the falling
        // drop picked out in the accent colour.
        "fill-color" => format!(
            "<path d='m8.5 4.5 2.148-2.148a1.205 1.205 0 0 1 1.704 0l7.296 7.296a1.205 1.205 0 0 1 0 1.704l-7.592 7.592a3.615 3.615 0 0 1-5.112 0l-3.888-3.888a3.615 3.615 0 0 1 0-5.112L5.67 7.33' \
              fill='{GOLD}' fill-opacity='0.20' stroke='{GREY}'/>\
             <path d='M11 7 6 2' stroke='{GREY}'/>\
             <path d='M18.992 12H2.041' stroke='{GREY}'/>\
             <path d='M21.145 18.38A3.34 3.34 0 0 1 20 16.5a3.3 3.3 0 0 1-1.145 1.88c-.575.46-.855 1.02-.855 1.595A2 2 0 0 0 20 22a2 2 0 0 0 2-2.025c0-.58-.285-1.13-.855-1.595' \
              fill='{GOLD}' stroke='{GOLD}' stroke-width='1.1'/>"
        ),

        // ---- schedule --------------------------------------------------
        "link" => format!("<path d='M10 14a4 4 0 010-5.7l2.1-2.1a4 4 0 015.7 5.7L16.6 13' stroke='{BLUE}'/><path d='M14 10a4 4 0 010 5.7l-2.1 2.1a4 4 0 01-5.7-5.7L7.4 11' stroke='{BLUE}'/>"),
        "unlink" => format!("<path d='M10.5 13.5L8 16a4 4 0 01-5.6-5.6L4.9 8' stroke='{GREY}'/><path d='M13.5 10.5L16 8a4 4 0 015.6 5.6L19.1 16' stroke='{GREY}'/><path d='M3 3l18 18' stroke='{RED}'/>"),
        "mark-on-track" => format!("<circle cx='12' cy='12' r='9' stroke='{GREEN}'/><path d='M7.5 12.3l3.2 3.2 6-6.4' stroke='{GREEN}' stroke-width='1.8'/>"),
        "respect-links" => format!("<path d='M4 7h7v5h9' stroke='{BLUE}'/><path d='M17 9l3 3-3 3' stroke='{BLUE}'/><rect x='2' y='4.5' width='4' height='5' rx='1' fill='{BLUE}' stroke='none'/>"),
        "inactivate" => format!("<rect x='3' y='9' width='18' height='6' rx='1' stroke='{GREY}'/><path d='M4 20L20 4' stroke='{RED}' stroke-width='1.7'/>"),
        "percent" => format!("<path d='M5 19L19 5' stroke='{GREY}'/><circle cx='7.5' cy='7.5' r='2.6' stroke='{BLUE}'/><circle cx='16.5' cy='16.5' r='2.6' stroke='{BLUE}'/>"),

        // ---- task modes ------------------------------------------------
        "manual-schedule" => format!(
            "<rect x='2' y='11.5' width='11' height='4.4' rx='1.6' fill='{BLUE}' stroke='none' opacity='0.6'/>\
             <rect x='7.5' y='18' width='11' height='4.4' rx='1.6' fill='{BLUE}' stroke='none' opacity='0.6'/>\
             <g transform='translate(10.4 -1.2) scale(0.6)'>\
               <path d='M12 17v5' stroke='{RED}' stroke-width='2.2'/>\
               <path d='M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z' \
                 fill='{RED}' fill-opacity='0.18' stroke='{RED}' stroke-width='2.2'/>\
             </g>"
        ),
        "auto-schedule" => format!(
            "<rect x='2' y='11.5' width='11' height='4.4' rx='1.6' fill='{BLUE}' stroke='none'/>\
             <rect x='7.5' y='18' width='11' height='4.4' rx='1.6' fill='{BLUE}' stroke='none' opacity='0.6'/>\
             <path d='M5.6 15.9v2.1h3.6' stroke='{GREY}' stroke-width='1.2'/>\
             <g transform='translate(11.2 -0.6) scale(0.56)'>\
               <path d='M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8' stroke='{GREEN}' stroke-width='2.4'/>\
               <path d='M21 3v5h-5' stroke='{GREEN}' stroke-width='2.4'/>\
               <path d='M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16' stroke='{GREEN}' stroke-width='2.4'/>\
               <path d='M8 16H3v5' stroke='{GREEN}' stroke-width='2.4'/>\
             </g>"
        ),
        "inspect" => format!("<circle cx='10.5' cy='10.5' r='6' stroke='{BLUE}'/><path d='M15 15l5 5' stroke='{GREY}'/><path d='M10.5 7.5v4M10.5 13.6v.4' stroke='{BLUE}'/>"),
        "move" => format!("<path d='M12 3v18M3 12h18' stroke='{GREY}'/><path d='M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3' stroke='{BLUE}'/>"),
        "mode" => format!("<rect x='3' y='5' width='18' height='4' rx='1' stroke='{BLUE}'/><rect x='3' y='13' width='18' height='4' rx='1' stroke='{GREY}'/>"),

        // ---- insert ----------------------------------------------------
        "task-add" => format!("<rect x='3' y='6' width='12' height='4' rx='1' fill='{BLUE}' stroke='none'/><rect x='3' y='13' width='8' height='4' rx='1' fill='{BLUE}' stroke='none' opacity='0.55'/><path d='M18 12v8M14 16h8' stroke='{GREEN}' stroke-width='1.9'/>"),
        "summary" => format!("<path d='M3 6h18l-2 4H5z' fill='{GREY}' stroke='none'/><rect x='7' y='13' width='7' height='3' rx='1' fill='{BLUE}' stroke='none'/><rect x='15' y='17' width='5' height='3' rx='1' fill='{BLUE}' stroke='none'/>"),
        "milestone" => format!("<path d='M12 4l6 8-6 8-6-8z' fill='{PURPLE}' stroke='none'/>"),
        "deliverable" => format!("<path d='M4 8l8-4 8 4-8 4z' stroke='{BLUE}'/><path d='M4 8v8l8 4 8-4V8' stroke='{GREY}'/>"),

        // ---- properties ------------------------------------------------
        "information" => format!("<circle cx='12' cy='12' r='9' stroke='{BLUE}'/><path d='M12 11v6' stroke='{BLUE}' stroke-width='1.8'/><circle cx='12' cy='7.6' r='1.1' fill='{BLUE}' stroke='none'/>"),
        "notes" => format!("<rect x='4' y='3' width='16' height='18' rx='1.5'/><path d='M8 8h8M8 12h8M8 16h5' stroke='{GREY}'/>"),
        "details" => format!("<rect x='3' y='4' width='18' height='7' rx='1'/><rect x='3' y='13' width='18' height='7' rx='1' stroke='{BLUE}'/>"),
        "add-to-timeline" => format!("<path d='M3 12h18' stroke='{GREY}'/><rect x='6' y='8' width='7' height='4' rx='1' fill='{BLUE}' stroke='none'/><path d='M18 15v5M15.5 17.5h5' stroke='{GREEN}' stroke-width='1.8'/>"),

        // ---- editing ---------------------------------------------------
        "scroll-to-task" => format!("<rect x='3' y='9' width='9' height='5' rx='1' fill='{BLUE}' stroke='none'/><path d='M15 11.5h6M18.5 8.5l3 3-3 3' stroke='{GREY}'/>"),
        "find" => format!("<circle cx='10.5' cy='10.5' r='6.5' stroke='{BLUE}'/><path d='M15.5 15.5l5 5' stroke='{GREY}'/>"),
        "clear" => format!("<path d='M6 6l12 12M18 6L6 18' stroke='{RED}' stroke-width='1.7'/>"),
        "fill-down" => format!("<rect x='5' y='3' width='14' height='5' rx='1' fill='{BLUE}' stroke='none'/><path d='M12 10v9M8.5 15.5L12 19l3.5-3.5' stroke='{GREY}'/>"),

        // ---- resources -------------------------------------------------
        "assign-resources" => format!("<circle cx='8' cy='8' r='3' stroke='{BLUE}'/><path d='M2.5 19c0-3.3 2.4-5.5 5.5-5.5s5.5 2.2 5.5 5.5' stroke='{BLUE}'/><path d='M18 8v8M14 12h8' stroke='{GREEN}' stroke-width='1.8'/>"),
        "resource-pool" => format!("<circle cx='7' cy='8' r='2.6' stroke='{BLUE}'/><circle cx='15' cy='8' r='2.6' stroke='{GREEN}'/><path d='M2.5 18c0-2.6 2-4.4 4.5-4.4s4.5 1.8 4.5 4.4M12.5 18c0-2.6 2-4.4 4.5-4.4s4.5 1.8 4.5 4.4' stroke='{GREY}'/>"),
        "substitute" => format!("<circle cx='7' cy='9' r='3' stroke='{GREY}'/><circle cx='17' cy='15' r='3' stroke='{BLUE}'/><path d='M11 7h7l-2-2M13 17H6l2 2' stroke='{GREEN}'/>"),
        "add-resource" => format!("<circle cx='10' cy='8' r='3.2' stroke='{BLUE}'/><path d='M4 19c0-3.3 2.7-5.6 6-5.6 1.3 0 2.5.35 3.5 1' stroke='{BLUE}'/><path d='M18 13v7M14.5 16.5h7' stroke='{GREEN}' stroke-width='1.8'/>"),
        "level" => format!("<rect x='3' y='14' width='4' height='6' fill='{BLUE}' stroke='none'/><rect x='10' y='6' width='4' height='14' fill='{RED}' stroke='none'/><rect x='17' y='11' width='4' height='9' fill='{BLUE}' stroke='none'/><path d='M2 10h20' stroke='{GREY}' stroke-dasharray='2 2'/>"),
        "level-options" => format!("<circle cx='12' cy='12' r='3.2' stroke='{BLUE}'/><path d='M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.5 5.5l2 2M16.5 16.5l2 2M18.5 5.5l-2 2M5.5 18.5l2-2' stroke='{GREY}'/>"),
        "next-over" => format!("<path d='M12 4v11M8 11l4 4 4-4' stroke='{RED}' stroke-width='1.7'/><path d='M5 19h14' stroke='{GREY}'/>"),

        // ---- reports ---------------------------------------------------
        "compare" => format!("<rect x='2.5' y='4' width='8' height='16' rx='1' stroke='{BLUE}'/><rect x='13.5' y='4' width='8' height='16' rx='1' stroke='{GREEN}'/><path d='M11 12h2' stroke='{GREY}'/>"),
        "dashboard" => format!("<rect x='3' y='3' width='8' height='7' rx='1' fill='{BLUE}' stroke='none'/><rect x='13' y='3' width='8' height='11' rx='1' stroke='{GREY}'/><rect x='3' y='12' width='8' height='9' rx='1' stroke='{GREY}'/><rect x='13' y='16' width='8' height='5' rx='1' fill='{GREEN}' stroke='none'/>"),
        "report-costs" => format!("<circle cx='12' cy='12' r='8.5' stroke='{GREY}'/><path d='M12 7v10M9.6 9.2h4a1.9 1.9 0 010 3.8h-3.6a1.9 1.9 0 000 3.8h4' stroke='{GREEN}'/>"),
        "report-progress" => format!("<circle cx='12' cy='12' r='8.5' stroke='{GREY}'/><path d='M12 3.5a8.5 8.5 0 018.5 8.5' stroke='{BLUE}' stroke-width='2.4'/><path d='M12 8v4l3 2' stroke='{GREY}'/>"),
        "report-custom" => format!("<rect x='3' y='4' width='18' height='16' rx='1'/><rect x='6' y='13' width='3' height='4' fill='{BLUE}' stroke='none'/><rect x='11' y='9' width='3' height='8' fill='{GREEN}' stroke='none'/><rect x='16' y='11' width='3' height='6' fill='{GOLD}' stroke='none'/>"),
        "visual-report" => format!("<path d='M12 12V3.5A8.5 8.5 0 1112 12z' fill='{BLUE}' stroke='none' opacity='0.85'/><circle cx='12' cy='12' r='8.5' stroke='{GREY}'/>"),

        // ---- project ---------------------------------------------------
        "subproject" => format!("<rect x='3' y='4' width='11' height='9' rx='1' stroke='{GREY}'/><rect x='10' y='11' width='11' height='9' rx='1' stroke='{BLUE}'/>"),
        "apps" => format!("<rect x='3' y='3' width='7' height='7' rx='1' fill='{BLUE}' stroke='none'/><rect x='14' y='3' width='7' height='7' rx='1' fill='{GREEN}' stroke='none'/><rect x='3' y='14' width='7' height='7' rx='1' fill='{GOLD}' stroke='none'/><rect x='14' y='14' width='7' height='7' rx='1' stroke='{GREY}'/>"),
        "project-info" => format!("<rect x='3' y='4' width='18' height='16' rx='1.5'/><path d='M3 9h18' stroke='{GREY}'/><path d='M7 13h5M7 16h9' stroke='{BLUE}'/>"),
        "custom-fields" => format!("<rect x='3' y='5' width='18' height='5' rx='1' stroke='{BLUE}'/><rect x='3' y='14' width='18' height='5' rx='1' stroke='{GREY}'/><path d='M6 7.5h3M6 16.5h3' stroke='{GREY}'/>"),
        "links-between" => format!("<rect x='2' y='7' width='7' height='10' rx='1' stroke='{BLUE}'/><rect x='15' y='7' width='7' height='10' rx='1' stroke='{GREEN}'/><path d='M9 12h6M13 10l2 2-2 2' stroke='{GREY}'/>"),
        "wbs" => format!("<rect x='9' y='2.5' width='6' height='4' rx='1' stroke='{BLUE}'/><rect x='2.5' y='16' width='6' height='4' rx='1' stroke='{GREY}'/><rect x='15.5' y='16' width='6' height='4' rx='1' stroke='{GREY}'/><path d='M12 6.5v5M5.5 16v-4.5h13V16' stroke='{GREY}'/>"),
        "working-time" => format!("<rect x='3' y='5' width='18' height='16' rx='1.5'/><path d='M3 10h18M8 3v4M16 3v4' stroke='{GREY}'/><circle cx='16' cy='16' r='3.4' stroke='{BLUE}'/><path d='M16 14.2V16l1.3 1' stroke='{BLUE}'/>"),
        "calculate" => format!("<rect x='4' y='2.5' width='16' height='19' rx='1.5'/><path d='M7 6.5h10' stroke='{GREY}'/><path d='M8 12h2M14 12h2M8 16.5h2M14 16.5h2' stroke='{BLUE}' stroke-width='1.8'/>"),
        "baseline" => format!("<rect x='3' y='6' width='13' height='3.5' rx='1' fill='{BLUE}' stroke='none'/><rect x='3' y='11' width='13' height='2.2' rx='1' fill='{GREY}' stroke='none' opacity='0.6'/><rect x='7' y='16' width='11' height='3.5' rx='1' fill='{BLUE}' stroke='none'/>"),
        "move-project" => format!("<rect x='3' y='7' width='11' height='4' rx='1' fill='{BLUE}' stroke='none' opacity='0.4'/><rect x='9' y='13' width='11' height='4' rx='1' fill='{BLUE}' stroke='none'/><path d='M5 19l3-3-3-3' stroke='{GREEN}'/>"),
        "status-date" => format!("<rect x='3' y='5' width='18' height='16' rx='1.5'/><path d='M3 10h18M8 3v4M16 3v4' stroke='{GREY}'/><path d='M12 12v8' stroke='{RED}' stroke-width='1.8' stroke-dasharray='2.5 2'/>"),
        "update-project" => format!("<path d='M20 12a8 8 0 11-2.6-5.9' stroke='{BLUE}' stroke-width='1.6'/><path d='M20 3.5V8h-4.5' stroke='{BLUE}' stroke-width='1.6'/>"),
        "sync" => format!("<path d='M4.5 10a7.5 7.5 0 0112.6-3.3' stroke='{GREEN}'/><path d='M19.5 14a7.5 7.5 0 01-12.6 3.3' stroke='{GREEN}'/><path d='M17.5 3v4h-4M6.5 21v-4h4' stroke='{GREY}'/>"),
        "spelling" => format!("<path d='M4 17L9 6l5 11M5.8 13.5h6.4' stroke='{GREY}'/><path d='M15 15.5l2.6 2.6L22 12' stroke='{GREEN}' stroke-width='1.9'/>"),

        // ---- view tab --------------------------------------------------
        "sort" => format!("<path d='M4 6h13M4 11h9M4 16h5' stroke='{GREY}'/><path d='M18 8v11M15.5 16.5L18 19l2.5-2.5' stroke='{BLUE}'/>"),
        "outline" => format!("<path d='M4 5h16M8 10h12M12 15h8M12 20h8' stroke='{GREY}'/><path d='M5 9.5h2.5v2.5' stroke='{BLUE}'/>"),
        "tables" => format!("<rect x='3' y='4' width='18' height='16' rx='1'/><path d='M3 9h18M3 14.5h18M10 4v16' stroke='{GREY}'/>"),
        "highlight" => format!("<path d='M6 14l6-9 5.5 3.5-6 9z' fill='{GOLD}' stroke='none' opacity='0.85'/><path d='M5 19h9' stroke='{GREY}' stroke-width='1.8'/>"),
        "filter" => format!("<path d='M3.5 5h17l-6.5 7.5V20l-4-2.5v-5z' stroke='{BLUE}'/>"),
        "group-by" => format!("<rect x='3' y='4' width='18' height='4' rx='1' fill='{BLUE}' stroke='none' opacity='0.75'/><rect x='6' y='10' width='15' height='3' rx='1' stroke='{GREY}'/><rect x='6' y='15.5' width='15' height='3' rx='1' stroke='{GREY}'/>"),
        "timescale" => format!("<path d='M3 8h18' stroke='{GREY}'/><path d='M6 8v4M10 8v6M14 8v4M18 8v6' stroke='{BLUE}'/><path d='M3 18h18' stroke='{GREY}'/>"),
        "zoom-in" => format!("<circle cx='10.5' cy='10.5' r='6.5' stroke='{BLUE}'/><path d='M10.5 7.8v5.4M7.8 10.5h5.4' stroke='{BLUE}'/><path d='M15.5 15.5l5 5' stroke='{GREY}'/>"),
        "zoom-out" => format!("<circle cx='10.5' cy='10.5' r='6.5' stroke='{BLUE}'/><path d='M7.8 10.5h5.4' stroke='{BLUE}'/><path d='M15.5 15.5l5 5' stroke='{GREY}'/>"),
        "entire-project" => format!("<rect x='2.5' y='6' width='19' height='12' rx='1' stroke='{GREY}'/><path d='M6 12h12M8 9.5L5.5 12 8 14.5M16 9.5l2.5 2.5-2.5 2.5' stroke='{BLUE}'/>"),
        "selected-tasks" => format!("<rect x='3' y='7' width='9' height='4' rx='1' fill='{BLUE}' stroke='none'/><rect x='3' y='13' width='6' height='4' rx='1' stroke='{GREY}'/><path d='M15 9.5l2.5 2.5L15 14.5M20 8v8' stroke='{GREEN}'/>"),
        "timeline-band" => format!("<path d='M2.5 12h19' stroke='{GREY}'/><circle cx='7' cy='12' r='1.8' fill='{BLUE}' stroke='none'/><circle cx='13' cy='12' r='1.8' fill='{GREEN}' stroke='none'/><circle cx='18.5' cy='12' r='1.8' fill='{GOLD}' stroke='none'/>"),
        "details-pane" => format!("<rect x='3' y='4' width='18' height='16' rx='1'/><path d='M3 13h18' stroke='{BLUE}' stroke-width='1.7'/>"),
        "new-window" => format!("<rect x='3' y='5' width='13' height='11' rx='1' stroke='{GREY}'/><rect x='8' y='9' width='13' height='11' rx='1' stroke='{BLUE}'/>"),
        "arrange-all" => format!("<rect x='3' y='4' width='8' height='7' rx='1' stroke='{BLUE}'/><rect x='13' y='4' width='8' height='7' rx='1' stroke='{GREY}'/><rect x='3' y='13' width='8' height='7' rx='1' stroke='{GREY}'/><rect x='13' y='13' width='8' height='7' rx='1' stroke='{GREY}'/>"),
        "hide" => format!("<path d='M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z' stroke='{GREY}'/><path d='M4 20L20 4' stroke='{RED}'/>"),
        "switch-windows" => format!("<rect x='3' y='4' width='12' height='9' rx='1' stroke='{GREY}'/><rect x='9' y='11' width='12' height='9' rx='1' stroke='{BLUE}'/>"),
        "macros" => format!("<path d='M4 5h6l-3 7 3 7H4' stroke='{BLUE}' stroke-width='1.6'/><path d='M20 5h-6l3 7-3 7h6' stroke='{BLUE}' stroke-width='1.6'/>"),

        // ---- format tab ------------------------------------------------
        "text-styles" => format!("<path d='M4 6.5V5h10v1.5M9 5v14M7 19h4' stroke='{GREY}'/><path d='M15 12v-1h6v1M18 11v8M16.5 19h3' stroke='{BLUE}'/>"),
        "gridlines" => format!("<rect x='3' y='4' width='18' height='16' rx='1' stroke='{GREY}'/><path d='M3 9.3h18M3 14.6h18M9 4v16M15 4v16' stroke='{BLUE}' stroke-dasharray='2 2'/>"),
        "layout" => format!("<rect x='3' y='5' width='7' height='4' rx='1' fill='{BLUE}' stroke='none'/><rect x='10' y='12' width='9' height='4' rx='1' fill='{BLUE}' stroke='none'/><path d='M6.5 9v5.5h3.5' stroke='{GREY}'/>"),
        "insert-column" => format!("<rect x='3' y='4' width='6' height='16' rx='1' stroke='{GREY}'/><rect x='11' y='4' width='6' height='16' rx='1' stroke='{GREY}'/><path d='M20 8v8M16.5 12H23' stroke='{GREEN}' stroke-width='1.8'/>"),
        "column-settings" => format!("<rect x='4' y='4' width='7' height='16' rx='1' stroke='{GREY}'/><rect x='13' y='4' width='7' height='16' rx='1' stroke='{BLUE}'/><path d='M15 8h3M15 11h3' stroke='{BLUE}'/>"),
        "critical-tasks" => format!("<rect x='3' y='6' width='11' height='4' rx='1' fill='{RED}' stroke='none'/><rect x='9' y='14' width='11' height='4' rx='1' fill='{RED}' stroke='none'/><path d='M14 8h-1v6h-4' stroke='{GREY}'/>"),
        "slack" => format!("<rect x='3' y='9' width='9' height='5' rx='1' fill='{BLUE}' stroke='none'/><path d='M12.5 11.5h7.5' stroke='{GREY}' stroke-dasharray='2 2'/><path d='M20 9v5' stroke='{GREY}'/>"),
        "drawing" => format!("<path d='M4 20l1.5-4.5L16 5a2.1 2.1 0 013 3L8.5 18.5z' stroke='{GREY}'/><path d='M14.5 6.5l3 3' stroke='{BLUE}'/>"),
        "outline-number" => format!("<path d='M4 6h1.5v5M4 11h3' stroke='{BLUE}'/><path d='M10 6h10M10 11h10M10 16h10' stroke='{GREY}'/><path d='M4 14.5h3v2.5H4v2.5h3' stroke='{BLUE}'/>"),
        "project-summary" => format!("<path d='M2.5 5h19l-2 4H4.5z' fill='{GREY}' stroke='none'/><rect x='6' y='12' width='8' height='3.5' rx='1' fill='{BLUE}' stroke='none'/><rect x='11' y='17' width='8' height='3.5' rx='1' fill='{BLUE}' stroke='none'/>"),

        // ---- help ------------------------------------------------------
        "help" => format!("<circle cx='12' cy='12' r='9' stroke='{BLUE}'/><path d='M9.4 9.2a2.7 2.7 0 015.2.9c0 1.8-2.6 2.2-2.6 4' stroke='{BLUE}'/><circle cx='12' cy='17.2' r='1.1' fill='{BLUE}' stroke='none'/>"),
        "support" => format!("<path d='M4 12a8 8 0 0116 0v4a3 3 0 01-3 3h-2' stroke='{BLUE}'/><rect x='2.5' y='11.5' width='4' height='6' rx='1.4' stroke='{GREY}'/><rect x='17.5' y='11.5' width='4' height='6' rx='1.4' stroke='{GREY}'/>"),
        "feedback" => format!("<path d='M4 4h16v11H9l-5 4z' stroke='{BLUE}'/><path d='M8.5 9.5h7' stroke='{GREY}'/>"),
        "training" => format!("<path d='M2.5 8.5L12 4l9.5 4.5L12 13z' stroke='{BLUE}'/><path d='M6.5 10.6V16c0 1.4 2.5 2.6 5.5 2.6s5.5-1.2 5.5-2.6v-5.4' stroke='{GREY}'/>"),
        "whats-new" => format!("<path d='M12 3l2.4 5.2 5.6.7-4.1 3.9 1.1 5.6L12 15.7 6.9 18.4 8 12.8 4 8.9l5.6-.7z' fill='{GOLD}' stroke='none'/>"),

        // ---- Lucide outlines (ISC) --------------------------------------
        // Chrome rather than a ribbon command, so it takes the colour of the
        // text it sits with instead of the Office palette the ribbon uses.
        "scale" => String::from(
            "<path d='M12 3v18'/>\
             <path d='m19 8 3 8a5 5 0 0 1-6 0zV7'/>\
             <path d='M3 7h1a17 17 0 0 0 8-2 17 17 0 0 0 8 2h1'/>\
             <path d='m5 8 3 8a5 5 0 0 1-6 0zV7'/>\
             <path d='M7 21h10'/>",
        ),
        "package" => format!(
            "<path d='M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z' stroke='{BLUE}'/>\
             <path d='M12 22V12' stroke='{GREY}'/>\
             <polyline points='3.29 7 12 12 20.71 7' stroke='{GREY}'/>\
             <path d='m7.5 4.27 9 5.15' stroke='{GREY}'/>"
        ),
        "info" => format!(
            "<circle cx='12' cy='12' r='10' stroke='{BLUE}'/>\
             <path d='M12 16v-4' stroke='{BLUE}'/>\
             <path d='M12 8h.01' stroke='{BLUE}'/>"
        ),

        // Column headings that are a symbol rather than a word.
        "col-indicators" => format!(
            "<path d='M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z' \
               fill='{GOLD}' fill-opacity='0.22' stroke='{GOLD}'/>\
             <line x1='4' y1='22' x2='4' y2='15' stroke='{GOLD}'/>"
        ),
        "col-mode" => format!(
            "<rect x='2.5' y='4.5' width='13' height='11' rx='1.6' stroke='{GREY}'/>\
             <path d='M2.5 8h13M6 3v3M12 3v3' stroke='{GREY}'/>\
             <circle cx='17' cy='16' r='4.6' stroke='{BLUE}'/>\
             <path d='M17 13.6V16l1.7 1' stroke='{BLUE}'/>"
        ),

        // ---- internal pane layout --------------------------------------
        // Each shows the layout it produces, rather than a bare square.
        "layout-left" => format!(
            "<rect x='2.5' y='4.5' width='19' height='15' rx='1.5' stroke='{GREY}'/>\
             <rect x='2.5' y='4.5' width='11' height='15' rx='1.5' fill='{BLUE}' stroke='none' opacity='0.85'/>\
             <path d='M13.5 4.5v15' stroke='{GREY}'/>"
        ),
        "layout-right" => format!(
            "<rect x='2.5' y='4.5' width='19' height='15' rx='1.5' stroke='{GREY}'/>\
             <rect x='10.5' y='4.5' width='11' height='15' rx='1.5' fill='{BLUE}' stroke='none' opacity='0.85'/>\
             <path d='M10.5 4.5v15' stroke='{GREY}'/>"
        ),
        "layout-split" => format!(
            "<rect x='2.5' y='4.5' width='19' height='15' rx='1.5' stroke='{GREY}'/>\
             <path d='M12 4.5v15' stroke='{BLUE}' stroke-width='1.8'/>\
             <path d='M6.6 10.4L4.6 12l2 1.6M17.4 10.4l2 1.6-2 1.6' stroke='{GREY}'/>"
        ),

        // ---- window controls -------------------------------------------
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
        "settings" => String::from(
            "<path d='M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z'/>\
             <circle cx='12' cy='12' r='3'/>",
        ),
        "save" => "<path d='M4 4h12l4 4v12H4z'/><path d='M8 4v5h7V4M8 20v-6h8v6'/>".into(),
        "undo" => "<path d='M4 9h10a5.5 5.5 0 010 11h-5'/><path d='M8 4.5L3.5 9 8 13.5'/>".into(),
        "redo" => "<path d='M20 9H10a5.5 5.5 0 000 11h5'/><path d='M16 4.5L20.5 9 16 13.5'/>".into(),
        "new" => format!("<path d='M6 3h8l5 5v13H6z'/><path d='M14 3v5h5' stroke='{GREY}'/><path d='M12 11v7M8.5 14.5h7' stroke='{GREEN}'/>"),
        "open" => format!("<path d='M3 6.5h6l2 2.5h10V19H3z' stroke='{GOLD}'/>"),
        "print" => "<path d='M7 8V3.5h10V8'/><rect x='3.5' y='8' width='17' height='8' rx='1.5'/><path d='M7 14h10v6.5H7z'/>".into(),
        "export" => format!("<path d='M6 3h8l5 5v13H6z'/><path d='M14 3v5h5' stroke='{GREY}'/><path d='M12 18v-7M9 14l3-3 3 3' stroke='{GREEN}'/>"),
        "close-doc" => format!("<path d='M6 3h8l5 5v13H6z'/><path d='M14 3v5h5' stroke='{GREY}'/><path d='M9.5 13.5l5 5M14.5 13.5l-5 5' stroke='{RED}'/>"),
        "account" => format!("<circle cx='12' cy='8.5' r='3.8' stroke='{BLUE}'/><path d='M4.5 20.5c0-4 3.4-6.6 7.5-6.6s7.5 2.6 7.5 6.6' stroke='{BLUE}'/>"),
        "options" => format!("<circle cx='12' cy='12' r='3.4' stroke='{BLUE}'/><path d='M12 2.5v3.2M12 18.3v3.2M2.5 12h3.2M18.3 12h3.2M5.2 5.2l2.3 2.3M16.5 16.5l2.3 2.3M18.8 5.2l-2.3 2.3M5.2 18.8l2.3-2.3' stroke='{GREY}'/>"),
        "back" => "<path d='M20 12H4M10 6l-6 6 6 6' stroke-width='1.8'/>".into(),
        "search" => format!("<circle cx='11' cy='11' r='6.5' stroke='{GREY}'/><path d='M15.8 15.8l4.7 4.7' stroke='{GREY}'/>"),
        "warning" => format!("<path d='M12 3.5l9.5 17H2.5z' stroke='{RED}'/><path d='M12 9.5v5' stroke='{RED}' stroke-width='1.8'/><circle cx='12' cy='17.5' r='1' fill='{RED}' stroke='none'/>"),
        "folder" => format!("<path d='M3 6h6l2 2.5h10V19H3z' stroke='{GOLD}'/>"),

        _ => format!("<rect x='5' y='3' width='14' height='18' rx='1.5' stroke='{GREY}'/><path d='M8.5 8h7M8.5 12h7M8.5 16h4' stroke='{GREY}'/>"),
    }
}
