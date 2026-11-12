//! Table tabs and editable `SELECT *` results: toolbar, filters, the grid
//! (a `GtkColumnView`, so only visible rows have widgets), row inspector,
//! pending-changes bar, status bar, staged-row preview card, and the
//! delayed Refreshing overlay.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use dbm_core::cell_values::{editable_text, numeric_column};
use dbm_core::models::{FilterOperator, OrderSpec, TablePage};
use dbm_core::sql_text::{display_value, inline_diff};
use gtk::glib::{self, BoxedAnyObject};
use gtk::prelude::*;
use serde_json::Value;

use crate::app::{App, set_text};
use crate::grid::{Cells, HoverCopy, cell_label, default_column_width, fill, row_of};
use crate::icons::{self, Icon};
use crate::messages::Slot;
use crate::state::{
    FILTER_OPERATORS, FilterDraft, MAX_PREVIEW_ROWS, RequestKind, State, TableState, count,
    display_prefix, editable, editable_column, filter_needs_value, large_value_size, plural,
    stage_cell, toggle_delete,
};
use crate::ui;

const COLLAPSED_COLUMN_WIDTH: i32 = 76;

/// One grid cell, reused by GTK for whichever row scrolls into view.
struct TableCell {
    root: gtk::Box,
    label: gtk::Label,
    /// The in-grid editor, built when the cell starts editing and dropped
    /// when it stops, so idle cells carry only their label.
    entry: RefCell<Option<gtk::Entry>>,
    new_entry: Box<dyn Fn() -> gtk::Entry>,
    copy: HoverCopy,
    /// The (row, column) it shows while bound.
    pos: Rc<Cell<Option<(usize, usize)>>>,
    editing: Cell<bool>,
}

pub struct TableView {
    pub root: gtk::Box,
    tab_id: u64,
    embedded: bool,
    app: App,
    // toolbar
    title: gtk::Label,
    counts: gtk::Label,
    refresh: gtk::Button,
    copy: gtk::Button,
    export: gtk::Button,
    selected_menu: gtk::MenuButton,
    selected_label: gtk::Label,
    inspector_toggle: gtk::Button,
    // filters
    filters: gtk::Box,
    filter_rows: gtk::Box,
    filter_signature: RefCell<String>,
    limit: gtk::SpinButton,
    sort: gtk::DropDown,
    sort_names: RefCell<Vec<String>>,
    direction_group: gtk::Box,
    direction: gtk::DropDown,
    reset_columns: gtk::Button,
    clear: gtk::Button,
    message: Slot,
    // body
    body: gtk::Stack,
    view: gtk::ColumnView,
    store: gtk::gio::ListStore,
    selection: gtk::MultiSelection,
    columns: RefCell<Vec<gtk::ColumnViewColumn>>,
    column_key: RefCell<String>,
    page_key: Cell<usize>,
    cells: Cells<TableCell>,
    style_key: RefCell<String>,
    refreshing: gtk::Box,
    // inspector
    inspector: gtk::Box,
    inspector_body: gtk::Box,
    inspector_key: RefCell<String>,
    inspector_entries: RefCell<Vec<(usize, gtk::Entry)>>,
    // bars
    pending_bar: gtk::Box,
    pending_count: gtk::Label,
    pending_chips: gtk::Box,
    save: gtk::Button,
    discard: gtk::Button,
    status: gtk::Box,
    rows_label: gtk::Label,
    per_page: gtk::Label,
    page_label: gtk::Label,
    previous: gtk::Button,
    next: gtk::Button,
    // staged-row preview
    preview: gtk::Popover,
    preview_row: Cell<Option<usize>>,
    preview_key: RefCell<String>,
    preview_hover: Cell<bool>,
    preview_close: RefCell<Option<glib::SourceId>>,
}

impl TableView {
    pub fn new(app: &App, tab_id: u64, embedded: bool) -> Rc<Self> {
        let root = ui::vbox(0);
        root.add_css_class("canvas");
        // Toolbar.
        let toolbar = ui::hbox(6);
        toolbar.add_css_class("toolbar");
        let title_icon = icons::image(Icon::Table, 15);
        title_icon.add_css_class("muted");
        let title = ui::ellipsized("", &[]);
        let counts = ui::ellipsized("", &["size-12", "faint"]);
        counts.set_hexpand(true);
        let refresh = ui::button(
            "toolbar-button",
            Some(Icon::Refresh),
            "Refresh",
            "Reload the current page with the same filters and sort",
        );
        let copy = ui::button(
            "toolbar-button",
            Some(Icon::Copy),
            "Copy visible",
            "Copies only the current preview page",
        );
        let export = ui::button(
            "toolbar-button",
            Some(Icon::Download),
            "Export all",
            "Prompts for a location and exports every filtered row",
        );
        let selected_label = gtk::Label::new(None);
        let selected_content = ui::hbox(5);
        selected_content.append(&selected_label);
        selected_content.append(&icons::image(Icon::ChevronDown, 12));
        let selected_menu = gtk::MenuButton::new();
        selected_menu.add_css_class("menu-trigger");
        selected_menu.set_child(Some(&selected_content));
        selected_menu.set_tooltip_text(Some("Actions for the selected rows"));
        selected_menu.set_valign(gtk::Align::Center);
        let inspector_toggle = gtk::Button::new();
        inspector_toggle.add_css_class("icon-toggle");
        inspector_toggle.set_child(Some(&icons::image(Icon::Inspector, 14)));
        inspector_toggle.set_valign(gtk::Align::Center);
        toolbar.append(&title_icon);
        toolbar.append(&title);
        toolbar.append(&counts);
        toolbar.append(&refresh);
        toolbar.append(&copy);
        toolbar.append(&export);
        toolbar.append(&selected_menu);
        toolbar.append(&inspector_toggle);
        // Filters.
        let filters = ui::vbox(6);
        filters.add_css_class("filters");
        let header = ui::hbox(8);
        let filter_icon = icons::image(Icon::Filter, 13);
        filter_icon.add_css_class("faint");
        header.append(&filter_icon);
        header.append(&ui::label("Filters", &["bold", "size-12", "secondary"]));
        header.append(&ui::label("All filters must match", &["size-12", "faint"]));
        let add_filter = ui::button(
            "text-button",
            Some(Icon::Plus),
            "Add filter",
            "Add another filter",
        );
        header.append(&add_filter);
        let controls = ui::hbox(12);
        controls.set_halign(gtk::Align::End);
        controls.set_hexpand(true);
        let group = |label: &str, widget: &gtk::Widget| {
            let group = ui::hbox(6);
            let text = ui::label(label, &["size-11", "faint"]);
            group.append(&text);
            group.append(widget);
            group
        };
        let limit = gtk::SpinButton::with_range(1.0, f64::from(MAX_PREVIEW_ROWS), 1.0);
        limit.set_value(f64::from(MAX_PREVIEW_ROWS));
        limit.set_width_chars(4);
        limit.add_css_class("mono");
        let sort = gtk::DropDown::from_strings(&[]);
        sort.set_size_request(170, -1);
        let direction = gtk::DropDown::from_strings(&["Ascending", "Descending"]);
        direction.set_size_request(142, -1);
        let direction_group = group("Direction", direction.upcast_ref());
        let reset_columns = ui::button(
            "text-button",
            None,
            "Reset columns",
            "Restore every column's default width",
        );
        let clear = ui::button("secondary-button", None, "Clear", "Clear every filter");
        let apply = ui::button("primary", None, "Apply filters", "");
        controls.append(&group("Preview limit", limit.upcast_ref()));
        controls.append(&group("Sort by", sort.upcast_ref()));
        controls.append(&direction_group);
        controls.append(&reset_columns);
        let buttons = ui::hbox(6);
        buttons.append(&clear);
        buttons.append(&apply);
        controls.append(&buttons);
        let filter_rows = ui::vbox(4);
        filters.append(&header);
        filters.append(&controls);
        filters.append(&filter_rows);
        let message = Slot::new();
        message.root.set_margin_bottom(8);
        // Grid.
        let store = gtk::gio::ListStore::new::<BoxedAnyObject>();
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let view = gtk::ColumnView::new(Some(selection.clone()));
        view.add_css_class("grid");
        view.add_css_class("table-grid");
        view.set_show_row_separators(true);
        view.set_show_column_separators(true);
        view.set_reorderable(false);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&view));
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);
        let dim = ui::vbox(0);
        dim.add_css_class("refreshing-dim");
        let pill = ui::label("Refreshing…", &["refreshing-pill"]);
        pill.set_halign(gtk::Align::Center);
        pill.set_valign(gtk::Align::Center);
        pill.set_vexpand(true);
        dim.append(&pill);
        dim.set_visible(false);
        dim.set_can_target(false);
        let grid_overlay = gtk::Overlay::new();
        grid_overlay.set_child(Some(&scroller));
        grid_overlay.add_overlay(&dim);
        let skeleton = ui::vbox(0);
        let header_band = ui::hbox(0);
        header_band.add_css_class("skeleton");
        header_band.add_css_class("header");
        skeleton.append(&header_band);
        for _ in 0..5 {
            let band = ui::hbox(0);
            band.add_css_class("skeleton");
            skeleton.append(&band);
        }
        let retry = ui::button(
            "secondary-button",
            Some(Icon::Refresh),
            "Couldn't load rows · Retry",
            "",
        );
        retry.set_halign(gtk::Align::Center);
        retry.set_valign(gtk::Align::Center);
        let empty = ui::label("No rows match this view.", &["muted"]);
        empty.set_xalign(0.5);
        let body = gtk::Stack::new();
        body.set_vexpand(true);
        body.set_hexpand(true);
        body.add_named(&skeleton, Some("skeleton"));
        body.add_named(&retry, Some("retry"));
        body.add_named(&empty, Some("empty"));
        body.add_named(&grid_overlay, Some("grid"));
        // Inspector.
        let inspector = ui::vbox(0);
        inspector.add_css_class("inspector");
        inspector.set_size_request(300, -1);
        // Fixed width as in egui: hexpand children (entries) would otherwise
        // make the inspector share the grid's extra space.
        inspector.set_hexpand(false);
        let inspector_body = ui::vbox(0);
        inspector_body.set_vexpand(true);
        inspector.append(&inspector_body);
        let middle = ui::hbox(0);
        middle.set_vexpand(true);
        middle.append(&body);
        middle.append(&inspector);
        // Bars.
        let pending_bar = ui::hbox(8);
        pending_bar.add_css_class("pending-bar");
        let pending_dot = ui::dot("#f0b14c", true, 8);
        let pending_count = ui::label("", &["bold", "strong"]);
        let pending_chips = ui::hbox(6);
        pending_chips.set_hexpand(true);
        let discard = ui::button("secondary-button", None, "Discard changes", "");
        let save = ui::button("primary", None, "Save changes", "");
        pending_bar.append(&pending_dot);
        pending_bar.append(&pending_count);
        pending_bar.append(&pending_chips);
        pending_bar.append(&discard);
        pending_bar.append(&save);
        let status = ui::hbox(6);
        status.add_css_class("statusbar");
        let rows_label = ui::label("", &[]);
        rows_label.set_hexpand(true);
        let per_page = ui::label("", &[]);
        let previous = ui::button(
            "toolbar-button",
            Some(Icon::ChevronLeft),
            "",
            "Previous page",
        );
        let page_label = ui::label("", &["page"]);
        let next = ui::button("toolbar-button", Some(Icon::ChevronRight), "", "Next page");
        status.append(&rows_label);
        status.append(&per_page);
        status.append(&previous);
        status.append(&page_label);
        status.append(&next);
        root.append(&toolbar);
        root.append(&filters);
        root.append(&message.root);
        root.append(&middle);
        root.append(&pending_bar);
        root.append(&status);
        let preview = gtk::Popover::new();
        preview.set_has_arrow(false);
        preview.set_autohide(false);
        preview.set_can_focus(false);
        preview.set_position(gtk::PositionType::Bottom);
        preview.add_css_class("preview");
        preview.set_parent(&view);
        let table = Rc::new(Self {
            root,
            tab_id,
            embedded,
            app: app.clone(),
            title,
            counts,
            refresh,
            copy,
            export,
            selected_menu,
            selected_label,
            inspector_toggle,
            filters,
            filter_rows,
            filter_signature: RefCell::new("unset".into()),
            limit,
            sort,
            sort_names: RefCell::new(Vec::new()),
            direction_group,
            direction,
            reset_columns,
            clear,
            message,
            body,
            view,
            store,
            selection,
            columns: RefCell::new(Vec::new()),
            column_key: RefCell::new(String::new()),
            page_key: Cell::new(0),
            cells: Rc::default(),
            style_key: RefCell::new(String::new()),
            refreshing: dim,
            inspector,
            inspector_body,
            inspector_key: RefCell::new("unset".into()),
            inspector_entries: RefCell::new(Vec::new()),
            pending_bar,
            pending_count,
            pending_chips,
            save,
            discard,
            status,
            rows_label,
            per_page,
            page_label,
            previous,
            next,
            preview,
            preview_row: Cell::new(None),
            preview_key: RefCell::new(String::new()),
            preview_hover: Cell::new(false),
            preview_close: RefCell::new(None),
        });
        table.connect(&add_filter, &apply, &retry);
        table
    }

    pub fn teardown(&self) {
        if let Some(source) = self.preview_close.borrow_mut().take() {
            source.remove();
        }
        self.preview.unparent();
    }

    fn connect(
        self: &Rc<Self>,
        add_filter: &gtk::Button,
        apply: &gtk::Button,
        retry: &gtk::Button,
    ) {
        let tab = self.tab_id;
        let app = &self.app;
        let handle = app.clone();
        self.refresh
            .connect_clicked(move |_| handle.update(|state| state.reload_table(tab)));
        let handle = app.clone();
        retry.connect_clicked(move |_| handle.update(|state| state.reload_table(tab)));
        let handle = app.clone();
        self.copy.connect_clicked(move |_| {
            handle.update(|state| {
                let rows = state
                    .tables
                    .get(&tab)
                    .map(TableState::copyable_rows)
                    .unwrap_or_default();
                state.copy_table_rows(tab, rows, "visible");
            });
        });
        let handle = app.clone();
        self.export
            .connect_clicked(move |_| handle.update(|state| state.request_export(tab)));
        let handle = app.clone();
        self.inspector_toggle.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.inspector_open = !table.inspector_open;
                }
            });
        });
        let handle = app.clone();
        let weak = Rc::downgrade(self);
        self.selected_menu.set_create_popup_func(move |button| {
            let Some(view) = weak.upgrade() else { return };
            let popover = ui::menu(view.selection_items(&handle, true), 220);
            button.set_popover(Some(&popover));
        });
        let handle = app.clone();
        add_filter.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    let first = table
                        .page
                        .as_ref()
                        .and_then(|p| p.metadata.columns.first())
                        .map_or_else(String::new, |c| c.name.clone());
                    table.filters.push(FilterDraft::new(&first));
                }
            });
        });
        let handle = app.clone();
        apply.connect_clicked(move |_| handle.update(|state| apply_filters(state, tab)));
        let handle = app.clone();
        self.clear.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.clear_filters();
                }
                state.reload_table(tab);
            });
        });
        let handle = app.clone();
        self.limit.connect_value_changed(move |spin| {
            let limit = spin.value_as_int().clamp(1, MAX_PREVIEW_ROWS as i32) as u32;
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab)
                    && table.limit != limit
                {
                    table.limit = limit;
                    table.page_index = 0;
                    state.reload_table(tab);
                }
            });
        });
        let handle = app.clone();
        let weak = Rc::downgrade(self);
        self.sort.connect_selected_notify(move |dropdown| {
            let Some(view) = weak.upgrade() else { return };
            let Some(column) = view
                .sort_names
                .borrow()
                .get(dropdown.selected() as usize)
                .cloned()
            else {
                return;
            };
            handle.update(|state| set_order(state, tab, column, None));
        });
        let handle = app.clone();
        self.direction.connect_selected_notify(move |dropdown| {
            let descending = dropdown.selected() == 1;
            handle.update(|state| {
                let Some(column) = state
                    .tables
                    .get(&tab)
                    .and_then(TableState::effective_order)
                    .map(|o| o.column)
                else {
                    return;
                };
                set_order(state, tab, column, Some(descending));
            });
        });
        let handle = app.clone();
        self.reset_columns.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.collapsed_columns.clear();
                    table.version += 1;
                }
            });
        });
        let weak = Rc::downgrade(self);
        self.reset_columns.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.reset_widths();
            }
        });
        let handle = app.clone();
        self.save
            .connect_clicked(move |_| handle.update(|state| state.save_changes(tab)));
        let handle = app.clone();
        self.discard
            .connect_clicked(move |_| handle.update(|state| state.discard_changes(tab)));
        let handle = app.clone();
        self.previous.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.page_index = table.page_index.saturating_sub(1);
                }
                state.reload_table(tab);
            });
        });
        let handle = app.clone();
        self.next.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.page_index += 1;
                }
                state.reload_table(tab);
            });
        });
        // Selection follows the grid.
        let handle = app.clone();
        self.selection
            .connect_selection_changed(move |model, _, _| {
                let selected = selected_rows(model);
                handle.update(|state| {
                    if let Some(table) = state.tables.get_mut(&tab)
                        && table.selected != selected
                    {
                        table.selected = selected;
                        table.version += 1;
                    }
                });
            });
        // Header clicks sort on the server.
        let handle = app.clone();
        let weak = Rc::downgrade(self);
        if let Some(sorter) = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>() {
            sorter.connect_changed(move |sorter, _| {
                let Some(view) = weak.upgrade() else { return };
                let Some(column) = sorter.primary_sort_column() else {
                    return;
                };
                let Some(index) = view.columns.borrow().iter().position(|c| *c == column) else {
                    return;
                };
                let descending = sorter.primary_sort_order() == gtk::SortType::Descending;
                handle.update(|state| {
                    let Some(name) = state
                        .tables
                        .get(&tab)
                        .and_then(|t| t.page.as_ref())
                        .and_then(|p| p.metadata.columns.get(index))
                        .map(|c| c.name.clone())
                    else {
                        return;
                    };
                    set_order(state, tab, name, Some(descending));
                });
            });
        }
        // Delete stages deletion of the selection; Escape clears it.
        let keys = gtk::EventControllerKey::new();
        let handle = app.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let mut handled = false;
            handle.update(|state| {
                let read_only = state.tab(tab).is_none_or(|t| state.read_only(t.profile_id));
                let saving = state.busy(RequestKind::Mutation, Some(tab));
                let Some(table) = state.tables.get_mut(&tab) else {
                    return;
                };
                if table.editing.is_some() || table.loading || saving {
                    return;
                }
                if key == gtk::gdk::Key::Delete || key == gtk::gdk::Key::KP_Delete {
                    let can = table
                        .page
                        .as_ref()
                        .is_some_and(|page| editable(read_only, page));
                    if can && !table.selected.is_empty() {
                        let rows: Vec<usize> = table.selected.iter().copied().collect();
                        toggle_delete(table, &rows);
                        handled = true;
                    }
                } else if key == gtk::gdk::Key::Escape && !table.selected.is_empty() {
                    table.selected.clear();
                    table.version += 1;
                    handled = true;
                }
            });
            if handled {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.view.add_controller(keys);
        // The preview card stays while the pointer is on it.
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_enter(move |_, _, _| {
            if let Some(view) = weak.upgrade() {
                view.preview_hover.set(true);
                view.cancel_preview_close();
            }
        });
        let weak = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(view) = weak.upgrade() {
                view.preview_hover.set(false);
                view.schedule_preview_close();
            }
        });
        self.preview.add_controller(motion);
    }

    /// "Copy selected as CSV" and staging deletion, for the N selected menu
    /// and the row context menu.
    fn selection_items(&self, app: &App, with_clear: bool) -> Vec<ui::MenuItem> {
        let tab = self.tab_id;
        let state = app.state.borrow();
        let Some(table) = state.tables.get(&tab) else {
            return Vec::new();
        };
        let selected: Vec<usize> = table.selected.iter().copied().collect();
        let mut items: Vec<ui::MenuItem> = Vec::new();
        let handle = app.clone();
        let rows = selected.clone();
        items.push((
            "Copy selected as CSV".into(),
            false,
            Box::new(move || {
                let rows = rows.clone();
                handle.update(move |state| state.copy_table_rows(tab, rows, "selected"));
            }),
        ));
        let read_only = state.tab(tab).is_none_or(|t| state.read_only(t.profile_id));
        let saving = state.busy(RequestKind::Mutation, Some(tab));
        if table.page.as_ref().is_some_and(|p| editable(read_only, p)) && !table.loading && !saving
        {
            let all_deleted = selected
                .iter()
                .all(|r| table.pending.get(r).is_some_and(|p| p.deleted));
            let label = if all_deleted {
                "Undo staged deletion".to_owned()
            } else if selected.len() == 1 {
                "Stage row for deletion".to_owned()
            } else {
                format!("Stage {} rows for deletion", selected.len())
            };
            let handle = app.clone();
            let rows = selected.clone();
            items.push((
                label,
                !all_deleted,
                Box::new(move || {
                    let rows = rows.clone();
                    handle.update(move |state| {
                        if let Some(table) = state.tables.get_mut(&tab) {
                            toggle_delete(table, &rows);
                        }
                    });
                }),
            ));
        }
        if with_clear {
            let handle = app.clone();
            items.push((
                "Clear selection".into(),
                false,
                Box::new(move || {
                    handle.update(|state| {
                        if let Some(table) = state.tables.get_mut(&tab) {
                            table.selected.clear();
                            table.version += 1;
                        }
                    });
                }),
            ));
        }
        items
    }

    fn reset_widths(&self) {
        let state = self.app.state.borrow();
        let Some(page) = state.tables.get(&self.tab_id).and_then(|t| t.page.clone()) else {
            return;
        };
        drop(state);
        for (column, meta) in self.columns.borrow().iter().zip(&page.metadata.columns) {
            column.set_fixed_width(default_column_width(&meta.data_type));
        }
        self.reset_columns.set_visible(false);
    }

    // ---- rendering ------------------------------------------------------

    pub fn refresh(self: &Rc<Self>, app: &App, state: &State) {
        let tab = self.tab_id;
        let Some(table) = state.tables.get(&tab) else {
            return;
        };
        let Some((profile, schema, name)) = state.table_target(tab) else {
            return;
        };
        let read_only = state.read_only(profile);
        let saving = state.busy(RequestKind::Mutation, Some(tab));
        let exporting = state.exporting(tab);
        self.refresh_toolbar(state, table, &schema, &name, exporting);
        self.message
            .render(app, table.message.as_ref(), move |app| {
                app.update(|state| {
                    if let Some(table) = state.tables.get_mut(&tab) {
                        table.message = None;
                    }
                });
            });
        let Some(page) = table.page.clone() else {
            self.filters.set_visible(false);
            self.inspector.set_visible(false);
            self.pending_bar.set_visible(false);
            self.status.set_visible(false);
            self.show(if table.loading { "skeleton" } else { "retry" });
            return;
        };
        self.filters.set_visible(true);
        self.status.set_visible(true);
        self.refresh_filters(table, &page);
        self.refresh_grid(app, table, &page, read_only);
        let interactive = !table.loading && !saving;
        self.view.set_sensitive(interactive);
        let overlay = table.loading
            && table
                .loading_since
                .is_some_and(|since| state.now - since >= 0.12);
        self.refreshing.set_visible(overlay);
        self.inspector.set_visible(table.inspector_open);
        if table.inspector_open {
            self.refresh_inspector(table, &page, read_only, interactive);
        }
        self.refresh_pending(table, read_only, saving);
        self.refresh_status(table, &page);
        self.refresh_preview(table, &page);
    }

    fn show(&self, name: &str) {
        if self.body.visible_child_name().as_deref() != Some(name) {
            self.body.set_visible_child_name(name);
        }
    }

    fn refresh_toolbar(
        &self,
        state: &State,
        table: &TableState,
        schema: &str,
        name: &str,
        exporting: bool,
    ) {
        let markup = if self.embedded {
            format!(
                "<span size='13pt' weight='bold' foreground='#f5f5f7'>{}</span>",
                glib::markup_escape_text(&format!("{schema}.{name}"))
            )
        } else {
            format!(
                "<span size='15pt' foreground='#a0a0a6'>{}</span><span size='15pt' weight='bold' foreground='#f5f5f7'>.{}</span>",
                glib::markup_escape_text(schema),
                glib::markup_escape_text(name)
            )
        };
        let markup = markup.replace("13pt", "9.75pt").replace("15pt", "11.25pt");
        if self.title.label() != markup {
            self.title.set_markup(&markup);
        }
        self.title
            .set_tooltip_text(Some(&format!("{schema}.{name}")));
        let counts = table.page.as_ref().map_or_else(String::new, |page| {
            let rows = page.total_rows.map_or_else(|| "—".to_owned(), count);
            format!("{rows} rows · {} columns", page.metadata.columns.len())
        });
        set_text(&self.counts, &counts);
        let has_page = table.page.is_some() && !table.loading;
        ui::set_button_text(
            &self.refresh,
            if table.loading {
                "Refreshing…"
            } else {
                "Refresh"
            },
        );
        self.refresh.set_sensitive(!table.loading);
        ui::set_button_text(
            &self.copy,
            &format!(
                "Copy visible ({})",
                count(table.copyable_rows().len() as u64)
            ),
        );
        self.copy.set_sensitive(has_page);
        let export = if exporting {
            let done = table
                .export_progress
                .as_ref()
                .map_or(0, |rows| rows.load(std::sync::atomic::Ordering::Relaxed));
            match table.total_rows() {
                Some(total) => format!("Exporting {} / {}…", count(done), count(total)),
                None => format!("Exporting {}…", count(done)),
            }
        } else {
            let total = table
                .total_rows()
                .map_or_else(String::new, |n| format!(" ({})", count(n)));
            format!("Export all{total}")
        };
        ui::set_button_text(&self.export, &export);
        self.export.set_sensitive(has_page && !exporting);
        self.selected_menu.set_visible(!table.selected.is_empty());
        set_text(
            &self.selected_label,
            &format!("{} selected", count(table.selected.len() as u64)),
        );
        if table.inspector_open {
            self.inspector_toggle.add_css_class("on");
        } else {
            self.inspector_toggle.remove_css_class("on");
        }
        self.inspector_toggle
            .set_tooltip_text(Some(if table.inspector_open {
                "Hide row inspector"
            } else {
                "Show row inspector"
            }));
        let _ = state;
    }

    fn refresh_filters(self: &Rc<Self>, table: &TableState, page: &TablePage) {
        let columns: Vec<String> = page
            .metadata
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        // Preview limit, sort, direction.
        if self.limit.value_as_int() != table.limit as i32 {
            self.limit.set_value(f64::from(table.limit));
        }
        if *self.sort_names.borrow() != columns {
            let labels: Vec<String> = columns
                .iter()
                .map(|name| {
                    if page.metadata.primary_key.contains(name) {
                        format!("{name} (primary key)")
                    } else {
                        name.clone()
                    }
                })
                .collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            self.sort.set_model(Some(&gtk::StringList::new(&labels)));
            *self.sort_names.borrow_mut() = columns.clone();
        }
        let order = table.effective_order();
        let selected = order
            .as_ref()
            .and_then(|o| columns.iter().position(|c| *c == o.column))
            .map_or(gtk::INVALID_LIST_POSITION, |i| i as u32);
        if self.sort.selected() != selected {
            self.sort.set_selected(selected);
        }
        self.direction_group.set_visible(order.is_some());
        let direction = u32::from(order.as_ref().is_some_and(|o| o.descending));
        if self.direction.selected() != direction {
            self.direction.set_selected(direction);
        }
        self.clear.set_visible(table.has_filters());
        let resized = self
            .columns
            .borrow()
            .iter()
            .zip(&page.metadata.columns)
            .enumerate()
            .any(|(i, (column, meta))| {
                !table.collapsed_columns.contains(&i)
                    && column.fixed_width() != default_column_width(&meta.data_type)
            });
        self.reset_columns
            .set_visible(resized || !table.collapsed_columns.is_empty());
        // Filter rows.
        let signature = format!(
            "{columns:?}|{}",
            table
                .filters
                .iter()
                .map(|f| format!("{}:{:?}", f.column, f.operator))
                .collect::<Vec<_>>()
                .join(",")
        );
        if *self.filter_signature.borrow() == signature {
            return;
        }
        *self.filter_signature.borrow_mut() = signature;
        ui::clear(&self.filter_rows);
        let column_list: Vec<&str> = columns.iter().map(String::as_str).collect();
        let operator_list: Vec<&str> = FILTER_OPERATORS.iter().map(|(_, l)| *l).collect();
        for (index, filter) in table.filters.iter().enumerate() {
            let row = ui::hbox(8);
            let column = gtk::DropDown::from_strings(&column_list);
            column.set_size_request(170, -1);
            if let Some(i) = columns.iter().position(|c| *c == filter.column) {
                column.set_selected(i as u32);
            }
            let operator = gtk::DropDown::from_strings(&operator_list);
            operator.set_size_request(170, -1);
            if let Some(i) = FILTER_OPERATORS
                .iter()
                .position(|(o, _)| *o == filter.operator)
            {
                operator.set_selected(i as u32);
            }
            row.append(&column);
            row.append(&operator);
            let tab = self.tab_id;
            if filter_needs_value(filter.operator) {
                let value = gtk::Entry::new();
                value.add_css_class("mono");
                value.set_hexpand(true);
                value.set_text(&filter.value);
                value.set_placeholder_text(Some(
                    if matches!(filter.operator, FilterOperator::In | FilterOperator::NotIn) {
                        "value 1, value 2, …"
                    } else {
                        "Value…"
                    },
                ));
                let handle = self.app.clone();
                value.connect_changed(move |entry| {
                    let text = entry.text().to_string();
                    handle.quiet(|state| {
                        if let Some(f) = state
                            .tables
                            .get_mut(&tab)
                            .and_then(|t| t.filters.get_mut(index))
                        {
                            f.value = text;
                        }
                    });
                });
                let handle = self.app.clone();
                value.connect_activate(move |_| handle.update(|state| apply_filters(state, tab)));
                row.append(&value);
            } else {
                let none = ui::label("No value needed", &["faint"]);
                none.set_hexpand(true);
                row.append(&none);
            }
            let handle = self.app.clone();
            let names = columns.clone();
            column.connect_selected_notify(move |dropdown| {
                let Some(name) = names.get(dropdown.selected() as usize).cloned() else {
                    return;
                };
                handle.update(|state| {
                    if let Some(f) = state
                        .tables
                        .get_mut(&tab)
                        .and_then(|t| t.filters.get_mut(index))
                    {
                        f.column = name;
                    }
                });
            });
            let handle = self.app.clone();
            operator.connect_selected_notify(move |dropdown| {
                let Some((op, _)) = FILTER_OPERATORS.get(dropdown.selected() as usize) else {
                    return;
                };
                let op = *op;
                handle.update(|state| {
                    if let Some(f) = state
                        .tables
                        .get_mut(&tab)
                        .and_then(|t| t.filters.get_mut(index))
                    {
                        f.operator = op;
                    }
                });
            });
            let remove = ui::button("toolbar-button", Some(Icon::Close), "", "Remove filter");
            let handle = self.app.clone();
            remove.connect_clicked(move |_| {
                handle.update(|state| {
                    if let Some(t) = state.tables.get_mut(&tab)
                        && index < t.filters.len()
                    {
                        t.filters.remove(index);
                    }
                });
            });
            row.append(&remove);
            self.filter_rows.append(&row);
        }
    }

    fn refresh_grid(
        self: &Rc<Self>,
        app: &App,
        table: &TableState,
        page: &std::sync::Arc<TablePage>,
        read_only: bool,
    ) {
        let column_key = page
            .metadata
            .columns
            .iter()
            .map(|c| format!("{}:{}", c.name, c.data_type))
            .collect::<Vec<_>>()
            .join(",")
            + &format!("|{:?}", page.metadata.primary_key);
        if *self.column_key.borrow() != column_key {
            *self.column_key.borrow_mut() = column_key;
            self.build_columns(app, page);
        }
        let page_key = std::sync::Arc::as_ptr(page) as usize;
        if self.page_key.get() != page_key {
            self.page_key.set(page_key);
            fill(&self.store, page.rows.len());
        }
        if page.rows.is_empty() {
            self.show("empty");
            return;
        }
        self.show("grid");
        // Headers: sort indicator, key glyph, collapsed columns.
        let order = table.effective_order();
        let mut headers = Vec::new();
        for (index, (column, meta)) in self
            .columns
            .borrow()
            .iter()
            .zip(&page.metadata.columns)
            .enumerate()
        {
            let collapsed = table.collapsed_columns.contains(&index);
            let key = page.metadata.primary_key.contains(&meta.name);
            if column.title().as_deref() != Some(meta.name.as_str()) {
                column.set_title(Some(&meta.name));
            }
            headers.push((
                meta.name.clone(),
                meta.data_type.clone(),
                key && !collapsed,
                collapsed,
            ));
            if collapsed && column.fixed_width() != COLLAPSED_COLUMN_WIDTH {
                column.set_fixed_width(COLLAPSED_COLUMN_WIDTH);
            } else if !collapsed && column.fixed_width() == COLLAPSED_COLUMN_WIDTH {
                column.set_fixed_width(default_column_width(&meta.data_type));
            }
            column.set_resizable(!collapsed);
            if order.as_ref().is_some_and(|o| o.column == meta.name) {
                let sort = if order.as_ref().is_some_and(|o| o.descending) {
                    gtk::SortType::Descending
                } else {
                    gtk::SortType::Ascending
                };
                let sorter = self.view.sorter().and_downcast::<gtk::ColumnViewSorter>();
                let current = sorter
                    .as_ref()
                    .and_then(|s| s.primary_sort_column())
                    .is_some_and(|c| c == *column)
                    && sorter.as_ref().map(|s| s.primary_sort_order()) == Some(sort);
                if !current {
                    self.view.sort_by_column(Some(column), sort);
                }
            }
        }
        decorate_headers(&self.view, &headers);
        // Selection.
        if selected_rows(&self.selection) != table.selected {
            let chosen = gtk::Bitset::new_empty();
            for row in &table.selected {
                chosen.add(*row as u32);
            }
            let mask = gtk::Bitset::new_range(0, self.store.n_items());
            self.selection.set_selection(&chosen, &mask);
        }
        let style_key = format!(
            "{}|{}|{:?}|{:?}|{read_only}",
            page_key, table.version, table.editing, table.collapsed_columns
        );
        if *self.style_key.borrow() != style_key {
            *self.style_key.borrow_mut() = style_key;
            for cell in self.cells.borrow().values() {
                style_cell(cell, table, page);
            }
        }
    }

    fn build_columns(self: &Rc<Self>, app: &App, page: &TablePage) {
        for column in self.columns.borrow_mut().drain(..) {
            self.view.remove_column(&column);
        }
        let mut columns = Vec::new();
        for (index, meta) in page.metadata.columns.iter().enumerate() {
            let factory = gtk::SignalListItemFactory::new();
            let weak = Rc::downgrade(self);
            let handle = app.clone();
            factory.connect_setup(move |_, item| {
                let Some(view) = weak.upgrade() else { return };
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let cell = Rc::new(view.new_cell(&handle));
                item.set_child(Some(&cell.root));
                view.cells
                    .borrow_mut()
                    .insert(cell.root.clone().upcast(), cell);
            });
            let cells = self.cells.clone();
            factory.connect_teardown(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(root) = item.child() {
                    cells.borrow_mut().remove(&root);
                }
            });
            let cells = self.cells.clone();
            let handle = app.clone();
            let tab = self.tab_id;
            factory.connect_bind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let (Some(row), Some(root)) = (row_of(item), item.child()) else {
                    return;
                };
                let Some(cell) = cells.borrow().get(&root).cloned() else {
                    return;
                };
                cell.pos.set(Some((row, index)));
                match handle.state.try_borrow() {
                    Ok(state) => {
                        if let Some(table) = state.tables.get(&tab)
                            && let Some(page) = table.page.clone()
                        {
                            style_cell(&cell, table, &page);
                        }
                    }
                    Err(_) => handle.queue_render(),
                }
            });
            let cells = self.cells.clone();
            factory.connect_unbind(move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(cell) = item
                    .child()
                    .and_then(|root| cells.borrow().get(&root).cloned())
                {
                    cell.pos.set(None);
                }
            });
            let column = gtk::ColumnViewColumn::new(Some(&meta.name), Some(factory));
            column.set_resizable(true);
            column.set_fixed_width(default_column_width(&meta.data_type));
            // Sorting happens on the server; the sorter only drives headers.
            column.set_sorter(Some(&gtk::CustomSorter::new(|_, _| gtk::Ordering::Equal)));
            let menu = gtk::gio::Menu::new();
            menu.append(
                Some("Collapse or expand column"),
                Some(&format!("grid.toggle-column({index})")),
            );
            column.set_header_menu(Some(&menu));
            let weak = Rc::downgrade(self);
            column.connect_fixed_width_notify(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.app.queue_render();
                }
            });
            self.view.append_column(&column);
            columns.push(column);
        }
        *self.columns.borrow_mut() = columns;
        let actions = gtk::gio::SimpleActionGroup::new();
        let toggle = gtk::gio::SimpleAction::new("toggle-column", Some(glib::VariantTy::INT32));
        let handle = app.clone();
        let tab = self.tab_id;
        toggle.connect_activate(move |_, parameter| {
            let Some(index) = parameter.and_then(|p| p.get::<i32>()) else {
                return;
            };
            let index = index as usize;
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    if !table.collapsed_columns.remove(&index) {
                        table.collapsed_columns.insert(index);
                    }
                    table.version += 1;
                }
            });
        });
        actions.add_action(&toggle);
        self.view.insert_action_group("grid", Some(&actions));
    }

    fn new_cell(self: &Rc<Self>, app: &App) -> TableCell {
        let root = ui::hbox(0);
        root.add_css_class("cell");
        let label = cell_label();
        root.append(&label);
        let tab = self.tab_id;
        let pos: Rc<Cell<Option<(usize, usize)>>> = Rc::default();
        // Copy the full value; not while editing or on a collapsed column.
        let handle = app.clone();
        let at = pos.clone();
        let weak = root.downgrade();
        let copy = HoverCopy::attach(
            &root,
            move || {
                weak.upgrade().is_some_and(|root| {
                    !root.has_css_class("editing") && !root.has_css_class("collapsed")
                })
            },
            move || {
                let (row, column) = at.get()?;
                let state = handle.state.borrow();
                state
                    .tables
                    .get(&tab)
                    .and_then(|t| t.values(row))
                    .and_then(|v| v.get(column))
                    .map(display_value)
            },
        );
        let handle = app.clone();
        let at = pos.clone();
        let cell = TableCell {
            root,
            label,
            entry: RefCell::new(None),
            new_entry: Box::new(move || new_editor(&handle, tab, at.clone())),
            copy,
            pos,
            editing: Cell::new(false),
        };
        // Double-click edits; right-click opens the row menu.
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let handle = app.clone();
        let pos = cell_pos(&cell);
        let weak = Rc::downgrade(self);
        let root = cell.root.downgrade();
        click.connect_pressed(move |gesture, presses, _, _| {
            let Some((row, column)) = pos() else { return };
            match gesture.current_button() {
                1 if presses == 2 => handle.update(|state| begin_edit(state, tab, row, column)),
                3 => {
                    if let (Some(view), Some(root)) = (weak.upgrade(), root.upgrade()) {
                        view.context_menu(&handle, row, &root);
                    }
                }
                _ => {}
            }
        });
        cell.root.add_controller(click);
        // Hover on a staged row opens its preview card.
        let motion = gtk::EventControllerMotion::new();
        let pos = cell_pos(&cell);
        let weak = Rc::downgrade(self);
        let root = cell.root.downgrade();
        motion.connect_enter(move |_, _, _| {
            if let (Some(view), Some((row, _)), Some(root)) =
                (weak.upgrade(), pos(), root.upgrade())
            {
                view.hover_row(row, &root);
            }
        });
        let weak = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(view) = weak.upgrade() {
                view.schedule_preview_close();
            }
        });
        cell.root.add_controller(motion);
        cell
    }

    fn context_menu(&self, app: &App, row: usize, cell: &gtk::Box) {
        let tab = self.tab_id;
        app.update(|state| {
            if let Some(table) = state.tables.get_mut(&tab)
                && !table.selected.contains(&row)
            {
                table.selected = BTreeSet::from([row]);
                table.version += 1;
            }
        });
        let popover = ui::menu(self.selection_items(app, false), 200);
        popover.set_parent(&self.view);
        if let Some(bounds) = cell.compute_bounds(&self.view) {
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                bounds.x() as i32,
                bounds.y() as i32 + bounds.height() as i32 / 2,
                1,
                1,
            )));
        }
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.popup();
    }

    // ---- inspector ------------------------------------------------------

    fn refresh_inspector(
        &self,
        table: &TableState,
        page: &std::sync::Arc<TablePage>,
        read_only: bool,
        interactive: bool,
    ) {
        let single = (table.selected.len() == 1)
            .then(|| table.selected.first().copied())
            .flatten()
            .filter(|&i| i < page.rows.len());
        let deleted = single.is_some_and(|i| table.pending.get(&i).is_some_and(|p| p.deleted));
        let edited = single.is_some_and(|i| table.pending.contains_key(&i));
        let key = format!(
            "{}|{single:?}|{deleted}|{edited}|{read_only}|{}|{interactive}",
            std::sync::Arc::as_ptr(page) as usize,
            table.selected.len()
        );
        if *self.inspector_key.borrow() == key {
            self.update_inspector_values(table, page, single);
            return;
        }
        *self.inspector_key.borrow_mut() = key;
        ui::clear(&self.inspector_body);
        self.inspector_entries.borrow_mut().clear();
        let tab = self.tab_id;
        let header = ui::hbox(8);
        header.add_css_class("inspector-header");
        header.append(&ui::label("Row", &["bold", "strong"]));
        if let Some(index) = single {
            let summary = page
                .metadata
                .primary_key
                .iter()
                .filter_map(|key| {
                    let i = page.metadata.columns.iter().position(|c| &c.name == key)?;
                    Some(format!("{key} = {}", display_value(&page.rows[index][i])))
                })
                .collect::<Vec<_>>()
                .join(", ");
            if !summary.is_empty() {
                header.append(&ui::ellipsized(&summary, &["size-12", "muted"]));
            }
        }
        if deleted {
            header.append(&ui::chip("Staged for deletion", "danger"));
        } else if edited {
            header.append(&ui::chip("Edited", "modified"));
        }
        header.append(&ui::spacer());
        let close = ui::button(
            "toolbar-button",
            Some(Icon::Close),
            "",
            "Hide row inspector",
        );
        let handle = self.app.clone();
        close.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.inspector_open = false;
                }
            });
        });
        header.append(&close);
        self.inspector_body.append(&header);
        let Some(index) = single else {
            let message = if table.selected.len() > 1 {
                format!(
                    "{} rows selected. Select a single row to inspect it.",
                    table.selected.len()
                )
            } else {
                "Select a row to inspect and edit its fields.".to_owned()
            };
            let empty = ui::label(&message, &["size-12-5", "faint"]);
            empty.set_wrap(true);
            empty.set_max_width_chars(30);
            empty.set_xalign(0.5);
            empty.set_justify(gtk::Justification::Center);
            empty.set_margin_top(28);
            empty.set_margin_start(20);
            empty.set_margin_end(20);
            self.inspector_body.append(&empty);
            return;
        };
        let fields = ui::vbox(12);
        fields.set_margin_start(14);
        fields.set_margin_end(14);
        fields.set_margin_top(14);
        fields.set_margin_bottom(14);
        let values = table.values(index).unwrap_or(&[]);
        for (column_index, column) in page.metadata.columns.iter().enumerate() {
            let field = ui::vbox(5);
            field.add_css_class("inspector-field");
            let is_key = page.metadata.primary_key.contains(&column.name);
            let original = &page.rows[index][column_index];
            let current = values.get(column_index).unwrap_or(&Value::Null);
            let changed = !deleted && current != original;
            let line = ui::hbox(6);
            line.append(&ui::label(&column.name, &["name"]));
            line.append(&ui::label(&column.data_type, &["type"]));
            line.append(&ui::spacer());
            if is_key {
                line.append(&ui::label("Primary key", &["size-11", "faint"]));
            } else if changed {
                let note = ui::ellipsized(
                    &format!("was {}", display_prefix(original, 200)),
                    &["size-11", "modified-text"],
                );
                note.set_max_width_chars(18);
                line.append(&note);
            }
            field.append(&line);
            if let Some(size) = large_value_size(current) {
                let preview = ui::ellipsized(
                    &format!("{}…", display_prefix(current, 160)),
                    &["size-12", "muted"],
                );
                field.append(&preview);
                let note = ui::label(
                    &format!("{size} value — too large to edit here; update it with a query."),
                    &["size-11", "faint"],
                );
                note.set_wrap(true);
                field.append(&note);
                fields.append(&field);
                continue;
            }
            let read_only_field = !editable_column(read_only, page, column_index)
                || deleted
                || is_key
                || !interactive;
            let entry = gtk::Entry::new();
            entry.set_text(&editable_text(current));
            if current.is_null() {
                entry.set_placeholder_text(Some("NULL"));
            }
            entry.set_editable(!read_only_field);
            entry.set_can_focus(!read_only_field);
            if read_only_field {
                entry.add_css_class("readonly");
            } else if changed {
                entry.add_css_class("changed");
            }
            entry.update_property(&[gtk::accessible::Property::Label(&column.name)]);
            if !read_only_field {
                let handle = self.app.clone();
                entry.connect_activate(move |entry| {
                    let text = entry.text().to_string();
                    handle.update(|state| stage_field(state, tab, index, column_index, &text));
                });
                let focus = gtk::EventControllerFocus::new();
                let handle = self.app.clone();
                let weak = entry.downgrade();
                focus.connect_leave(move |_| {
                    if let Some(entry) = weak.upgrade() {
                        let text = entry.text().to_string();
                        handle.update(|state| stage_field(state, tab, index, column_index, &text));
                    }
                });
                entry.add_controller(focus);
                let keys = gtk::EventControllerKey::new();
                let handle = self.app.clone();
                let weak = entry.downgrade();
                keys.connect_key_pressed(move |_, key, _, _| {
                    if key != gtk::gdk::Key::Escape {
                        return glib::Propagation::Proceed;
                    }
                    if let Some(entry) = weak.upgrade() {
                        let state = handle.state.borrow();
                        let text = state
                            .tables
                            .get(&tab)
                            .and_then(|t| t.values(index))
                            .and_then(|v| v.get(column_index))
                            .map(editable_text)
                            .unwrap_or_default();
                        entry.set_text(&text);
                    }
                    glib::Propagation::Stop
                });
                entry.add_controller(keys);
            }
            self.inspector_entries
                .borrow_mut()
                .push((column_index, entry.clone()));
            field.append(&entry);
            fields.append(&field);
        }
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&fields));
        self.inspector_body.append(&scroller);
        if editable(read_only, page) {
            let footer = ui::vbox(0);
            footer.add_css_class("inspector-footer");
            let (icon, label) = if deleted {
                (Icon::Undo, "Restore row")
            } else {
                (Icon::Trash, "Delete row")
            };
            let button = ui::button("secondary-button", Some(icon), label, "");
            if !deleted {
                button.add_css_class("danger-text");
            }
            button.set_sensitive(interactive);
            let handle = self.app.clone();
            button.connect_clicked(move |_| {
                handle.update(|state| {
                    if let Some(table) = state.tables.get_mut(&tab) {
                        toggle_delete(table, &[index]);
                    }
                });
            });
            footer.append(&button);
            self.inspector_body.append(&footer);
        }
    }

    /// Keeps the same fields and updates their values and states in place,
    /// so moving between fields keeps focus.
    fn update_inspector_values(&self, table: &TableState, page: &TablePage, single: Option<usize>) {
        let Some(index) = single else { return };
        let Some(values) = table.values(index) else {
            return;
        };
        for (column, entry) in self.inspector_entries.borrow().iter() {
            let current = values.get(*column).unwrap_or(&Value::Null);
            let changed = current != &page.rows[index][*column];
            if !entry.has_focus() {
                let text = editable_text(current);
                if entry.text() != text {
                    entry.set_text(&text);
                }
            }
            if !entry.has_css_class("readonly") {
                if changed {
                    entry.add_css_class("changed");
                } else {
                    entry.remove_css_class("changed");
                }
            }
        }
    }

    fn refresh_pending(&self, table: &TableState, read_only: bool, saving: bool) {
        let count = table.pending.len();
        self.pending_bar.set_visible(count > 0);
        if count == 0 {
            return;
        }
        let deleted = table.pending.values().filter(|p| p.deleted).count();
        let edited = count - deleted;
        set_text(
            &self.pending_count,
            &plural(count, "pending change", "pending changes"),
        );
        ui::clear(&self.pending_chips);
        if edited > 0 {
            self.pending_chips.append(&ui::chip(
                &plural(edited, "edited row", "edited rows"),
                "modified",
            ));
        }
        if deleted > 0 {
            self.pending_chips.append(&ui::chip(
                &plural(deleted, "deletion", "deletions"),
                "danger",
            ));
        }
        if read_only {
            self.pending_chips
                .append(&ui::chip("Read-only connection", "muted"));
        }
        ui::set_button_text(
            &self.save,
            &if saving {
                "Saving…".to_owned()
            } else {
                format!("Save changes ({count})")
            },
        );
        self.save.set_sensitive(!read_only && !saving);
        self.discard.set_sensitive(!saving);
    }

    fn refresh_status(&self, table: &TableState, page: &TablePage) {
        let total = page
            .total_rows
            .map_or_else(String::new, |n| format!(" of {}", count(n)));
        let rows = if page.rows.is_empty() {
            format!("No rows{total}")
        } else {
            format!(
                "Rows {}–{}{total}",
                count(page.offset + 1),
                count(page.offset + page.rows.len() as u32)
            )
        };
        set_text(&self.rows_label, &rows);
        set_text(&self.per_page, &format!("{} per page", page.limit));
        let paged = page.offset > 0 || page.has_more;
        self.previous.set_visible(paged);
        self.next.set_visible(paged);
        self.page_label.set_visible(paged);
        let clean = table.pending.is_empty();
        self.previous
            .set_sensitive(page.offset > 0 && clean && !table.loading);
        self.next
            .set_sensitive(page.has_more && clean && !table.loading);
        let current = page.offset / page.limit.max(1) + 1;
        let pages = page
            .total_rows
            .map(|n| n.div_ceil(u64::from(page.limit.max(1))).max(1));
        set_text(
            &self.page_label,
            &match pages {
                Some(pages) => format!("Page {} of {}", count(current), count(pages)),
                None => format!("Page {}", count(current)),
            },
        );
    }

    // ---- staged-row preview card ---------------------------------------

    fn hover_row(&self, row: usize, cell: &gtk::Box) {
        let staged = self
            .app
            .state
            .try_borrow()
            .ok()
            .and_then(|s| {
                s.tables
                    .get(&self.tab_id)
                    .map(|t| t.pending.contains_key(&row))
            })
            .unwrap_or(false);
        if !staged {
            // Crossing other rows on the way to the card doesn't retarget it.
            if self.preview_row.get().is_some() {
                self.schedule_preview_close();
            }
            return;
        }
        self.cancel_preview_close();
        if self.preview_row.get() != Some(row) {
            self.preview_row.set(Some(row));
            self.preview_key.borrow_mut().clear();
        }
        if let Some(bounds) = cell.compute_bounds(&self.view) {
            self.preview.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                16,
                (bounds.y() + bounds.height()) as i32,
                1,
                1,
            )));
        }
        self.app.queue_render();
    }

    fn cancel_preview_close(&self) {
        if let Some(source) = self.preview_close.borrow_mut().take() {
            source.remove();
        }
    }

    /// Closes the card 140 ms after the pointer leaves both it and the row.
    fn schedule_preview_close(&self) {
        if self.preview_row.get().is_none() || self.preview_close.borrow().is_some() {
            return;
        }
        let popover = self.preview.downgrade();
        let handle = self.app.clone();
        let tab = self.tab_id;
        let source =
            glib::timeout_add_local_once(std::time::Duration::from_millis(140), move || {
                let views = handle.views.borrow();
                let view = match views.get(&tab) {
                    Some(crate::app::View::Table(view)) => Some(view.clone()),
                    Some(crate::app::View::Query(query)) => query.table_view(),
                    None => None,
                };
                drop(views);
                if let Some(view) = view {
                    view.preview_close.borrow_mut().take();
                    if view.preview_hover.get() {
                        return;
                    }
                    view.preview_row.set(None);
                }
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
            });
        *self.preview_close.borrow_mut() = Some(source);
    }

    fn refresh_preview(&self, table: &TableState, page: &TablePage) {
        let Some(row) = self.preview_row.get() else {
            if self.preview.is_visible() {
                self.preview.popdown();
            }
            return;
        };
        let Some(pending) = table.pending.get(&row) else {
            self.preview_row.set(None);
            self.preview.popdown();
            return;
        };
        let key = format!("{row}|{:?}|{}", pending.changes, pending.deleted);
        if *self.preview_key.borrow() != key {
            *self.preview_key.borrow_mut() = key;
            let content = ui::vbox(10);
            content.set_size_request(380, -1);
            let header = ui::hbox(8);
            header.append(&ui::dot(
                if pending.deleted {
                    "#ff8a80"
                } else {
                    "#f0b14c"
                },
                true,
                7,
            ));
            let title = ui::label(
                if pending.deleted {
                    "Pending delete"
                } else {
                    "Pending edit"
                },
                &["bold", "size-12-5", "strong"],
            );
            title.set_hexpand(true);
            header.append(&title);
            let discard = ui::button(
                "secondary-button",
                None,
                if pending.deleted {
                    "Undo delete"
                } else {
                    "Discard edit"
                },
                "",
            );
            discard.add_css_class("small");
            let handle = self.app.clone();
            let tab = self.tab_id;
            let popover = self.preview.downgrade();
            discard.connect_clicked(move |_| {
                if let Some(popover) = popover.upgrade() {
                    popover.popdown();
                }
                handle.update(|state| {
                    if let Some(table) = state.tables.get_mut(&tab) {
                        table.discard_row(row);
                    }
                });
            });
            header.append(&discard);
            content.append(&header);
            if pending.deleted {
                content.append(&ui::label(
                    "This row will be deleted when changes are saved.",
                    &["size-12", "muted"],
                ));
            } else {
                for (index, column) in page.metadata.columns.iter().enumerate() {
                    if pending.changes[index] == pending.original[index] {
                        continue;
                    }
                    content.append(&change_diff(
                        &column.name,
                        &display_value(&pending.original[index]),
                        &display_value(&pending.changes[index]),
                    ));
                }
            }
            self.preview.set_child(Some(&content));
        }
        if !self.preview.is_visible() {
            self.preview.popup();
        }
    }
}

/// `.change-diff`: the column name, then Before → After with the changed
/// characters marked.
fn change_diff(name: &str, before: &str, after: &str) -> gtk::Box {
    let diff = inline_diff(before, after);
    let block = ui::vbox(4);
    let rule = ui::hbox(0);
    rule.add_css_class("diff-rule");
    rule.set_size_request(-1, 1);
    block.append(&rule);
    block.append(&ui::label(name, &["size-11-5", "secondary"]));
    let row = ui::hbox(8);
    let side = |caption: &str, changed: &str, class: &str, mark: &str| {
        let column = ui::vbox(4);
        column.set_hexpand(true);
        column.set_homogeneous(false);
        column.append(&ui::label(caption, &["size-11", "faint"]));
        let cut = |s: &str| s.chars().take(600).collect::<String>();
        let markup = format!(
            "{}<span background='{mark}'>{}</span>{}",
            glib::markup_escape_text(&cut(&diff.prefix)),
            glib::markup_escape_text(&cut(changed)),
            glib::markup_escape_text(&cut(&diff.suffix)),
        );
        let text = gtk::Label::new(None);
        text.set_markup(&markup);
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_wrap_mode(gtk::pango::WrapMode::Char);
        text.set_lines(12);
        text.set_max_width_chars(42);
        text.add_css_class("diff-box");
        text.add_css_class(class);
        column.append(&text);
        column
    };
    row.append(&side("Before", &diff.removed, "before", "#ff6b6138"));
    let arrow = icons::image(Icon::ArrowRight, 14);
    arrow.add_css_class("faint");
    row.append(&arrow);
    row.append(&side("After", &diff.added, "after", "#f0b14c52"));
    block.append(&row);
    block
}

/// The in-grid editor for the cell at `pos`: Enter or leaving stages,
/// Escape reverts.
fn new_editor(app: &App, tab: u64, pos: Rc<Cell<Option<(usize, usize)>>>) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.add_css_class("cell-editor");
    entry.set_hexpand(true);
    // Keep the editor from widening its fixed-width column.
    entry.set_width_chars(1);
    let handle = app.clone();
    let at = pos.clone();
    entry.connect_activate(move |entry| {
        let Some((row, column)) = at.get() else {
            return;
        };
        let text = entry.text().to_string();
        handle.update(|state| commit_edit(state, tab, row, column, &text));
    });
    let keys = gtk::EventControllerKey::new();
    let handle = app.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            handle.update(|state| {
                if let Some(table) = state.tables.get_mut(&tab) {
                    table.editing = None;
                }
            });
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    entry.add_controller(keys);
    let focus = gtk::EventControllerFocus::new();
    let handle = app.clone();
    let weak = entry.downgrade();
    focus.connect_leave(move |_| {
        let (Some((row, column)), Some(entry)) = (pos.get(), weak.upgrade()) else {
            return;
        };
        let text = entry.text().to_string();
        handle.update(|state| {
            if state.tables.get(&tab).and_then(|t| t.editing) == Some((row, column)) {
                commit_edit(state, tab, row, column, &text);
            }
        });
    });
    entry.add_controller(focus);
    entry
}

fn cell_pos(cell: &TableCell) -> impl Fn() -> Option<(usize, usize)> + 'static {
    let pos = cell.pos.clone();
    move || pos.get()
}

/// Paints one cell for its row and column: value, staged and selected
/// states, and the in-grid editor.
fn style_cell(cell: &TableCell, table: &TableState, page: &TablePage) {
    let Some((row, column)) = cell.pos.get() else {
        return;
    };
    let pending = table.pending.get(&row);
    let deleted = pending.is_some_and(|p| p.deleted);
    let modified = pending.is_some_and(|p| !p.deleted);
    let changed =
        !deleted && pending.is_some_and(|p| p.changes.get(column) != p.original.get(column));
    let selected = table.selected.contains(&row);
    let collapsed = table.collapsed_columns.contains(&column);
    let classes = [
        ("selected-row", selected),
        ("row-deleted", deleted),
        ("row-modified", modified),
        ("cell-changed", changed),
        ("edge-danger", column == 0 && deleted),
        ("edge-modified", column == 0 && modified),
        ("edge-accent", column == 0 && selected && pending.is_none()),
        ("collapsed", collapsed),
    ];
    for (class, on) in classes {
        if on {
            cell.root.add_css_class(class);
        } else {
            cell.root.remove_css_class(class);
        }
    }
    let value = table
        .values(row)
        .and_then(|v| v.get(column))
        .unwrap_or(&Value::Null);
    let editing = table.editing == Some((row, column));
    if editing {
        if !cell.editing.replace(true) {
            let entry = (cell.new_entry)();
            entry.set_text(&editable_text(value));
            cell.copy.hide(&cell.root);
            cell.label.set_visible(false);
            cell.root.prepend(&entry);
            *cell.entry.borrow_mut() = Some(entry.clone());
            let entry = entry.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(entry) = entry.upgrade() {
                    // Focusing a cell makes ColumnView scroll it into view
                    // even when it is already visible; keep the grid still.
                    let scroller = entry
                        .ancestor(gtk::ScrolledWindow::static_type())
                        .and_downcast::<gtk::ScrolledWindow>();
                    let before = scroller.as_ref().map(|s| s.hadjustment().value());
                    entry.grab_focus();
                    if let (Some(scroller), Some(before)) = (scroller, before) {
                        glib::idle_add_local_once(move || scroller.hadjustment().set_value(before));
                    }
                }
            });
        }
        cell.root.add_css_class("editing");
        return;
    }
    if cell.editing.replace(false)
        && let Some(entry) = cell.entry.borrow_mut().take()
    {
        cell.root.remove(&entry);
        cell.label.set_visible(true);
    }
    cell.root.remove_css_class("editing");
    if collapsed {
        cell.copy.hide(&cell.root);
    }
    let numeric = page
        .metadata
        .columns
        .get(column)
        .is_some_and(|c| numeric_column(&c.data_type));
    let text = display_prefix(value, 200).replace('\n', " ");
    if cell.label.text() != text {
        cell.label.set_text(&text);
    }
    cell.label.set_xalign(if numeric { 1.0 } else { 0.0 });
    if value.is_null() {
        cell.label.add_css_class("null");
    } else {
        cell.label.remove_css_class("null");
    }
    // Long values show in full on hover, unless the row is staged (its
    // preview card shows instead).
    let tooltip =
        (pending.is_none() && text.chars().count() > 24).then(|| display_prefix(value, 2000));
    cell.label.set_tooltip_text(tooltip.as_deref());
}

fn selected_rows(model: &gtk::MultiSelection) -> BTreeSet<usize> {
    let bitset = model.selection();
    (0..bitset.size())
        .map(|i| bitset.nth(i as u32) as usize)
        .collect()
}

fn apply_filters(state: &mut State, tab: u64) {
    if let Some(table) = state.tables.get_mut(&tab) {
        table.apply_filters();
    }
    state.reload_table(tab);
}

/// Sorts by `column`; a new column starts ascending.
fn set_order(state: &mut State, tab: u64, column: String, descending: Option<bool>) {
    let Some(table) = state.tables.get_mut(&tab) else {
        return;
    };
    let current = table.effective_order();
    let same_column = current.as_ref().is_some_and(|o| o.column == column);
    let descending = descending.filter(|_| same_column).unwrap_or(false);
    if same_column && current.as_ref().is_some_and(|o| o.descending == descending) {
        return;
    }
    if !table.pending.is_empty() {
        state.table_error(tab, crate::state::PENDING_REFRESH_ERROR);
        // Put the header and dropdown back on the shown order.
        if let Some(table) = state.tables.get_mut(&tab) {
            table.version += 1;
        }
        return;
    }
    table.order = Some(OrderSpec { column, descending });
    table.page_index = 0;
    state.load_table(tab);
}

fn begin_edit(state: &mut State, tab: u64, row: usize, column: usize) {
    let read_only = state.tab(tab).is_none_or(|t| state.read_only(t.profile_id));
    let saving = state.busy(RequestKind::Mutation, Some(tab));
    let now = state.now;
    let Some(table) = state.tables.get_mut(&tab) else {
        return;
    };
    let Some(page) = table.page.clone() else {
        return;
    };
    if table.loading || saving || !editable_column(read_only, &page, column) {
        return;
    }
    if table.pending.get(&row).is_some_and(|p| p.deleted) {
        return;
    }
    table.selected = BTreeSet::from([row]);
    let value = table
        .values(row)
        .and_then(|v| v.get(column))
        .cloned()
        .unwrap_or(Value::Null);
    if let Some(size) = large_value_size(&value) {
        table.message = Some(crate::state::Message::error(
            format!("This {size} value is too large to edit in the grid; update it with a query."),
            now,
        ));
    } else {
        table.editing = Some((row, column));
    }
    table.version += 1;
}

fn commit_edit(state: &mut State, tab: u64, row: usize, column: usize, text: &str) {
    let Some(table) = state.tables.get_mut(&tab) else {
        return;
    };
    table.editing = None;
    let current = table
        .values(row)
        .and_then(|v| v.get(column))
        .map(editable_text);
    if current.as_deref() != Some(text) {
        stage_cell(table, row, column, text);
    }
    table.version += 1;
}

fn stage_field(state: &mut State, tab: u64, row: usize, column: usize, text: &str) {
    let Some(table) = state.tables.get_mut(&tab) else {
        return;
    };
    let current = table
        .values(row)
        .and_then(|v| v.get(column))
        .map(editable_text);
    if current.as_deref() != Some(text) {
        stage_cell(table, row, column, text);
    }
}

/// Gives each column header egui's look: a key icon before primary-key
/// names and the data type in a muted, smaller run after the name.
/// ColumnView only takes a plain title, so this edits the title widgets it
/// builds; their order matches the columns'.
fn decorate_headers(view: &gtk::ColumnView, headers: &[(String, String, bool, bool)]) {
    let Some(header) = view.first_child() else {
        return;
    };
    let mut title = header.first_child();
    for (name, data_type, key, collapsed) in headers {
        let Some(widget) = title else {
            return;
        };
        title = widget.next_sibling();
        let Some(label) = find_label(&widget) else {
            continue;
        };
        let markup = if *collapsed || data_type.is_empty() {
            glib::markup_escape_text(name).to_string()
        } else {
            format!(
                "{}  <span size=\"small\" foreground=\"#5c5c62\">{}</span>",
                glib::markup_escape_text(name),
                glib::markup_escape_text(data_type)
            )
        };
        if label.label() != markup {
            label.set_markup(&markup);
        }
        let Some(parent) = label.parent() else {
            continue;
        };
        let marker = label
            .prev_sibling()
            .filter(|w| w.has_css_class("header-key"));
        match (marker, key) {
            (None, true) => {
                let icon = icons::image(Icon::Key, 11);
                icon.add_css_class("header-key");
                icon.set_tooltip_text(Some("Primary key"));
                icon.set_can_target(true);
                icon.set_margin_end(5);
                icon.insert_before(&parent, Some(&label));
            }
            (Some(icon), false) => icon.unparent(),
            _ => {}
        }
    }
}

fn find_label(widget: &gtk::Widget) -> Option<gtk::Label> {
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(label) = current.downcast_ref::<gtk::Label>() {
            return Some(label.clone());
        }
        if let Some(found) = find_label(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::state::tests::{app, page};

    #[test]
    fn sorting_a_new_column_starts_ascending_and_staged_edits_block_it() {
        let mut state = app();
        let id = uuid::Uuid::from_u128(1);
        state.open_table(id, "public".into(), "customers".into());
        let tab = state.active_tab.unwrap();
        state.tables.get_mut(&tab).unwrap().page = Some(Arc::new(page()));
        state.pending_requests.clear();
        state.queued.clear();
        set_order(&mut state, tab, "email".into(), Some(true));
        let order = state.tables[&tab].order.clone().unwrap();
        assert_eq!((order.column.as_str(), order.descending), ("email", false));
        set_order(&mut state, tab, "email".into(), Some(true));
        assert!(state.tables[&tab].order.as_ref().unwrap().descending);
        stage_cell(state.tables.get_mut(&tab).unwrap(), 0, 1, "x@example.com");
        set_order(&mut state, tab, "id".into(), None);
        assert_eq!(state.tables[&tab].order.as_ref().unwrap().column, "email");
        assert!(state.tables[&tab].message.is_some());
    }

    #[test]
    fn grid_edits_stage_only_real_changes() {
        let mut state = app();
        let id = uuid::Uuid::from_u128(1);
        state.open_table(id, "public".into(), "customers".into());
        let tab = state.active_tab.unwrap();
        state.tables.get_mut(&tab).unwrap().page = Some(Arc::new(page()));
        let unchanged = editable_text(&page().rows[0][1]);
        commit_edit(&mut state, tab, 0, 1, &unchanged);
        assert!(state.tables[&tab].pending.is_empty());
        commit_edit(&mut state, tab, 0, 1, "new@example.com");
        assert_eq!(state.tables[&tab].pending.len(), 1);
        assert!(state.tables[&tab].editing.is_none());
    }
}
