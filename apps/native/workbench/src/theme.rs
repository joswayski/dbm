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
    // `cursor: pointer` on every clickable control, as in the desktop app.
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
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

/// Primary action: accent fill, white semibold text. One per surface.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.to_owned())
            .color(Color32::WHITE)
            .strong(),
    )
    .fill(ACCENT_STRONG)
    .stroke(Stroke::NONE)
}

pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).color(DANGER))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
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
    egui::RichText::new(text)
        .font(ui_font(11.0))
        .strong()
        .color(MUTED)
}

pub fn eyebrow(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .font(ui_font(10.5))
        .strong()
        .color(FAINT)
}
