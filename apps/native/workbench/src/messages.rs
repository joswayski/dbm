//! The desktop app's message surfaces: the app error banner under the top
//! bar, inline errors and notices inside query and table views, the export
//! result notice, and the bottom-right toast. Errors fade after 10 seconds
//! and notices after 6; an export result stays until dismissed.

use std::path::PathBuf;

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Stroke};

use crate::icons::{self, Icon};
use crate::theme::{self, ui_font};

pub const ERROR_SECONDS: f64 = 10.0;
pub const NOTICE_SECONDS: f64 = 6.0;

const ERROR_TEXT: Color32 = Color32::from_rgb(0xff, 0xb3, 0xac);
const NOTICE_TEXT: Color32 = Color32::from_rgb(0x9b, 0xe7, 0xbf);
const EXPORT_LINK: Color32 = Color32::from_rgb(0xc6, 0xf3, 0xda);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Error,
    Notice,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub text: String,
    pub kind: Kind,
    pub shown_at: f64,
    /// A finished export: the file and how many rows it holds.
    pub export: Option<(PathBuf, u64)>,
}

impl Message {
    pub fn error(text: impl Into<String>, now: f64) -> Self {
        Self {
            text: text.into(),
            kind: Kind::Error,
            shown_at: now,
            export: None,
        }
    }

    pub fn notice(text: impl Into<String>, now: f64) -> Self {
        Self {
            text: text.into(),
            kind: Kind::Notice,
            shown_at: now,
            export: None,
        }
    }

    pub fn exported(path: PathBuf, rows: u64, now: f64) -> Self {
        Self {
            text: String::new(),
            kind: Kind::Notice,
            shown_at: now,
            export: Some((path, rows)),
        }
    }

    fn lifetime(&self) -> Option<f64> {
        match (self.export.is_some(), self.kind) {
            (true, _) => None,
            (false, Kind::Error) => Some(ERROR_SECONDS),
            (false, Kind::Notice) => Some(NOTICE_SECONDS),
        }
    }

    /// Opacity for the fade-in and fade-out, or `None` once it has expired.
    /// Schedules the repaints the fade and expiry need.
    pub fn opacity(&self, ctx: &egui::Context, now: f64) -> Option<f32> {
        let Some(lifetime) = self.lifetime() else {
            return Some(1.0);
        };
        let age = now - self.shown_at;
        if age >= lifetime {
            return None;
        }
        let fade = 0.4;
        let opacity = if age < 0.15 {
            ctx.request_repaint();
            (age / 0.15) as f32
        } else if age > lifetime - fade {
            ctx.request_repaint();
            ((lifetime - age) / fade) as f32
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(lifetime - fade - age));
            1.0
        };
        Some(opacity.clamp(0.0, 1.0))
    }
}

/// What the person did with an inline message.
#[derive(PartialEq, Eq)]
pub enum Response {
    None,
    Dismiss,
    Open,
    Reveal,
}

fn colors(kind: Kind) -> (Color32, Color32, Color32) {
    match kind {
        Kind::Error => (
            Color32::from_rgba_unmultiplied(255, 107, 97, 23),
            Color32::from_rgba_unmultiplied(255, 107, 97, 64),
            ERROR_TEXT,
        ),
        Kind::Notice => (
            Color32::from_rgba_unmultiplied(90, 211, 148, 26),
            Color32::from_rgba_unmultiplied(90, 211, 148, 64),
            NOTICE_TEXT,
        ),
    }
}

/// Wrapped message text with a dismiss button on the right. Centered text
/// keeps a matching 24 px gutter on the left, like the desktop grid layout.
fn text_row(ui: &mut egui::Ui, text: &str, color: Color32, centered: bool, tooltip: &str) -> bool {
    let full = ui.available_width();
    let gutter = if centered { 24.0 + 8.0 } else { 0.0 };
    let wrap = (full - gutter - 24.0 - 8.0).max(40.0);
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), ui_font(12.5), color, wrap);
    job.halign = if centered { Align::Center } else { Align::Min };
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let height = galley.size().y.max(24.0);
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(full, height), egui::Sense::hover());
    let text_x = if centered {
        rect.left() + gutter + wrap / 2.0
    } else {
        rect.left()
    };
    let text_pos = egui::pos2(text_x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, color);
    let button = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 24.0, rect.center().y - 12.0),
        egui::Vec2::splat(24.0),
    );
    let mut clicked = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| {
        clicked = dismiss_button(ui, color, tooltip);
    });
    clicked
}

fn dismiss_button(ui: &mut egui::Ui, color: Color32, tooltip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(24.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 5.0, Color32::from_white_alpha(12));
    }
    icons::paint(ui.painter(), rect.shrink(5.5), Icon::Close, color);
    response.on_hover_text(tooltip).clicked()
}

/// Inline error or notice inside a query or table view (8 px from the
/// content above, 12 px from the sides). Errors are centered, as in the
/// desktop app; notices read left to right.
pub fn inline(ui: &mut egui::Ui, message: &Message, opacity: f32) -> Response {
    let (fill, stroke, text) = colors(message.kind);
    let mut response = Response::None;
    ui.scope(|ui| {
        ui.set_opacity(opacity);
        Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke))
            .corner_radius(CornerRadius::same(7))
            .outer_margin(Margin {
                left: 12,
                right: 12,
                top: 8,
                bottom: 0,
            })
            .inner_margin(Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if let Some((path, rows)) = &message.export {
                        response = export_body(ui, path, *rows);
                        return;
                    }
                    let centered = message.kind == Kind::Error;
                    if text_row(ui, &message.text, text, centered, "Dismiss message") {
                        response = Response::Dismiss;
                    }
                });
            });
    });
    response
}

fn export_body(ui: &mut egui::Ui, path: &std::path::Path, rows: u64) -> Response {
    let mut response = Response::None;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let noun = if rows == 1 { "row" } else { "rows" };
    let font = ui_font(12.5);
    ui.spacing_mut().item_spacing.x = 0.0;
    ui.label(
        RichText::new(format!(
            "Exported {} filtered {noun} to ",
            crate::theme::count(rows)
        ))
        .font(font.clone())
        .color(NOTICE_TEXT),
    );
    let link = ui.add(
        egui::Label::new(
            RichText::new(&name)
                .font(crate::theme::medium(font.size))
                .underline()
                .color(EXPORT_LINK),
        )
        .sense(egui::Sense::click()),
    );
    if link
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Open the exported file")
        .clicked()
    {
        response = Response::Open;
    }
    ui.label(RichText::new(".").font(font).color(NOTICE_TEXT));
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        if dismiss_button(ui, NOTICE_TEXT, "Dismiss export result") {
            response = Response::Dismiss;
        }
        if ui
            .add(
                crate::theme::secondary_button("Show in folder")
                    .icon(Icon::Folder)
                    .small(),
            )
            .on_hover_text("Reveal the exported file")
            .clicked()
        {
            response = Response::Reveal;
        }
    });
    response
}

/// Background for the full-width app error strip under the top bar.
pub fn banner_frame(opacity: f32) -> Frame {
    let (fill, _, _) = colors(Kind::Error);
    Frame::new()
        .fill(fill.gamma_multiply(opacity))
        .inner_margin(Margin::symmetric(14, 6))
}

/// Full-width app error strip under the top bar, text centered.
pub fn banner(ui: &mut egui::Ui, message: &Message, opacity: f32) -> Response {
    let (_, stroke, text) = colors(Kind::Error);
    let mut response = Response::None;
    ui.set_opacity(opacity);
    let clip = ui.clip_rect();
    ui.painter().hline(
        clip.x_range(),
        clip.bottom() - 0.5,
        Stroke::new(1.0, stroke),
    );
    if text_row(ui, &message.text, text, true, "Dismiss error") {
        response = Response::Dismiss;
    }
    response
}

/// Bottom-right toast with a status dot, used for schema refresh summaries.
pub fn toast(ctx: &egui::Context, text: &str, dot: Color32, opacity: f32) -> bool {
    let mut dismiss = false;
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::Vec2::new(-16.0, -16.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            Frame::new()
                .fill(theme::POPOVER)
                .stroke(Stroke::new(1.0, theme::BORDER_STRONG))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin {
                    left: 14,
                    right: 10,
                    top: 10,
                    bottom: 10,
                })
                .shadow(egui::Shadow {
                    offset: [0, 16],
                    blur: 40,
                    spread: 0,
                    color: Color32::from_black_alpha(140),
                })
                .show(ui, |ui| {
                    ui.set_max_width(520.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let (rect, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(8.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, dot);
                        ui.add(
                            egui::Label::new(
                                RichText::new(text).font(ui_font(12.5)).color(theme::TEXT),
                            )
                            .wrap(),
                        );
                        if dismiss_button(ui, theme::MUTED, "Dismiss notification") {
                            dismiss = true;
                        }
                    });
                });
        });
    dismiss
}
