//! The top bar (connection identity, pending work, demo chip, Help menu and
//! update control) and the tab strip.

use dbm_desktop::updates;
use dbm_ui::a11y::{Node, Role};
use dbm_ui::icons::{self, Icon};
use dbm_ui::text_edit::TextEdit;
use dbm_ui::widgets::{self, accent_button, icon_button_in, secondary_button};
use dbm_ui::{
    Color32, Dir, Id, Pos2, Radius, Rect, ScrollArea, Sense, Shape, Stroke, Ui, Vec2, pos2, theme,
    vec2,
};

use crate::state::{Tab, TabKind, Workbench};

const TAB_TOP: Radius = Radius::top(7.0);

impl Workbench {
    pub(super) fn top_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let profile = self
            .active_tab
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
            .map(|t| t.profile_id)
            .or(self.active_profile)
            .and_then(|id| self.profile(id).cloned());
        ui.painter().rect_filled(rect, 0.0, theme::CHROME);
        ui.painter().hline(
            rect.left()..=rect.right(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, theme::BORDER),
        );
        let inner = rect.inset(16.0, 0.0, 12.0, 1.0);
        let mut row = ui.child(inner, Dir::Left);
        row.spacing.x = 8.0;
        // `.update-control`: only on channel builds.
        if self.updates.enabled() && !self.demo {
            self.help_menu(&mut row);
            self.update_control(&mut row);
        }
        if self.demo {
            let text = "Demo fixture · not a live connection";
            let w = row.ctx.fonts.width(text, theme::ui(11.0)) + 12.0;
            let r = row.allocate(vec2(w, 20.0));
            widgets::chip_in(&mut row, r, text, theme::MODIFIED).on_hover_text(
                "Isolated deterministic data; profiles and edits are not persisted.",
            );
        }
        if let Some(label) = self.pending_requests.values().next().map(|m| m.label) {
            widgets::spinner(&mut row, 14.0);
            row.label(label, theme::ui(12.5), theme::MUTED);
        }
        // The identity takes what is left and truncates before the controls.
        let left = row.available_rect();
        let mut id_row = row.child(left, Dir::Right);
        id_row.spacing.x = 9.0;
        let Some(profile) = profile else {
            id_row.label("No active connection", theme::ui(12.5), theme::MUTED);
            return;
        };
        let color = self.profile_color(profile.id);
        let dot = id_row.allocate(Vec2::splat(8.0));
        id_row.painter().circle_filled(dot.center(), 4.0, color);
        let max = id_row.available_width();
        let name_font = theme::bold(13.0);
        let (name, _) = id_row
            .ctx
            .fonts
            .truncate(&profile.name, name_font, max.max(0.0));
        id_row.label(&name, name_font, theme::TEXT_STRONG);
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
        let chip_width = if profile.read_only { 80.0 } else { 0.0 };
        let max = (id_row.available_width() - chip_width).max(0.0);
        let (target, _) = id_row.ctx.fonts.truncate(&target, theme::ui(11.0), max);
        id_row.label(&target, theme::ui(11.0), theme::FAINT);
        if profile.read_only {
            widgets::chip(&mut id_row, "Read-only", theme::MUTED);
        }
    }

    fn help_menu(&mut self, ui: &mut Ui) {
        let busy = matches!(
            self.updates.state,
            updates::State::Checking | updates::State::Installing
        );
        let font = theme::ui(12.5);
        let w = ui.ctx.fonts.width("Help", font) + 18.0;
        let rect = ui.allocate(vec2(w, 26.0));
        let id = ui.make_id("help-menu");
        let r = ui.interact(rect, id, Sense::CLICK);
        let mut node = Node::new(id.0, Role::MenuButton, "Help", rect);
        node.focusable = true;
        ui.a11y(node);
        {
            let open = ui.ctx.is_popup_open(id.with("menu"));
            let mut p = ui.painter();
            if r.hovered || open {
                p.rect_filled(rect, 6.0, Color32::from_white_alpha(15));
            }
            p.text_mid(
                rect.left() + 9.0,
                rect.center().y,
                "Help",
                font,
                theme::TEXT,
            );
        }
        let mut open = false;
        let mut check = false;
        widgets::menu(ui, &r, 200.0, |ui| {
            if widgets::menu_item(ui, "What’s new", false).clicked {
                open = true;
                return true;
            }
            if widgets::menu_item_enabled(ui, "Check for updates…", false, !busy).clicked {
                check = true;
                return true;
            }
            false
        });
        if open {
            self.updates.open_whats_new();
        }
        if check {
            self.updates.check(false);
        }
    }

    fn update_control(&mut self, ui: &mut Ui) {
        let height = match self.updates.state {
            updates::State::Idle => 14.0,
            updates::State::Available(_) | updates::State::Failed(_) => 34.0,
            _ => 28.0,
        };
        let rect = ui.allocate(vec2(200.0, height));
        let mut col = ui.child(rect, Dir::Down);
        col.spacing.y = 0.0;
        let label = self.updates.label();
        let detail = self.updates.detail();
        let right = |ui: &Ui, w: f32| {
            Rect::from_min_size(pos2(rect.right() - w, ui.cursor().y), vec2(w, 0.0))
        };
        match &self.updates.state {
            updates::State::Available(_) | updates::State::Failed(_) => {
                let available = matches!(self.updates.state, updates::State::Available(_));
                let button = if available {
                    accent_button(&label)
                } else {
                    secondary_button(&label)
                }
                .small();
                let w = button.width(&col);
                let r = right(&col, w);
                let br = Rect::from_min_size(r.min, vec2(w, 24.0));
                col.advance_past(br);
                if button
                    .show_in(&mut col, br)
                    .on_hover_text(detail.clone())
                    .clicked
                {
                    if let updates::State::Available(update) = &self.updates.state {
                        self.confirm_update = Some(update.clone());
                    } else {
                        self.updates.check(false);
                    }
                }
            }
            updates::State::Idle => {}
            _ => {
                let font = theme::ui(11.0);
                let w = col.ctx.fonts.width(&label, font);
                let r = right(&col, w);
                col.painter().text(r.min, label.clone(), font, theme::MUTED);
                col.advance_past(Rect::from_min_size(r.min, vec2(w, font.line_height())));
            }
        }
        let version = self.updates.version_label();
        let font = theme::ui(10.0);
        let w = col.ctx.fonts.width(&version, font);
        let r = Rect::from_min_size(right(&col, w).min, vec2(w, font.line_height()));
        col.painter()
            .text(r.min, version.clone(), font, theme::MUTED);
        let id = col.make_id("version");
        // Click copies the version, for bug reports.
        let response = col
            .interact(r, id, Sense::CLICK)
            .on_hover_text(format!("{detail}\nClick to copy the version"));
        if response.clicked {
            let text = version.trim_start_matches("Version ").to_string();
            col.ctx.copy_text(&text);
            self.show_toast(format!("Copied version {text}"), true);
        }
    }

    pub(super) fn tab_strip(&mut self, ui: &mut Ui, strip: Rect) {
        ui.painter().rect_filled(strip, 0.0, theme::CHROME);
        let inner = strip.inset(6.0, 4.0, 6.0, 0.0);
        let mut active_rect = None;
        let scroll_id = Id::new("tab-strip");
        let mut c = ui.child(inner, Dir::Down);
        c.set_clip(inner);
        let scroll_to = self.tab_scroll_target(&mut c);
        ScrollArea::horizontal(scroll_id)
            .scroll_to_x(scroll_to)
            .hide_bars()
            .show(&mut c, |ui| {
                // A row only as wide as its tabs, so the strip scrolls just
                // as far as they reach.
                let row_rect = Rect::from_min_size(ui.cursor(), vec2(ui.available_width(), 31.0));
                let used = {
                    let mut row = ui.child(row_rect, Dir::Right);
                    let ui = &mut row;
                    ui.spacing.x = 1.0;
                    ui.a11y_group(Id::new("tabs"), Role::Tab, "Tabs", inner, |ui| {
                        let tabs: Vec<Tab> = self.tabs.iter().map(Tab::without_sql).collect();
                        for tab in &tabs {
                            if let Some(r) = self.tab_button(ui, tab, strip.bottom()) {
                                active_rect = Some(r);
                            }
                        }
                    });
                    if let Some(profile) = self
                        .active_profile
                        .filter(|p| self.workspaces.contains_key(p))
                    {
                        let r = ui.allocate(vec2(28.0, 28.0));
                        if icon_button_in(ui, r, true, Icon::Plus, None, "New query").clicked {
                            self.open_query(profile, true);
                        }
                    }
                    row.min_rect()
                };
                ui.advance_past(
                    used.with_min_y(row_rect.top())
                        .with_max_y(row_rect.bottom()),
                );
            });
        // The strip's bottom line, with a gap under the active tab.
        let y = strip.bottom() - 0.5;
        let stroke = Stroke::new(1.0, theme::BORDER);
        let mut p = ui.painter();
        match active_rect
            .map(|r: Rect| (r.left().max(inner.left()), r.right().min(inner.right())))
            .filter(|(a, b)| b > a)
        {
            Some((a, b)) => {
                p.hline(strip.left()..=a + 0.5, y, stroke);
                p.hline(b - 0.5..=strip.right(), y, stroke);
            }
            None => p.hline(strip.left()..=strip.right(), y, stroke),
        }
    }

    /// Scrolls a newly selected tab into view, once.
    fn tab_scroll_target(&mut self, ui: &mut Ui) -> Option<f32> {
        let active = self.active_tab?;
        if ui.ctx.data::<u64>(Id::new("tab-in-view")) == Some(active) {
            return None;
        }
        let rect: Rect = ui.ctx.data(Id::new(("tab-rect", active)))?;
        let state = ScrollArea::state(ui, Id::new("tab-strip"));
        let left = rect.left() - ui.max_rect().left() + state.offset.x;
        let view = ui.max_rect().width();
        ui.ctx.insert_data(Id::new("tab-in-view"), active);
        if left < state.offset.x {
            Some(left)
        } else if left + rect.width() > state.offset.x + view {
            Some(left + rect.width() - view)
        } else {
            None
        }
    }

    /// One tab; returns its rect when it is the active one.
    fn tab_button(&mut self, ui: &mut Ui, tab: &Tab, bottom: f32) -> Option<Rect> {
        let active = self.active_tab == Some(tab.id);
        let color = self.profile_color(tab.profile_id);
        let tid = Id::new(("tab", tab.id));
        if tab.collapsed {
            let rect = ui.allocate(vec2(64.0, 31.0));
            let response = ui
                .interact(rect, tid, Sense::CLICK)
                .on_hover_text(format!("Expand {}", tab.title));
            let mut node = Node::new(tid.0, Role::TabItem, &tab.title, rect);
            node.focusable = true;
            node.selected = Some(active);
            ui.a11y(node);
            let close =
                Rect::from_center_size(rect.right_center() - vec2(10.0, 0.0), Vec2::splat(16.0));
            let cr = ui
                .interact(close, tid.with("close"), Sense::CLICK)
                .on_hover_text(format!("Close {}", tab.title));
            let mut p = ui.painter();
            if response.hovered {
                p.rect_filled(rect, TAB_TOP, Color32::from_white_alpha(10));
            }
            icons::paint(
                &mut p,
                Rect::from_center_size(rect.center() - vec2(8.0, 6.0), Vec2::splat(12.0)),
                Icon::Expand,
                theme::FAINT,
            );
            let font = theme::ui(9.5);
            let (title, _) = p.fonts().truncate(&tab.title, font, 44.0);
            p.text(
                pos2(rect.left() + 5.0, rect.center().y + 2.0),
                title,
                font,
                theme::FAINT,
            );
            icons::paint(
                &mut p,
                close.shrink(3.0),
                Icon::Close,
                if cr.hovered {
                    theme::TEXT
                } else {
                    theme::FAINT
                },
            );
            if cr.clicked {
                self.close_tab(tab.id);
            } else if response.clicked {
                self.select_tab(tab.id);
            }
            return None;
        }
        let renaming = self.renaming.as_ref().is_some_and(|(id, _)| *id == tab.id);
        let title_font = if active {
            theme::bold(12.5)
        } else {
            theme::ui(12.5)
        };
        let title_w = if renaming {
            140.0
        } else {
            ui.ctx.fonts.width(&tab.title, title_font).min(230.0)
        };
        let query = tab.kind == TabKind::Query;
        let width = 10.0
            + 13.0
            + 6.0
            + title_w
            + 6.0
            + if query && !renaming { 28.0 } else { 0.0 }
            + if active { 34.0 } else { 0.0 }
            + 28.0
            + 4.0;
        let rect = ui.allocate(vec2(width, 31.0));
        ui.ctx.insert_data(Id::new(("tab-rect", tab.id)), rect);
        // The whole tab is the click target (select, middle click closes,
        // double click renames); its buttons sit on top.
        let whole = ui.interact(rect, tid, Sense::CLICK);
        let hovered = ui.rect_contains_pointer(rect);
        let mut node = Node::new(tid.0, Role::TabItem, &tab.title, rect);
        node.focusable = true;
        node.selected = Some(active);
        ui.a11y(node);
        if whole.middle_clicked {
            self.close_tab(tab.id);
            return None;
        }
        if whole.clicked {
            self.select_tab(tab.id);
        }
        let input_id = Id::new(("tab-title-input", tab.id));
        if whole.double_clicked && query {
            self.renaming = Some((tab.id, tab.title.clone()));
            ui.ctx.request_focus(input_id);
        }
        {
            let mut p = ui.painter();
            if active {
                // As on macOS: outlined on the left, top and right and open
                // at the bottom, so it joins the canvas below.
                p.rect_filled(
                    Rect::from_ltrb(rect.left(), rect.top(), rect.right(), bottom),
                    TAB_TOP,
                    theme::BG,
                );
                p.add(Shape::Path {
                    points: open_tab_outline(rect, bottom),
                    stroke: Stroke::new(1.0, theme::BORDER),
                    closed: false,
                });
                p.rect_filled(
                    Rect::from_min_size(rect.min, vec2(rect.width(), 2.0)),
                    TAB_TOP,
                    color,
                );
            } else if hovered {
                p.rect_filled(rect, TAB_TOP, Color32::from_white_alpha(10));
            }
            let glyph = if query { Icon::Code } else { Icon::Table };
            icons::paint(
                &mut p,
                Rect::from_center_size(
                    pos2(rect.left() + 16.5, rect.center().y),
                    Vec2::splat(13.0),
                ),
                glyph,
                color.gamma_multiply(0.85),
            );
        }
        let mut x = rect.left() + 29.0;
        let y = rect.center().y;
        if let Some((_, draft)) = self.renaming.as_mut().filter(|(id, _)| *id == tab.id) {
            // `.tab-title-input`: 140 px on the edit surface.
            let field = Rect::from_min_size(pos2(x, y - 12.0), vec2(140.0, 24.0));
            let out = TextEdit::singleline(input_id, draft)
                .font(theme::ui(12.5))
                .margin(6.0, 3.0)
                .background(theme::EDIT_SURFACE)
                .name("Tab name")
                .show_in(ui, field);
            if out.response.lost_focus
                || out.response.clicked_elsewhere
                || out.submitted
                || out.escaped
            {
                let title = draft.trim().to_owned();
                if !out.escaped
                    && !title.is_empty()
                    && let Some(t) = self.tabs.iter_mut().find(|t| t.id == tab.id)
                {
                    t.title = title;
                }
                self.renaming = None;
            }
            x += 146.0;
        } else {
            let color = if active {
                theme::TEXT_STRONG
            } else if hovered {
                theme::TEXT
            } else {
                theme::MUTED
            };
            let (_, elided) = ui
                .painter()
                .text_truncated(x, y, &tab.title, title_font, color, 230.0);
            if elided {
                let _ = whole.clone().on_hover_text(tab.title.clone());
            }
            x += title_w + 6.0;
            // `.tab-rename`: a pencil after the title on hover; its space is
            // kept so the tab doesn't change width.
            if query {
                let plate = Rect::from_min_size(pos2(x, y - 11.0), Vec2::splat(22.0));
                if hovered {
                    let pencil = ui
                        .interact(plate, Id::new(("tab-rename", tab.id)), Sense::CLICK)
                        .on_hover_text(format!("Rename {}", tab.title));
                    let mut p = ui.painter();
                    let fill = if pencil.hovered {
                        theme::CONTROL_HOVER
                    } else if active {
                        theme::BG
                    } else {
                        theme::CHROME
                    };
                    p.rect_filled(plate, 5.0, fill);
                    icons::paint(
                        &mut p,
                        plate.shrink(5.0),
                        Icon::Pencil,
                        if pencil.hovered {
                            theme::TEXT
                        } else {
                            theme::FAINT
                        },
                    );
                    if pencil.clicked {
                        self.renaming = Some((tab.id, tab.title.clone()));
                        ui.ctx.request_focus(input_id);
                    }
                }
                x += 28.0;
            }
        }
        if active {
            let r = Rect::from_min_size(pos2(x, y - 14.0), vec2(28.0, 28.0));
            ui.scope(|ui| {
                ui.push_id(tab.id);
                if icon_button_in(
                    ui,
                    r,
                    true,
                    Icon::Collapse,
                    None,
                    &format!("Collapse {}", tab.title),
                )
                .clicked
                {
                    self.collapse_tab(tab.id);
                }
            });
            x += 34.0;
        }
        // Close shows on hover and on the active tab; its space is kept.
        if hovered || active {
            let r = Rect::from_min_size(pos2(x, y - 14.0), vec2(28.0, 28.0));
            let clicked = ui
                .scope(|ui| {
                    ui.push_id(tab.id);
                    icon_button_in(
                        ui,
                        r,
                        true,
                        Icon::Close,
                        None,
                        &format!("Close {}", tab.title),
                    )
                    .clicked
                })
                .0;
            if clicked {
                self.close_tab(tab.id);
            }
        }
        active.then_some(rect)
    }
}

/// The left, top and right edges of an active tab, with its top corners
/// rounded to 7 px, running down to `bottom`.
fn open_tab_outline(rect: Rect, bottom: f32) -> Vec<Pos2> {
    let (left, right, top) = (rect.left() + 0.5, rect.right() - 0.5, rect.top() + 0.5);
    let radius = 7.0_f32.min((right - left) / 2.0);
    let corner = |center: Pos2, from: f32| {
        (0..=6).map(move |step| {
            let angle = from + std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
            pos2(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
    };
    std::iter::once(pos2(left, bottom))
        .chain(corner(
            pos2(left + radius, top + radius),
            std::f32::consts::PI,
        ))
        .chain(corner(
            pos2(right - radius, top + radius),
            1.5 * std::f32::consts::PI,
        ))
        .chain(std::iter::once(pos2(right, bottom)))
        .collect()
}
