//! The display list a frame produces: shapes in layers, each clipped. The
//! Direct2D renderer and the software (snapshot) renderer both replay it.

use crate::geom::{Color32, Pos2, Radius, Rect, Stroke, Vec2};
use crate::text::Font;

#[derive(Clone, Debug)]
pub enum Shape {
    Rect {
        rect: Rect,
        radius: Radius,
        fill: Color32,
        /// Drawn inside the rectangle's edge.
        stroke: Stroke,
    },
    Circle {
        center: Pos2,
        radius: f32,
        fill: Color32,
        stroke: Stroke,
    },
    Ellipse {
        center: Pos2,
        radii: Vec2,
        stroke: Stroke,
    },
    Path {
        points: Vec<Pos2>,
        closed: bool,
        stroke: Stroke,
    },
    /// A filled convex polygon.
    Polygon { points: Vec<Pos2>, fill: Color32 },
    /// One line of text; `pos` is the top-left of its line box.
    Text {
        pos: Pos2,
        text: String,
        font: Font,
        color: Color32,
        underline: bool,
        strikethrough: bool,
    },
    /// A soft drop shadow under a rounded rectangle.
    Shadow {
        rect: Rect,
        radius: f32,
        blur: f32,
        color: Color32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Background = 0,
    /// Floating controls over content, such as Run selection.
    Middle = 1,
    /// Dialog backdrops and windows.
    Modal = 2,
    /// Menus, combo lists, completion, toasts: above dialogs.
    Popup = 3,
    Tooltip = 4,
}

pub const LAYERS: usize = 5;

#[derive(Clone, Debug)]
pub struct ClippedShape {
    pub clip: Rect,
    pub shape: Shape,
}

#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub layers: [Vec<ClippedShape>; LAYERS],
}

impl DisplayList {
    pub fn iter(&self) -> impl Iterator<Item = &ClippedShape> {
        self.layers.iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.layers.iter().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every text run, in paint order; tests read the screen through it.
    pub fn texts(&self) -> Vec<&str> {
        self.iter()
            .filter_map(|s| match &s.shape {
                Shape::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Paints into one layer of the frame's display list with a clip and an
/// opacity applied to every colour.
pub struct Painter<'a> {
    pub(crate) list: &'a mut DisplayList,
    pub(crate) layer: Layer,
    pub(crate) clip: Rect,
    pub(crate) opacity: f32,
    pub(crate) fonts: &'a crate::text::Fonts,
}

impl<'a> Painter<'a> {
    fn fade(&self, c: Color32) -> Color32 {
        if self.opacity >= 1.0 {
            c
        } else {
            c.gamma_multiply(self.opacity)
        }
    }

    pub fn clip_rect(&self) -> Rect {
        self.clip
    }

    pub fn fonts(&self) -> &'a crate::text::Fonts {
        self.fonts
    }

    pub fn add(&mut self, shape: Shape) {
        let shape = match shape {
            Shape::Rect {
                rect,
                radius,
                fill,
                stroke,
            } => Shape::Rect {
                rect,
                radius,
                fill: self.fade(fill),
                stroke: Stroke::new(stroke.width, self.fade(stroke.color)),
            },
            Shape::Circle {
                center,
                radius,
                fill,
                stroke,
            } => Shape::Circle {
                center,
                radius,
                fill: self.fade(fill),
                stroke: Stroke::new(stroke.width, self.fade(stroke.color)),
            },
            Shape::Ellipse {
                center,
                radii,
                stroke,
            } => Shape::Ellipse {
                center,
                radii,
                stroke: Stroke::new(stroke.width, self.fade(stroke.color)),
            },
            Shape::Path {
                points,
                closed,
                stroke,
            } => Shape::Path {
                points,
                closed,
                stroke: Stroke::new(stroke.width, self.fade(stroke.color)),
            },
            Shape::Polygon { points, fill } => Shape::Polygon {
                points,
                fill: self.fade(fill),
            },
            Shape::Text {
                pos,
                text,
                font,
                color,
                underline,
                strikethrough,
            } => Shape::Text {
                pos,
                text,
                font,
                color: self.fade(color),
                underline,
                strikethrough,
            },
            Shape::Shadow {
                rect,
                radius,
                blur,
                color,
            } => Shape::Shadow {
                rect,
                radius,
                blur,
                color: self.fade(color),
            },
        };
        let bounds = shape_bounds(&shape);
        if !bounds.intersects(self.clip) {
            return;
        }
        self.list.layers[self.layer as usize].push(ClippedShape {
            clip: self.clip,
            shape,
        });
    }

    pub fn rect_filled(&mut self, rect: Rect, radius: impl Into<Radius>, fill: Color32) {
        if fill.a == 0 {
            return;
        }
        self.add(Shape::Rect {
            rect,
            radius: radius.into(),
            fill,
            stroke: Stroke::NONE,
        });
    }

    pub fn rect(&mut self, rect: Rect, radius: impl Into<Radius>, fill: Color32, stroke: Stroke) {
        self.add(Shape::Rect {
            rect,
            radius: radius.into(),
            fill,
            stroke,
        });
    }

    pub fn rect_stroke(&mut self, rect: Rect, radius: impl Into<Radius>, stroke: Stroke) {
        self.add(Shape::Rect {
            rect,
            radius: radius.into(),
            fill: Color32::TRANSPARENT,
            stroke,
        });
    }

    pub fn circle_filled(&mut self, center: Pos2, radius: f32, fill: Color32) {
        self.add(Shape::Circle {
            center,
            radius,
            fill,
            stroke: Stroke::NONE,
        });
    }

    pub fn circle_stroke(&mut self, center: Pos2, radius: f32, stroke: Stroke) {
        self.add(Shape::Circle {
            center,
            radius,
            fill: Color32::TRANSPARENT,
            stroke,
        });
    }

    pub fn line(&mut self, points: Vec<Pos2>, stroke: Stroke) {
        self.add(Shape::Path {
            points,
            closed: false,
            stroke,
        });
    }

    pub fn line_segment(&mut self, [a, b]: [Pos2; 2], stroke: Stroke) {
        self.line(vec![a, b], stroke);
    }

    /// A 1 px horizontal rule at `y` (crisp when `y` is on a half pixel).
    pub fn hline(&mut self, x: std::ops::RangeInclusive<f32>, y: f32, stroke: Stroke) {
        let w = stroke.width;
        self.rect_filled(
            Rect::from_ltrb(*x.start(), y - w / 2.0, *x.end(), y + w / 2.0),
            0.0,
            stroke.color,
        );
    }

    pub fn vline(&mut self, x: f32, y: std::ops::RangeInclusive<f32>, stroke: Stroke) {
        let w = stroke.width;
        self.rect_filled(
            Rect::from_ltrb(x - w / 2.0, *y.start(), x + w / 2.0, *y.end()),
            0.0,
            stroke.color,
        );
    }

    pub fn shadow(&mut self, rect: Rect, radius: f32, offset: Vec2, blur: f32, color: Color32) {
        self.add(Shape::Shadow {
            rect: rect.translate(offset),
            radius,
            blur,
            color,
        });
    }

    /// A single line of text with its line box's top-left at `pos`.
    pub fn text(&mut self, pos: Pos2, text: impl Into<String>, font: Font, color: Color32) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.add(Shape::Text {
            pos,
            text,
            font,
            color,
            underline: false,
            strikethrough: false,
        });
    }

    pub fn text_styled(
        &mut self,
        pos: Pos2,
        text: impl Into<String>,
        font: Font,
        color: Color32,
        underline: bool,
        strikethrough: bool,
    ) {
        self.add(Shape::Text {
            pos,
            text: text.into(),
            font,
            color,
            underline,
            strikethrough,
        });
    }

    /// Text centred vertically on `y`, starting at `x`. Returns its width.
    pub fn text_mid(&mut self, x: f32, y: f32, text: &str, font: Font, color: Color32) -> f32 {
        let w = self.fonts.width(text, font);
        self.text(
            Pos2 {
                x,
                y: y - font.line_height() / 2.0,
            },
            text,
            font,
            color,
        );
        w
    }

    /// Text truncated with an ellipsis to `max_width`, vertically centred on
    /// `y`. Returns (width, elided).
    pub fn text_truncated(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        font: Font,
        color: Color32,
        max_width: f32,
    ) -> (f32, bool) {
        let (shown, elided) = self.fonts.truncate(text, font, max_width.max(0.0));
        let w = self.text_mid(x, y, &shown, font, color);
        (w, elided)
    }

    /// Narrows the clip; returns a painter for the same layer.
    pub fn with_clip(&mut self, clip: Rect) -> Painter<'_> {
        Painter {
            list: self.list,
            layer: self.layer,
            clip: self.clip.intersect(clip),
            opacity: self.opacity,
            fonts: self.fonts,
        }
    }
}

fn shape_bounds(shape: &Shape) -> Rect {
    match shape {
        Shape::Rect { rect, stroke, .. } => rect.expand(stroke.width),
        Shape::Circle {
            center,
            radius,
            stroke,
            ..
        } => Rect::from_center_size(*center, Vec2::splat((radius + stroke.width) * 2.0)),
        Shape::Ellipse {
            center,
            radii,
            stroke,
        } => Rect::from_center_size(*center, (*radii + Vec2::splat(stroke.width)) * 2.0),
        Shape::Path { points, stroke, .. } => points
            .iter()
            .fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)))
            .expand(stroke.width),
        Shape::Polygon { points, .. } => points
            .iter()
            .fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p))),
        Shape::Text { pos, font, .. } => {
            // Generous: the run's width isn't known here.
            Rect::from_min_size(
                *pos,
                Vec2 {
                    x: 1e6,
                    y: font.line_height(),
                },
            )
        }
        Shape::Shadow { rect, blur, .. } => rect.expand(*blur),
    }
}
