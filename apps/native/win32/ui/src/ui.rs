//! Layout: a `Ui` places widgets one after another in a rectangle, downward,
//! left to right or right to left, and hands out painters and responses.

use std::hash::Hash;

use crate::a11y::{Node, Role};
use crate::ctx::{Context, Id, Response, Sense};
use crate::geom::{Color32, Pos2, Radius, Rect, Stroke, Vec2, pos2, vec2};
use crate::paint::{Layer, Painter, Shape};
use crate::text::Font;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Down,
    Up,
    Right,
    Left,
}

pub struct Ui<'a> {
    pub ctx: &'a mut Context,
    max: Rect,
    cursor: Pos2,
    dir: Dir,
    pub spacing: Vec2,
    used: Rect,
    clip: Rect,
    layer: Layer,
    enabled: bool,
    opacity: f32,
    id: Id,
    a11y_parent: Option<u64>,
    centered: bool,
}

/// Margins: left, right, top, bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Margin {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Margin {
    pub const ZERO: Margin = Margin::same(0.0);

    pub const fn same(v: f32) -> Self {
        Margin {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }

    pub const fn symmetric(x: f32, y: f32) -> Self {
        Margin {
            left: x,
            right: x,
            top: y,
            bottom: y,
        }
    }

    pub const fn new(left: f32, right: f32, top: f32, bottom: f32) -> Self {
        Margin {
            left,
            right,
            top,
            bottom,
        }
    }

    pub fn sum(&self) -> Vec2 {
        vec2(self.left + self.right, self.top + self.bottom)
    }
}

/// A background box around content: fill, border, radius, padding.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub fill: Color32,
    pub stroke: Stroke,
    pub radius: Radius,
    pub inner: Margin,
    pub outer: Margin,
    pub shadow: Option<(Vec2, f32, Color32)>,
}

impl Frame {
    pub fn new() -> Self {
        Frame::default()
    }

    pub fn fill(mut self, c: Color32) -> Self {
        self.fill = c;
        self
    }

    pub fn stroke(mut self, s: Stroke) -> Self {
        self.stroke = s;
        self
    }

    pub fn radius(mut self, r: impl Into<Radius>) -> Self {
        self.radius = r.into();
        self
    }

    pub fn inner_margin(mut self, m: Margin) -> Self {
        self.inner = m;
        self
    }

    pub fn outer_margin(mut self, m: Margin) -> Self {
        self.outer = m;
        self
    }

    pub fn shadow(mut self, offset: Vec2, blur: f32, color: Color32) -> Self {
        self.shadow = Some((offset, blur, color));
        self
    }

    /// Paints the frame into `rect` (its outer edge, before the outer margin).
    pub fn paint(&self, ui: &mut Ui, rect: Rect) {
        let mut p = ui.painter();
        if let Some((offset, blur, color)) = self.shadow {
            p.shadow(rect, self.radius.nw, offset, blur, color);
        }
        p.rect(rect, self.radius, self.fill, self.stroke);
    }
}

impl<'a> Ui<'a> {
    pub fn root(ctx: &'a mut Context, rect: Rect, layer: Layer) -> Self {
        Ui {
            ctx,
            max: rect,
            cursor: rect.min,
            dir: Dir::Down,
            spacing: vec2(8.0, 4.0),
            used: Rect::from_min_max(rect.min, rect.min),
            clip: rect,
            layer,
            enabled: true,
            opacity: 1.0,
            id: Id::new("root"),
            a11y_parent: None,
            centered: false,
        }
    }

    /// A child in `rect` with its own direction; the parent is unchanged.
    pub fn child(&mut self, rect: Rect, dir: Dir) -> Ui<'_> {
        let cursor = match dir {
            Dir::Down | Dir::Right => rect.min,
            Dir::Left => rect.right_top(),
            Dir::Up => rect.left_bottom(),
        };
        Ui {
            ctx: self.ctx,
            max: rect,
            cursor,
            dir,
            spacing: self.spacing,
            used: Rect::from_min_max(cursor, cursor),
            clip: self.clip,
            layer: self.layer,
            enabled: self.enabled,
            opacity: self.opacity,
            id: self.id,
            a11y_parent: self.a11y_parent,
            centered: false,
        }
    }

    // ---- geometry ---------------------------------------------------------

    pub fn max_rect(&self) -> Rect {
        self.max
    }

    pub fn clip_rect(&self) -> Rect {
        self.clip
    }

    pub fn cursor(&self) -> Pos2 {
        self.cursor
    }

    /// The area used so far.
    pub fn min_rect(&self) -> Rect {
        self.used
    }

    pub fn available_rect(&self) -> Rect {
        match self.dir {
            Dir::Down => self.max.with_min_y(self.cursor.y),
            Dir::Up => self.max.with_max_y(self.cursor.y),
            Dir::Right => self.max.with_min_x(self.cursor.x),
            Dir::Left => self.max.with_max_x(self.cursor.x),
        }
    }

    pub fn available_width(&self) -> f32 {
        self.available_rect().width().max(0.0)
    }

    pub fn available_height(&self) -> f32 {
        self.available_rect().height().max(0.0)
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    pub fn id(&self) -> Id {
        self.id
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled &= enabled;
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity *= opacity;
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity outright and returns the previous one.
    pub fn replace_opacity(&mut self, opacity: f32) -> f32 {
        std::mem::replace(&mut self.opacity, opacity)
    }

    /// A position in this layer's paint order; `move_last_behind` puts a
    /// shape painted later (a background sized to its content) back here.
    pub fn paint_mark(&self) -> usize {
        self.ctx.list.layers[self.layer as usize].len()
    }

    /// Moves the most recently painted shapes, `count` of them, to `mark`.
    pub fn move_last_behind(&mut self, mark: usize, count: usize) {
        let layer = &mut self.ctx.list.layers[self.layer as usize];
        let len = layer.len();
        if count == 0 || mark + count > len {
            return;
        }
        let moved: Vec<_> = layer.drain(len - count..).collect();
        for (i, shape) in moved.into_iter().enumerate() {
            layer.insert(mark + i, shape);
        }
    }

    pub fn set_clip(&mut self, clip: Rect) {
        self.clip = self.clip.intersect(clip);
    }

    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    pub fn push_id(&mut self, salt: impl Hash) {
        self.id = self.id.with(salt);
    }

    pub fn make_id(&self, salt: impl Hash) -> Id {
        self.id.with(salt)
    }

    /// Centres items horizontally in a downward layout.
    pub fn set_centered(&mut self, centered: bool) {
        self.centered = centered;
    }

    /// Constrains the width of a downward layout (from the left).
    pub fn set_max_width(&mut self, width: f32) {
        let right = (self.max.left() + width).min(self.max.right());
        self.max.max.x = right;
    }

    pub fn rect_contains_pointer(&self, rect: Rect) -> bool {
        self.ctx
            .pointer()
            .is_some_and(|p| rect.intersect(self.clip).contains(p))
    }

    // ---- placement ----------------------------------------------------------

    /// Reserves `size` at the cursor and advances it.
    pub fn allocate(&mut self, size: Vec2) -> Rect {
        let rect = match self.dir {
            Dir::Down => {
                let x = if self.centered {
                    self.max.center().x - size.x / 2.0
                } else {
                    self.max.left()
                };
                let r = Rect::from_min_size(pos2(x, self.cursor.y), size);
                self.cursor.y += size.y + self.spacing.y;
                r
            }
            Dir::Up => {
                let r = Rect::from_min_size(pos2(self.max.left(), self.cursor.y - size.y), size);
                self.cursor.y -= size.y + self.spacing.y;
                r
            }
            Dir::Right => {
                let y = self.max.center().y - size.y / 2.0;
                let r = Rect::from_min_size(pos2(self.cursor.x, y), size);
                self.cursor.x += size.x + self.spacing.x;
                r
            }
            Dir::Left => {
                let y = self.max.center().y - size.y / 2.0;
                let r = Rect::from_min_size(pos2(self.cursor.x - size.x, y), size);
                self.cursor.x -= size.x + self.spacing.x;
                r
            }
        };
        self.used = self.used.union(rect);
        rect
    }

    pub fn allocate_response(&mut self, size: Vec2, id: Id, sense: Sense) -> (Rect, Response) {
        let rect = self.allocate(size);
        let r = self.interact(rect, id, sense);
        (rect, r)
    }

    /// The full available width (in a downward layout) at height `h`.
    pub fn allocate_row(&mut self, h: f32) -> Rect {
        let w = self.available_width();
        self.allocate(vec2(w, h))
    }

    pub fn add_space(&mut self, amount: f32) {
        match self.dir {
            Dir::Down => self.cursor.y += amount,
            Dir::Up => self.cursor.y -= amount,
            Dir::Right => self.cursor.x += amount,
            Dir::Left => self.cursor.x -= amount,
        }
    }

    /// Marks `rect` as used and moves the cursor past it.
    pub fn advance_past(&mut self, rect: Rect) {
        self.used = self.used.union(rect);
        match self.dir {
            Dir::Down => self.cursor.y = self.cursor.y.max(rect.bottom() + self.spacing.y),
            Dir::Up => self.cursor.y = self.cursor.y.min(rect.top() - self.spacing.y),
            Dir::Right => self.cursor.x = self.cursor.x.max(rect.right() + self.spacing.x),
            Dir::Left => self.cursor.x = self.cursor.x.min(rect.left() - self.spacing.x),
        }
    }

    // ---- nesting ----------------------------------------------------------

    /// A row of height `h` across the available width, laid out left to right.
    pub fn horizontal<R>(&mut self, h: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = match self.dir {
            Dir::Down | Dir::Up => self.allocate_row(h),
            _ => {
                let w = self.available_width();
                self.allocate(vec2(w, h))
            }
        };
        let mut child = self.child(rect, Dir::Right);
        f(&mut child)
    }

    /// A downward column in the remaining space of a horizontal layout,
    /// advancing past what it used.
    pub fn vertical<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = self.available_rect();
        let (inner, used) = {
            let mut child = self.child(rect, Dir::Down);
            let inner = f(&mut child);
            (inner, child.used)
        };
        self.advance_past(used);
        inner
    }

    /// The rest of a row, laid out from its right edge leftwards.
    pub fn right_to_left<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = self.available_rect();
        let (inner, used) = {
            let mut child = self.child(rect, Dir::Left);
            let inner = f(&mut child);
            (inner, child.used)
        };
        self.used = self.used.union(used);
        inner
    }

    /// The rest of a right-to-left row, laid out from its left edge.
    pub fn left_to_right<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = self.available_rect();
        let (inner, used) = {
            let mut child = self.child(rect, Dir::Right);
            let inner = f(&mut child);
            (inner, child.used)
        };
        self.used = self.used.union(used);
        inner
    }

    /// Runs `f` in the same place and direction; the parent advances past
    /// what it used. Settings changed inside stay inside.
    pub fn scope<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
        let rect = self.available_rect();
        let centered = self.centered;
        let (inner, used) = {
            let mut child = self.child(rect, self.dir);
            child.centered = centered;
            let inner = f(&mut child);
            (inner, child.used)
        };
        if used.is_positive() || used.width() > 0.0 || used.height() > 0.0 {
            self.advance_past(used);
        }
        (inner, used)
    }

    /// Content inside a frame: margins, then a background painted behind
    /// it once its size is known. In a downward layout the frame takes the
    /// full width.
    pub fn frame<R>(&mut self, frame: Frame, f: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
        let avail = self.available_rect();
        let outer = Rect::from_ltrb(
            avail.left() + frame.outer.left,
            avail.top() + frame.outer.top,
            avail.right() - frame.outer.right,
            avail.bottom() - frame.outer.bottom,
        );
        let inner_max = outer.inset(
            frame.inner.left,
            frame.inner.top,
            frame.inner.right,
            frame.inner.bottom,
        );
        let layer = self.layer as usize;
        let slot = self.ctx.list.layers[layer].len();
        self.ctx.list.layers[layer].push(crate::paint::ClippedShape {
            clip: Rect::NOTHING,
            shape: Shape::Rect {
                rect: Rect::NOTHING,
                radius: Radius::ZERO,
                fill: Color32::TRANSPARENT,
                stroke: Stroke::NONE,
            },
        });
        let (inner, used) = {
            let mut child = self.child(inner_max, Dir::Down);
            let inner = f(&mut child);
            (inner, child.used)
        };
        let full_width = matches!(self.dir, Dir::Down | Dir::Up);
        let content = if full_width {
            Rect::from_ltrb(
                inner_max.left(),
                inner_max.top(),
                inner_max.right(),
                used.bottom().max(inner_max.top()),
            )
        } else {
            Rect::from_ltrb(
                inner_max.left(),
                inner_max.top(),
                used.right().max(inner_max.left()),
                used.bottom().max(inner_max.top()),
            )
        };
        let rect = Rect::from_ltrb(
            content.left() - frame.inner.left,
            content.top() - frame.inner.top,
            content.right() + frame.inner.right,
            content.bottom() + frame.inner.bottom,
        );
        // Fill the reserved slot (and a shadow before it, if any).
        let fade = |c: Color32| c.gamma_multiply(self.opacity);
        self.ctx.list.layers[layer][slot] = crate::paint::ClippedShape {
            clip: self.clip,
            shape: Shape::Rect {
                rect,
                radius: frame.radius,
                fill: fade(frame.fill),
                stroke: Stroke::new(frame.stroke.width, fade(frame.stroke.color)),
            },
        };
        if let Some((offset, blur, color)) = frame.shadow {
            self.ctx.list.layers[layer].insert(
                slot,
                crate::paint::ClippedShape {
                    clip: Rect::EVERYTHING,
                    shape: Shape::Shadow {
                        rect: rect.translate(offset),
                        radius: frame.radius.nw,
                        blur,
                        color: fade(color),
                    },
                },
            );
        }
        let with_outer = Rect::from_ltrb(
            rect.left() - frame.outer.left,
            rect.top() - frame.outer.top,
            rect.right() + frame.outer.right,
            rect.bottom() + frame.outer.bottom,
        );
        self.advance_past(with_outer);
        (inner, rect)
    }

    /// Two columns side by side across the available width, `gap` apart;
    /// the parent advances past the taller one.
    pub fn columns2(&mut self, gap: f32, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
        let avail = self.available_rect();
        let w = (avail.width() - gap) / 2.0;
        let l = Rect::from_min_size(avail.min, vec2(w, avail.height()));
        let r = Rect::from_min_size(
            pos2(avail.left() + w + gap, avail.top()),
            vec2(w, avail.height()),
        );
        let used_l = {
            let mut c = self.child(l, Dir::Down);
            left(&mut c);
            c.used
        };
        let used_r = {
            let mut c = self.child(r, Dir::Down);
            right(&mut c);
            c.used
        };
        let bottom = used_l.bottom().max(used_r.bottom()).max(avail.top());
        self.advance_past(Rect::from_ltrb(
            avail.left(),
            avail.top(),
            avail.right(),
            bottom,
        ));
    }

    /// Groups the controls `f` adds under one accessibility node.
    pub fn a11y_group<R>(
        &mut self,
        id: Id,
        role: Role,
        name: &str,
        rect: Rect,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let mut node = Node::new(id.0, role, name, rect);
        node.parent = self.a11y_parent;
        self.ctx.a11y_node(node);
        let previous = self.a11y_parent.replace(id.0);
        let inner = f(self);
        self.a11y_parent = previous;
        inner
    }

    pub fn set_a11y_parent(&mut self, parent: Option<u64>) {
        self.a11y_parent = parent;
    }

    pub fn a11y_parent(&self) -> Option<u64> {
        self.a11y_parent
    }

    /// Records an accessibility node for a control under the current group.
    pub fn a11y(&mut self, mut node: Node) {
        node.parent = self.a11y_parent;
        node.enabled &= self.enabled;
        self.ctx.a11y_node(node);
    }

    // ---- interaction and painting ------------------------------------------

    pub fn interact(&mut self, rect: Rect, id: Id, sense: Sense) -> Response {
        let enabled = self.enabled;
        self.ctx
            .interact(id, rect, self.clip, self.layer, sense, enabled)
    }

    pub fn painter(&mut self) -> Painter<'_> {
        let (layer, clip, opacity) = (self.layer, self.clip, self.opacity);
        self.ctx.painter(layer, clip, opacity)
    }

    pub fn painter_at(&mut self, clip: Rect) -> Painter<'_> {
        let (layer, opacity) = (self.layer, self.opacity);
        let clip = self.clip.intersect(clip);
        self.ctx.painter(layer, clip, opacity)
    }

    // ---- basic widgets ------------------------------------------------------

    /// One line of text, its own width.
    pub fn label(&mut self, text: &str, font: Font, color: Color32) -> Rect {
        let w = self.ctx.fonts.width(text, font);
        let rect = self.allocate(vec2(w, font.line_height()));
        self.painter().text(rect.min, text, font, color);
        self.a11y(Node::new(
            self.id.with(("label", text)).0,
            Role::Text,
            text,
            rect,
        ));
        rect
    }

    /// One line cut to the available width with an ellipsis; returns its
    /// rect and whether it was cut.
    pub fn label_truncated(&mut self, text: &str, font: Font, color: Color32) -> (Rect, bool) {
        let max = self.available_width();
        let (shown, elided) = self.ctx.fonts.truncate(text, font, max);
        let w = self.ctx.fonts.width(&shown, font);
        let rect = self.allocate(vec2(w, font.line_height()));
        self.painter().text(rect.min, shown, font, color);
        self.a11y(Node::new(
            self.id.with(("label", text)).0,
            Role::Text,
            text,
            rect,
        ));
        (rect, elided)
    }

    /// Text wrapped to the available width (or `max_width`), left aligned
    /// or centred.
    pub fn label_wrapped(
        &mut self,
        text: &str,
        font: Font,
        color: Color32,
        centered: bool,
    ) -> Rect {
        let max = self.available_width().max(20.0);
        let lines = self.ctx.fonts.wrap(text, font, max);
        let lh = font.line_height();
        let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let size = vec2(if centered { max } else { widest }, lh * lines.len() as f32);
        let rect = self.allocate(size);
        let mut p = self.painter();
        for (i, line) in lines.iter().enumerate() {
            let x = if centered {
                rect.center().x - line.width / 2.0
            } else {
                rect.left()
            };
            p.text(
                pos2(x, rect.top() + i as f32 * lh),
                &text[line.range.clone()],
                font,
                color,
            );
        }
        self.a11y(Node::new(
            self.id.with(("label", text)).0,
            Role::Text,
            text,
            rect,
        ));
        rect
    }

    /// A clickable 1 px rule across the available width.
    pub fn separator(&mut self, color: Color32) {
        let w = self.available_width();
        let rect = self.allocate(vec2(w, 1.0));
        self.painter().rect_filled(rect, 0.0, color);
    }

    /// An area for floating content (popups, toasts) at `pos` in `layer`.
    pub fn area<R>(&mut self, layer: Layer, rect: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
        let screen = self.ctx.screen_rect();
        let mut ui = Ui::root(self.ctx, rect, layer);
        ui.clip = screen;
        ui.a11y_parent = self.a11y_parent;
        ui.id = self.id;
        f(&mut ui)
    }
}
