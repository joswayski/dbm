//! Graphite controls, drawn to match the egui app pixel for pixel where it
//! matters: action buttons, toolbar buttons, chips, checkboxes, combo boxes,
//! menus, spinners, modals and the toast.

use crate::a11y::{Node, Role};
use crate::ctx::{Id, Response, Sense};
use crate::geom::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use crate::icons::{self, Icon};
use crate::input::{Key, Modifiers};
use crate::paint::Layer;
use crate::text::Font;
use crate::theme::{self, *};
use crate::ui::{Dir, Margin, Ui};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Accent,
    Danger,
}

/// `.primary-button` / `.secondary-button` / `.danger-button`: 30 px,
/// radius 7, an optional leading icon and a trailing shortcut pill.
pub struct ActionButton<'a> {
    text: &'a str,
    icon: Option<Icon>,
    shortcut: Option<&'a str>,
    kind: ButtonKind,
    min_width: f32,
    small: bool,
    enabled: bool,
    id: Option<Id>,
}

pub fn primary_button(text: &str) -> ActionButton<'_> {
    ActionButton {
        text,
        icon: None,
        shortcut: None,
        kind: ButtonKind::Primary,
        min_width: 0.0,
        small: false,
        enabled: true,
        id: None,
    }
}

pub fn secondary_button(text: &str) -> ActionButton<'_> {
    ActionButton {
        kind: ButtonKind::Secondary,
        ..primary_button(text)
    }
}

pub fn accent_button(text: &str) -> ActionButton<'_> {
    ActionButton {
        kind: ButtonKind::Accent,
        ..primary_button(text)
    }
}

pub fn danger_button(text: &str) -> ActionButton<'_> {
    ActionButton {
        kind: ButtonKind::Danger,
        ..primary_button(text)
    }
}

impl<'a> ActionButton<'a> {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn shortcut(mut self, s: &'a str) -> Self {
        self.shortcut = Some(s);
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// A stable id when the label changes ("Run" / "Running…").
    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    fn font(&self) -> Font {
        let size = if self.small { 12.0 } else { 12.5 };
        if self.kind == ButtonKind::Primary {
            theme::bold(size)
        } else {
            theme::ui(size)
        }
    }

    pub fn width(&self, ui: &Ui) -> f32 {
        let fonts = &ui.ctx.fonts;
        let text = fonts.width(self.text, self.font());
        let icon = if self.icon.is_some() { 18.0 } else { 0.0 };
        let kbd = self
            .shortcut
            .map_or(0.0, |s| fonts.width(s, theme::ui(11.0)) + 16.0);
        ((if self.small { 20.0 } else { 24.0 }) + icon + text + kbd).max(self.min_width)
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let w = self.width(ui);
        let h = if self.small { 26.0 } else { 30.0 };
        let rect = ui.allocate(vec2(w, h));
        self.show_in(ui, rect)
    }

    pub fn show_in(self, ui: &mut Ui, rect: Rect) -> Response {
        let enabled = self.enabled && ui.is_enabled();
        let id = self.id.unwrap_or_else(|| ui.make_id(("button", self.text)));
        let mut node = Node::new(id.0, Role::Button, self.text, rect);
        node.focusable = true;
        node.enabled = enabled;
        node.shortcut = self.shortcut.map(str::to_owned);
        let r = ui.interact_enabled(rect, id, Sense::CLICK, enabled);
        ui.a11y(node);
        let hovered = r.hovered && enabled;
        let pressed = r.is_pointer_down && enabled;
        let (fill, stroke, color) = match self.kind {
            ButtonKind::Danger => (
                Color32::from_rgba_unmultiplied(255, 107, 97, if hovered { 41 } else { 23 }),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 107, 97, 77)),
                DANGER,
            ),
            ButtonKind::Accent => (
                if hovered {
                    Color32::from_rgba_premultiplied(18, 35, 58, 58)
                } else {
                    ACCENT_SOFT
                },
                Stroke::new(1.0, ACCENT.gamma_multiply(0.55)),
                ACCENT_TEXT,
            ),
            ButtonKind::Secondary => (
                if pressed {
                    CONTROL_ACTIVE
                } else if hovered {
                    CONTROL_HOVER
                } else {
                    CONTROL
                },
                Stroke::new(1.0, BORDER_STRONG),
                TEXT,
            ),
            ButtonKind::Primary => (
                if pressed {
                    ACCENT
                } else if hovered {
                    Color32::from_rgb(0x45, 0x85, 0xd6)
                } else {
                    ACCENT_STRONG
                },
                Stroke::NONE,
                Color32::WHITE,
            ),
        };
        let alpha = if enabled { 1.0 } else { 0.45 };
        let font = self.font();
        let fonts_w = ui.ctx.fonts.width(self.text, font);
        let kbd_w = self.shortcut.map(|s| ui.ctx.fonts.width(s, theme::ui(11.0)));
        let icon_w = if self.icon.is_some() { 18.0 } else { 0.0 };
        let content = icon_w + fonts_w + kbd_w.map_or(0.0, |w| w + 16.0);
        let mut p = ui.painter();
        p.rect(
            rect,
            7.0,
            fill.gamma_multiply(alpha),
            Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha)),
        );
        let color = color.gamma_multiply(alpha);
        let mut x = rect.center().x - content / 2.0;
        let y = rect.center().y;
        if let Some(icon) = self.icon {
            icons::paint(&mut p, Rect::from_center_size(pos2(x + 6.0, y), Vec2::splat(12.0)), icon, color);
            x += icon_w;
        }
        p.text_mid(x, y, self.text, font, color);
        x += fonts_w;
        if let (Some(s), Some(w)) = (self.shortcut, kbd_w) {
            let lh = theme::ui(11.0).line_height();
            let pill = Rect::from_min_size(pos2(x + 6.0, y - lh / 2.0 - 1.0), vec2(w + 10.0, lh + 2.0));
            p.rect_filled(pill, 4.0, Color32::from_white_alpha(51).gamma_multiply(alpha));
            p.text_mid(pill.left() + 5.0, y, s, theme::ui(11.0), color);
        }
        r
    }
}

impl Ui<'_> {
    /// `interact` with an explicit enabled state (a disabled button inside
    /// an enabled area).
    pub fn interact_enabled(&mut self, rect: Rect, id: Id, sense: Sense, enabled: bool) -> Response {
        let was = self.is_enabled();
        if was && !enabled {
            let mut r = {
                let clip = self.clip_rect();
                let layer = self.layer();
                self.ctx.interact(id, rect, clip, layer, sense, false)
            };
            r.enabled = false;
            return r;
        }
        self.interact(rect, id, sense)
    }

    pub fn button(&mut self, b: ActionButton) -> Response {
        b.show(self)
    }
}

/// `.toolbar-button`: a transparent 28 px button with an icon and an
/// optional label; the label (or tooltip) is its accessible name.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: Option<&str>, tooltip: &str) -> Response {
    icon_button_enabled(ui, true, icon, label, tooltip)
}

pub fn icon_button_enabled(ui: &mut Ui, enabled: bool, icon: Icon, label: Option<&str>, tooltip: &str) -> Response {
    let font = theme::ui(12.5);
    let text_w = label.map(|l| ui.ctx.fonts.width(l, font));
    let pad = if label.is_some() { 9.0 } else { 7.0 };
    let w = pad * 2.0 + 14.0 + text_w.map_or(0.0, |w| w + 6.0);
    let rect = ui.allocate(vec2(w, 28.0));
    icon_button_in(ui, rect, enabled, icon, label, tooltip)
}

pub fn icon_button_in(ui: &mut Ui, rect: Rect, enabled: bool, icon: Icon, label: Option<&str>, tooltip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let name = label.unwrap_or(tooltip);
    let id = ui.make_id(("icon-button", tooltip, icon as u8));
    let r = ui.interact_enabled(rect, id, Sense::CLICK, enabled);
    let mut node = Node::new(id.0, Role::Button, name, rect);
    node.focusable = true;
    node.enabled = enabled;
    ui.a11y(node);
    let r = r.on_any_hover_text(tooltip);
    let hovered = r.hovered && enabled;
    let pad = if label.is_some() { 9.0 } else { 7.0 };
    let mut p = ui.painter();
    if hovered || r.has_focus {
        p.rect_filled(rect, 6.0, Color32::from_white_alpha(15));
    }
    let color = if !enabled {
        FAINT
    } else if hovered {
        TEXT
    } else {
        SECONDARY
    };
    let icon_rect = Rect::from_center_size(
        pos2(if label.is_some() { rect.left() + pad + 7.0 } else { rect.center().x }, rect.center().y),
        Vec2::splat(14.0),
    );
    icons::paint(&mut p, icon_rect, icon, color);
    if let Some(l) = label {
        p.text_mid(icon_rect.right() + 6.0, rect.center().y, l, theme::ui(12.5), color);
    }
    r
}

/// `.text-button`: accent text with an optional icon and an accent wash on hover.
pub fn text_button(ui: &mut Ui, icon: Option<Icon>, label: &str, size: f32, tooltip: &str) -> Response {
    let enabled = ui.is_enabled();
    let font = theme::ui(size);
    let tw = ui.ctx.fonts.width(label, font);
    let icon_size = size + 1.0;
    let iw = icon.map_or(0.0, |_| icon_size + 4.0);
    let rect = ui.allocate(vec2(12.0 + iw + tw, font.line_height() + 6.0));
    let id = ui.make_id(("text-button", label));
    let r = ui.interact(rect, id, Sense::CLICK).on_any_hover_text(tooltip);
    let mut node = Node::new(id.0, Role::Button, label, rect);
    node.focusable = true;
    ui.a11y(node);
    let mut p = ui.painter();
    if r.hovered && enabled {
        p.rect_filled(rect, 5.0, ACCENT_SOFT);
    }
    let color = if enabled { ACCENT_TEXT } else { FAINT };
    let mut x = rect.left() + 6.0;
    if let Some(icon) = icon {
        icons::paint(&mut p, Rect::from_center_size(pos2(x + icon_size / 2.0, rect.center().y), Vec2::splat(icon_size)), icon, color);
        x += iw;
    }
    p.text_mid(x, rect.center().y, label, font, color);
    r
}

/// `.selection-actions-trigger`: accent text with a chevron, opening a menu.
pub fn menu_trigger(ui: &mut Ui, label: &str, tooltip: &str) -> Response {
    let font = theme::ui(12.5);
    let tw = ui.ctx.fonts.width(label, font);
    let rect = ui.allocate(vec2(18.0 + tw + 17.0, 28.0));
    let id = ui.make_id(("menu-trigger", tooltip));
    let r = ui.interact(rect, id, Sense::CLICK).on_hover_text(tooltip);
    let mut node = Node::new(id.0, Role::MenuButton, label, rect);
    node.focusable = true;
    ui.a11y(node);
    let mut p = ui.painter();
    if r.hovered {
        p.rect_filled(rect, 6.0, Color32::from_white_alpha(15));
    }
    let y = rect.center().y;
    p.text_mid(rect.left() + 9.0, y, label, font, ACCENT_TEXT);
    icons::paint(&mut p, Rect::from_center_size(pos2(rect.left() + 9.0 + tw + 11.0, y), Vec2::splat(12.0)), Icon::ChevronDown, ACCENT_TEXT);
    r
}

/// `.icon-toggle`: 30×28; on shows an accent-soft fill and accent icon.
pub fn toggle(ui: &mut Ui, icon: Icon, on: bool, tooltip: &str) -> Response {
    let rect = ui.allocate(vec2(30.0, 28.0));
    let id = ui.make_id(("toggle", tooltip));
    let r = ui.interact(rect, id, Sense::CLICK).on_hover_text(tooltip);
    let mut node = Node::new(id.0, Role::Button, tooltip, rect);
    node.focusable = true;
    node.toggled = Some(on);
    ui.a11y(node);
    let (fill, color) = if on {
        (ACCENT_SOFT, ACCENT_TEXT)
    } else if r.hovered {
        (Color32::from_white_alpha(15), TEXT)
    } else {
        (Color32::TRANSPARENT, MUTED)
    };
    let mut p = ui.painter();
    p.rect_filled(rect, 6.0, fill);
    icons::paint(&mut p, Rect::from_center_size(rect.center(), Vec2::splat(14.0)), icon, color);
    r
}

/// A non-interactive icon.
pub fn icon(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Rect {
    let rect = ui.allocate(Vec2::splat(size));
    icons::paint(&mut ui.painter(), rect, icon, color);
    rect
}

/// A small rounded label, such as "Read-only".
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let font = theme::ui(11.0);
    let w = ui.ctx.fonts.width(text, font);
    let rect = ui.allocate(vec2(w + 12.0, font.line_height() + 4.0));
    chip_in(ui, rect, text, color)
}

pub fn chip_in(ui: &mut Ui, rect: Rect, text: &str, color: Color32) -> Response {
    let font = theme::ui(11.0);
    let id = ui.make_id(("chip", text));
    let r = ui.interact(rect, id, Sense::HOVER);
    ui.a11y(Node::new(id.0, Role::Text, text, rect));
    let w = ui.ctx.fonts.width(text, font);
    let mut p = ui.painter();
    p.rect_filled(rect, 5.0, color.gamma_multiply(0.14));
    p.text_mid(rect.center().x - w / 2.0, rect.center().y, text, font, color);
    r
}

/// A checkbox with its label.
pub fn checkbox(ui: &mut Ui, value: &mut bool, label: &str) -> Response {
    let font = theme::ui(13.0);
    let w = ui.ctx.fonts.width(label, font);
    let rect = ui.allocate(vec2(18.0 + 8.0 + w, 22.0));
    let id = ui.make_id(("checkbox", label));
    let mut r = ui.interact(rect, id, Sense::CLICK);
    if r.clicked {
        *value = !*value;
        r.changed = true;
    }
    let mut node = Node::new(id.0, Role::CheckBox, label, rect);
    node.focusable = true;
    node.toggled = Some(*value);
    ui.a11y(node);
    let enabled = ui.is_enabled();
    let alpha = if enabled { 1.0 } else { 0.45 };
    let bx = Rect::from_center_size(pos2(rect.left() + 9.0, rect.center().y), Vec2::splat(16.0));
    let mut p = ui.painter();
    if *value {
        p.rect_filled(bx, 4.0, ACCENT_STRONG.gamma_multiply(alpha));
        icons::paint(&mut p, bx.shrink(2.0), Icon::Check, Color32::WHITE.gamma_multiply(alpha));
    } else {
        let fill = if r.hovered { CONTROL_HOVER } else { CONTROL };
        p.rect(bx, 4.0, fill.gamma_multiply(alpha), Stroke::new(1.0, BORDER_STRONG.gamma_multiply(alpha)));
    }
    let color = if r.hovered && enabled { TEXT_STRONG } else { TEXT };
    p.text_mid(rect.left() + 26.0, rect.center().y, label, font, color.gamma_multiply(alpha));
    r
}

/// A spinning arc while something loads.
pub fn spinner(ui: &mut Ui, size: f32) -> Rect {
    let rect = ui.allocate(Vec2::splat(size));
    spinner_in(ui, rect);
    rect
}

pub fn spinner_in(ui: &mut Ui, rect: Rect) {
    let t = ui.ctx.time();
    let start = (t * 5.0) as f32;
    let r = rect.width().min(rect.height()) / 2.0 - 1.5;
    let c = rect.center();
    let points: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = start + i as f32 / 20.0 * 4.4;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    ui.painter().line(points, Stroke::new(2.0, MUTED));
    ui.ctx.request_repaint_after(1.0 / 30.0);
}

// ---- popups ---------------------------------------------------------------

/// Shows a popup under `anchor` while it is open. A press outside closes
/// it; `f` returning `true` (an item was chosen) closes it too.
pub fn popup(ui: &mut Ui, id: Id, anchor: Rect, width: f32, f: impl FnOnce(&mut Ui) -> bool) -> bool {
    if !ui.ctx.is_popup_open(id) {
        return false;
    }
    let screen = ui.ctx.screen_rect();
    let height: f32 = ui.ctx.data(id.with("height")).unwrap_or(0.0);
    let mut pos = pos2(anchor.left(), anchor.bottom() + 2.0);
    if pos.x + width > screen.right() - 6.0 {
        pos.x = (anchor.right() - width).max(6.0);
    }
    if height > 0.0 && pos.y + height > screen.bottom() - 6.0 && anchor.top() - height - 2.0 > 6.0 {
        pos.y = anchor.top() - height - 2.0;
    }
    let max_h = (screen.bottom() - pos.y - 6.0).max(80.0);
    let rect = Rect::from_min_size(pos, vec2(width, if height > 0.0 { height.min(max_h) } else { max_h }));
    ui.ctx.set_popup_area(rect);
    let mut chosen = false;
    let mut measured = 0.0;
    ui.area(Layer::Popup, rect, |ui| {
        // Blocks the content underneath.
        let _ = ui.interact(rect, id.with("bg"), Sense::HOVER);
        let mut p = ui.painter();
        p.shadow(rect, 8.0, vec2(0.0, 8.0), 24.0, Color32::from_black_alpha(110));
        p.rect(rect, 8.0, POPOVER, Stroke::new(1.0, BORDER_STRONG));
        ui.a11y_group(id.with("menu"), Role::Menu, "", rect, |ui| {
            let inner = rect.shrink(4.0);
            let mut child = ui.child(inner, Dir::Down);
            child.spacing = vec2(0.0, 1.0);
            child.set_clip(rect);
            let out = crate::scroll::ScrollArea::vertical(id.with("scroll")).show(&mut child, |ui| f(ui));
            chosen = out.inner;
            measured = out.state.content.y + 8.0;
        });
    });
    if (measured - height).abs() > 0.5 {
        ui.ctx.insert_data(id.with("height"), measured);
        ui.ctx.request_rerun();
    }
    if ui.ctx.consume_key(Modifiers::NONE, Key::Escape) {
        chosen = true;
    }
    if chosen {
        ui.ctx.close_popup();
    }
    chosen
}

/// A menu row: 28 px, 12.5 px text, accent fill on hover; danger rows are red.
pub fn menu_item(ui: &mut Ui, label: &str, danger: bool) -> Response {
    menu_item_enabled(ui, label, danger, true)
}

pub fn menu_item_enabled(ui: &mut Ui, label: &str, danger: bool, enabled: bool) -> Response {
    let rect = ui.allocate_row(28.0);
    let id = ui.make_id(("menu-item", label));
    let r = ui.interact_enabled(rect, id, Sense::CLICK, enabled);
    let mut node = Node::new(id.0, Role::MenuItem, label, rect);
    node.focusable = true;
    node.enabled = enabled;
    ui.a11y(node);
    let hovered = r.hovered && enabled || r.has_focus;
    let (fill, color) = match (danger, hovered) {
        (false, true) => (ACCENT_STRONG, Color32::WHITE),
        (false, false) => (Color32::TRANSPARENT, TEXT),
        (true, true) => (Color32::from_rgba_unmultiplied(255, 107, 97, 46), Color32::from_rgb(0xff, 0xb3, 0xac)),
        (true, false) => (Color32::TRANSPARENT, DANGER),
    };
    let alpha = if enabled { 1.0 } else { 0.45 };
    let mut p = ui.painter();
    p.rect_filled(rect, 5.0, fill);
    p.text_mid(rect.left() + 8.0, rect.center().y, label, theme::ui(12.5), color.gamma_multiply(alpha));
    r
}

/// A selectable list row in a combo box popup.
pub fn select_item(ui: &mut Ui, label: &str, selected: bool) -> Response {
    let rect = ui.allocate_row(26.0);
    let id = ui.make_id(("select-item", label));
    let r = ui.interact(rect, id, Sense::CLICK);
    let mut node = Node::new(id.0, Role::ListItem, label, rect);
    node.focusable = true;
    node.selected = Some(selected);
    ui.a11y(node);
    let mut p = ui.painter();
    let (fill, color) = if r.hovered || r.has_focus {
        (ACCENT_STRONG, Color32::WHITE)
    } else if selected {
        (CONTROL_ACTIVE, TEXT_STRONG)
    } else {
        (Color32::TRANSPARENT, TEXT)
    };
    p.rect_filled(rect, 5.0, fill);
    let max = rect.width() - 16.0;
    p.text_truncated(rect.left() + 8.0, rect.center().y, label, theme::ui(12.5), color, max);
    r
}

/// A combo box: a control showing `selected`, opening a list. `f` adds the
/// options (with `select_item`) and returns whether one was chosen.
pub fn combo_box(ui: &mut Ui, id: Id, selected: &str, width: f32, f: impl FnOnce(&mut Ui) -> bool) -> Response {
    let rect = ui.allocate(vec2(width, 28.0));
    combo_box_in(ui, id, rect, selected, f)
}

pub fn combo_box_in(ui: &mut Ui, id: Id, rect: Rect, selected: &str, f: impl FnOnce(&mut Ui) -> bool) -> Response {
    let mut r = ui.interact(rect, id, Sense::CLICK);
    let open = ui.ctx.is_popup_open(id);
    let mut node = Node::new(id.0, Role::ComboBox, "", rect);
    node.value = Some(selected.to_owned());
    node.focusable = true;
    node.expanded = Some(open);
    ui.a11y(node);
    if r.clicked {
        ui.ctx.toggle_popup(id);
    }
    let enabled = ui.is_enabled();
    let alpha = if enabled { 1.0 } else { 0.45 };
    let fill = if open {
        CONTROL_ACTIVE
    } else if r.hovered {
        CONTROL_HOVER
    } else {
        CONTROL
    };
    {
        let mut p = ui.painter();
        p.rect(rect, 6.0, fill.gamma_multiply(alpha), Stroke::new(1.0, BORDER_STRONG.gamma_multiply(alpha)));
        p.text_truncated(rect.left() + 9.0, rect.center().y, selected, theme::ui(12.5), TEXT.gamma_multiply(alpha), rect.width() - 34.0);
        icons::paint(&mut p, Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), Vec2::splat(12.0)), Icon::ChevronDown, MUTED.gamma_multiply(alpha));
    }
    let width = rect.width().max(160.0);
    if popup(ui, id, rect, width, f) {
        r.changed = true;
    }
    r
}

/// A menu opened from a button: `f` adds `menu_item`s and returns whether
/// one was chosen.
pub fn menu(ui: &mut Ui, trigger: &Response, width: f32, f: impl FnOnce(&mut Ui) -> bool) {
    let id = trigger.id.with("menu");
    if trigger.clicked {
        ui.ctx.toggle_popup(id);
    }
    popup(ui, id, trigger.rect, width, f);
}

/// A context menu opened by a right click on `target`, at the pointer.
pub fn context_menu(ui: &mut Ui, target: &Response, width: f32, f: impl FnOnce(&mut Ui) -> bool) {
    let id = target.id.with("context");
    if target.secondary_clicked {
        if let Some(p) = ui.ctx.pointer() {
            ui.ctx.insert_data(id.with("at"), p);
        }
        ui.ctx.open_popup(id);
    }
    if ui.ctx.is_popup_open(id) {
        let at: Pos2 = ui.ctx.data(id.with("at")).unwrap_or(target.rect.left_bottom());
        popup(ui, id, Rect::from_min_size(at, Vec2::ZERO), width, f);
    }
}

// ---- modals and toasts ----------------------------------------------------

pub struct ModalOutput<R> {
    pub inner: Option<R>,
    pub backdrop_clicked: bool,
}

/// A centred dialog over a 55% backdrop. Its height is its content's,
/// scrolling when the window is too short.
pub fn modal<R>(ui: &mut Ui, title: &str, width: f32, f: impl FnOnce(&mut Ui) -> R) -> ModalOutput<R> {
    let id = Id::new(("modal", title));
    let screen = ui.ctx.screen_rect();
    let mut backdrop_clicked = false;
    let inner = ui.area(Layer::Modal, screen, |ui| {
        let r = ui.interact(screen, id.with("backdrop"), Sense::CLICK.unfocusable().plain());
        backdrop_clicked = r.clicked;
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        let height: f32 = ui.ctx.data(id.with("height")).unwrap_or(200.0);
        let max_h = (screen.height() - 80.0).max(120.0);
        let h = height.min(max_h);
        let w = width.min(screen.width() - 32.0);
        let rect = Rect::from_center_size(screen.center(), vec2(w, h));
        // Blocks the backdrop under the window.
        let _ = ui.interact(rect, id.with("window"), Sense::CLICK.unfocusable().plain());
        let mut p = ui.painter();
        p.shadow(rect, 14.0, vec2(0.0, 16.0), 40.0, Color32::from_black_alpha(140));
        p.rect(rect, 14.0, POPOVER, Stroke::new(1.0, BORDER_STRONG));
        let mut measured = 0.0;
        let inner = ui.a11y_group(id, Role::Dialog, title, rect, |ui| {
            let mut child = ui.child(rect.shrink(18.0), Dir::Down);
            child.set_clip(rect.shrink(1.0));
            let out = crate::scroll::ScrollArea::vertical(id.with("scroll")).solid_bar().show(&mut child, f);
            measured = out.state.content.y + 36.0;
            out.inner
        });
        if (measured - height).abs() > 0.5 {
            ui.ctx.insert_data(id.with("height"), measured);
            ui.ctx.request_rerun();
        }
        inner
    });
    ModalOutput {
        inner: Some(inner),
        backdrop_clicked,
    }
}

/// The dialog's eyebrow and title.
pub fn dialog_title(ui: &mut Ui, eyebrow: Option<&str>, title: &str) {
    if let Some(e) = eyebrow {
        ui.label(e, theme::bold(10.5), FAINT);
    }
    ui.label_wrapped(title, theme::bold(17.0), TEXT_STRONG, false);
    ui.add_space(10.0);
}

/// Bottom-right toast with a status dot; returns true when dismissed.
pub fn toast(ui: &mut Ui, text: &str, dot: Color32, opacity: f32) -> bool {
    let screen = ui.ctx.screen_rect();
    let font = theme::ui(12.5);
    let lines = ui.ctx.fonts.wrap(text, font, 520.0 - 14.0 - 10.0 - 20.0 - 24.0 - 12.0);
    let tw = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let lh = font.line_height();
    let size = vec2(14.0 + 8.0 + 12.0 + tw + 12.0 + 24.0 + 10.0, (lines.len() as f32 * lh).max(24.0) + 20.0);
    let rect = Rect::from_min_size(screen.right_bottom() - size - vec2(16.0, 16.0), size);
    let mut dismiss = false;
    ui.area(Layer::Popup, rect, |ui| {
        ui.set_opacity(opacity);
        let _ = ui.interact(rect, Id::new("toast-bg"), Sense::HOVER);
        {
            let mut p = ui.painter();
            p.shadow(rect, 10.0, vec2(0.0, 16.0), 40.0, Color32::from_black_alpha(140));
            p.rect(rect, 10.0, POPOVER, Stroke::new(1.0, BORDER_STRONG));
            p.circle_filled(pos2(rect.left() + 18.0, rect.center().y), 4.0, dot);
            let top = rect.center().y - lines.len() as f32 * lh / 2.0;
            for (i, l) in lines.iter().enumerate() {
                p.text(pos2(rect.left() + 34.0, top + i as f32 * lh), &text[l.range.clone()], font, TEXT);
            }
        }
        let mut node = Node::new(Id::new("toast").0, Role::Text, text, rect);
        node.parent = None;
        ui.a11y(node);
        let b = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), Vec2::splat(24.0));
        dismiss = dismiss_button(ui, b, MUTED, "Dismiss notification").clicked;
    });
    dismiss
}

/// A 24 px close button.
pub fn dismiss_button(ui: &mut Ui, rect: Rect, color: Color32, tooltip: &str) -> Response {
    let id = ui.make_id(("dismiss", tooltip));
    let r = ui.interact(rect, id, Sense::CLICK).on_hover_text(tooltip);
    let mut node = Node::new(id.0, Role::Button, tooltip, rect);
    node.focusable = true;
    ui.a11y(node);
    let mut p = ui.painter();
    if r.hovered {
        p.rect_filled(rect, 5.0, Color32::from_white_alpha(12));
    }
    icons::paint(&mut p, rect.shrink(5.5), Icon::Close, color);
    r
}

/// Margins helper used by panels.
pub fn inset(rect: Rect, m: Margin) -> Rect {
    rect.inset(m.left, m.top, m.right, m.bottom)
}
