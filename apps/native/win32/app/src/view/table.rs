//! The table tab: toolbar, filters and query controls, the editable grid
//! (sort, resize, collapse, selection, staged edits and deletes, the row
//! context menu and the staged-row hover card), the row inspector, the
//! pending-changes bar and the status bar with paging.

use std::cell::RefCell;
use std::sync::atomic::Ordering;

use dbm_core::cell_values::{editable_text, numeric_column};
use dbm_core::models::{FilterOperator, TableColumn, TablePage};
use dbm_core::sql_text::{display_value, inline_diff};
use dbm_ui::a11y::{Node, Role};
use dbm_ui::icons::{self, Icon};
use dbm_ui::input::{Key, Modifiers};
use dbm_ui::text_edit::TextEdit;
use dbm_ui::widgets::{self, icon_button, icon_button_enabled, primary_button, secondary_button};
use dbm_ui::{
    Color32, CursorIcon, Dir, Id, Layer, Radius, Rect, ScrollArea, Sense, Stroke, Ui, Vec2, pos2,
    theme, vec2,
};
use serde_json::Value;
use uuid::Uuid;

use super::grid;
use crate::messages::{self, Message};
use crate::state::{RequestKind, Workbench};
use crate::table::{
    COLLAPSED_COLUMN_WIDTH, FILTER_OPERATORS, FilterDraft, MAX_PREVIEW_ROWS, PENDING_EXPORT_ERROR,
    PENDING_REFRESH_ERROR, PendingRow, PreviewCard, TableState, default_column_width,
    display_prefix, filter_needs_value, large_value_size, operator_label, rows_label, stage_cell,
    toggle_delete,
};

enum Action {
    Reload,
    Save,
    Export,
    /// CSV text and a description such as "3 visible rows".
    Copy(String, String),
    Error(&'static str),
}

struct Cx<'a> {
    tab_id: u64,
    schema: &'a str,
    table: &'a str,
    embedded: bool,
    read_only: bool,
    saving: bool,
    exporting: bool,
}

impl Cx<'_> {
    fn id(&self, name: &str) -> Id {
        Id::new((name, self.tab_id, self.embedded))
    }
}

fn editable(cx: &Cx<'_>, page: &TablePage) -> bool {
    !cx.read_only && !page.metadata.primary_key.is_empty()
}

fn editable_column(cx: &Cx<'_>, page: &TablePage, column: usize) -> bool {
    editable(cx, page)
        && !page
            .metadata
            .primary_key
            .contains(&page.metadata.columns[column].name)
}

fn reload_or_block(state: &TableState) -> Action {
    if state.pending.is_empty() {
        Action::Reload
    } else {
        Action::Error(PENDING_REFRESH_ERROR)
    }
}

fn cell_editor_id(tab_id: u64, row: usize, column: usize) -> Id {
    Id::new(("cell-editor", tab_id, row, column))
}

fn field_editor_id(tab_id: u64, row: usize, column: usize) -> Id {
    Id::new(("field-editor", tab_id, row, column))
}

/// A chrome bar: fill and a hairline on the edge it shares with the grid.
fn bar(ui: &mut Ui, rect: Rect, bottom_line: bool) {
    let mut p = ui.painter();
    p.rect_filled(rect, 0.0, theme::CHROME);
    let y = if bottom_line {
        rect.bottom() - 0.5
    } else {
        rect.top() + 0.5
    };
    p.hline(
        rect.left()..=rect.right(),
        y,
        Stroke::new(1.0, theme::BORDER),
    );
}

impl Workbench {
    pub(super) fn table_ui(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        tab_id: u64,
        profile_id: Uuid,
        embedded: bool,
    ) {
        let Some((_, schema, table)) = self.table_target(tab_id) else {
            return;
        };
        let cx = Cx {
            tab_id,
            schema: &schema,
            table: &table,
            embedded,
            read_only: self.profile(profile_id).is_none_or(|p| p.read_only),
            saving: self.busy(RequestKind::Mutation, Some(tab_id)),
            exporting: self.exporting(tab_id),
        };
        let Some(mut state) = self.tables.remove(&tab_id) else {
            return;
        };
        let actions = show(ui, rect, &cx, &mut state);
        self.tables.insert(tab_id, state);
        for action in actions {
            match action {
                Action::Reload => self.load_table(tab_id),
                Action::Save => self.save_changes(tab_id),
                Action::Copy(text, which) => {
                    ui.ctx.copy_text(&text);
                    self.table_notice(
                        tab_id,
                        Message::notice(format!("Copied {which} as CSV."), self.now),
                    );
                }
                Action::Export => self.request_export(tab_id),
                Action::Error(message) => self.table_error(tab_id, message),
            }
        }
    }
}

fn show(ui: &mut Ui, rect: Rect, cx: &Cx<'_>, state: &mut TableState) -> Vec<Action> {
    let mut actions = Vec::new();
    let (toolbar_rect, rest) = rect.split_top(theme::TOOLBAR_HEIGHT);
    bar(ui, toolbar_rect, true);
    toolbar(
        ui,
        toolbar_rect.inset(14.0, 0.0, 14.0, 1.0),
        cx,
        state,
        &mut actions,
    );
    let Some(page) = state.page.clone() else {
        let mut c = ui.child(rest, Dir::Down);
        message_panel(&mut c, state);
        let area = c.available_rect();
        if state.loading {
            loading_skeleton(&mut c, area);
        } else {
            let b = secondary_button("Couldn't load rows · Retry").icon(Icon::Refresh);
            let w = b.width(&c);
            if b.show_in(&mut c, Rect::from_center_size(area.center(), vec2(w, 30.0)))
                .clicked
            {
                actions.push(Action::Reload);
            }
        }
        return actions;
    };
    let filters_h = filter_panel_height(ui, cx, state, rest.width());
    let (filters_rect, rest) = rest.split_top(filters_h);
    bar(ui, filters_rect, true);
    filter_panel(
        ui,
        filters_rect.inset(12.0, 7.0, 10.0, 7.0),
        cx,
        state,
        &page,
        &mut actions,
    );
    let (status_rect, mut rest) = rest.split_bottom(theme::STATUS_BAR_HEIGHT);
    bar(ui, status_rect, false);
    status_bar(
        ui,
        status_rect.inset(14.0, 0.0, 14.0, 0.0),
        state,
        &page,
        &mut actions,
    );
    if !state.pending.is_empty() {
        let (pending_rect, r) = rest.split_bottom(theme::PENDING_BAR_HEIGHT);
        rest = r;
        bar(ui, pending_rect, false);
        pending_bar(
            ui,
            pending_rect.inset(14.0, 0.0, 14.0, 0.0),
            cx,
            state,
            &mut actions,
        );
    }
    let rest = {
        let mut c = ui.child(rest, Dir::Down);
        message_panel(&mut c, state);
        c.available_rect()
    };
    let interactive = !state.loading && !cx.saving;
    let area = if state.inspector_open {
        let (inspector_rect, left) = rest.split_right(theme::INSPECTOR_WIDTH);
        ui.painter().rect_filled(inspector_rect, 0.0, theme::CHROME);
        ui.painter().vline(
            inspector_rect.left() + 0.5,
            inspector_rect.top()..=inspector_rect.bottom(),
            Stroke::new(1.0, theme::BORDER),
        );
        let mut c = ui.child(inspector_rect, Dir::Down);
        c.set_clip(inspector_rect);
        c.set_enabled(interactive);
        inspector(
            &mut c,
            inspector_rect.with_min_x(inspector_rect.left() + 1.0),
            cx,
            state,
            &page,
        );
        left
    } else {
        rest
    };
    {
        let mut c = ui.child(area, Dir::Down);
        c.set_clip(area);
        c.set_enabled(interactive);
        data_grid(&mut c, area, cx, state, &page, &mut actions);
    }
    loading_overlay(ui, area, state);
    commit_unfocused_drafts(ui, cx.tab_id, state);
    actions
}

fn toolbar(
    ui: &mut Ui,
    rect: Rect,
    cx: &Cx<'_>,
    state: &mut TableState,
    actions: &mut Vec<Action>,
) {
    let mut row = ui.child(rect, Dir::Left);
    row.spacing.x = 6.0;
    let tip = if state.inspector_open {
        "Hide row inspector"
    } else {
        "Show row inspector"
    };
    if widgets::toggle(&mut row, Icon::Inspector, state.inspector_open, tip).clicked {
        state.inspector_open = !state.inspector_open;
    }
    let has_page = state.page.is_some() && !state.loading;
    let selected: Vec<usize> = state.selected.iter().copied().collect();
    if !selected.is_empty() {
        let trigger = widgets::menu_trigger(
            &mut row,
            &format!("{} selected", theme::count(selected.len() as u64)),
            "Actions for the selected rows",
        );
        let mut chosen = None;
        widgets::menu(&mut row, &trigger, 220.0, |ui| {
            chosen = selection_menu(ui, cx, state, &selected, true);
            chosen.is_some()
        });
        apply_menu(chosen, state, &selected, actions);
    }
    let total = state
        .total_rows()
        .map_or_else(String::new, |n| format!(" ({})", theme::count(n)));
    let export_label = if cx.exporting {
        row.ctx.request_repaint_after(0.2);
        let done = state
            .export_progress
            .as_ref()
            .map_or(0, |rows| rows.load(Ordering::Relaxed));
        match state.total_rows() {
            Some(total) => format!(
                "Exporting {} / {}…",
                theme::count(done),
                theme::count(total)
            ),
            None => format!("Exporting {}…", theme::count(done)),
        }
    } else {
        format!("Export all{total}")
    };
    if icon_button_enabled(
        &mut row,
        has_page && !cx.exporting,
        Icon::Download,
        Some(&export_label),
        "Prompts for a location and exports every filtered row",
    )
    .clicked
    {
        actions.push(if state.pending.is_empty() {
            Action::Export
        } else {
            Action::Error(PENDING_EXPORT_ERROR)
        });
    }
    let copyable = state.copyable_rows();
    if icon_button_enabled(
        &mut row,
        has_page,
        Icon::Copy,
        Some(&format!(
            "Copy visible ({})",
            theme::count(copyable.len() as u64)
        )),
        "Copies only the current preview page",
    )
    .clicked
    {
        let label = rows_label(copyable.len(), "visible");
        actions.push(Action::Copy(state.csv(copyable.into_iter()), label));
    }
    let refresh = if state.loading {
        "Refreshing…"
    } else {
        "Refresh"
    };
    if icon_button_enabled(
        &mut row,
        !state.loading,
        Icon::Refresh,
        Some(refresh),
        "Reload the current page with the same filters and sort",
    )
    .clicked
    {
        actions.push(reload_or_block(state));
    }
    // The title takes what is left and truncates before the counts.
    let left = row.available_rect();
    let mut l = row.child(left, Dir::Right);
    l.spacing.x = 8.0;
    widgets::icon(&mut l, Icon::Table, 15.0, theme::MUTED);
    let counts = state.page.as_ref().map(|page| {
        let rows = page.total_rows.map_or_else(|| "—".to_owned(), theme::count);
        format!("{rows} rows · {} columns", page.metadata.columns.len())
    });
    let counts_w = counts
        .as_ref()
        .map_or(0.0, |c| l.ctx.fonts.width(c, theme::ui(12.0)) + 8.0);
    let room = (l.available_width() - counts_w).max(0.0);
    let x = l.cursor().x;
    let y = left.center().y;
    let full = format!("{}.{}", cx.schema, cx.table);
    let used = {
        let mut p = l.painter();
        if cx.embedded {
            p.text_truncated(x, y, &full, theme::bold(13.0), theme::TEXT_STRONG, room)
                .0
        } else {
            let (a, _) = p.text_truncated(x, y, cx.schema, theme::ui(15.0), theme::MUTED, room);
            let (b, _) = p.text_truncated(
                x + a,
                y,
                &format!(".{}", cx.table),
                theme::bold(15.0),
                theme::TEXT_STRONG,
                room - a,
            );
            a + b
        }
    };
    let title_rect = Rect::from_min_size(pos2(x, left.top()), vec2(used, left.height()));
    let tid = l.make_id("table-title");
    let mut node = Node::new(tid.0, Role::Text, &full, title_rect);
    node.focusable = false;
    l.a11y(node);
    l.advance_past(title_rect);
    if let Some(counts) = counts {
        l.label(&counts, theme::ui(12.0), theme::FAINT);
    }
}

/// What a selection menu item asks for.
#[derive(Clone, Copy)]
enum MenuChoice {
    Copy,
    ToggleDelete,
    Clear,
}

/// The "N selected" menu, and (without "Clear selection") the row context
/// menu, as in the desktop.
fn selection_menu(
    ui: &mut Ui,
    cx: &Cx<'_>,
    state: &TableState,
    selected: &[usize],
    with_clear: bool,
) -> Option<MenuChoice> {
    if widgets::menu_item(ui, "Copy selected as CSV", false).clicked {
        return Some(MenuChoice::Copy);
    }
    let page = state.page.as_ref()?;
    // Staged rows are keyed by position, so stage nothing while the page is
    // being replaced or saved.
    if editable(cx, page) && !state.loading && !cx.saving {
        let all_deleted = selected
            .iter()
            .all(|r| state.pending.get(r).is_some_and(|p| p.deleted));
        let label = if all_deleted {
            "Undo staged deletion".to_owned()
        } else if selected.len() == 1 {
            "Stage row for deletion".to_owned()
        } else {
            format!("Stage {} rows for deletion", selected.len())
        };
        if widgets::menu_item(ui, &label, !all_deleted).clicked {
            return Some(MenuChoice::ToggleDelete);
        }
    }
    if with_clear && widgets::menu_item(ui, "Clear selection", false).clicked {
        return Some(MenuChoice::Clear);
    }
    None
}

fn apply_menu(
    choice: Option<MenuChoice>,
    state: &mut TableState,
    selected: &[usize],
    actions: &mut Vec<Action>,
) {
    match choice {
        Some(MenuChoice::Copy) => {
            let rows: Vec<usize> = selected
                .iter()
                .copied()
                .filter(|r| !state.pending.get(r).is_some_and(|p| p.deleted))
                .collect();
            let label = rows_label(rows.len(), "selected");
            actions.push(Action::Copy(state.csv(rows.into_iter()), label));
        }
        Some(MenuChoice::ToggleDelete) => toggle_delete(state, selected),
        Some(MenuChoice::Clear) => {
            state.selected.clear();
            state.anchor = None;
        }
        None => {}
    }
}

fn controls_width_id(cx: &Cx<'_>) -> Id {
    cx.id("filter-controls-width")
}

fn header_width(ui: &Ui) -> f32 {
    let f = &ui.ctx.fonts;
    13.0 + 8.0
        + f.width("Filters", theme::bold(12.0))
        + 8.0
        + f.width("All filters must match", theme::ui(12.0))
        + 8.0
        + 12.0
        + 13.0
        + 4.0
        + f.width("Add filter", theme::ui(12.0))
}

/// The filter panel's height: a header row, the query controls beside it
/// (or on their own row when they don't fit), and one row per filter.
fn filter_panel_height(ui: &Ui, cx: &Cx<'_>, state: &TableState, width: f32) -> f32 {
    let known: f32 = ui.ctx.data(controls_width_id(cx)).unwrap_or(0.0);
    let inner = width - 22.0;
    let wrapped = header_width(ui) + 16.0 + known > inner;
    14.0 + 30.0 + if wrapped { 32.0 } else { 0.0 } + state.filters.len() as f32 * 32.0
}

fn filter_panel(
    ui: &mut Ui,
    rect: Rect,
    cx: &Cx<'_>,
    state: &mut TableState,
    page: &TablePage,
    actions: &mut Vec<Action>,
) {
    let columns: Vec<&str> = page
        .metadata
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    let mut apply = false;
    let known: f32 = ui.ctx.data(controls_width_id(cx)).unwrap_or(0.0);
    let wrapped = header_width(ui) + 16.0 + known > rect.width();
    let mut c = ui.child(rect, Dir::Down);
    c.spacing.y = 2.0;
    c.horizontal(30.0, |ui| {
        ui.spacing.x = 8.0;
        widgets::icon(ui, Icon::Filter, 13.0, theme::FAINT);
        ui.label("Filters", theme::bold(12.0), theme::SECONDARY);
        ui.label("All filters must match", theme::ui(12.0), theme::FAINT);
        if widgets::text_button(
            ui,
            Some(Icon::Plus),
            "Add filter",
            12.0,
            "Add another filter",
        )
        .clicked
        {
            state.filters.push(FilterDraft::new(
                columns.first().copied().unwrap_or_default(),
            ));
        }
    });
    let controls_row = if wrapped {
        c.allocate_row(30.0)
    } else {
        Rect::from_min_size(rect.min, vec2(rect.width(), 30.0))
    };
    let measured = {
        let mut r = c.child(controls_row, Dir::Left);
        r.spacing.x = 6.0;
        query_controls(&mut r, cx, state, page, &columns, actions, &mut apply);
        r.min_rect().width()
    };
    if (measured - known).abs() > 0.5 {
        c.ctx.insert_data(controls_width_id(cx), measured);
        c.ctx.request_rerun();
    }
    let mut remove = None;
    for (index, filter) in state.filters.iter_mut().enumerate() {
        c.horizontal(30.0, |ui| {
            ui.spacing.x = 8.0;
            ui.push_id(("filter", index));
            let mut column = None;
            widgets::combo_box(
                ui,
                Id::new((cx.tab_id, cx.embedded, "filter-column", index)),
                &filter.column,
                170.0,
                |ui| {
                    for name in &columns {
                        if widgets::select_item(ui, name, filter.column == *name).clicked {
                            column = Some((*name).to_owned());
                            return true;
                        }
                    }
                    false
                },
            );
            if let Some(column) = column {
                filter.column = column;
            }
            let mut operator = None;
            widgets::combo_box(
                ui,
                Id::new((cx.tab_id, cx.embedded, "filter-operator", index)),
                operator_label(filter.operator),
                170.0,
                |ui| {
                    for (op, label) in FILTER_OPERATORS {
                        if widgets::select_item(ui, label, filter.operator == op).clicked {
                            operator = Some(op);
                            return true;
                        }
                    }
                    false
                },
            );
            if let Some(op) = operator {
                filter.operator = op;
            }
            let value_width = (ui.available_width() - 40.0).max(120.0);
            if filter_needs_value(filter.operator) {
                let hint = if matches!(filter.operator, FilterOperator::In | FilterOperator::NotIn)
                {
                    "value 1, value 2, …"
                } else {
                    "Value…"
                };
                let out = TextEdit::singleline(
                    Id::new((cx.tab_id, cx.embedded, "filter-value", index)),
                    &mut filter.value,
                )
                .hint(hint)
                .font(theme::ui(12.0))
                .margin(9.0, 6.0)
                .width(value_width)
                .name("Filter value")
                .show(ui);
                if out.submitted {
                    apply = true;
                }
            } else {
                let r = ui.allocate(vec2(value_width, 24.0));
                ui.painter().text_mid(
                    r.left() + 2.0,
                    r.center().y,
                    "No value needed",
                    theme::ui(12.5),
                    theme::FAINT,
                );
            }
            if icon_button(ui, Icon::Close, None, "Remove filter").clicked {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        state.filters.remove(index);
    }
    if apply {
        state.apply_filters();
        actions.push(reload_or_block(state));
    }
}

/// Apply, Clear, Reset columns, direction, sort and preview limit, laid out
/// from the right edge leftwards.
fn query_controls(
    ui: &mut Ui,
    cx: &Cx<'_>,
    state: &mut TableState,
    page: &TablePage,
    columns: &[&str],
    actions: &mut Vec<Action>,
    apply: &mut bool,
) {
    let labeled = |ui: &mut Ui, text: &str| {
        ui.label(text, theme::ui(11.0), theme::FAINT);
    };
    if primary_button("Apply filters")
        .id(cx.id("apply-filters"))
        .show(ui)
        .clicked
    {
        *apply = true;
    }
    if state.has_filters()
        && secondary_button("Clear")
            .id(cx.id("clear-filters"))
            .show(ui)
            .clicked
    {
        state.filters = columns
            .first()
            .map(|c| FilterDraft::new(c))
            .into_iter()
            .collect();
        state.applied.clear();
        state.page_index = 0;
        actions.push(reload_or_block(state));
    }
    if (state.columns_resized || !state.collapsed_columns.is_empty())
        && widgets::text_button(
            ui,
            None,
            "Reset columns",
            12.0,
            "Restore every column's default width",
        )
        .clicked
    {
        state.collapsed_columns.clear();
        state.widths.clear();
    }
    ui.add_space(6.0);
    let effective = state.effective_order();
    let mut descending = effective.as_ref().is_some_and(|o| o.descending);
    if effective.is_some() {
        let mut chosen = None;
        let text = |d: bool| if d { "Descending" } else { "Ascending" };
        let r = widgets::combo_box(ui, cx.id("sort-direction"), text(descending), 142.0, |ui| {
            for value in [false, true] {
                if widgets::select_item(ui, text(value), value == descending).clicked {
                    chosen = Some(value);
                    return true;
                }
            }
            false
        });
        let icon = if descending {
            Icon::ArrowDown
        } else {
            Icon::ArrowUp
        };
        icons::paint(
            &mut ui.painter(),
            Rect::from_center_size(
                pos2(r.rect.right() - 30.0, r.rect.center().y),
                Vec2::splat(12.0),
            ),
            icon,
            theme::MUTED,
        );
        if let Some(d) = chosen {
            descending = d;
        }
        labeled(ui, "Direction");
        ui.add_space(6.0);
    }
    let mut column = effective
        .as_ref()
        .map(|o| o.column.clone())
        .unwrap_or_default();
    let label = |name: &str| {
        if page.metadata.primary_key.iter().any(|k| k == name) {
            format!("{name} (primary key)")
        } else if name.is_empty() {
            "Choose a sort column".to_owned()
        } else {
            name.to_owned()
        }
    };
    let mut picked = None;
    widgets::combo_box(ui, cx.id("sort-column"), &label(&column), 170.0, |ui| {
        for name in columns {
            if widgets::select_item(ui, &label(name), column == *name).clicked {
                picked = Some((*name).to_owned());
                return true;
            }
        }
        false
    });
    if let Some(p) = picked {
        column = p;
    }
    labeled(ui, "Sort by");
    ui.add_space(6.0);
    let changed_column = effective.as_ref().map(|o| o.column.as_str()) != Some(column.as_str())
        && !column.is_empty();
    let changed_direction = effective
        .as_ref()
        .is_some_and(|o| o.descending != descending);
    if changed_column || changed_direction {
        state.order = Some(dbm_core::models::OrderSpec {
            column,
            descending: descending && !changed_column,
        });
        state.page_index = 0;
        actions.push(reload_or_block(state));
    }
    // `.limit-input-wrap`: an 84×28 field with a 22 px stepper column.
    let rect = ui.allocate(vec2(84.0, 28.0));
    let field = Rect::from_ltrb(rect.left(), rect.top(), rect.right() - 23.0, rect.bottom());
    ui.painter().rect_filled(rect, 6.0, theme::CONTROL);
    let out = TextEdit::singleline(cx.id("limit"), &mut state.limit_input)
        .frameless()
        .font(theme::ui(12.0))
        .margin(9.0, 6.0)
        .name("Preview limit")
        .show_in(ui, field);
    let focused = out.response.has_focus;
    ui.painter().rect_stroke(
        rect,
        6.0,
        Stroke::new(
            1.0,
            if focused {
                theme::ACCENT
            } else {
                theme::BORDER_STRONG
            },
        ),
    );
    let step = limit_stepper(
        ui,
        cx,
        Rect::from_ltrb(
            rect.right() - 23.0,
            rect.top() + 1.0,
            rect.right() - 1.0,
            rect.bottom() - 1.0,
        ),
    );
    if out.response.lost_focus || out.submitted || step != 0 {
        let limit = state
            .limit_input
            .trim()
            .parse::<i64>()
            .map_or(i64::from(state.limit), |n| n + step)
            .clamp(1, i64::from(MAX_PREVIEW_ROWS)) as u32;
        state.limit_input = limit.to_string();
        if limit != state.limit {
            state.limit = limit;
            state.page_index = 0;
            actions.push(reload_or_block(state));
        }
    }
    labeled(ui, "Preview limit");
}

/// `.limit-stepper`: up/down halves split by hairlines. Returns +1, -1 or 0.
fn limit_stepper(ui: &mut Ui, cx: &Cx<'_>, column: Rect) -> i64 {
    let divider = Stroke::new(1.0, theme::BORDER_STRONG);
    ui.painter()
        .vline(column.left() + 0.5, column.top()..=column.bottom(), divider);
    let mut step = 0;
    let half = column.height() / 2.0;
    for (index, (icon, amount, tip)) in [
        (Icon::ChevronUp, 1, "Increase preview limit"),
        (Icon::ChevronDown, -1, "Decrease preview limit"),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::from_min_size(
            pos2(column.left() + 1.0, column.top() + half * index as f32),
            vec2(column.width() - 1.0, half),
        );
        let id = cx.id("limit-step").with(index);
        let r = ui.interact(rect, id, Sense::CLICK).on_hover_text(tip);
        let mut node = Node::new(id.0, Role::Button, tip, rect);
        node.focusable = true;
        ui.a11y(node);
        let mut p = ui.painter();
        if r.hovered {
            let corner = if index == 0 {
                Radius {
                    ne: 5.0,
                    ..Radius::ZERO
                }
            } else {
                Radius {
                    se: 5.0,
                    ..Radius::ZERO
                }
            };
            p.rect_filled(rect, corner, Color32::from_white_alpha(15));
        }
        icons::paint(
            &mut p,
            Rect::from_center_size(rect.center(), Vec2::splat(10.0)),
            icon,
            if r.hovered { theme::TEXT } else { theme::MUTED },
        );
        if r.clicked {
            step = amount;
        }
    }
    ui.painter()
        .hline(column.left()..=column.right(), column.center().y, divider);
    step
}

fn status_bar(
    ui: &mut Ui,
    rect: Rect,
    state: &mut TableState,
    page: &TablePage,
    actions: &mut Vec<Action>,
) {
    // `.status-bar`: 11.5 px #8e8e93; the page label is `--secondary`.
    let font = theme::ui(11.5);
    let muted = Color32::from_rgb(0x8e, 0x8e, 0x93);
    let total = page
        .total_rows
        .map_or_else(String::new, |n| format!(" of {}", theme::count(n)));
    let rows = if page.rows.is_empty() {
        format!("No rows{total}")
    } else {
        format!(
            "Rows {}–{}{total}",
            theme::count(page.offset + 1),
            theme::count(page.offset + page.rows.len() as u32)
        )
    };
    ui.painter()
        .text_mid(rect.left(), rect.center().y, &rows, font, muted);
    let mut row = ui.child(rect, Dir::Left);
    row.spacing.x = 4.0;
    let clean = state.pending.is_empty();
    if page.offset > 0 || page.has_more {
        if icon_button_enabled(
            &mut row,
            page.has_more && clean && !state.loading,
            Icon::ChevronRight,
            None,
            "Next page",
        )
        .clicked
        {
            state.page_index += 1;
            actions.push(Action::Reload);
        }
        let current = page.offset / page.limit.max(1) + 1;
        let pages = page
            .total_rows
            .map(|n| n.div_ceil(u64::from(page.limit.max(1))).max(1));
        let label = match pages {
            Some(pages) => format!("Page {} of {}", theme::count(current), theme::count(pages)),
            None => format!("Page {}", theme::count(current)),
        };
        row.label(&label, font, theme::SECONDARY);
        if icon_button_enabled(
            &mut row,
            page.offset > 0 && clean && !state.loading,
            Icon::ChevronLeft,
            None,
            "Previous page",
        )
        .clicked
        {
            state.page_index = state.page_index.saturating_sub(1);
            actions.push(Action::Reload);
        }
        row.add_space(8.0);
    }
    row.label(&format!("{} per page", page.limit), font, muted);
}

fn pending_bar(
    ui: &mut Ui,
    rect: Rect,
    cx: &Cx<'_>,
    state: &mut TableState,
    actions: &mut Vec<Action>,
) {
    let count = state.pending.len();
    let deleted = state.pending.values().filter(|p| p.deleted).count();
    let edited = count - deleted;
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    {
        let mut row = ui.child(rect, Dir::Left);
        row.spacing.x = 8.0;
        let save = if cx.saving {
            "Saving…".to_owned()
        } else {
            format!("Save changes ({count})")
        };
        if primary_button(&save)
            .enabled(!cx.read_only && !cx.saving)
            .id(cx.id("save"))
            .show(&mut row)
            .clicked
        {
            actions.push(Action::Save);
        }
        if secondary_button("Discard changes")
            .enabled(!cx.saving)
            .id(cx.id("discard"))
            .show(&mut row)
            .clicked
        {
            state.pending.clear();
            state.drafts.clear();
            state.editing = None;
        }
    }
    let mut row = ui.child(rect, Dir::Right);
    row.spacing.x = 8.0;
    let dot = row.allocate(Vec2::splat(10.0));
    row.painter()
        .circle_filled(dot.center(), 4.0, theme::MODIFIED);
    row.label(
        &plural(count, "pending change", "pending changes"),
        theme::bold(13.0),
        theme::TEXT_STRONG,
    );
    if edited > 0 {
        widgets::chip(
            &mut row,
            &plural(edited, "edited row", "edited rows"),
            theme::MODIFIED,
        );
    }
    if deleted > 0 {
        widgets::chip(
            &mut row,
            &plural(deleted, "deletion", "deletions"),
            theme::DANGER,
        );
    }
    if cx.read_only {
        widgets::chip(&mut row, "Read-only connection", theme::MUTED);
    }
}

/// The inline error, notice, or export result above the grid.
fn message_panel(ui: &mut Ui, state: &mut TableState) {
    let Some(message) = &state.message else {
        return;
    };
    let now = ui.ctx.time();
    let Some(opacity) = message.opacity(ui.ctx, now) else {
        state.message = None;
        return;
    };
    let response = messages::inline(ui, message, opacity);
    ui.add_space(8.0);
    let path = message.export.as_ref().map(|(path, _)| path.clone());
    match (response, path) {
        (messages::Response::Dismiss, _) => state.message = None,
        (messages::Response::Open, Some(path)) => crate::platform::open_path(&path, false),
        (messages::Response::Reveal, Some(path)) => crate::platform::open_path(&path, true),
        _ => {}
    }
}

/// `.initial-grid-skeleton`: a header band and five rows with a moving
/// highlight while the first page loads.
fn loading_skeleton(ui: &mut Ui, area: Rect) {
    let time = ui.ctx.time();
    ui.ctx.request_repaint_after(1.0 / 30.0);
    let width = area.width();
    let mut p = ui.painter_at(area);
    let mut band = |y: f32, height: f32, base: Color32| {
        let rect = Rect::from_min_size(pos2(area.left(), y), vec2(width, height));
        p.rect_filled(rect, 0.0, base);
        let phase = (time % 1.5 / 1.5) as f32;
        let center = rect.left() + (phase * 2.2 - 0.6) * width;
        let glow = Rect::from_center_size(pos2(center, rect.center().y), vec2(width * 0.5, height))
            .intersect(rect);
        if glow.is_positive() {
            p.rect_filled(glow, 0.0, Color32::from_white_alpha(5));
        }
        p.hline(
            rect.left()..=rect.right(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, theme::HAIRLINE),
        );
    };
    band(area.top(), theme::HEADER_HEIGHT, theme::GRID_HEADER);
    for row in 0..5 {
        band(
            area.top() + theme::HEADER_HEIGHT + row as f32 * theme::ROW_HEIGHT,
            theme::ROW_HEIGHT,
            Color32::from_white_alpha(4),
        );
    }
}

/// Dims the grid and shows a "Refreshing…" pill once a reload of an
/// already visible page has taken longer than 120 ms.
fn loading_overlay(ui: &mut Ui, area: Rect, state: &mut TableState) {
    if !state.loading {
        state.loading_since = None;
        return;
    }
    let now = ui.ctx.time();
    let since = *state.loading_since.get_or_insert(now);
    if now - since < 0.12 {
        ui.ctx.request_repaint_after(0.12 - (now - since));
        return;
    }
    let fade = (((now - since - 0.12) / 0.18) as f32).clamp(0.0, 1.0);
    if fade < 1.0 {
        ui.ctx.request_repaint_after(1.0 / 60.0);
    }
    let font = theme::ui(12.0);
    let w = ui.ctx.fonts.width("Refreshing…", font);
    let mut p = ui.painter_at(area);
    p.rect_filled(
        area,
        0.0,
        Color32::from_rgba_unmultiplied(22, 22, 24, 89).gamma_multiply(fade),
    );
    let pill = Rect::from_center_size(area.center(), vec2(w + 24.0, font.line_height() + 12.0));
    p.rect(
        pill,
        pill.height() / 2.0,
        theme::POPOVER.gamma_multiply(fade),
        Stroke::new(1.0, theme::BORDER_STRONG.gamma_multiply(fade)),
    );
    p.text_mid(
        pill.left() + 12.0,
        pill.center().y,
        "Refreshing…",
        font,
        theme::SECONDARY.gamma_multiply(fade),
    );
}

/// Where a header's collapse button sits.
fn collapse_button_rect(header: Rect) -> Rect {
    Rect::from_center_size(header.right_center() - vec2(16.0, 0.0), Vec2::splat(24.0))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HeaderClick {
    Sort,
    Toggle,
}

/// Column header: key glyph, name, type, sort indicator and a collapse
/// button on hover. Collapsed columns show a narrow strip that expands on
/// click.
fn header_cell(
    ui: &mut Ui,
    rect: Rect,
    name: &str,
    data_type: &str,
    primary_key: bool,
    sort: Option<bool>,
    collapsed: bool,
) -> Option<HeaderClick> {
    let id = ui.make_id(("header", name));
    if collapsed {
        let r = ui
            .interact(rect, id.with("expand"), Sense::CLICK)
            .on_hover_text(format!("Expand {name}"));
        let mut node = Node::new(id.0, Role::Button, format!("Expand {name}"), rect);
        node.focusable = true;
        ui.a11y(node);
        let mut p = ui.painter();
        icons::paint(
            &mut p,
            Rect::from_center_size(rect.center() - vec2(0.0, 6.0), Vec2::splat(12.0)),
            Icon::Expand,
            theme::MUTED,
        );
        let font = theme::ui(9.5);
        let (shown, _) = p.fonts().truncate(name, font, rect.width() - 8.0);
        let w = p.fonts().width(&shown, font);
        p.text(
            pos2(rect.center().x - w / 2.0, rect.center().y + 2.0),
            shown,
            font,
            theme::FAINT,
        );
        return r.clicked.then_some(HeaderClick::Toggle);
    }
    let hovered = ui.rect_contains_pointer(rect);
    let button = collapse_button_rect(rect);
    let sort_rect = Rect::from_ltrb(rect.left(), rect.top(), button.left() - 4.0, rect.bottom());
    let sort_hovered = ui.rect_contains_pointer(sort_rect);
    let indicator = match sort {
        Some(true) => Some((Icon::ArrowDown, theme::ACCENT_TEXT)),
        Some(false) => Some((Icon::ArrowUp, theme::ACCENT_TEXT)),
        None if sort_hovered => Some((Icon::Sort, Color32::from_rgb(0x5c, 0x5c, 0x62))),
        None => None,
    };
    let limit = sort_rect.right() - 4.0 - if indicator.is_some() { 17.0 } else { 0.0 };
    let mut x = rect.left() + 10.0;
    {
        let mut p = ui.painter();
        if sort_hovered {
            p.rect_filled(sort_rect, 0.0, Color32::from_white_alpha(8));
        }
        if let Some((icon, color)) = indicator {
            icons::paint(
                &mut p,
                Rect::from_center_size(
                    pos2(sort_rect.right() - 10.0, rect.center().y),
                    Vec2::splat(12.0),
                ),
                icon,
                color,
            );
        }
        if primary_key {
            icons::paint(
                &mut p,
                Rect::from_center_size(pos2(x + 5.5, rect.center().y), Vec2::splat(11.0)),
                Icon::Key,
                theme::MODIFIED,
            );
            x += 16.0;
        }
        if limit > x {
            let color = if sort.is_some() {
                theme::TEXT_STRONG
            } else {
                theme::TEXT
            };
            let (w, _) =
                p.text_truncated(x, rect.center().y, name, theme::ui(12.0), color, limit - x);
            x += w + 5.0;
        }
        if !data_type.is_empty() && limit - x > 12.0 {
            p.text_truncated(
                x,
                rect.center().y,
                data_type,
                theme::ui(11.0),
                theme::FAINT,
                limit - x,
            );
        }
    }
    let sort_click = ui
        .interact(sort_rect, id.with("sort"), Sense::CLICK)
        .on_hover_text(format!("Sort by {name}"));
    let mut node = Node::new(id.0, Role::Header, name, rect);
    node.focusable = true;
    ui.a11y(node);
    if hovered {
        let toggle = ui
            .interact(button, id.with("collapse"), Sense::CLICK)
            .on_hover_text(format!("Collapse {name}"));
        let mut p = ui.painter();
        if toggle.hovered {
            p.rect_filled(button, 6.0, theme::CONTROL_HOVER);
        }
        icons::paint(
            &mut p,
            button.shrink(6.0),
            Icon::Collapse,
            if toggle.hovered {
                theme::TEXT
            } else {
                theme::FAINT
            },
        );
        if toggle.clicked {
            return Some(HeaderClick::Toggle);
        }
    }
    sort_click.clicked.then_some(HeaderClick::Sort)
}

#[derive(Default)]
struct GridEvents {
    sort: Option<String>,
    toggle: Option<usize>,
    clicked: Option<usize>,
    context: Option<(usize, dbm_ui::Rect)>,
    edit: Option<(usize, usize)>,
    hovered_staged: Option<(usize, Rect)>,
    next_action: Option<usize>,
}

fn data_grid(
    ui: &mut Ui,
    area: Rect,
    cx: &Cx<'_>,
    state: &mut TableState,
    page: &TablePage,
    actions: &mut Vec<Action>,
) {
    let tab_id = cx.tab_id;
    let columns = &page.metadata.columns;
    if page.rows.is_empty() {
        let text = "No rows match this view.";
        let w = ui.ctx.fonts.width(text, theme::ui(12.5));
        ui.painter().text_mid(
            area.center().x - w / 2.0,
            area.center().y,
            text,
            theme::ui(12.5),
            theme::MUTED,
        );
        return;
    }
    let numeric: Vec<bool> = columns
        .iter()
        .map(|c| numeric_column(&c.data_type))
        .collect();
    let order = state.effective_order();
    let action_id = cx.id("column-action");
    let action: Option<usize> = ui.ctx.data(action_id);
    let mut widths: Vec<f32> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if state.collapsed_columns.contains(&i) {
                COLLAPSED_COLUMN_WIDTH
            } else {
                state
                    .widths
                    .get(&i)
                    .copied()
                    .unwrap_or_else(|| default_column_width(&c.data_type))
            }
        })
        .collect();
    let lefts: Vec<f32> = widths
        .iter()
        .scan(0.0, |x, w| {
            let l = *x;
            *x += w;
            Some(l)
        })
        .collect();
    let modifiers = ui.ctx.modifiers();
    let events = RefCell::new(GridEvents::default());
    let mut drafts = std::mem::take(&mut state.drafts);
    let mut focus_editor = state.focus_editor;
    {
        let st: &TableState = state;
        grid::grid(
            ui,
            area,
            cx.id("grid"),
            &mut widths,
            page.rows.len(),
            |ui, column, rect| {
                let c = &columns[column];
                let collapsed = st.collapsed_columns.contains(&column);
                if action == Some(column) {
                    ui.painter()
                        .rect_filled(rect, 0.0, Color32::from_rgb(0x23, 0x2a, 0x36));
                }
                let on_action = if collapsed {
                    ui.rect_contains_pointer(rect)
                } else {
                    ui.rect_contains_pointer(collapse_button_rect(rect))
                };
                if on_action {
                    events.borrow_mut().next_action = Some(column);
                }
                let sort = order
                    .as_ref()
                    .filter(|o| o.column == c.name)
                    .map(|o| o.descending);
                let key = page.metadata.primary_key.contains(&c.name);
                match header_cell(ui, rect, &c.name, &c.data_type, key, sort, collapsed) {
                    Some(HeaderClick::Sort) => events.borrow_mut().sort = Some(c.name.clone()),
                    Some(HeaderClick::Toggle) => events.borrow_mut().toggle = Some(column),
                    None => {}
                }
                if action.is_some_and(|a| a != column) {
                    ui.painter()
                        .rect_filled(rect, 0.0, theme::GRID_HEADER.gamma_multiply(0.62));
                }
            },
            |ui, index, rect, ev| {
                let pending = st.pending.get(&index);
                let deleted = pending.is_some_and(|p| p.deleted);
                let modified = pending.is_some_and(|p| !p.deleted);
                let selected = st.selected.contains(&index);
                let mut p = ui.painter();
                if selected {
                    p.rect_filled(rect, 0.0, theme::ACCENT_SOFT);
                }
                if deleted {
                    p.rect_filled(rect, 0.0, theme::DANGER_SOFT);
                } else if modified {
                    p.rect_filled(rect, 0.0, theme::MODIFIED_ROW_SOFT);
                }
                let edge = if deleted {
                    Some(theme::DANGER)
                } else if modified {
                    Some(theme::MODIFIED)
                } else if selected {
                    Some(theme::ACCENT)
                } else {
                    None
                };
                if let Some(color) = edge {
                    p.rect_filled(
                        Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                        0.0,
                        color,
                    );
                }
                let mut e = events.borrow_mut();
                if pending.is_some() && ev.hovered {
                    e.hovered_staged = Some((index, rect));
                }
                if ev.clicked {
                    e.clicked = Some(index);
                }
                if ev.secondary_clicked {
                    e.context = Some((index, rect));
                }
                if ev.double_clicked
                    && let Some(px) = ui.ctx.pointer()
                {
                    let x = px.x - rect.left();
                    if let Some(column) = lefts.iter().rposition(|l| *l <= x)
                        && editable_column(cx, page, column)
                        && !deleted
                    {
                        e.edit = Some((index, column));
                    }
                }
            },
            |ui, index, column, rect| {
                let pending = st.pending.get(&index);
                let deleted = pending.is_some_and(|p| p.deleted);
                let values = pending.map_or(&page.rows[index][..], |p| &p.changes[..]);
                if action == Some(column) {
                    ui.painter()
                        .rect_filled(rect, 0.0, theme::ACCENT.gamma_multiply(0.06));
                }
                let changed = pending.is_some_and(|p| p.changes[column] != p.original[column]);
                if changed && !deleted {
                    let mut p = ui.painter();
                    p.rect_filled(rect, 0.0, theme::MODIFIED_SOFT);
                    p.circle_filled(rect.right_top() + vec2(-6.0, 6.0), 2.5, theme::MODIFIED);
                }
                if st.editing == Some((index, column)) {
                    {
                        let mut p = ui.painter();
                        p.rect_filled(rect, 0.0, theme::EDIT_SURFACE);
                        p.rect_stroke(rect.shrink(1.75), 0.0, Stroke::new(1.5, theme::ACCENT));
                    }
                    let current = values.get(column).map_or_else(String::new, editable_text);
                    let draft = drafts.entry((index, column)).or_insert(current);
                    let id = cell_editor_id(tab_id, index, column);
                    TextEdit::singleline(id, draft)
                        .frameless()
                        .font(theme::ui(12.0))
                        .margin(10.0, 7.0)
                        .name(&columns[column].name)
                        .show_in(ui, rect);
                    if focus_editor {
                        ui.ctx.request_focus(id);
                        focus_editor = false;
                    }
                    return;
                }
                let value = values.get(column).unwrap_or(&Value::Null);
                let color = deleted.then_some(theme::DANGER);
                if st.collapsed_columns.contains(&column) {
                    let (text, font, base) = grid::cell_text(value);
                    ui.painter().text_truncated(
                        rect.left() + 10.0,
                        rect.center().y,
                        &text,
                        font,
                        color.unwrap_or(base),
                        rect.width() - 14.0,
                    );
                } else {
                    grid::value_cell(
                        ui,
                        rect,
                        value,
                        color,
                        deleted,
                        numeric[column],
                        pending.is_none(),
                    );
                }
                if action.is_some_and(|a| a != column) {
                    ui.painter()
                        .rect_filled(rect, 0.0, theme::BG.gamma_multiply(0.62));
                }
            },
        );
    }
    state.drafts = drafts;
    state.focus_editor = focus_editor;
    // Widths dragged this frame persist; collapsed columns keep theirs.
    for (i, w) in widths.iter().enumerate() {
        if !state.collapsed_columns.contains(&i)
            && (*w - default_column_width(&columns[i].data_type)).abs() > 0.5
        {
            state.widths.insert(i, *w);
        }
    }
    state.columns_resized = !state.widths.is_empty();
    let e = events.into_inner();
    if let Some(index) = e.toggle
        && !state.collapsed_columns.remove(&index)
    {
        state.collapsed_columns.insert(index);
    }
    if e.next_action != action {
        match e.next_action {
            Some(column) => ui.ctx.insert_data(action_id, column),
            None => ui.ctx.remove_data(action_id),
        }
        ui.ctx.request_repaint();
    }
    let mut discard = None;
    update_preview(ui, cx, state, columns, e.hovered_staged, &mut discard);
    if let Some(row) = discard {
        state.discard_row(row);
    }
    if let Some(column) = e.sort {
        if state.pending.is_empty() {
            state.sort_by(column);
            actions.push(Action::Reload);
        } else {
            actions.push(Action::Error(PENDING_REFRESH_ERROR));
        }
    }
    let context_id = cx.id("grid-context");
    if let Some((index, rect)) = e.context {
        if !state.selected.contains(&index) {
            state.selected = std::collections::BTreeSet::from([index]);
            state.anchor = Some(index);
        }
        let target = dbm_ui::Response {
            id: context_id,
            rect,
            secondary_clicked: true,
            ..Default::default()
        };
        widgets::context_menu(ui, &target, 200.0, |_| false);
    } else {
        let target = dbm_ui::Response {
            id: context_id,
            rect: area,
            ..Default::default()
        };
        let selected: Vec<usize> = state.selected.iter().copied().collect();
        let mut chosen = None;
        widgets::context_menu(ui, &target, 200.0, |ui| {
            chosen = selection_menu(ui, cx, state, &selected, false);
            chosen.is_some()
        });
        apply_menu(chosen, state, &selected, actions);
    }
    if let Some(index) = e.clicked {
        state.click_row(index, modifiers.shift, modifiers.ctrl);
    }
    if let Some(cell) = e.edit {
        state.selected = std::collections::BTreeSet::from([cell.0]);
        state.anchor = Some(cell.0);
        let value = state
            .pending
            .get(&cell.0)
            .map_or(&page.rows[cell.0][cell.1], |p| &p.changes[cell.1]);
        if let Some(size) = large_value_size(value) {
            let text = format!(
                "This {size} value is too large to edit in the grid; update it with a query."
            );
            state.message = Some(Message::error(text, ui.ctx.time()));
        } else {
            state.editing = Some(cell);
            state.focus_editor = true;
            ui.ctx.request_repaint();
        }
    }
    // Delete toggles staged deletion for the selection; Escape clears it.
    // Only when no text field has focus.
    let free = ui.ctx.focused().is_none() && ui.ctx.focused_at_frame_start().is_none();
    if free
        && ui.is_enabled()
        && !state.selected.is_empty()
        && editable(cx, page)
        && ui.ctx.consume_key(Modifiers::NONE, Key::Delete)
    {
        let rows: Vec<usize> = state.selected.iter().copied().collect();
        toggle_delete(state, &rows);
    }
    if free && ui.ctx.key_pressed(Key::Escape) {
        state.selected.clear();
    }
}

/// Opens, keeps, or closes the staged-row card: it follows the hovered
/// staged row, stays while the pointer is on the card, and closes 140 ms
/// after the pointer leaves both.
fn update_preview(
    ui: &mut Ui,
    cx: &Cx<'_>,
    state: &mut TableState,
    columns: &[TableColumn],
    hovered: Option<(usize, Rect)>,
    discard_row: &mut Option<usize>,
) {
    let now = ui.ctx.time();
    if let Some((row, anchor)) = hovered {
        match &mut state.preview {
            Some(card) if card.row != row && card.close_at.is_some() => {}
            Some(card) if card.row == row => {
                card.anchor = anchor;
                card.close_at = None;
            }
            _ => {
                state.preview = Some(PreviewCard {
                    row,
                    anchor,
                    close_at: None,
                })
            }
        }
    }
    let Some(mut card) = state.preview else {
        return;
    };
    let Some(pending) = state.pending.get(&card.row) else {
        state.preview = None;
        return;
    };
    let (over_card, discard) =
        change_preview_card(ui, cx.id("change-preview"), card.anchor, columns, pending);
    if discard {
        *discard_row = Some(card.row);
        state.preview = None;
        return;
    }
    if over_card || hovered.is_some_and(|(row, _)| row == card.row) {
        card.close_at = None;
    } else if let Some(close_at) = card.close_at {
        if now >= close_at {
            state.preview = None;
            return;
        }
        ui.ctx.request_repaint_after(close_at - now);
    } else {
        card.close_at = Some(now + 0.14);
        ui.ctx.request_repaint_after(0.14);
    }
    state.preview = Some(card);
}

/// The desktop's `ChangePreview`: a card under the staged row with Before →
/// After per changed column (changed characters marked) or the pending
/// delete, and a button that discards the change. Returns whether the
/// pointer is over the card and whether the button was clicked.
fn change_preview_card(
    ui: &mut Ui,
    id: Id,
    anchor: Rect,
    columns: &[TableColumn],
    pending: &PendingRow,
) -> (bool, bool) {
    let screen = ui.ctx.screen_rect();
    let changes: Vec<(&str, String, String)> = columns
        .iter()
        .enumerate()
        .filter(|(index, _)| pending.changes[*index] != pending.original[*index])
        .map(|(index, column)| {
            (
                column.name.as_str(),
                display_value(&pending.original[index]),
                display_value(&pending.changes[index]),
            )
        })
        .collect();
    let longest = changes
        .iter()
        .map(|(_, b, a)| b.chars().count().max(a.chars().count()))
        .max()
        .unwrap_or(0);
    let ideal = longest.min(42) as f32 * 7.0 * 2.0 + 90.0;
    let width = ideal.clamp(380.0, 680.0).min(screen.width() - 32.0);
    let left = (anchor.left() + 16.0).clamp(
        screen.left() + 16.0,
        (screen.right() - width - 16.0).max(screen.left() + 16.0),
    );
    let space_below = (screen.bottom() - anchor.bottom() - 16.0).max(0.0);
    let space_above = (anchor.top() - screen.top() - 16.0).max(0.0);
    let below = space_below >= 260.0 || space_below >= space_above;
    let max_height = (if below { space_below } else { space_above }).clamp(140.0, 520.0);
    let height = ui
        .ctx
        .data::<f32>(id.with("height"))
        .unwrap_or(120.0)
        .min(max_height);
    let top = if below {
        anchor.bottom()
    } else {
        anchor.top() - height
    };
    let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
    let mut discard = false;
    let mut measured = 0.0;
    ui.area(Layer::Popup, rect, |ui| {
        let _ = ui.interact(rect, id.with("bg"), Sense::HOVER);
        {
            let mut p = ui.painter();
            p.shadow(
                rect,
                10.0,
                vec2(0.0, 8.0),
                24.0,
                Color32::from_black_alpha(110),
            );
            p.rect(
                rect,
                10.0,
                theme::POPOVER,
                Stroke::new(1.0, theme::BORDER_STRONG),
            );
        }
        let inner = rect.shrink(12.0);
        let mut c = ui.child(inner, Dir::Down);
        c.set_clip(rect);
        c.spacing.y = 10.0;
        c.horizontal(24.0, |ui| {
            ui.spacing.x = 8.0;
            let dot = ui.allocate(Vec2::splat(7.0));
            ui.painter().circle_filled(
                dot.center(),
                3.5,
                if pending.deleted {
                    theme::DANGER
                } else {
                    theme::MODIFIED
                },
            );
            ui.label(
                if pending.deleted {
                    "Pending delete"
                } else {
                    "Pending edit"
                },
                theme::bold(12.5),
                theme::TEXT_STRONG,
            );
            ui.right_to_left(|ui| {
                let label = if pending.deleted {
                    "Undo delete"
                } else {
                    "Discard edit"
                };
                if secondary_button(label)
                    .small()
                    .id(id.with("discard"))
                    .show(ui)
                    .clicked
                {
                    discard = true;
                }
            });
        });
        if pending.deleted {
            c.label_wrapped(
                "This row will be deleted when changes are saved.",
                theme::ui(12.0),
                theme::MUTED,
                false,
            );
        } else {
            for (name, before, after) in &changes {
                change_diff(&mut c, name, before, after);
            }
        }
        measured = c.min_rect().height() + 24.0;
    });
    if (measured - height).abs() > 0.5 && measured > 0.0 {
        ui.ctx
            .insert_data(id.with("height"), measured.min(max_height));
        ui.ctx.request_rerun();
    }
    (ui.rect_contains_pointer(rect), discard)
}

/// `.change-diff`: a rule, the column name, then Before → After boxes.
fn change_diff(ui: &mut Ui, name: &str, before: &str, after: &str) {
    let diff = inline_diff(before, after);
    ui.separator(theme::BORDER);
    ui.label(name, theme::ui(11.5), theme::SECONDARY);
    let avail = ui.available_rect();
    let box_w = ((avail.width() - 32.0) / 2.0).max(60.0);
    let font = theme::ui(11.5);
    let caption = theme::ui(11.0);
    let lh = font.line_height();
    let before_lines = diff_lines(
        ui,
        [&diff.prefix, &diff.removed, &diff.suffix],
        font,
        box_w - 16.0,
    );
    let after_lines = diff_lines(
        ui,
        [&diff.prefix, &diff.added, &diff.suffix],
        font,
        box_w - 16.0,
    );
    let rows = before_lines.len().max(after_lines.len()).max(1);
    let box_h = rows as f32 * lh + 12.0;
    let total = caption.line_height() + 4.0 + box_h;
    let mut p = ui.painter();
    let draw = |p: &mut dbm_ui::Painter,
                x: f32,
                label: &str,
                lines: &[Vec<(String, bool)>],
                fill: Color32,
                color: Color32,
                mark: Color32| {
        p.text(pos2(x, avail.top()), label, caption, theme::FAINT);
        let r = Rect::from_min_size(
            pos2(x, avail.top() + caption.line_height() + 4.0),
            vec2(box_w, box_h),
        );
        p.rect_filled(r, 6.0, fill);
        for (i, line) in lines.iter().enumerate() {
            let mut cx = r.left() + 8.0;
            let y = r.top() + 6.0 + i as f32 * lh;
            for (text, marked) in line {
                let w = p.fonts().width(text, font);
                if *marked {
                    p.rect_filled(Rect::from_min_size(pos2(cx, y), vec2(w, lh)), 2.0, mark);
                }
                p.text(pos2(cx, y), text.clone(), font, color);
                cx += w;
            }
        }
    };
    draw(
        &mut p,
        avail.left(),
        "Before",
        &before_lines,
        Color32::from_rgb(0x1e, 0x1e, 0x20),
        theme::MUTED,
        Color32::from_rgba_unmultiplied(255, 107, 97, 56),
    );
    icons::paint(
        &mut p,
        Rect::from_center_size(
            pos2(
                avail.left() + box_w + 16.0,
                avail.top() + caption.line_height() + 4.0 + 13.0,
            ),
            Vec2::splat(14.0),
        ),
        Icon::ArrowRight,
        theme::FAINT,
    );
    draw(
        &mut p,
        avail.left() + box_w + 32.0,
        "After",
        &after_lines,
        theme::MODIFIED_SOFT,
        theme::TEXT_STRONG,
        Color32::from_rgba_unmultiplied(240, 177, 76, 82),
    );
    ui.allocate(vec2(avail.width(), total));
}

/// Wraps prefix, changed and suffix text anywhere at `width`, at most 12
/// lines and 600 characters, keeping which runs are marked.
fn diff_lines(
    ui: &Ui,
    parts: [&String; 3],
    font: dbm_ui::Font,
    width: f32,
) -> Vec<Vec<(String, bool)>> {
    const LIMIT: usize = 600;
    let mut lines: Vec<Vec<(String, bool)>> = vec![Vec::new()];
    let mut x = 0.0;
    let mut used = 0;
    let total: usize = parts.iter().map(|p| p.chars().count()).sum();
    for (part, marked) in [(parts[0], false), (parts[1], true), (parts[2], false)] {
        for ch in part.chars() {
            if used >= LIMIT {
                break;
            }
            used += 1;
            let w = ui.ctx.fonts.advance(ch, font);
            if ch == '\n' || x + w > width {
                if lines.len() >= 12 {
                    return lines;
                }
                lines.push(Vec::new());
                x = 0.0;
                if ch == '\n' {
                    continue;
                }
            }
            x += w;
            let line = lines.last_mut().expect("one line");
            match line.last_mut() {
                Some((text, m)) if *m == marked => text.push(ch),
                _ => line.push((ch.to_string(), marked)),
            }
        }
    }
    if total > LIMIT
        && let Some(line) = lines.last_mut()
    {
        line.push(("…".to_owned(), false));
    }
    lines
}

/// Stages drafts whose editor no longer has focus; Escape drops the focused
/// draft instead, reverting the field.
fn commit_unfocused_drafts(ui: &mut Ui, tab_id: u64, state: &mut TableState) {
    let focused = ui.ctx.focused();
    let before = ui.ctx.focused_at_frame_start();
    let escape = ui.ctx.key_pressed(Key::Escape);
    let keys: Vec<(usize, usize)> = state.drafts.keys().copied().collect();
    for key in keys {
        let ids = [
            cell_editor_id(tab_id, key.0, key.1),
            field_editor_id(tab_id, key.0, key.1),
        ];
        let has_focus = focused.is_some_and(|f| ids.contains(&f));
        let opening = state.editing == Some(key) && state.focus_editor;
        if escape && (has_focus || before.is_some_and(|b| ids.contains(&b))) {
            state.drafts.remove(&key);
            if let Some(id) = focused {
                ui.ctx.surrender_focus(id);
            }
            if state.editing == Some(key) {
                state.editing = None;
            }
        } else if !(has_focus || opening) {
            if let Some(text) = state.drafts.remove(&key) {
                stage_cell(state, key.0, key.1, &text);
            }
            if state.editing == Some(key) {
                state.editing = None;
            }
        }
    }
}

fn inspector(ui: &mut Ui, rect: Rect, cx: &Cx<'_>, state: &mut TableState, page: &TablePage) {
    let columns = &page.metadata.columns;
    let single = (state.selected.len() == 1)
        .then(|| state.selected.first().copied())
        .flatten()
        .filter(|&i| i < page.rows.len());
    let pending = single.and_then(|i| state.pending.get(&i).cloned());
    let deleted = pending.as_ref().is_some_and(|p| p.deleted);
    // Header: 40 px, like `.row-inspector-header`.
    let (header, body) = rect.split_top(40.0);
    {
        let mut row = ui.child(header.inset(14.0, 0.0, 8.0, 0.0), Dir::Left);
        if icon_button(&mut row, Icon::Close, None, "Hide row inspector").clicked {
            state.inspector_open = false;
        }
        let left = row.available_rect();
        let mut l = row.child(left, Dir::Right);
        l.spacing.x = 8.0;
        l.label("Row", theme::bold(13.0), theme::TEXT_STRONG);
        if let Some(index) = single {
            let summary = page
                .metadata
                .primary_key
                .iter()
                .filter_map(|key| {
                    let i = columns.iter().position(|c| &c.name == key)?;
                    Some(format!("{key} = {}", display_value(&page.rows[index][i])))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let chip_w = if deleted {
                140.0
            } else if pending.is_some() {
                60.0
            } else {
                0.0
            };
            if !summary.is_empty() {
                let max = (l.available_width() - chip_w).max(0.0);
                let (shown, _) = l.ctx.fonts.truncate(&summary, theme::ui(12.0), max);
                l.label(&shown, theme::ui(12.0), theme::MUTED);
            }
        }
        if deleted {
            widgets::chip(&mut l, "Staged for deletion", theme::DANGER);
        } else if pending.is_some() {
            widgets::chip(&mut l, "Edited", theme::MODIFIED);
        }
    }
    ui.painter().hline(
        rect.left()..=rect.right(),
        header.bottom() - 0.5,
        Stroke::new(1.0, theme::BORDER),
    );
    let Some(index) = single else {
        let message = if state.selected.len() > 1 {
            format!(
                "{} rows selected. Select a single row to inspect it.",
                state.selected.len()
            )
        } else {
            "Select a row to inspect and edit its fields.".to_owned()
        };
        let mut c = ui.child(body.inset(20.0, 28.0, 20.0, 0.0), Dir::Down);
        c.label_wrapped(&message, theme::ui(12.5), theme::FAINT, true);
        return;
    };
    let row = page.rows[index].clone();
    let values: Vec<Value> = pending.as_ref().map_or(row, |p| p.changes.clone());
    let footer = editable(cx, page);
    let (footer_rect, fields) = if footer {
        body.split_bottom(55.0)
    } else {
        (Rect::NOTHING, body)
    };
    {
        let mut c = ui.child(fields, Dir::Down);
        c.set_clip(fields);
        ScrollArea::vertical(cx.id("inspector")).show(&mut c, |ui| {
            let avail = ui.available_rect();
            let mut f = ui.child(avail.inset(14.0, 14.0, 14.0, 0.0), Dir::Down);
            f.spacing.y = 5.0;
            for column_index in 0..columns.len() {
                if column_index > 0 {
                    f.add_space(2.0);
                }
                inspector_field(
                    &mut f,
                    cx,
                    state,
                    page,
                    (index, column_index),
                    &values,
                    deleted,
                );
            }
            let used = f.min_rect();
            ui.advance_past(Rect::from_ltrb(
                avail.left(),
                avail.top(),
                avail.right(),
                used.bottom() + 14.0,
            ));
        });
    }
    if footer {
        ui.painter().hline(
            rect.left()..=rect.right(),
            footer_rect.top() + 0.5,
            Stroke::new(1.0, theme::BORDER),
        );
        let (icon, label, color) = if deleted {
            (Icon::Undo, "Restore row", theme::TEXT)
        } else {
            (Icon::Trash, "Delete row", theme::DANGER)
        };
        let b = footer_rect.inset(14.0, 12.0, 14.0, 13.0);
        if secondary_icon_button(ui, b, icon, label, color).clicked {
            toggle_delete(state, &[index]);
        }
    }
}

/// One `.inspector-field`: name, type, a right-aligned note, then a 26 px
/// input. Read-only fields render as plain muted text.
fn inspector_field(
    ui: &mut Ui,
    cx: &Cx<'_>,
    state: &mut TableState,
    page: &TablePage,
    key: (usize, usize),
    values: &[Value],
    deleted: bool,
) {
    let (index, column_index) = key;
    let column = &page.metadata.columns[column_index];
    let is_key = page.metadata.primary_key.contains(&column.name);
    let original = &page.rows[index][column_index];
    let current = values.get(column_index).unwrap_or(&Value::Null);
    let changed = !deleted && current != original;
    ui.horizontal(18.0, |ui| {
        ui.spacing.x = 6.0;
        ui.label(&column.name, theme::ui(11.5), theme::SECONDARY);
        ui.label(
            &column.data_type,
            theme::ui(11.0),
            Color32::from_rgb(0x6f, 0x6f, 0x76),
        );
        let note = if is_key {
            Some(("Primary key".to_owned(), theme::FAINT))
        } else if changed {
            Some((
                format!("was {}", display_prefix(original, 200)),
                theme::MODIFIED,
            ))
        } else {
            None
        };
        if let Some((note, color)) = note {
            let room = ui.available_width();
            let font = theme::ui(11.0);
            let (shown, _) = ui.ctx.fonts.truncate(&note, font, room.max(0.0));
            let w = ui.ctx.fonts.width(&shown, font);
            let r = ui.max_rect();
            ui.painter()
                .text_mid(r.right() - w, r.center().y, &shown, font, color);
        }
    });
    // Laying out a multi-megabyte value costs seconds, so large values get
    // a read-only preview.
    if let Some(size) = large_value_size(current) {
        let preview = display_prefix(current, 160);
        ui.label_truncated(&format!("{preview}…"), theme::ui(12.0), theme::MUTED);
        ui.label_wrapped(
            &format!("{size} value — too large to edit here; update it with a query."),
            theme::ui(11.0),
            theme::FAINT,
            false,
        );
        return;
    }
    let read_only = !editable_column(cx, page, column_index) || deleted || is_key;
    let mut text = state
        .drafts
        .get(&key)
        .cloned()
        .unwrap_or_else(|| editable_text(current));
    let id = field_editor_id(cx.tab_id, index, column_index);
    let focused = ui.ctx.has_focus(id);
    let (fill, stroke) = if focused && !read_only {
        (theme::EDIT_SURFACE, theme::ACCENT)
    } else if read_only {
        (Color32::TRANSPARENT, Color32::TRANSPARENT)
    } else if changed {
        (
            theme::MODIFIED_SOFT,
            Color32::from_rgba_unmultiplied(240, 177, 76, 115),
        )
    } else {
        (Color32::from_rgb(0x23, 0x23, 0x26), theme::BORDER_STRONG)
    };
    let rect = ui.allocate_row(26.0);
    ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0, stroke));
    let out = TextEdit::singleline(id, &mut text)
        .frameless()
        .interactive(!read_only)
        .margin(6.0, 5.0)
        .font(theme::ui(12.0))
        .text_color(if read_only { theme::MUTED } else { theme::TEXT })
        .hint(if current.is_null() { "NULL" } else { "" })
        .hint_italic()
        .name(&column.name)
        .show_in(ui, rect);
    if out.response.changed && !read_only {
        state.drafts.insert(key, text);
    }
}

/// `.secondary-button`: control fill, strong border, 30 px, icon + label.
fn secondary_icon_button(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    label: &str,
    color: Color32,
) -> dbm_ui::Response {
    let id = ui.make_id(("secondary-icon", label));
    let r = ui.interact(rect, id, Sense::CLICK);
    let mut node = Node::new(id.0, Role::Button, label, rect);
    node.focusable = true;
    ui.a11y(node);
    if r.hovered {
        ui.ctx.set_cursor(CursorIcon::PointingHand);
    }
    let font = theme::ui(12.5);
    let tw = ui.ctx.fonts.width(label, font);
    let mut p = ui.painter();
    p.rect(
        rect,
        7.0,
        if r.hovered {
            theme::CONTROL_HOVER
        } else {
            theme::CONTROL
        },
        Stroke::new(1.0, theme::BORDER_STRONG),
    );
    let start = rect.center().x - (20.0 + tw) / 2.0;
    icons::paint(
        &mut p,
        Rect::from_center_size(pos2(start + 7.0, rect.center().y), Vec2::splat(14.0)),
        icon,
        color,
    );
    p.text_mid(start + 20.0, rect.center().y, label, font, color);
    r
}
