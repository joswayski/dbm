//! Scroll areas with thin overlay scrollbars. Content size comes from the
//! previous frame (a change requests another pass).

use crate::ctx::{CursorIcon, Id, Sense};
use crate::geom::{Color32, Rect, Vec2, pos2, vec2};
use crate::ui::{Dir, Ui};

/// Points scrolled per wheel notch, as egui uses.
pub const WHEEL_STEP: f32 = 50.0;
const BAR: f32 = 8.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    pub offset: Vec2,
    pub content: Vec2,
    pub viewport: Vec2,
}

pub struct ScrollArea {
    id: Id,
    vertical: bool,
    horizontal: bool,
    max_height: Option<f32>,
    /// Shrinks to the content's height (up to `max_height`).
    shrink_height: bool,
    bar_inset: bool,
    hide_bars: bool,
    /// Scrolls this far, once (for "scroll into view").
    scroll_to: Option<Vec2>,
}

pub struct ScrollOutput<R> {
    pub inner: R,
    /// The visible area.
    pub rect: Rect,
    pub state: ScrollState,
}

impl ScrollArea {
    pub fn vertical(id: Id) -> Self {
        ScrollArea {
            id,
            vertical: true,
            horizontal: false,
            max_height: None,
            shrink_height: false,
            bar_inset: false,
            hide_bars: false,
            scroll_to: None,
        }
    }

    pub fn horizontal(id: Id) -> Self {
        ScrollArea {
            horizontal: true,
            vertical: false,
            ..ScrollArea::vertical(id)
        }
    }

    pub fn both(id: Id) -> Self {
        ScrollArea {
            horizontal: true,
            ..ScrollArea::vertical(id)
        }
    }

    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = Some(h);
        self
    }

    /// As tall as the content, up to `max_height`.
    pub fn shrink_to_content(mut self) -> Self {
        self.shrink_height = true;
        self
    }

    /// The scrollbar takes its own column instead of overlaying content.
    pub fn solid_bar(mut self) -> Self {
        self.bar_inset = true;
        self
    }

    /// Scrolls with the wheel only, without drawing scrollbars.
    pub fn hide_bars(mut self) -> Self {
        self.hide_bars = true;
        self
    }

    pub fn scroll_to_x(mut self, x: Option<f32>) -> Self {
        if let Some(x) = x {
            self.scroll_to = Some(vec2(x, f32::NAN));
        }
        self
    }

    pub fn state(ui: &Ui, id: Id) -> ScrollState {
        ui.ctx.data::<ScrollState>(id).unwrap_or_default()
    }

    pub fn set_offset(ui: &mut Ui, id: Id, offset: Vec2) {
        ui.ctx.data_mut(id, ScrollState::default).offset = offset;
    }

    pub fn show<R>(self, ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> ScrollOutput<R> {
        self.show_inner(ui, |ui, _| f(ui))
    }

    /// Lays out only the rows of height `row_height` that are visible:
    /// `f` gets the child Ui (positioned at the first visible row) and the
    /// range of visible row indexes.
    pub fn show_rows<R>(
        self,
        ui: &mut Ui,
        row_height: f32,
        rows: usize,
        f: impl FnOnce(&mut Ui, std::ops::Range<usize>) -> R,
    ) -> ScrollOutput<R> {
        self.show_inner(ui, |ui, state| {
            let first = ((state.offset.y / row_height).floor().max(0.0)) as usize;
            let visible = (state.viewport.y / row_height).ceil() as usize + 2;
            let range = first.min(rows)..(first + visible).min(rows);
            let top = ui.cursor().y;
            ui.add_space(range.start as f32 * row_height);
            let inner = f(ui, range.clone());
            // Reserve the full height so the content size is right.
            let width = ui.min_rect().width().max(1.0);
            let left = ui.max_rect().left();
            ui.advance_past(Rect::from_min_size(
                pos2(left, top),
                vec2(width, rows as f32 * row_height),
            ));
            inner
        })
    }

    fn show_inner<R>(
        self,
        ui: &mut Ui,
        f: impl FnOnce(&mut Ui, ScrollState) -> R,
    ) -> ScrollOutput<R> {
        let mut state = ui.ctx.data::<ScrollState>(self.id).unwrap_or_default();
        let avail = ui.available_rect();
        let mut height = avail.height();
        if let Some(max) = self.max_height {
            height = height.min(max);
        }
        if self.shrink_height {
            height = height.min(state.content.y.max(1.0));
            if state.content.y == 0.0 {
                // Unknown yet: lay out once at full height, then shrink.
                height = self
                    .max_height
                    .unwrap_or(avail.height())
                    .min(avail.height());
            }
        }
        let mut outer = Rect::from_min_size(avail.min, vec2(avail.width(), height));
        let needs_v = self.vertical && state.content.y > outer.height() + 0.5;
        let viewport = if self.bar_inset && needs_v {
            outer.with_max_x(outer.right() - BAR - 2.0)
        } else {
            outer
        };
        state.viewport = viewport.size();
        // Wheel: the innermost area under the pointer takes it.
        if ui.rect_contains_pointer(viewport) {
            let can_h = self.horizontal && state.content.x > viewport.width() + 0.5;
            let can_v = self.vertical && state.content.y > viewport.height() + 0.5;
            if can_h || can_v {
                let delta = ui.ctx.take_wheel(can_h, can_v);
                state.offset = state.offset - delta;
            }
        }
        if let Some(target) = self.scroll_to {
            if !target.x.is_nan() {
                state.offset.x = target.x;
            }
            if !target.y.is_nan() {
                state.offset.y = target.y;
            }
        }
        let max_offset = vec2(
            (state.content.x - viewport.width()).max(0.0),
            (state.content.y - viewport.height()).max(0.0),
        );
        state.offset = vec2(
            state.offset.x.clamp(0.0, max_offset.x),
            state.offset.y.clamp(0.0, max_offset.y),
        );
        let content_max = Rect::from_min_size(
            viewport.min - state.offset,
            vec2(
                if self.horizontal {
                    1e6
                } else {
                    viewport.width()
                },
                if self.vertical {
                    1e7
                } else {
                    viewport.height()
                },
            ),
        );
        let (inner, used) = {
            let mut child = ui.child(content_max, Dir::Down);
            child.set_clip(viewport);
            let inner = f(&mut child, state);
            (inner, child.min_rect())
        };
        let content = vec2(
            (used.right() - content_max.left()).max(0.0),
            (used.bottom() - content_max.top()).max(0.0),
        );
        if (content - state.content).length() > 0.5 {
            state.content = content;
            ui.ctx.request_rerun();
        }
        if self.shrink_height {
            outer = outer.with_max_y(outer.top() + height);
        }
        let state = if self.hide_bars {
            state
        } else {
            self.scrollbars(ui, viewport, state)
        };
        ui.ctx.insert_data(self.id, state);
        ui.advance_past(outer);
        ScrollOutput {
            inner,
            rect: viewport,
            state,
        }
    }

    fn scrollbars(&self, ui: &mut Ui, viewport: Rect, mut state: ScrollState) -> ScrollState {
        let hovered_area = ui.rect_contains_pointer(viewport);
        for vertical in [true, false] {
            let (enabled, content, view, offset) = if vertical {
                (
                    self.vertical,
                    state.content.y,
                    viewport.height(),
                    state.offset.y,
                )
            } else {
                (
                    self.horizontal,
                    state.content.x,
                    viewport.width(),
                    state.offset.x,
                )
            };
            if !enabled || content <= view + 0.5 {
                continue;
            }
            let track = if vertical {
                let x = if self.bar_inset {
                    viewport.right() + 2.0
                } else {
                    viewport.right() - BAR
                };
                Rect::from_ltrb(x, viewport.top() + 2.0, x + BAR, viewport.bottom() - 2.0)
            } else {
                Rect::from_ltrb(
                    viewport.left() + 2.0,
                    viewport.bottom() - BAR,
                    viewport.right() - 2.0,
                    viewport.bottom(),
                )
            };
            let length = if vertical {
                track.height()
            } else {
                track.width()
            };
            if length <= 1.0 {
                continue;
            }
            // At least 24 px, unless the track itself is shorter.
            let thumb_len = (length * view / content).max(24.0).min(length);
            let travel = length - thumb_len;
            let max_offset = content - view;
            let start = if max_offset > 0.0 {
                travel * offset / max_offset
            } else {
                0.0
            };
            let thumb = if vertical {
                Rect::from_min_size(
                    pos2(track.left(), track.top() + start),
                    vec2(BAR, thumb_len),
                )
            } else {
                Rect::from_min_size(
                    pos2(track.left() + start, track.top()),
                    vec2(thumb_len, BAR),
                )
            };
            let id = self.id.with(("bar", vertical));
            let r = ui.interact(thumb, id, Sense::DRAG.unfocusable());
            if r.dragged && travel > 0.0 {
                let d = if vertical {
                    r.drag_delta.y
                } else {
                    r.drag_delta.x
                };
                let next = (offset + d * max_offset / travel).clamp(0.0, max_offset);
                if vertical {
                    state.offset.y = next;
                } else {
                    state.offset.x = next;
                }
                ui.ctx.request_repaint();
            }
            if r.hovered || r.dragged {
                ui.ctx.set_cursor(CursorIcon::Default);
            }
            let alpha = if r.dragged || r.is_pointer_down {
                110
            } else if r.hovered {
                80
            } else if hovered_area || self.bar_inset {
                48
            } else {
                28
            };
            // Thin at rest, full width while hovered (an overlay bar).
            let shown = if r.hovered || r.dragged || self.bar_inset {
                thumb
            } else if vertical {
                thumb
                    .with_min_x(thumb.right() - 4.0)
                    .translate(vec2(-2.0, 0.0))
            } else {
                thumb
                    .with_min_y(thumb.bottom() - 4.0)
                    .translate(vec2(0.0, -2.0))
            };
            let mut p = ui.painter();
            p.rect_filled(shown, 4.0, Color32::from_white_alpha(alpha));
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::ctx::{Context, Id, NullPlatform};
    use crate::geom::{Rect, pos2, vec2};
    use crate::input::RawInput;
    use crate::paint::Layer;
    use crate::text::{Fonts, MonoMetrics};
    use crate::ui::Ui;

    /// A scroll area shorter than the minimum thumb (a tiny first window)
    /// must lay out without panicking, on the first frame and the rerun.
    #[test]
    fn overflowing_area_shorter_than_the_minimum_thumb() {
        let mut ctx = Context::new(
            Fonts::new(Box::new(MonoMetrics)),
            Rc::new(NullPlatform::default()),
        );
        for _ in 0..3 {
            ctx.begin_frame(RawInput {
                screen: vec2(200.0, 12.0),
                ..Default::default()
            });
            let mut ui = Ui::root(
                &mut ctx,
                Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 12.0)),
                Layer::Background,
            );
            super::ScrollArea::vertical(Id::new("tiny")).show(&mut ui, |ui| {
                ui.allocate(vec2(100.0, 400.0));
            });
            ctx.end_frame();
        }
    }
}
