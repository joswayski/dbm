//! The sidebar: saved connections, the active connection's database picker,
//! schema filter and tree, the New connection button, collapse, and the
//! resize edge (220-480 px, double click restores 260, arrows step 10).

use dbm_core::models::{ConnectionProfile, DatabaseEngine, SchemaNode};
use dbm_desktop::worker::Command;
use dbm_core::sql_text::filter_schema_nodes;
use dbm_ui::a11y::{Node, Role};
use dbm_ui::icons::{self, Icon};
use dbm_ui::input::{Key, Modifiers};
use dbm_ui::text_edit::TextEdit;
use dbm_ui::widgets::{self, icon_button, icon_button_in, secondary_button};
use dbm_ui::{Color32, CursorIcon, Dir, Id, Rect, ScrollArea, Sense, Stroke, Ui, Vec2, pos2, theme, vec2};
use uuid::Uuid;

use crate::state::{PASSWORD_LOAD, ProfileForm, RequestKind, Workbench, engine_label};

pub fn section_label(ui: &mut Ui, text: &str) -> Rect {
    ui.label(text, theme::bold(11.0), theme::MUTED)
}

/// An 18 px row with a section label on the left; `right` fills it from the
/// right edge.
fn section_row(ui: &mut Ui, text: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(24.0, |ui| {
        section_label(ui, text);
        ui.right_to_left(right);
    });
}

impl Workbench {
    pub(super) fn sidebar(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, theme::SIDEBAR);
        ui.a11y_group(Id::new("sidebar"), Role::Group, "Connections", rect, |ui| {
            if self.sidebar_collapsed {
                let mut c = ui.child(rect.shrink2(vec2(8.0, 12.0)), Dir::Down);
                c.set_centered(true);
                if icon_button(&mut c, Icon::Sidebar, None, "Expand sidebar").clicked {
                    self.sidebar_collapsed = false;
                }
                return;
            }
            self.sidebar_expanded(ui, rect);
        });
        if !self.sidebar_collapsed {
            self.sidebar_resize(ui, rect);
        } else {
            ui.painter().vline(rect.right() - 0.5, rect.top()..=rect.bottom(), Stroke::new(1.0, theme::BORDER));
        }
    }

    fn sidebar_expanded(&mut self, ui: &mut Ui, rect: Rect) {
        let inner = rect.shrink(10.0);
        // `.sidebar-footer`: a hairline above a full-width New connection.
        let (body, footer) = inner.split_bottom(40.0);
        ui.painter().hline(rect.left()..=rect.right(), footer.top() + 0.5, Stroke::new(1.0, theme::BORDER));
        {
            let mut f = ui.child(footer.inset(2.0, 10.0, 2.0, 0.0), Dir::Down);
            let w = f.available_width();
            if secondary_button("New connection").icon(Icon::Plus).min_width(w).show(&mut f).clicked {
                self.profile_form = Some(ProfileForm::fresh());
            }
        }
        let mut c = ui.child(body, Dir::Down);
        c.set_clip(body.expand(2.0).intersect(rect));
        section_row(&mut c, "Connections", |ui| {
            if icon_button(ui, Icon::Sidebar, None, "Collapse sidebar").clicked {
                self.sidebar_collapsed = true;
            }
        });
        c.add_space(4.0);
        ScrollArea::vertical(Id::new("sidebar-scroll")).show(&mut c, |ui| {
            ui.spacing.y = 2.0;
            if self.profiles.is_empty() {
                ui.add_space(18.0);
                ui.set_centered(true);
                ui.label("No saved connections.", theme::ui(12.0), theme::FAINT);
            }
            for profile in self.profiles.clone() {
                ui.scope(|ui| {
                    ui.push_id(profile.id);
                    self.connection_item(ui, &profile);
                });
            }
        });
    }

    /// The draggable right edge, with the 2 px accent line while hovered,
    /// dragged or focused.
    fn sidebar_resize(&mut self, ui: &mut Ui, rect: Rect) {
        let edge = Rect::from_ltrb(rect.right() - 3.0, rect.top(), rect.right() + 3.0, rect.bottom());
        let id = Id::new("sidebar-resize");
        let sense = Sense { focusable: true, ..Sense::DRAG };
        let r = ui.interact(edge, id, sense);
        let mut node = Node::new(id.0, Role::Separator, "Resize sidebar", edge);
        node.focusable = true;
        node.value = Some(format!("{}", self.sidebar_width.round()));
        ui.a11y(node);
        if r.hovered || r.dragged {
            ui.ctx.set_cursor(CursorIcon::ResizeHorizontal);
        }
        if r.pressed {
            ui.ctx.close_popup();
            ui.ctx.request_focus(id);
        }
        if r.dragged {
            if let Some(p) = ui.ctx.pointer() {
                self.sidebar_width = (p.x - rect.left()).clamp(220.0, 480.0);
            }
        }
        if r.double_clicked {
            self.sidebar_width = 260.0;
        }
        if r.has_focus {
            let right = ui.ctx.consume_key(Modifiers::NONE, Key::ArrowRight);
            let left = ui.ctx.consume_key(Modifiers::NONE, Key::ArrowLeft);
            let delta = f32::from(i8::from(right) - i8::from(left)) * 10.0;
            if delta != 0.0 {
                ui.ctx.close_popup();
                self.sidebar_width = (self.sidebar_width + delta).clamp(220.0, 480.0);
            }
        }
        let mut p = ui.painter();
        if r.hovered || r.dragged || r.has_focus {
            p.rect_filled(Rect::from_ltrb(rect.right() - 1.0, rect.top(), rect.right() + 1.0, rect.bottom()), 0.0, theme::ACCENT);
        } else {
            p.vline(rect.right() - 0.5, rect.top()..=rect.bottom(), Stroke::new(1.0, theme::BORDER));
        }
    }

    fn connection_item(&mut self, ui: &mut Ui, profile: &ConnectionProfile) {
        let id = profile.id;
        let connected = self.workspaces.contains_key(&id);
        let active = self.active_profile == Some(id);
        let expanded = active && connected && !self.collapsed_profiles.contains(&id);
        let color = self.profile_color(id);
        let connecting = self.connecting(id);
        let rect = ui.allocate_row(44.0);
        let rid = ui.make_id("connection");
        let response = ui.interact(rect, rid, Sense::CLICK);
        let mut node = Node::new(rid.0, Role::Button, &profile.name, rect);
        node.focusable = true;
        node.selected = Some(active);
        ui.a11y(node);
        let actions_width = if connected && active { 60.0 } else { 34.0 };
        let chip_width = if profile.read_only { 68.0 } else { 0.0 };
        let text_left = rect.left() + 26.0;
        let text_width = (rect.right() - actions_width - chip_width - text_left).max(40.0);
        let target = if profile.username.is_empty() { profile.host.clone() } else { format!("{}@{}", profile.username, profile.host) };
        let subtitle = format!("{} · {target}", engine_label(profile.engine));
        let elided = {
            let mut p = ui.painter();
            // `.connection-item`: 6% wash on hover, 7% when active.
            if active {
                p.rect_filled(rect, 6.0, Color32::from_white_alpha(18));
            } else if response.hovered {
                p.rect_filled(rect, 6.0, Color32::from_white_alpha(15));
            }
            let dot = pos2(rect.left() + 13.0, rect.center().y);
            if connected {
                p.circle_filled(dot, 4.0, color);
            } else {
                p.circle_stroke(dot, 3.5, Stroke::new(1.5, color));
            }
            let (name_font, sub_font) = (theme::ui(13.0), theme::ui(11.0));
            let a = p.text_truncated(text_left, rect.top() + 6.0 + name_font.line_height() / 2.0, &profile.name, name_font, theme::TEXT, text_width).1;
            let b = p.text_truncated(text_left, rect.top() + 24.0 + sub_font.line_height() / 2.0, &subtitle, sub_font, theme::FAINT, text_width).1;
            a || b
        };
        let _ = match (elided, connected) {
            (true, true) => response.clone().on_hover_text(format!("{}\n{subtitle}", profile.name)),
            (true, false) => response.clone().on_hover_text(format!("Connect to {}\n{subtitle}", profile.name)),
            (false, true) => response.clone(),
            (false, false) => response.clone().on_hover_text("Connect"),
        };
        if response.clicked {
            if connected {
                self.activate_profile(id);
            } else {
                self.active_profile = Some(id);
                if !connecting {
                    self.send(Command::Connect(id), "Connecting", Some(id));
                }
            }
        }
        let more_rect = Rect::from_min_size(pos2(rect.right() - 30.0, rect.top() + 1.0), vec2(28.0, 28.0));
        let more = icon_button_in(ui, more_rect, true, Icon::More, None, "Connection actions");
        let mut edit = false;
        let mut disconnect = false;
        widgets::menu(ui, &more, 200.0, |ui| {
            if widgets::menu_item(ui, "Edit connection", false).clicked {
                edit = true;
                return true;
            }
            if connected && widgets::menu_item(ui, "Disconnect", true).on_hover_text("Close this connection and its tabs").clicked {
                disconnect = true;
                return true;
            }
            false
        });
        if edit {
            let current = self.workspaces.get(&id).map_or(profile, |w| &w.profile);
            let form = ProfileForm::from_profile(current);
            let input = form.input();
            self.profile_form = Some(form);
            self.dispatch(Command::ConnectionUrl(input, true), PASSWORD_LOAD, None, Some(id), RequestKind::Form);
        }
        if disconnect && self.profile_is_clean(id) {
            self.send(Command::Disconnect(id), "Disconnecting", None);
        }
        let second = Rect::from_min_size(pos2(more_rect.left() - 28.0, more_rect.top()), vec2(28.0, 28.0));
        if connecting {
            widgets::spinner_in(ui, Rect::from_center_size(second.center(), Vec2::splat(14.0)));
        } else if connected && active {
            let (icon, tip) = if expanded { (Icon::ChevronUp, "Collapse connection") } else { (Icon::ChevronDown, "Expand connection") };
            if icon_button_in(ui, second, true, icon, None, tip).clicked && !self.collapsed_profiles.remove(&id) {
                self.collapsed_profiles.insert(id);
            }
        }
        if profile.read_only {
            let chip = Rect::from_min_size(pos2(rect.right() - actions_width - 64.0, rect.top() + 4.0), vec2(64.0, 20.0));
            let w = ui.ctx.fonts.width("Read-only", theme::ui(11.0)) + 12.0;
            widgets::chip_in(ui, Rect::from_min_size(chip.min, vec2(w, 20.0)), "Read-only", theme::MUTED);
        }
        if expanded {
            self.workspace_tree(ui, profile, color);
            ui.add_space(6.0);
        }
    }

    fn workspace_tree(&mut self, ui: &mut Ui, profile: &ConnectionProfile, color: Color32) {
        let id = profile.id;
        let Some(workspace) = self.workspaces.get(&id).cloned() else { return };
        let redis = profile.engine == DatabaseEngine::Redis;
        let switching = self.metas().any(|m| m.profile == Some(id) && m.label == "Switching database");
        // Inset under the connection row, with room for the overlay scrollbar.
        let avail = ui.available_rect();
        let area = avail.inset(10.0, 2.0, 8.0, 0.0);
        let used = {
            let mut c = ui.child(area, Dir::Down);
            c.spacing.y = 4.0;
            section_label(&mut c, if redis { "Database index" } else { "Database" });
            let mut chosen = None;
            c.set_enabled(!switching);
            let current = workspace.profile.default_database.clone();
            let w = c.available_width();
            widgets::combo_box(&mut c, Id::new(("db", id)), &current, w, |ui| {
                for db in workspace.databases.iter().filter(|d| d.is_connectable) {
                    if widgets::select_item(ui, &db.name, db.name == current).clicked {
                        chosen = Some(db.name.clone());
                        return true;
                    }
                }
                false
            });
            c.set_enabled(true);
            if let Some(database) = chosen.filter(|d| *d != current)
                && self.profile_is_clean(id)
            {
                self.send(Command::SwitchDatabase(id, database), "Switching database", Some(id));
            }
            c.add_space(8.0);
            let refreshing = self.pending_requests.values().any(|m| m.profile == Some(id) && (m.label == "Loading schema" || m.label == "Refreshing schema"));
            let mut refresh = false;
            section_row(&mut c, if redis { "Keyspace" } else { "Schema" }, |ui| {
                ui.set_enabled(!refreshing);
                let label = if refreshing { "Refreshing…" } else { "Refresh" };
                refresh = widgets::text_button(ui, Some(Icon::Refresh), label, 11.5, "Reload the schema tree").clicked;
            });
            if refresh {
                self.send(Command::Schema(id), "Refreshing schema", Some(id));
            }
            let filter = self.schema_filters.entry(id).or_default();
            // `.schema-filter`: 28 px, 12 px text after a search glyph.
            let out = TextEdit::singleline(Id::new(("schema-filter", id)), filter)
                .hint(if redis { "Filter keys…" } else { "Filter tables…" })
                .font(theme::ui(12.0))
                .margin(9.0, 6.0)
                .pad_left(18.0)
                .name("Filter")
                .show(&mut c);
            icons::paint(&mut c.painter(), Rect::from_center_size(out.response.rect.left_center() + vec2(14.5, 0.0), Vec2::splat(13.0)), Icon::Search, theme::FAINT);
            // Escape clears the filter and keeps it focused.
            if out.escaped {
                filter.clear();
                c.ctx.request_focus(out.response.id);
            }
            let query = filter.trim().to_owned();
            let all = self.schemas.get(&id).map_or(&[][..], Vec::as_slice);
            let filtered;
            let nodes = if query.is_empty() {
                all
            } else {
                filtered = filter_schema_nodes(all, &query);
                filtered.as_slice()
            };
            let selected = self.active_tab.and_then(|tab| self.table_target(tab)).filter(|(p, _, _)| *p == id).map(|(_, s, t)| (s, t));
            let mut open = None;
            c.spacing.y = 0.0;
            for node in nodes {
                schema_branch(&mut c, (id, &mut self.schema_toggled), node, 0, !query.is_empty(), (selected.as_ref(), color), &mut open);
            }
            if !query.is_empty() && nodes.is_empty() {
                c.add_space(4.0);
                c.label_wrapped(&format!("No matches for “{query}”."), theme::ui(12.5), theme::MUTED, false);
            }
            if !self.schemas.contains_key(&id) {
                c.add_space(4.0);
                c.horizontal(20.0, |ui| {
                    widgets::spinner(ui, 14.0);
                    ui.label("Loading…", theme::ui(12.5), theme::MUTED);
                });
            }
            if let Some((schema, table)) = open {
                self.open_table(id, schema, table);
            }
            c.min_rect()
        };
        ui.advance_past(Rect::from_ltrb(avail.left(), avail.top(), avail.right(), used.bottom()));
    }
}

/// One schema tree row and, when open, its children. Branches start open at
/// depth 0; `toggled` holds the ones flipped from that default.
fn schema_branch(
    ui: &mut Ui,
    (profile, toggled): (Uuid, &mut std::collections::HashSet<(Uuid, String)>),
    node: &SchemaNode,
    depth: usize,
    force_open: bool,
    (selected, color): (Option<&(String, String)>, Color32),
    open: &mut Option<(String, String)>,
) {
    let leaf = node.schema.as_ref().zip(node.table.as_ref());
    // `.schema-node-button`: 26 px rows padded `8 + depth * 14`, a 12 px caret
    // column, a 15 px icon, then 12.5 px text.
    let x0 = 8.0 + depth as f32 * 14.0;
    let rect = ui.allocate_row(26.0);
    let key = format!("{depth}/{}/{}/{}", node.kind, node.schema.as_deref().unwrap_or(""), node.name);
    let is_open = leaf.is_none() && (force_open || ((depth < 1) != toggled.contains(&(profile, key.clone()))));
    // Large schemas have thousands of rows; only draw the visible ones.
    if rect.intersects(ui.clip_rect()) {
        let id = ui.make_id(("schema-node", &key));
        let response = ui.interact(rect, id, Sense::CLICK);
        let is_selected = leaf.is_some_and(|(s, t)| selected.is_some_and(|(ss, tt)| ss == s && tt == t));
        let mut a = Node::new(id.0, Role::TreeItem, &node.name, rect);
        a.focusable = true;
        a.selected = leaf.map(|_| is_selected);
        a.expanded = leaf.is_none().then_some(is_open);
        ui.a11y(a);
        let glyph = match (leaf.is_some(), node.kind.as_str()) {
            (false, _) => Icon::Folder,
            (true, "key") => Icon::Key,
            (true, "view") => Icon::View,
            (true, _) => Icon::Table,
        };
        let icon_color = if is_selected {
            color.lerp(Color32::WHITE, 0.45)
        } else if node.kind == "schema" {
            theme::MUTED
        } else {
            theme::FAINT
        };
        let text_left = rect.left() + x0 + 18.0 + 15.0 + 6.0;
        let elided = {
            let mut p = ui.painter();
            if is_selected {
                p.rect_filled(rect, 6.0, color.gamma_multiply(0.24));
            } else if response.hovered {
                p.rect_filled(rect, 6.0, Color32::from_white_alpha(15));
            }
            if leaf.is_none() {
                let caret = if is_open { Icon::ChevronDown } else { Icon::ChevronRight };
                icons::paint(&mut p, Rect::from_center_size(pos2(rect.left() + x0 + 6.0, rect.center().y), Vec2::splat(11.0)), caret, theme::FAINT);
            }
            icons::paint(&mut p, Rect::from_center_size(pos2(rect.left() + x0 + 25.5, rect.center().y), Vec2::splat(13.0)), glyph, icon_color);
            let (font, text) = if is_selected {
                (theme::bold(12.5), Color32::WHITE)
            } else if response.hovered {
                (theme::ui(12.5), theme::TEXT)
            } else {
                (theme::ui(12.5), theme::SECONDARY)
            };
            p.text_truncated(text_left, rect.center().y, &node.name, font, text, rect.right() - text_left - 8.0).1
        };
        if elided {
            let _ = response.clone().on_hover_text(node.name.clone());
        }
        if response.clicked {
            if let Some((schema, table)) = leaf {
                *open = Some((schema.clone(), table.clone()));
            } else if !toggled.remove(&(profile, key.clone())) {
                toggled.insert((profile, key));
            }
        }
    }
    if is_open {
        for child in &node.children {
            schema_branch(ui, (profile, toggled), child, depth + 1, force_open, (selected, color), open);
        }
    }
}
