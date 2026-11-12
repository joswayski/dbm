//! A software renderer for the display list (feature `raster`): tiny-skia
//! for shapes and ab_glyph for Space Mono, so screens can be snapshot off
//! Windows. It approximates Direct2D closely enough to check layout; the
//! Windows renderer stays the reference for how text looks.

use ab_glyph::{Font as _, FontRef, PxScale, ScaleFont as _};
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, Stroke as SkStroke, Transform};

use crate::geom::{Color32, Pos2, Radius, Rect};
use crate::paint::{DisplayList, Shape};
use crate::text::{ASCENT, Font, FontBackend};

static FACES: [&[u8]; 4] = [
    include_bytes!("../../../workbench/assets/SpaceMono-Regular.ttf"),
    include_bytes!("../../../workbench/assets/SpaceMono-Bold.ttf"),
    include_bytes!("../../../workbench/assets/SpaceMono-Italic.ttf"),
    include_bytes!("../../../workbench/assets/SpaceMono-BoldItalic.ttf"),
];

fn face(font: Font) -> FontRef<'static> {
    let i = usize::from(font.bold) | usize::from(font.italic) << 1;
    FontRef::try_from_slice(FACES[i]).expect("bundled Space Mono parses")
}

/// Glyph advances from the bundled faces.
pub struct RasterMetrics;

impl FontBackend for RasterMetrics {
    fn advance(&self, ch: char, font: Font) -> f32 {
        let f = face(font);
        let id = f.glyph_id(ch);
        if id.0 == 0 && !ch.is_ascii() {
            return font.size * 0.612;
        }
        f.as_scaled(PxScale::from(font.size * em_to_px(&f)))
            .h_advance(id)
    }
}

/// ab_glyph scales by pixel height (ascent - descent); Fonts sizes are em.
fn em_to_px(f: &FontRef<'_>) -> f32 {
    let upem = f.units_per_em().unwrap_or(1000.0);
    (f.ascent_unscaled() - f.descent_unscaled()) / upem
}

fn sk_color(c: Color32) -> tiny_skia::Color {
    let [r, g, b, a] = c.to_unmultiplied_f32();
    tiny_skia::Color::from_rgba(r, g, b, a).unwrap_or(tiny_skia::Color::TRANSPARENT)
}

fn paint(c: Color32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(sk_color(c));
    p.anti_alias = true;
    p
}

fn rounded(r: Rect, radius: Radius) -> Option<tiny_skia::Path> {
    let lim = (r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    let (nw, ne, se, sw) = (
        radius.nw.min(lim),
        radius.ne.min(lim),
        radius.se.min(lim),
        radius.sw.min(lim),
    );
    const K: f32 = 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(r.left() + nw, r.top());
    pb.line_to(r.right() - ne, r.top());
    pb.cubic_to(
        r.right() - ne + ne * K,
        r.top(),
        r.right(),
        r.top() + ne - ne * K,
        r.right(),
        r.top() + ne,
    );
    pb.line_to(r.right(), r.bottom() - se);
    pb.cubic_to(
        r.right(),
        r.bottom() - se + se * K,
        r.right() - se + se * K,
        r.bottom(),
        r.right() - se,
        r.bottom(),
    );
    pb.line_to(r.left() + sw, r.bottom());
    pb.cubic_to(
        r.left() + sw - sw * K,
        r.bottom(),
        r.left(),
        r.bottom() - sw + sw * K,
        r.left(),
        r.bottom() - sw,
    );
    pb.line_to(r.left(), r.top() + nw);
    pb.cubic_to(
        r.left(),
        r.top() + nw - nw * K,
        r.left() + nw - nw * K,
        r.top(),
        r.left() + nw,
        r.top(),
    );
    pb.close();
    pb.finish()
}

fn polyline(points: &[Pos2], closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.x, first.y);
    for p in rest {
        pb.line_to(p.x, p.y);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// Renders `list` at `scale` pixels per DIP into a `width`×`height` pixmap.
pub fn render(list: &DisplayList, width: u32, height: u32, scale: f32, clear: Color32) -> Pixmap {
    let mut pm = Pixmap::new(width.max(1), height.max(1)).expect("non-empty pixmap");
    pm.fill(sk_color(clear));
    let t = Transform::from_scale(scale, scale);
    let mut mask_for: Option<(Rect, Mask)> = None;
    for item in list.iter() {
        let clip = item.clip.intersect(Rect::from_ltrb(
            0.0,
            0.0,
            width as f32 / scale,
            height as f32 / scale,
        ));
        if !clip.is_positive() {
            continue;
        }
        if mask_for.as_ref().is_none_or(|(r, _)| *r != clip) {
            let mut m = Mask::new(width.max(1), height.max(1)).expect("mask");
            if let Some(rect) =
                tiny_skia::Rect::from_ltrb(clip.left(), clip.top(), clip.right(), clip.bottom())
            {
                m.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, false, t);
            }
            mask_for = Some((clip, m));
        }
        let mask = mask_for.as_ref().map(|(_, m)| m);
        match &item.shape {
            Shape::Rect {
                rect,
                radius,
                fill,
                stroke,
            } => {
                if !fill.is_transparent()
                    && let Some(p) = rounded(*rect, *radius)
                {
                    pm.fill_path(&p, &paint(*fill), FillRule::Winding, t, mask);
                }
                if !stroke.is_none() {
                    let h = stroke.width / 2.0;
                    let rad = Radius {
                        nw: (radius.nw - h).max(0.0),
                        ne: (radius.ne - h).max(0.0),
                        sw: (radius.sw - h).max(0.0),
                        se: (radius.se - h).max(0.0),
                    };
                    if let Some(p) = rounded(rect.shrink(h), rad) {
                        let s = SkStroke {
                            width: stroke.width,
                            ..Default::default()
                        };
                        pm.stroke_path(&p, &paint(stroke.color), &s, t, mask);
                    }
                }
            }
            Shape::Circle {
                center,
                radius,
                fill,
                stroke,
            } => {
                if let Some(p) = PathBuilder::from_circle(center.x, center.y, *radius) {
                    if !fill.is_transparent() {
                        pm.fill_path(&p, &paint(*fill), FillRule::Winding, t, mask);
                    }
                    if !stroke.is_none() {
                        pm.stroke_path(
                            &p,
                            &paint(stroke.color),
                            &SkStroke {
                                width: stroke.width,
                                ..Default::default()
                            },
                            t,
                            mask,
                        );
                    }
                }
            }
            Shape::Ellipse {
                center,
                radii,
                stroke,
            } => {
                if let Some(r) = tiny_skia::Rect::from_xywh(
                    center.x - radii.x,
                    center.y - radii.y,
                    radii.x * 2.0,
                    radii.y * 2.0,
                ) && let Some(p) = PathBuilder::from_oval(r)
                {
                    pm.stroke_path(
                        &p,
                        &paint(stroke.color),
                        &SkStroke {
                            width: stroke.width,
                            ..Default::default()
                        },
                        t,
                        mask,
                    );
                }
            }
            Shape::Path {
                points,
                closed,
                stroke,
            } => {
                if let Some(p) = polyline(points, *closed) {
                    let s = SkStroke {
                        width: stroke.width,
                        line_cap: tiny_skia::LineCap::Round,
                        line_join: tiny_skia::LineJoin::Round,
                        ..Default::default()
                    };
                    pm.stroke_path(&p, &paint(stroke.color), &s, t, mask);
                }
            }
            Shape::Polygon { points, fill } => {
                if let Some(p) = polyline(points, true) {
                    pm.fill_path(&p, &paint(*fill), FillRule::Winding, t, mask);
                }
            }
            Shape::Text {
                pos,
                text,
                font,
                color,
                underline,
                strikethrough,
            } => {
                let w = draw_text(&mut pm, clip, scale, *pos, text, *font, *color);
                let line = |pm: &mut Pixmap, y: f32| {
                    if let Some(r) =
                        tiny_skia::Rect::from_xywh(pos.x, y, w, (font.size / 14.0).max(1.0))
                    {
                        pm.fill_rect(r, &paint(*color), t, mask);
                    }
                };
                if *underline {
                    line(&mut pm, pos.y + font.size * (ASCENT + 0.12));
                }
                if *strikethrough {
                    line(&mut pm, pos.y + font.size * (ASCENT - 0.3));
                }
            }
            Shape::Shadow {
                rect,
                radius,
                blur,
                color,
            } => {
                let steps = 6;
                for i in 0..steps {
                    let s = (i as f32 + 1.0) / steps as f32;
                    let grow = blur * 0.5 * s;
                    let alpha = 1.0 / steps as f32 * (1.0 - s * 0.6);
                    if let Some(p) =
                        rounded(rect.expand(grow - blur * 0.25), Radius::same(radius + grow))
                    {
                        pm.fill_path(
                            &p,
                            &paint(color.gamma_multiply(alpha)),
                            FillRule::Winding,
                            t,
                            None,
                        );
                    }
                }
            }
        }
    }
    pm
}

/// Blends glyph coverage into the pixmap; returns the text's width in DIPs.
fn draw_text(
    pm: &mut Pixmap,
    clip: Rect,
    scale: f32,
    pos: Pos2,
    text: &str,
    font: Font,
    color: Color32,
) -> f32 {
    let f = face(font);
    let sf = f.as_scaled(PxScale::from(font.size * scale * em_to_px(&f)));
    let [r, g, b, a] = color.to_unmultiplied_f32();
    let (cl, ct, cr, cb) = (
        (clip.left() * scale) as i32,
        (clip.top() * scale) as i32,
        (clip.right() * scale).ceil() as i32,
        (clip.bottom() * scale).ceil() as i32,
    );
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let baseline = (pos.y + font.size * ASCENT) * scale;
    let mut x = pos.x * scale;
    let data = pm.pixels_mut();
    for ch in text.chars() {
        let id = f.glyph_id(ch);
        let glyph = id.with_scale_and_position(sf.scale(), ab_glyph::point(x, baseline));
        if let Some(outline) = f.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < cl.max(0)
                    || py < ct.max(0)
                    || px >= cr.min(w)
                    || py >= cb.min(h)
                    || cov <= 0.0
                {
                    return;
                }
                let k = (cov.min(1.0) * a).clamp(0.0, 1.0);
                let i = (py * w + px) as usize;
                let d = data[i].demultiply();
                let mix = |s: f32, d: u8| {
                    ((s * 255.0) * k + f32::from(d) * (1.0 - k))
                        .round()
                        .clamp(0.0, 255.0) as u8
                };
                let out_a = (k * 255.0 + f32::from(d.alpha()) * (1.0 - k))
                    .round()
                    .clamp(0.0, 255.0) as u8;
                let c = tiny_skia::ColorU8::from_rgba(
                    mix(r, d.red()),
                    mix(g, d.green()),
                    mix(b, d.blue()),
                    out_a,
                );
                data[i] = c.premultiply();
            });
        }
        x += sf.h_advance(id);
    }
    x / scale - pos.x
}
