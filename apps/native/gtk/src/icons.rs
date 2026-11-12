//! Stroke icons drawn with cairo on a 16 px grid, the same glyphs as the
//! egui app's `icons.rs` and the AppKit `Icons.swift`. They take their color
//! from the widget's CSS `color`, so hover and disabled states come from CSS.

use std::f64::consts::{FRAC_PI_2, PI};

use gtk::cairo::Context;
use gtk::prelude::*;

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
    ArrowRight,
    Folder,
    Trash,
    Undo,
    Filter,
}

/// A widget showing `icon` at `size` px in its CSS color.
pub fn image(icon: Icon, size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .can_target(false)
        .css_classes(["icon"])
        .build();
    set(&area, icon);
    area
}

/// Replaces the glyph an `image` draws.
pub fn set(area: &gtk::DrawingArea, icon: Icon) {
    area.set_draw_func(move |area, cr, width, height| {
        let color = area.color();
        cr.set_source_rgba(
            color.red().into(),
            color.green().into(),
            color.blue().into(),
            color.alpha().into(),
        );
        paint(cr, f64::from(width), f64::from(height), icon);
    });
    area.queue_draw();
}

/// Paints `icon` centered in a `width` × `height` box with the current source.
pub fn paint(cr: &Context, width: f64, height: f64, icon: Icon) {
    let scale = width.min(height) / 16.0;
    cr.save().ok();
    cr.translate(width / 2.0 - 8.0 * scale, height / 2.0 - 8.0 * scale);
    cr.scale(scale, scale);
    // 1.5 px at 16 px, never thinner than 1.1 px at small sizes.
    cr.set_line_width(1.5 * scale.max(0.75) / scale);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    let line = |a: (f64, f64), b: (f64, f64)| {
        cr.move_to(a.0, a.1);
        cr.line_to(b.0, b.1);
        cr.stroke().ok();
    };
    let path = |points: &[(f64, f64)]| {
        if let Some((first, rest)) = points.split_first() {
            cr.move_to(first.0, first.1);
            for p in rest {
                cr.line_to(p.0, p.1);
            }
            cr.stroke().ok();
        }
    };
    let closed = |points: &[(f64, f64)]| {
        if let Some((first, rest)) = points.split_first() {
            cr.move_to(first.0, first.1);
            for p in rest {
                cr.line_to(p.0, p.1);
            }
            cr.close_path();
            cr.stroke().ok();
        }
    };
    let rounded = |x0: f64, y0: f64, x1: f64, y1: f64, r: f64| {
        cr.new_sub_path();
        cr.arc(x1 - r, y0 + r, r, -FRAC_PI_2, 0.0);
        cr.arc(x1 - r, y1 - r, r, 0.0, FRAC_PI_2);
        cr.arc(x0 + r, y1 - r, r, FRAC_PI_2, PI);
        cr.arc(x0 + r, y0 + r, r, PI, 1.5 * PI);
        cr.close_path();
        cr.stroke().ok();
    };
    let circle = |x: f64, y: f64, r: f64| {
        cr.new_sub_path();
        cr.arc(x, y, r, 0.0, 2.0 * PI);
        cr.stroke().ok();
    };
    let dot = |x: f64, y: f64, r: f64| {
        cr.new_sub_path();
        cr.arc(x, y, r, 0.0, 2.0 * PI);
        cr.fill().ok();
    };
    let half_ellipse = |cx: f64, cy: f64, rx: f64, ry: f64| {
        let points: Vec<(f64, f64)> = (0..=16)
            .map(|i| {
                let t = PI * f64::from(i) / 16.0;
                (cx - rx * t.cos(), cy + ry * t.sin())
            })
            .collect();
        path(&points);
    };
    match icon {
        Icon::Plus => {
            line((8.0, 3.0), (8.0, 13.0));
            line((3.0, 8.0), (13.0, 8.0));
        }
        Icon::More => {
            for x in [3.5, 8.0, 12.5] {
                dot(x, 8.0, 1.2);
            }
        }
        Icon::Database => {
            cr.save().ok();
            cr.translate(8.0, 4.0);
            cr.scale(5.0, 1.8);
            cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI);
            cr.restore().ok();
            cr.stroke().ok();
            line((3.0, 4.0), (3.0, 12.0));
            line((13.0, 4.0), (13.0, 12.0));
            half_ellipse(8.0, 8.0, 5.0, 1.8);
            half_ellipse(8.0, 12.0, 5.0, 1.8);
        }
        Icon::Refresh => {
            let points: Vec<(f64, f64)> = (0..=20)
                .map(|i| {
                    let angle = (-40.0 + f64::from(i) * 14.0).to_radians();
                    (8.0 + angle.cos() * 4.8, 8.0 + angle.sin() * 4.8)
                })
                .collect();
            let end = points[points.len() - 1];
            path(&points);
            line(end, (end.0 + 0.4, end.1 - 3.0));
            line(end, (end.0 + 3.0, end.1 - 0.4));
        }
        Icon::Table => {
            rounded(2.5, 3.0, 13.5, 13.0, 1.5);
            line((2.5, 6.5), (13.5, 6.5));
            line((6.5, 6.5), (6.5, 13.0));
        }
        Icon::Key => {
            circle(5.5, 8.0, 2.6);
            line((8.1, 8.0), (13.5, 8.0));
            line((11.5, 8.0), (11.5, 10.5));
            line((13.5, 8.0), (13.5, 10.0));
        }
        Icon::Filter => closed(&[
            (2.0, 3.3),
            (14.0, 3.3),
            (9.3, 8.7),
            (9.3, 12.7),
            (6.7, 11.3),
            (6.7, 8.7),
        ]),
        Icon::Trash => {
            line((3.0, 4.5), (13.0, 4.5));
            path(&[(6.5, 4.5), (6.5, 3.0), (9.5, 3.0), (9.5, 4.5)]);
            path(&[(4.5, 4.5), (5.2, 13.0), (10.8, 13.0), (11.5, 4.5)]);
        }
        Icon::Undo => {
            path(&[(5.5, 3.5), (3.0, 6.0), (5.5, 8.5)]);
            cr.move_to(3.0, 6.0);
            cr.curve_to(12.0, 6.0, 14.0, 13.0, 8.0, 13.0);
            cr.stroke().ok();
        }
        Icon::Folder => closed(&[
            (2.0, 4.0),
            (6.0, 4.0),
            (7.5, 5.5),
            (14.0, 5.5),
            (14.0, 12.5),
            (2.0, 12.5),
        ]),
        Icon::View => {
            rounded(2.5, 3.0, 13.5, 13.0, 1.5);
            line((2.5, 6.5), (13.5, 6.5));
            circle(8.0, 9.8, 1.6);
        }
        Icon::ChevronRight => path(&[(6.0, 4.0), (10.0, 8.0), (6.0, 12.0)]),
        Icon::ChevronLeft => path(&[(10.0, 4.0), (6.0, 8.0), (10.0, 12.0)]),
        Icon::ChevronUp => path(&[(4.0, 10.0), (8.0, 6.0), (12.0, 10.0)]),
        Icon::ChevronDown => path(&[(4.0, 6.0), (8.0, 10.0), (12.0, 6.0)]),
        Icon::Play => {
            cr.move_to(5.0, 3.5);
            cr.line_to(12.5, 8.0);
            cr.line_to(5.0, 12.5);
            cr.close_path();
            cr.fill().ok();
        }
        Icon::Close => {
            line((4.5, 4.5), (11.5, 11.5));
            line((11.5, 4.5), (4.5, 11.5));
        }
        Icon::Copy => {
            rounded(5.5, 5.5, 13.0, 13.0, 1.5);
            path(&[(3.0, 10.5), (3.0, 3.0), (10.5, 3.0)]);
        }
        Icon::Download => {
            line((8.0, 2.5), (8.0, 10.0));
            path(&[(4.5, 7.0), (8.0, 10.5), (11.5, 7.0)]);
            line((3.0, 13.0), (13.0, 13.0));
        }
        Icon::Pencil => {
            path(&[
                (3.0, 13.0),
                (3.6, 10.2),
                (10.8, 3.0),
                (13.0, 5.2),
                (5.8, 12.4),
                (3.0, 13.0),
            ]);
            line((9.3, 4.5), (11.5, 6.7));
        }
        Icon::Expand => {
            line((3.0, 8.0), (13.0, 8.0));
            path(&[(10.0, 5.0), (13.0, 8.0), (10.0, 11.0)]);
            line((3.0, 4.0), (3.0, 12.0));
        }
        Icon::Collapse => {
            line((3.0, 8.0), (13.0, 8.0));
            path(&[(6.0, 5.0), (3.0, 8.0), (6.0, 11.0)]);
            line((13.0, 4.0), (13.0, 12.0));
        }
        Icon::Sidebar => {
            rounded(2.5, 3.0, 13.5, 13.0, 1.5);
            line((6.5, 3.0), (6.5, 13.0));
        }
        Icon::Inspector => {
            rounded(2.5, 3.0, 13.5, 13.0, 1.5);
            line((9.5, 3.0), (9.5, 13.0));
        }
        Icon::Check => path(&[(3.5, 8.5), (6.5, 11.5), (12.5, 4.5)]),
        Icon::Alert => {
            circle(8.0, 8.0, 5.5);
            line((8.0, 5.0), (8.0, 8.8));
            dot(8.0, 11.0, 0.9);
        }
        Icon::Code => {
            path(&[(6.0, 4.5), (2.5, 8.0), (6.0, 11.5)]);
            path(&[(10.0, 4.5), (13.5, 8.0), (10.0, 11.5)]);
        }
        Icon::ArrowRight => {
            line((3.33, 8.0), (12.67, 8.0));
            path(&[(8.67, 4.0), (12.67, 8.0), (8.67, 12.0)]);
        }
    }
    cr.restore().ok();
}
