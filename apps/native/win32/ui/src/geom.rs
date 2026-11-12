//! Points, sizes, rectangles and colours in device-independent pixels (DIPs).

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pos2 {
    pub x: f32,
    pub y: f32,
}

pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

pub const fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2 { x, y }
}

impl Vec2 {
    pub const ZERO: Self = vec2(0.0, 0.0);

    pub const fn splat(v: f32) -> Self {
        vec2(v, v)
    }

    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub fn max(self, other: Self) -> Self {
        vec2(self.x.max(other.x), self.y.max(other.y))
    }

    pub fn min(self, other: Self) -> Self {
        vec2(self.x.min(other.x), self.y.min(other.y))
    }
}

impl Pos2 {
    pub const ZERO: Self = pos2(0.0, 0.0);

    pub fn to_vec2(self) -> Vec2 {
        vec2(self.x, self.y)
    }

    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        vec2(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        vec2(self.x - o.x, self.y - o.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        vec2(-self.x, -self.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        vec2(self.x * s, self.y * s)
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;
    fn mul(self, v: Vec2) -> Vec2 {
        v * self
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl Add<Vec2> for Pos2 {
    type Output = Pos2;
    fn add(self, o: Vec2) -> Pos2 {
        pos2(self.x + o.x, self.y + o.y)
    }
}

impl Sub<Vec2> for Pos2 {
    type Output = Pos2;
    fn sub(self, o: Vec2) -> Pos2 {
        pos2(self.x - o.x, self.y - o.y)
    }
}

impl Sub for Pos2 {
    type Output = Vec2;
    fn sub(self, o: Pos2) -> Vec2 {
        vec2(self.x - o.x, self.y - o.y)
    }
}

impl AddAssign<Vec2> for Pos2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl SubAssign<Vec2> for Pos2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Pos2,
    pub max: Pos2,
}

impl Rect {
    pub const NOTHING: Self = Rect {
        min: pos2(f32::INFINITY, f32::INFINITY),
        max: pos2(f32::NEG_INFINITY, f32::NEG_INFINITY),
    };
    pub const EVERYTHING: Self = Rect {
        min: pos2(-1e9, -1e9),
        max: pos2(1e9, 1e9),
    };

    pub const fn from_min_max(min: Pos2, max: Pos2) -> Self {
        Rect { min, max }
    }

    pub fn from_min_size(min: Pos2, size: Vec2) -> Self {
        Rect {
            min,
            max: min + size,
        }
    }

    pub fn from_center_size(center: Pos2, size: Vec2) -> Self {
        Rect::from_min_size(center - size * 0.5, size)
    }

    pub fn from_ltrb(l: f32, t: f32, r: f32, b: f32) -> Self {
        Rect::from_min_max(pos2(l, t), pos2(r, b))
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn left(&self) -> f32 {
        self.min.x
    }

    pub fn right(&self) -> f32 {
        self.max.x
    }

    pub fn top(&self) -> f32 {
        self.min.y
    }

    pub fn bottom(&self) -> f32 {
        self.max.y
    }

    pub fn center(&self) -> Pos2 {
        pos2(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }

    pub fn left_center(&self) -> Pos2 {
        pos2(self.min.x, self.center().y)
    }

    pub fn right_center(&self) -> Pos2 {
        pos2(self.max.x, self.center().y)
    }

    pub fn left_top(&self) -> Pos2 {
        self.min
    }

    pub fn right_top(&self) -> Pos2 {
        pos2(self.max.x, self.min.y)
    }

    pub fn left_bottom(&self) -> Pos2 {
        pos2(self.min.x, self.max.y)
    }

    pub fn right_bottom(&self) -> Pos2 {
        self.max
    }

    pub fn contains(&self, p: Pos2) -> bool {
        p.x >= self.min.x && p.x < self.max.x && p.y >= self.min.y && p.y < self.max.y
    }

    pub fn is_positive(&self) -> bool {
        self.max.x > self.min.x && self.max.y > self.min.y
    }

    pub fn intersects(&self, o: Rect) -> bool {
        self.min.x < o.max.x && o.min.x < self.max.x && self.min.y < o.max.y && o.min.y < self.max.y
    }

    pub fn intersect(&self, o: Rect) -> Rect {
        Rect::from_ltrb(
            self.min.x.max(o.min.x),
            self.min.y.max(o.min.y),
            self.max.x.min(o.max.x),
            self.max.y.min(o.max.y),
        )
    }

    pub fn union(&self, o: Rect) -> Rect {
        Rect::from_ltrb(
            self.min.x.min(o.min.x),
            self.min.y.min(o.min.y),
            self.max.x.max(o.max.x),
            self.max.y.max(o.max.y),
        )
    }

    pub fn shrink(&self, v: f32) -> Rect {
        self.shrink2(Vec2::splat(v))
    }

    pub fn shrink2(&self, v: Vec2) -> Rect {
        Rect::from_min_max(self.min + v, self.max - v)
    }

    pub fn expand(&self, v: f32) -> Rect {
        self.shrink(-v)
    }

    pub fn translate(&self, v: Vec2) -> Rect {
        Rect::from_min_max(self.min + v, self.max + v)
    }

    /// Insets each side: left, top, right, bottom.
    pub fn inset(&self, l: f32, t: f32, r: f32, b: f32) -> Rect {
        Rect::from_ltrb(
            self.min.x + l,
            self.min.y + t,
            self.max.x - r,
            self.max.y - b,
        )
    }

    pub fn with_min_x(mut self, x: f32) -> Rect {
        self.min.x = x;
        self
    }

    pub fn with_max_x(mut self, x: f32) -> Rect {
        self.max.x = x;
        self
    }

    pub fn with_min_y(mut self, y: f32) -> Rect {
        self.min.y = y;
        self
    }

    pub fn with_max_y(mut self, y: f32) -> Rect {
        self.max.y = y;
        self
    }

    /// Cuts `w` off the left: (left part, rest).
    pub fn split_left(&self, w: f32) -> (Rect, Rect) {
        let x = (self.min.x + w).min(self.max.x);
        (self.with_max_x(x), self.with_min_x(x))
    }

    /// Cuts `w` off the right: (right part, rest).
    pub fn split_right(&self, w: f32) -> (Rect, Rect) {
        let x = (self.max.x - w).max(self.min.x);
        (self.with_min_x(x), self.with_max_x(x))
    }

    /// Cuts `h` off the top: (top part, rest).
    pub fn split_top(&self, h: f32) -> (Rect, Rect) {
        let y = (self.min.y + h).min(self.max.y);
        (self.with_max_y(y), self.with_min_y(y))
    }

    /// Cuts `h` off the bottom: (bottom part, rest).
    pub fn split_bottom(&self, h: f32) -> (Rect, Rect) {
        let y = (self.max.y - h).max(self.min.y);
        (self.with_min_y(y), self.with_max_y(y))
    }
}

/// A colour with premultiplied alpha, as egui's `Color32`, so the Graphite
/// tokens carry over value for value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color32 {
    pub const TRANSPARENT: Self = Color32::from_rgba_premultiplied(0, 0, 0, 0);
    pub const WHITE: Self = Color32::from_rgb(255, 255, 255);
    pub const BLACK: Self = Color32::from_rgb(0, 0, 0);

    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color32 { r, g, b, a: 255 }
    }

    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color32 { r, g, b, a }
    }

    pub fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        let m = |c: u8| ((c as u32 * a as u32 + 127) / 255) as u8;
        Color32 {
            r: m(r),
            g: m(g),
            b: m(b),
            a,
        }
    }

    pub const fn from_white_alpha(a: u8) -> Self {
        Color32 {
            r: a,
            g: a,
            b: a,
            a,
        }
    }

    pub const fn from_black_alpha(a: u8) -> Self {
        Color32 {
            r: 0,
            g: 0,
            b: 0,
            a,
        }
    }

    pub fn from_hex(v: u32) -> Self {
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Scales every premultiplied channel: fades the colour by `f`.
    pub fn gamma_multiply(self, f: f32) -> Self {
        let f = f.clamp(0.0, 1.0);
        let m = |c: u8| (c as f32 * f + 0.5) as u8;
        Color32 {
            r: m(self.r),
            g: m(self.g),
            b: m(self.b),
            a: m(self.a),
        }
    }

    pub fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Straight-alpha channels in 0..=1, for renderers.
    pub fn to_unmultiplied_f32(self) -> [f32; 4] {
        if self.a == 0 {
            return [0.0; 4];
        }
        let a = self.a as f32 / 255.0;
        [
            (self.r as f32 / 255.0 / a).min(1.0),
            (self.g as f32 / 255.0 / a).min(1.0),
            (self.b as f32 / 255.0 / a).min(1.0),
            a,
        ]
    }

    /// Linear blend of two opaque-ish colours, `t` from `self` to `other`.
    pub fn lerp(self, other: Color32, t: f32) -> Color32 {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color32 {
            r: l(self.r, other.r),
            g: l(self.g, other.g),
            b: l(self.b, other.b),
            a: l(self.a, other.a),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: Color32,
}

impl Stroke {
    pub const NONE: Stroke = Stroke {
        width: 0.0,
        color: Color32::TRANSPARENT,
    };

    pub fn new(width: f32, color: Color32) -> Self {
        Stroke { width, color }
    }

    pub fn is_none(&self) -> bool {
        self.width <= 0.0 || self.color.a == 0
    }
}

/// Corner radii: north-west, north-east, south-west, south-east.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Radius {
    pub nw: f32,
    pub ne: f32,
    pub sw: f32,
    pub se: f32,
}

impl Radius {
    pub const ZERO: Radius = Radius::same(0.0);

    pub const fn same(r: f32) -> Self {
        Radius {
            nw: r,
            ne: r,
            sw: r,
            se: r,
        }
    }

    pub const fn top(r: f32) -> Self {
        Radius {
            nw: r,
            ne: r,
            sw: 0.0,
            se: 0.0,
        }
    }

    pub const fn bottom(r: f32) -> Self {
        Radius {
            nw: 0.0,
            ne: 0.0,
            sw: r,
            se: r,
        }
    }

    pub fn is_uniform(&self) -> bool {
        self.nw == self.ne && self.ne == self.sw && self.sw == self.se
    }
}

impl From<f32> for Radius {
    fn from(r: f32) -> Self {
        Radius::same(r)
    }
}

impl From<i32> for Radius {
    fn from(r: i32) -> Self {
        Radius::same(r as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmultiplied_alpha_round_trips_through_premultiplied_storage() {
        let c = Color32::from_rgba_unmultiplied(76, 154, 255, 64);
        assert_eq!((c.r, c.g, c.b, c.a), (19, 39, 64, 64));
        let [r, g, b, a] = c.to_unmultiplied_f32();
        assert!((r - 76.0 / 255.0).abs() < 0.02);
        assert!((g - 154.0 / 255.0).abs() < 0.02);
        assert!((b - 1.0).abs() < 0.001);
        assert!((a - 64.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn splits_never_cross_the_opposite_edge() {
        let r = Rect::from_ltrb(0.0, 0.0, 100.0, 50.0);
        let (l, rest) = r.split_left(30.0);
        assert_eq!((l.right(), rest.left()), (30.0, 30.0));
        let (l, rest) = r.split_left(300.0);
        assert_eq!((l.right(), rest.width()), (100.0, 0.0));
        let (b, rest) = r.split_bottom(20.0);
        assert_eq!((b.top(), rest.bottom()), (30.0, 30.0));
    }
}
