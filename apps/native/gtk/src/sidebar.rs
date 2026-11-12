//! The connection sidebar: saved connections with their status, and under
//! the active connection its database picker, schema refresh, filter, and
//! schema or keyspace tree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use dbm_core::models::{DatabaseEngine, SchemaNode};
use dbm_core::sql_text::filter_schema_nodes;
use gtk::prelude::*;
use uuid::Uuid;

use crate::app::{App, set_text};
use crate::icons::{self, Icon};
use crate::state::{State, engine_label};
use crate::ui;

pub struct Sidebar {
    pub root: gtk::Box,
    list: gtk::Box,
    collapse: gtk::Button,
    new_connection: gtk::Button,
    signature: RefCell<String>,
    workspaces: RefCell<HashMap<Uuid, Rc<Workspace>>>,
}

/// The widgets under a connected profile. They outlive sidebar rebuilds so
/// the filter keeps its text and focus.
struct Workspace {
    root: gtk::Box,
    database_label: gtk::Label,
    databases: gtk::DropDown,
    names: RefCell<Vec<String>>,
    schema_label: gtk::Label,
    refresh: gtk::Button,
    filter: gtk::Entry,
    tree: gtk::Box,
    tree_signature: RefCell<String>,
}

impl Sidebar {
    pub fn new() -> Self {
        let root = ui::vbox(0);
        root.add_css_class("sidebar");
        root.set_size_request(220, -1);
        let header = ui::hbox(8);
        header.set_margin_start(12);
        header.set_margin_end(10);
        header.set_margin_top(10);
        header.set_margin_bottom(4);
        let title = ui::label("Connections", &["section-label"]);
        title.set_hexpand(true);
        let collapse = ui::button(
            "toolbar-button",
            Some(Icon::Sidebar),
            "",
            "Collapse sidebar",
        );
        header.append(&title);
        header.append(&collapse);
        let list = ui::vbox(0);
        list.set_margin_start(10);
        list.set_margin_end(10);
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&list));
        let footer = ui::vbox(0);
        footer.add_css_class("sidebar-footer");
        footer.set_margin_start(10);
        footer.set_margin_end(10);
        footer.set_margin_bottom(10);
        let new_connection = ui::button(
            "secondary-button",
            Some(Icon::Plus),
            "New connection",
            "Create a connection profile",
        );
        footer.append(&new_connection);
        root.append(&header);
        root.append(&scroller);
        root.append(&footer);
        Self {
            root,
            list,
            collapse,
            new_connection,
            signature: RefCell::new("unset".into()),
            workspaces: RefCell::new(HashMap::new()),
        }
    }
}

pub fn connect(app: &App) {
    let sidebar = &app.shell.sidebar;
    let handle = app.clone();
    sidebar
        .collapse
        .connect_clicked(move |_| handle.update(|state| state.sidebar_collapsed = true));
    let handle = app.clone();
    sidebar.new_connection.connect_clicked(move |_| {
        handle.update(|state| state.profile_form = Some(crate::state::ProfileForm::fresh()));
    });
}

fn subtitle(profile: &dbm_core::models::ConnectionProfile) -> String {
    let target = if profile.username.is_empty() {
        profile.host.clone()
    } else {
        format!("{}@{}", profile.username, profile.host)
    };
    format!("{} · {target}", engine_label(profile.engine))
}

pub fn render(app: &App, state: &State) {
    let sidebar = &app.shell.sidebar;
    // Drop widgets of profiles that are gone or disconnected.
    sidebar
        .workspaces
        .borrow_mut()
        .retain(|id, _| state.workspaces.contains_key(id));
    let mut signature = String::new();
    for profile in &state.profiles {
        let id = profile.id;
        let connected = state.workspaces.contains_key(&id);
        let active = state.active_profile == Some(id);
        let expanded = active && connected && !state.collapsed_profiles.contains(&id);
        let connecting = state.has_label(id, &["Connecting", "Switching database"]);
        signature.push_str(&format!(
            "{id}|{}|{}|{}|{}|{connected}{active}{expanded}{connecting};",
            profile.name,
            subtitle(profile),
            state.profile_color(id),
            profile.read_only
        ));
    }
    if *sidebar.signature.borrow() != signature {
        *sidebar.signature.borrow_mut() = signature;
        rebuild(app, state);
    }
    for (id, workspace) in sidebar.workspaces.borrow().iter() {
        refresh_workspace(app, state, *id, workspace);
    }
}

fn rebuild(app: &App, state: &State) {
    let sidebar = &app.shell.sidebar;
    // Keep each workspace's widgets: detach them before their rows go.
    for workspace in sidebar.workspaces.borrow().values() {
        if let Some(parent) = workspace.root.parent().and_downcast::<gtk::Box>() {
            parent.remove(&workspace.root);
        }
    }
    ui::clear(&sidebar.list);
    if state.profiles.is_empty() {
        let empty = ui::label("No saved connections.", &["size-12", "faint"]);
        empty.set_xalign(0.5);
        empty.set_margin_top(18);
        sidebar.list.append(&empty);
    }
    for profile in &state.profiles {
        let id = profile.id;
        let connected = state.workspaces.contains_key(&id);
        let active = state.active_profile == Some(id);
        let expanded = active && connected && !state.collapsed_profiles.contains(&id);
        let connecting = state.has_label(id, &["Connecting", "Switching database"]);
        let color = state.profile_color(id);
        let row = ui::hbox(0);
        row.add_css_class("connection-item");
        if active {
            row.add_css_class("active");
        }
        let item = gtk::Button::new();
        item.add_css_class("connection-item");
        item.set_hexpand(true);
        let content = ui::hbox(0);
        let dot = ui::dot(&color, connected, 8);
        dot.set_margin_start(9);
        dot.set_margin_end(9);
        let text = ui::vbox(2);
        text.set_valign(gtk::Align::Center);
        text.set_hexpand(true);
        let name = ui::ellipsized(&profile.name, &["connection-name"]);
        let detail = ui::ellipsized(&subtitle(profile), &["connection-subtitle"]);
        text.append(&name);
        text.append(&detail);
        content.append(&dot);
        content.append(&text);
        if profile.read_only {
            let chip = ui::chip("Read-only", "muted");
            chip.set_margin_end(4);
            content.append(&chip);
        }
        item.set_child(Some(&content));
        item.set_tooltip_text(Some(&if connected {
            format!("{}\n{}", profile.name, subtitle(profile))
        } else {
            format!("Connect to {}\n{}", profile.name, subtitle(profile))
        }));
        let handle = app.clone();
        item.connect_clicked(move |_| handle.update(|state| state.select_connection(id)));
        row.append(&item);
        let actions = ui::hbox(0);
        actions.set_valign(gtk::Align::Start);
        actions.set_margin_top(1);
        if connecting {
            let spinner = gtk::Spinner::new();
            spinner.set_spinning(true);
            spinner.set_margin_end(6);
            spinner.set_valign(gtk::Align::Center);
            actions.append(&spinner);
        } else if connected && active {
            let (icon, tip) = if expanded {
                (Icon::ChevronUp, "Collapse connection")
            } else {
                (Icon::ChevronDown, "Expand connection")
            };
            let toggle = ui::button("toolbar-button", Some(icon), "", tip);
            let handle = app.clone();
            toggle.connect_clicked(move |_| {
                handle.update(|state| {
                    if !state.collapsed_profiles.remove(&id) {
                        state.collapsed_profiles.insert(id);
                    }
                });
            });
            actions.append(&toggle);
        }
        let handle = app.clone();
        let more = ui::menu_button(
            "toolbar-button",
            &icons::image(Icon::More, 14),
            "Connection actions",
            move || handle.profile_menu(id, connected),
            200,
        );
        actions.append(&more);
        row.append(&actions);
        let block = ui::vbox(0);
        block.append(&row);
        if expanded {
            let workspace = sidebar
                .workspaces
                .borrow_mut()
                .entry(id)
                .or_insert_with(|| Workspace::new(app, id))
                .clone();
            block.append(&workspace.root);
        }
        sidebar.list.append(&block);
    }
}

impl Workspace {
    fn new(app: &App, id: Uuid) -> Rc<Self> {
        let root = ui::vbox(4);
        root.set_margin_start(10);
        root.set_margin_end(8);
        root.set_margin_top(2);
        root.set_margin_bottom(6);
        let database_label = ui::label("Database", &["section-label"]);
        let databases = gtk::DropDown::from_strings(&[]);
        databases.set_hexpand(true);
        let schema_header = ui::hbox(4);
        schema_header.set_margin_top(10);
        let schema_label = ui::label("Schema", &["section-label"]);
        schema_label.set_hexpand(true);
        let refresh = ui::button(
            "text-button",
            Some(Icon::Refresh),
            "Refresh",
            "Reload the schema tree",
        );
        schema_header.append(&schema_label);
        schema_header.append(&refresh);
        let filter = gtk::Entry::new();
        filter.add_css_class("schema-filter");
        filter.set_primary_icon_name(Some("system-search-symbolic"));
        let tree = ui::vbox(0);
        root.append(&database_label);
        root.append(&databases);
        root.append(&schema_header);
        root.append(&filter);
        root.append(&tree);
        let workspace = Rc::new(Self {
            root,
            database_label,
            databases,
            names: RefCell::new(Vec::new()),
            schema_label,
            refresh,
            filter,
            tree,
            tree_signature: RefCell::new("unset".into()),
        });
        let handle = app.clone();
        let weak = Rc::downgrade(&workspace);
        workspace
            .databases
            .connect_selected_notify(move |dropdown| {
                let Some(workspace) = weak.upgrade() else {
                    return;
                };
                let Some(name) = workspace
                    .names
                    .borrow()
                    .get(dropdown.selected() as usize)
                    .cloned()
                else {
                    return;
                };
                handle.update(|state| state.switch_database(id, name));
            });
        let handle = app.clone();
        workspace
            .refresh
            .connect_clicked(move |_| handle.update(|state| state.refresh_schema(id)));
        let handle = app.clone();
        workspace.filter.connect_changed(move |entry| {
            let text = entry.text().to_string();
            handle.update(|state| {
                state.schema_filters.insert(id, text);
            });
        });
        // Escape clears the filter and keeps it focused.
        let keys = gtk::EventControllerKey::new();
        let entry = workspace.filter.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(entry) = entry.upgrade()
                && !entry.text().is_empty()
            {
                entry.set_text("");
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        workspace.filter.add_controller(keys);
        workspace
    }
}

fn refresh_workspace(app: &App, state: &State, id: Uuid, workspace: &Workspace) {
    let Some(info) = state.workspaces.get(&id) else {
        return;
    };
    let redis = info.profile.engine == DatabaseEngine::Redis;
    set_text(
        &workspace.database_label,
        if redis { "Database index" } else { "Database" },
    );
    set_text(
        &workspace.schema_label,
        if redis { "Keyspace" } else { "Schema" },
    );
    workspace.filter.set_placeholder_text(Some(if redis {
        "Filter keys…"
    } else {
        "Filter tables…"
    }));
    let names: Vec<String> = info
        .databases
        .iter()
        .filter(|d| d.is_connectable)
        .map(|d| d.name.clone())
        .collect();
    if *workspace.names.borrow() != names {
        let strings: Vec<&str> = names.iter().map(String::as_str).collect();
        workspace
            .databases
            .set_model(Some(&gtk::StringList::new(&strings)));
        *workspace.names.borrow_mut() = names.clone();
    }
    let current = names
        .iter()
        .position(|n| *n == info.profile.default_database)
        .map_or(gtk::INVALID_LIST_POSITION, |i| i as u32);
    if workspace.databases.selected() != current {
        workspace.databases.set_selected(current);
    }
    workspace
        .databases
        .set_sensitive(!state.has_label(id, &["Switching database"]));
    let refreshing = state.pending_requests.values().any(|m| {
        m.profile == Some(id) && matches!(m.label, "Loading schema" | "Refreshing schema")
    });
    workspace.refresh.set_sensitive(!refreshing);
    ui::set_button_text(
        &workspace.refresh,
        if refreshing {
            "Refreshing…"
        } else {
            "Refresh"
        },
    );
    let query = state.schema_filters.get(&id).cloned().unwrap_or_default();
    if workspace.filter.text() != query {
        workspace.filter.set_text(&query);
    }
    let selected = state
        .active_tab
        .and_then(|tab| state.table_target(tab))
        .filter(|(profile, _, _)| *profile == id)
        .map(|(_, schema, table)| format!("{schema}.{table}"));
    let tree = state.schemas.get(&id);
    let toggled: Vec<&String> = state
        .toggled_nodes
        .iter()
        .filter(|(p, _)| *p == id)
        .map(|(_, path)| path)
        .collect();
    let signature = format!(
        "{:p}|{query}|{selected:?}|{toggled:?}|{}",
        tree.map_or(std::ptr::null(), |t| t.as_ptr()),
        state.profile_color(id),
    );
    if *workspace.tree_signature.borrow() == signature {
        return;
    }
    *workspace.tree_signature.borrow_mut() = signature;
    ui::clear(&workspace.tree);
    let Some(all) = tree else {
        let loading = ui::hbox(6);
        let spinner = gtk::Spinner::new();
        spinner.set_spinning(true);
        loading.append(&spinner);
        loading.append(&ui::label("Loading…", &["muted"]));
        workspace.tree.append(&loading);
        return;
    };
    let filtering = !query.trim().is_empty();
    let filtered;
    let nodes = if filtering {
        filtered = filter_schema_nodes(all, &query);
        filtered.as_slice()
    } else {
        all.as_slice()
    };
    let color = state.profile_color(id);
    for node in nodes {
        branch(
            app,
            state,
            &workspace.tree,
            id,
            node,
            0,
            "",
            filtering,
            (&selected, &color),
        );
    }
    if filtering && nodes.is_empty() {
        let empty = ui::label(&format!("No matches for “{}”.", query.trim()), &["muted"]);
        empty.set_wrap(true);
        workspace.tree.append(&empty);
    }
}

#[allow(clippy::too_many_arguments)]
fn branch(
    app: &App,
    state: &State,
    tree: &gtk::Box,
    profile: Uuid,
    node: &SchemaNode,
    depth: i32,
    parent_path: &str,
    force_open: bool,
    (selected, color): (&Option<String>, &str),
) {
    let leaf = node.schema.as_ref().zip(node.table.as_ref());
    let path = format!("{parent_path}/{}:{}", node.kind, node.name);
    let open = leaf.is_none()
        && (force_open || (depth < 1) != state.toggled_nodes.contains(&(profile, path.clone())));
    let is_selected = leaf.is_some_and(|(schema, table)| {
        selected.as_deref() == Some(format!("{schema}.{table}").as_str())
    });
    let row = gtk::Button::new();
    row.add_css_class("schema-row");
    if is_selected {
        row.add_css_class("selected");
        row.add_css_class(&ui::color_class(color));
    }
    let content = ui::hbox(0);
    content.set_margin_start(8 + depth * 14);
    let caret = icons::image(
        if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        11,
    );
    caret.add_css_class("caret");
    caret.set_size_request(12, 12);
    caret.set_opacity(if leaf.is_none() { 1.0 } else { 0.0 });
    caret.set_margin_end(6);
    let glyph = match (leaf.is_some(), node.kind.as_str()) {
        (false, _) => Icon::Folder,
        (true, "key") => Icon::Key,
        (true, "view") => Icon::View,
        (true, _) => Icon::Table,
    };
    let icon = icons::image(glyph, 13);
    if node.kind == "schema" {
        icon.add_css_class("schema");
    }
    icon.set_margin_end(6);
    let name = ui::ellipsized(&node.name, &[]);
    content.append(&caret);
    content.append(&icon);
    content.append(&name);
    row.set_child(Some(&content));
    // The full name when the sidebar cuts it off.
    if node.name.chars().count() > 22 {
        row.set_tooltip_text(Some(&node.name));
    }
    row.update_property(&[gtk::accessible::Property::Label(&node.name)]);
    let handle = app.clone();
    match leaf {
        Some((schema, table)) => {
            let (schema, table) = (schema.clone(), table.clone());
            row.connect_clicked(move |_| {
                let (schema, table) = (schema.clone(), table.clone());
                handle.update(move |state| state.open_table(profile, schema, table));
            });
        }
        None => {
            let key = (profile, path.clone());
            row.connect_clicked(move |_| {
                let key = key.clone();
                handle.update(move |state| {
                    if !state.toggled_nodes.remove(&key) {
                        state.toggled_nodes.insert(key);
                    }
                });
            });
        }
    }
    tree.append(&row);
    if open {
        for child in &node.children {
            branch(
                app,
                state,
                tree,
                profile,
                child,
                depth + 1,
                &path,
                force_open,
                (selected, color),
            );
        }
    }
}
