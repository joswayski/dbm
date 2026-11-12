//! Dialogs: the connection form, delete connection, destructive query,
//! install update, What's new and large export.

use std::sync::Mutex;

use dbm_core::connection_url::{display_connection_url, format_connection_url};
use dbm_core::models::{DatabaseEngine, TlsMode};
use dbm_desktop::updates::{self, WhatsNew};
use dbm_desktop::worker::Command;
use dbm_ui::input::Key;
use dbm_ui::text_edit::TextEdit;
use dbm_ui::widgets::{
    self, danger_button, dialog_title, icon_button_in, modal, primary_button, secondary_button,
};
use dbm_ui::{Color32, Dir, Id, Rect, Sense, Stroke, Ui, Vec2, pos2, theme, vec2};

use crate::state::{
    FeedbackKind, PASSWORD_LOAD, RequestKind, Workbench, engine_label, engine_preset_name,
};

/// The colour the system picker returned, picked up on the next frame.
static PICKED_COLOR: Mutex<Option<[u8; 3]>> = Mutex::new(None);

/// Two columns 12 px apart; `f` fills column 0, then column 1, and the
/// parent advances past the taller.
fn columns(ui: &mut Ui, mut f: impl FnMut(&mut Ui, usize)) {
    let avail = ui.available_rect();
    let w = (avail.width() - 12.0) / 2.0;
    let mut bottom = avail.top();
    for i in 0..2 {
        let r = Rect::from_min_size(
            pos2(avail.left() + i as f32 * (w + 12.0), avail.top()),
            vec2(w, avail.height()),
        );
        let mut c = ui.child(r, Dir::Down);
        c.spacing.y = 4.0;
        f(&mut c, i);
        bottom = bottom.max(c.min_rect().bottom());
    }
    ui.advance_past(Rect::from_ltrb(
        avail.left(),
        avail.top(),
        avail.right(),
        bottom,
    ));
}

fn form_label(ui: &mut Ui, text: &str) {
    ui.label(text, theme::ui(12.0), theme::MUTED);
}

fn field(
    ui: &mut Ui,
    id: Id,
    text: &mut String,
    hint: &str,
    name: &str,
) -> dbm_ui::text_edit::TextEditOutput {
    TextEdit::singleline(id, text)
        .hint(hint)
        .margin(9.0, 7.0)
        .name(name)
        .show(ui)
}

/// A right-aligned row of buttons; `f` adds them right to left.
fn button_row<R>(ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = ui.allocate_row(30.0);
    let mut row = ui.child(rect, Dir::Left);
    row.spacing.x = 8.0;
    f(&mut row)
}

/// "Version 2026.10.18.2 · October 18, 2026", the version linking to the
/// release's source commit when it is known.
fn version_line(ui: &mut Ui, version: &str, commit: Option<&str>, date: Option<&str>) {
    let font = theme::ui(12.0);
    ui.horizontal(18.0, |ui| {
        ui.spacing.x = 0.0;
        let text = format!("Version {version}");
        match commit.and_then(dbm_update::commit_url) {
            Some(url) => {
                let w = ui.ctx.fonts.width(&text, font);
                let r = ui.allocate(vec2(w, font.line_height()));
                let id = ui.make_id(("version-link", version));
                let resp = ui.interact(r, id, Sense::CLICK).on_hover_text(url.clone());
                let mut node = dbm_ui::a11y::Node::new(id.0, dbm_ui::a11y::Role::Link, &text, r);
                node.focusable = true;
                ui.a11y(node);
                ui.painter().text_styled(
                    r.min,
                    text,
                    font,
                    theme::ACCENT_TEXT,
                    resp.hovered,
                    false,
                );
                if resp.clicked {
                    ui.ctx.open_url(&url);
                }
            }
            None => {
                ui.label(&text, font, theme::MUTED);
            }
        }
        if let Some(date) = date {
            ui.label(&format!(" · {date}"), font, theme::MUTED);
        }
    });
}

/// Bold titles over short, muted descriptions.
fn release_highlights(ui: &mut Ui, highlights: &[dbm_update::Highlight]) {
    for h in highlights {
        ui.label_wrapped(&h.title, theme::bold(13.0), theme::TEXT_STRONG, false);
        ui.label_wrapped(&h.body, theme::ui(12.5), theme::SECONDARY, false);
        ui.add_space(8.0);
    }
}

/// Cancel (Escape) and a confirm button; `Some(true)` confirms.
fn confirm_buttons(
    ui: &mut Ui,
    confirm: widgets::ActionButton<'_>,
    cancel: &str,
    enabled: bool,
) -> Option<bool> {
    button_row(ui, |row| {
        if confirm.enabled(enabled).show(row).clicked {
            return Some(true);
        }
        if secondary_button(cancel).show(row).clicked {
            return Some(false);
        }
        None
    })
}

impl Workbench {
    /// Captures' fallback: the install error, Try again, and a link to
    /// download the latest release by hand.
    fn update_failed_dialog(&mut self, ui: &mut Ui) {
        let updates::State::Failed(error) = &self.updates.state else {
            return;
        };
        if !self.updates.failure_open {
            return;
        }
        let error = error.clone();
        let out = modal(ui, "Update failed", 440.0, |ui| {
            dialog_title(ui, None, "The update couldn’t install");
            ui.label_wrapped(&error, theme::ui(12.5), theme::SECONDARY, false);
            ui.add_space(12.0);
            ui.label_wrapped(
                "Try again, or download the latest version from anyba.se and install it over this one. Your connections and history stay.",
                theme::ui(12.0),
                theme::MUTED,
                false,
            );
            ui.add_space(14.0);
            confirm_buttons(
                ui,
                primary_button("Download from anyba.se"),
                "Try again",
                true,
            )
        });
        match out.inner.flatten() {
            Some(true) => {
                ui.ctx.open_url(updates::DOWNLOAD_PAGE_URL);
                self.updates.failure_open = false;
            }
            Some(false) => self.updates.check(false),
            None if out.backdrop_clicked || ui.ctx.key_pressed(Key::Escape) => {
                self.updates.failure_open = false;
            }
            None => {}
        }
    }

    pub(super) fn dialogs(&mut self, ui: &mut Ui) {
        self.profile_dialog(ui);
        if let Some(id) = self.confirm_delete {
            let name = self.profile(id).map(|p| p.name.clone()).unwrap_or_default();
            let busy = self.metas().any(|m| m.label == "Deleting connection");
            let out = modal(ui, "Delete connection", 420.0, |ui| {
                dialog_title(ui, None, &format!("Delete connection “{name}”?"));
                ui.label_wrapped("Saved password and query history for this profile will be removed. The database itself is not changed.", theme::ui(12.5), theme::SECONDARY, false);
                ui.add_space(14.0);
                confirm_buttons(ui, danger_button("Delete"), "Cancel", !busy)
            });
            let escape = ui.ctx.key_pressed(Key::Escape);
            match out
                .inner
                .flatten()
                .or((out.backdrop_clicked || escape).then_some(false))
            {
                Some(true) if self.profile_is_clean(id) => {
                    self.send(Command::DeleteProfile(id), "Deleting connection", None)
                }
                Some(false) => self.confirm_delete = None,
                _ => {}
            }
        }
        if let Some(confirm) = &self.confirm_query {
            let preview: String = confirm.sql.chars().take(400).collect();
            let out = modal(ui, "Run destructive query", 480.0, |ui| {
                dialog_title(ui, None, "Run this query?");
                ui.label_wrapped(
                    "This query may change or remove many rows.",
                    theme::ui(12.5),
                    theme::SECONDARY,
                    false,
                );
                ui.add_space(8.0);
                let (_, _) = ui.frame(
                    dbm_ui::Frame::new()
                        .fill(theme::BG)
                        .radius(6.0)
                        .inner_margin(dbm_ui::Margin::same(8.0)),
                    |ui| {
                        ui.label_wrapped(&preview, theme::ui(12.0), theme::TEXT, false);
                    },
                );
                ui.add_space(14.0);
                confirm_buttons(ui, danger_button("Run anyway"), "Cancel", true)
            });
            let escape = ui.ctx.key_pressed(Key::Escape);
            if let Some(run) = out
                .inner
                .flatten()
                .or((out.backdrop_clicked || escape).then_some(false))
                && let Some(confirm) = self.confirm_query.take()
                && run
            {
                self.run_query(confirm.tab, confirm.sql, confirm.refresh, true);
            }
        }
        if let Some(update) = self.confirm_update.clone() {
            let out = modal(ui, "Install update", 460.0, |ui| {
                dialog_title(
                    ui,
                    None,
                    &format!("Install Anybase {} and restart now?", update.version),
                );
                version_line(
                    ui,
                    &update.version,
                    update.commit.as_deref(),
                    update.released.as_deref(),
                );
                ui.add_space(12.0);
                ui.label("What’s new", theme::bold(10.5), theme::FAINT);
                ui.separator(theme::BORDER);
                ui.add_space(4.0);
                match &update.highlights {
                    Some(h) if h.is_empty() => {
                        ui.label_wrapped(
                            "Behind-the-scenes fixes this time.",
                            theme::ui(12.5),
                            theme::SECONDARY,
                            false,
                        );
                    }
                    Some(h) => release_highlights(ui, h),
                    None if !update.notes.trim().is_empty() => {
                        ui.label_wrapped(
                            update.notes.trim(),
                            theme::ui(12.5),
                            theme::SECONDARY,
                            false,
                        );
                    }
                    None => {
                        ui.label_wrapped(
                            "Release highlights aren’t available for this update.",
                            theme::ui(12.5),
                            theme::SECONDARY,
                            false,
                        );
                    }
                }
                ui.add_space(14.0);
                ui.label_wrapped(
                    "Unsaved query text and pending table edits will be lost.",
                    theme::ui(12.0),
                    theme::MUTED,
                    false,
                );
                ui.add_space(14.0);
                confirm_buttons(ui, primary_button("Install and restart"), "Later", true)
            });
            let escape = ui.ctx.key_pressed(Key::Escape);
            if let Some(install) = out
                .inner
                .flatten()
                .or((out.backdrop_clicked || escape).then_some(false))
            {
                self.confirm_update = None;
                if install {
                    self.updates.install(update);
                }
            }
        }
        self.update_failed_dialog(ui);
        self.whats_new_dialog(ui);
        if let Some(tab_id) = self.confirm_export {
            let total = self
                .tables
                .get(&tab_id)
                .and_then(|s| s.page.as_ref())
                .and_then(|p| p.total_rows)
                .unwrap_or(0);
            let out = modal(ui, "Large export", 420.0, |ui| {
                dialog_title(ui, None, "Export a large table?");
                ui.label_wrapped(
                    &format!(
                        "This export contains {} rows and may take a while. Continue?",
                        theme::count(total)
                    ),
                    theme::ui(12.5),
                    theme::SECONDARY,
                    false,
                );
                ui.add_space(14.0);
                confirm_buttons(ui, primary_button("Export"), "Cancel", true)
            });
            let escape = ui.ctx.key_pressed(Key::Escape);
            if let Some(export) = out
                .inner
                .flatten()
                .or((out.backdrop_clicked || escape).then_some(false))
            {
                self.confirm_export = None;
                if export {
                    self.start_export(tab_id);
                }
            }
        }
    }

    /// Release highlights grouped by UTC day, newest first.
    fn whats_new_dialog(&mut self, ui: &mut Ui) {
        if matches!(self.updates.whats_new, WhatsNew::Closed) {
            return;
        }
        let mut retry = false;
        let state = &self.updates.whats_new;
        let out = modal(ui, "What’s new", 460.0, |ui| {
            dialog_title(ui, None, "What’s new");
            match state {
                WhatsNew::Loading => {
                    ui.horizontal(20.0, |ui| {
                        widgets::spinner(ui, 14.0);
                        ui.label("Loading release highlights…", theme::ui(12.5), theme::MUTED);
                    });
                }
                WhatsNew::Failed(error) => {
                    let r = ui.label_wrapped(
                        "Release highlights couldn’t be loaded.",
                        theme::ui(12.5),
                        theme::SECONDARY,
                        false,
                    );
                    let id = ui.make_id("whats-new-error");
                    let _ = ui
                        .interact(r, id, Sense::HOVER)
                        .on_hover_text(error.clone());
                    if secondary_button("Retry").small().show(ui).clicked {
                        retry = true;
                    }
                }
                WhatsNew::Shown(days) if days.is_empty() => {
                    ui.label_wrapped(
                        "No release highlights for this version yet.",
                        theme::ui(12.5),
                        theme::SECONDARY,
                        false,
                    );
                }
                WhatsNew::Shown(days) => {
                    for day in days {
                        ui.label(&day.date, theme::ui(12.0), theme::MUTED);
                        ui.separator(theme::BORDER);
                        ui.add_space(4.0);
                        for release in &day.releases {
                            version_line(ui, &release.version, Some(&release.commit), None);
                            ui.add_space(4.0);
                            release_highlights(ui, &release.highlights);
                        }
                        ui.add_space(6.0);
                    }
                }
                WhatsNew::Closed => {}
            }
            ui.add_space(8.0);
            button_row(ui, |row| primary_button("Done").show(row).clicked)
        });
        let done =
            out.inner.unwrap_or(false) || out.backdrop_clicked || ui.ctx.key_pressed(Key::Escape);
        if retry {
            self.updates.whats_new = WhatsNew::Closed;
            self.updates.open_whats_new();
        } else if done {
            self.updates.close_whats_new();
        }
        let _ = updates::State::Idle;
    }

    fn profile_dialog(&mut self, ui: &mut Ui) {
        let Some(mut form) = self.profile_form.take() else {
            return;
        };
        if let Some([r, g, b]) = PICKED_COLOR.lock().ok().and_then(|mut c| c.take()) {
            form.color = format!("#{r:02x}{g:02x}{b:02x}");
        }
        // Loading the saved password can wait behind other work; it neither
        // locks the form nor keeps it open.
        let busy = self
            .metas()
            .any(|m| m.kind == RequestKind::Form && m.label != PASSWORD_LOAD);
        let mut close = false;
        let mut test = false;
        let mut save = false;
        let mut delete = false;
        let mut copy_url = false;
        let mut url_enter = false;
        let mut field_enter = false;
        let mut pick_color = false;
        let out = modal(ui, "Connection", 600.0, |ui| {
            let top = ui.cursor();
            let w = ui.available_width();
            dialog_title(
                ui,
                Some(&engine_label(form.engine).to_uppercase()),
                if form.id.is_some() {
                    "Edit connection"
                } else {
                    "New connection"
                },
            );
            if icon_button_in(
                ui,
                Rect::from_min_size(pos2(top.x + w - 28.0, top.y), Vec2::splat(28.0)),
                true,
                dbm_ui::icons::Icon::Close,
                None,
                "Close",
            )
            .clicked
            {
                close = true;
            }
            ui.set_enabled(!busy);
            ui.spacing.y = 4.0;
            form_label(ui, "Database engine");
            // A segmented control on the control fill.
            let seg = ui.allocate(vec2(3.0 * 120.0 + 2.0 * 2.0 + 4.0, 30.0));
            ui.painter().rect_filled(seg, 7.0, theme::CONTROL);
            for (i, engine) in [
                DatabaseEngine::Postgres,
                DatabaseEngine::Mysql,
                DatabaseEngine::Redis,
            ]
            .into_iter()
            .enumerate()
            {
                let r = Rect::from_min_size(
                    pos2(seg.left() + 2.0 + i as f32 * 122.0, seg.top() + 2.0),
                    vec2(120.0, 26.0),
                );
                let id = Id::new(("engine", i));
                let resp = ui.interact(r, id, Sense::CLICK);
                let selected = form.engine == engine;
                let mut node = dbm_ui::a11y::Node::new(
                    id.0,
                    dbm_ui::a11y::Role::Button,
                    engine_label(engine),
                    r,
                );
                node.focusable = true;
                node.selected = Some(selected);
                ui.a11y(node);
                let mut p = ui.painter();
                if selected {
                    p.rect_filled(r, 5.0, theme::CONTROL_ACTIVE);
                } else if resp.hovered {
                    p.rect_filled(r, 5.0, Color32::from_white_alpha(10));
                }
                let label = engine_label(engine);
                let tw = p.fonts().width(label, theme::ui(12.5));
                p.text_mid(
                    r.center().x - tw / 2.0,
                    r.center().y,
                    label,
                    theme::ui(12.5),
                    if selected {
                        theme::TEXT_STRONG
                    } else {
                        theme::MUTED
                    },
                );
                if resp.clicked {
                    form.set_engine(engine);
                }
            }
            ui.add_space(8.0);
            form_label(ui, "Connection URL");
            let preview = if form.password_loaded {
                display_connection_url(&form.input(), form.show_url_password).unwrap_or_else(|e| e)
            } else if form.password_load_failed {
                "Saved password could not be loaded.".into()
            } else {
                "Loading connection URL…".into()
            };
            ui.horizontal(32.0, |ui| {
                let mut text = preview.clone();
                let w = (ui.available_width() - 108.0).max(1.0);
                TextEdit::singleline(Id::new("profile-url-preview"), &mut text)
                    .read_only()
                    .margin(9.0, 7.0)
                    .width(w)
                    .name("Connection URL")
                    .show(ui);
                if secondary_button("Copy URL").show(ui).clicked {
                    copy_url = true;
                }
            });
            ui.horizontal(32.0, |ui| {
                widgets::checkbox(ui, &mut form.show_url_password, "Show password");
                if secondary_button(if form.importing_url {
                    "Cancel import"
                } else {
                    "Import URL"
                })
                .show(ui)
                .clicked
                {
                    form.importing_url = !form.importing_url;
                }
            });
            if form.importing_url {
                ui.horizontal(32.0, |ui| {
                    let o = TextEdit::singleline(Id::new("profile-import-url"), &mut form.url)
                        .password(true)
                        .hint("Paste a connection URL")
                        .margin(9.0, 7.0)
                        .width(400.0)
                        .name("Connection URL to import")
                        .show(ui);
                    url_enter = o.submitted;
                    let can = !form.url.trim().is_empty();
                    let clicked = secondary_button("Import").enabled(can).show(ui).clicked;
                    if (clicked || o.submitted || (o.response.changed && form.url.contains("://")))
                        && can
                    {
                        form.import_url();
                    }
                });
            }
            ui.add_space(8.0);
            form_label(ui, "Name");
            field_enter |= field(
                ui,
                Id::new("profile-name"),
                &mut form.name,
                engine_preset_name(form.engine),
                "Name",
            )
            .submitted;
            ui.add_space(8.0);
            form_label(ui, "Connection color");
            ui.horizontal(26.0, |ui| {
                for color in theme::CONNECTION_COLORS {
                    let r = ui.allocate(Vec2::splat(22.0));
                    let id = Id::new(("profile-color", color));
                    let resp = ui
                        .interact(r, id, Sense::CLICK)
                        .on_hover_text(format!("Use connection color {color}"));
                    let mut node = dbm_ui::a11y::Node::new(
                        id.0,
                        dbm_ui::a11y::Role::Button,
                        format!("Connection color {color}"),
                        r,
                    );
                    node.focusable = true;
                    node.selected = Some(form.color.eq_ignore_ascii_case(color));
                    ui.a11y(node);
                    let mut p = ui.painter();
                    p.circle_filled(r.center(), 9.0, theme::parse_color(color));
                    if form.color.eq_ignore_ascii_case(color) {
                        p.circle_stroke(r.center(), 11.0, Stroke::new(2.0, theme::TEXT_STRONG));
                    }
                    if resp.clicked {
                        form.color = color.into();
                    }
                }
                ui.add_space(8.0);
                let r = ui.allocate(vec2(28.0, 22.0));
                let id = Id::new("profile-color-custom");
                let resp = ui
                    .interact(r, id, Sense::CLICK)
                    .on_hover_text("Choose a custom color");
                let mut node = dbm_ui::a11y::Node::new(
                    id.0,
                    dbm_ui::a11y::Role::Button,
                    "Choose a custom color",
                    r,
                );
                node.focusable = true;
                ui.a11y(node);
                ui.painter().rect(
                    r,
                    5.0,
                    theme::parse_color(&form.color),
                    Stroke::new(1.0, theme::BORDER_STRONG),
                );
                if resp.clicked {
                    pick_color = true;
                }
                ui.label("Custom", theme::ui(12.5), theme::MUTED);
            });
            ui.add_space(8.0);
            let redis = form.engine == DatabaseEngine::Redis;
            let password_hint = if form.password_load_failed {
                "Enter password"
            } else if form.id.is_some() && !form.password_loaded {
                "Loading saved password…"
            } else {
                "Stored in OS keychain"
            };
            let mut enter = false;
            columns(ui, |c, i| {
                if i == 0 {
                    form_label(c, "Host");
                    enter |=
                        field(c, Id::new("profile-host"), &mut form.host, "", "Host").submitted;
                } else {
                    form_label(c, "Port");
                    let o = TextEdit::singleline(Id::new("profile-port"), &mut form.port_text)
                        .char_limit(5)
                        .margin(9.0, 7.0)
                        .name("Port")
                        .show(c);
                    enter |= o.submitted;
                    if o.response.changed {
                        form.port_edited();
                    }
                    if !o.response.has_focus {
                        form.port_text = form.port.to_string();
                    }
                }
            });
            ui.add_space(6.0);
            columns(ui, |c, i| {
                if i == 0 {
                    form_label(
                        c,
                        if redis {
                            "Username (ACL, optional)"
                        } else {
                            "Username"
                        },
                    );
                    enter |= field(
                        c,
                        Id::new("profile-user"),
                        &mut form.username,
                        if redis { "default" } else { "" },
                        "Username",
                    )
                    .submitted;
                } else {
                    form_label(c, if redis { "Database index" } else { "Database" });
                    enter |= field(
                        c,
                        Id::new("profile-db"),
                        &mut form.database,
                        if redis { "0" } else { "" },
                        "Database",
                    )
                    .submitted;
                }
            });
            ui.add_space(6.0);
            columns(ui, |c, i| {
                if i == 0 {
                    form_label(c, "Password");
                    c.horizontal(32.0, |ui| {
                        let w = (ui.available_width() - 72.0).max(1.0);
                        let o =
                            TextEdit::singleline(Id::new("profile-password"), &mut form.password)
                                .password(!form.show_password)
                                .hint(password_hint)
                                .margin(9.0, 7.0)
                                .width(w)
                                .name("Password")
                                .show(ui);
                        enter |= o.submitted;
                        if o.response.changed {
                            form.password_edited = true;
                            form.password_loaded = true;
                            form.password_load_failed = false;
                        }
                        if secondary_button(if form.show_password { "Hide" } else { "Show" })
                            .show(ui)
                            .clicked
                        {
                            form.show_password = !form.show_password;
                        }
                    });
                } else {
                    form_label(c, "TLS");
                    let label = |t: &TlsMode| match t {
                        TlsMode::Preferred => "Preferred",
                        TlsMode::Required => "Required",
                        TlsMode::Disabled => "Disabled",
                    };
                    let w = c.available_width();
                    let mut chosen = None;
                    widgets::combo_box(c, Id::new("profile-tls"), label(&form.tls), w, |ui| {
                        for t in [TlsMode::Preferred, TlsMode::Required, TlsMode::Disabled] {
                            if widgets::select_item(ui, label(&t), form.tls == t).clicked {
                                chosen = Some(t);
                                return true;
                            }
                        }
                        false
                    });
                    if let Some(t) = chosen {
                        form.tls = t;
                    }
                }
            });
            field_enter |= enter;
            ui.add_space(8.0);
            form_label(ui, "CA certificate path (optional)");
            field_enter |= field(
                ui,
                Id::new("profile-ca"),
                &mut form.ca_cert_path,
                "/path/to/root-ca.pem",
                "CA certificate path",
            )
            .submitted;
            ui.add_space(8.0);
            widgets::checkbox(
                ui,
                &mut form.read_only,
                "Read-only profile (blocks GUI edits and mutations)",
            );
            widgets::checkbox(ui, &mut form.open_on_startup, "Open on startup")
                .on_hover_text("Connect to this profile's saved database when Anybase launches.");
            ui.set_enabled(true);
            if let Some((kind, message)) = &form.feedback {
                ui.add_space(8.0);
                let (fill, stroke, color) = match kind {
                    FeedbackKind::Info => (
                        theme::ACCENT_SOFT,
                        Color32::from_rgba_unmultiplied(76, 154, 255, 77),
                        theme::ACCENT_TEXT,
                    ),
                    FeedbackKind::Success => (
                        Color32::from_rgba_unmultiplied(90, 211, 148, 26),
                        Color32::from_rgba_unmultiplied(90, 211, 148, 77),
                        Color32::from_rgb(0x9b, 0xe7, 0xbf),
                    ),
                    FeedbackKind::Error => (
                        theme::DANGER_SOFT,
                        Color32::from_rgba_unmultiplied(255, 107, 97, 77),
                        Color32::from_rgb(0xff, 0xb3, 0xac),
                    ),
                };
                ui.frame(
                    dbm_ui::Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(1.0, stroke))
                        .radius(7.0)
                        .inner_margin(dbm_ui::Margin::symmetric(12.0, 9.0)),
                    |ui| {
                        ui.label_wrapped(message, theme::ui(12.0), color, false);
                    },
                );
            }
            ui.add_space(8.0);
            ui.frame(dbm_ui::Frame::new().fill(Color32::from_white_alpha(10)).radius(7.0).inner_margin(dbm_ui::Margin::symmetric(12.0, 10.0)), |ui| {
                ui.label_wrapped("Passwords are stored in your operating system credential manager and are never written to Anybase's profile database.", theme::ui(11.5), theme::FAINT, false);
            });
            ui.add_space(12.0);
            let rect = ui.allocate_row(30.0);
            {
                let mut row = ui.child(rect, Dir::Left);
                row.spacing.x = 8.0;
                let label = match (&form.feedback, form.saving) {
                    (Some((FeedbackKind::Info, _)), true) => "Testing…",
                    (_, true) => "Connecting…",
                    _ => "Save & connect",
                };
                if primary_button(label).enabled(!busy).show(&mut row).clicked {
                    save = true;
                }
                let testing = busy && !form.saving;
                if secondary_button(if testing {
                    "Testing…"
                } else {
                    "Test connection"
                })
                .enabled(!busy)
                .show(&mut row)
                .clicked
                {
                    test = true;
                }
            }
            if form.id.is_some() {
                let mut row = ui.child(rect, Dir::Right);
                if danger_button("Delete")
                    .enabled(!busy)
                    .show(&mut row)
                    .clicked
                {
                    delete = true;
                }
            }
        });
        close |= out.backdrop_clicked || ui.ctx.key_pressed(Key::Escape);
        // Enter in a text field saves, like submitting the desktop form.
        if field_enter && !url_enter && !busy {
            save = true;
        }
        if pick_color {
            let c = theme::parse_color(&form.color).to_unmultiplied_f32();
            let initial = [
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
            ];
            crate::platform::pick_color(initial, self.wake.clone(), |picked| {
                if let (Some(c), Ok(mut slot)) = (picked, PICKED_COLOR.lock()) {
                    *slot = Some(c);
                }
            });
        }
        if copy_url {
            form.saving = false;
            if form.password_loaded && !form.password_load_failed {
                match format_connection_url(&form.input(), Some(&form.password)) {
                    Ok(url) => {
                        ui.ctx.copy_text(&url);
                        form.feedback = Some((FeedbackKind::Success, "URL copied.".into()));
                    }
                    Err(error) => form.feedback = Some((FeedbackKind::Error, error)),
                }
            } else {
                form.feedback = Some((
                    FeedbackKind::Error,
                    "Cannot copy the full URL because the saved password did not load.".into(),
                ));
            }
        }
        if test {
            form.feedback = None;
            form.saving = false;
            self.dispatch(
                Command::TestProfile(form.input()),
                "Testing connection",
                None,
                None,
                RequestKind::Form,
            );
        }
        if save && form.id.is_none_or(|id| self.profile_is_clean(id)) {
            form.saving = true;
            form.feedback = Some((
                FeedbackKind::Info,
                "Testing connection before saving…".into(),
            ));
            self.dispatch(
                Command::TestProfile(form.input()),
                "Testing connection",
                None,
                None,
                RequestKind::Form,
            );
        }
        if delete
            && let Some(id) = form.id
            && self.profile_is_clean(id)
        {
            self.confirm_delete = Some(id);
        }
        if close && !busy {
            for meta in self.pending_requests.values_mut() {
                if meta.label == PASSWORD_LOAD {
                    meta.stale = true;
                }
            }
            self.queued.retain(|q| q.meta.label != PASSWORD_LOAD);
            return;
        }
        self.profile_form = Some(form);
    }
}
