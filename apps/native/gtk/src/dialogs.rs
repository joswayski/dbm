//! Modal dialogs over a dimmed backdrop: the connection form, delete and
//! destructive-query confirmations, the update review, What's new, and the
//! large-export warning. A dialog rebuilds when its `signature` changes;
//! typing into a field updates the state without rebuilding.

use dbm_core::connection_url::display_connection_url;
use dbm_core::models::{DatabaseEngine, TlsMode};
use dbm_desktop::updates::WhatsNew;
use gtk::glib;
use gtk::prelude::*;

use crate::app::App;
use crate::icons::Icon;
use crate::state::{
    CONNECTION_COLORS, FeedbackKind, ProfileForm, State, count, engine_label, engine_preset_name,
};
use crate::ui;

/// What the open dialog shows; a change rebuilds it.
pub fn signature(state: &State) -> String {
    if let Some(id) = state.confirm_delete {
        let busy = state.requests().any(|m| m.label == "Deleting connection");
        return format!("delete|{id}|{busy}");
    }
    if let Some(form) = &state.profile_form {
        return format!(
            "form|{:?}|{}|{}|{:?}",
            form.id,
            form.revision,
            state.profile_form_busy(),
            form.feedback
        );
    }
    if let Some(confirm) = &state.confirm_query {
        return format!("query|{}|{}", confirm.tab, confirm.sql);
    }
    if let Some(update) = &state.confirm_update {
        return format!("update|{}", update.build);
    }
    match &state.updates.whats_new {
        WhatsNew::Closed => {}
        WhatsNew::Loading => return "whatsnew|loading".into(),
        WhatsNew::Failed(error) => return format!("whatsnew|failed|{error}"),
        WhatsNew::Shown(days) => return format!("whatsnew|{}", days.len()),
    }
    if let Some(tab) = state.confirm_export {
        return format!("export|{tab}");
    }
    String::new()
}

/// Escape and backdrop clicks: close the topmost dialog.
pub fn dismiss(state: &mut State) {
    if state.confirm_delete.take().is_some() {
        return;
    }
    if state.profile_form.is_some() {
        state.close_profile_form();
        return;
    }
    if state.confirm_query.take().is_some() || state.confirm_update.take().is_some() {
        return;
    }
    if !matches!(state.updates.whats_new, WhatsNew::Closed) {
        state.updates.close_whats_new();
        return;
    }
    state.confirm_export = None;
}

/// Per-render updates that need no rebuild. Dialogs change only through
/// their signature today, so there is nothing to do between rebuilds.
pub fn refresh(_app: &App, _state: &State) {}

pub fn build(app: &App, state: &State) -> Option<gtk::Widget> {
    let (content, width) = if let Some(id) = state.confirm_delete {
        (delete_dialog(app, state, id), 420)
    } else if let Some(form) = &state.profile_form {
        (form_dialog(app, state, form), 600)
    } else if let Some(confirm) = &state.confirm_query {
        (query_dialog(app, &confirm.sql), 480)
    } else if let Some(update) = &state.confirm_update {
        (update_dialog(app, update), 460)
    } else if !matches!(state.updates.whats_new, WhatsNew::Closed) {
        (whats_new_dialog(app, &state.updates.whats_new), 460)
    } else if let Some(tab) = state.confirm_export {
        let total = state
            .tables
            .get(&tab)
            .and_then(|t| t.total_rows())
            .unwrap_or(0);
        (export_dialog(app, total), 420)
    } else {
        return None;
    };
    let dialog = ui::vbox(0);
    dialog.add_css_class("dialog");
    dialog.set_size_request(width, -1);
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_propagate_natural_height(true);
    scroller.set_max_content_height(
        (app.window.height().max(app.window.default_height()) - 120).max(200),
    );
    scroller.set_child(Some(&content));
    dialog.append(&scroller);
    let backdrop = gtk::CenterBox::new();
    backdrop.set_vexpand(true);
    backdrop.set_hexpand(true);
    let column = ui::vbox(0);
    column.set_valign(gtk::Align::Center);
    column.append(&dialog);
    backdrop.set_center_widget(Some(&column));
    // A click on the backdrop, not the dialog, closes it.
    let click = gtk::GestureClick::new();
    let handle = app.clone();
    let inside = dialog.downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(dialog) = inside.upgrade() else {
            return;
        };
        let Some(widget) = gesture.widget() else {
            return;
        };
        let picked = widget.pick(x, y, gtk::PickFlags::DEFAULT);
        let on_dialog = picked.is_some_and(|w| w == dialog || w.is_ancestor(&dialog));
        if !on_dialog {
            handle.update(dismiss);
        }
    });
    backdrop.add_controller(click);
    Some(backdrop.upcast())
}

fn title(text: &str, eyebrow: Option<&str>) -> gtk::Box {
    let column = ui::vbox(2);
    if let Some(eyebrow) = eyebrow {
        column.append(&ui::label(eyebrow, &["eyebrow"]));
    }
    let label = ui::label(text, &["bold", "size-17", "strong"]);
    label.set_wrap(true);
    column.append(&label);
    column.set_margin_bottom(10);
    column
}

fn body(text: &str, classes: &[&str]) -> gtk::Label {
    let label = ui::label(text, classes);
    label.set_wrap(true);
    label
}

fn buttons(items: Vec<gtk::Button>) -> gtk::Box {
    let row = ui::hbox(8);
    row.set_halign(gtk::Align::End);
    row.set_margin_top(14);
    for item in items {
        row.append(&item);
    }
    row
}

fn delete_dialog(app: &App, state: &State, id: uuid::Uuid) -> gtk::Box {
    let name = state
        .profiles
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let busy = state.requests().any(|m| m.label == "Deleting connection");
    let content = ui::vbox(0);
    content.append(&title(&format!("Delete connection “{name}”?"), None));
    content.append(&body(
        "Saved password and query history for this profile will be removed. The database itself is not changed.",
        &["secondary"],
    ));
    let cancel = ui::button("secondary-button", None, "Cancel", "");
    let handle = app.clone();
    cancel.connect_clicked(move |_| handle.update(|state| state.confirm_delete = None));
    let delete = ui::button("danger-button", None, "Delete", "");
    delete.set_sensitive(!busy);
    let handle = app.clone();
    delete.connect_clicked(move |_| handle.update(State::delete_profile));
    content.append(&buttons(vec![cancel, delete]));
    content
}

fn query_dialog(app: &App, sql: &str) -> gtk::Box {
    let content = ui::vbox(0);
    content.append(&title("Run this query?", None));
    content.append(&body(
        "This query may change or remove many rows.",
        &["secondary"],
    ));
    let preview: String = sql.chars().take(400).collect();
    let code = body(&preview, &["sql-preview"]);
    code.set_margin_top(8);
    content.append(&code);
    let cancel = ui::button("secondary-button", None, "Cancel", "");
    let handle = app.clone();
    cancel.connect_clicked(move |_| handle.update(|state| state.confirm_query_choice(false)));
    let run = ui::button("danger-button", None, "Run anyway", "");
    let handle = app.clone();
    run.connect_clicked(move |_| handle.update(|state| state.confirm_query_choice(true)));
    content.append(&buttons(vec![cancel, run]));
    content
}

fn export_dialog(app: &App, total: u64) -> gtk::Box {
    let content = ui::vbox(0);
    content.append(&title("Export a large table?", None));
    content.append(&body(
        &format!(
            "This export contains {} rows and may take a while. Continue?",
            count(total)
        ),
        &["secondary"],
    ));
    let cancel = ui::button("secondary-button", None, "Cancel", "");
    let handle = app.clone();
    cancel.connect_clicked(move |_| handle.update(|state| state.confirm_export_choice(false)));
    let export = ui::button("primary", None, "Export", "");
    let handle = app.clone();
    export.connect_clicked(move |_| handle.update(|state| state.confirm_export_choice(true)));
    content.append(&buttons(vec![cancel, export]));
    content
}

/// "Version 2026.10.18.2 · October 18, 2026", the version linking to the
/// release's source commit when it is known.
fn version_line(version: &str, commit: Option<&str>, date: Option<&str>) -> gtk::Label {
    let text = format!("Version {version}");
    let mut markup = match commit.and_then(dbm_update::commit_url) {
        Some(url) => format!(
            "<a href=\"{}\">{}</a>",
            glib::markup_escape_text(&url),
            glib::markup_escape_text(&text)
        ),
        None => glib::markup_escape_text(&text).to_string(),
    };
    if let Some(date) = date {
        markup.push_str(&glib::markup_escape_text(&format!(" · {date}")));
    }
    let label = gtk::Label::new(None);
    label.set_markup(&markup);
    label.set_xalign(0.0);
    label.add_css_class("size-12");
    label.add_css_class("muted");
    label
}

fn highlights(content: &gtk::Box, items: &[dbm_update::Highlight]) {
    for item in items {
        content.append(&body(&item.title, &["bold", "strong"]));
        let text = body(&item.body, &["secondary"]);
        text.set_margin_bottom(8);
        content.append(&text);
    }
}

fn update_dialog(app: &App, update: &dbm_update::Available) -> gtk::Box {
    let content = ui::vbox(2);
    content.append(&title(
        &format!("Install Anybase {} and restart now?", update.version),
        None,
    ));
    content.append(&version_line(
        &update.version,
        update.commit.as_deref(),
        update.released.as_deref(),
    ));
    let eyebrow = ui::label("What’s new", &["eyebrow"]);
    eyebrow.set_margin_top(12);
    content.append(&eyebrow);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    match &update.highlights {
        Some(items) if items.is_empty() => {
            content.append(&body("Behind-the-scenes fixes this time.", &["secondary"]));
        }
        Some(items) => highlights(&content, items),
        None if !update.notes.trim().is_empty() => {
            content.append(&body(update.notes.trim(), &["secondary"]));
        }
        None => content.append(&body(
            "Release highlights aren’t available for this update.",
            &["secondary"],
        )),
    }
    let warning = body(
        "Unsaved query text and pending table edits will be lost.",
        &["size-12", "muted"],
    );
    warning.set_margin_top(14);
    content.append(&warning);
    let later = ui::button("secondary-button", None, "Later", "");
    let handle = app.clone();
    later.connect_clicked(move |_| handle.update(|state| state.confirm_update = None));
    let install = ui::button("primary", None, "Install and restart", "");
    let handle = app.clone();
    let update = update.clone();
    install.connect_clicked(move |_| {
        let update = update.clone();
        handle.update(move |state| {
            state.confirm_update = None;
            state.updates.install(update);
        });
    });
    content.append(&buttons(vec![later, install]));
    content
}

fn whats_new_dialog(app: &App, whats_new: &WhatsNew) -> gtk::Box {
    let content = ui::vbox(2);
    content.append(&title("What’s new", None));
    match whats_new {
        WhatsNew::Loading => {
            let row = ui::hbox(8);
            let spinner = gtk::Spinner::new();
            spinner.set_spinning(true);
            row.append(&spinner);
            row.append(&ui::label("Loading release highlights…", &["muted"]));
            content.append(&row);
        }
        WhatsNew::Failed(error) => {
            let text = body("Release highlights couldn’t be loaded.", &["secondary"]);
            text.set_tooltip_text(Some(error));
            content.append(&text);
            let retry = ui::button("secondary-button", None, "Retry", "");
            retry.add_css_class("small");
            retry.set_halign(gtk::Align::Start);
            let handle = app.clone();
            retry.connect_clicked(move |_| {
                handle.update(|state| {
                    state.updates.whats_new = WhatsNew::Closed;
                    state.updates.open_whats_new();
                });
            });
            content.append(&retry);
        }
        WhatsNew::Shown(days) if days.is_empty() => {
            content.append(&body(
                "No release highlights for this version yet.",
                &["secondary"],
            ));
        }
        WhatsNew::Shown(days) => {
            for day in days {
                content.append(&ui::label(&day.date, &["size-12", "muted"]));
                content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                for release in &day.releases {
                    let line = version_line(&release.version, Some(&release.commit), None);
                    line.set_margin_top(4);
                    content.append(&line);
                    highlights(&content, &release.highlights);
                }
            }
        }
        WhatsNew::Closed => {}
    }
    let done = ui::button("primary", None, "Done", "");
    let handle = app.clone();
    done.connect_clicked(move |_| handle.update(|state| state.updates.close_whats_new()));
    content.append(&buttons(vec![done]));
    content
}

// ---- connection form ------------------------------------------------------

/// A field in the form: typing updates the state without a rebuild and
/// refreshes the URL preview.
fn field(
    app: &App,
    preview: &gtk::Entry,
    text: &str,
    placeholder: &str,
    set: impl Fn(&mut ProfileForm, String) + 'static,
) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.set_text(text);
    entry.set_hexpand(true);
    if !placeholder.is_empty() {
        entry.set_placeholder_text(Some(placeholder));
    }
    let handle = app.clone();
    let preview = preview.downgrade();
    entry.connect_changed(move |entry| {
        let text = entry.text().to_string();
        handle.quiet(|state| {
            if let Some(form) = &mut state.profile_form {
                set(form, text);
            }
        });
        if let Some(preview) = preview.upgrade() {
            refresh_url_preview(&handle, &preview);
        }
    });
    // Enter in a text field saves, like submitting a form.
    let handle = app.clone();
    entry.connect_activate(move |_| handle.update(State::save_profile));
    entry
}

fn url_preview_text(form: &ProfileForm) -> String {
    if form.password_loaded {
        display_connection_url(&form.input(), form.show_url_password).unwrap_or_else(|e| e)
    } else if form.password_load_failed {
        "Saved password could not be loaded.".into()
    } else {
        "Loading connection URL…".into()
    }
}

fn refresh_url_preview(app: &App, preview: &gtk::Entry) {
    let Ok(state) = app.state.try_borrow() else {
        return;
    };
    if let Some(form) = &state.profile_form {
        let text = url_preview_text(form);
        if preview.text() != text {
            preview.set_text(&text);
            preview.set_tooltip_text(Some(&text));
        }
    }
}

fn labeled(label: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let column = ui::vbox(4);
    column.set_hexpand(true);
    column.append(&ui::label(label, &["form-label"]));
    column.append(widget);
    column
}

fn two(a: gtk::Box, b: gtk::Box) -> gtk::Box {
    let row = ui::hbox(12);
    row.set_homogeneous(true);
    row.append(&a);
    row.append(&b);
    row
}

fn update_form(app: &App, change: impl FnOnce(&mut ProfileForm)) {
    app.update(|state| {
        if let Some(form) = &mut state.profile_form {
            change(form);
            form.revision += 1;
        }
    });
}

#[allow(clippy::too_many_lines)]
fn form_dialog(app: &App, state: &State, form: &ProfileForm) -> gtk::Box {
    let busy = state.profile_form_busy();
    let redis = form.engine == DatabaseEngine::Redis;
    let content = ui::vbox(8);
    let header = ui::hbox(8);
    let heading = title(
        if form.id.is_some() {
            "Edit connection"
        } else {
            "New connection"
        },
        Some(&engine_label(form.engine).to_uppercase()),
    );
    heading.set_hexpand(true);
    header.append(&heading);
    let close = ui::button("toolbar-button", Some(Icon::Close), "", "Close");
    close.set_valign(gtk::Align::Start);
    let handle = app.clone();
    close.connect_clicked(move |_| handle.update(State::close_profile_form));
    header.append(&close);
    content.append(&header);
    let fields = ui::vbox(8);
    fields.set_sensitive(!busy);
    // Engine.
    let segmented = ui::hbox(2);
    segmented.add_css_class("segmented");
    segmented.set_halign(gtk::Align::Start);
    for engine in [
        DatabaseEngine::Postgres,
        DatabaseEngine::Mysql,
        DatabaseEngine::Redis,
    ] {
        let button = gtk::Button::with_label(engine_label(engine));
        if form.engine == engine {
            button.add_css_class("selected");
        }
        let handle = app.clone();
        button.connect_clicked(move |_| {
            handle.update(|state| {
                if let Some(form) = &mut state.profile_form {
                    form.set_engine(engine);
                }
            });
        });
        segmented.append(&button);
    }
    fields.append(&labeled("Database engine", &segmented));
    // Connection URL.
    let preview = gtk::Entry::new();
    preview.set_editable(false);
    preview.set_hexpand(true);
    let text = url_preview_text(form);
    preview.set_text(&text);
    preview.set_tooltip_text(Some(&text));
    let copy_url = ui::button("secondary-button", None, "Copy URL", "");
    let handle = app.clone();
    copy_url.connect_clicked(move |_| handle.update(State::copy_profile_url));
    let url_row = ui::hbox(8);
    url_row.append(&preview);
    url_row.append(&copy_url);
    fields.append(&labeled("Connection URL", &url_row));
    let url_options = ui::hbox(12);
    let show_url = gtk::CheckButton::with_label("Show password");
    show_url.set_active(form.show_url_password);
    let handle = app.clone();
    show_url.connect_toggled(move |check| {
        let on = check.is_active();
        update_form(&handle, |form| form.show_url_password = on);
    });
    url_options.append(&show_url);
    let import = ui::button(
        "secondary-button",
        None,
        if form.importing_url {
            "Cancel import"
        } else {
            "Import URL"
        },
        "",
    );
    let handle = app.clone();
    import.connect_clicked(move |_| {
        update_form(&handle, |form| form.importing_url = !form.importing_url);
    });
    url_options.append(&import);
    fields.append(&url_options);
    if form.importing_url {
        let row = ui::hbox(8);
        let url = gtk::PasswordEntry::new();
        url.set_hexpand(true);
        url.set_show_peek_icon(true);
        url.set_text(&form.url);
        url.set_property("placeholder-text", "Paste a connection URL");
        let import = ui::button("secondary-button", None, "Import", "");
        import.set_sensitive(!form.url.trim().is_empty());
        let handle = app.clone();
        let button = import.downgrade();
        url.connect_changed(move |entry| {
            let text = entry.text().to_string();
            if let Some(button) = button.upgrade() {
                button.set_sensitive(!text.trim().is_empty());
            }
            handle.quiet(|state| {
                if let Some(form) = &mut state.profile_form {
                    form.url = text;
                }
            });
        });
        let run_import = {
            let handle = app.clone();
            move || {
                handle.update(|state| {
                    if let Some(form) = &mut state.profile_form
                        && !form.url.trim().is_empty()
                    {
                        form.import_url();
                    }
                });
            }
        };
        let on_activate = run_import.clone();
        url.connect_activate(move |_| on_activate());
        import.connect_clicked(move |_| run_import());
        row.append(&url);
        row.append(&import);
        fields.append(&row);
        let focus = url.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(url) = focus.upgrade() {
                url.grab_focus();
            }
        });
    }
    // Name and color.
    let name = field(
        app,
        &preview,
        &form.name,
        engine_preset_name(form.engine),
        |f, v| f.name = v,
    );
    fields.append(&labeled("Name", &name));
    let colors = ui::hbox(6);
    for color in CONNECTION_COLORS {
        let swatch = gtk::Button::new();
        swatch.add_css_class("swatch-button");
        let dot = ui::dot(color, true, 18);
        if form.color.eq_ignore_ascii_case(color) {
            dot.add_css_class("swatch-selected");
        }
        swatch.set_child(Some(&dot));
        swatch.set_tooltip_text(Some(&format!("Use connection color {color}")));
        let handle = app.clone();
        swatch.connect_clicked(move |_| update_form(&handle, |form| form.color = color.into()));
        colors.append(&swatch);
    }
    let custom = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    if let Ok(rgba) = gtk::gdk::RGBA::parse(&form.color) {
        custom.set_rgba(&rgba);
    }
    custom.set_tooltip_text(Some("Choose a custom color"));
    let handle = app.clone();
    custom.connect_rgba_notify(move |button| {
        let rgba = button.rgba();
        let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex = format!(
            "#{:02x}{:02x}{:02x}",
            byte(rgba.red()),
            byte(rgba.green()),
            byte(rgba.blue())
        );
        update_form(&handle, |form| {
            if !form.color.eq_ignore_ascii_case(&hex) {
                form.color = hex;
            }
        });
    });
    colors.append(&custom);
    colors.append(&ui::label("Custom", &["muted"]));
    fields.append(&labeled("Connection color", &colors));
    // Host and port.
    let host = field(app, &preview, &form.host, "", |f, v| f.host = v);
    let port = field(app, &preview, &form.port.to_string(), "", |f, v| {
        let digits: String = v.chars().filter(char::is_ascii_digit).collect();
        if let Ok(port) = digits.parse::<u16>()
            && port > 0
        {
            f.port = port;
        }
    });
    port.set_max_length(5);
    port.set_input_purpose(gtk::InputPurpose::Digits);
    fields.append(&two(labeled("Host", &host), labeled("Port", &port)));
    let username = field(
        app,
        &preview,
        &form.username,
        if redis { "default" } else { "" },
        |f, v| f.username = v,
    );
    let database = field(
        app,
        &preview,
        &form.database,
        if redis { "0" } else { "" },
        |f, v| f.database = v,
    );
    fields.append(&two(
        labeled(
            if redis {
                "Username (ACL, optional)"
            } else {
                "Username"
            },
            &username,
        ),
        labeled(if redis { "Database index" } else { "Database" }, &database),
    ));
    // Password and TLS.
    let password = gtk::Entry::new();
    password.set_hexpand(true);
    password.set_visibility(form.show_password);
    password.set_text(&form.password);
    password.set_placeholder_text(Some(if form.password_load_failed {
        "Enter password"
    } else if form.id.is_some() && !form.password_loaded {
        "Loading saved password…"
    } else {
        "Stored in OS keychain"
    }));
    let handle = app.clone();
    let preview_ref = preview.downgrade();
    password.connect_changed(move |entry| {
        let text = entry.text().to_string();
        handle.quiet(|state| {
            if let Some(form) = &mut state.profile_form
                && form.password != text
            {
                form.password = text;
                form.password_edited = true;
                form.password_loaded = true;
                form.password_load_failed = false;
            }
        });
        if let Some(preview) = preview_ref.upgrade() {
            refresh_url_preview(&handle, &preview);
        }
    });
    let handle = app.clone();
    password.connect_activate(move |_| handle.update(State::save_profile));
    let reveal = ui::button(
        "secondary-button",
        None,
        if form.show_password { "Hide" } else { "Show" },
        "",
    );
    let handle = app.clone();
    reveal.connect_clicked(move |_| {
        update_form(&handle, |form| form.show_password = !form.show_password);
    });
    let password_row = ui::hbox(8);
    password_row.append(&password);
    password_row.append(&reveal);
    let tls = gtk::DropDown::from_strings(&["Preferred", "Required", "Disabled"]);
    tls.set_selected(match form.tls {
        TlsMode::Preferred => 0,
        TlsMode::Required => 1,
        TlsMode::Disabled => 2,
    });
    let handle = app.clone();
    let preview_ref = preview.downgrade();
    tls.connect_selected_notify(move |dropdown| {
        let mode = match dropdown.selected() {
            1 => TlsMode::Required,
            2 => TlsMode::Disabled,
            _ => TlsMode::Preferred,
        };
        handle.quiet(|state| {
            if let Some(form) = &mut state.profile_form {
                form.tls = mode;
            }
        });
        if let Some(preview) = preview_ref.upgrade() {
            refresh_url_preview(&handle, &preview);
        }
    });
    fields.append(&two(
        labeled("Password", &password_row),
        labeled("TLS", &tls),
    ));
    let ca = field(
        app,
        &preview,
        &form.ca_cert_path,
        "/path/to/root-ca.pem",
        |f, v| f.ca_cert_path = v,
    );
    fields.append(&labeled("CA certificate path (optional)", &ca));
    let read_only =
        gtk::CheckButton::with_label("Read-only profile (blocks GUI edits and mutations)");
    read_only.set_active(form.read_only);
    let handle = app.clone();
    read_only.connect_toggled(move |check| {
        let on = check.is_active();
        handle.quiet(|state| {
            if let Some(form) = &mut state.profile_form {
                form.read_only = on;
            }
        });
    });
    let startup = gtk::CheckButton::with_label("Open on startup");
    startup.set_tooltip_text(Some(
        "Connect to this profile's saved database when Anybase launches.",
    ));
    startup.set_active(form.open_on_startup);
    let handle = app.clone();
    startup.connect_toggled(move |check| {
        let on = check.is_active();
        handle.quiet(|state| {
            if let Some(form) = &mut state.profile_form {
                form.open_on_startup = on;
            }
        });
    });
    fields.append(&read_only);
    fields.append(&startup);
    content.append(&fields);
    if let Some((kind, message)) = &form.feedback {
        let feedback = body(message, &["feedback"]);
        feedback.add_css_class(match kind {
            FeedbackKind::Info => "info",
            FeedbackKind::Success => "success",
            FeedbackKind::Error => "error",
        });
        content.append(&feedback);
    }
    content.append(&body(
        "Passwords are stored in your operating system credential manager and are never written to Anybase's profile database.",
        &["note"],
    ));
    let actions = ui::hbox(8);
    actions.set_margin_top(4);
    if form.id.is_some() {
        let delete = ui::button("danger-button", None, "Delete", "");
        delete.set_sensitive(!busy);
        let handle = app.clone();
        delete.connect_clicked(move |_| handle.update(State::request_delete_profile));
        actions.append(&delete);
    }
    actions.append(&ui::spacer());
    let testing = busy && !form.saving;
    let test = ui::button(
        "secondary-button",
        None,
        if testing {
            "Testing…"
        } else {
            "Test connection"
        },
        "",
    );
    test.set_sensitive(!busy);
    let handle = app.clone();
    test.connect_clicked(move |_| handle.update(State::test_profile));
    let save_label = match (&form.feedback, form.saving) {
        (Some((FeedbackKind::Info, _)), true) => "Testing…",
        (_, true) => "Connecting…",
        _ => "Save & connect",
    };
    let save = ui::button("primary", None, save_label, "");
    save.set_sensitive(!busy);
    let handle = app.clone();
    save.connect_clicked(move |_| handle.update(State::save_profile));
    actions.append(&test);
    actions.append(&save);
    content.append(&actions);
    // A fresh dialog starts in its first field; a rebuild keeps quiet.
    if form.revision == 0 && form.id.is_none() {
        let name = name.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(name) = name.upgrade() {
                name.grab_focus();
            }
        });
    }
    content
}
