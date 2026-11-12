//! Data grids on `GtkColumnView`: only visible rows get widgets, so 10,000
//! result rows scroll as smoothly as one page. This module holds the
//! read-only query result grid and the cell pieces the table grid shares.

use std::cell::RefCell;
use std::rc::Rc;

use dbm_core::cell_values::numeric_column;
use dbm_core::models::QueryColumn;
use dbm_core::sql_text::display_value;
use gtk::glib::{self, BoxedAnyObject};
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
    pub fn new() -> Self {
        let root = ui::hbox(0);
        root.add_css_class("cell");
        let label = cell_label();
        root.append(&label);
        let value = Rc::new(RefCell::new(Value::Null));
        let shared = value.clone();
        HoverCopy::attach(
            &root,
            || true,
            move || Some(display_value(&shared.borrow())),
        );
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

/// A cell's value label.
pub fn cell_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label.set_margin_start(10);
    label.add_css_class("cell-text");
    label
}

/// A cell's copy button, built only while the pointer is over the cell: a
/// button in every cell costs a few widgets, an icon render and accessible
/// objects each, for a control only one cell shows at a time.
#[derive(Clone, Default)]
pub struct HoverCopy(Rc<RefCell<Option<gtk::Button>>>);

impl HoverCopy {
    /// Shows the button on `root` on hover when `allowed()`; it copies `text()`.
    pub fn attach(
        root: &gtk::Box,
        allowed: impl Fn() -> bool + 'static,
        text: impl Fn() -> Option<String> + 'static,
    ) -> Self {
        let copy = Self::default();
        let text = Rc::new(text);
        let motion = gtk::EventControllerMotion::new();
        let (slot, weak) = (copy.clone(), root.downgrade());
        motion.connect_enter(move |_, _, _| {
            if let Some(root) = weak.upgrade()
                && allowed()
            {
                slot.show(&root, text.clone());
            }
        });
        let (slot, weak) = (copy.clone(), root.downgrade());
        motion.connect_leave(move |_| {
            if let Some(root) = weak.upgrade() {
                slot.hide(&root);
            }
        });
        root.add_controller(motion);
        copy
    }

    fn show(&self, root: &gtk::Box, text: Rc<impl Fn() -> Option<String> + 'static>) {
        if self.0.borrow().is_some() {
            return;
        }
        let button = ui::button("toolbar-button", Some(Icon::Copy), "", "Copy value");
        button.add_css_class("cell-copy");
        button.set_focus_on_click(false);
        button.connect_clicked(move |button| {
            if let Some(text) = text() {
                button.clipboard().set_text(&text);
            }
        });
        root.add_css_class("copying");
        root.append(&button);
        *self.0.borrow_mut() = Some(button);
    }

    pub fn hide(&self, root: &gtk::Box) {
        if let Some(button) = self.0.borrow_mut().take() {
            root.remove(&button);
            root.remove_css_class("copying");
        }
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
type Values = Rc<dyn Fn(usize, usize) -> Result<Value, ()>>;

/// Column names, row count, and row storage of the result on screen.
type ResultKey = (Vec<String>, usize, *const Vec<Value>);

pub struct ResultGrid {
    pub root: gtk::ScrolledWindow,
    view: gtk::ColumnView,
    store: gtk::gio::ListStore,
    /// The value at (row, column) of the shown result, read from where the
    /// result lives so the grid keeps no copy of its rows; `Err` while that
    /// is busy.
    value: Values,
    key: RefCell<Option<ResultKey>>,
    cells: Cells<Cell>,
}

impl ResultGrid {
    pub fn new(value: impl Fn(usize, usize) -> Result<Value, ()> + 'static) -> Self {
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
            value: Rc::new(value),
            key: RefCell::new(None),
            cells: Rc::default(),
        }
    }

    /// Shows a new result; the same result again changes nothing.
    pub fn show(&self, columns: &[QueryColumn], rows: &[Vec<Value>]) {
        // Empty results all share one dangling row pointer, so the column
        // names tell two of them apart.
        let names = columns.iter().map(|c| c.name.clone()).collect();
        let key = Some((names, rows.len(), rows.as_ptr()));
        if *self.key.borrow() == key {
            return;
        }
        *self.key.borrow_mut() = key;
        while let Some(column) = self
            .view
            .columns()
            .item(0)
            .and_downcast::<gtk::ColumnViewColumn>()
        {
            self.view.remove_column(&column);
        }
        let share = columns.len() < 6;
        for (index, column) in columns.iter().enumerate() {
            let numeric = numeric_column(&column.data_type);
            let factory = gtk::SignalListItemFactory::new();
            let cells = self.cells.clone();
            factory.connect_setup(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let cell = Cell::new();
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
            let value = self.value.clone();
            let cells = self.cells.clone();
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let (Some(row), Some(root)) = (row_of(item), item.child()) else {
                    return;
                };
                if let Some(cell) = cells.borrow().get(&root).cloned() {
                    show_value(cell, value.clone(), row, index, numeric);
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

/// Fills `cell` with (row, column), retrying once the result is free.
fn show_value(cell: Rc<Cell>, value: Values, row: usize, column: usize, numeric: bool) {
    match value(row, column) {
        Ok(v) => cell.show(&v, numeric),
        Err(()) => {
            glib::idle_add_local_once(move || show_value(cell, value, row, column, numeric));
        }
    }
}

/// Cells by their root widget, so a bind finds the cell it fills; entries
/// go when GTK tears the widget down.
pub type Cells<T> = Rc<RefCell<std::collections::HashMap<gtk::Widget, Rc<T>>>>;
