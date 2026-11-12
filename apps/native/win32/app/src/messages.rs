//! Message surfaces: the app error strip under the top bar, inline errors
//! and notices in query and table views, the export result, and the toast.
//! Errors fade after 10 seconds and notices after 6; an export result stays
//! until dismissed. The same rules as the egui app's `messages.rs`.

use std::path::PathBuf;

use dbm_ui::a11y::{Node, Role};
use dbm_ui::icons::Icon;
use dbm_ui::theme;
use dbm_ui::widgets::{self, dismiss_button, secondary_button};
use dbm_ui::{Color32, Context, Rect, Sense, Stroke, Ui, pos2, vec2};

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
    /// Schedules the frames the fade and expiry need.
    pub fn opacity(&self, ctx: &mut Context, now: f64) -> Option<f32> {
        let Some(lifetime) = self.lifetime() else {
            return Some(1.0);
        };
        let age = now - self.shown_at;
        if age >= lifetime {
            return None;
        }
        let fade = 0.4;
        let opacity = if age < 0.15 {
            ctx.request_repaint_after(1.0 / 60.0);
            (age / 0.15) as f32
        } else if age > lifetime - fade {
            ctx.request_repaint_after(1.0 / 60.0);
            ((lifetime - age) / fade) as f32
        } else {
            ctx.request_repaint_after(lifetime - fade - age);
            1.0
        };
        Some(opacity.clamp(0.0, 1.0))
    }
}

/// What the person did with an inline message.
#[derive(PartialEq, Eq, Debug)]
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

/// Wrapped text with a dismiss button on the right, inside `rect`'s width;
/// returns the height it needs and whether dismiss was clicked.
fn text_row(
    ui: &mut Ui,
    x: (f32, f32),
    top: f32,
    text: &str,
    color: Color32,
    centered: bool,
    tooltip: &str,
) -> (f32, bool) {
    let (left, right) = x;
    let font = theme::ui(12.5);
    let gutter = if centered { 24.0 + 8.0 } else { 0.0 };
    let wrap = (right - left - gutter - 24.0 - 8.0).max(40.0);
    let lines = ui.ctx.fonts.wrap(text, font, wrap);
    let lh = font.line_height();
    let height = (lines.len() as f32 * lh).max(24.0);
    let text_top = top + (height - lines.len() as f32 * lh) / 2.0;
    {
        let mut p = ui.painter();
        for (i, line) in lines.iter().enumerate() {
            let x = if centered {
                left + gutter + wrap / 2.0 - line.width / 2.0
            } else {
                left
            };
            p.text(
                pos2(x, text_top + i as f32 * lh),
                &text[line.range.clone()],
                font,
                color,
            );
        }
    }
    let rect = Rect::from_ltrb(left, top, right, top + height);
    ui.a11y(Node::new(
        ui.make_id(("message", text)).0,
        Role::Text,
        text,
        rect,
    ));
    let button = Rect::from_min_size(
        pos2(right - 24.0, top + height / 2.0 - 12.0),
        vec2(24.0, 24.0),
    );
    let clicked = dismiss_button(ui, button, color, tooltip).clicked;
    (height, clicked)
}

/// Inline error or notice in a query or table view: 8 px below the content
/// above, 12 px from the sides. Errors are centred; notices read left to right.
pub fn inline(ui: &mut Ui, message: &Message, opacity: f32) -> Response {
    let (fill, stroke, text) = colors(message.kind);
    let mut response = Response::None;
    let avail = ui.available_rect();
    let outer = Rect::from_ltrb(
        avail.left() + 12.0,
        avail.top() + 8.0,
        avail.right() - 12.0,
        avail.top() + 8.0,
    );
    let inner_x = (outer.left() + 12.0, outer.right() - 12.0);
    let top = outer.top() + 6.0;
    let previous = ui.replace_opacity(ui.opacity() * opacity);
    let mark = ui.paint_mark();
    let height = if let Some((path, rows)) = &message.export {
        let (h, r) = export_body(ui, inner_x, top, path, *rows);
        response = r;
        h
    } else {
        let centered = message.kind == Kind::Error;
        let (h, dismissed) = text_row(
            ui,
            inner_x,
            top,
            &message.text,
            text,
            centered,
            "Dismiss message",
        );
        if dismissed {
            response = Response::Dismiss;
        }
        h
    };
    let rect = Rect::from_ltrb(outer.left(), outer.top(), outer.right(), top + height + 6.0);
    let before = ui.paint_mark();
    ui.painter().rect(rect, 7.0, fill, Stroke::new(1.0, stroke));
    let painted = ui.paint_mark() - before;
    ui.move_last_behind(mark, painted);
    ui.replace_opacity(previous);
    ui.advance_past(rect.with_min_y(avail.top()));
    response
}

fn export_body(
    ui: &mut Ui,
    (left, right): (f32, f32),
    top: f32,
    path: &std::path::Path,
    rows: u64,
) -> (f32, Response) {
    let mut response = Response::None;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let noun = if rows == 1 { "row" } else { "rows" };
    let font = theme::ui(12.5);
    let height = 26.0;
    let y = top + height / 2.0;
    let lead = format!("Exported {} {noun} to ", theme::count(rows));
    let lead_w = ui.painter().text_mid(left, y, &lead, font, NOTICE_TEXT);
    let room = (right - left - lead_w - 180.0).max(40.0);
    let (name_w, _) = ui
        .painter()
        .text_truncated(left + lead_w, y, &name, font, EXPORT_LINK, room);
    let link_rect = Rect::from_ltrb(
        left + lead_w,
        top + 4.0,
        left + lead_w + name_w,
        top + height - 4.0,
    );
    let id = ui.make_id(("export-link", &name));
    let link = ui
        .interact(link_rect, id, Sense::CLICK)
        .on_hover_text("Open the exported file");
    ui.painter().hline(
        link_rect.left()..=link_rect.right(),
        y + 8.0,
        Stroke::new(1.0, EXPORT_LINK),
    );
    let mut node = Node::new(id.0, Role::Link, &name, link_rect);
    node.focusable = true;
    ui.a11y(node);
    if link.clicked {
        response = Response::Open;
    }
    ui.painter()
        .text_mid(link_rect.right(), y, ".", font, NOTICE_TEXT);
    let dismiss = Rect::from_min_size(pos2(right - 24.0, y - 12.0), vec2(24.0, 24.0));
    if dismiss_button(ui, dismiss, NOTICE_TEXT, "Dismiss export result").clicked {
        response = Response::Dismiss;
    }
    let show = secondary_button("Show in folder")
        .icon(Icon::Folder)
        .small();
    let w = show.width(ui);
    let rect = Rect::from_min_size(pos2(dismiss.left() - 5.0 - w, y - 13.0), vec2(w, 26.0));
    if show
        .show_in(ui, rect)
        .on_hover_text("Reveal the exported file")
        .clicked
    {
        response = Response::Reveal;
    }
    (height, response)
}

/// Full-width app error strip under the top bar, text centred. Returns its
/// height and the person's response.
pub fn banner(ui: &mut Ui, rect_top: Rect, message: &Message, opacity: f32) -> (f32, Response) {
    let (fill, stroke, text) = colors(Kind::Error);
    let previous = ui.replace_opacity(ui.opacity() * opacity);
    let mark = ui.paint_mark();
    let x = (rect_top.left() + 14.0, rect_top.right() - 14.0);
    let (h, dismissed) = text_row(
        ui,
        x,
        rect_top.top() + 6.0,
        &message.text,
        text,
        true,
        "Dismiss error",
    );
    let rect = Rect::from_ltrb(
        rect_top.left(),
        rect_top.top(),
        rect_top.right(),
        rect_top.top() + h + 12.0,
    );
    let before = ui.paint_mark();
    ui.painter().rect_filled(rect, 0.0, fill);
    let painted = ui.paint_mark() - before;
    ui.move_last_behind(mark, painted);
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, stroke),
    );
    ui.replace_opacity(previous);
    (
        h + 12.0,
        if dismissed {
            Response::Dismiss
        } else {
            Response::None
        },
    )
}

/// Bottom-right toast with a status dot, for schema refresh summaries.
pub fn toast(ui: &mut Ui, text: &str, dot: Color32, opacity: f32) -> bool {
    widgets::toast(ui, text, dot, opacity)
}
