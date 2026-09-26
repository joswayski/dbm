//! Graphite design tokens (`docs/design-system.md`) for egui.

use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, Vec2};

pub const BG: Color32 = Color32::from_rgb(0x16, 0x16, 0x18);
pub const CHROME: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1e);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x21);
pub const GRID_HEADER: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1c);
pub const CONTROL: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x32, 0x32, 0x35);
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3d);
pub const POPOVER: Color32 = Color32::from_rgb(0x26, 0x26, 0x29);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x20, 0x20, 0x23);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x35, 0x35, 0x38);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xf5, 0xf5, 0xf7);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xea);
pub const SECONDARY: Color32 = Color32::from_rgb(0xc7, 0xc7, 0xcc);
pub const MUTED: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa6);
pub const FAINT: Color32 = Color32::from_rgb(0x80, 0x80, 0x87);
pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);
pub const ACCENT_STRONG: Color32 = Color32::from_rgb(0x3b, 0x78, 0xc7);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x7d, 0xb6, 0xff);
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(11, 22, 36, 36);
pub const EDIT_SURFACE: Color32 = Color32::from_rgb(0x10, 0x19, 0x2a);
pub const MODIFIED: Color32 = Color32::from_rgb(0xf0, 0xb1, 0x4c);
pub const MODIFIED_SOFT: Color32 = Color32::from_rgba_premultiplied(29, 21, 9, 31);
pub const SUCCESS: Color32 = Color32::from_rgb(0x5a, 0xd3, 0x94);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x8a, 0x80);
pub const DANGER_SOFT: Color32 = Color32::from_rgba_premultiplied(23, 10, 9, 23);

/// Connection colors offered in the profile editor.
pub const CONNECTION_COLORS: [&str; 8] = [
    "#4c9aff", "#ff9f43", "#3dd6c6", "#b48cff", "#ff6b8a", "#7ed957", "#f0b14c", "#8e8e93",
];
pub const DEFAULT_CONNECTION_COLOR: &str = "#4c9aff";

pub const ROW_HEIGHT: f32 = 32.0;
pub const HEADER_HEIGHT: f32 = 34.0;
pub const TOOLBAR_HEIGHT: f32 = 44.0;
pub const PENDING_BAR_HEIGHT: f32 = 48.0;
pub const STATUS_BAR_HEIGHT: f32 = 28.0;
pub const INSPECTOR_WIDTH: f32 = 300.0;

pub fn parse_color(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    if h.len() == 6 {
        if let Ok(v) = u32::from_str_radix(h, 16) {
            return Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
        }
    }
    ACCENT
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn ui_font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Geist at weight 500. egui draws a variable font's default instance only,
/// so the heavier weights are bundled as static instances.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("medium".into()))
}

/// Geist at weight 600, for headings and primary buttons.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

pub fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Geist".into(),
        egui::FontData::from_static(include_bytes!("../assets/geist.ttf")).into(),
    );
    fonts.font_data.insert(
        "Geist Mono".into(),
        egui::FontData::from_static(include_bytes!("../assets/geist-mono.ttf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Geist".into());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "Geist Mono".into());
    let fallbacks = fonts.families[&egui::FontFamily::Proportional][1..].to_vec();
    for (family, name, bytes) in [
        (
            "medium",
            "Geist Medium",
            &include_bytes!("../assets/geist-medium.ttf")[..],
        ),
        (
            "semibold",
            "Geist SemiBold",
            &include_bytes!("../assets/geist-semibold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(bytes).into());
        let mut list = vec![name.to_owned()];
        list.extend(fallbacks.iter().cloned());
        fonts
            .families
            .insert(egui::FontFamily::Name(family.into()), list);
    }
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = None;
    visuals.panel_fill = CHROME;
    visuals.window_fill = POPOVER;
    visuals.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.extreme_bg_color = CONTROL;
    visuals.faint_bg_color = Color32::from_rgb(0x19, 0x19, 0x1b);
    visuals.code_bg_color = BG;
    visuals.hyperlink_color = ACCENT_TEXT;
    visuals.warn_fg_color = MODIFIED;
    visuals.error_fg_color = DANGER;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(76, 154, 255, 64);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.text_edit_bg_color = Some(CONTROL);
    let radius = CornerRadius::same(6);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = CHROME;
    widgets.noninteractive.weak_bg_fill = CHROME;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = radius;
    for (state, fill, stroke) in [
        (&mut widgets.inactive, CONTROL, BORDER_STRONG),
        (&mut widgets.hovered, CONTROL_HOVER, BORDER_STRONG),
        (&mut widgets.active, CONTROL_ACTIVE, ACCENT),
        (&mut widgets.open, CONTROL_ACTIVE, BORDER_STRONG),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, TEXT);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
    widgets.inactive.fg_stroke = Stroke::new(1.0, SECONDARY);
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_STRONG);
    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 4.0);
        style.spacing.button_padding = Vec2::new(10.0, 4.0);
        style.spacing.interact_size = Vec2::new(28.0, 28.0);
        style.spacing.menu_margin = egui::Margin::same(4);
        style.interaction.selectable_labels = false;
        // Tooltips show promptly and even while the pointer drifts; egui's
        // defaults (0.5 s, pointer held still) read as missing.
        style.interaction.tooltip_delay = 0.25;
        style.interaction.show_tooltips_only_when_still = false;
        style
            .text_styles
            .insert(egui::TextStyle::Body, ui_font(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, ui_font(12.5));
        style
            .text_styles
            .insert(egui::TextStyle::Small, ui_font(11.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, ui_font(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, mono(12.0));
    });
}

/// A menu row (`.selection-actions-menu button`): 28 px, 12.5 px text,
/// accent fill with white text on hover; danger rows are red with a red wash.
pub fn menu_item(ui: &mut egui::Ui, label: &str, danger: bool) -> egui::Response {
    let width = ui.available_width().max(ui.min_rect().width());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let hovered = response.hovered();
    let (fill, color) = match (danger, hovered) {
        (false, true) => (ACCENT_STRONG, Color32::WHITE),
        (false, false) => (Color32::TRANSPARENT, TEXT),
        (true, true) => (
            Color32::from_rgba_unmultiplied(255, 107, 97, 46),
            Color32::from_rgb(0xff, 0xb3, 0xac),
        ),
        (true, false) => (Color32::TRANSPARENT, DANGER),
    };
    ui.painter().rect_filled(rect, 5, fill);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), ui_font(12.5), color);
    ui.painter().galley(
        egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}

/// Menu popups: `padding: 4px` with 1 px between rows, at least `width`.
pub fn menu_scope(ui: &mut egui::Ui, width: f32) {
    ui.spacing_mut().item_spacing.y = 1.0;
    ui.set_min_width(width - 8.0);
}

/// A count with thousands separators, like `toLocaleString()` in en-US.
pub fn count(value: impl Into<u64>) -> String {
    let digits = value.into().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Rectangles, with their layer, where clickable rows keep the arrow cursor
/// (`.data-grid tbody tr { cursor: default }`).
#[derive(Clone, Default)]
struct PlainCursorZones(Vec<(egui::LayerId, egui::Rect)>);

/// Keeps the default cursor over `rect` on `ui`'s layer this frame.
pub fn plain_cursor_zone(ui: &egui::Ui, rect: egui::Rect) {
    let layer = ui.layer_id();
    ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_default::<PlainCursorZones>(egui::Id::NULL)
            .0
            .push((layer, rect));
    });
}

/// The widget that had keyboard focus when the previous frame ended. egui
/// drops focus as soon as Escape is pressed, so Escape handlers ask this.
pub fn focused_last_frame(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.data(|d| d.get_temp::<Option<egui::Id>>(egui::Id::new("focused-last-frame")))
        .flatten()
}

/// Runs after the UI is built. egui never reads `Visuals::interact_cursor`,
/// so this applies `button { cursor: pointer }`: a hovered clickable widget
/// that set no cursor of its own gets the pointing hand, or not-allowed when
/// disabled. It also records the focused widget for `focused_last_frame`.
pub fn end_frame(ctx: &egui::Context) {
    let zones = ctx.data_mut(|d| {
        d.remove_temp::<PlainCursorZones>(egui::Id::NULL)
            .unwrap_or_default()
    });
    let focused = ctx.memory(|m| m.focused());
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("focused-last-frame"), focused));
    if ctx.output(|o| o.cursor_icon) != egui::CursorIcon::Default {
        return;
    }
    let hovered: Vec<egui::Id> = ctx.interaction_snapshot(|s| s.hovered.iter().copied().collect());
    let pointer = ctx.pointer_hover_pos();
    for id in hovered {
        let Some(response) = ctx.read_response(id) else {
            continue;
        };
        if !response.sense.senses_click() {
            continue;
        }
        let plain = zones.0.iter().any(|(layer, rect)| {
            *layer == response.layer_id && pointer.is_some_and(|p| rect.contains(p))
        });
        if plain {
            continue;
        }
        ctx.set_cursor_icon(if response.enabled() {
            egui::CursorIcon::PointingHand
        } else {
            egui::CursorIcon::NotAllowed
        });
        return;
    }
}

/// Primary action: accent fill, white semibold text. One per surface.
pub fn primary_button(text: &str) -> ActionButton {
    ActionButton {
        text: text.to_owned(),
        icon: None,
        shortcut: None,
        kind: ButtonKind::Primary,
        min_width: 0.0,
    }
}

/// `.secondary-button`: control fill, strong border, text colour.
pub fn secondary_button(text: &str) -> ActionButton {
    ActionButton {
        kind: ButtonKind::Secondary,
        ..primary_button(text)
    }
}

/// `.danger-button`: danger text on a soft red fill with a red outline.
pub fn danger_button(text: &str) -> ActionButton {
    ActionButton {
        kind: ButtonKind::Danger,
        ..primary_button(text)
    }
}

/// `.primary-button` / `.danger-button`: 30 px, radius 7, 12 px padding, an
/// optional leading icon and a trailing shortcut pill (`kbd`).
pub struct ActionButton {
    text: String,
    icon: Option<crate::icons::Icon>,
    shortcut: Option<String>,
    kind: ButtonKind,
    min_width: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

impl ActionButton {
    pub fn icon(mut self, icon: crate::icons::Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.to_owned());
        self
    }

    /// Stretches the button, keeping its content centred.
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }
}

impl egui::Widget for ActionButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let enabled = ui.is_enabled();
        let font = if self.kind == ButtonKind::Primary {
            semibold(12.5)
        } else {
            medium(12.5)
        };
        let text = ui
            .painter()
            .layout_no_wrap(self.text.clone(), font, Color32::WHITE);
        let kbd = self.shortcut.as_ref().map(|s| {
            ui.painter()
                .layout_no_wrap(s.clone(), medium(11.0), Color32::WHITE)
        });
        let icon_width = if self.icon.is_some() { 12.0 + 6.0 } else { 0.0 };
        let kbd_width = kbd.as_ref().map_or(0.0, |g| g.size().x + 10.0 + 6.0);
        let content = icon_width + text.size().x + kbd_width;
        let width = (24.0 + content).max(self.min_width);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 30.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &self.text)
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let hovered = response.hovered() && enabled;
        let pressed = response.is_pointer_button_down_on() && enabled;
        let (fill, stroke, color) = match self.kind {
            ButtonKind::Danger => (
                Color32::from_rgba_unmultiplied(255, 107, 97, if hovered { 41 } else { 23 }),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 107, 97, 77)),
                DANGER,
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
        // `button:disabled { opacity: .45 }`
        let alpha = if enabled { 1.0 } else { 0.45 };
        let painter = ui.painter();
        painter.rect(
            rect,
            7,
            fill.gamma_multiply(alpha),
            Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha)),
            egui::StrokeKind::Inside,
        );
        let color = color.gamma_multiply(alpha);
        let mut x = rect.center().x - content / 2.0;
        let y = rect.center().y;
        if let Some(icon) = self.icon {
            crate::icons::paint(
                painter,
                egui::Rect::from_center_size(egui::pos2(x + 6.0, y), Vec2::splat(12.0)),
                icon,
                color,
            );
            x += icon_width;
        }
        let text_width = text.size().x;
        painter.galley(egui::pos2(x, y - text.size().y / 2.0), text, color);
        x += text_width;
        if let Some(kbd) = kbd {
            let pill = egui::Rect::from_min_size(
                egui::pos2(x + 6.0, y - kbd.size().y / 2.0 - 1.0),
                kbd.size() + Vec2::new(10.0, 2.0),
            );
            painter.rect_filled(pill, 4, Color32::from_white_alpha(51).gamma_multiply(alpha));
            painter.galley(pill.min + Vec2::new(5.0, 1.0), kbd, color);
        }
        response
    }
}

/// Small rounded label, e.g. "Read-only" or "truncated".
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), ui_font(11.0), color);
    let size = galley.size() + Vec2::new(12.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(5), color.gamma_multiply(0.14));
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// 11 pt semibold section label in sentence case.
pub fn section_label(text: &str) -> egui::RichText {
    egui::RichText::new(text).font(semibold(11.0)).color(MUTED)
}

pub fn eyebrow(text: &str) -> egui::RichText {
    egui::RichText::new(text).font(semibold(10.5)).color(FAINT)
}

#[cfg(test)]
mod tests {
    #[test]
    fn counts_use_thousands_separators() {
        assert_eq!(super::count(0u32), "0");
        assert_eq!(super::count(999u32), "999");
        assert_eq!(super::count(1_000u32), "1,000");
        assert_eq!(super::count(1_234_567u64), "1,234,567");
    }
}
