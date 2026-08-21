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
use crate::viewport::{ColumnDrag, GridScroll, Part, Reach};
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
pub fn TaskGrid(part: Part) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let project = &s.project;
    let rows = s.layout_rows();
    let columns = s.columns.clone();
    // The table is as wide as its columns; the pane showing it may be narrower,
    // in which case it scrolls.
    let table_width: f64 = columns.iter().map(|c| c.width).sum();
    // How wide the pane is, is no longer this component's business. It draws
    // its columns at their own width and the split clips and shifts it; what
    // this does say is how far there is to go, so the strip along the bottom
    // can offer the whole of it.
    let mut reach = use_context::<Signal<Reach>>();
    use_effect(use_reactive!(|table_width| {
        let now = reach();
        if now.table != table_width {
            reach.set(Reach { table: table_width, ..now });
        }
    }));
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
    //
    // Held by the split rather than here. The grip that starts the drag is in
    // the heading and the rows the pointer then travels over are in the body,
    // and since those are now two boxes in two rows of the split, a signal
    // private to either one would only be half the drag. The sheet that
    // catches the movement is the split's too, for the same reason: it has to
    // be able to cover both.
    let mut resizing = use_context::<Signal<ColumnDrag>>();

    // Where this planner's pointer is, for the others in a live session. Kept
    // out of the plan's state so that moving a mouse across the table does not
    // redraw it; the live timer picks it up a few times a second.
    let mut pointing = use_context::<crate::state::Pointing>().0;

    // Only the rows inside the scrolled viewport are drawn; the rest are stood
    // in for by a spacer, so the pane still scrolls its full height.
    //
    // The scrolling itself belongs to the split now, because both panes ride
    // one scroll container, which is what makes their rows stay level without
    // anything having to be kept in step. What arrives here is only the answer
    // to "which rows are on screen", and it is only written when that answer
    // changes.
    let scroll = use_context::<GridScroll>().0;
    let rows_len = rows.len();
    let window = scroll().window(rows_len);

    // The heading, written once and put in both halves of the grid.
    //
    // Splitting the heading from the rows is what pins it: it is not in the
    // box that scrolls, so there is nothing for it to scroll away from.
    //
    // The copy in the row table is the heading of the real table: the widths
    // of the columns are then one decision rather than two that have to agree,
    // which is what "make the header part of the table" asks for and what the
    // titles drifting off their columns was. It is pulled up out of sight by
    // its own height there, so it costs no room. The copy above is the one you
    // see, and its grips are the ones that resize a column.
    let heading = |ghost: bool| {
        // No height at all for the copy that is only there to settle the
        // widths. It has to be said here rather than in the stylesheet: the
        // height is written on the cell itself, and a style written on an
        // element beats any rule, so a rule saying "no height" was simply
        // overruled and the row table wore a band of empty as tall as a
        // heading.
        let tall = if ghost { 0.0 } else { HEADER_H };
        rsx! {
                thead { class: if ghost { "ghost" } else { "" },
                    tr {
                        for (index, column) in columns.iter().enumerate() {
                            {
                                let field = column.field;
                                rsx! {
                                    th {
                                        key: "h{index}",
                                        class: "{align_class(field.align())}",
                                        title: "{field.label()}: {field.description()}",
                                        style: "height: {tall}px; width: {column.width}px;",
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
        }
    };

    let head = rsx! {
        div { class: "grid-head", style: "width: {table_width}px;",
            table { class: "{grid_class}", style: "width: {table_width}px;",
                // A colgroup in both, saying the same widths. It was left out
                // once, on the grounds that `table-layout: fixed` reads its
                // widths from the first row and the heading cells carried
                // them. That stopped being true when the heading was lifted
                // out: the row table's first row became the spacer standing in
                // for the rows scrolled off above, and a spacer has no cells
                // to read a width from. Fixed layout with nothing to read
                // shares the width out evenly, which is a heading whose
                // columns do not line up with the cells under it.
                {colgroup(&columns)}
                {heading(false)}
            }
        }
    };

    let body = rsx! {
        div { class: "grid-body", style: "width: {table_width}px;",
            table { class: "{grid_class}", style: "width: {table_width}px;",
                {colgroup(&columns)}
                // The same heading, of no height at all. It is here so that
                // the widths of the columns are settled by one heading rather
                // than by two that have to agree, and it is flattened rather
                // than pulled up by its own height, because a pull-up is a
                // number that has to match another number and this is not.
                {heading(true)}
                tbody {
                    // Always here, at whatever height the rows above need,
                    // rather than appearing when that height is more than
                    // nothing. A child that comes and goes changes the shape
                    // of the list it is in, and this list is also wholly
                    // replaced whenever a different plan is opened. Dioxus
                    // addresses nodes by a path through the template it
                    // expects, and a path into a child that is not there is
                    // what `blitz_dom` reports as `invalid key` before taking
                    // the process down. A row of no height costs nothing and
                    // keeps the shape still.
                    tr { key: "spacer-above", class: "row-spacer",
                        // With a cell in it. A row of a table is as tall as its
                        // cells and no taller: a `tr` holding nothing has
                        // nothing for a height to apply to, so the rows this
                        // stands in for went unaccounted and everything drawn
                        // below it rode up by that much, which is the top rows
                        // sliding under the heading as soon as you scrolled.
                        td { colspan: "{columns.len()}", style: "height: {window.above}px;" }
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
                                        tr { key: "band-{line}", class: "row band",
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
                                            // Keyed by the task's own id, not by
                                            // where it happens to sit. A positional
                                            // key means the same key stands for a
                                            // different task the moment two rows
                                            // change places, so a reorder is a
                                            // wholesale rebuild rather than a move,
                                            // and rebuilding a list this deeply
                                            // nested is what walks a template path
                                            // into a node that is not there.
                                            key: "task-{task.id}",
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

                                            for (_position, column) in columns.iter().enumerate() {
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
                                                            // By field, not by position: a
                                                            // column moved or hidden must
                                                            // not make every cell after it
                                                            // a different node.
                                                            key: "cell-{field:?}",
                                                            class: "{cell_class}",
                                                            style: "height: {ROW_H}px; width: {column.width}px;",

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
                                                                &currency, &value, column.width,
                                                                is_editing, editor,
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

                    tr { key: "spacer-below", class: "row-spacer",
                        td { colspan: "{columns.len()}", style: "height: {window.below}px;" }
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
                                                style: "height: {ROW_H}px; width: {column.width}px;",
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

            // Inside the body and after the table, so the split's scrolling
            // carries other people's pointers along with the rows they are on
            // and clips the ones that have scrolled off.
            crate::cursors::TableCursors {}
        }
    };

    match part {
        Part::Head => head,
        Part::Body => body,
    }
}

/// A piece of text cut to the room it has.
///
/// Cut here rather than clipped by the renderer. `overflow: hidden` is what a
/// stylesheet would say, and it costs a painting layer per cell; the renderer
/// keeps a thousand layers for a whole frame and a table drawing fifty rows of
/// eight columns wants four hundred of them, which is most of the window's
/// ration spent on a table. Whatever is painted after the ration runs out is
/// not clipped at all, and that is one bug that shows up as three: cells over
/// the splitter, a dropdown's list outside its box, a menu outside its panel.
///
/// The width comes from an average character, so the cut lands a character or
/// so out either way on a proportional face. That is the trade: a cut that is
/// approximately in the right place everywhere, against an exact one in the
/// first four hundred cells and none at all after that.
///
/// Cut plainly, with nothing added to mark it. An ellipsis costs a character
/// of the room there already was not enough of.
fn shorten(text: &str, room: f64) -> String {
    if !overflows(text, room) {
        return text.to_string();
    }
    const CHAR_W: f64 = 6.2;
    const PADDING: f64 = 12.0;
    let fits = ((room - PADDING) / CHAR_W).floor().max(0.0) as usize;
    text.chars().take(fits).collect()
}

/// Whether a piece of text is too long for the room it has.
///
/// An estimate, from an average character width, and it only has to be roughly
/// right. What hangs on it is whether the cell is given `overflow: hidden`,
/// and that is not free: the renderer paints every clipping box as a layer of
/// its own and keeps only a thousand of them. A table drawing ninety rows of
/// eight columns asks for seven hundred and forty before anything else in the
/// window has had one, and once the ration runs out the renderer stops
/// clipping silently. Everything painted after that point spills: table cells
/// over the splitter and into the chart, a dropdown's list outside its own
/// box, a menu outside its panel. One symptom, in three places that had
/// nothing to do with each other.
///
/// So a cell is only clipped when its text would actually come out of it,
/// which on an ordinary screenful is a handful of cells rather than all of
/// them.
fn overflows(text: &str, room: f64) -> bool {
    const CHAR_W: f64 = 6.2;
    const PADDING: f64 = 12.0;
    text.chars().count() as f64 * CHAR_W > room - PADDING
}

/// The column widths, stated once per table.
///
/// Both halves of the grid are given the same one, so the titles and the cells
/// cannot come to different conclusions about where a column ends.
fn colgroup(columns: &[crate::state::ColumnSpec]) -> Element {
    rsx! {
        colgroup {
            for (index, column) in columns.iter().enumerate() {
                col { key: "col{index}", style: "width: {column.width}px;" }
            }
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
    // How wide the column is, so the text can be cut to it rather than clipped
    // to it. See `shorten`.
    room: f64,
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
            // The twisty in front of the name takes its own room, and the
            // indent takes more the deeper the task sits.
            let text = shorten(&text, room - indent - 14.0);
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

        _ => {
            let text = shorten(value, room);
            rsx! { "{text}" }
        }
    }
}

fn indicator_glyph(project: &aop_core::Project, index: usize) -> Element {
    let task = &project.tasks[index];
    if task.percent_complete >= 100 {
        return rsx! { span { style: "color: var(--accent);", "\u{2714}" } };
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
        return rsx! { span { style: "color: var(--ink-soft);", "\u{270e}" } };
    }
    if task.deadline.is_some() {
        return rsx! { span { style: "color: var(--bar-critical-edge);", "\u{2691}" } };
    }
    if task.constraint != ConstraintType::AsSoonAsPossible {
        return rsx! { span { style: "color: var(--contextual);", "\u{25c9}" } };
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

#[cfg(test)]
mod overflow_tests {
    use super::*;

    #[test]
    fn a_cell_with_room_to_spare_is_not_clipped() {
        // This is the whole point. A clip is a painting layer and the renderer
        // keeps a thousand; a table that asks for one per cell spends the lot
        // and everything painted afterwards stops being clipped at all.
        assert!(!overflows("3 days", 90.0));
        assert!(!overflows("Mon 13/10/25", 110.0));
        assert!(!overflows("", 20.0));
    }

    #[test]
    fn a_cell_whose_text_would_come_out_of_it_is_clipped() {
        assert!(overflows("CCT tool (Data Migration) Part of Fusion TIF", 120.0));
        assert!(overflows("Liesl Hollander", 60.0));
    }

    #[test]
    fn a_column_narrower_than_its_padding_still_answers() {
        // No arithmetic that can go negative and wrap: the answer for a column
        // with no room at all is "yes, clip it".
        assert!(overflows("a", 0.0));
        assert!(overflows("a", 6.0));
    }
}

#[cfg(test)]
mod shorten_tests {
    use super::*;

    #[test]
    fn text_that_fits_is_handed_back_whole() {
        assert_eq!(shorten("3 days", 90.0), "3 days");
        assert_eq!(shorten("Mon 13/10/25", 110.0), "Mon 13/10/25");
    }

    #[test]
    fn text_that_does_not_fit_is_cut() {
        let whole = "CCT tool (Data Migration) Part of Fusion TIF";
        let cut = shorten(whole, 120.0);
        assert!(cut.chars().count() < whole.chars().count());
        assert!(whole.starts_with(&cut), "cut, not rewritten");
        // And it has to actually fit, or the whole exercise is pointless.
        assert!(!overflows(&cut, 120.0));
    }

    #[test]
    fn a_column_with_no_room_at_all_shows_nothing() {
        // Rather than an ellipsis on its own, or an arithmetic wrap.
        assert_eq!(shorten("anything", 0.0), "");
        assert_eq!(shorten("anything", 12.0), "");
    }

    #[test]
    fn a_cut_never_splits_a_character() {
        // Counted in characters, not bytes: a task may be named in any script,
        // and slicing a string by bytes panics in the middle of a character.
        let name = "r\u{e9}sum\u{e9} r\u{e9}sum\u{e9} r\u{e9}sum\u{e9}";
        let cut = shorten(name, 40.0);
        assert!(cut.chars().count() < name.chars().count());
        assert!(name.starts_with(&cut));
    }
}
