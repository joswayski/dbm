//! A table tab's model (and an editable `SELECT *` result's): the page on
//! screen, the controls, selection, and staged edits and deletes. Ported
//! rule for rule from the egui app's `table_view.rs`; `view/table.rs` draws it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use dbm_core::cell_values::{numeric_column, parse_cell_input};
use dbm_core::models::{FilterCondition, FilterOperator, OrderSpec, TablePage, TablePageRequest};
use dbm_core::sql_text::{csv_document, display_value};
use serde_json::Value;
use uuid::Uuid;

use crate::messages::Message;

pub const MAX_PREVIEW_ROWS: u32 = 200;
pub const COLLAPSED_COLUMN_WIDTH: f32 = 76.0;
pub const PENDING_EXPORT_ERROR: &str = "Save or discard pending row changes before exporting.";
pub const PENDING_REFRESH_ERROR: &str = "Save or discard pending row changes before refreshing.";
/// Values above this size are previewed rather than edited.
pub const LARGE_VALUE_BYTES: usize = 64 * 1024;

pub const FILTER_OPERATORS: [(FilterOperator, &str); 13] = [
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

pub fn operator_label(operator: FilterOperator) -> &'static str {
    FILTER_OPERATORS
        .iter()
        .find(|(op, _)| *op == operator)
        .map_or("", |(_, label)| label)
}

pub fn filter_needs_value(operator: FilterOperator) -> bool {
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
    pub column: String,
    pub operator: FilterOperator,
    pub value: String,
}

impl FilterDraft {
    pub fn new(column: &str) -> Self {
        Self {
            column: column.to_owned(),
            operator: FilterOperator::Contains,
            value: String::new(),
        }
    }
}

/// The staged-row hover card.
#[derive(Clone, Copy, Debug)]
pub struct PreviewCard {
    pub row: usize,
    pub anchor: dbm_ui::Rect,
    pub close_at: Option<f64>,
}

pub struct TableState {
    pub page: Option<Arc<TablePage>>,
    /// The request that produced `page`, restored when a later load fails.
    pub loaded: Option<TablePageRequest>,
    /// The request in flight, recorded as `loaded` when its page arrives.
    pub requested: Option<TablePageRequest>,
    /// Loads sent and not yet answered; only the last belongs to `requested`.
    pub in_flight: u32,
    pub loading: bool,
    pub loading_since: Option<f64>,
    pub message: Option<Message>,
    pub columns_resized: bool,
    /// Column widths dragged away from their defaults, by index.
    pub widths: HashMap<usize, f32>,
    pub export_progress: Option<Arc<AtomicU64>>,
    pub page_index: u32,
    pub limit: u32,
    pub limit_input: String,
    pub filters: Vec<FilterDraft>,
    pub applied: Vec<FilterCondition>,
    pub order: Option<OrderSpec>,
    pub selected: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub pending: BTreeMap<usize, PendingRow>,
    /// Uncommitted editor text by (row, column), staged when its field
    /// loses focus or Enter is pressed and dropped on Escape.
    pub drafts: HashMap<(usize, usize), String>,
    pub editing: Option<(usize, usize)>,
    pub focus_editor: bool,
    pub inspector_open: bool,
    pub collapsed_columns: BTreeSet<usize>,
    pub preview: Option<PreviewCard>,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            page: None,
            loaded: None,
            requested: None,
            in_flight: 0,
            loading: false,
            loading_since: None,
            message: None,
            columns_resized: false,
            widths: HashMap::new(),
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
            preview: None,
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

    pub fn loaded(&mut self, page: TablePage, request: TablePageRequest) {
        self.loading = false;
        self.page_index = page.offset / page.limit.max(1);
        if self.filters.is_empty()
            && let Some(first) = page.metadata.columns.first()
        {
            self.filters.push(FilterDraft::new(&first.name));
        }
        self.loaded = Some(request);
        self.selected.clear();
        self.anchor = None;
        self.drafts.clear();
        self.editing = None;
        self.page = Some(Arc::new(page));
    }

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

    pub fn effective_order(&self) -> Option<OrderSpec> {
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
    pub fn csv(&self, rows: impl Iterator<Item = usize>) -> String {
        let Some(page) = &self.page else {
            return String::new();
        };
        let columns: Vec<String> = page.metadata.columns.iter().map(|c| c.name.clone()).collect();
        let values: Vec<Vec<Value>> = rows
            .filter_map(|i| {
                let row = page.rows.get(i)?;
                Some(self.pending.get(&i).map_or_else(|| row.clone(), |p| p.changes.clone()))
            })
            .collect();
        csv_document(&columns, &values)
    }

    pub fn copyable_rows(&self) -> Vec<usize> {
        let count = self.page.as_ref().map_or(0, |p| p.rows.len());
        (0..count).filter(|i| !self.pending.get(i).is_some_and(|p| p.deleted)).collect()
    }

    /// Applies the filter drafts that have a column and (when needed) a value.
    pub fn apply_filters(&mut self) {
        self.applied = self
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
        self.page_index = 0;
    }

    pub fn has_filters(&self) -> bool {
        !self.applied.is_empty()
            || self.filters.len() > 1
            || self.filters.iter().any(|f| !filter_needs_value(f.operator) || !f.value.trim().is_empty())
    }

    /// Clicking a header sorts by it, flipping direction on the current column.
    pub fn sort_by(&mut self, column: String) {
        let order = self.effective_order();
        let descending = order.as_ref().is_some_and(|o| o.column == column && !o.descending);
        self.order = Some(OrderSpec { column, descending });
        self.page_index = 0;
    }

    /// Plain, Shift (range) and Ctrl (toggle) row clicks.
    pub fn click_row(&mut self, index: usize, shift: bool, ctrl: bool) {
        if shift {
            let anchor = self.anchor.unwrap_or(index);
            self.selected = (anchor.min(index)..=anchor.max(index)).collect();
        } else if ctrl {
            if !self.selected.remove(&index) {
                self.selected.insert(index);
            }
            self.anchor = Some(index);
        } else {
            self.selected = BTreeSet::from([index]);
            self.anchor = Some(index);
        }
    }

    /// Undoing a delete keeps the row's edits, as the desktop does.
    pub fn discard_row(&mut self, row: usize) {
        match self.pending.get_mut(&row) {
            Some(p) if p.deleted && p.changes != p.original => p.deleted = false,
            _ => {
                self.pending.remove(&row);
            }
        }
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
            page.metadata.columns.iter().position(|c| &c.name == key).map(|i| original[i].clone())
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
    let Some(values) = page.rows.get(row) else { return };
    let pending = state.pending.entry(row).or_insert_with(|| pending_row(page, values));
    pending.changes[column] =
        parse_cell_input(text, &page.metadata.columns[column], &pending.original[column]);
    if !pending.deleted && pending.changes == pending.original {
        state.pending.remove(&row);
    }
}

/// Stages (or, when every row is already staged, undoes) deletion of `rows`.
pub fn toggle_delete(state: &mut TableState, rows: &[usize]) {
    let all_deleted = rows.iter().all(|r| state.pending.get(r).is_some_and(|p| p.deleted));
    stage_delete(state, rows, !all_deleted);
}

pub fn stage_delete(state: &mut TableState, rows: &[usize], deleted: bool) {
    let Some(page) = &state.page else { return };
    for &row in rows {
        if deleted {
            if let Some(values) = page.rows.get(row) {
                state.pending.entry(row).or_insert_with(|| pending_row(page, values)).deleted = true;
            }
        } else if let Some(pending) = state.pending.get_mut(&row) {
            pending.deleted = false;
            if pending.changes == pending.original {
                state.pending.remove(&row);
            }
        }
    }
}

/// The desktop's `defaultColumnWidth`, rule for rule.
pub fn default_column_width(data_type: &str) -> f32 {
    let lower = data_type.to_ascii_lowercase();
    if lower.contains("json") || lower.contains("array") {
        320.0
    } else if lower.contains("timestamp") {
        230.0
    } else if lower.contains("text") || lower.contains("character") || lower.contains("uuid") {
        220.0
    } else if lower.starts_with("bool") {
        110.0
    } else if numeric_column(data_type) {
        130.0
    } else {
        160.0
    }
}

/// The first `chars` characters of `display_value(value)`, without copying a
/// long string or serializing a large JSON document every frame.
pub fn display_prefix(value: &Value, chars: usize) -> String {
    struct Capped(Vec<u8>, usize);
    impl std::io::Write for Capped {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let room = self.1.saturating_sub(self.0.len());
            if room == 0 {
                return Err(std::io::ErrorKind::WriteZero.into());
            }
            let take = room.min(buf.len());
            self.0.extend_from_slice(&buf[..take]);
            Ok(take)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    match value {
        Value::String(text) => text.chars().take(chars).collect(),
        Value::Array(_) | Value::Object(_) => {
            let mut w = Capped(Vec::new(), chars * 4);
            let _ = serde_json::to_writer(&mut w, value);
            String::from_utf8_lossy(&w.0).chars().take(chars).collect()
        }
        other => display_value(other),
    }
}

/// A grid cell's one-line text.
pub fn cell_display(value: &Value) -> String {
    if value.is_null() {
        "NULL".into()
    } else {
        display_prefix(value, 200).replace('\n', " ")
    }
}

/// A human-readable size for a value too large to edit in place.
pub fn large_value_size(value: &Value) -> Option<String> {
    struct Counting(usize);
    impl std::io::Write for Counting {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let len = match value {
        Value::String(text) => text.len(),
        Value::Null | Value::Bool(_) | Value::Number(_) => return None,
        other => {
            let mut c = Counting(0);
            let _ = serde_json::to_writer(&mut c, other);
            c.0
        }
    };
    (len > LARGE_VALUE_BYTES).then(|| {
        if len >= 1024 * 1024 {
            format!("{:.1} MiB", len as f64 / (1024.0 * 1024.0))
        } else {
            format!("{} KiB", len / 1024)
        }
    })
}

pub fn rows_label(count: usize, which: &str) -> String {
    format!("{count} {which} {}", if count == 1 { "row" } else { "rows" })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use dbm_core::demo::table_page as demo_page;
    use serde_json::json;

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

    fn state() -> TableState {
        let mut s = TableState::default();
        let p = page();
        let request = s.request(Uuid::nil(), "public", "customers");
        s.loaded(p, request);
        s
    }

    #[test]
    fn staging_uses_typed_parsing_and_drops_no_op_edits() {
        let mut s = state();
        let page = s.page.clone().unwrap();
        let column = page.metadata.columns.iter().position(|c| !page.metadata.primary_key.contains(&c.name)).unwrap();
        let original = dbm_core::cell_values::editable_text(&page.rows[0][column]);
        stage_cell(&mut s, 0, column, "changed value");
        assert!(s.pending.contains_key(&0));
        stage_cell(&mut s, 0, column, &original);
        assert!(s.pending.is_empty(), "reverting the text unstages the row");
    }

    #[test]
    fn copy_skips_deleted_rows_and_uses_staged_values() {
        let mut s = state();
        toggle_delete(&mut s, &[1]);
        let rows = s.copyable_rows();
        assert!(!rows.contains(&1));
        assert_eq!(rows.len(), s.page.as_ref().unwrap().rows.len() - 1);
        // All rows staged: toggling again undoes the deletion.
        toggle_delete(&mut s, &[1]);
        assert!(s.pending.is_empty());
    }

    #[test]
    fn undoing_a_delete_keeps_the_rows_edits() {
        let mut s = state();
        let page = s.page.clone().unwrap();
        let column = page.metadata.columns.iter().position(|c| !page.metadata.primary_key.contains(&c.name)).unwrap();
        stage_cell(&mut s, 2, column, "edit");
        toggle_delete(&mut s, &[2]);
        s.discard_row(2);
        let p = &s.pending[&2];
        assert!(!p.deleted && p.changes != p.original);
        s.discard_row(2);
        assert!(s.pending.is_empty());
    }

    #[test]
    fn failed_page_loads_restore_the_shown_page_state() {
        let mut s = state();
        s.page_index = 4;
        s.limit = 10;
        s.order = Some(OrderSpec { column: "x".into(), descending: true });
        s.restore_loaded();
        assert_eq!((s.page_index, s.limit), (0, MAX_PREVIEW_ROWS));
        assert!(s.order.is_none());
    }

    #[test]
    fn shift_selects_a_range_from_the_anchor_and_ctrl_toggles() {
        let mut s = state();
        s.click_row(1, false, false);
        s.click_row(3, true, false);
        assert_eq!(s.selected, BTreeSet::from([1, 2, 3]));
        s.click_row(2, false, true);
        assert_eq!(s.selected, BTreeSet::from([1, 3]));
    }

    #[test]
    fn large_values_are_previewed_not_edited() {
        assert!(large_value_size(&json!("x".repeat(LARGE_VALUE_BYTES + 1))).is_some());
        assert!(large_value_size(&json!("small")).is_none());
        assert_eq!(large_value_size(&json!("x".repeat(2 * 1024 * 1024))).as_deref(), Some("2.0 MiB"));
    }

    #[test]
    fn sorting_the_current_column_flips_direction_and_a_new_column_starts_ascending() {
        let mut s = state();
        let pk = s.page.as_ref().unwrap().metadata.primary_key[0].clone();
        s.sort_by(pk.clone());
        assert!(s.order.as_ref().unwrap().descending, "the pk is the implicit ascending sort");
        s.sort_by("email".into());
        assert!(!s.order.as_ref().unwrap().descending);
    }
}
