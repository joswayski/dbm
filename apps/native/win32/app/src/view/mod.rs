//! Draws the workbench each frame: sidebar, top bar, error strip, tab
//! strip, the active tab, dialogs and the toast, in the Graphite layout the
//! egui and AppKit apps share.

mod chrome;
mod dialogs;
pub mod grid;
mod query;
mod sidebar;
mod table;

use dbm_ui::input::{Key, Modifiers};
use dbm_ui::theme;
use dbm_ui::{Rect, Ui};

use crate::messages;
use crate::state::{ProfileForm, Workbench};
use crate::store::{self, Store};

pub struct View {
    pub app: Workbench,
    pub store: Store,
    saved: (bool, f32),
}

impl View {
    pub fn new(app: Workbench, store: Store) -> Self {
        let mut view = View {
            app,
            store,
            saved: (false, 260.0),
        };
        view.app.sidebar_collapsed = view.store.get(store::SIDEBAR_COLLAPSED) == Some("true");
        if let Some(w) = view
            .store
            .get(store::SIDEBAR_WIDTH)
            .and_then(|v| v.parse::<f32>().ok())
        {
            view.app.sidebar_width = w.clamp(220.0, 480.0);
        }
        view.saved = (view.app.sidebar_collapsed, view.app.sidebar_width);
        if !view.app.demo {
            let acknowledged = view
                .store
                .get(store::WHATS_NEW)
                .and_then(|v| v.parse().ok());
            view.app.updates.start_whats_new(acknowledged);
        }
        view
    }

    pub fn update(&mut self, ui: &mut Ui) {
        let app = &mut self.app;
        app.now = ui.ctx.time();
        let screen = ui.ctx.screen_rect();
        app.window_width = screen.width();
        if !app.demo
            && let Some(next) = app.updates.poll(app.now)
        {
            ui.ctx.request_repaint_after(next.as_secs_f64());
        }
        app.drain();
        if ui.ctx.close_requested()
            && let Some(blocker) = app.close_blocker()
        {
            ui.ctx.cancel_close();
            app.show_error(blocker);
        }
        if !app.pending_requests.is_empty() {
            ui.ctx.request_repaint_after(0.25);
        }
        shortcuts(app, ui);
        ui.painter().rect_filled(screen, 0.0, theme::CHROME);
        let sidebar_w = if app.sidebar_collapsed {
            48.0
        } else {
            app.sidebar_width
        };
        let (side, main) = screen.split_left(sidebar_w);
        app.sidebar(ui, side);
        let (top, rest) = main.split_top(44.0);
        app.top_bar(ui, top);
        let rest = app.banner_ui(ui, rest);
        let (strip, content) = rest.split_top(36.0);
        app.tab_strip(ui, strip);
        ui.painter().rect_filled(content, 0.0, theme::BG);
        app.content(ui, content);
        app.dialogs(ui);
        toast(app, ui);
        self.persist();
    }

    /// Writes the settings file when something in it changed; the What's
    /// new acknowledgement is stored at once (an update exits without notice).
    fn persist(&mut self) {
        let mut dirty = false;
        let now = (self.app.sidebar_collapsed, self.app.sidebar_width);
        if now != self.saved {
            self.store.set(store::SIDEBAR_COLLAPSED, now.0.to_string());
            self.store
                .set(store::SIDEBAR_WIDTH, format!("{}", now.1.round()));
            self.saved = now;
            dirty = true;
        }
        if let Some(build) = self.app.updates.take_acknowledged() {
            self.store.set(store::WHATS_NEW, build.to_string());
            dirty = true;
        }
        if dirty {
            self.store.flush();
        }
    }
}

/// Ctrl+N, Ctrl+T and Ctrl+W, taken before any field sees them so Ctrl+W
/// closes the tab even while the editor has focus.
fn shortcuts(app: &mut Workbench, ui: &mut Ui) {
    if app.modal_open() {
        return;
    }
    if ui.ctx.consume_key(Modifiers::COMMAND, Key::N) {
        app.profile_form = Some(ProfileForm::fresh());
    }
    if ui.ctx.consume_key(Modifiers::COMMAND, Key::T)
        && let Some(profile) = app
            .active_profile
            .filter(|p| app.workspaces.contains_key(p))
    {
        app.open_query(profile, true);
    }
    if ui.ctx.consume_key(Modifiers::COMMAND, Key::W)
        && let Some(tab) = app.active_tab
    {
        app.close_tab(tab);
    }
}

fn toast(app: &mut Workbench, ui: &mut Ui) {
    let Some(t) = &app.toast else { return };
    let Some(opacity) = t.message.opacity(ui.ctx, app.now) else {
        app.toast = None;
        return;
    };
    let dot = if t.success {
        theme::SUCCESS
    } else {
        theme::ACCENT
    };
    let text = t.message.text.clone();
    if messages::toast(ui, &text, dot, opacity) {
        app.toast = None;
    }
}

impl Workbench {
    fn banner_ui(&mut self, ui: &mut Ui, rest: Rect) -> Rect {
        let Some(banner) = self.banner.clone() else {
            return rest;
        };
        let Some(opacity) = banner.opacity(ui.ctx, self.now) else {
            self.banner = None;
            return rest;
        };
        let (h, response) = {
            let mut child = ui.child(rest, dbm_ui::Dir::Down);
            messages::banner(&mut child, rest, &banner, opacity)
        };
        if response == messages::Response::Dismiss {
            self.banner = None;
        }
        rest.split_top(h).1
    }

    fn content(&mut self, ui: &mut Ui, rect: Rect) {
        let tab = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
            .map(|t| (t.id, t.kind.clone(), t.profile_id));
        let mut child = ui.child(rect, dbm_ui::Dir::Down);
        child.set_clip(rect);
        match tab {
            None => self.welcome(&mut child, rect),
            Some((id, crate::state::TabKind::Query, _)) => self.query_ui(&mut child, rect, id),
            Some((id, crate::state::TabKind::Table { .. }, profile)) => {
                self.table_ui(&mut child, rect, id, profile, false)
            }
        }
    }

    fn welcome(&mut self, ui: &mut Ui, rect: Rect) {
        use dbm_ui::icons::{self, Icon};
        use dbm_ui::{Color32, Stroke, Vec2, pos2, vec2};
        let profile = self.active_profile.and_then(|id| self.profile(id).cloned());
        let connected = profile
            .as_ref()
            .is_some_and(|p| self.workspaces.contains_key(&p.id));
        let title = profile
            .as_ref()
            .map_or("No connection selected", |p| p.name.as_str())
            .to_owned();
        let message = match (&profile, connected) {
            (Some(_), true) => {
                "Choose a table from the sidebar or open a new query with the plus button above."
            }
            (Some(_), false) => {
                "This connection is selected but not connected. Select it again to connect."
            }
            (None, _) if self.profiles.is_empty() => {
                "Create a connection from the sidebar to get started."
            }
            (None, _) => "Select a saved connection from the sidebar to browse its data.",
        };
        let width = (rect.width() - 80.0).clamp(200.0, 520.0);
        let column = Rect::from_min_size(
            pos2(
                rect.center().x - width / 2.0,
                rect.top() + rect.height() * 0.18,
            ),
            vec2(width, rect.height()),
        );
        let mut c = ui.child(column, dbm_ui::Dir::Down);
        c.set_centered(true);
        c.spacing.y = 0.0;
        let tile = c.allocate(Vec2::splat(64.0));
        {
            let mut p = c.painter();
            p.rect(
                tile,
                16.0,
                Color32::from_rgb(0x27, 0x27, 0x2a),
                Stroke::new(1.0, Color32::from_white_alpha(18)),
            );
            icons::paint(
                &mut p,
                Rect::from_center_size(tile.center(), Vec2::splat(28.0)),
                Icon::Database,
                theme::ACCENT_TEXT,
            );
        }
        c.add_space(22.0);
        let font = theme::bold(20.0);
        let w = c.ctx.fonts.width(&title, font).min(width);
        c.allocate(vec2(w, 0.0));
        c.label_wrapped(&title, font, theme::TEXT_STRONG, true);
        c.add_space(8.0);
        c.label_wrapped(message, theme::ui(13.0), theme::MUTED, true);
    }
}
