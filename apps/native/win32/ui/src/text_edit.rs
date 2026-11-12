//! Single- and multi-line text fields: caret, selection, clipboard, undo,
//! word and line navigation, password masking, hint text, and coloured
//! spans for syntax highlighting. Offsets are UTF-8 byte offsets on char
//! boundaries.

use crate::a11y::{Node, Role};
use crate::ctx::{CursorIcon, Id, Response, Sense};
use crate::geom::{Color32, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use crate::input::{Event, Key, Modifiers};
use crate::text::Font;
use crate::ui::Ui;

/// Per-field memory kept between frames.
#[derive(Clone, Debug, Default)]
pub struct EditState {
    /// (anchor, caret) byte offsets.
    pub selection: (usize, usize),
    pub scroll: Vec2,
    undo: Vec<(String, (usize, usize))>,
    redo: Vec<(String, (usize, usize))>,
    /// The caret's x while moving up and down, so it doesn't drift.
    goal_x: Option<f32>,
    drag_anchor: Option<usize>,
    blink_from: f64,
}

impl EditState {
    pub fn load(ui: &Ui, id: Id) -> Option<EditState> {
        ui.ctx.data::<EditState>(id)
    }

    pub fn store(self, ui: &mut Ui, id: Id) {
        ui.ctx.insert_data(id, self);
    }

    pub fn set_caret(&mut self, at: usize) {
        self.selection = (at, at);
    }
}

pub struct TextEdit<'t> {
    text: &'t mut String,
    id: Id,
    font: Font,
    multiline: bool,
    password: bool,
    hint: &'t str,
    hint_italic: bool,
    width: Option<f32>,
    rows: usize,
    margin: Vec2,
    /// Extra room on the left, for a leading glyph.
    pad_left: f32,
    frame: bool,
    background: Option<Color32>,
    text_color: Color32,
    interactive: bool,
    char_limit: usize,
    colors: Option<&'t dyn Fn(&str) -> Vec<(usize, Color32)>>,
    name: &'t str,
    read_only: bool,
}

pub struct TextEditOutput {
    pub response: Response,
    /// Where the text starts (top-left of the first line, before scrolling).
    pub text_origin: Pos2,
    pub state: EditState,
    /// Enter pressed in a single-line field (it then loses focus).
    pub submitted: bool,
    /// Escape pressed while focused (it then loses focus).
    pub escaped: bool,
    pub line_height: f32,
}

pub const TEXT_COLOR: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xea);
const HINT_COLOR: Color32 = Color32::from_rgb(0x80, 0x80, 0x87);
const FIELD: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2d);
const BORDER: Color32 = Color32::from_rgb(0x35, 0x35, 0x38);
const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x9a, 0xff);

impl<'t> TextEdit<'t> {
    pub fn singleline(id: Id, text: &'t mut String) -> Self {
        TextEdit {
            text,
            id,
            font: Font::regular(13.0),
            multiline: false,
            password: false,
            hint: "",
            hint_italic: false,
            width: None,
            rows: 1,
            margin: vec2(9.0, 7.0),
            pad_left: 0.0,
            frame: true,
            background: None,
            text_color: TEXT_COLOR,
            interactive: true,
            char_limit: usize::MAX,
            colors: None,
            name: "",
            read_only: false,
        }
    }

    pub fn multiline(id: Id, text: &'t mut String) -> Self {
        TextEdit {
            multiline: true,
            rows: 4,
            ..TextEdit::singleline(id, text)
        }
    }

    pub fn font(mut self, f: Font) -> Self {
        self.font = f;
        self
    }

    pub fn password(mut self, on: bool) -> Self {
        self.password = on;
        self
    }

    pub fn hint(mut self, hint: &'t str) -> Self {
        self.hint = hint;
        self
    }

    pub fn hint_italic(mut self) -> Self {
        self.hint_italic = true;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows;
        self
    }

    pub fn margin(mut self, x: f32, y: f32) -> Self {
        self.margin = vec2(x, y);
        self
    }

    /// Adds `w` to the left margin, leaving room for a leading icon.
    pub fn pad_left(mut self, w: f32) -> Self {
        self.pad_left = w;
        self
    }

    pub fn frameless(mut self) -> Self {
        self.frame = false;
        self
    }

    pub fn background(mut self, c: Color32) -> Self {
        self.background = Some(c);
        self
    }

    pub fn text_color(mut self, c: Color32) -> Self {
        self.text_color = c;
        self
    }

    pub fn interactive(mut self, on: bool) -> Self {
        self.interactive = on;
        self
    }

    /// Selectable and copyable but not editable.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    pub fn char_limit(mut self, n: usize) -> Self {
        self.char_limit = n;
        self
    }

    /// Colour spans: (start byte, colour) pairs in order, for highlighting.
    pub fn colors(mut self, f: &'t dyn Fn(&str) -> Vec<(usize, Color32)>) -> Self {
        self.colors = Some(f);
        self
    }

    /// The accessible name (the field's label).
    pub fn name(mut self, name: &'t str) -> Self {
        self.name = name;
        self
    }

    pub fn show(self, ui: &mut Ui) -> TextEditOutput {
        let lh = self.font.line_height();
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let height = if self.multiline {
            ui.available_height()
                .max(lh * self.rows as f32 + self.margin.y * 2.0)
        } else {
            lh + self.margin.y * 2.0
        };
        let rect = ui.allocate(vec2(width, height));
        self.show_in(ui, rect)
    }

    pub fn show_in(mut self, ui: &mut Ui, rect: Rect) -> TextEditOutput {
        let id = self.id;
        let font = self.font;
        let lh = font.line_height();
        let sense = if self.interactive {
            Sense::DRAG
        } else {
            Sense::HOVER
        };
        let mut sense = sense;
        sense.focusable = self.interactive;
        sense.plain_cursor = true;
        let mut response = ui.interact(rect, id, sense);
        let mut st = ui.ctx.data::<EditState>(id).unwrap_or_default();
        let len = self.text.len();
        st.selection.0 = clamp_boundary(self.text, st.selection.0.min(len));
        st.selection.1 = clamp_boundary(self.text, st.selection.1.min(len));
        let inner = rect
            .shrink2(self.margin)
            .with_min_x(rect.left() + self.margin.x + self.pad_left);
        let password = self.password;
        let display = |text: &str| -> String {
            if password {
                "•".repeat(text.chars().count())
            } else {
                text.to_owned()
            }
        };
        let editable = self.interactive && !self.read_only && ui.is_enabled();
        let mut submitted = false;
        let mut escaped = false;
        if response.hovered && self.interactive {
            ui.ctx.set_cursor(CursorIcon::Text);
        }
        if let Some(value) = ui.ctx.take_a11y_value(id)
            && editable
        {
            push_undo(&mut st, self.text);
            *self.text = value;
            st.set_caret(self.text.len());
            response.changed = true;
        }
        // Pointer: place caret, extend with Shift, select words on double click.
        let shown = display(self.text);
        let lines = layout_lines(&shown, self.multiline);
        let origin = pos2(inner.left() - st.scroll.x, inner.top() - st.scroll.y);
        if (response.pressed || response.dragged)
            && self.interactive
            && let Some(p) = ui.ctx.pointer()
        {
            let at = map_offset(
                &shown,
                self.text,
                hit(&ui.ctx.fonts, &shown, &lines, font, origin, p),
            );
            if response.pressed {
                ui.ctx.request_focus(id);
                st.blink_from = ui.ctx.time();
                if response.double_clicked {
                    let (a, b) = word_at(self.text, at);
                    st.selection = (a, b);
                    st.drag_anchor = None;
                } else if ui.ctx.modifiers().shift {
                    st.selection.1 = at;
                    st.drag_anchor = Some(st.selection.0);
                } else {
                    st.selection = (at, at);
                    st.drag_anchor = Some(at);
                }
                st.goal_x = None;
            } else if let Some(anchor) = st.drag_anchor {
                st.selection = (anchor, at);
            }
        }
        if response.drag_stopped || !ui.ctx.primary_down() {
            st.drag_anchor = None;
        }
        let focused = ui.ctx.has_focus(id);
        response.has_focus = focused;
        if focused && self.interactive {
            let events = ui.ctx.take_events();
            let mut rest = Vec::new();
            for event in events {
                match event {
                    Event::Text(t) if editable => {
                        let t: String = if self.multiline {
                            t.chars()
                                .filter(|c| *c == '\n' || !c.is_control())
                                .collect()
                        } else {
                            t.chars().filter(|c| !c.is_control()).collect()
                        };
                        if t.is_empty() {
                            continue;
                        }
                        let room = self.char_limit.saturating_sub(
                            self.text.chars().count()
                                - selected(self.text, st.selection).chars().count(),
                        );
                        let t: String = t.chars().take(room).collect();
                        if !t.is_empty() {
                            insert(self.text, &mut st, &t);
                            response.changed = true;
                        }
                    }
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                    } => {
                        match self.key(ui, &mut st, key, modifiers, &lines, origin, editable) {
                            KeyResult::Handled(changed) => response.changed |= changed,
                            KeyResult::Submit => {
                                submitted = true;
                                ui.ctx.surrender_focus(id);
                                ui.ctx.report_lost(id);
                                response.lost_focus = true;
                            }
                            KeyResult::Escape => {
                                escaped = true;
                                ui.ctx.surrender_focus(id);
                                ui.ctx.report_lost(id);
                                response.lost_focus = true;
                            }
                            KeyResult::Unhandled => rest.push(Event::Key {
                                key,
                                pressed: true,
                                modifiers,
                            }),
                        }
                        st.blink_from = ui.ctx.time();
                    }
                    other => rest.push(other),
                }
            }
            ui.ctx.put_back_events(rest);
        }
        // The wheel scrolls a multi-line field (the SQL editor).
        if self.multiline && response.hovered {
            let lines_now = layout_lines(self.text, true);
            let widest = lines_now
                .iter()
                .map(|l| ui.ctx.fonts.width(&self.text[l.start..l.end], font))
                .fold(0.0, f32::max);
            let content = vec2(widest + 8.0, lines_now.len() as f32 * lh);
            let inner = rect.shrink2(self.margin);
            let can_h = content.x > inner.width();
            let can_v = content.y > inner.height();
            if can_h || can_v {
                let d = ui.ctx.take_wheel(can_h, can_v);
                st.scroll.x = (st.scroll.x - d.x).clamp(0.0, (content.x - inner.width()).max(0.0));
                st.scroll.y = (st.scroll.y - d.y).clamp(0.0, (content.y - inner.height()).max(0.0));
            }
        }
        // Lay out again if the text changed.
        let shown = display(self.text);
        let lines = layout_lines(&shown, self.multiline);
        let fonts = &ui.ctx.fonts;
        let caret_shown = map_offset_rev(self.text, &shown, st.selection.1);
        let (cl, cx) = locate(fonts, &shown, &lines, font, caret_shown);
        // Keep the caret in view.
        if focused {
            let caret_x = cx;
            let caret_y = cl as f32 * lh;
            if caret_x - st.scroll.x > inner.width() - 2.0 {
                st.scroll.x = caret_x - inner.width() + 2.0;
            }
            if caret_x < st.scroll.x {
                st.scroll.x = caret_x;
            }
            if self.multiline {
                if caret_y + lh - st.scroll.y > inner.height() {
                    st.scroll.y = caret_y + lh - inner.height();
                }
                if caret_y < st.scroll.y {
                    st.scroll.y = caret_y;
                }
            }
        }
        if !self.multiline {
            let w = fonts.width(&shown, font);
            st.scroll.x = st.scroll.x.min((w - inner.width() + 2.0).max(0.0)).max(0.0);
            st.scroll.y = 0.0;
        }
        let origin = pos2(inner.left() - st.scroll.x, inner.top() - st.scroll.y);
        // Paint.
        let enabled = ui.is_enabled();
        if self.frame {
            let border = if focused { ACCENT } else { BORDER };
            let fill = self.background.unwrap_or(FIELD);
            ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0, border));
        } else if let Some(bg) = self.background {
            ui.painter().rect_filled(rect, 0.0, bg);
        }
        let clip = if self.frame { rect.shrink(1.0) } else { rect };
        let spans = if self.password {
            Vec::new()
        } else {
            self.colors.map(|f| f(&shown)).unwrap_or_default()
        };
        let time = ui.ctx.time();
        let color = if enabled {
            self.text_color
        } else {
            self.text_color.gamma_multiply(0.6)
        };
        {
            let mut p = ui.painter_at(clip);
            let fonts = p.fonts();
            // Selection.
            let (a, b) = order(st.selection);
            if a != b && (focused || !self.interactive || self.read_only) {
                let (a, b) = (
                    map_offset_rev(self.text, &shown, a),
                    map_offset_rev(self.text, &shown, b),
                );
                let rects: Vec<Rect> = lines
                    .iter()
                    .enumerate()
                    .filter_map(|(i, line)| {
                        let s = a.max(line.start);
                        let e = b.min(line.end);
                        if s > e || (s == e && !(a <= line.end && b > line.end)) {
                            return None;
                        }
                        let x0 = fonts.width(&shown[line.start..s], font);
                        let mut x1 = fonts.width(&shown[line.start..e], font);
                        if b > line.end {
                            x1 += font.size * 0.4;
                        }
                        Some(Rect::from_ltrb(
                            origin.x + x0,
                            origin.y + i as f32 * lh,
                            origin.x + x1,
                            origin.y + (i + 1) as f32 * lh,
                        ))
                    })
                    .collect();
                for r in rects {
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(76, 154, 255, 64));
                }
            }
            if shown.is_empty() && !self.hint.is_empty() {
                let hf = if self.hint_italic {
                    font.italics()
                } else {
                    font
                };
                p.text(origin, self.hint, hf, HINT_COLOR);
            }
            let first = ((st.scroll.y / lh).floor().max(0.0)) as usize;
            let visible = (clip.height() / lh).ceil() as usize + 2;
            for (i, line) in lines.iter().enumerate().skip(first).take(visible) {
                let y = origin.y + i as f32 * lh;
                let text = &shown[line.start..line.end];
                if spans.is_empty() {
                    p.text(pos2(origin.x, y), text, font, color);
                    continue;
                }
                // Coloured runs within the line.
                let mut x = origin.x;
                let mut at = line.start;
                while at < line.end {
                    let idx = spans.partition_point(|(s, _)| *s <= at);
                    let c = if idx == 0 { color } else { spans[idx - 1].1 };
                    let next = spans
                        .get(idx)
                        .map_or(line.end, |(s, _)| (*s).min(line.end))
                        .max(at + 1);
                    let next = clamp_boundary(&shown, next.min(line.end));
                    let next = if next <= at { line.end } else { next };
                    let run = &shown[at..next];
                    p.text(pos2(x, y), run, font, c);
                    x += fonts.width(run, font);
                    at = next;
                }
            }
            // Caret: blinks every half second, solid while typing.
            if focused && editable {
                let phase = ((time - st.blink_from) / 0.5) as i64;
                if phase % 2 == 0 {
                    let x = (origin.x + cx).round();
                    let y = origin.y + cl as f32 * lh;
                    p.rect_filled(
                        Rect::from_ltrb(x, y + 1.0, x + 1.5, y + lh - 1.0),
                        0.0,
                        TEXT_COLOR,
                    );
                }
            }
        }
        if focused && editable {
            let next = 0.5 - ((time - st.blink_from) % 0.5);
            ui.ctx.request_repaint_after(next);
            ui.ctx.set_ime_rect(Rect::from_min_size(
                pos2(origin.x + cx, origin.y + cl as f32 * lh),
                vec2(1.0, lh),
            ));
        }
        let mut node = Node::new(id.0, Role::Edit, self.name, rect);
        node.value = Some(if self.password {
            String::new()
        } else {
            self.text.clone()
        });
        node.focusable = self.interactive;
        node.password = self.password;
        node.read_only = !editable;
        ui.a11y(node);
        if response.lost_focus || (ui.ctx.lost_pending() == Some(id) && !focused) {
            response.lost_focus = true;
        }
        let out_state = st.clone();
        ui.ctx.insert_data(id, st);
        TextEditOutput {
            response,
            text_origin: origin,
            state: out_state,
            submitted,
            escaped,
            line_height: lh,
        }
    }

    fn key(
        &mut self,
        ui: &mut Ui,
        st: &mut EditState,
        key: Key,
        m: Modifiers,
        lines: &[LineSpan],
        origin: Pos2,
        editable: bool,
    ) -> KeyResult {
        let text = &mut *self.text;
        let word = m.ctrl;
        let caret = st.selection.1;
        let moved = |st: &mut EditState, to: usize, keep_goal: bool| {
            if m.shift {
                st.selection.1 = to;
            } else {
                st.selection = (to, to);
            }
            if !keep_goal {
                st.goal_x = None;
            }
        };
        match key {
            Key::ArrowLeft => {
                let (a, b) = order(st.selection);
                let to = if a != b && !m.shift {
                    a
                } else if word {
                    prev_word(text, caret)
                } else {
                    prev_char(text, caret)
                };
                moved(st, to, false);
            }
            Key::ArrowRight => {
                let (a, b) = order(st.selection);
                let to = if a != b && !m.shift {
                    b
                } else if word {
                    next_word(text, caret)
                } else {
                    next_char(text, caret)
                };
                moved(st, to, false);
            }
            Key::Home => {
                let to = if word || !self.multiline {
                    0
                } else {
                    text[..caret].rfind('\n').map_or(0, |i| i + 1)
                };
                moved(st, to, false);
            }
            Key::End => {
                let to = if word || !self.multiline {
                    text.len()
                } else {
                    text[caret..].find('\n').map_or(text.len(), |i| caret + i)
                };
                moved(st, to, false);
            }
            Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown if self.multiline => {
                let fonts = &ui.ctx.fonts;
                let shown_caret = caret; // multiline fields are never masked
                let (line, x) = locate(fonts, text, lines, self.font, shown_caret);
                let goal = st.goal_x.unwrap_or(x);
                let step = if matches!(key, Key::PageUp | Key::PageDown) {
                    20
                } else {
                    1
                };
                let target = if matches!(key, Key::ArrowUp | Key::PageUp) {
                    line.checked_sub(step).or(Some(0)).filter(|_| line > 0)
                } else {
                    Some((line + step).min(lines.len() - 1)).filter(|_| line + 1 < lines.len())
                };
                let to = match target {
                    Some(l) => {
                        let lh = self.font.line_height();
                        let p = pos2(origin.x + goal, origin.y + l as f32 * lh + lh / 2.0);
                        hit(fonts, text, lines, self.font, origin, p)
                    }
                    None if matches!(key, Key::ArrowUp | Key::PageUp) => 0,
                    None => text.len(),
                };
                moved(st, to, true);
                st.goal_x = Some(goal);
            }
            Key::A if m.ctrl => st.selection = (0, text.len()),
            Key::C if m.ctrl => {
                if !self.password {
                    let s = selected(text, st.selection);
                    if !s.is_empty() {
                        ui.ctx.copy_text(s);
                    }
                }
            }
            Key::X if m.ctrl => {
                if !self.password && editable {
                    let s = selected(text, st.selection).to_owned();
                    if !s.is_empty() {
                        ui.ctx.copy_text(&s);
                        push_undo(st, text);
                        replace_selection(text, st, "");
                        return KeyResult::Handled(true);
                    }
                }
            }
            Key::V if m.ctrl && editable => {
                if let Some(mut s) = ui.ctx.platform.clipboard() {
                    s = s.replace("\r\n", "\n");
                    if !self.multiline {
                        s = s.replace('\n', " ");
                    }
                    let room = self.char_limit.saturating_sub(
                        text.chars().count() - selected(text, st.selection).chars().count(),
                    );
                    let s: String = s.chars().take(room).collect();
                    push_undo(st, text);
                    replace_selection(text, st, &s);
                    return KeyResult::Handled(true);
                }
            }
            Key::Z if m.ctrl && editable => {
                let from = if m.shift {
                    st.redo.pop()
                } else {
                    st.undo.pop()
                };
                if let Some((t, sel)) = from {
                    let current = (std::mem::replace(text, t), st.selection);
                    if m.shift {
                        st.undo.push(current)
                    } else {
                        st.redo.push(current)
                    }
                    st.selection = sel;
                    return KeyResult::Handled(true);
                }
            }
            Key::Y if m.ctrl && editable => {
                if let Some((t, sel)) = st.redo.pop() {
                    st.undo.push((std::mem::replace(text, t), st.selection));
                    st.selection = sel;
                    return KeyResult::Handled(true);
                }
            }
            Key::Backspace if editable => {
                let (a, b) = order(st.selection);
                push_undo(st, text);
                if a == b {
                    let from = if word {
                        prev_word(text, a)
                    } else {
                        prev_char(text, a)
                    };
                    st.selection = (from, a);
                }
                replace_selection(text, st, "");
                return KeyResult::Handled(true);
            }
            Key::Delete if editable => {
                let (a, b) = order(st.selection);
                push_undo(st, text);
                if a == b {
                    let to = if word {
                        next_word(text, a)
                    } else {
                        next_char(text, a)
                    };
                    st.selection = (a, to);
                }
                replace_selection(text, st, "");
                return KeyResult::Handled(true);
            }
            Key::Enter if self.multiline && !m.ctrl && editable => {
                // Keep the current line's indentation, like a code editor.
                let start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
                let indent: String = text[start..]
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                insert(text, st, &format!("\n{indent}"));
                return KeyResult::Handled(true);
            }
            Key::Tab if self.multiline && !m.ctrl && editable && !m.shift => {
                insert(text, st, "  ");
                return KeyResult::Handled(true);
            }
            Key::Enter if !self.multiline && !m.ctrl => return KeyResult::Submit,
            Key::Escape => return KeyResult::Escape,
            _ => return KeyResult::Unhandled,
        }
        KeyResult::Handled(false)
    }
}

enum KeyResult {
    Handled(bool),
    Submit,
    Escape,
    Unhandled,
}

#[derive(Clone, Copy, Debug)]
pub struct LineSpan {
    pub start: usize,
    pub end: usize,
}

/// Lines split at `\n` (no soft wrap: the editor scrolls horizontally).
pub fn layout_lines(text: &str, multiline: bool) -> Vec<LineSpan> {
    if !multiline {
        return vec![LineSpan {
            start: 0,
            end: text.len(),
        }];
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            out.push(LineSpan { start, end: i });
            start = i + 1;
        }
    }
    out.push(LineSpan {
        start,
        end: text.len(),
    });
    out
}

/// (line index, x) of a byte offset in the shown text.
pub fn locate(
    fonts: &crate::text::Fonts,
    shown: &str,
    lines: &[LineSpan],
    font: Font,
    at: usize,
) -> (usize, f32) {
    let line = lines
        .iter()
        .position(|l| at <= l.end)
        .unwrap_or(lines.len().saturating_sub(1));
    let l = lines[line];
    let at = at.clamp(l.start, l.end);
    (line, fonts.width(&shown[l.start..at], font))
}

fn hit(
    fonts: &crate::text::Fonts,
    shown: &str,
    lines: &[LineSpan],
    font: Font,
    origin: Pos2,
    p: Pos2,
) -> usize {
    let lh = font.line_height();
    let line = (((p.y - origin.y) / lh).floor().max(0.0) as usize).min(lines.len() - 1);
    let l = lines[line];
    let mut x = origin.x;
    for (i, c) in shown[l.start..l.end].char_indices() {
        let w = fonts.advance(c, font);
        if p.x < x + w / 2.0 {
            return l.start + i;
        }
        x += w;
    }
    l.end
}

/// Maps an offset in the shown (possibly masked) text to the real text.
fn map_offset(shown: &str, text: &str, at: usize) -> usize {
    if shown.len() == text.len() {
        return at;
    }
    let n = shown[..at].chars().count();
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}

fn map_offset_rev(text: &str, shown: &str, at: usize) -> usize {
    if shown.len() == text.len() {
        return at;
    }
    let n = text[..at.min(text.len())].chars().count();
    shown.char_indices().nth(n).map_or(shown.len(), |(i, _)| i)
}

fn clamp_boundary(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

pub fn order((a, b): (usize, usize)) -> (usize, usize) {
    (a.min(b), a.max(b))
}

fn selected(text: &str, sel: (usize, usize)) -> &str {
    let (a, b) = order(sel);
    &text[a..b]
}

fn push_undo(st: &mut EditState, text: &str) {
    st.undo.push((text.to_owned(), st.selection));
    if st.undo.len() > 200 {
        st.undo.remove(0);
    }
    st.redo.clear();
}

fn insert(text: &mut String, st: &mut EditState, s: &str) {
    // Typing coalesces into one undo step per word.
    let boundary = s.chars().any(|c| c.is_whitespace()) || st.selection.0 != st.selection.1;
    if boundary || st.undo.is_empty() {
        push_undo(st, text);
    }
    replace_selection(text, st, s);
}

fn replace_selection(text: &mut String, st: &mut EditState, s: &str) {
    let (a, b) = order(st.selection);
    text.replace_range(a..b, s);
    let at = a + s.len();
    st.selection = (at, at);
    st.goal_x = None;
}

fn prev_char(text: &str, at: usize) -> usize {
    text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

fn next_char(text: &str, at: usize) -> usize {
    text[at..]
        .chars()
        .next()
        .map_or(text.len(), |c| at + c.len_utf8())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn prev_word(text: &str, at: usize) -> usize {
    let mut i = at;
    let chars: Vec<(usize, char)> = text[..at].char_indices().collect();
    let mut k = chars.len();
    while k > 0 && !is_word(chars[k - 1].1) {
        k -= 1;
        i = chars[k].0;
    }
    while k > 0 && is_word(chars[k - 1].1) {
        k -= 1;
        i = chars[k].0;
    }
    i
}

pub fn next_word(text: &str, at: usize) -> usize {
    let mut it = text[at..].char_indices().peekable();
    let mut end = text.len();
    while let Some(&(_, c)) = it.peek() {
        if is_word(c) {
            break;
        }
        it.next();
    }
    while let Some(&(i, c)) = it.peek() {
        if !is_word(c) {
            end = at + i;
            return end;
        }
        it.next();
    }
    end
}

fn word_at(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(text.len(), |(i, _)| at + i);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_motion_skips_punctuation_then_a_word() {
        let t = "SELECT id, name FROM t";
        assert_eq!(next_word(t, 0), 6);
        assert_eq!(next_word(t, 6), 9);
        assert_eq!(prev_word(t, 15), 11);
        assert_eq!(prev_word(t, 11), 7);
        assert_eq!(word_at(t, 12), (11, 15));
    }

    #[test]
    fn masked_offsets_map_by_character_count() {
        // "é" is two bytes; its bullet is three.
        let text = "aéb";
        let shown = "•••";
        assert_eq!(map_offset(shown, text, 3), 1);
        assert_eq!(map_offset(shown, text, 6), 3);
        assert_eq!(map_offset_rev(text, shown, 3), 6);
    }

    #[test]
    fn undo_restores_text_and_caret_and_redo_reapplies() {
        let mut text = String::from("abc");
        let mut st = EditState::default();
        st.set_caret(3);
        push_undo(&mut st, &text);
        st.selection = (0, 3);
        replace_selection(&mut text, &mut st, "x");
        assert_eq!(text, "x");
        let (t, sel) = st.undo.pop().unwrap();
        st.redo
            .push((std::mem::replace(&mut text, t), st.selection));
        st.selection = sel;
        assert_eq!((text.as_str(), st.selection), ("abc", (3, 3)));
    }
}
