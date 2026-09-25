//! Stroke icons painted on a 16 px grid, matching the 1.6 px glyphs in the
//! desktop app's `Icon.tsx` without depending on font coverage.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Vec2};

use crate::theme;

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
    SortUp,
    SortDown,
    Folder,
    Trash,
    Undo,
    Filter,
}

/// Paints `icon` centered in `rect` using a 16 px design grid.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let scale = rect.width().min(rect.height()) / 16.0;
    let origin = rect.center() - Vec2::splat(8.0 * scale);
    let p = |x: f32, y: f32| Pos2::new(origin.x + x * scale, origin.y + y * scale);
    let stroke = Stroke::new(1.5 * scale.max(0.75), color);
    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], stroke);
    };
    let path = |points: Vec<Pos2>| {
        painter.add(Shape::line(points, stroke));
    };
    match icon {
        Icon::Plus => {
            line(p(8.0, 3.0), p(8.0, 13.0));
            line(p(3.0, 8.0), p(13.0, 8.0));
        }
        Icon::More => {
            for x in [3.5, 8.0, 12.5] {
                painter.circle_filled(p(x, 8.0), 1.2 * scale, color);
            }
        }
        Icon::Database => {
            painter.add(egui::epaint::EllipseShape::stroke(
                p(8.0, 4.0),
                Vec2::new(5.0, 1.8) * scale,
                stroke,
            ));
            line(p(3.0, 4.0), p(3.0, 12.0));
            line(p(13.0, 4.0), p(13.0, 12.0));
            path(arc(p(8.0, 8.0), 5.0 * scale, 1.8 * scale));
            path(arc(p(8.0, 12.0), 5.0 * scale, 1.8 * scale));
        }
        Icon::Refresh => {
            let center = p(8.0, 8.0);
            let radius = 4.8 * scale;
            let points: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let angle = (-40.0 + i as f32 * 14.0_f32).to_radians();
                    center + Vec2::angled(angle) * radius
                })
                .collect();
            let end = *points.last().unwrap_or(&center);
            path(points);
            line(end, end + Vec2::new(0.4, -3.0) * scale);
            line(end, end + Vec2::new(3.0, -0.4) * scale);
        }
        Icon::Table => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.5, 3.0), p(13.5, 13.0)),
                1.5 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(p(2.5, 6.5), p(13.5, 6.5));
            line(p(6.5, 6.5), p(6.5, 13.0));
        }
        Icon::Key => {
            painter.circle_stroke(p(5.5, 8.0), 2.6 * scale, stroke);
            line(p(8.1, 8.0), p(13.5, 8.0));
            line(p(11.5, 8.0), p(11.5, 10.5));
            line(p(13.5, 8.0), p(13.5, 10.0));
        }
        Icon::Filter => {
            painter.add(Shape::closed_line(
                vec![
                    p(2.0, 3.3),
                    p(14.0, 3.3),
                    p(9.3, 8.7),
                    p(9.3, 12.7),
                    p(6.7, 11.3),
                    p(6.7, 8.7),
                ],
                stroke,
            ));
        }
        Icon::Trash => {
            line(p(3.0, 4.5), p(13.0, 4.5));
            path(vec![p(6.5, 4.5), p(6.5, 3.0), p(9.5, 3.0), p(9.5, 4.5)]);
            path(vec![p(4.5, 4.5), p(5.2, 13.0), p(10.8, 13.0), p(11.5, 4.5)]);
        }
        Icon::Undo => {
            path(vec![p(5.5, 3.5), p(3.0, 6.0), p(5.5, 8.5)]);
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [p(3.0, 6.0), p(12.0, 6.0), p(14.0, 13.0), p(8.0, 13.0)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
        }
        Icon::Folder => {
            painter.add(Shape::closed_line(
                vec![
                    p(2.0, 4.0),
                    p(6.0, 4.0),
                    p(7.5, 5.5),
                    p(14.0, 5.5),
                    p(14.0, 12.5),
                    p(2.0, 12.5),
                ],
                stroke,
            ));
        }
        Icon::View => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.5, 3.0), p(13.5, 13.0)),
                1.5 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(p(2.5, 6.5), p(13.5, 6.5));
            painter.circle_stroke(p(8.0, 9.8), 1.6 * scale, stroke);
        }
        Icon::ChevronRight => {
            path(vec![p(6.0, 4.0), p(10.0, 8.0), p(6.0, 12.0)]);
        }
        Icon::ChevronLeft => {
            path(vec![p(10.0, 4.0), p(6.0, 8.0), p(10.0, 12.0)]);
        }
        Icon::ChevronUp => {
            path(vec![p(4.0, 10.0), p(8.0, 6.0), p(12.0, 10.0)]);
        }
        Icon::ChevronDown => {
            path(vec![p(4.0, 6.0), p(8.0, 10.0), p(12.0, 6.0)]);
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![p(5.0, 3.5), p(12.5, 8.0), p(5.0, 12.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Close => {
            line(p(4.5, 4.5), p(11.5, 11.5));
            line(p(11.5, 4.5), p(4.5, 11.5));
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(p(5.5, 5.5), p(13.0, 13.0)),
                1.5 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            path(vec![p(3.0, 10.5), p(3.0, 3.0), p(10.5, 3.0)]);
        }
        Icon::Download => {
            line(p(8.0, 2.5), p(8.0, 10.0));
            path(vec![p(4.5, 7.0), p(8.0, 10.5), p(11.5, 7.0)]);
            line(p(3.0, 13.0), p(13.0, 13.0));
        }
        Icon::Pencil => {
            path(vec![
                p(3.0, 13.0),
                p(3.6, 10.2),
                p(10.8, 3.0),
                p(13.0, 5.2),
                p(5.8, 12.4),
                p(3.0, 13.0),
            ]);
            line(p(9.3, 4.5), p(11.5, 6.7));
        }
        Icon::Expand => {
            line(p(3.0, 8.0), p(13.0, 8.0));
            path(vec![p(10.0, 5.0), p(13.0, 8.0), p(10.0, 11.0)]);
            line(p(3.0, 4.0), p(3.0, 12.0));
        }
        Icon::Collapse => {
            line(p(3.0, 8.0), p(13.0, 8.0));
            path(vec![p(6.0, 5.0), p(3.0, 8.0), p(6.0, 11.0)]);
            line(p(13.0, 4.0), p(13.0, 12.0));
        }
        Icon::Sidebar => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.5, 3.0), p(13.5, 13.0)),
                1.5 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(p(6.5, 3.0), p(6.5, 13.0));
        }
        Icon::Inspector => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.5, 3.0), p(13.5, 13.0)),
                1.5 * scale,
                stroke,
                egui::StrokeKind::Middle,
            );
            line(p(9.5, 3.0), p(9.5, 13.0));
        }
        Icon::Check => {
            path(vec![p(3.5, 8.5), p(6.5, 11.5), p(12.5, 4.5)]);
        }
        Icon::Alert => {
            painter.circle_stroke(p(8.0, 8.0), 5.5 * scale, stroke);
            line(p(8.0, 5.0), p(8.0, 8.8));
            painter.circle_filled(p(8.0, 11.0), 0.9 * scale, color);
        }
        Icon::Code => {
            path(vec![p(6.0, 4.5), p(2.5, 8.0), p(6.0, 11.5)]);
            path(vec![p(10.0, 4.5), p(13.5, 8.0), p(10.0, 11.5)]);
        }
        Icon::SortUp => {
            painter.add(Shape::convex_polygon(
                vec![p(8.0, 5.0), p(11.5, 10.5), p(4.5, 10.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::SortDown => {
            painter.add(Shape::convex_polygon(
                vec![p(4.5, 5.5), p(11.5, 5.5), p(8.0, 11.0)],
                color,
                Stroke::NONE,
            ));
        }
    }
}

/// The lower half of an ellipse, for the database glyph's bands.
fn arc(center: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..=16)
        .map(|i| {
            let t = std::f32::consts::PI * i as f32 / 16.0;
            Pos2::new(center.x - rx * t.cos(), center.y + ry * t.sin())
        })
        .collect()
}

/// A non-interactive icon occupying `size`.
pub fn show(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint(ui.painter(), rect, icon, color);
    response
}

/// A transparent toolbar button with an icon and optional label. The label
/// (or tooltip) doubles as the accessible name.
pub fn button(ui: &mut egui::Ui, icon: Icon, label: Option<&str>, tooltip: &str) -> Response {
    let enabled = ui.is_enabled();
    let text = label.map(|l| {
        ui.painter().layout_no_wrap(
            l.to_owned(),
            egui::FontId::proportional(12.5),
            theme::SECONDARY,
        )
    });
    let icon_size = 14.0;
    let padding = Vec2::new(7.0, 5.0);
    let width = padding.x * 2.0 + icon_size + text.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
    let response = response.on_hover_text(tooltip);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label.unwrap_or(tooltip))
    });
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && enabled;
        if hovered || response.has_focus() {
            ui.painter().rect_filled(rect, 6, theme::CONTROL_HOVER);
        }
        let color = if !enabled {
            theme::FAINT
        } else if hovered {
            theme::TEXT_STRONG
        } else {
            theme::SECONDARY
        };
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + padding.x + icon_size / 2.0, rect.center().y),
            Vec2::splat(icon_size),
        );
        paint(ui.painter(), icon_rect, icon, color);
        if let Some(galley) = text {
            let pos = Pos2::new(
                icon_rect.right() + 6.0,
                rect.center().y - galley.size().y / 2.0,
            );
            ui.painter().galley(pos, galley, color);
        }
    }
    response
}
