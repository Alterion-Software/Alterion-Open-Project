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

    let (ax, ay) = anchor();
    let list_width = if width > 0.0 { width.max(120.0) } else { 260.0 };

    rsx! {
        button {
            class: "{class}",
            style: "{style}",
            title: "{selected}",
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
            span { class: "dd-caret", "\u{25be}" }
        }

        if open() {
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
    let (ax, ay) = anchor();
    let size = if large { 28 } else { 16 };

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
            span { class: "caret", "\u{25be}" }
        }

        if open() {
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

    let (ax, ay) = anchor();

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
                span { class: "caret", "\u{25be}" }
            }
        }

        if open() {
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
        }
    }
}
