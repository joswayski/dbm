//! Data grids on `GtkColumnView`: only visible rows get widgets, so 10,000
//! result rows scroll as smoothly as one page. This module holds the
//! read-only query result grid and the cell pieces the table grid shares.

use std::cell::RefCell;
use std::rc::Rc;

use dbm_core::cell_values::numeric_column;
use dbm_core::models::QueryColumn;
use dbm_core::sql_text::display_value;
use gtk::glib::BoxedAnyObject;
use gtk::prelude::*;
use serde_json::Value;

use crate::icons::Icon;
use crate::state::display_prefix;
use crate::ui;

/// One cell: the value, NULL in faint italics, and a copy button that shows
/// while the cell is hovered.
pub struct Cell {
    pub root: gtk::Box,
    pub label: gtk::Label,
    pub value: Rc<RefCell<Value>>,
}

impl Cell {
    pub fn new(window: impl Fn() -> Option<gtk::Widget> + 'static) -> Self {
        let root = ui::hbox(0);
        root.add_css_class("cell");
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_single_line_mode(true);
        label.set_margin_start(10);
        label.add_css_class("cell-text");
        let copy = ui::button("toolbar-button", Some(Icon::Copy), "", "Copy value");
        copy.add_css_class("cell-copy");
        copy.set_focus_on_click(false);
        let value = Rc::new(RefCell::new(Value::Null));
        let shared = value.clone();
        copy.connect_clicked(move |_| {
            if let Some(widget) = window() {
                widget
                    .clipboard()
                    .set_text(&display_value(&shared.borrow()));
            }
        });
        root.append(&label);
        root.append(&copy);
        Self { root, label, value }
    }

    pub fn show(&self, value: &Value, numeric: bool) {
        let text = display_prefix(value, 200).replace('\n', " ");
        if self.label.text() != text {
            self.label.set_text(&text);
        }
        self.label.set_xalign(if numeric { 1.0 } else { 0.0 });
        if value.is_null() {
            self.label.add_css_class("null");
        } else {
            self.label.remove_css_class("null");
        }
        // Long values show in full on hover.
        let tooltip = (text.chars().count() > 24).then(|| display_prefix(value, 2000));
        self.label.set_tooltip_text(tooltip.as_deref());
        *self.value.borrow_mut() = value.clone();
    }
}

/// The row index a list item holds.
pub fn row_of(item: &gtk::ListItem) -> Option<usize> {
    item.item()
        .and_downcast::<BoxedAnyObject>()
        .map(|object| *object.borrow::<usize>())
}

/// A store of `count` row indexes.
pub fn fill(store: &gtk::gio::ListStore, count: usize) {
    let rows: Vec<BoxedAnyObject> = (0..count).map(BoxedAnyObject::new).collect();
    store.splice(0, store.n_items(), &rows);
}

/// The desktop's `defaultColumnWidth`, rule for rule.
pub fn default_column_width(data_type: &str) -> i32 {
    let lower = data_type.to_ascii_lowercase();
    if lower.contains("json") || lower.contains("array") {
        320
    } else if lower.contains("timestamp") {
        230
    } else if lower.contains("text") || lower.contains("character") || lower.contains("uuid") {
        220
    } else if lower.starts_with("bool") {
        110
    } else if numeric_column(data_type) {
        130
    } else {
        160
    }
}

/// Read-only query results.
pub struct ResultGrid {
    pub root: gtk::ScrolledWindow,
    view: gtk::ColumnView,
    store: gtk::gio::ListStore,
    rows: Rc<RefCell<Rc<Vec<Vec<Value>>>>>,
    key: RefCell<Option<(usize, usize, *const Vec<Value>)>>,
    cells: Cells<Cell>,
}

impl ResultGrid {
    pub fn new() -> Self {
        let store = gtk::gio::ListStore::new::<BoxedAnyObject>();
        let selection = gtk::NoSelection::new(Some(store.clone()));
        let view = gtk::ColumnView::new(Some(selection));
        view.add_css_class("grid");
        view.set_show_row_separators(true);
        view.set_show_column_separators(true);
        view.set_reorderable(false);
        let root = gtk::ScrolledWindow::new();
        root.set_child(Some(&view));
        root.set_vexpand(true);
        root.set_hexpand(true);
        Self {
            root,
            view,
            store,
            rows: Rc::new(RefCell::new(Rc::new(Vec::new()))),
            key: RefCell::new(None),
            cells: Rc::default(),
        }
    }

    /// Shows a new result; the same result again changes nothing.
    pub fn show(&self, columns: &[QueryColumn], rows: &[Vec<Value>]) {
        let key = (columns.len(), rows.len(), rows.as_ptr());
        if *self.key.borrow() == Some(key) {
            return;
        }
        *self.key.borrow_mut() = Some(key);
        while let Some(column) = self
            .view
            .columns()
            .item(0)
            .and_downcast::<gtk::ColumnViewColumn>()
        {
            self.view.remove_column(&column);
        }
        *self.rows.borrow_mut() = Rc::new(rows.to_vec());
        let share = columns.len() < 6;
        for (index, column) in columns.iter().enumerate() {
            let numeric = numeric_column(&column.data_type);
            let factory = gtk::SignalListItemFactory::new();
            let view = self.view.downgrade();
            let cells = self.cells.clone();
            factory.connect_setup(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let view = view.clone();
                let cell = Cell::new(move || view.upgrade().map(|v| v.upcast()));
                item.set_child(Some(&cell.root));
                cells
                    .borrow_mut()
                    .insert(cell.root.clone().upcast(), Rc::new(cell));
            });
            let cells = self.cells.clone();
            factory.connect_teardown(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(root) = item.child() {
                    cells.borrow_mut().remove(&root);
                }
            });
            let rows = self.rows.clone();
            let cells = self.cells.clone();
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let (Some(row), Some(root)) = (row_of(item), item.child()) else {
                    return;
                };
                let rows = rows.borrow();
                let value = rows
                    .get(row)
                    .and_then(|r| r.get(index))
                    .cloned()
                    .unwrap_or(Value::Null);
                if let Some(cell) = cells.borrow().get(&root) {
                    cell.show(&value, numeric);
                }
            });
            let view_column = gtk::ColumnViewColumn::new(Some(&column.name), Some(factory));
            view_column.set_resizable(true);
            view_column.set_fixed_width(170);
            view_column.set_expand(share);
            self.view.append_column(&view_column);
        }
        fill(&self.store, rows.len());
    }
}

/// Cells by their root widget, so a bind finds the cell it fills; entries
/// go when GTK tears the widget down.
pub type Cells<T> = Rc<RefCell<std::collections::HashMap<gtk::Widget, Rc<T>>>>;
