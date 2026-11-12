//! The window and its render loop. `State` holds everything; input calls
//! `App::update`, which changes the state and schedules one `render` that
//! brings the widgets in line. Widgets that only change on demand (the
//! sidebar, tab strip, dialogs) rebuild when a signature of what they show
//! changes, so a render never takes the widget under the pointer away for
//! nothing.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gtk::glib;
use gtk::prelude::*;
use uuid::Uuid;

use crate::icons::Icon;
use crate::settings::{self, Settings};
use crate::state::{Effect, State, TabKind};
use crate::ui;
use crate::{dialogs, query, sidebar, table, tabs};

pub const DEFAULT_SIZE: (i32, i32) = (1280, 800);
const SIDEBAR_DEFAULT: i32 = 260;

pub enum View {
    Query(Rc<query::QueryView>),
    Table(Rc<table::TableView>),
}

pub struct Shell {
    pub paned: gtk::Paned,
    pub sidebar: sidebar::Sidebar,
    pub collapsed_sidebar: gtk::Box,
    pub topbar: Topbar,
    pub banner: gtk::Revealer,
    pub banner_label: gtk::Label,
    pub tabs: tabs::TabStrip,
    pub stack: gtk::Stack,
    pub welcome: tabs::Welcome,
    pub toast: gtk::Box,
    pub toast_dot: gtk::Box,
    pub toast_label: gtk::Label,
    pub modal: gtk::Box,
}

pub struct Topbar {
    pub root: gtk::Box,
    pub identity: gtk::Box,
    pub dot: gtk::Box,
    pub name: gtk::Label,
    pub target: gtk::Label,
    pub read_only: gtk::Label,
    pub none: gtk::Label,
    pub spinner: gtk::Spinner,
    pub busy: gtk::Label,
    pub demo: gtk::Label,
    pub updates: gtk::Box,
    pub updates_signature: RefCell<String>,
}

pub struct Inner {
    pub state: RefCell<State>,
    pub settings: RefCell<Settings>,
    pub window: gtk::ApplicationWindow,
    pub shell: Shell,
    pub views: RefCell<HashMap<u64, View>>,
    rendering: Cell<bool>,
    render_queued: Cell<bool>,
    started: Instant,
    /// The next scheduled tick, in seconds since launch.
    timer: RefCell<Option<(f64, glib::SourceId)>>,
    css: gtk::CssProvider,
    css_signature: RefCell<String>,
    dialog_signature: RefCell<String>,
}

#[derive(Clone)]
pub struct App(pub Rc<Inner>);

impl std::ops::Deref for App {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl App {
    pub fn new(application: &gtk::Application, demo: bool) -> Self {
        let (wake_tx, wake_rx) = async_channel::unbounded::<()>();
        let wake: dbm_desktop::Wake = std::sync::Arc::new(move || {
            let _ = wake_tx.try_send(());
        });
        let settings = Settings::load(demo);
        let mut state = State::new(wake, demo);
        state.sidebar_collapsed = settings.get(settings::SIDEBAR_COLLAPSED) == Some("true");
        if !demo {
            let acknowledged = settings
                .get(settings::WHATS_NEW)
                .and_then(|value| value.parse().ok());
            state.updates.start_whats_new(acknowledged);
        }
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title(if demo { "Anybase — DEMO" } else { "Anybase" })
            .icon_name("dbm")
            .build();
        let (size, maximized) = settings.window();
        let (width, height) = size
            .filter(|(w, h)| *w >= 900 && *h >= 600)
            .unwrap_or(DEFAULT_SIZE);
        window.set_default_size(width, height);
        if maximized {
            window.maximize();
        }
        state.window_width = width;
        let shell = build_shell();
        window.set_child(Some(&shell_root(&shell)));
        let sidebar_width = settings
            .get(settings::SIDEBAR_WIDTH)
            .and_then(|w| w.parse().ok())
            .unwrap_or(SIDEBAR_DEFAULT)
            .clamp(220, 480);
        shell.paned.set_position(sidebar_width);
        let css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &gtk::prelude::WidgetExt::display(&window),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
        let app = Self(Rc::new(Inner {
            state: RefCell::new(state),
            settings: RefCell::new(settings),
            window,
            shell,
            views: RefCell::new(HashMap::new()),
            rendering: Cell::new(false),
            render_queued: Cell::new(false),
            started: Instant::now(),
            timer: RefCell::new(None),
            css,
            css_signature: RefCell::new(String::new()),
            dialog_signature: RefCell::new(String::new()),
        }));
        app.connect();
        let weak = Rc::downgrade(&app.0);
        glib::MainContext::default().spawn_local(async move {
            while wake_rx.recv().await.is_ok() {
                while wake_rx.try_recv().is_ok() {}
                let Some(inner) = weak.upgrade() else { break };
                App(inner).on_wake();
            }
        });
        app.on_wake();
        app
    }

    pub fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Changes the state and renders. Ignored while rendering, so setting a
    /// widget's value never feeds back into the state.
    pub fn update(&self, change: impl FnOnce(&mut State)) {
        if self.quiet(change) {
            self.queue_render();
        }
    }

    /// Changes the state without rendering, for text typed into a field
    /// that already shows it. Returns whether the change ran.
    pub fn quiet(&self, change: impl FnOnce(&mut State)) -> bool {
        if self.rendering.get() {
            return false;
        }
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return false;
        };
        state.now = self.now();
        change(&mut state);
        let effects = std::mem::take(&mut state.effects);
        drop(state);
        for effect in effects {
            self.perform(effect);
        }
        true
    }

    pub fn queue_render(&self) {
        if self.render_queued.replace(true) {
            return;
        }
        let app = self.clone();
        glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
            app.render_queued.set(false);
            app.render();
            glib::ControlFlow::Break
        });
    }

    fn on_wake(&self) {
        let next_check = self.quiet_with(|state, now| {
            state.drain();
            if state.demo {
                None
            } else {
                state.updates.poll(now)
            }
        });
        if let Some(delay) = next_check.flatten() {
            self.schedule(self.now() + delay.as_secs_f64());
        }
        self.queue_render();
    }

    fn quiet_with<R>(&self, change: impl FnOnce(&mut State, f64) -> R) -> Option<R> {
        let now = self.now();
        let mut result = None;
        self.quiet(|state| result = Some(change(state, now)));
        result
    }

    /// Wakes the app at `at` (seconds since launch) to expire messages, poll
    /// updates, or show progress.
    pub fn schedule(&self, at: f64) {
        let mut timer = self.timer.borrow_mut();
        if timer.as_ref().is_some_and(|(when, _)| *when <= at) {
            return;
        }
        if let Some((_, source)) = timer.take() {
            source.remove();
        }
        let delay = (at - self.now()).max(0.01);
        let app = self.clone();
        let source =
            glib::timeout_add_local_once(std::time::Duration::from_secs_f64(delay), move || {
                app.timer.borrow_mut().take();
                app.on_wake();
            });
        *timer = Some((at, source));
    }

    fn perform(&self, effect: Effect) {
        match effect {
            Effect::Copy(text) => self.window.clipboard().set_text(&text),
            Effect::PickExport { tab, file_name } => {
                let app = self.clone();
                save_dialog(&self.window, &file_name, move |path| {
                    app.update(|state| state.export_chosen(tab, path));
                });
            }
            Effect::SaveResult {
                tab,
                file_name,
                columns,
                rows,
            } => {
                let app = self.clone();
                save_dialog(&self.window, &file_name, move |path| {
                    let Some(path) = path else {
                        app.update(|state| state.result_saved(tab, Ok(None)));
                        return;
                    };
                    glib::MainContext::default().spawn_local(async move {
                        let written = gtk::gio::spawn_blocking(move || {
                            dbm_core::export::export_result_csv(&path, &columns, &rows)
                                .map(|rows| Some((path, rows)))
                        })
                        .await
                        .unwrap_or_else(|_| Err("The CSV writer stopped.".into()));
                        app.update(|state| state.result_saved(tab, written));
                    });
                });
            }
            Effect::SetSql(tab) => {
                if let Some(View::Query(view)) = self.views.borrow().get(&tab) {
                    let state = self.state.borrow();
                    if let Some(tab) = state.tab(tab) {
                        view.set_text(&tab.sql);
                    }
                }
            }
        }
    }

    pub fn present(&self) {
        self.window.present();
    }

    fn connect(&self) {
        let app = self.clone();
        self.window.connect_close_request(move |_| {
            let mut close = true;
            app.update(|state| close = state.close_requested());
            if close {
                app.save_settings();
                glib::Propagation::Proceed
            } else {
                glib::Propagation::Stop
            }
        });
        let app = self.clone();
        self.window.connect_default_width_notify(move |window| {
            let width = window.width().max(window.default_width());
            app.quiet(|state| state.window_width = width);
        });
        let app = self.clone();
        self.shell.paned.connect_position_notify(move |paned| {
            let position = paned.position();
            if !(220..=480).contains(&position) && !app.state.borrow().sidebar_collapsed {
                paned.set_position(position.clamp(220, 480));
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let app = self.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| app.shortcut(key, modifiers));
        self.window.add_controller(keys);
        // Top bar and banner controls.
        let app = self.clone();
        dismiss_button(&self.shell.banner, move || {
            app.update(|state| state.banner = None);
        });
        let app = self.clone();
        let dismiss = ui::button("dismiss", Some(Icon::Close), "", "Dismiss notification");
        dismiss.connect_clicked(move |_| app.update(|state| state.toast = None));
        self.shell.toast.append(&dismiss);
        sidebar::connect(self);
        let app = self.clone();
        let expand = ui::button("toolbar-button", Some(Icon::Sidebar), "", "Expand sidebar");
        expand.connect_clicked(move |_| app.update(|state| state.sidebar_collapsed = false));
        self.shell.collapsed_sidebar.append(&expand);
    }

    /// New connection, new query, and close tab (Ctrl+N/T/W), taken before
    /// any widget sees them; Escape closes dialogs.
    fn shortcut(&self, key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> glib::Propagation {
        let control = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let modal = self.state.borrow().modal_open();
        if key == gtk::gdk::Key::Escape && modal {
            self.update(dialogs::dismiss);
            return glib::Propagation::Stop;
        }
        if !control || modal {
            return glib::Propagation::Proceed;
        }
        let lower = key.to_lower();
        if lower == gtk::gdk::Key::n {
            self.update(|state| state.profile_form = Some(crate::state::ProfileForm::fresh()));
        } else if lower == gtk::gdk::Key::t {
            self.update(|state| {
                if let Some(profile) = state
                    .active_profile
                    .filter(|p| state.workspaces.contains_key(p))
                {
                    state.open_query(profile, true);
                }
            });
        } else if lower == gtk::gdk::Key::w {
            self.update(|state| {
                if let Some(tab) = state.active_tab {
                    state.close_tab(tab);
                }
            });
        } else {
            return glib::Propagation::Proceed;
        }
        glib::Propagation::Stop
    }

    pub fn save_settings(&self) {
        let collapsed = self.state.borrow().sidebar_collapsed;
        let mut settings = self.settings.borrow_mut();
        settings.set(settings::SIDEBAR_COLLAPSED, collapsed.to_string());
        if !collapsed {
            settings.set(
                settings::SIDEBAR_WIDTH,
                self.shell.paned.position().to_string(),
            );
        }
        let maximized = self.window.is_maximized();
        let size = (self.window.default_width(), self.window.default_height());
        settings.set_window(size, maximized);
        settings.save();
    }

    // ---- rendering ----------------------------------------------------

    pub fn render(&self) {
        if self.rendering.replace(true) {
            return;
        }
        let next = {
            let mut state = self.state.borrow_mut();
            state.now = self.now();
            let next = state.expire_messages();
            // The query tab's history panel loads on first show, as in egui.
            let query_profile = state
                .active_tab
                .and_then(|id| state.tabs.iter().find(|t| t.id == id))
                .filter(|t| t.kind == TabKind::Query)
                .map(|t| t.profile_id);
            if let Some(profile) = query_profile {
                state.ensure_history(profile);
            }
            // Stored at once: installing an update exits without a save.
            if let Some(build) = state.updates.take_acknowledged() {
                let mut settings = self.settings.borrow_mut();
                settings.set(settings::WHATS_NEW, build.to_string());
                settings.save();
            }
            next
        };
        let state = self.state.borrow();
        self.render_css(&state);
        self.render_sidebar_mode(&state);
        sidebar::render(self, &state);
        self.render_topbar(&state);
        self.render_banner(&state);
        self.shell.tabs.render(self, &state);
        self.render_content(&state);
        self.render_toast(&state);
        self.render_dialog(&state);
        let busy = self.ticks_needed(&state);
        drop(state);
        self.rendering.set(false);
        if let Some(at) = next {
            self.schedule(at);
        }
        if busy {
            self.schedule(self.now() + 0.2);
        }
    }

    /// Exports report progress and slow table loads fade in an overlay, so
    /// both need renders without other events.
    fn ticks_needed(&self, state: &State) -> bool {
        state.tables.values().any(|table| {
            table.export_progress.is_some()
                || table
                    .loading_since
                    .is_some_and(|since| table.loading && state.now - since < 0.5)
        })
    }

    fn render_css(&self, state: &State) {
        // Every palette swatch and the form's custom color need a class too.
        let colors: Vec<String> = state
            .profiles
            .iter()
            .map(|p| state.profile_color(p.id))
            .chain(
                crate::state::CONNECTION_COLORS
                    .iter()
                    .map(|c| (*c).to_owned()),
            )
            .chain(state.profile_form.as_ref().map(|f| f.color.clone()))
            .collect();
        let signature = colors.join(",");
        if *self.css_signature.borrow() != signature {
            self.css
                .load_from_string(&ui::connection_css(colors.iter().map(String::as_str)));
            *self.css_signature.borrow_mut() = signature;
        }
    }

    fn render_sidebar_mode(&self, state: &State) {
        let paned = &self.shell.paned;
        let collapsed = state.sidebar_collapsed;
        let showing_collapsed = paned
            .start_child()
            .is_some_and(|child| child == self.shell.collapsed_sidebar);
        if collapsed && !showing_collapsed {
            self.settings.borrow_mut().set(
                settings::SIDEBAR_WIDTH,
                paned.position().clamp(220, 480).to_string(),
            );
            paned.set_start_child(Some(&self.shell.collapsed_sidebar));
            paned.set_resize_start_child(false);
            paned.set_shrink_start_child(false);
            paned.set_position(48);
        } else if !collapsed && showing_collapsed {
            paned.set_start_child(Some(&self.shell.sidebar.root));
            let width = self
                .settings
                .borrow()
                .get(settings::SIDEBAR_WIDTH)
                .and_then(|w| w.parse().ok())
                .unwrap_or(SIDEBAR_DEFAULT);
            paned.set_position(width);
        }
    }

    fn render_topbar(&self, state: &State) {
        let bar = &self.shell.topbar;
        let profile = state
            .active_tab
            .and_then(|id| state.tab(id))
            .map(|t| t.profile_id)
            .or(state.active_profile)
            .and_then(|id| state.profile(id));
        bar.identity.set_visible(profile.is_some());
        bar.none.set_visible(profile.is_none());
        if let Some(profile) = profile {
            set_color_class(&bar.dot, &state.profile_color(profile.id));
            set_text(&bar.name, &profile.name);
            let target = if profile.username.is_empty() {
                format!(
                    "{}:{}/{}",
                    profile.host, profile.port, profile.default_database
                )
            } else {
                format!(
                    "{}@{}:{}/{}",
                    profile.username, profile.host, profile.port, profile.default_database
                )
            };
            set_text(&bar.target, &target);
            bar.read_only.set_visible(profile.read_only);
        }
        let pending = state.pending_requests.values().next();
        bar.spinner.set_visible(pending.is_some());
        bar.spinner.set_spinning(pending.is_some());
        bar.busy.set_visible(pending.is_some());
        if let Some(meta) = pending {
            set_text(&bar.busy, meta.label);
        }
        bar.demo.set_visible(state.demo);
        self.render_updates(state);
    }

    /// The update control, only on channel builds.
    fn render_updates(&self, state: &State) {
        use dbm_desktop::updates::State as Update;
        let bar = &self.shell.topbar;
        let updates = &state.updates;
        let enabled = updates.enabled() && !state.demo;
        let signature = if enabled {
            format!(
                "{}|{}|{}",
                updates.label(),
                updates.detail(),
                updates.version_label()
            )
        } else {
            String::new()
        };
        if *bar.updates_signature.borrow() == signature {
            return;
        }
        *bar.updates_signature.borrow_mut() = signature;
        ui::clear(&bar.updates);
        bar.updates.set_visible(enabled);
        if !enabled {
            return;
        }
        let column = ui::vbox(0);
        column.set_valign(gtk::Align::Center);
        let label = updates.label();
        let detail = updates.detail();
        match &updates.state {
            Update::Available(update) => {
                let button = ui::button("accent-button", None, &label, &detail);
                button.add_css_class("small");
                let app = self.clone();
                let update = update.clone();
                button.connect_clicked(move |_| {
                    let update = update.clone();
                    app.update(move |state| state.confirm_update = Some(update));
                });
                column.append(&button);
            }
            Update::Failed(_) => {
                let button = ui::button("secondary-button", None, &label, &detail);
                button.add_css_class("small");
                let app = self.clone();
                // Shows the error with Try again and the download fallback.
                button.connect_clicked(move |_| {
                    app.update(|state| state.updates.failure_open = true)
                });
                column.append(&button);
            }
            Update::Idle => {}
            _ => column.append(&ui::label(&label, &["size-11", "muted"])),
        }
        let version = ui::label(&updates.version_label(), &["update-version"]);
        version.set_xalign(1.0);
        version.set_tooltip_text(Some(&detail));
        // Selectable, so the version can be copied into bug reports.
        version.set_selectable(true);
        column.append(&version);
        let busy = matches!(updates.state, Update::Checking | Update::Installing);
        let app = self.clone();
        let help = ui::menu_button(
            "toolbar-button",
            &gtk::Label::new(Some("Help")),
            "Help",
            move || {
                let whats_new = app.clone();
                let check = app.clone();
                let mut items: Vec<ui::MenuItem> = vec![(
                    "What’s new".into(),
                    false,
                    Box::new(move || whats_new.update(|state| state.updates.open_whats_new())),
                )];
                if !busy {
                    items.push((
                        "Check for updates…".into(),
                        false,
                        Box::new(move || check.update(|state| state.updates.check(false))),
                    ));
                }
                let download = app.clone();
                items.push((
                    "Download the latest version…".into(),
                    false,
                    Box::new(move || crate::dialogs::open_download_page(&download)),
                ));
                items
            },
            200,
        );
        bar.updates.append(&column);
        bar.updates.append(&help);
    }

    fn render_banner(&self, state: &State) {
        let shell = &self.shell;
        shell.banner.set_reveal_child(state.banner.is_some());
        if let Some(banner) = &state.banner {
            set_text(&shell.banner_label, &banner.text);
        }
    }

    fn render_toast(&self, state: &State) {
        let shell = &self.shell;
        shell.toast.set_visible(state.toast.is_some());
        if let Some(toast) = &state.toast {
            set_text(&shell.toast_label, &toast.message.text);
            let color = if toast.success { "#5ad394" } else { "#4c9aff" };
            set_color_class(&shell.toast_dot, color);
        }
    }

    fn render_dialog(&self, state: &State) {
        let signature = dialogs::signature(state);
        if *self.dialog_signature.borrow() != signature {
            *self.dialog_signature.borrow_mut() = signature;
            ui::clear(&self.shell.modal);
            if let Some(dialog) = dialogs::build(self, state) {
                self.shell.modal.append(&dialog);
            }
        }
        dialogs::refresh(self, state);
        self.shell
            .modal
            .set_visible(self.shell.modal.first_child().is_some());
    }

    fn render_content(&self, state: &State) {
        let stack = &self.shell.stack;
        // Views for closed tabs go; views for new tabs are created.
        let mut views = self.views.borrow_mut();
        views.retain(|id, view| {
            let keep = state.tab(*id).is_some();
            if !keep {
                let widget = match view {
                    View::Query(view) => view.root.clone().upcast::<gtk::Widget>(),
                    View::Table(view) => view.root.clone().upcast(),
                };
                if let View::Table(view) = view {
                    view.teardown();
                }
                if let View::Query(view) = view {
                    view.teardown();
                }
                stack.remove(&widget);
            }
            keep
        });
        for tab in &state.tabs {
            if views.contains_key(&tab.id) {
                continue;
            }
            let view = match &tab.kind {
                TabKind::Query => {
                    let view = query::QueryView::new(self, tab.id, state.engine(tab.profile_id));
                    view.set_text(&tab.sql);
                    stack.add_named(&view.root, Some(&tab.id.to_string()));
                    View::Query(view)
                }
                TabKind::Table { .. } => {
                    let view = table::TableView::new(self, tab.id, false);
                    stack.add_named(&view.root, Some(&tab.id.to_string()));
                    View::Table(view)
                }
            };
            views.insert(tab.id, view);
        }
        match state
            .active_tab
            .and_then(|id| views.get(&id).map(|v| (id, v)))
        {
            Some((id, view)) => {
                match view {
                    View::Query(view) => view.refresh(self, state),
                    View::Table(view) => view.refresh(self, state),
                }
                let name = id.to_string();
                if stack.visible_child_name().as_deref() != Some(name.as_str()) {
                    stack.set_visible_child_name(&name);
                }
            }
            None => {
                self.shell.welcome.refresh(state);
                if stack.visible_child_name().as_deref() != Some("welcome") {
                    stack.set_visible_child_name("welcome");
                }
            }
        }
    }

    pub fn profile_menu(&self, id: Uuid, connected: bool) -> Vec<ui::MenuItem> {
        let edit = self.clone();
        let mut items: Vec<ui::MenuItem> = vec![(
            "Edit connection".into(),
            false,
            Box::new(move || edit.update(|state| state.edit_profile(id))),
        )];
        if connected {
            let disconnect = self.clone();
            items.push((
                "Disconnect".into(),
                true,
                Box::new(move || disconnect.update(|state| state.disconnect(id))),
            ));
        }
        items
    }
}

pub fn set_text(label: &gtk::Label, text: &str) {
    if label.text() != text {
        label.set_text(text);
    }
}

/// Swaps a widget's connection color class.
pub fn set_color_class(widget: &impl IsA<gtk::Widget>, color: &str) {
    let class = ui::color_class(color);
    for existing in widget.css_classes() {
        if existing.starts_with("c-") && existing != class {
            widget.remove_css_class(&existing);
        }
    }
    widget.add_css_class(&class);
}

fn dismiss_button(revealer: &gtk::Revealer, action: impl Fn() + 'static) {
    if let Some(content) = revealer.child().and_then(|c| c.downcast::<gtk::Box>().ok()) {
        let button = ui::button("dismiss", Some(Icon::Close), "", "Dismiss error");
        button.connect_clicked(move |_| action());
        content.append(&button);
    }
}

/// The desktop save dialog (through the portal when one runs), with a CSV
/// filter. `done` receives the chosen path, or `None` when canceled.
fn save_dialog(
    window: &gtk::ApplicationWindow,
    file_name: &str,
    done: impl FnOnce(Option<std::path::PathBuf>) + 'static,
) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("CSV"));
    filter.add_pattern("*.csv");
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title("Export CSV")
        .initial_name(file_name)
        .filters(&filters)
        .default_filter(&filter)
        .modal(true)
        .build();
    dialog.save(
        Some(window),
        None::<&gtk::gio::Cancellable>,
        move |result| {
            done(result.ok().and_then(|file| file.path()));
        },
    );
}

/// Opens an exported file with its default app, or shows it in its folder.
pub fn open_path(window: &gtk::ApplicationWindow, path: &std::path::Path, reveal: bool) {
    let launcher = gtk::FileLauncher::new(Some(&gtk::gio::File::for_path(path)));
    if reveal {
        launcher.open_containing_folder(Some(window), None::<&gtk::gio::Cancellable>, |_| {});
    } else {
        launcher.launch(Some(window), None::<&gtk::gio::Cancellable>, |_| {});
    }
}

fn build_shell() -> Shell {
    let topbar = build_topbar();
    let banner_label = ui::label("", &[]);
    banner_label.set_hexpand(true);
    banner_label.set_xalign(0.5);
    banner_label.set_wrap(true);
    let banner_box = ui::hbox(8);
    banner_box.add_css_class("banner");
    banner_box.append(&banner_label);
    let banner = gtk::Revealer::new();
    banner.set_child(Some(&banner_box));
    banner.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    banner.set_transition_duration(150);
    let tabs = tabs::TabStrip::new();
    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_hexpand(true);
    stack.add_css_class("canvas");
    let welcome = tabs::Welcome::new();
    stack.add_named(&welcome.root, Some("welcome"));
    let sidebar = sidebar::Sidebar::new();
    let collapsed_sidebar = ui::vbox(0);
    collapsed_sidebar.add_css_class("sidebar");
    collapsed_sidebar.set_size_request(48, -1);
    collapsed_sidebar.set_margin_top(0);
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&sidebar.root));
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);
    paned.set_shrink_end_child(false);
    let main = ui::vbox(0);
    main.append(&topbar.root);
    main.append(&banner);
    main.append(&tabs.root);
    main.append(&stack);
    paned.set_end_child(Some(&main));
    let toast_dot = ui::dot("#4c9aff", true, 8);
    let toast_label = ui::label("", &["size-12-5"]);
    toast_label.set_wrap(true);
    toast_label.set_max_width_chars(60);
    let toast = ui::hbox(12);
    toast.add_css_class("toast");
    toast.append(&toast_dot);
    toast.append(&toast_label);
    toast.set_halign(gtk::Align::End);
    toast.set_valign(gtk::Align::End);
    toast.set_visible(false);
    let modal = ui::vbox(0);
    modal.add_css_class("modal-backdrop");
    modal.set_visible(false);
    Shell {
        paned,
        sidebar,
        collapsed_sidebar,
        topbar,
        banner,
        banner_label,
        tabs,
        stack,
        welcome,
        toast,
        toast_dot,
        toast_label,
        modal,
    }
}

fn shell_root(shell: &Shell) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("app");
    overlay.set_size_request(900, 600);
    overlay.set_child(Some(&shell.paned));
    overlay.add_overlay(&shell.toast);
    overlay.add_overlay(&shell.modal);
    overlay
}

fn build_topbar() -> Topbar {
    let root = ui::hbox(9);
    root.add_css_class("topbar");
    let dot = ui::dot("#4c9aff", true, 8);
    let name = ui::ellipsized("", &["bold", "strong"]);
    let target = ui::ellipsized("", &["size-11", "faint"]);
    target.set_hexpand(true);
    let read_only = ui::chip("Read-only", "muted");
    let identity = ui::hbox(9);
    identity.set_hexpand(true);
    identity.append(&dot);
    identity.append(&name);
    identity.append(&target);
    identity.append(&read_only);
    let none = ui::label("No active connection", &["muted"]);
    none.set_hexpand(true);
    let spinner = gtk::Spinner::new();
    let busy = ui::label("", &["muted"]);
    let demo = gtk::Label::new(Some("Demo fixture · not a live connection"));
    demo.add_css_class("demo-chip");
    demo.set_valign(gtk::Align::Center);
    demo.set_tooltip_text(Some(
        "Isolated deterministic data; profiles and edits are not persisted.",
    ));
    let updates = ui::hbox(8);
    root.append(&identity);
    root.append(&none);
    root.append(&spinner);
    root.append(&busy);
    root.append(&demo);
    root.append(&updates);
    Topbar {
        root,
        identity,
        dot,
        name,
        target,
        read_only,
        none,
        spinner,
        busy,
        demo,
        updates,
        updates_signature: RefCell::new("unset".into()),
    }
}
