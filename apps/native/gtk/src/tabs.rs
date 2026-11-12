//! The tab strip (connection-colored, renamable, collapsible tabs) and the
//! welcome pane shown when no tab is open.

use std::cell::RefCell;

use gtk::prelude::*;

use crate::app::{App, set_text};
use crate::icons::{self, Icon};
use crate::state::{State, Tab, TabKind};
use crate::ui;

pub struct TabStrip {
    pub root: gtk::Box,
    tabs: gtk::Box,
    scroller: gtk::ScrolledWindow,
    signature: RefCell<String>,
}

impl TabStrip {
    pub fn new() -> Self {
        let root = ui::hbox(0);
        root.add_css_class("tabstrip");
        let tabs = ui::hbox(1);
        tabs.set_valign(gtk::Align::End);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        scroller.set_hexpand(true);
        scroller.set_propagate_natural_height(true);
        scroller.set_child(Some(&tabs));
        root.append(&scroller);
        Self {
            root,
            tabs,
            scroller,
            signature: RefCell::new("unset".into()),
        }
    }

    pub fn render(&self, app: &App, state: &State) {
        let mut signature = format!(
            "{:?}|{:?}|{:?}|",
            state.active_tab,
            state.renaming,
            state
                .active_profile
                .filter(|p| state.workspaces.contains_key(p))
        );
        for tab in &state.tabs {
            signature.push_str(&format!(
                "{}:{}:{}:{}:{};",
                tab.id,
                tab.title,
                tab.collapsed,
                state.profile_color(tab.profile_id),
                matches!(tab.kind, TabKind::Query)
            ));
        }
        if *self.signature.borrow() == signature {
            return;
        }
        *self.signature.borrow_mut() = signature;
        ui::clear(&self.tabs);
        let mut active_widget = None;
        for tab in &state.tabs {
            let widget = if tab.collapsed {
                collapsed_tab(app, tab)
            } else {
                self.tab(app, state, tab)
            };
            if state.active_tab == Some(tab.id) {
                active_widget = Some(widget.clone());
            }
            self.tabs.append(&widget);
        }
        if let Some(profile) = state
            .active_profile
            .filter(|p| state.workspaces.contains_key(p))
        {
            let plus = ui::button("toolbar-button", Some(Icon::Plus), "", "New query");
            plus.set_valign(gtk::Align::Center);
            let handle = app.clone();
            plus.connect_clicked(move |_| handle.update(|state| state.open_query(profile, true)));
            self.tabs.append(&plus);
        }
        // Newly selected tabs scroll into view.
        if let Some(widget) = active_widget {
            let scroller = self.scroller.clone();
            gtk::glib::idle_add_local_once(move || {
                if let Some(bounds) = widget.compute_bounds(&scroller) {
                    let adjustment = scroller.hadjustment();
                    if adjustment.upper() <= adjustment.page_size() {
                        return;
                    }
                    let (left, right) = (
                        f64::from(bounds.x()),
                        f64::from(bounds.x() + bounds.width()),
                    );
                    let value = adjustment.value();
                    if left < 0.0 {
                        adjustment.set_value(value + left);
                    } else if right > adjustment.page_size() {
                        adjustment.set_value(value + right - adjustment.page_size());
                    }
                }
            });
        }
    }

    fn tab(&self, app: &App, state: &State, tab: &Tab) -> gtk::Widget {
        let id = tab.id;
        let active = state.active_tab == Some(id);
        let color = state.profile_color(tab.profile_id);
        let content = ui::hbox(6);
        content.add_css_class("tab");
        if active {
            content.add_css_class("active");
        }
        content.set_focusable(true);
        content.set_tooltip_text(Some(&tab.title));
        content.update_property(&[gtk::accessible::Property::Label(&tab.title)]);
        let glyph = icons::image(
            if tab.kind == TabKind::Query {
                Icon::Code
            } else {
                Icon::Table
            },
            13,
        );
        glyph.add_css_class("tab-icon");
        glyph.add_css_class(&ui::color_class(&color));
        content.append(&glyph);
        if state.renaming == Some(id) {
            let entry = gtk::Entry::new();
            entry.add_css_class("tab-title-input");
            entry.set_text(&tab.title);
            entry.set_width_chars(14);
            entry.set_max_width_chars(14);
            let handle = app.clone();
            entry.connect_activate(move |entry| {
                let title = entry.text().to_string();
                handle.update(move |state| state.rename_tab(id, &title));
            });
            let keys = gtk::EventControllerKey::new();
            let handle = app.clone();
            keys.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Escape {
                    handle.update(|state| state.renaming = None);
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            entry.add_controller(keys);
            let focus = gtk::EventControllerFocus::new();
            let handle = app.clone();
            let weak = entry.downgrade();
            focus.connect_leave(move |_| {
                if let Some(entry) = weak.upgrade() {
                    let title = entry.text().to_string();
                    handle.update(move |state| {
                        if state.renaming == Some(id) {
                            state.rename_tab(id, &title);
                        }
                    });
                }
            });
            entry.add_controller(focus);
            content.append(&entry);
            let entry = entry.downgrade();
            gtk::glib::idle_add_local_once(move || {
                if let Some(entry) = entry.upgrade() {
                    entry.grab_focus();
                    entry.select_region(0, -1);
                }
            });
        } else {
            let title = ui::ellipsized(&tab.title, &[]);
            title.set_max_width_chars(24);
            content.append(&title);
            if tab.kind == TabKind::Query {
                let pencil = ui::button(
                    "tab-close",
                    Some(Icon::Pencil),
                    "",
                    &format!("Rename {}", tab.title),
                );
                pencil.add_css_class("hover-only");
                let handle = app.clone();
                pencil.connect_clicked(move |_| {
                    handle.update(|state| state.renaming = Some(id));
                });
                content.append(&pencil);
            }
        }
        if active {
            let collapse = ui::button(
                "tab-close",
                Some(Icon::Collapse),
                "",
                &format!("Collapse {}", tab.title),
            );
            let handle = app.clone();
            collapse.connect_clicked(move |_| handle.update(|state| state.collapse_tab(id)));
            content.append(&collapse);
        }
        let close = ui::button(
            "tab-close",
            Some(Icon::Close),
            "",
            &format!("Close {}", tab.title),
        );
        if !active {
            close.add_css_class("hover-only");
        }
        let handle = app.clone();
        close.connect_clicked(move |_| handle.update(|state| state.close_tab(id)));
        content.append(&close);
        // The whole tab selects; double-click renames a query; middle-click
        // closes.
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let handle = app.clone();
        let query = tab.kind == TabKind::Query;
        click.connect_pressed(move |gesture, presses, x, y| {
            if on_button(gesture, x, y) {
                return;
            }
            match gesture.current_button() {
                2 => handle.update(|state| state.close_tab(id)),
                1 if presses == 2 && query => handle.update(|state| state.renaming = Some(id)),
                1 => handle.update(|state| state.select_tab(id)),
                _ => {}
            }
        });
        content.add_controller(click);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&content));
        if active {
            let edge = ui::hbox(0);
            edge.add_css_class("tab-edge");
            edge.add_css_class(&ui::color_class(&color));
            edge.set_size_request(-1, 2);
            edge.set_valign(gtk::Align::Start);
            edge.set_can_target(false);
            overlay.add_overlay(&edge);
        }
        overlay.upcast()
    }
}

fn collapsed_tab(app: &App, tab: &Tab) -> gtk::Widget {
    let id = tab.id;
    let content = ui::hbox(2);
    content.add_css_class("tab");
    content.add_css_class("collapsed");
    content.set_tooltip_text(Some(&format!("Expand {}", tab.title)));
    let column = ui::vbox(0);
    column.set_valign(gtk::Align::Center);
    column.append(&icons::image(Icon::Expand, 12));
    let title = ui::ellipsized(&tab.title, &["faint"]);
    title.set_max_width_chars(5);
    title.add_css_class("size-11");
    column.append(&title);
    column.set_hexpand(true);
    content.append(&column);
    // Keep the collapsed tab narrow; the column's hexpand would otherwise
    // stretch it across the whole strip.
    content.set_hexpand(false);
    let close = ui::button(
        "tab-close",
        Some(Icon::Close),
        "",
        &format!("Close {}", tab.title),
    );
    let handle = app.clone();
    close.connect_clicked(move |_| handle.update(|state| state.close_tab(id)));
    content.append(&close);
    let click = gtk::GestureClick::new();
    let handle = app.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        if !on_button(gesture, x, y) {
            handle.update(|state| state.select_tab(id));
        }
    });
    content.add_controller(click);
    content.upcast()
}

/// Whether a press on a tab landed on one of its buttons (close, collapse,
/// rename). The tab's own gesture sees the press first and would select the
/// tab, re-rendering the strip and removing the button before it could
/// receive its click. Hit-tests each button's own area: `Widget::pick`
/// misses these buttons inside the padded tab.
fn on_button(gesture: &gtk::GestureClick, x: f64, y: f64) -> bool {
    let Some(widget) = gesture.widget() else {
        return false;
    };
    let point = gtk::graphene::Point::new(x as f32, y as f32);
    let mut child = widget.first_child();
    while let Some(current) = child {
        if current.is::<gtk::Button>()
            && let Some(inner) = widget.compute_point(&current, &point)
            && current.contains(f64::from(inner.x()), f64::from(inner.y()))
        {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

pub struct Welcome {
    pub root: gtk::Box,
    title: gtk::Label,
    message: gtk::Label,
}

impl Welcome {
    pub fn new() -> Self {
        let root = ui::vbox(0);
        root.add_css_class("canvas");
        root.set_valign(gtk::Align::Fill);
        let column = ui::vbox(0);
        column.set_halign(gtk::Align::Center);
        column.set_valign(gtk::Align::Start);
        column.set_margin_top(140);
        let icon = ui::vbox(0);
        icon.add_css_class("welcome-icon");
        icon.set_halign(gtk::Align::Center);
        let glyph = icons::image(crate::icons::Icon::Database, 28);
        glyph.set_vexpand(true);
        icon.append(&glyph);
        let title = ui::label("", &["bold", "size-20", "strong"]);
        title.set_xalign(0.5);
        title.set_margin_top(22);
        let message = ui::label("", &["muted"]);
        message.set_xalign(0.5);
        message.set_justify(gtk::Justification::Center);
        message.set_wrap(true);
        message.set_max_width_chars(60);
        message.set_margin_top(8);
        column.append(&icon);
        column.append(&title);
        column.append(&message);
        root.append(&column);
        Self {
            root,
            title,
            message,
        }
    }

    pub fn refresh(&self, state: &State) {
        let profile = state.active_profile.and_then(|id| state.profile(id));
        let connected = profile.is_some_and(|p| state.workspaces.contains_key(&p.id));
        set_text(
            &self.title,
            profile.map_or("No connection selected", |p| p.name.as_str()),
        );
        let message = match (profile, connected) {
            (Some(_), true) => {
                "Choose a table from the sidebar or open a new query with the plus button above."
            }
            (Some(_), false) => {
                "This connection is selected but not connected. Select it again to connect."
            }
            (None, _) if state.profiles.is_empty() => {
                "Create a connection from the sidebar to get started."
            }
            (None, _) => "Select a saved connection from the sidebar to browse its data.",
        };
        set_text(&self.message, message);
    }
}
