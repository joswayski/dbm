//! Table tabs (and editable `SELECT *` results): toolbar, filters, grid, row
//! inspector, pending-changes bar, and status bar, mirroring
//! `apps/desktop/ui/src/TableView.tsx`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use dbm_core::cell_values::{editable_text, numeric_column, parse_cell_input};
use dbm_core::models::{
    FilterCondition, FilterOperator, OrderSpec, QueryColumn, TablePage, TablePageRequest,
};
use dbm_core::sql_text::{csv_document, display_value, inline_diff};
use eframe::egui::{self, Color32, Frame, Margin, Rect, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;
use uuid::Uuid;

use crate::icons::{self, Icon};
use crate::messages::{self, Message};
use crate::theme::{self, chip, mono, primary_button, ui_font};

pub const MAX_PREVIEW_ROWS: u32 = 200;
const COLLAPSED_COLUMN_WIDTH: f32 = 76.0;
const DEFAULT_COLUMN_WIDTH: f32 = 170.0;
pub const PENDING_EXPORT_ERROR: &str = "Save or discard pending row changes before exporting.";
pub const PENDING_REFRESH_ERROR: &str = "Save or discard pending row changes before refreshing.";

const FILTER_OPERATORS: [(FilterOperator, &str); 13] = [
    (FilterOperator::Equals, "Equals"),
    (FilterOperator::NotEquals, "Does not equal"),
    (FilterOperator::Contains, "Contains"),
    (FilterOperator::StartsWith, "Starts with"),
    (FilterOperator::EndsWith, "Ends with"),
    (FilterOperator::GreaterThan, "Greater than"),
    (FilterOperator::GreaterThanOrEqual, "Greater than or equal"),
    (FilterOperator::LessThan, "Less than"),
    (FilterOperator::LessThanOrEqual, "Less than or equal"),
    (FilterOperator::In, "In list"),
    (FilterOperator::NotIn, "Not in list"),
    (FilterOperator::IsNull, "Is null"),
    (FilterOperator::IsNotNull, "Is not null"),
];

fn operator_label(operator: FilterOperator) -> &'static str {
    FILTER_OPERATORS
        .iter()
        .find(|(op, _)| *op == operator)
        .map_or("", |(_, label)| label)
}

fn filter_needs_value(operator: FilterOperator) -> bool {
    !matches!(operator, FilterOperator::IsNull | FilterOperator::IsNotNull)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingRow {
    pub original: Vec<Value>,
    pub changes: Vec<Value>,
    pub primary_key: Vec<Value>,
    pub xmin: Option<String>,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct FilterDraft {
    column: String,
    operator: FilterOperator,
    value: String,
}

impl FilterDraft {
    fn new(column: &str) -> Self {
        Self {
            column: column.to_owned(),
            operator: FilterOperator::Contains,
            value: String::new(),
        }
    }
}

pub struct TableState {
    pub page: Option<TablePage>,
    /// The request that produced `page`, restored when a later load fails.
    pub loaded: Option<TablePageRequest>,
    /// The request in flight, recorded as `loaded` when its page arrives.
    pub requested: Option<TablePageRequest>,
    pub loading: bool,
    /// When the current load started, for the delayed "Refreshing…" overlay.
    loading_since: Option<f64>,
    /// Inline error, notice, or export result under the filters.
    pub message: Option<Message>,
    /// A column was dragged away from its default width.
    columns_resized: bool,
    /// Set by "Reset columns"; the grid forgets its widths on the next frame.
    reset_columns: bool,
    /// Rows written so far by a running export.
    pub export_progress: Option<Arc<AtomicU64>>,
    pub page_index: u32,
    pub limit: u32,
    limit_input: String,
    filters: Vec<FilterDraft>,
    pub applied: Vec<FilterCondition>,
    pub order: Option<OrderSpec>,
    selected: BTreeSet<usize>,
    anchor: Option<usize>,
    pub pending: BTreeMap<usize, PendingRow>,
    /// Uncommitted editor text by (row, column). A draft is staged when its
    /// field loses focus or Enter is pressed, and dropped on Escape.
    pub drafts: HashMap<(usize, usize), String>,
    editing: Option<(usize, usize)>,
    focus_editor: bool,
    inspector_open: bool,
    /// Column indexes collapsed to a narrow strip.
    collapsed_columns: BTreeSet<usize>,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            page: None,
            loaded: None,
            requested: None,
            loading: false,
            loading_since: None,
            message: None,
            columns_resized: false,
            reset_columns: false,
            export_progress: None,
            page_index: 0,
            limit: MAX_PREVIEW_ROWS,
            limit_input: MAX_PREVIEW_ROWS.to_string(),
            filters: vec![],
            applied: vec![],
            order: None,
            selected: BTreeSet::new(),
            anchor: None,
            pending: BTreeMap::new(),
            drafts: HashMap::new(),
            editing: None,
            focus_editor: false,
            inspector_open: true,
            collapsed_columns: BTreeSet::new(),
        }
    }
}

impl TableState {
    pub fn request(&self, profile_id: Uuid, schema: &str, table: &str) -> TablePageRequest {
        TablePageRequest {
            profile_id,
            schema: schema.into(),
            table: table.into(),
            offset: self.page_index * self.limit,
            limit: self.limit,
            filters: self.applied.clone(),
            order_by: self.order.clone(),
            include_total: Some(true),
        }
    }

    /// A page arrived for `request`.
    pub fn loaded(&mut self, page: TablePage, request: TablePageRequest) {
        self.loading = false;
        self.page_index = page.offset / page.limit.max(1);
        if self.filters.is_empty() {
            if let Some(first) = page.metadata.columns.first() {
                self.filters.push(FilterDraft::new(&first.name));
            }
        }
        self.loaded = Some(request);
        self.selected.clear();
        self.anchor = None;
        self.drafts.clear();
        self.editing = None;
        self.page = Some(page);
    }

    /// A load failed: return the controls to the page still on screen.
    pub fn restore_loaded(&mut self) {
        self.loading = false;
        if let Some(request) = &self.loaded {
            self.page_index = request.offset / request.limit.max(1);
            self.limit = request.limit;
            self.limit_input = request.limit.to_string();
            self.applied.clone_from(&request.filters);
            self.order.clone_from(&request.order_by);
        } else {
            self.page_index = 0;
        }
    }

    fn effective_order(&self) -> Option<OrderSpec> {
        self.order.clone().or_else(|| {
            let page = self.page.as_ref()?;
            Some(OrderSpec {
                column: page.metadata.primary_key.first()?.clone(),
                descending: false,
            })
        })
    }

    pub fn dirty(&self) -> bool {
        !self.pending.is_empty() || !self.drafts.is_empty()
    }

    pub fn total_rows(&self) -> Option<u64> {
        self.page.as_ref().and_then(|p| p.total_rows)
    }

    /// Rows shown on the page, with staged edits applied, as CSV.
    fn csv(&self, rows: impl Iterator<Item = usize>) -> String {
        let Some(page) = &self.page else {
            return String::new();
        };
        let columns: Vec<String> = page
            .metadata
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let values: Vec<Vec<Value>> = rows
            .filter_map(|i| {
                let row = page.rows.get(i)?;
                Some(
                    self.pending
                        .get(&i)
                        .map_or_else(|| row.clone(), |p| p.changes.clone()),
                )
            })
            .collect();
        csv_document(&columns, &values)
    }

    fn copyable_rows(&self) -> Vec<usize> {
        let count = self.page.as_ref().map_or(0, |p| p.rows.len());
        (0..count)
            .filter(|i| !self.pending.get(i).is_some_and(|p| p.deleted))
            .collect()
    }
}

pub fn pending_row(page: &TablePage, row: &[Value]) -> PendingRow {
    let count = page.metadata.columns.len();
    let original = row[..count.min(row.len())].to_vec();
    let primary_key = page
        .metadata
        .primary_key
        .iter()
        .filter_map(|key| {
            page.metadata
                .columns
                .iter()
                .position(|c| &c.name == key)
                .map(|i| original[i].clone())
        })
        .collect();
    PendingRow {
        changes: original.clone(),
        original,
        primary_key,
        xmin: row.get(count).and_then(Value::as_str).map(str::to_owned),
        deleted: false,
    }
}

pub fn stage_cell(state: &mut TableState, row: usize, column: usize, text: &str) {
    let Some(page) = &state.page else { return };
    let Some(values) = page.rows.get(row) else {
        return;
    };
    let pending = state
        .pending
        .entry(row)
        .or_insert_with(|| pending_row(page, values));
    pending.changes[column] = parse_cell_input(
        text,
        &page.metadata.columns[column],
        &pending.original[column],
    );
    if !pending.deleted && pending.changes == pending.original {
        state.pending.remove(&row);
    }
}

/// Stages (or, when every row is already staged, undoes) deletion of `rows`.
pub fn toggle_delete(state: &mut TableState, rows: &[usize]) {
    let all_deleted = rows
        .iter()
        .all(|r| state.pending.get(r).is_some_and(|p| p.deleted));
    stage_delete(state, rows, !all_deleted);
}

pub fn stage_delete(state: &mut TableState, rows: &[usize], deleted: bool) {
    let Some(page) = &state.page else { return };
    for &row in rows {
        if deleted {
            if let Some(values) = page.rows.get(row) {
                state
                    .pending
                    .entry(row)
                    .or_insert_with(|| pending_row(page, values))
                    .deleted = true;
            }
        } else if let Some(pending) = state.pending.get_mut(&row) {
            pending.deleted = false;
            if pending.changes == pending.original {
                state.pending.remove(&row);
            }
        }
    }
}

pub enum TableAction {
    Reload,
    Save,
    Export,
    /// CSV text and a description such as "3 visible rows".
    Copy(String, String),
    Error(&'static str),
}

pub struct TableContext<'a> {
    pub tab_id: u64,
    pub schema: &'a str,
    pub table: &'a str,
    pub embedded: bool,
    pub read_only: bool,
    pub saving: bool,
    pub exporting: bool,
}

/// Lays out one table view inside `ui` and reports what the workbench must do.
pub fn show(ui: &mut egui::Ui, cx: &TableContext<'_>, state: &mut TableState) -> Vec<TableAction> {
    let mut actions = Vec::new();
    let id = |name: &str| egui::Id::new((name, cx.tab_id, cx.embedded));
    let bar = |margin: Margin| {
        Frame::new()
            .fill(theme::CHROME)
            .inner_margin(margin)
            .stroke(Stroke::new(1.0, theme::BORDER))
    };

    egui::TopBottomPanel::top(id("table-toolbar"))
        .exact_height(theme::TOOLBAR_HEIGHT)
        .frame(bar(Margin::symmetric(14, 0)))
        .show_inside(ui, |ui| toolbar(ui, cx, state, &mut actions));
    let Some(page) = state.page.clone() else {
        message_panel(ui, id("table-message"), state);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG))
            .show_inside(ui, |ui| {
                if state.loading {
                    loading_skeleton(ui);
                    return;
                }
                ui.centered_and_justified(|ui| {
                    if ui.button("Couldn't load rows · Retry").clicked() {
                        actions.push(TableAction::Reload);
                    }
                });
            });
        return actions;
    };
    egui::TopBottomPanel::top(id("table-filters"))
        .frame(bar(Margin {
            left: 12,
            right: 10,
            top: 7,
            bottom: 7,
        }))
        .show_inside(ui, |ui| filter_panel(ui, state, &page, &mut actions));
    message_panel(ui, id("table-message"), state);
    egui::TopBottomPanel::bottom(id("table-status"))
        .exact_height(theme::STATUS_BAR_HEIGHT)
        .frame(bar(Margin::symmetric(14, 0)))
        .show_inside(ui, |ui| status_bar(ui, state, &page, &mut actions));
    if !state.pending.is_empty() {
        egui::TopBottomPanel::bottom(id("table-pending"))
            .exact_height(theme::PENDING_BAR_HEIGHT)
            .frame(bar(Margin::symmetric(14, 0)))
            .show_inside(ui, |ui| pending_bar(ui, state, cx, &mut actions));
    }
    let interactive = !state.loading && !cx.saving;
    if state.inspector_open {
        egui::SidePanel::right(id("table-inspector"))
            .exact_width(theme::INSPECTOR_WIDTH)
            .resizable(false)
            .frame(bar(Margin::same(0)))
            .show_inside(ui, |ui| {
                ui.add_enabled_ui(interactive, |ui| inspector(ui, cx, state, &page));
            });
    }
    egui::CentralPanel::default()
        .frame(Frame::new().fill(theme::BG))
        .show_inside(ui, |ui| {
            let area = ui.max_rect();
            ui.add_enabled_ui(interactive, |ui| grid(ui, cx, state, &page, &mut actions));
            loading_overlay(ui, area, state);
        });
    commit_unfocused_drafts(ui, cx.tab_id, state);
    actions
}

fn editable(cx: &TableContext<'_>, page: &TablePage) -> bool {
    !cx.read_only && !page.metadata.primary_key.is_empty()
}

fn editable_column(cx: &TableContext<'_>, page: &TablePage, column: usize) -> bool {
    editable(cx, page)
        && !page
            .metadata
            .primary_key
            .contains(&page.metadata.columns[column].name)
}

fn reload_or_block(state: &TableState) -> TableAction {
    if state.pending.is_empty() {
        TableAction::Reload
    } else {
        TableAction::Error(PENDING_REFRESH_ERROR)
    }
}

fn toolbar(
    ui: &mut egui::Ui,
    cx: &TableContext<'_>,
    state: &mut TableState,
    actions: &mut Vec<TableAction>,
) {
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        icons::show(ui, Icon::Table, 15.0, theme::MUTED);
        if !cx.embedded {
            ui.label(
                RichText::new(cx.schema)
                    .font(ui_font(15.0))
                    .color(theme::MUTED),
            );
            ui.add_space(-6.0);
            ui.label(
                RichText::new(format!(".{}", cx.table))
                    .font(ui_font(15.0))
                    .strong()
                    .color(theme::TEXT_STRONG),
            );
        } else {
            ui.label(
                RichText::new(format!("{}.{}", cx.schema, cx.table))
                    .strong()
                    .color(theme::TEXT_STRONG),
            );
        }
        if let Some(page) = &state.page {
            let rows = page
                .total_rows
                .map_or_else(|| "—".to_owned(), |n| n.to_string());
            ui.label(
                RichText::new(format!(
                    "{rows} rows · {} columns",
                    page.metadata.columns.len()
                ))
                .font(ui_font(12.0))
                .color(theme::FAINT),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (tip, fill) = if state.inspector_open {
                ("Hide row inspector", Some(theme::ACCENT_SOFT))
            } else {
                ("Show row inspector", None)
            };
            let toggle = icons::button(ui, Icon::Inspector, None, tip);
            if let Some(fill) = fill {
                ui.painter().rect_stroke(
                    toggle.rect,
                    6,
                    Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.6)),
                    egui::StrokeKind::Inside,
                );
                ui.painter().rect_filled(toggle.rect, 6, fill);
            }
            if toggle.clicked() {
                state.inspector_open = !state.inspector_open;
            }
            let has_page = state.page.is_some() && !state.loading;
            let selected: Vec<usize> = state.selected.iter().copied().collect();
            if !selected.is_empty() {
                let menu = icons::button(
                    ui,
                    Icon::ChevronDown,
                    Some(&format!("{} selected", selected.len())),
                    "Actions for the selected rows",
                );
                egui::Popup::menu(&menu).show(|ui| {
                    selection_menu(ui, cx, state, &selected, actions);
                });
            }
            let total = state
                .total_rows()
                .map_or_else(String::new, |n| format!(" ({n})"));
            let export_label = if cx.exporting {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
                let done = state
                    .export_progress
                    .as_ref()
                    .map_or(0, |rows| rows.load(Ordering::Relaxed));
                match state.total_rows() {
                    Some(total) => format!("Exporting {done} / {total}…"),
                    None => format!("Exporting {done}…"),
                }
            } else {
                format!("Export all{total}")
            };
            if enabled_icon_button(
                ui,
                has_page && !cx.exporting,
                Icon::Download,
                Some(&export_label),
                "Prompts for a location and exports every filtered row",
            ) {
                actions.push(if state.pending.is_empty() {
                    TableAction::Export
                } else {
                    TableAction::Error(PENDING_EXPORT_ERROR)
                });
            }
            let copyable = state.copyable_rows();
            if enabled_icon_button(
                ui,
                has_page,
                Icon::Copy,
                Some(&format!("Copy visible ({})", copyable.len())),
                "Copies only the current preview page",
            ) {
                let label = rows_label(copyable.len(), "visible");
                actions.push(TableAction::Copy(state.csv(copyable.into_iter()), label));
            }
            let refresh = if state.loading {
                "Refreshing…"
            } else {
                "Refresh"
            };
            if enabled_icon_button(
                ui,
                !state.loading,
                Icon::Refresh,
                Some(refresh),
                "Reload the current page with the same filters and sort",
            ) {
                actions.push(reload_or_block(state));
            }
        });
    });
}

/// "N selected" menu and the grid's context menu.
fn selection_menu(
    ui: &mut egui::Ui,
    cx: &TableContext<'_>,
    state: &mut TableState,
    selected: &[usize],
    actions: &mut Vec<TableAction>,
) {
    if ui.button("Copy selected as CSV").clicked() {
        let rows: Vec<usize> = selected
            .iter()
            .copied()
            .filter(|r| !state.pending.get(r).is_some_and(|p| p.deleted))
            .collect();
        let label = rows_label(rows.len(), "selected");
        actions.push(TableAction::Copy(state.csv(rows.into_iter()), label));
        ui.close();
    }
    let Some(page) = state.page.as_ref() else {
        return;
    };
    if editable(cx, page) {
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
        let button = if all_deleted {
            egui::Button::new(label)
        } else {
            egui::Button::new(RichText::new(label).color(theme::DANGER))
        };
        if ui.add(button).clicked() {
            toggle_delete(state, selected);
            ui.close();
        }
    }
    if ui.button("Clear selection").clicked() {
        state.selected.clear();
        state.anchor = None;
        ui.close();
    }
}

fn rows_label(count: usize, which: &str) -> String {
    format!(
        "{count} {which} {}",
        if count == 1 { "row" } else { "rows" }
    )
}

fn enabled_icon_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: Icon,
    label: Option<&str>,
    tooltip: &str,
) -> bool {
    ui.add_enabled_ui(enabled, |ui| icons::button(ui, icon, label, tooltip))
        .inner
        .clicked()
}

fn labeled(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(ui_font(12.0)).color(theme::MUTED));
}

fn filter_panel(
    ui: &mut egui::Ui,
    state: &mut TableState,
    page: &TablePage,
    actions: &mut Vec<TableAction>,
) {
    let columns: Vec<&str> = page
        .metadata
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    let mut apply = false;
    // Like `.filter-panel`: the header on the left and the query controls
    // right-aligned beside it, dropping to their own line when they don't
    // fit. The controls' width is measured on the previous frame.
    let width_id = ui.id().with("filter-controls-width");
    let known: f32 = ui.data(|d| d.get_temp(width_id)).unwrap_or(0.0);
    let mut measured = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        icons::show(ui, Icon::Filter, 13.0, theme::FAINT);
        ui.label(
            RichText::new("Filters")
                .font(ui_font(12.0))
                .strong()
                .color(theme::SECONDARY),
        );
        ui.label(
            RichText::new("All filters must match")
                .font(ui_font(12.0))
                .color(theme::FAINT),
        );
        let add = icons::button(ui, Icon::Plus, Some("Add filter"), "Add another filter");
        if add.clicked() {
            state.filters.push(FilterDraft::new(
                columns.first().copied().unwrap_or_default(),
            ));
        }
        // 16 px from the header, and the spacing before the controls.
        let room = ui.available_width() - 16.0 - 2.0 * ui.spacing().item_spacing.x;
        if known <= room {
            ui.add_space(room - known + 16.0);
            let controls =
                ui.scope(|ui| query_controls(ui, state, page, &columns, actions, &mut apply));
            measured = Some(controls.response.rect.width());
        }
    });
    if measured.is_none() {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space((ui.available_width() - known - ui.spacing().item_spacing.x).max(0.0));
            let controls =
                ui.scope(|ui| query_controls(ui, state, page, &columns, actions, &mut apply));
            measured = Some(controls.response.rect.width());
        });
    }
    if let Some(width) = measured {
        if (width - known).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(width_id, width));
            ui.ctx().request_repaint();
        }
    }
    ui.add_space(2.0);
    let mut remove = None;
    for (index, filter) in state.filters.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("filter-column", index))
                .selected_text(&filter.column)
                .width(170.0)
                .show_ui(ui, |ui| {
                    for column in &columns {
                        ui.selectable_value(&mut filter.column, (*column).to_owned(), *column);
                    }
                });
            egui::ComboBox::from_id_salt(("filter-operator", index))
                .selected_text(operator_label(filter.operator))
                .width(170.0)
                .show_ui(ui, |ui| {
                    for (operator, label) in FILTER_OPERATORS {
                        ui.selectable_value(&mut filter.operator, operator, label);
                    }
                });
            let value_width = (ui.available_width() - 40.0).max(120.0);
            if filter_needs_value(filter.operator) {
                let hint = if matches!(filter.operator, FilterOperator::In | FilterOperator::NotIn)
                {
                    "value 1, value 2, …"
                } else {
                    "Value…"
                };
                let response = ui.add(
                    egui::TextEdit::singleline(&mut filter.value)
                        .hint_text(hint)
                        .desired_width(value_width),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    apply = true;
                }
            } else {
                ui.add_sized(
                    [value_width, 24.0],
                    egui::Label::new(RichText::new("No value needed").color(theme::FAINT)),
                );
            }
            if icons::button(ui, Icon::Close, None, "Remove filter").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        state.filters.remove(index);
    }
    if apply {
        state.applied = state
            .filters
            .iter()
            .filter(|f| !f.column.is_empty())
            .filter(|f| !filter_needs_value(f.operator) || !f.value.trim().is_empty())
            .map(|f| FilterCondition {
                column: f.column.clone(),
                operator: f.operator,
                value: filter_needs_value(f.operator).then(|| f.value.trim().to_owned()),
            })
            .collect();
        state.page_index = 0;
        actions.push(reload_or_block(state));
    }
}

/// Preview limit, sort, direction, reset, clear, and apply, left to right.
fn query_controls(
    ui: &mut egui::Ui,
    state: &mut TableState,
    page: &TablePage,
    columns: &[&str],
    actions: &mut Vec<TableAction>,
    apply: &mut bool,
) {
    // 6 px between a label and its control, 12 px between groups.
    ui.spacing_mut().item_spacing.x = 6.0;
    labeled(ui, "Preview limit");
    // The stepper sits inside the field, as in the desktop app.
    let visuals = ui.visuals().widgets.inactive;
    let (response, step) = Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(visuals.bg_stroke)
        .corner_radius(visuals.corner_radius)
        .inner_margin(Margin {
            left: 8,
            right: 2,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.limit_input)
                    .frame(false)
                    .desired_width(42.0)
                    .vertical_align(egui::Align::Center)
                    .min_size(Vec2::new(42.0, 26.0)),
            );
            (response, limit_stepper(ui))
        })
        .inner;
    if response.lost_focus() || step != 0 {
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
    ui.add_space(6.0);
    labeled(ui, "Sort by");
    let effective = state.effective_order();
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
    egui::ComboBox::from_id_salt("sort-column")
        .selected_text(label(&column))
        .width(170.0)
        .show_ui(ui, |ui| {
            for name in columns {
                ui.selectable_value(&mut column, (*name).to_owned(), label(name));
            }
        });
    let mut descending = effective.as_ref().is_some_and(|o| o.descending);
    if effective.is_some() {
        ui.add_space(6.0);
        labeled(ui, "Direction");
        egui::ComboBox::from_id_salt("sort-direction")
            .selected_text(if descending {
                "Descending"
            } else {
                "Ascending"
            })
            .width(116.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut descending, false, "Ascending");
                ui.selectable_value(&mut descending, true, "Descending");
            });
    }
    let changed_column = effective.as_ref().map(|o| o.column.as_str()) != Some(column.as_str())
        && !column.is_empty();
    let changed_direction = effective
        .as_ref()
        .is_some_and(|o| o.descending != descending);
    if changed_column || changed_direction {
        state.order = Some(OrderSpec {
            column,
            descending: descending && !changed_column,
        });
        state.page_index = 0;
        actions.push(reload_or_block(state));
    }
    if state.columns_resized || !state.collapsed_columns.is_empty() {
        ui.add_space(6.0);
    }
    if (state.columns_resized || !state.collapsed_columns.is_empty())
        && ui
            .add(
                egui::Button::new(
                    RichText::new("Reset columns")
                        .font(ui_font(12.0))
                        .color(theme::ACCENT_TEXT),
                )
                .frame(false),
            )
            .on_hover_text("Restore every column's default width")
            .clicked()
    {
        state.collapsed_columns.clear();
        state.reset_columns = true;
    }
    ui.add_space(6.0);
    let has_filters = !state.applied.is_empty()
        || state.filters.len() > 1
        || state
            .filters
            .iter()
            .any(|f| !filter_needs_value(f.operator) || !f.value.trim().is_empty());
    if has_filters && ui.button("Clear").clicked() {
        state.filters = columns
            .first()
            .map(|c| FilterDraft::new(c))
            .into_iter()
            .collect();
        state.applied.clear();
        state.page_index = 0;
        actions.push(reload_or_block(state));
    }
    if ui.add(primary_button("Apply filters")).clicked() {
        *apply = true;
    }
}

fn status_bar(
    ui: &mut egui::Ui,
    state: &mut TableState,
    page: &TablePage,
    actions: &mut Vec<TableAction>,
) {
    let text = |t: String| RichText::new(t).font(ui_font(12.0)).color(theme::MUTED);
    ui.horizontal_centered(|ui| {
        let total = page
            .total_rows
            .map_or_else(String::new, |n| format!(" of {n}"));
        let rows = if page.rows.is_empty() {
            format!("No rows{total}")
        } else {
            format!(
                "Rows {}–{}{total}",
                page.offset + 1,
                page.offset + page.rows.len() as u32
            )
        };
        ui.label(text(rows));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clean = state.pending.is_empty();
            if page.offset > 0 || page.has_more {
                if enabled_icon_button(
                    ui,
                    page.has_more && clean && !state.loading,
                    Icon::ChevronRight,
                    None,
                    "Next page",
                ) {
                    state.page_index += 1;
                    actions.push(TableAction::Reload);
                }
                let current = page.offset / page.limit.max(1) + 1;
                let pages = page
                    .total_rows
                    .map(|n| n.div_ceil(u64::from(page.limit.max(1))).max(1));
                ui.label(text(match pages {
                    Some(pages) => format!("Page {current} of {pages}"),
                    None => format!("Page {current}"),
                }));
                if enabled_icon_button(
                    ui,
                    page.offset > 0 && clean && !state.loading,
                    Icon::ChevronLeft,
                    None,
                    "Previous page",
                ) {
                    state.page_index = state.page_index.saturating_sub(1);
                    actions.push(TableAction::Reload);
                }
                ui.add_space(8.0);
            }
            ui.label(text(format!("{} per page", page.limit)));
        });
    });
}

fn pending_bar(
    ui: &mut egui::Ui,
    state: &mut TableState,
    cx: &TableContext<'_>,
    actions: &mut Vec<TableAction>,
) {
    let count = state.pending.len();
    let deleted = state.pending.values().filter(|p| p.deleted).count();
    let edited = count - deleted;
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    ui.horizontal_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 4.0, theme::MODIFIED);
        ui.label(
            RichText::new(plural(count, "pending change", "pending changes"))
                .strong()
                .color(theme::TEXT_STRONG),
        );
        if edited > 0 {
            chip(
                ui,
                &plural(edited, "edited row", "edited rows"),
                theme::MODIFIED,
            );
        }
        if deleted > 0 {
            chip(ui, &plural(deleted, "deletion", "deletions"), theme::DANGER);
        }
        if cx.read_only {
            chip(ui, "Read-only connection", theme::MUTED);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = if cx.saving {
                "Saving…".to_owned()
            } else {
                format!("Save changes ({count})")
            };
            if ui
                .add_enabled(!cx.read_only && !cx.saving, primary_button(&save))
                .clicked()
            {
                actions.push(TableAction::Save);
            }
            if ui
                .add_enabled(!cx.saving, egui::Button::new("Discard changes"))
                .clicked()
            {
                state.pending.clear();
                state.drafts.clear();
                state.editing = None;
            }
        });
    });
}

pub fn cell_text(value: &Value) -> RichText {
    if value.is_null() {
        RichText::new("NULL")
            .font(mono(12.0))
            .italics()
            .color(theme::FAINT)
    } else {
        let text = display_value(value);
        let short: String = text.chars().take(200).collect();
        RichText::new(short.replace('\n', " "))
            .font(mono(12.0))
            .color(theme::TEXT)
    }
}

fn hairline(ui: &egui::Ui) {
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, theme::HAIRLINE),
    );
}

/// Grid chrome shared by result and table grids: a header band across the
/// full width, and column resize lines that only show on hover.
/// The inline error, notice, or export result between the filters and grid.
fn message_panel(ui: &mut egui::Ui, id: egui::Id, state: &mut TableState) {
    let Some(message) = &state.message else {
        return;
    };
    let now = ui.input(|i| i.time);
    let Some(opacity) = message.opacity(ui.ctx(), now) else {
        state.message = None;
        return;
    };
    let mut response = messages::Response::None;
    egui::TopBottomPanel::top(id)
        .frame(Frame::new().fill(theme::BG).inner_margin(Margin {
            left: 0,
            right: 0,
            top: 0,
            bottom: 8,
        }))
        .show_separator_line(false)
        .show_inside(ui, |ui| response = messages::inline(ui, message, opacity));
    let path = message.export.as_ref().map(|(path, _)| path.clone());
    match (response, path) {
        (messages::Response::Dismiss, _) => state.message = None,
        (messages::Response::Open, Some(path)) => crate::workbench::open_path(&path, false),
        (messages::Response::Reveal, Some(path)) => crate::workbench::open_path(&path, true),
        _ => {}
    }
}

/// `.initial-grid-skeleton`: a header band and five rows with a moving
/// highlight while the first page loads.
fn loading_skeleton(ui: &mut egui::Ui) {
    let top = ui.max_rect().min;
    let width = ui.max_rect().width();
    let time = ui.input(|i| i.time);
    ui.ctx().request_repaint();
    let (response_rect, _) = ui.allocate_exact_size(
        Vec2::new(width, theme::HEADER_HEIGHT + 5.0 * theme::ROW_HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter_at(response_rect);
    let band = |y: f32, height: f32, base: Color32| {
        let rect = Rect::from_min_size(egui::pos2(top.x, y), Vec2::new(width, height));
        painter.rect_filled(rect, 0.0, base);
        // A soft highlight sweeping left to right every 1.5 s.
        let phase = (time % 1.5 / 1.5) as f32;
        let center = rect.left() + (phase * 2.2 - 0.6) * width;
        let glow = Rect::from_center_size(
            egui::pos2(center, rect.center().y),
            Vec2::new(width * 0.5, height),
        )
        .intersect(rect);
        if glow.is_positive() {
            painter.rect_filled(glow, 0.0, Color32::from_white_alpha(5));
        }
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, theme::HAIRLINE),
        );
    };
    band(top.y, theme::HEADER_HEIGHT, theme::GRID_HEADER);
    for row in 0..5 {
        band(
            top.y + theme::HEADER_HEIGHT + row as f32 * theme::ROW_HEIGHT,
            theme::ROW_HEIGHT,
            Color32::from_white_alpha(4),
        );
    }
}

/// Up/down arrows beside the preview limit, like the desktop stepper.
fn limit_stepper(ui: &mut egui::Ui) -> i64 {
    let mut step = 0;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (icon, amount, tip) in [
            (Icon::ChevronUp, 1, "Increase preview limit"),
            (Icon::ChevronDown, -1, "Decrease preview limit"),
        ] {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(16.0, 12.0), Sense::click());
            let color = if response.hovered() {
                theme::TEXT
            } else {
                theme::FAINT
            };
            if response.hovered() {
                ui.painter().rect_filled(rect, 2.0, theme::CONTROL_HOVER);
            }
            icons::paint(ui.painter(), rect.shrink2(Vec2::new(3.0, 1.0)), icon, color);
            if response.on_hover_text(tip).clicked() {
                step = amount;
            }
        }
    });
    step
}

/// Dims the grid and shows a "Refreshing…" pill once a reload of an already
/// visible page has taken longer than 120 ms, as the desktop app does.
fn loading_overlay(ui: &mut egui::Ui, area: Rect, state: &mut TableState) {
    if !state.loading {
        state.loading_since = None;
        return;
    }
    let now = ui.input(|i| i.time);
    let since = *state.loading_since.get_or_insert(now);
    if now - since < 0.12 {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(0.12 - (now - since)));
        return;
    }
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, Color32::from_black_alpha(90));
    let galley =
        painter.layout_no_wrap("Refreshing…".into(), theme::ui_font(12.0), theme::SECONDARY);
    let pill = Rect::from_center_size(area.center(), galley.size() + Vec2::new(24.0, 12.0));
    painter.rect(
        pill,
        pill.height() / 2.0,
        theme::POPOVER,
        Stroke::new(1.0, theme::BORDER_STRONG),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        pill.center() - galley.size() / 2.0,
        galley,
        theme::SECONDARY,
    );
}

fn grid_scope(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::NONE;
    let band = egui::Rect::from_min_size(
        ui.cursor().min,
        Vec2::new(
            ui.available_width().max(ui.clip_rect().width()),
            theme::HEADER_HEIGHT,
        ),
    );
    ui.painter().rect_filled(band, 0, theme::GRID_HEADER);
    ui.painter().hline(
        band.x_range(),
        band.bottom() - 0.5,
        Stroke::new(1.0, theme::BORDER),
    );
}

fn aligned(ui: &mut egui::Ui, right: bool, add: impl FnOnce(&mut egui::Ui)) {
    let layout = if right {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.with_layout(layout, add);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HeaderClick {
    Sort,
    Toggle,
}

/// Column header: key glyph, name, type in faint mono, sort indicator, and
/// (for table grids) a collapse button. Collapsed columns show a narrow
/// strip that expands on click.
fn header_cell(
    ui: &mut egui::Ui,
    name: &str,
    data_type: &str,
    primary_key: bool,
    sort: Option<bool>,
    collapsible: Option<bool>,
) -> Option<HeaderClick> {
    let rect = ui.max_rect();
    // `.data-grid th { border-right: 1px solid #242427 }`
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, Color32::from_rgb(0x24, 0x24, 0x27)),
    );
    if collapsible == Some(true) {
        let response = ui
            .interact(rect, ui.id().with(("expand", name)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Expand {name}"));
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(rect.center() - Vec2::new(0.0, 6.0), Vec2::splat(12.0)),
            Icon::Expand,
            theme::MUTED,
        );
        let mut job =
            egui::text::LayoutJob::simple_singleline(name.to_owned(), ui_font(9.5), theme::FAINT);
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 8.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y + 2.0,
            ),
            galley,
            theme::FAINT,
        );
        return response.clicked().then_some(HeaderClick::Toggle);
    }
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add_space(4.0);
            if primary_key {
                icons::show(ui, Icon::Key, 11.0, theme::MODIFIED).on_hover_text("Primary key");
            }
            ui.label(
                RichText::new(name)
                    .size(12.5)
                    .strong()
                    .color(theme::TEXT_STRONG),
            );
            if !data_type.is_empty() {
                ui.label(
                    RichText::new(data_type)
                        .font(mono(11.0))
                        .color(theme::FAINT),
                );
            }
            if let Some(descending) = sort {
                let icon = if descending {
                    Icon::SortDown
                } else {
                    Icon::SortUp
                };
                icons::show(ui, icon, 10.0, theme::ACCENT);
            }
        })
        .response;
    let sort_click = ui
        .interact(rect, response.id.with("sort"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("Sort by {name}"));
    if collapsible == Some(false) && sort_click.hovered() {
        let button = egui::Rect::from_center_size(
            rect.right_center() - Vec2::new(14.0, 0.0),
            Vec2::splat(20.0),
        );
        let toggle = ui
            .interact(button, response.id.with("collapse"), Sense::click())
            .on_hover_text(format!("Collapse {name}"));
        ui.painter().rect_filled(button, 5, theme::CONTROL_HOVER);
        icons::paint(
            ui.painter(),
            button.shrink(4.0),
            Icon::ChevronLeft,
            theme::SECONDARY,
        );
        if toggle.clicked() {
            return Some(HeaderClick::Toggle);
        }
    }
    sort_click.clicked().then_some(HeaderClick::Sort)
}

/// The staged change on a row: before/after with the changed middle marked.
fn change_preview(
    ui: &mut egui::Ui,
    columns: &[dbm_core::models::TableColumn],
    pending: &PendingRow,
) {
    ui.set_max_width(420.0);
    if pending.deleted {
        ui.label(
            RichText::new("Pending delete")
                .strong()
                .color(theme::DANGER),
        );
        ui.label(
            RichText::new("This row will be deleted when changes are saved.")
                .color(theme::SECONDARY),
        );
        return;
    }
    ui.label(
        RichText::new("Pending edit")
            .strong()
            .color(theme::MODIFIED),
    );
    for (index, column) in columns.iter().enumerate() {
        if pending.changes[index] == pending.original[index] {
            continue;
        }
        let diff = inline_diff(
            &display_value(&pending.original[index]),
            &display_value(&pending.changes[index]),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(&column.name)
                .strong()
                .color(theme::TEXT_STRONG),
        );
        let part =
            |job: &mut egui::text::LayoutJob, text: &str, background: Color32, color: Color32| {
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: mono(12.0),
                        color,
                        background,
                        ..Default::default()
                    },
                );
            };
        let mut before = egui::text::LayoutJob::default();
        part(
            &mut before,
            &diff.prefix,
            Color32::TRANSPARENT,
            theme::MUTED,
        );
        part(
            &mut before,
            &diff.removed,
            theme::DANGER_SOFT,
            theme::DANGER,
        );
        part(
            &mut before,
            &diff.suffix,
            Color32::TRANSPARENT,
            theme::MUTED,
        );
        let mut after = egui::text::LayoutJob::default();
        part(&mut after, &diff.prefix, Color32::TRANSPARENT, theme::TEXT);
        part(
            &mut after,
            &diff.added,
            Color32::from_rgba_unmultiplied(90, 211, 148, 40),
            theme::SUCCESS,
        );
        part(&mut after, &diff.suffix, Color32::TRANSPARENT, theme::TEXT);
        ui.horizontal_wrapped(|ui| {
            ui.label(before);
            ui.label(RichText::new("→").color(theme::FAINT));
            ui.label(after);
        });
    }
}

/// Read-only query results.
pub fn result_grid(ui: &mut egui::Ui, tab_id: u64, columns: &[QueryColumn], rows: &[Vec<Value>]) {
    if columns.is_empty() {
        return;
    }
    let numeric: Vec<bool> = columns
        .iter()
        .map(|c| numeric_column(&c.data_type))
        .collect();
    egui::ScrollArea::horizontal()
        .id_salt(("result-scroll", tab_id))
        .show(ui, |ui| {
            grid_scope(ui);
            TableBuilder::new(ui)
                .id_salt(("result", tab_id))
                .auto_shrink([false, false])
                .columns(
                    Column::initial(DEFAULT_COLUMN_WIDTH)
                        .at_least(60.0)
                        .resizable(true)
                        .clip(true),
                    columns.len(),
                )
                .header(theme::HEADER_HEIGHT, |mut header| {
                    for column in columns {
                        header.col(|ui| {
                            header_cell(ui, &column.name, "", false, None, None);
                        });
                    }
                })
                .body(|body| {
                    body.rows(theme::ROW_HEIGHT, rows.len(), |mut row| {
                        let index = row.index();
                        for (column, value) in rows[index].iter().enumerate().take(columns.len()) {
                            row.col(|ui| {
                                hairline(ui);
                                ui.add_space(6.0);
                                aligned(ui, numeric[column], |ui| {
                                    ui.add_space(6.0);
                                    ui.label(cell_text(value));
                                });
                            });
                        }
                    });
                });
        });
}

fn grid(
    ui: &mut egui::Ui,
    cx: &TableContext<'_>,
    state: &mut TableState,
    page: &TablePage,
    actions: &mut Vec<TableAction>,
) {
    let tab_id = cx.tab_id;
    let columns = &page.metadata.columns;
    let numeric: Vec<bool> = columns
        .iter()
        .map(|c| numeric_column(&c.data_type))
        .collect();
    let order = state.effective_order();
    let mut sort_clicked = None;
    let mut clicked_row = None;
    let mut context_row = None;
    let mut edit_cell = None;
    let mut toggled_column = None;
    let mut discard_row = None;
    let modifiers = ui.input(|i| i.modifiers);
    if page.rows.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("No rows match this view.").color(theme::MUTED));
        });
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt(("grid-scroll", tab_id, cx.embedded))
        .show(ui, |ui| {
            grid_scope(ui);
            let mut builder = TableBuilder::new(ui)
                .id_salt(("grid", tab_id, cx.embedded))
                .sense(Sense::click())
                .auto_shrink([false, false]);
            if std::mem::take(&mut state.reset_columns) {
                builder.reset();
            }
            let mut resized = false;
            for index in 0..columns.len() {
                builder = builder.column(if state.collapsed_columns.contains(&index) {
                    Column::exact(COLLAPSED_COLUMN_WIDTH).clip(true)
                } else {
                    Column::initial(DEFAULT_COLUMN_WIDTH)
                        .at_least(60.0)
                        .resizable(true)
                        .clip(true)
                });
            }
            builder
                .header(theme::HEADER_HEIGHT, |mut header| {
                    for (index, column) in columns.iter().enumerate() {
                        header.col(|ui| {
                            let sort = order
                                .as_ref()
                                .filter(|o| o.column == column.name)
                                .map(|o| o.descending);
                            let key = page.metadata.primary_key.contains(&column.name);
                            let collapsed = state.collapsed_columns.contains(&index);
                            let width = ui.max_rect().width();
                            resized |= !collapsed && (width - DEFAULT_COLUMN_WIDTH).abs() > 0.5;
                            match header_cell(
                                ui,
                                &column.name,
                                &column.data_type,
                                key,
                                sort,
                                Some(collapsed),
                            ) {
                                Some(HeaderClick::Sort) => sort_clicked = Some(column.name.clone()),
                                Some(HeaderClick::Toggle) => toggled_column = Some(index),
                                None => {}
                            }
                        });
                    }
                })
                .body(|body| {
                    state.columns_resized = resized;
                    body.rows(theme::ROW_HEIGHT, page.rows.len(), |mut row| {
                        let index = row.index();
                        let pending = state.pending.get(&index);
                        let deleted = pending.is_some_and(|p| p.deleted);
                        let modified = pending.is_some_and(|p| !p.deleted);
                        let selected = state.selected.contains(&index);
                        let values = pending.map_or(&page.rows[index][..], |p| &p.changes[..]);
                        for (column, &right_align) in numeric.iter().enumerate() {
                            let editing = state.editing == Some((index, column));
                            let (_, response) = row.col(|ui| {
                                let rect = ui.max_rect();
                                hairline(ui);
                                if selected {
                                    ui.painter().rect_filled(rect, 0, theme::ACCENT_SOFT);
                                }
                                if deleted {
                                    ui.painter().rect_filled(rect, 0, theme::DANGER_SOFT);
                                }
                                let changed = pending
                                    .is_some_and(|p| p.changes[column] != p.original[column]);
                                if changed && !deleted {
                                    ui.painter().rect_filled(rect, 0, theme::MODIFIED_SOFT);
                                    ui.painter().circle_filled(
                                        rect.right_top() + Vec2::new(-6.0, 6.0),
                                        2.5,
                                        theme::MODIFIED,
                                    );
                                }
                                // Row state edge on the first column.
                                let edge = if deleted {
                                    Some(theme::DANGER)
                                } else if modified {
                                    Some(theme::MODIFIED)
                                } else if selected {
                                    Some(theme::ACCENT)
                                } else {
                                    None
                                };
                                if let (0, Some(color)) = (column, edge) {
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            rect.min,
                                            Vec2::new(2.0, rect.height()),
                                        ),
                                        0,
                                        color,
                                    );
                                }
                                if editing {
                                    ui.painter().rect_filled(rect, 0, theme::EDIT_SURFACE);
                                    ui.painter().rect_stroke(
                                        rect.shrink(1.0),
                                        0,
                                        Stroke::new(1.5, theme::ACCENT),
                                        egui::StrokeKind::Inside,
                                    );
                                    let current =
                                        values.get(column).map_or_else(String::new, editable_text);
                                    let draft =
                                        state.drafts.entry((index, column)).or_insert(current);
                                    let edit = ui.add(
                                        egui::TextEdit::singleline(draft)
                                            .id(cell_editor_id(tab_id, index, column))
                                            .font(mono(12.0))
                                            .frame(false)
                                            .desired_width(f32::INFINITY),
                                    );
                                    if state.focus_editor {
                                        edit.request_focus();
                                        state.focus_editor = false;
                                    }
                                    return;
                                }
                                let value = values.get(column).unwrap_or(&Value::Null);
                                let mut text = cell_text(value);
                                if deleted {
                                    text = text.color(theme::DANGER).strikethrough();
                                }
                                aligned(ui, right_align, |ui| {
                                    ui.add_space(8.0);
                                    ui.label(text);
                                });
                            });
                            if response.double_clicked()
                                && editable_column(cx, page, column)
                                && !deleted
                            {
                                edit_cell = Some((index, column));
                            }
                        }
                        let preview = state.pending.get(&index).cloned();
                        let mut response = row.response();
                        if let Some(preview) = &preview {
                            response =
                                response.on_hover_ui(|ui| change_preview(ui, columns, preview));
                        }
                        if response.clicked() {
                            clicked_row = Some(index);
                        }
                        if response.secondary_clicked() {
                            context_row = Some(index);
                        }
                        response.context_menu(|ui| {
                            if let Some(preview) = &preview {
                                let label = if preview.deleted {
                                    "Undo delete"
                                } else {
                                    "Discard edit"
                                };
                                if ui.button(label).clicked() {
                                    discard_row = Some(index);
                                    ui.close();
                                }
                                ui.separator();
                            }
                            let selected: Vec<usize> = state.selected.iter().copied().collect();
                            selection_menu(ui, cx, state, &selected, actions);
                        });
                    });
                });
        });
    if let Some(index) = toggled_column
        && !state.collapsed_columns.remove(&index)
    {
        state.collapsed_columns.insert(index);
    }
    if let Some(row) = discard_row {
        state.pending.remove(&row);
    }
    if let Some(column) = sort_clicked {
        if state.pending.is_empty() {
            let descending = order
                .as_ref()
                .is_some_and(|o| o.column == column && !o.descending);
            state.order = Some(OrderSpec { column, descending });
            state.page_index = 0;
            actions.push(TableAction::Reload);
        } else {
            actions.push(TableAction::Error(PENDING_REFRESH_ERROR));
        }
    }
    if let Some(index) = context_row.filter(|i| !state.selected.contains(i)) {
        state.selected = BTreeSet::from([index]);
        state.anchor = Some(index);
    }
    if let Some(index) = clicked_row {
        if modifiers.shift {
            let anchor = state.anchor.unwrap_or(index);
            state.selected = (anchor.min(index)..=anchor.max(index)).collect();
        } else if modifiers.command {
            if !state.selected.remove(&index) {
                state.selected.insert(index);
            }
            state.anchor = Some(index);
        } else {
            state.selected = BTreeSet::from([index]);
            state.anchor = Some(index);
        }
    }
    if let Some(cell) = edit_cell {
        state.selected = BTreeSet::from([cell.0]);
        state.anchor = Some(cell.0);
        state.editing = Some(cell);
        state.focus_editor = true;
    }
    // Keyboard: Delete toggles staged deletion for the selection; Escape
    // clears it. Only when no text field has focus.
    let free = ui.memory(|m| m.focused().is_none());
    if free
        && !state.selected.is_empty()
        && editable(cx, page)
        && ui.input(|i| i.key_pressed(egui::Key::Delete))
    {
        let rows: Vec<usize> = state.selected.iter().copied().collect();
        toggle_delete(state, &rows);
    }
    if free && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.selected.clear();
    }
}

fn cell_editor_id(tab_id: u64, row: usize, column: usize) -> egui::Id {
    egui::Id::new(("cell-editor", tab_id, row, column))
}

fn field_editor_id(tab_id: u64, row: usize, column: usize) -> egui::Id {
    egui::Id::new(("field-editor", tab_id, row, column))
}

/// Stages drafts whose editor no longer has focus; Escape drops the focused
/// draft instead, reverting the field.
fn commit_unfocused_drafts(ui: &egui::Ui, tab_id: u64, state: &mut TableState) {
    let focused = ui.memory(|m| m.focused());
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let keys: Vec<(usize, usize)> = state.drafts.keys().copied().collect();
    for key in keys {
        let has_focus = focused == Some(cell_editor_id(tab_id, key.0, key.1))
            || focused == Some(field_editor_id(tab_id, key.0, key.1));
        let opening = state.editing == Some(key) && state.focus_editor;
        if has_focus && escape {
            state.drafts.remove(&key);
            if let Some(id) = focused {
                ui.memory_mut(|m| m.surrender_focus(id));
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

fn inspector(ui: &mut egui::Ui, cx: &TableContext<'_>, state: &mut TableState, page: &TablePage) {
    let columns = &page.metadata.columns;
    let single = (state.selected.len() == 1)
        .then(|| state.selected.first().copied())
        .flatten()
        .filter(|&i| i < page.rows.len());
    let pending = single.and_then(|i| state.pending.get(&i).cloned());
    let deleted = pending.as_ref().is_some_and(|p| p.deleted);
    // Header: 40 px, like `.row-inspector-header`.
    let header = Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_height(40.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new("Row")
                        .font(ui_font(13.0))
                        .strong()
                        .color(theme::TEXT_STRONG),
                );
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
                    if !summary.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(summary).font(mono(12.0)).color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    }
                }
                if deleted {
                    chip(ui, "Staged for deletion", theme::DANGER);
                } else if pending.is_some() {
                    chip(ui, "Edited", theme::MODIFIED);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::button(ui, Icon::Close, None, "Hide row inspector").clicked() {
                        state.inspector_open = false;
                    }
                });
            });
        });
    ui.painter().hline(
        ui.max_rect().x_range(),
        header.response.rect.bottom(),
        Stroke::new(1.0, theme::BORDER),
    );
    let Some(index) = single else {
        Frame::new()
            .inner_margin(Margin::symmetric(20, 28))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let message = if state.selected.len() > 1 {
                        format!(
                            "{} rows selected. Select a single row to inspect it.",
                            state.selected.len()
                        )
                    } else {
                        "Select a row to inspect and edit its fields.".to_owned()
                    };
                    ui.label(
                        RichText::new(message)
                            .font(ui_font(12.5))
                            .color(theme::FAINT),
                    );
                });
            });
        return;
    };
    let row = &page.rows[index];
    let values = pending.as_ref().map_or(&row[..], |p| &p.changes[..]);
    let footer = editable(cx, page);
    egui::ScrollArea::vertical()
        .id_salt(("inspector", cx.tab_id, cx.embedded))
        .auto_shrink([false, false])
        .max_height(ui.available_height() - if footer { 55.0 } else { 0.0 })
        .show(ui, |ui| {
            Frame::new().inner_margin(Margin::same(14)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                for column_index in 0..columns.len() {
                    // 5 px spacing on each side plus 2 px: the desktop's 12 px gap.
                    if column_index > 0 {
                        ui.add_space(2.0);
                    }
                    inspector_field(ui, cx, state, page, (index, column_index), values, deleted);
                }
            });
        });
    if footer {
        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0, theme::BORDER),
        );
        Frame::new()
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                let (icon, label, color) = if deleted {
                    (Icon::Undo, "Restore row", theme::TEXT)
                } else {
                    (Icon::Trash, "Delete row", theme::DANGER)
                };
                if secondary_icon_button(ui, icon, label, color, ui.available_width()).clicked() {
                    toggle_delete(state, &[index]);
                }
            });
    }
}

/// One `.inspector-field`: mono name, type, a right-aligned note, then a
/// 30 px input. Read-only fields (primary keys, deleted rows, read-only
/// profiles) render as plain muted text like the desktop's `[readonly]`.
fn inspector_field(
    ui: &mut egui::Ui,
    cx: &TableContext<'_>,
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
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            RichText::new(&column.name)
                .font(mono(11.5))
                .color(theme::SECONDARY),
        );
        ui.label(
            RichText::new(&column.data_type)
                .font(mono(11.0))
                .color(Color32::from_rgb(0x6f, 0x6f, 0x76)),
        );
        let note = if is_key {
            Some(("Primary key".to_owned(), theme::FAINT))
        } else if changed {
            Some((format!("was {}", display_value(original)), theme::MODIFIED))
        } else {
            None
        };
        if let Some((note, color)) = note {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(RichText::new(note).font(ui_font(11.0)).color(color))
                        .truncate(),
                );
            });
        }
    });
    let read_only = !editable_column(cx, page, column_index) || deleted || is_key;
    let mut text = state
        .drafts
        .get(&key)
        .cloned()
        .unwrap_or_else(|| editable_text(current));
    let (fill, stroke) = if read_only {
        (Color32::TRANSPARENT, Color32::TRANSPARENT)
    } else if changed {
        (
            theme::MODIFIED_SOFT,
            Color32::from_rgba_unmultiplied(240, 177, 76, 115),
        )
    } else {
        (Color32::from_rgb(0x23, 0x23, 0x26), theme::BORDER_STRONG)
    };
    let id = field_editor_id(cx.tab_id, index, column_index);
    let focused = ui.memory(|m| m.has_focus(id));
    let (fill, stroke) = if focused && !read_only {
        (theme::EDIT_SURFACE, theme::ACCENT)
    } else {
        (fill, stroke)
    };
    let frame = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(Margin::symmetric(10, 0));
    let response = frame
        .show(ui, |ui| {
            ui.set_height(28.0);
            ui.centered_and_justified(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .id(id)
                        .interactive(!read_only)
                        .frame(false)
                        .font(mono(12.0))
                        .text_color(if read_only { theme::MUTED } else { theme::TEXT })
                        .hint_text(
                            RichText::new(if current.is_null() { "NULL" } else { "" })
                                .italics()
                                .color(theme::FAINT),
                        )
                        .desired_width(f32::INFINITY),
                )
            })
            .inner
        })
        .inner;
    if response.changed() && !read_only {
        state.drafts.insert(key, text);
    }
}

/// `.secondary-button`: control fill, strong border, 30 px, icon + label.
fn secondary_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    color: Color32,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.0), Sense::click());
    let fill = if response.hovered() {
        theme::CONTROL_HOVER
    } else {
        theme::CONTROL
    };
    ui.painter().rect(
        rect,
        7.0,
        fill,
        Stroke::new(1.0, theme::BORDER_STRONG),
        egui::StrokeKind::Inside,
    );
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), ui_font(12.5), color);
    let total = 14.0 + 6.0 + galley.size().x;
    let start = rect.center().x - total / 2.0;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(egui::pos2(start + 7.0, rect.center().y), Vec2::splat(14.0)),
        icon,
        color,
    );
    ui.painter().galley(
        egui::pos2(start + 20.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
pub mod tests {
    use serde_json::json;

    use super::*;
    use dbm_core::demo::table_page as demo_page;

    pub fn page() -> TablePage {
        demo_page(&TablePageRequest {
            profile_id: Uuid::nil(),
            schema: "public".into(),
            table: "customers".into(),
            offset: 0,
            limit: 5,
            filters: vec![],
            order_by: None,
            include_total: Some(true),
        })
    }

    #[test]
    fn failed_page_loads_restore_the_shown_page_state() {
        let mut state = TableState::default();
        let request = TablePageRequest {
            limit: 5,
            ..state.request(Uuid::nil(), "public", "customers")
        };
        state.loaded(page(), request);
        state.page_index = 3;
        state.applied = vec![FilterCondition {
            column: "email".into(),
            operator: FilterOperator::Contains,
            value: Some("x".into()),
        }];
        state.loading = true;
        state.restore_loaded();
        assert_eq!(
            (state.page_index, state.limit, state.loading),
            (0, 5, false)
        );
        assert!(state.applied.is_empty());
    }

    #[test]
    fn staging_uses_typed_parsing_and_drops_no_op_edits() {
        let mut state = TableState {
            page: Some(page()),
            ..TableState::default()
        };
        stage_cell(&mut state, 0, 2, "no");
        assert_eq!(state.pending[&0].changes[2], json!(false));
        stage_cell(&mut state, 0, 3, "");
        assert_eq!(
            state.pending[&0].changes[3],
            Value::Null,
            "nullable empty is NULL"
        );
        stage_cell(&mut state, 0, 2, "true");
        stage_cell(&mut state, 0, 3, "Customer since 2001");
        assert!(
            state.pending.is_empty(),
            "reverting every field unstages the row"
        );
        toggle_delete(&mut state, &[1, 2]);
        assert_eq!(state.pending.len(), 2);
        stage_delete(&mut state, &[1], false);
        assert_eq!(state.pending.keys().copied().collect::<Vec<_>>(), vec![2]);
        toggle_delete(&mut state, &[2]);
        assert!(
            state.pending.is_empty(),
            "toggling an all-deleted selection restores it"
        );
    }

    #[test]
    fn copy_skips_deleted_rows_and_uses_staged_values() {
        let mut state = TableState {
            page: Some(page()),
            ..TableState::default()
        };
        stage_cell(&mut state, 0, 1, "ada@example.com");
        stage_delete(&mut state, &[1], true);
        let csv = state.csv(state.copyable_rows().into_iter());
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "id,email,active,note");
        assert_eq!(lines[1], "1,ada@example.com,true,Customer since 2001");
        assert!(lines[2].starts_with("3,"), "row 2 is staged for deletion");
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn pending_rows_preserve_pk_and_xmin() {
        let page = page();
        let p = pending_row(&page, &page.rows[0]);
        assert_eq!(p.primary_key, vec![json!(1)]);
        assert_eq!(p.xmin.as_deref(), Some("xmin-1"));
        assert_eq!(p.original.len(), page.metadata.columns.len());
    }
}
