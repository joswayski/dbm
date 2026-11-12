//! Query tabs: toolbar (title, completion status, Refresh, Run), the code
//! editor with line numbers, SQL/Redis highlighting, the active statement
//! band and keyword completion, the per-profile history, and the results:
//! a read-only grid or, for `SELECT * FROM table`, the editable table view.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dbm_core::models::DatabaseEngine;
use dbm_core::sql_text::{ExecutionKind, TokenKind, completions, execution_target, highlight};
use gtk::prelude::*;

use crate::app::{App, set_text};
use crate::grid::ResultGrid;
use crate::icons::{self, Icon};
use crate::messages::Slot;
use crate::state::{Kind, RequestKind, State, count};
use crate::table::TableView;
use crate::ui;

const KEYWORD: &str = "#c69cff";
const STRING: &str = "#9fd88a";
const NUMBER: &str = "#f0b14c";
const COMMENT: &str = "#808087";
const QUOTED: &str = "#7db6ff";

pub struct QueryView {
    pub root: gtk::Box,
    tab_id: u64,
    engine: DatabaseEngine,
    title: gtk::Label,
    status: gtk::Label,
    refresh: gtk::Button,
    run: gtk::Button,
    selection_run: gtk::Button,
    pub editor: gtk::TextView,
    buffer: gtk::TextBuffer,
    gutter: gtk::DrawingArea,
    hint: gtk::Label,
    tags: Tags,
    history_count: gtk::Label,
    history: gtk::ListBox,
    history_stack: gtk::Stack,
    history_signature: RefCell<String>,
    message: Slot,
    results: gtk::Stack,
    actions: gtk::Box,
    copy: gtk::Button,
    save: gtk::Button,
    truncated: gtk::Label,
    notices: gtk::Box,
    notices_signature: RefCell<String>,
    grid: ResultGrid,
    table: RefCell<Option<Rc<TableView>>>,
    completion: gtk::Popover,
    completion_list: gtk::ListBox,
    /// The open completion: where the word starts (bytes) and its items.
    completion_state: RefCell<Option<(usize, Vec<String>)>>,
    /// Set while the view writes the buffer itself.
    writing: Cell<bool>,
}

struct Tags {
    kinds: [gtk::TextTag; 5],
    active: gtk::TextTag,
    line: gtk::TextTag,
}

impl QueryView {
    pub fn new(app: &App, tab_id: u64, engine: DatabaseEngine) -> Rc<Self> {
        let redis = engine == DatabaseEngine::Redis;
        let root = ui::vbox(0);
        root.add_css_class("canvas");
        // Toolbar.
        let toolbar = ui::hbox(8);
        toolbar.add_css_class("query-toolbar");
        let title = ui::ellipsized("", &["bold", "size-15", "strong"]);
        let status = ui::ellipsized("", &["size-11-5", "muted"]);
        status.set_hexpand(true);
        let refresh = ui::button(
            "secondary-button",
            Some(Icon::Refresh),
            "Refresh",
            "Re-run the last executed statement for fresh results",
        );
        let run = ui::primary_with_shortcut(Icon::Play, "Run statement", "Ctrl+Enter");
        run.set_tooltip_text(Some(if redis {
            "Run the selected command (Command/Ctrl+Enter)"
        } else {
            "Run the selected SQL (Command/Ctrl+Enter)"
        }));
        toolbar.append(&title);
        toolbar.append(&status);
        toolbar.append(&refresh);
        toolbar.append(&run);
        // Editor.
        let table = gtk::TextTagTable::new();
        let kinds = [KEYWORD, STRING, NUMBER, COMMENT, QUOTED]
            .map(|color| gtk::TextTag::builder().foreground(color).build());
        let active = gtk::TextTag::builder()
            .paragraph_background("rgba(76,154,255,0.07)")
            .build();
        let line = gtk::TextTag::builder()
            .paragraph_background("rgba(255,255,255,0.024)")
            .build();
        table.add(&line);
        table.add(&active);
        for tag in &kinds {
            table.add(tag);
        }
        let buffer = gtk::TextBuffer::new(Some(&table));
        let editor = gtk::TextView::with_buffer(&buffer);
        editor.add_css_class("code");
        editor.set_monospace(true);
        editor.set_wrap_mode(gtk::WrapMode::None);
        editor.set_top_margin(6);
        editor.set_left_margin(10);
        editor.set_vexpand(true);
        editor.set_hexpand(true);
        editor.update_property(&[gtk::accessible::Property::Label(if redis {
            "Redis command editor"
        } else {
            "SQL editor"
        })]);
        let gutter = gtk::DrawingArea::new();
        gutter.add_css_class("gutter-line");
        gutter.set_content_width(36);
        editor.set_gutter(gtk::TextWindowType::Left, Some(&gutter));
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_child(Some(&editor));
        scroller.set_vexpand(true);
        let hint = ui::label(
            if redis {
                "The outlined command or selection will run · Command/Ctrl+Enter · Preview limit: 10,000 rows"
            } else {
                "The outlined statement or selected SQL will run · Command/Ctrl+Enter · Preview limit: 10,000 rows"
            },
            &["editor-hint"],
        );
        hint.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let selection_run = ui::button("primary", Some(Icon::Play), "Run selection", "");
        selection_run.set_halign(gtk::Align::End);
        selection_run.set_valign(gtk::Align::Start);
        selection_run.set_margin_top(8);
        selection_run.set_margin_end(10);
        selection_run.set_visible(false);
        let editor_overlay = gtk::Overlay::new();
        editor_overlay.set_child(Some(&scroller));
        editor_overlay.add_overlay(&selection_run);
        let editor_card = ui::vbox(0);
        editor_card.add_css_class("card");
        editor_card.set_overflow(gtk::Overflow::Hidden);
        editor_card.append(&editor_overlay);
        editor_card.append(&hint);
        // History.
        let history_header = ui::hbox(8);
        history_header.add_css_class("history-header");
        let history_title = ui::label("History", &["bold", "size-12", "secondary"]);
        history_title.set_hexpand(true);
        let history_count = ui::label("0", &["size-11-5", "faint"]);
        history_header.append(&history_title);
        history_header.append(&history_count);
        let history = gtk::ListBox::new();
        history.set_selection_mode(gtk::SelectionMode::None);
        history.add_css_class("history-list");
        let history_scroller = gtk::ScrolledWindow::new();
        history_scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        history_scroller.set_child(Some(&history));
        let empty = ui::label("Run a query to start history.", &["muted"]);
        empty.set_wrap(true);
        empty.set_margin_start(12);
        empty.set_margin_top(12);
        empty.set_valign(gtk::Align::Start);
        let history_stack = gtk::Stack::new();
        history_stack.add_named(&empty, Some("empty"));
        history_stack.add_named(&history_scroller, Some("list"));
        history_stack.set_vexpand(true);
        let history_card = ui::vbox(0);
        history_card.add_css_class("card");
        history_card.add_css_class("chrome");
        history_card.set_overflow(gtk::Overflow::Hidden);
        history_card.set_size_request(200, -1);
        history_card.append(&history_header);
        history_card.append(&history_stack);
        let top = gtk::Paned::new(gtk::Orientation::Horizontal);
        top.set_start_child(Some(&editor_card));
        top.set_end_child(Some(&history_card));
        top.set_resize_end_child(false);
        top.set_shrink_end_child(false);
        top.set_shrink_start_child(false);
        top.add_css_class("editor-split");
        top.set_margin_start(14);
        top.set_margin_end(14);
        top.set_margin_bottom(12);
        // Results.
        let message = Slot::new();
        let copy = ui::button(
            "toolbar-button",
            Some(Icon::Copy),
            "Copy results",
            "Copy the displayed query results to the clipboard as CSV, including column headers",
        );
        let save = ui::button(
            "toolbar-button",
            Some(Icon::Download),
            "Save CSV",
            "Save the displayed query results to a CSV file",
        );
        let truncated = ui::label("", &["size-11-5", "modified-text"]);
        let actions = ui::hbox(4);
        actions.set_margin_start(14);
        actions.set_margin_end(14);
        actions.set_margin_top(6);
        actions.set_margin_bottom(6);
        actions.append(&copy);
        actions.append(&save);
        actions.append(&truncated);
        let notices = ui::vbox(2);
        notices.set_margin_start(14);
        notices.set_margin_end(14);
        let grid = ResultGrid::new();
        let grid_card = ui::vbox(0);
        grid_card.add_css_class("card");
        grid_card.set_overflow(gtk::Overflow::Hidden);
        grid_card.set_margin_start(14);
        grid_card.set_margin_end(14);
        grid_card.set_margin_bottom(12);
        grid_card.append(&grid.root);
        let grid_page = ui::vbox(0);
        grid_page.append(&actions);
        grid_page.append(&notices);
        grid_page.append(&grid_card);
        let placeholder = ui::label("Results will appear here.", &["empty-state"]);
        placeholder.set_xalign(0.5);
        let done = ui::label(
            "Statement completed without a result set.",
            &["empty-state"],
        );
        done.set_xalign(0.5);
        let results = gtk::Stack::new();
        results.set_vexpand(true);
        results.add_named(&placeholder, Some("placeholder"));
        results.add_named(&done, Some("empty"));
        results.add_named(&grid_page, Some("grid"));
        let bottom = ui::vbox(0);
        bottom.append(&message.root);
        bottom.append(&results);
        let split = gtk::Paned::new(gtk::Orientation::Vertical);
        split.add_css_class("editor-split");
        split.set_start_child(Some(&top));
        split.set_end_child(Some(&bottom));
        split.set_position(260);
        split.set_shrink_start_child(false);
        split.set_shrink_end_child(false);
        split.set_vexpand(true);
        root.append(&toolbar);
        root.append(&split);
        // Completion.
        let completion_list = gtk::ListBox::new();
        completion_list.add_css_class("completion");
        completion_list.set_selection_mode(gtk::SelectionMode::Single);
        let completion = gtk::Popover::new();
        completion.set_has_arrow(false);
        completion.set_autohide(false);
        completion.set_can_focus(false);
        completion.set_position(gtk::PositionType::Bottom);
        completion.set_child(Some(&completion_list));
        completion.set_parent(&editor);
        let view = Rc::new(Self {
            root,
            tab_id,
            engine,
            title,
            status,
            refresh,
            run,
            selection_run,
            editor,
            buffer,
            gutter,
            hint,
            tags: Tags {
                kinds,
                active,
                line,
            },
            history_count,
            history,
            history_stack,
            history_signature: RefCell::new("unset".into()),
            message,
            results,
            actions,
            copy,
            save,
            truncated,
            notices,
            notices_signature: RefCell::new(String::new()),
            grid,
            table: RefCell::new(None),
            completion,
            completion_list,
            completion_state: RefCell::new(None),
            writing: Cell::new(false),
        });
        view.connect(app, &scroller);
        view
    }

    fn connect(self: &Rc<Self>, app: &App, scroller: &gtk::ScrolledWindow) {
        let tab_id = self.tab_id;
        let weak = Rc::downgrade(self);
        self.gutter.set_draw_func(move |area, cr, width, _| {
            if let Some(view) = weak.upgrade() {
                view.draw_gutter(area, cr, width);
            }
        });
        let gutter = self.gutter.clone();
        scroller
            .vadjustment()
            .connect_value_changed(move |_| gutter.queue_draw());
        let handle = app.clone();
        self.run.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(sql) = target(state, tab_id).map(|t| t.sql) {
                    state.run_query(tab_id, sql, false, false);
                }
            });
        });
        let handle = app.clone();
        self.selection_run.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(sql) = target(state, tab_id).map(|t| t.sql) {
                    state.run_query(tab_id, sql, false, false);
                }
            });
        });
        let handle = app.clone();
        self.refresh.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(sql) = state.tab(tab_id).and_then(|t| t.last_executed.clone()) {
                    state.run_query(tab_id, sql, true, false);
                }
            });
        });
        let handle = app.clone();
        self.copy
            .connect_clicked(move |_| handle.update(|state| state.copy_result(tab_id)));
        let handle = app.clone();
        self.save
            .connect_clicked(move |_| handle.update(|state| state.save_result(tab_id)));
        // Text and selection.
        let handle = app.clone();
        let weak = Rc::downgrade(self);
        self.buffer.connect_changed(move |buffer| {
            let Some(view) = weak.upgrade() else { return };
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            view.highlight(&text);
            view.gutter.queue_draw();
            let (from, to) = view.selection_bytes(&text);
            let typed = !view.writing.get();
            handle.update(|state| {
                if let Some(tab) = state.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.sql = text.clone();
                    tab.selection = (from, to);
                }
            });
            if typed {
                view.refine_completion(&text, from, to);
            }
            view.mark_active(&text, from, to);
        });
        let handle = app.clone();
        let weak = Rc::downgrade(self);
        self.buffer.connect_mark_set(move |buffer, _, mark| {
            let Some(view) = weak.upgrade() else { return };
            if mark.name().as_deref() != Some("insert")
                && mark.name().as_deref() != Some("selection_bound")
            {
                return;
            }
            let text = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            let (from, to) = view.selection_bytes(&text);
            let mut changed = false;
            handle.quiet(|state| {
                if let Some(tab) = state.tabs.iter_mut().find(|t| t.id == tab_id) {
                    changed = tab.selection != (from, to) && tab.sql == text;
                    if changed {
                        tab.selection = (from, to);
                    }
                }
            });
            if changed {
                view.close_completion();
                view.mark_active(&text, from, to);
                handle.queue_render();
            }
        });
        // Keys: Ctrl+Enter runs; completion keys go to the open list.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let handle = app.clone();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(view) = weak.upgrade() else {
                return gtk::glib::Propagation::Proceed;
            };
            let control = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            if control && matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter) {
                view.close_completion();
                handle.update(|state| {
                    if let Some(sql) = target(state, tab_id).map(|t| t.sql) {
                        state.run_query(tab_id, sql, false, false);
                    }
                });
                return gtk::glib::Propagation::Stop;
            }
            if view.completion_key(key) {
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        self.editor.add_controller(keys);
        let weak = Rc::downgrade(self);
        self.completion_list.connect_row_activated(move |_, row| {
            if let Some(view) = weak.upgrade() {
                view.accept_completion(row.index() as usize);
            }
        });
        let handle = app.clone();
        self.history.connect_row_activated(move |_, row| {
            let sql = row.widget_name().to_string();
            let index: usize = sql.parse().unwrap_or(usize::MAX);
            handle.update(|state| {
                let Some(profile) = state.tab(tab_id).map(|t| t.profile_id) else {
                    return;
                };
                let Some(database) = state
                    .workspaces
                    .get(&profile)
                    .map(|w| w.profile.default_database.clone())
                else {
                    return;
                };
                if let Some(entry) = state
                    .histories
                    .get(&(profile, database))
                    .and_then(|e| e.get(index))
                {
                    let sql = entry.sql.clone();
                    state.pick_history(tab_id, sql);
                }
            });
        });
    }

    /// The embedded table viewer, while a `SELECT *` result shows.
    pub fn table_view(&self) -> Option<Rc<TableView>> {
        self.table.borrow().clone()
    }

    pub fn teardown(&self) {
        self.completion.unparent();
        if let Some(table) = self.table.borrow().as_ref() {
            table.teardown();
        }
    }

    /// Replaces the editor text (a new tab or a history pick).
    pub fn set_text(&self, text: &str) {
        self.writing.set(true);
        self.buffer.set_text(text);
        self.buffer.place_cursor(&self.buffer.end_iter());
        self.writing.set(false);
    }

    fn selection_bytes(&self, text: &str) -> (usize, usize) {
        let byte = |iter: &gtk::TextIter| {
            text.char_indices()
                .nth(iter.offset() as usize)
                .map_or(text.len(), |(i, _)| i)
        };
        let insert = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let bound = self.buffer.iter_at_mark(&self.buffer.selection_bound());
        (byte(&bound), byte(&insert))
    }

    fn highlight(&self, text: &str) {
        let (start, end) = self.buffer.bounds();
        for tag in &self.tags.kinds {
            self.buffer.remove_tag(tag, &start, &end);
        }
        let chars = char_offsets(text);
        for token in highlight(self.engine, text) {
            let tag = &self.tags.kinds[match token.kind {
                TokenKind::Keyword => 0,
                TokenKind::String => 1,
                TokenKind::Number => 2,
                TokenKind::Comment => 3,
                TokenKind::QuotedIdentifier => 4,
            }];
            let from = self.buffer.iter_at_offset(chars[token.from] as i32);
            let to = self
                .buffer
                .iter_at_offset(chars[token.to.min(text.len())] as i32);
            self.buffer.apply_tag(tag, &from, &to);
        }
    }

    /// The band under the statement Ctrl+Enter runs, and the cursor's line.
    fn mark_active(&self, text: &str, from: usize, to: usize) {
        let (start, end) = self.buffer.bounds();
        self.buffer.remove_tag(&self.tags.active, &start, &end);
        self.buffer.remove_tag(&self.tags.line, &start, &end);
        let insert = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let mut line_start = insert;
        line_start.set_line_offset(0);
        let mut line_end = insert;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        self.buffer
            .apply_tag(&self.tags.line, &line_start, &line_end);
        if from != to {
            return;
        }
        if let Some(target) = execution_target(self.engine, text, from, to) {
            let chars = char_offsets(text);
            let a = self.buffer.iter_at_offset(chars[target.from] as i32);
            let b = self
                .buffer
                .iter_at_offset(chars[target.to.min(text.len())] as i32);
            self.buffer.apply_tag(&self.tags.active, &a, &b);
        }
    }

    fn draw_gutter(&self, area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: i32) {
        let lines = self.buffer.line_count().max(1);
        let digits = lines.to_string().len().max(2) as i32;
        let wanted = digits * 8 + 14;
        if area.content_width() != wanted {
            area.set_content_width(wanted);
        }
        cr.set_source_rgb(
            0x16 as f64 / 255.0,
            0x16 as f64 / 255.0,
            0x18 as f64 / 255.0,
        );
        cr.paint().ok();
        cr.set_source_rgb(
            0x20 as f64 / 255.0,
            0x20 as f64 / 255.0,
            0x23 as f64 / 255.0,
        );
        cr.rectangle(f64::from(width) - 1.0, 0.0, 1.0, f64::from(area.height()));
        cr.fill().ok();
        cr.set_source_rgb(
            0x5c as f64 / 255.0,
            0x5c as f64 / 255.0,
            0x62 as f64 / 255.0,
        );
        let visible = self.editor.visible_rect();
        let (mut iter, _) = self.editor.line_at_y(visible.y());
        loop {
            let (y, height) = self.editor.line_yrange(&iter);
            if y > visible.y() + visible.height() {
                break;
            }
            let (_, window_y) =
                self.editor
                    .buffer_to_window_coords(gtk::TextWindowType::Left, 0, y);
            let number = (iter.line() + 1).to_string();
            let layout = area.create_pango_layout(Some(&number));
            let (text_width, text_height) = layout.pixel_size();
            cr.move_to(
                f64::from(width - 8 - text_width),
                f64::from(window_y) + f64::from(height - text_height) / 2.0,
            );
            pangocairo::functions::show_layout(cr, &layout);
            if !iter.forward_line() {
                break;
            }
        }
    }

    // ---- completion -----------------------------------------------------

    /// Opens or refines keyword completion after typing an identifier.
    fn refine_completion(&self, text: &str, from: usize, to: usize) {
        if from != to {
            self.close_completion();
            return;
        }
        let start = text[..to]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        let prefix = &text[start..to];
        let typed_letter = prefix
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphabetic());
        let mut items = if prefix.len() >= 2 && typed_letter {
            completions(self.engine, prefix)
        } else {
            Vec::new()
        };
        items.truncate(12);
        if items.is_empty() {
            self.close_completion();
            return;
        }
        ui::clear_list(&self.completion_list);
        for item in &items {
            let label = ui::label(item, &[]);
            self.completion_list.append(&label);
        }
        if let Some(row) = self.completion_list.row_at_index(0) {
            self.completion_list.select_row(Some(&row));
        }
        let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert());
        let location = self.editor.iter_location(&cursor);
        let (x, y) = self.editor.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            location.x(),
            location.y(),
        );
        self.completion
            .set_pointing_to(Some(&gtk::gdk::Rectangle::new(x, y, 1, location.height())));
        *self.completion_state.borrow_mut() = Some((start, items));
        self.completion.popup();
    }

    fn close_completion(&self) {
        if self.completion_state.borrow_mut().take().is_some() {
            self.completion.popdown();
        }
    }

    fn completion_key(&self, key: gtk::gdk::Key) -> bool {
        let Some(count) = self
            .completion_state
            .borrow()
            .as_ref()
            .map(|(_, i)| i.len())
        else {
            return false;
        };
        let selected = self
            .completion_list
            .selected_row()
            .map_or(0, |r| r.index() as usize);
        let select = |index: usize| {
            if let Some(row) = self.completion_list.row_at_index(index as i32) {
                self.completion_list.select_row(Some(&row));
            }
        };
        match key {
            gtk::gdk::Key::Escape => self.close_completion(),
            gtk::gdk::Key::Down => select((selected + 1) % count),
            gtk::gdk::Key::Up => select((selected + count - 1) % count),
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::Tab => {
                self.accept_completion(selected);
            }
            _ => return false,
        }
        true
    }

    fn accept_completion(&self, index: usize) {
        let Some((start, items)) = self.completion_state.borrow_mut().take() else {
            return;
        };
        self.completion.popdown();
        let Some(word) = items.get(index) else { return };
        let text = self
            .buffer
            .text(&self.buffer.start_iter(), &self.buffer.end_iter(), false)
            .to_string();
        let (_, to) = self.selection_bytes(&text);
        if start > to || !text.is_char_boundary(start) {
            return;
        }
        let chars = char_offsets(&text);
        let mut from = self.buffer.iter_at_offset(chars[start] as i32);
        let mut end = self.buffer.iter_at_offset(chars[to] as i32);
        self.writing.set(true);
        self.buffer.delete(&mut from, &mut end);
        self.buffer.insert(&mut from, word);
        self.writing.set(false);
        self.editor.grab_focus();
    }

    // ---- rendering ------------------------------------------------------

    pub fn refresh(&self, app: &App, state: &State) {
        let tab_id = self.tab_id;
        let Some(tab) = state.tab(tab_id) else { return };
        let redis = self.engine == DatabaseEngine::Redis;
        let running = state.busy(RequestKind::Query, Some(tab_id));
        let exporting = state.exporting(tab_id);
        let target = target(state, tab_id);
        let label = state.pending_label(tab_id);
        let selection = target
            .as_ref()
            .is_some_and(|t| t.kind == ExecutionKind::Selection);
        let run_label = if running && label == Some("Running query") {
            "Running…"
        } else if selection {
            "Run selection"
        } else if redis {
            "Run command"
        } else {
            "Run statement"
        };
        ui::set_button_text(&self.run, run_label);
        self.run
            .set_sensitive(!running && !exporting && target.is_some());
        self.selection_run.set_visible(selection);
        self.selection_run.set_sensitive(!running && !exporting);
        let refreshing = running && label == Some("Refreshing query");
        ui::set_button_text(
            &self.refresh,
            if refreshing {
                "Refreshing…"
            } else {
                "Refresh"
            },
        );
        self.refresh.set_sensitive(
            !running && !exporting && tab.last_executed.is_some() && !state.tab_dirty(tab_id),
        );
        self.refresh
            .set_tooltip_text(Some(if tab.last_executed.is_some() {
                "Re-run the last executed statement for fresh results"
            } else {
                "Run a statement first to enable refresh"
            }));
        set_text(&self.title, &tab.title);
        let error = state
            .query_errors
            .get(&tab_id)
            .is_some_and(|m| m.kind == Kind::Error);
        let status = match state.query_results.get(&tab_id) {
            Some(result) if !running && !error => {
                let mut status = format!("Query completed in {} ms", result.duration_ms);
                if tab.embedded.is_none() {
                    status.push_str(&format!(" · {} rows", count(result.row_count as u64)));
                    if let Some(affected) = result.affected_rows {
                        status.push_str(&format!(" · {} affected", count(affected)));
                    }
                }
                status
            }
            _ => String::new(),
        };
        set_text(&self.status, &status);
        self.status.set_tooltip_text(Some(&status));
        self.refresh_history(state, tab.profile_id);
        self.message
            .render(app, state.query_errors.get(&tab_id), move |app| {
                app.update(|state| {
                    state.query_errors.remove(&tab_id);
                });
            });
        self.refresh_results(app, state, running, exporting);
        let _ = &self.hint;
    }

    fn refresh_history(&self, state: &State, profile: uuid::Uuid) {
        let database = state
            .workspaces
            .get(&profile)
            .map(|w| w.profile.default_database.clone())
            .unwrap_or_default();
        let entries = state
            .histories
            .get(&(profile, database))
            .map_or(&[][..], Vec::as_slice);
        let entries = &entries[..entries.len().min(100)];
        let signature = format!(
            "{}|{:?}",
            entries.len(),
            entries.first().map(|e| e.executed_at)
        );
        if *self.history_signature.borrow() == signature {
            return;
        }
        *self.history_signature.borrow_mut() = signature;
        set_text(&self.history_count, &count(entries.len() as u64));
        self.history_stack
            .set_visible_child_name(if entries.is_empty() { "empty" } else { "list" });
        ui::clear_list(&self.history);
        for (index, entry) in entries.iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&index.to_string());
            row.add_css_class("history-row");
            let content = ui::hbox(8);
            let glyph = icons::image(
                if entry.success {
                    Icon::Check
                } else {
                    Icon::Alert
                },
                12,
            );
            glyph.add_css_class(if entry.success {
                "success-text"
            } else {
                "danger-text"
            });
            glyph.set_valign(gtk::Align::Start);
            glyph.set_margin_top(4);
            let text = ui::vbox(4);
            text.set_hexpand(true);
            let one_line = entry.sql.split_whitespace().collect::<Vec<_>>().join(" ");
            text.append(&ui::ellipsized(&one_line, &["size-11-5", "secondary"]));
            let time = entry
                .executed_at
                .with_timezone(&chrono::Local)
                .format("%-I:%M:%S %p")
                .to_string();
            text.append(&ui::label(&time, &["size-11", "faint"]));
            content.append(&glyph);
            content.append(&text);
            row.set_child(Some(&content));
            row.set_tooltip_text(Some(&format!("{} ms\n\n{}", entry.duration_ms, entry.sql)));
            self.history.append(&row);
        }
    }

    fn refresh_results(&self, app: &App, state: &State, running: bool, exporting: bool) {
        let tab_id = self.tab_id;
        let Some(tab) = state.tab(tab_id) else { return };
        let Some(result) = state.query_results.get(&tab_id) else {
            self.show_page("placeholder");
            return;
        };
        if tab.embedded.is_some() {
            let table = self
                .table
                .borrow_mut()
                .get_or_insert_with(|| {
                    let view = TableView::new(app, tab_id, true);
                    self.results.add_named(&view.root, Some("table"));
                    view
                })
                .clone();
            table.refresh(app, state);
            self.show_page("table");
            return;
        }
        if let Some(table) = self.table.borrow_mut().take() {
            table.teardown();
            self.results.remove(&table.root);
        }
        if result.columns.is_empty() {
            self.show_page("empty");
            return;
        }
        let enabled = !running && !exporting;
        self.actions.set_sensitive(enabled);
        ui::set_button_text(
            &self.copy,
            &format!("Copy results ({})", count(result.rows.len() as u64)),
        );
        ui::set_button_text(&self.save, if exporting { "Saving…" } else { "Save CSV" });
        self.truncated.set_visible(result.truncated);
        set_text(
            &self.truncated,
            &format!("Showing first {} rows.", count(result.rows.len() as u64)),
        );
        let notices = result.notices.join("\n");
        if *self.notices_signature.borrow() != notices {
            ui::clear(&self.notices);
            for notice in &result.notices {
                self.notices
                    .append(&ui::label(notice, &["size-11-5", "muted"]));
            }
            *self.notices_signature.borrow_mut() = notices;
        }
        self.grid.show(&result.columns, &result.rows);
        self.show_page("grid");
    }

    fn show_page(&self, name: &str) {
        if self.results.visible_child_name().as_deref() != Some(name) {
            self.results.set_visible_child_name(name);
        }
    }
}

/// What Ctrl+Enter runs in a tab: its selection or the statement under the
/// cursor.
fn target(state: &State, tab_id: u64) -> Option<dbm_core::sql_text::ExecutionTarget> {
    let tab = state.tab(tab_id)?;
    execution_target(
        state.engine(tab.profile_id),
        &tab.sql,
        tab.selection.0,
        tab.selection.1,
    )
}

/// The character offset of every byte position (and the end), for mapping
/// the core's byte ranges onto GTK's character offsets.
fn char_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0; text.len() + 1];
    let mut chars = 0;
    for (index, ch) in text.char_indices() {
        for slot in &mut offsets[index..index + ch.len_utf8()] {
            *slot = chars;
        }
        chars += 1;
    }
    offsets[text.len()] = chars;
    offsets
}

#[cfg(test)]
mod tests {
    #[test]
    fn char_offsets_map_multibyte_text() {
        let offsets = super::char_offsets("aé;b");
        assert_eq!(offsets, vec![0, 1, 1, 2, 3, 4]);
    }
}
