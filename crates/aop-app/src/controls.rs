//! Shared form controls.
//!
//! Native `<select>` elements are drawn by the webview itself, so they ignore
//! the theme and open an operating-system list. This dropdown replaces them:
//! the trigger is a styled button, and the list is a fixed-position popup
//! anchored to the click, which keeps it out of the ribbon's clipped scroller.

use dioxus::prelude::*;

#[derive(Clone, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

impl Choice {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    /// A choice whose stored value is the same as its label.
    pub fn plain(label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            value: label.clone(),
            label,
        }
    }
}

#[component]
pub fn Dropdown(
    /// The currently selected value, matched against `Choice::value`.
    value: String,
    options: Vec<Choice>,
    /// Trigger width in pixels; 0 means stretch to fill the row.
    width: f64,
    /// Taller trigger for dialog forms.
    large: bool,
    disabled: bool,
    on_pick: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut anchor = use_signal(|| (0.0f64, 0.0f64));

    let selected = options
        .iter()
        .find(|c| c.value == value)
        .map(|c| c.label.clone())
        .unwrap_or_else(|| value.clone());

    let mut class = String::from("dd");
    if large {
        class.push_str(" lg");
    }
    if disabled {
        class.push_str(" disabled");
    }
    let style = if width > 0.0 {
        format!("width: {width}px;")
    } else {
        "flex: 1;".to_string()
    };

    let list_width = if width > 0.0 { width.max(120.0) } else { 260.0 };

    // The list is handed to the layer at the root rather than rendered here.
    // Rendered here it is a child of the ribbon, which is ninety four pixels
    // tall and clips its contents so it can collapse to nothing, and no
    // amount of positioning gets a list out of a box that clips. At the root
    // its parent is the window, which is what `position: fixed` used to mean
    // and what `absolute` means to an engine with no `fixed`. See
    // `crate::floating`.
    let mut floating = use_context::<crate::floating::Layer>();
    let mine = use_hook(crate::floating::claim);
    {
        let options = options.clone();
        let value = value.clone();
        use_effect(move || {
            if !open() {
                floating.clear(mine);
                return;
            }
            let (ax, ay) = anchor();
            let options = options.clone();
            let value = value.clone();
            floating.put(
                mine,
                rsx! {
                    div {
                        class: "ctx-scrim",
                        onclick: move |event| {
                            event.stop_propagation();
                            open.set(false);
                        },
                        oncontextmenu: move |event| {
                            event.prevent_default();
                            open.set(false);
                        },
                    }
                    div {
                        class: "dd-list",
                        style: "left: {ax.max(4.0)}px; top: {ay.max(4.0)}px; width: {list_width}px;",
                        onclick: move |event| event.stop_propagation(),
                        for choice in options.iter() {
                            {
                                let picked = choice.value == value;
                                let item_class = if picked { "dd-item on" } else { "dd-item" };
                                let chosen = choice.value.clone();
                                rsx! {
                                    button {
                                        key: "{choice.value}",
                                        class: "{item_class}",
                                        onclick: move |event| {
                                            event.stop_propagation();
                                            open.set(false);
                                            on_pick.call(chosen.clone());
                                        },
                                        span { class: "tick", if picked { "\u{2713}" } }
                                        span { "{choice.label}" }
                                    }
                                }
                            }
                        }
                    }
                },
            );
        });
    }

    rsx! {
        button {
            class: "{class}",
            style: "{style}",
            title: "{selected}",
            disabled,
            onclick: move |event| {
                if disabled {
                    return;
                }
                let point = event.client_coordinates();
                // Drop the list just under the trigger the pointer landed on.
                let offset = if large { 22.0 } else { 16.0 };
                anchor.set((point.x - 8.0, point.y + offset));
                open.set(!open());
            },
            span { class: "dd-value", "{selected}" }
            span { class: "dd-caret", {crate::icons::icon("caret-down", 13)} }
        }
    }
}

/// A dropdown entry that carries an icon, for ribbon menus.
#[derive(Clone, PartialEq)]
pub struct MenuOption {
    pub glyph: String,
    pub label: String,
    pub value: String,
}

impl MenuOption {
    pub fn new(glyph: &str, label: &str, value: &str) -> Self {
        Self {
            glyph: glyph.to_string(),
            label: label.to_string(),
            value: value.to_string(),
        }
    }

    /// A separator line rather than a command.
    pub fn separator() -> Self {
        Self {
            glyph: String::new(),
            label: String::new(),
            value: "-".into(),
        }
    }

    fn is_separator(&self) -> bool {
        self.value == "-"
    }
}

/// A ribbon button that opens a menu instead of firing a single command.
#[component]
pub fn MenuBtn(
    glyph: String,
    caption: String,
    large: bool,
    enabled: bool,
    options: Vec<MenuOption>,
    on_pick: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut anchor = use_signal(|| (0.0f64, 0.0f64));

    let base = if large { "rbtn-lg" } else { "rbtn-sm" };
    let class = if enabled {
        base.to_string()
    } else {
        format!("{base} disabled")
    };
    let size = if large { 28 } else { 16 };

    // Handed to the layer at the root, not rendered here. Inside the ribbon
    // it is a child of a box that clips, which a list dropped from a ribbon
    // button has to escape, and so does the scrim behind it: clipped, the
    // scrim only covers the ribbon, so clicking anywhere else never reached it
    // and the menu would not close. See `crate::floating`.
    let mut floating = use_context::<crate::floating::Layer>();
    let mine = use_hook(crate::floating::claim);
    {
        let options = options.clone();
        use_effect(move || {
            if !open() {
                floating.clear(mine);
                return;
            }
            let (ax, ay) = anchor();
            let options = options.clone();
            floating.put(mine, rsx! {
            div {
                class: "ctx-scrim",
                onclick: move |event| {
                    event.stop_propagation();
                    open.set(false);
                },
                oncontextmenu: move |event| {
                    event.prevent_default();
                    open.set(false);
                },
            }
            div {
                class: "dd-list",
                style: "left: {ax.max(4.0)}px; top: {ay.max(4.0)}px; min-width: 236px;",
                onclick: move |event| event.stop_propagation(),
                for (index, option) in options.iter().enumerate() {
                    if option.is_separator() {
                        div { key: "sep{index}", class: "ctxsep" }
                    } else {
                        {
                            let chosen = option.value.clone();
                            rsx! {
                                button {
                                    key: "opt{index}",
                                    class: "dd-item",
                                    onclick: move |event| {
                                        event.stop_propagation();
                                        open.set(false);
                                        on_pick.call(chosen.clone());
                                    },
                                    span { class: "tick", {crate::icons::icon(&option.glyph, 15)} }
                                    span { "{option.label}" }
                                }
                            }
                        }
                    }
                }
            }
        });
        });
    }

    rsx! {
        button {
            class: "{class}",
            title: "{caption}",
            onclick: move |event| {
                if !enabled {
                    return;
                }
                let point = event.client_coordinates();
                let drop = if large { 46.0 } else { 18.0 };
                anchor.set((point.x - 14.0, point.y + drop));
                open.set(!open());
            },
            span { class: "glyph", {crate::icons::icon(&glyph, size)} }
            span { class: "caption", "{caption}" }
            span { class: "caret", {crate::icons::icon("caret-down", 12)} }
        }

    }
}

/// A typeable field with a dropdown attached, the way the Font box works.
#[component]
pub fn ComboBox(
    value: String,
    options: Vec<Choice>,
    width: f64,
    on_pick: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let mut anchor = use_signal(|| (0.0f64, 0.0f64));
    let mut draft = use_signal(|| value.clone());


    // As in `Dropdown` and `MenuBtn`: the panel and the scrim behind it go to
    // the layer at the root. See `crate::floating`.
    let mut floating = use_context::<crate::floating::Layer>();
    let mine = use_hook(crate::floating::claim);
    {
        let options = options.clone();
        use_effect(move || {
            if !open() {
                floating.clear(mine);
                return;
            }
            let (ax, ay) = anchor();
            // Narrowed to what has been typed. The list is whatever fonts the
            // machine has, which runs to several hundred, and finding one by
            // scrolling is not finding it. Letters in order rather than a
            // substring, so `tnr` reaches Times New Roman.
            let typed = draft();
            let options: Vec<Choice> = options
                .iter()
                .filter(|choice| fuzzy_matches(&choice.label, &typed))
                .cloned()
                .collect();
            floating.put(mine, rsx! {
            div {
                class: "ctx-scrim",
                onclick: move |event| {
                    event.stop_propagation();
                    open.set(false);
                },
            }
            div {
                class: "dd-list",
                style: "left: {ax.max(4.0)}px; top: {ay.max(4.0)}px; min-width: {width.max(120.0)}px;",
                onclick: move |event| event.stop_propagation(),
                if options.is_empty() {
                    div { class: "dd-empty", "No font matches that" }
                }
                for choice in options.iter() {
                    {
                        let picked = choice.value == draft();
                        let item_class = if picked { "dd-item on" } else { "dd-item" };
                        let chosen = choice.value.clone();
                        rsx! {
                            button {
                                key: "{choice.value}",
                                class: "{item_class}",
                                onclick: move |event| {
                                    event.stop_propagation();
                                    open.set(false);
                                    draft.set(chosen.clone());
                                    on_pick.call(chosen.clone());
                                },
                                span { class: "tick", if picked { "\u{2713}" } }
                                span { "{choice.label}" }
                            }
                        }
                    }
                }
            }
        });
        });
    }

    rsx! {
        div { class: "combo", style: "width: {width}px;",
            input {
                class: "combo-input",
                value: "{draft}",
                oninput: move |event| draft.set(event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter {
                        on_pick.call(draft());
                    }
                },
                onblur: move |_| on_pick.call(draft()),
            }
            button {
                class: "combo-caret",
                onclick: move |event| {
                    let point = event.client_coordinates();
                    anchor.set((point.x - width + 22.0, point.y + 16.0));
                    open.set(!open());
                },
                span { class: "caret", {crate::icons::icon("caret-down", 12)} }
            }
        }

    }
}

/// Put one value on the system clipboard.
///
/// Through the webview, which is the only clipboard this build can reach. The
/// value is encoded as JSON rather than pasted into the script, so a value
/// containing a quote is a string containing a quote and not a syntax error.
///
/// Here rather than beside whichever page wanted it first, because three
/// unrelated places now copy something and a second copy of this would be a
/// second place for the escaping to be got wrong.
///
/// Two routes, and the second is not a nicety. `navigator.clipboard` exists
/// only in a secure context. WebKitGTK treats this application's own protocol
/// as trusted and hands it over; WebView2 does not, so on Windows the object
/// is undefined and the previous `navigator.clipboard && ...` short circuited
/// to nothing at all: every copy in the application silently did nothing, with
/// no error anywhere, which is the worst way for a feature to be missing.
///
/// `execCommand` is deprecated and works in a plain context, which is exactly
/// the case that needs it.
pub fn copy_to_clipboard(value: &str) {
    let Ok(encoded) = serde_json::to_string(value) else {
        return;
    };
    document::eval(&format!(
        r#"(function (text) {{
             if (navigator.clipboard && window.isSecureContext) {{
               navigator.clipboard.writeText(text);
               return;
             }}
             // The fallback has to be attached to the document and selected
             // before the copy will take, and reading it back is what the
             // browser refuses off screen, so it is placed out of sight
             // rather than hidden.
             var box = document.createElement("textarea");
             box.value = text;
             box.setAttribute("readonly", "");
             box.style.position = "fixed";
             box.style.top = "-1000px";
             document.body.appendChild(box);
             box.select();
             try {{ document.execCommand("copy"); }} catch (e) {{}}
             document.body.removeChild(box);
           }})({encoded});"#
    ));
}

/// Whether a typed fragment matches a name, letters in order but not adjacent.
///
/// The font list holds whatever the machine has, which on a developer's
/// machine is several hundred families. Finding one by scrolling is not
/// finding it. Typing `tnr` should reach Times New Roman, and `dejavus` should
/// reach DejaVu Sans, which is what matching in order rather than as a
/// substring buys: the letters have to appear, in that order, and nothing says
/// they have to be next to each other.
///
/// Case is ignored, and so is anything that is not a letter or a digit, so a
/// space or a hyphen in either the query or the name is never the reason a
/// font cannot be found.
pub fn fuzzy_matches(name: &str, query: &str) -> bool {
    let mut wanted = query
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase());
    let mut next = match wanted.next() {
        None => return true, // nothing typed matches everything
        Some(c) => c,
    };
    for letter in name.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()) {
        if letter == next {
            match wanted.next() {
                None => return true,
                Some(c) => next = c,
            }
        }
    }
    false
}

#[cfg(test)]
mod fuzzy_tests {
    use super::fuzzy_matches;

    #[test]
    fn initials_reach_a_font_nobody_wants_to_scroll_to() {
        assert!(fuzzy_matches("Times New Roman", "tnr"));
        assert!(fuzzy_matches("DejaVu Sans Mono", "dvsm"));
    }

    #[test]
    fn a_run_of_letters_still_works_the_obvious_way() {
        assert!(fuzzy_matches("Liberation Serif", "serif"));
        assert!(fuzzy_matches("Noto Sans", "noto"));
    }

    #[test]
    fn spaces_and_hyphens_never_stand_in_the_way() {
        assert!(fuzzy_matches("Noto Sans", "notosans"));
        assert!(fuzzy_matches("Noto Sans", "noto sans"));
        assert!(fuzzy_matches("IBM Plex Mono", "ibm-plex"));
    }

    #[test]
    fn the_letters_have_to_be_in_that_order() {
        assert!(!fuzzy_matches("Times New Roman", "rnt"));
        assert!(!fuzzy_matches("Arial", "arialx"));
    }

    #[test]
    fn nothing_typed_offers_everything() {
        assert!(fuzzy_matches("Arial", ""));
        assert!(fuzzy_matches("Arial", "   "));
    }

    #[test]
    fn case_is_not_the_reason_a_font_cannot_be_found() {
        assert!(fuzzy_matches("Times New Roman", "TIMES"));
        assert!(fuzzy_matches("times new roman", "TnR"));
    }
}
