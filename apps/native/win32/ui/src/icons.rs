//! Stroke icons on a 16 px grid, the same glyphs as the egui app's
//! `icons.rs` and the AppKit app's `Icons.swift`.

use crate::geom::{Color32, Pos2, Rect, Stroke, Vec2, pos2};
use crate::paint::{Painter, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Plus,
    More,
    Database,
    Refresh,
    Table,
    Key,
    View,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    Play,
    Close,
    Copy,
    Download,
    Inspector,
    Sidebar,
    Pencil,
    Expand,
    Collapse,
    Check,
    Alert,
    Code,
    Sort,
    ArrowUp,
    ArrowDown,
    ArrowRight,
    Search,
    Folder,
    Trash,
    Undo,
    Filter,
}

/// Paints `icon` centred in `rect`.
pub fn paint(painter: &mut Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 16.0;
    let origin = rect.center() - Vec2::splat(8.0 * scale);
    let p = |x: f32, y: f32| pos2(origin.x + x * scale, origin.y + y * scale);
    let stroke = Stroke::new(1.5 * scale.max(0.75), color);
    let line = |painter: &mut Painter, a: Pos2, b: Pos2| painter.line(vec![a, b], stroke);
    let path = |painter: &mut Painter, points: Vec<Pos2>| painter.line(points, stroke);
    let closed = |painter: &mut Painter, points: Vec<Pos2>| {
        painter.add(Shape::Path {
            points,
            closed: true,
            stroke,
        })
    };
    let frame = |painter: &mut Painter| {
        painter.rect_stroke(
            Rect::from_min_max(p(2.5, 3.0), p(13.5, 13.0)).expand(stroke.width / 2.0),
            1.5 * scale,
            stroke,
        )
    };
    match icon {
        Icon::Plus => {
            line(painter, p(8.0, 3.0), p(8.0, 13.0));
            line(painter, p(3.0, 8.0), p(13.0, 8.0));
        }
        Icon::More => {
            for x in [3.5, 8.0, 12.5] {
                painter.circle_filled(p(x, 8.0), 1.2 * scale, color);
            }
        }
        Icon::Database => {
            painter.add(Shape::Ellipse {
                center: p(8.0, 4.0),
                radii: Vec2 { x: 5.0, y: 1.8 } * scale,
                stroke,
            });
            line(painter, p(3.0, 4.0), p(3.0, 12.0));
            line(painter, p(13.0, 4.0), p(13.0, 12.0));
            path(painter, arc(p(8.0, 8.0), 5.0 * scale, 1.8 * scale));
            path(painter, arc(p(8.0, 12.0), 5.0 * scale, 1.8 * scale));
        }
        Icon::Refresh => {
            let center = p(8.0, 8.0);
            let radius = 4.8 * scale;
            let points: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = (-40.0 + i as f32 * 14.0_f32).to_radians();
                    center + Vec2 { x: a.cos(), y: a.sin() } * radius
                })
                .collect();
            let end = *points.last().unwrap_or(&center);
            path(painter, points);
            line(painter, end, end + Vec2 { x: 0.4, y: -3.0 } * scale);
            line(painter, end, end + Vec2 { x: 3.0, y: -0.4 } * scale);
        }
        Icon::Table => {
            frame(painter);
            line(painter, p(2.5, 6.5), p(13.5, 6.5));
            line(painter, p(6.5, 6.5), p(6.5, 13.0));
        }
        Icon::Key => {
            painter.circle_stroke(p(5.5, 8.0), 2.6 * scale, stroke);
            line(painter, p(8.1, 8.0), p(13.5, 8.0));
            line(painter, p(11.5, 8.0), p(11.5, 10.5));
            line(painter, p(13.5, 8.0), p(13.5, 10.0));
        }
        Icon::Filter => closed(
            painter,
            vec![p(2.0, 3.3), p(14.0, 3.3), p(9.3, 8.7), p(9.3, 12.7), p(6.7, 11.3), p(6.7, 8.7)],
        ),
        Icon::Trash => {
            line(painter, p(3.0, 4.5), p(13.0, 4.5));
            path(painter, vec![p(6.5, 4.5), p(6.5, 3.0), p(9.5, 3.0), p(9.5, 4.5)]);
            path(painter, vec![p(4.5, 4.5), p(5.2, 13.0), p(10.8, 13.0), p(11.5, 4.5)]);
        }
        Icon::Undo => {
            path(painter, vec![p(5.5, 3.5), p(3.0, 6.0), p(5.5, 8.5)]);
            path(painter, cubic([p(3.0, 6.0), p(12.0, 6.0), p(14.0, 13.0), p(8.0, 13.0)]));
        }
        Icon::Folder => closed(
            painter,
            vec![p(2.0, 4.0), p(6.0, 4.0), p(7.5, 5.5), p(14.0, 5.5), p(14.0, 12.5), p(2.0, 12.5)],
        ),
        Icon::View => {
            frame(painter);
            line(painter, p(2.5, 6.5), p(13.5, 6.5));
            painter.circle_stroke(p(8.0, 9.8), 1.6 * scale, stroke);
        }
        Icon::ChevronRight => path(painter, vec![p(6.0, 4.0), p(10.0, 8.0), p(6.0, 12.0)]),
        Icon::ChevronLeft => path(painter, vec![p(10.0, 4.0), p(6.0, 8.0), p(10.0, 12.0)]),
        Icon::ChevronUp => path(painter, vec![p(4.0, 10.0), p(8.0, 6.0), p(12.0, 10.0)]),
        Icon::ChevronDown => path(painter, vec![p(4.0, 6.0), p(8.0, 10.0), p(12.0, 6.0)]),
        Icon::Play => painter.add(Shape::Polygon {
            points: vec![p(5.0, 3.5), p(12.5, 8.0), p(5.0, 12.5)],
            fill: color,
        }),
        Icon::Close => {
            line(painter, p(4.5, 4.5), p(11.5, 11.5));
            line(painter, p(11.5, 4.5), p(4.5, 11.5));
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(p(5.5, 5.5), p(13.0, 13.0)).expand(stroke.width / 2.0),
                1.5 * scale,
                stroke,
            );
            path(painter, vec![p(3.0, 10.5), p(3.0, 3.0), p(10.5, 3.0)]);
        }
        Icon::Download => {
            line(painter, p(8.0, 2.5), p(8.0, 10.0));
            path(painter, vec![p(4.5, 7.0), p(8.0, 10.5), p(11.5, 7.0)]);
            line(painter, p(3.0, 13.0), p(13.0, 13.0));
        }
        Icon::Pencil => {
            path(painter, vec![p(3.0, 13.0), p(3.6, 10.2), p(10.8, 3.0), p(13.0, 5.2), p(5.8, 12.4), p(3.0, 13.0)]);
            line(painter, p(9.3, 4.5), p(11.5, 6.7));
        }
        Icon::Expand => {
            line(painter, p(3.0, 8.0), p(13.0, 8.0));
            path(painter, vec![p(10.0, 5.0), p(13.0, 8.0), p(10.0, 11.0)]);
            line(painter, p(3.0, 4.0), p(3.0, 12.0));
        }
        Icon::Collapse => {
            line(painter, p(3.0, 8.0), p(13.0, 8.0));
            path(painter, vec![p(6.0, 5.0), p(3.0, 8.0), p(6.0, 11.0)]);
            line(painter, p(13.0, 4.0), p(13.0, 12.0));
        }
        Icon::Sidebar => {
            frame(painter);
            line(painter, p(6.5, 3.0), p(6.5, 13.0));
        }
        Icon::Inspector => {
            frame(painter);
            line(painter, p(9.5, 3.0), p(9.5, 13.0));
        }
        Icon::Check => path(painter, vec![p(3.5, 8.5), p(6.5, 11.5), p(12.5, 4.5)]),
        Icon::Alert => {
            painter.circle_stroke(p(8.0, 8.0), 5.5 * scale, stroke);
            line(painter, p(8.0, 5.0), p(8.0, 8.8));
            painter.circle_filled(p(8.0, 11.0), 0.9 * scale, color);
        }
        Icon::Code => {
            path(painter, vec![p(6.0, 4.5), p(2.5, 8.0), p(6.0, 11.5)]);
            path(painter, vec![p(10.0, 4.5), p(13.5, 8.0), p(10.0, 11.5)]);
        }
        Icon::Sort => {
            path(painter, vec![p(4.67, 2.67), p(4.67, 13.33)]);
            path(painter, vec![p(2.67, 11.33), p(4.67, 13.33), p(6.67, 11.33)]);
            path(painter, vec![p(11.33, 13.33), p(11.33, 2.67)]);
            path(painter, vec![p(9.33, 4.67), p(11.33, 2.67), p(13.33, 4.67)]);
        }
        Icon::ArrowUp => {
            line(painter, p(8.0, 12.67), p(8.0, 3.33));
            path(painter, vec![p(4.0, 7.33), p(8.0, 3.33), p(12.0, 7.33)]);
        }
        Icon::Search => {
            painter.circle_stroke(p(6.75, 6.75), 3.75 * scale, stroke);
            line(painter, p(9.5, 9.5), p(13.0, 13.0));
        }
        Icon::ArrowRight => {
            line(painter, p(3.33, 8.0), p(12.67, 8.0));
            path(painter, vec![p(8.67, 4.0), p(12.67, 8.0), p(8.67, 12.0)]);
        }
        Icon::ArrowDown => {
            line(painter, p(8.0, 3.33), p(8.0, 12.67));
            path(painter, vec![p(4.0, 8.67), p(8.0, 12.67), p(12.0, 8.67)]);
        }
    }
}

/// The lower half of an ellipse, for the database glyph's bands.
fn arc(center: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..=16)
        .map(|i| {
            let t = std::f32::consts::PI * i as f32 / 16.0;
            pos2(center.x - rx * t.cos(), center.y + ry * t.sin())
        })
        .collect()
}

fn cubic([a, b, c, d]: [Pos2; 4]) -> Vec<Pos2> {
    (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            pos2(
                w[0] * a.x + w[1] * b.x + w[2] * c.x + w[3] * d.x,
                w[0] * a.y + w[1] * b.y + w[2] * c.y + w[3] * d.y,
            )
        })
        .collect()
}
