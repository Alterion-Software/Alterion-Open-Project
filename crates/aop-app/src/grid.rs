//! The task table.
//!
//! Columns are configurable: the list in `AppState::columns` decides which
//! fields appear, in what order and at what width, so the same component draws
//! the Entry table, a cost table, or anything else the user assembles.
//!
//! Rows can be dragged to reorder, and dropping onto the middle of a row nests
//! the dragged block underneath it.

use dioxus::prelude::*;

use aop_core::grouping::GroupRow;
use aop_core::{format_work, Align, ConstraintType, Field, TaskMode};

use crate::icons::icon;
use crate::gantt::{HEADER_H, ROW_H};
use crate::viewport::PaneScroll;
use crate::state::{format_date, AppState, Column, Dialog, DropWhere};

/// Where in a row's height the pointer sits decides how a drop is applied.
fn drop_zone(offset_y: f64) -> DropWhere {
    if offset_y < ROW_H * 0.32 {
        DropWhere::Above
    } else if offset_y > ROW_H * 0.68 {
        DropWhere::Below
    } else {
        DropWhere::Into
    }
}

/// The cell editor a field opens, if it can be typed into at all.
fn editor_for(field: Field) -> Option<Column> {
    match field {
        Field::Name => Some(Column::Name),
        Field::Duration => Some(Column::Duration),
        Field::Start => Some(Column::Start),
        Field::Finish => Some(Column::Finish),
        Field::Predecessors => Some(Column::Predecessors),
        Field::Successors => Some(Column::Successors),
        Field::ResourceNames => Some(Column::Resources),
        _ => None,
    }
}

/// Fields edited by a floating picker rather than by a text box in the cell.
///
/// Every one of these names other rows, so a list with the outline visible is
/// the only way to pick one without knowing its number by heart. The picker
/// still takes typed numbers, for planners who do.
fn edits_in_a_popup(field: Field) -> bool {
    popup_column(field).is_some()
}

/// The cell editor a picker field opens.
///
/// One place says which column a field belongs to, so the grid cannot open the
/// predecessor picker over a Successors cell.
fn popup_column(field: Field) -> Option<Column> {
    match field {
        Field::Predecessors => Some(Column::Predecessors),
        Field::Successors => Some(Column::Successors),
        Field::ResourceNames => Some(Column::Resources),
        _ => None,
    }
}

fn align_class(align: Align) -> &'static str {
    match align {
        Align::Left => "",
        Align::Right => "c-num",
        Align::Centre => "c-mid",
    }
}

/// An input that replaces a cell while it is being edited.
#[component]
/// The editor for a cell that a picker also writes to.
///
/// Two things edit a dependency cell: this box, and the tick list floating
/// under it. Both read and write `AppState::cell_draft`, so whichever moves
/// last is what the cell says. Committing happens on Enter or when the picker
/// is dismissed, never on blur, since clicking a tick box blurs this input.
#[component]
fn PickerCellEditor(row: usize, column: Column) -> Element {
    let mut state = use_context::<Signal<AppState>>();

    // The draft is local. Writing every keystroke into the shared state would
    // redraw the whole grid, which throws the caret out of the box being typed
    // in; that is exactly what it used to do.
    let mut draft = use_signal(|| state.read().cell_draft.clone());

    // The picker writes to the plan directly, so when a box is ticked the text
    // here has to catch up. The counter is what says a change came from the
    // picker rather than from anywhere else in the application.
    //
    // The counter is read through a memo and the text through `peek`, and both
    // halves of that matter. Reading the state directly inside the effect
    // subscribes it to every field, so any unrelated change anywhere, a hover
    // being the easiest one to trip over, would re-run this and overwrite
    // whatever was being typed with what the plan last said.
    let stamp = use_memo(move || state.read().picker_edits);
    use_effect(move || {
        let _ = stamp();
        draft.set(state.peek().cell_draft.clone());
    });

    // What is being typed, for anybody else with this plan open. Its own
    // signal so a keystroke never redraws the window, and cleared when this
    // editor goes so nobody is left watching an abandoned word.
    let mut shared = use_context::<crate::state::Drafting>().0;
    use_drop(move || shared.set(None));

    let mut commit = move || {
        let text = draft();
        // Only when it says something different from the plan. Ticking a box in
        // the picker changes the plan and refreshes this text, and committing
        // the text again on the way out would write the old value back over the
        // change that was just made.
        if text == state.peek().cell_text(row, column) {
            state.write().editing = None;
            return;
        }
        state.write().commit_cell(row, column, &text);
    };

    rsx! {
        div { class: "picker-cell",
            input {
                class: "cell-input",
                autofocus: true,
                value: "{draft}",
                onclick: move |event| event.stop_propagation(),
                onmousedown: move |event| event.stop_propagation(),
                ondoubleclick: move |event| event.stop_propagation(),
                onmouseup: move |event| event.stop_propagation(),
                oninput: move |event| {
                draft.set(event.value());
                shared.set(Some(event.value()));
            },
                // Clicking inside the picker does not blur this box, because
                // the picker prevents it, so a blur means the planner has
                // clicked away and whatever they typed should be kept.
                onblur: move |_| commit(),
                onkeydown: move |event| match event.key() {
                    Key::Enter => commit(),
                    Key::Escape => state.write().editing = None,
                    _ => {}
                },
            }
            // Says out loud that there is a list behind this cell. Clicking it
            // reopens the picker if it has been dismissed.
            button {
                class: "picker-caret",
                tabindex: "-1",
                title: "Choose from a list",
                onmousedown: move |event| event.prevent_default(),
                onclick: move |event| {
                    event.stop_propagation();
                    let point = event.client_coordinates();
                    state.write().edit_cell_at(row, column, point.x, point.y);
                },
                {crate::icons::icon("caret-down", 12)}
            }
        }
    }
}

#[component]
fn CellEditor(row: usize, column: Column, initial: String) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut draft = use_signal(|| initial.clone());
    let mut settled = use_signal(|| false);
    // What is being typed, for anybody else with this plan open. Its own
    // signal rather than a field on the plan, so a keystroke never redraws the
    // window, and never anywhere near the change log: an abandoned edit must
    // not become a permanent record of something that never happened.
    let mut shared = use_context::<crate::state::Drafting>().0;

    // Cleared when this editor goes, however it goes. The cell is released by
    // the same act, so nobody is left looking at half a word somebody
    // abandoned two minutes ago.
    use_drop(move || shared.set(None));

    let mut commit = move || {
        if settled() {
            return;
        }
        settled.set(true);
        let text = draft();
        let is_new_row = row >= state.read().project.tasks.len();
        if is_new_row {
            if !text.trim().is_empty() {
                state.write().append_task(text.trim());
            }
            state.write().editing = None;
        } else {
            state.write().commit_cell(row, column, &text);
        }
    };

    rsx! {
        input {
            class: "cell-input",
            autofocus: true,
            value: "{draft}",
            // Clicks inside the editor must not reach the row underneath:
            // selecting a row clears the edit, so moving the caret with the
            // mouse would otherwise throw away what was being typed.
            onclick: move |event| event.stop_propagation(),
            onmousedown: move |event| event.stop_propagation(),
            ondoubleclick: move |event| event.stop_propagation(),
            onmouseup: move |event| event.stop_propagation(),
            oninput: move |event| {
                draft.set(event.value());
                shared.set(Some(event.value()));
            },
            onblur: move |_| commit(),
            onkeydown: move |event| match event.key() {
                Key::Enter => commit(),
                Key::Escape => {
                    settled.set(true);
                    state.write().editing = None;
                }
                _ => {}
            },
        }
    }
}

#[component]
pub fn TaskGrid() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;
    let rows = s.layout_rows();
    let columns = s.columns.clone();
    // The table is as wide as its columns; the pane showing it may be narrower,
    // in which case it scrolls.
    let table_width: f64 = columns.iter().map(|c| c.width).sum();
    let pane_width = s.table_view_width();
    let editing = s.editing;
    let show_wbs = s.show_outline_number;
    let currency = project.currency_symbol.clone();
    let pattern = s.date_pattern();
    let text_styles = s.text_styles.clone();
    let grid_class = {
        let mut class = String::from("grid");
        if !s.grid_rows {
            class.push_str(" no-rows");
        }
        if !s.grid_columns {
            class.push_str(" no-columns");
        }
        class
    };
    let drag_row = s.drag_row;
    let drop_target = s.drop_target;

    // Which column is being resized, where the drag started and its width then.
    let mut resizing = use_signal(|| None::<(usize, f64, f64)>);

    // Where this planner's pointer is, for the others in a live session. Kept
    // out of the plan's state so that moving a mouse across the table does not
    // redraw it; the live timer picks it up a few times a second.
    let mut pointing = use_context::<crate::state::Pointing>().0;

    // Only the rows inside the scrolled viewport are drawn; the rest are stood
    // in for by a spacer, so the pane still scrolls its full height.
    let mut scroll = use_signal(PaneScroll::default);
    let rows_len = rows.len();
    let window = scroll().window(rows_len);

    rsx! {
        // While a column is being resized the whole window listens, so moving
        // the pointer faster than the grip can follow never drops the drag.
        if resizing().is_some() {
            div {
                class: "drag-shield col-resize",
                onmousemove: move |event| {
                    if let Some((column, from_x, from_width)) = resizing() {
                        let moved = event.client_coordinates().x - from_x;
                        state.write().set_column_width(column, from_width + moved);
                    }
                },
                onmouseup: move |_| resizing.set(None),
            }
        }

        div {
            class: "grid-pane",
            style: "width: {pane_width}px;",
            onscroll: move |event| {
                let data = event.data();
                let seen = PaneScroll {
                    top: data.scroll_top(),
                    height: data.client_height() as f64,
                    left: data.scroll_left(),
                    width: data.client_width() as f64,
                };
                // Writing on every scroll event would redraw the pane even when
                // the same rows are still the ones on screen.
                if scroll().window(rows_len) != seen.window(rows_len) {
                    scroll.set(seen);
                }
            },

            table { class: "{grid_class}", style: "width: {table_width}px;",
                colgroup {
                    for (index, column) in columns.iter().enumerate() {
                        col { key: "c{index}", style: "width: {column.width}px;" }
                    }
                }

                thead {
                    tr {
                        for (index, column) in columns.iter().enumerate() {
                            {
                                let field = column.field;
                                rsx! {
                                    th {
                                        key: "h{index}",
                                        class: "{align_class(field.align())}",
                                        title: "{field.label()}: {field.description()}",
                                        style: "height: {HEADER_H}px; width: {column.width}px;",
                                        oncontextmenu: move |event| {
                                            event.prevent_default();
                                            let point = event.client_coordinates();
                                            state.write().open_column_menu(index, point.x, point.y);
                                        },
                                        span { class: "th-inner",
                                            // Two columns are titled with a
                                            // symbol; the rest with their name.
                                            match field {
                                                Field::Indicators => rsx! {
                                                    span { class: "th-icon", {crate::icons::icon("col-indicators", 13)} }
                                                },
                                                Field::TaskMode => rsx! {
                                                    span { class: "th-icon", {crate::icons::icon("col-mode", 13)} }
                                                },
                                                _ => rsx! { "{field.heading()}" },
                                            }
                                        }
                                        div {
                                            class: "col-grip",
                                            title: "Drag to resize; double-click resets every column",
                                            onmousedown: move |event| {
                                                event.prevent_default();
                                                event.stop_propagation();
                                                let width = state
                                                    .read()
                                                    .columns
                                                    .get(index)
                                                    .map(|c| c.width)
                                                    .unwrap_or(90.0);
                                                resizing.set(Some((
                                                    index,
                                                    event.client_coordinates().x,
                                                    width,
                                                )));
                                            },
                                            ondoubleclick: move |_| state.write().reset_columns(),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                tbody {
                    if window.above > 0.0 {
                        tr { class: "row-spacer", style: "height: {window.above}px;" }
                    }

                    for (offset, row) in rows[window.first..window.end].iter().enumerate() {
                        {
                            // A band and a task each take exactly one line, in
                            // this pane and in the chart alike, which is what
                            // keeps the two scrolling in step.
                            match row {
                                GroupRow::Band { label, count, work_minutes, cost, depth } => {
                                    let line = window.first + offset;
                                    let span_all = columns.len();
                                    let indent = 12.0 + *depth as f64 * 14.0;
                                    let noun = if *count == 1 { "task" } else { "tasks" };
                                    let totals = format!(
                                        "{count} {noun}  \u{00b7}  {}  \u{00b7}  {currency}{cost:.2}",
                                        format_work(*work_minutes),
                                    );
                                    rsx! {
                                        tr { key: "band{line}", class: "row band",
                                            td {
                                                colspan: "{span_all}",
                                                style: "height: {ROW_H}px; padding-left: {indent}px;",
                                                span { class: "band-label", "{label}" }
                                                span { class: "band-totals", "{totals}" }
                                            }
                                        }
                                    }
                                }
                                &GroupRow::Task(index) => {
                                    let task = &project.tasks[index];
                                    let summary = project.is_summary(index);
                                    let selected = s.is_selected(index);

                                    let mut class = String::from("row");
                                    if selected { class.push_str(" selected"); }
                                    if summary { class.push_str(" summary"); }
                                    if !task.active { class.push_str(" inactive"); }
                                    if s.show_critical && aop_core::issues::shows_as_critical(project, index) {
                                        class.push_str(" critical");
                                    }
                                    if drag_row == Some(index) { class.push_str(" dragging"); }
                                    if let Some((target, mode)) = drop_target
                                        && target == index {
                                            class.push_str(match mode {
                                                DropWhere::Above => " drop-above",
                                                DropWhere::Below => " drop-below",
                                                DropWhere::Into => " drop-into",
                                            });
                                        }

                                    // The category's look first, then whatever the
                                    // planner set on this row on top. Only the parts
                                    // actually set are written: leaving one unset
                                    // means "the theme's", not "black".
                                    let row_style = text_styles.css_for(project, index);

                                    rsx! {
                                        tr {
                                            key: "row{index}",
                                            class: "{class}",
                                            style: "{row_style}",

                                            // WebKit will not reliably start an HTML5 drag
                                            // on a table row, so reordering runs on plain
                                            // pointer events instead.
                                            onmousemove: move |event| {
                                                // The row is the event's own
                                                // element, so its coordinates
                                                // are already the table's: no
                                                // rectangle to measure and no
                                                // scroll offset to subtract.
                                                let along = event.element_coordinates().x;
                                                let at = state.peek().table_pointer(index, along);
                                                if *pointing.peek() != Some(at) {
                                                    pointing.set(Some(at));
                                                }
                                                if resizing().is_none() && state.read().drag_row.is_some() {
                                                    let mode = drop_zone(event.element_coordinates().y);
                                                    state.write().hover_drop(index, mode);
                                                }
                                            },
                                            onmouseup: move |_| {
                                                if state.read().drag_row.is_some() {
                                                    state.write().finish_drag();
                                                }
                                            },

                                            onclick: move |event| {
                                                let mut writer = state.write();
                                                if event.modifiers().ctrl() {
                                                    writer.toggle_selection(index);
                                                } else if event.modifiers().shift() {
                                                    writer.extend_selection(index);
                                                } else {
                                                    writer.select(index);
                                                }
                                            },

                                            oncontextmenu: move |event| {
                                                event.prevent_default();
                                                let point = event.client_coordinates();
                                                state.write().open_task_menu(index, point.x, point.y);
                                            },

                                            for (position, column) in columns.iter().enumerate() {
                                                {
                                                    let field = column.field;
                                                    let editor = editor_for(field);
                                                    let is_editing =
                                                        editor.map(|c| editing == Some((index, c))).unwrap_or(false);
                                                    let mut cell_class = String::from(align_class(field.align()));
                                                    if field == Field::Id {
                                                        cell_class.push_str(" rownum");
                                                    }
                                                    let value = field.value(project, index, pattern);

                                                    rsx! {
                                                        td {
                                                            key: "c{position}",
                                                            class: "{cell_class}",
                                                            style: "height: {ROW_H}px;",

                                                            // Which column the cursor is on, for the commands
                                                            // that act on a column rather than a whole row.
                                                            onclick: move |event| {
                                                                state.write().fill_field = Some(field);
                                                                // Predecessors and resources name other
                                                                // rows, so one click opens the list rather
                                                                // than making the planner recall a number.
                                                                if let Some(column) = popup_column(field) {
                                                                    event.stop_propagation();
                                                                    let point = event.client_coordinates();
                                                                    let mut writer = state.write();
                                                                    writer.select(index);
                                                                    writer.edit_cell_at(index, column, point.x, point.y);
                                                                }
                                                            },

                                                            // The ID cell doubles as the drag handle.
                                                            onmousedown: move |event| {
                                                                if field == Field::Id {
                                                                    event.prevent_default();
                                                                    let mut writer = state.write();
                                                                    writer.select(index);
                                                                    writer.begin_drag(index);
                                                                }
                                                            },

                                                            ondoubleclick: move |event| match field {
                                                                // These open a picker, not a text box.
                                                                field if edits_in_a_popup(field) => {
                                                                    let point = event.client_coordinates();
                                                                    if let Some(column) = popup_column(field) {
                                                                        state.write().edit_cell_at(index, column, point.x, point.y);
                                                                    }
                                                                }
                                                                Field::TaskMode => {
                                                                    let next = if state.read().project.tasks[index].mode
                                                                        == TaskMode::Auto
                                                                    {
                                                                        TaskMode::Manual
                                                                    } else {
                                                                        TaskMode::Auto
                                                                    };
                                                                    state.write().select(index);
                                                                    state.write().set_task_mode(next);
                                                                }
                                                                _ => {
                                                                    if let Some(column) = editor {
                                                                        state.write().editing = Some((index, column));
                                                                    } else {
                                                                        state.write().dialog =
                                                                            Some(Dialog::TaskInformation(index));
                                                                    }
                                                                }
                                                            },

                                                            {cell_body(
                                                                field, project, index, summary, show_wbs,
                                                                &currency, &value, is_editing, editor,
                                                            )}
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if window.below > 0.0 {
                        tr { class: "row-spacer", style: "height: {window.below}px;" }
                    }

                    // The blank row at the bottom, where typing creates a task.
                    {
                        let new_row = project.tasks.len();
                        let name_at = columns.iter().position(|c| c.field == Field::Name);
                        rsx! {
                            tr { class: "row add-row",
                                onmousemove: move |_| {
                                    if state.read().drag_row.is_some() {
                                        let last = state.read().project.tasks.len().checked_sub(1);
                                        if let Some(last) = last {
                                            state.write().hover_drop(last, DropWhere::Below);
                                        }
                                    }
                                },
                                onmouseup: move |_| {
                                    if state.read().drag_row.is_some() {
                                        state.write().finish_drag();
                                    }
                                },

                                for (position, column) in columns.iter().enumerate() {
                                    {
                                        let is_name = Some(position) == name_at;
                                        let is_id = column.field == Field::Id;
                                        let cell_class = if is_id { "rownum" } else { "" };
                                        rsx! {
                                            td {
                                                key: "n{position}",
                                                class: "{cell_class}",
                                                style: "height: {ROW_H}px;",
                                                onclick: move |_| {
                                                    if is_name {
                                                        state.write().editing = Some((new_row, Column::Name));
                                                    }
                                                },
                                                if is_id {
                                                    "{new_row + 1}"
                                                } else if is_name {
                                                    if editing == Some((new_row, Column::Name)) {
                                                        CellEditor {
                                                            row: new_row,
                                                            column: Column::Name,
                                                            initial: String::new(),
                                                        }
                                                    } else {
                                                        div { class: "cell-name", style: "padding-left: 12px;",
                                                            "Click to add a task" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Inside the pane and after the table, so the pane's own scrolling
            // carries other people's pointers along with the rows they are on
            // and clips the ones that have scrolled off.
            crate::cursors::TableCursors {}
        }
    }
}

/// Draw the contents of one cell.
#[allow(clippy::too_many_arguments)]
fn cell_body(
    field: Field,
    project: &aop_core::Project,
    index: usize,
    summary: bool,
    show_wbs: bool,
    currency: &str,
    value: &str,
    is_editing: bool,
    editor: Option<Column>,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let task = &project.tasks[index];

    // A cell the picker also writes to gets its own editor: the text lives in
    // shared state so ticking a box in the picker and typing here cannot
    // disagree, and it does not commit on blur, because clicking the picker
    // blurs it.
    if is_editing && let Some(column) = popup_column(field) {
        return rsx! { PickerCellEditor { row: index, column } };
    }

    // Every other cell is replaced by a plain text box.
    if is_editing
        && let Some(column) = editor {
            let initial = match field {
                // Dates are edited in an unambiguous form, whatever the display
                // format happens to be set to.
                Field::Start => task.scheduled.start.format("%Y-%m-%d").to_string(),
                Field::Finish => task.scheduled.finish.format("%Y-%m-%d").to_string(),
                Field::Name => task.name.clone(),
                _ => value.to_string(),
            };
            return rsx! { CellEditor { row: index, column, initial } };
        }

    match field {
        Field::Id => rsx! { "{index + 1}" },

        Field::Indicators => rsx! {
            span {
                title: "{indicator_tooltip(project, index, task.scheduled.cost, currency)}",
                {indicator_glyph(project, index)}
            }
        },

        Field::TaskMode => rsx! {
            span {
                // A hand for placed by a person, a bolt for placed by the
                // scheduler. Two triangles told you a row differed from its
                // neighbour without saying how.
                class: if task.mode == TaskMode::Auto { "mode-glyph auto" } else { "mode-glyph manual" },
                title: "{task.mode.label()}",
                if task.mode == TaskMode::Auto {
                    {icon("mode-auto", 13)}
                } else {
                    {icon("mode-manual", 13)}
                }
            }
        },

        Field::Name => {
            let indent = task.outline_level as f64 * 12.0;
            let text = if task.name.is_empty() {
                String::new()
            } else if show_wbs {
                format!("{}  {}", project.wbs(index), task.name)
            } else {
                task.name.clone()
            };
            rsx! {
                div { class: "cell-name", style: "padding-left: {indent}px;",
                    if summary {
                        span {
                            class: "twisty",
                            onclick: move |event| {
                                event.stop_propagation();
                                state.write().toggle_collapse(index);
                            },
                            if task.collapsed { "\u{25b6}" } else { "\u{25bc}" }
                        }
                    } else {
                        span { class: "twisty" }
                    }
                    span { "{text}" }
                }
            }
        }

        // Yes/No fields read better as a tick than as the word.
        Field::Critical | Field::Milestone | Field::Summary | Field::Active => {
            let on = value == "Yes";
            let colour = match field {
                Field::Critical => "var(--warn)",
                Field::Active => "var(--ink-soft)",
                _ => "var(--accent)",
            };
            rsx! {
                if on {
                    span { style: "color: {colour};", "\u{2713}" }
                }
            }
        }

        _ => rsx! { "{value}" },
    }
}

fn indicator_glyph(project: &aop_core::Project, index: usize) -> Element {
    let task = &project.tasks[index];
    if task.percent_complete >= 100 {
        return rsx! { span { style: "color: var(--accent);", {crate::icons::icon("tick", 13)} } };
    }
    // Being on the critical path is the thing most worth flagging: any slip
    // here moves the finish date. Once that has been acknowledged the marker
    // stays but goes quiet, so the row still says there is something known
    // about it without shouting.
    if task.scheduled.critical && !project.is_summary(index) {
        let class = if aop_core::issues::is_ignored(project, index, aop_core::model::IssueKind::Critical) {
            "ind-critical ignored"
        } else {
            "ind-critical"
        };
        return rsx! { span { class: "{class}", "\u{26a0}" } };
    }
    if !task.notes.is_empty() {
        return rsx! { span { style: "color: var(--ink-soft);", {crate::icons::icon("pencil", 13)} } };
    }
    if task.deadline.is_some() {
        return rsx! { span { style: "color: var(--bar-critical-edge);", {crate::icons::icon("flag", 13)} } };
    }
    if task.constraint != ConstraintType::AsSoonAsPossible {
        return rsx! { span { style: "color: var(--contextual);", {crate::icons::icon("fisheye", 13)} } };
    }
    rsx! { span {} }
}

fn indicator_tooltip(
    project: &aop_core::Project,
    index: usize,
    cost: f64,
    currency: &str,
) -> String {
    let task = &project.tasks[index];
    let mut lines = vec![format!("{}  \u{00b7}  {}% complete", task.name, task.percent_complete)];

    if let Some(reason) = aop_core::critical_reason(project, index) {
        lines.push(reason);
    }

    // A constraint is meaningless without the date it holds the task to.
    if task.constraint != ConstraintType::AsSoonAsPossible {
        lines.push(match task.constraint_date {
            Some(date) => format!("{}: {}", task.constraint.label(), format_date(date)),
            None => task.constraint.label().to_string(),
        });
    }

    if let Some(deadline) = task.deadline {
        lines.push(format!("Deadline: {}", format_date(deadline)));
    }
    if task.mode == TaskMode::Manual {
        lines.push("Manually scheduled: its links do not move it".into());
    }
    if !task.active {
        lines.push("Inactive: the scheduler ignores it".into());
    }
    if cost > 0.0 {
        lines.push(format!("Cost: {currency}{cost:.2}"));
    }
    if !task.notes.is_empty() {
        lines.push(task.notes.clone());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use aop_core::MINUTES_PER_DAY;

    /// A four row outline: Phase / Child A / Child B / Standalone, so exactly
    /// one of the four rows is a summary.
    fn outlined() -> AppState {
        let mut state = AppState::new();
        state.project.tasks.clear();
        state.project.links.clear();
        for name in ["Phase", "Child A", "Child B", "Standalone"] {
            state.project.push_task(name, MINUTES_PER_DAY);
        }
        state.project.tasks[1].outline_level = 1;
        state.project.tasks[2].outline_level = 1;
        state.reschedule();
        state
    }

    fn task_indices(rows: &[GroupRow]) -> Vec<usize> {
        rows.iter()
            .filter_map(|row| match row {
                &GroupRow::Task(index) => Some(index),
                GroupRow::Band { .. } => None,
            })
            .collect()
    }

    #[test]
    fn with_no_grouping_the_layout_is_the_visible_rows_and_nothing_more() {
        // Both panes now lay out from `layout_rows`, so an ungrouped plan has
        // to come back exactly as `visible_rows` drew it: one extra or missing
        // line here would slide every bar in the chart off its row.
        let state = outlined();
        let layout = state.layout_rows();
        let visible = state.visible_rows();
        assert_eq!(task_indices(&layout), visible);
        assert_eq!(layout.len(), visible.len(), "no bands when nothing groups");
    }

    #[test]
    fn a_grouped_layout_still_holds_every_leaf_row_exactly_once() {
        // A band is a heading, not a task, so grouping adds lines without ever
        // dropping or repeating one of the rows the plan actually has.
        let mut state = outlined();
        state.set_group_by("duration");
        let layout = state.layout_rows();
        let leaves = (0..state.project.tasks.len())
            .filter(|&index| !state.project.is_summary(index))
            .count();
        assert_eq!(task_indices(&layout).len(), leaves);
        assert!(layout.len() > leaves, "the bands take lines of their own");
    }
}
