//! Frame state: hit testing, pointer and keyboard routing, focus, popups,
//! tooltips, per-widget memory and the accessibility tree.
//!
//! Hit testing uses the rectangles the previous frame registered, like Dear
//! ImGui and egui. After a frame the backend asks [`FrameOutput::rerun`]:
//! when the new rectangles change what is under the pointer, it runs the
//! frame again (with no new input) before presenting, so hover never lags.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::a11y::{Node, Role, Tree};
use crate::geom::{Color32, Pos2, Rect, Stroke, Vec2};
use crate::input::{A11yAction, Button, Event, Key, Modifiers, RawInput};
use crate::paint::{DisplayList, Layer, Painter};
use crate::text::Fonts;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub u64);

impl Id {
    pub fn new(source: impl Hash) -> Id {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        Id(h.finish())
    }

    pub fn with(self, child: impl Hash) -> Id {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.0.hash(&mut h);
        child.hash(&mut h);
        Id(h.finish())
    }
}

/// What a widget listens to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sense {
    pub click: bool,
    pub drag: bool,
    /// Takes keyboard focus with Tab; Enter or Space then click it.
    pub focusable: bool,
    /// Keeps the arrow cursor although it is clickable (grid rows).
    pub plain_cursor: bool,
}

impl Sense {
    pub const HOVER: Sense = Sense {
        click: false,
        drag: false,
        focusable: false,
        plain_cursor: false,
    };
    pub const CLICK: Sense = Sense {
        click: true,
        drag: false,
        focusable: true,
        plain_cursor: false,
    };
    pub const DRAG: Sense = Sense {
        click: true,
        drag: true,
        focusable: false,
        plain_cursor: false,
    };

    pub fn plain(mut self) -> Sense {
        self.plain_cursor = true;
        self
    }

    pub fn unfocusable(mut self) -> Sense {
        self.focusable = false;
        self
    }

    fn interactive(&self) -> bool {
        self.click || self.drag
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    #[default]
    Default,
    PointingHand,
    Text,
    ResizeHorizontal,
    ResizeVertical,
    NotAllowed,
}

/// Services the toolkit asks of the operating system.
pub trait Platform {
    fn set_clipboard(&self, text: &str);
    fn clipboard(&self) -> Option<String>;
    fn open_url(&self, url: &str);
}

/// A platform with no clipboard or shell, for tests.
#[derive(Default)]
pub struct NullPlatform {
    pub clipboard: RefCell<Option<String>>,
    pub opened: RefCell<Vec<String>>,
}

impl Platform for NullPlatform {
    fn set_clipboard(&self, text: &str) {
        *self.clipboard.borrow_mut() = Some(text.to_owned());
    }

    fn clipboard(&self) -> Option<String> {
        self.clipboard.borrow().clone()
    }

    fn open_url(&self, url: &str) {
        self.opened.borrow_mut().push(url.to_owned());
    }
}

#[derive(Clone, Copy, Debug)]
struct Hit {
    id: Id,
    rect: Rect,
    layer: Layer,
    sense: Sense,
}

#[derive(Clone, Debug, Default)]
pub struct Response {
    pub id: Id,
    pub rect: Rect,
    pub enabled: bool,
    pub hovered: bool,
    /// The primary button went down on it this frame.
    pub pressed: bool,
    /// The primary button is held down on it.
    pub is_pointer_down: bool,
    pub clicked: bool,
    pub double_clicked: bool,
    pub secondary_clicked: bool,
    pub middle_clicked: bool,
    pub dragged: bool,
    pub drag_started: bool,
    pub drag_stopped: bool,
    pub drag_delta: Vec2,
    pub has_focus: bool,
    pub gained_focus: bool,
    pub lost_focus: bool,
    /// A text field's text changed this frame.
    pub changed: bool,
    /// The primary button went down somewhere else this frame.
    pub clicked_elsewhere: bool,
}

thread_local! {
    /// The tooltip asked for this frame by a hovered response.
    static TIP: RefCell<Option<(Id, Rect, String)>> = const { RefCell::new(None) };
}

impl Response {
    pub fn hovered(&self) -> bool {
        self.hovered
    }

    pub fn clicked(&self) -> bool {
        self.clicked
    }

    /// Shows `text` near the pointer after a short hover, while enabled.
    pub fn on_hover_text(self, text: impl Into<String>) -> Self {
        if self.hovered && self.enabled {
            let text = text.into();
            TIP.with(|t| *t.borrow_mut() = Some((self.id, self.rect, text)));
        }
        self
    }

    pub fn on_disabled_hover_text(self, text: impl Into<String>) -> Self {
        if self.hovered && !self.enabled {
            let text = text.into();
            TIP.with(|t| *t.borrow_mut() = Some((self.id, self.rect, text)));
        }
        self
    }

    /// `on_hover_text` for both enabled and disabled states.
    pub fn on_any_hover_text(self, text: impl Into<String>) -> Self {
        if self.hovered {
            let text = text.into();
            TIP.with(|t| *t.borrow_mut() = Some((self.id, self.rect, text)));
        }
        self
    }

    pub fn union_clicks(mut self, other: &Response) -> Self {
        self.clicked |= other.clicked;
        self.hovered |= other.hovered;
        self
    }
}

/// What a frame produced for the backend.
pub struct FrameOutput {
    pub list: DisplayList,
    pub cursor: CursorIcon,
    /// Seconds until the next frame is needed without input; 0 for now.
    pub repaint_after: Option<f64>,
    /// The frame changed what is under the pointer or its own layout: run
    /// it again before presenting.
    pub rerun: bool,
    pub a11y: Tree,
    /// Where an IME composition window should sit (the caret), in DIPs.
    pub ime_rect: Option<Rect>,
    /// The app accepted a close request (or none was made).
    pub close: bool,
}

pub const TOOLTIP_DELAY: f64 = 0.25;

pub struct Context {
    pub fonts: Fonts,
    pub platform: Rc<dyn Platform>,
    time: f64,
    screen: Vec2,
    pointer: Option<Pos2>,
    last_pointer: Option<Pos2>,
    modifiers: Modifiers,
    button_down: [bool; 3],
    prev_hits: Vec<Hit>,
    hits: Vec<Hit>,
    hovered: Option<Id>,
    top_layer: Option<Layer>,
    active: Option<Id>,
    active_secondary: Option<Id>,
    active_middle: Option<Id>,
    drag_started: bool,
    focus: Option<Id>,
    focus_at_start: Option<Id>,
    focus_claimed: bool,
    /// Lost focus last frame without the widget reporting it then.
    lost_pending: Option<Id>,
    lost_reported: Option<Id>,
    focus_rect: Option<Rect>,
    focusables: Vec<Id>,
    keyboard_nav: bool,
    memory: HashMap<Id, Box<dyn Any>>,
    pub(crate) open_popup: Option<Id>,
    popup_area: Option<Rect>,
    popup_area_next: Option<Rect>,
    tip_id: Option<(Id, f64)>,
    // Frame input.
    events: Vec<Event>,
    pressed: Option<(Button, Pos2, bool)>,
    released: Option<(Button, Pos2)>,
    wheel: Vec2,
    a11y_actions: Vec<A11yAction>,
    close_requested: bool,
    close_cancelled: bool,
    // Frame output.
    pub(crate) list: DisplayList,
    a11y: Vec<Node>,
    cursor: CursorIcon,
    cursor_set: bool,
    repaint_after: Option<f64>,
    unstable: bool,
    ime_rect: Option<Rect>,
}

impl Context {
    pub fn new(fonts: Fonts, platform: Rc<dyn Platform>) -> Self {
        Context {
            fonts,
            platform,
            time: 0.0,
            screen: Vec2::ZERO,
            pointer: None,
            last_pointer: None,
            modifiers: Modifiers::NONE,
            button_down: [false; 3],
            prev_hits: Vec::new(),
            hits: Vec::new(),
            hovered: None,
            top_layer: None,
            active: None,
            active_secondary: None,
            active_middle: None,
            drag_started: false,
            focus: None,
            focus_at_start: None,
            focus_claimed: false,
            lost_pending: None,
            lost_reported: None,
            focus_rect: None,
            focusables: Vec::new(),
            keyboard_nav: false,
            memory: HashMap::new(),
            open_popup: None,
            popup_area: None,
            popup_area_next: None,
            tip_id: None,
            events: Vec::new(),
            pressed: None,
            released: None,
            wheel: Vec2::ZERO,
            a11y_actions: Vec::new(),
            close_requested: false,
            close_cancelled: false,
            list: DisplayList::default(),
            a11y: Vec::new(),
            cursor: CursorIcon::Default,
            cursor_set: false,
            repaint_after: None,
            unstable: false,
            ime_rect: None,
        }
    }

    // ---- frame lifecycle -------------------------------------------------

    pub fn begin_frame(&mut self, input: RawInput) {
        self.time = input.time;
        self.screen = input.screen;
        self.modifiers = input.modifiers;
        self.last_pointer = self.pointer;
        self.events.clear();
        self.pressed = None;
        self.released = None;
        self.wheel = Vec2::ZERO;
        self.a11y_actions.clear();
        self.close_requested = false;
        self.close_cancelled = false;
        for event in input.events {
            match event {
                Event::PointerMoved(p) => self.pointer = Some(p),
                Event::PointerGone => self.pointer = None,
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    double,
                } => {
                    self.pointer = Some(pos);
                    let index = button as usize;
                    self.button_down[index] = pressed;
                    if pressed {
                        if self.pressed.is_none() {
                            self.pressed = Some((button, pos, double));
                        }
                    } else if self.released.is_none() {
                        self.released = Some((button, pos));
                    }
                }
                Event::Wheel(delta) => self.wheel += delta,
                Event::FocusLost => {
                    self.button_down = [false; 3];
                    self.active = None;
                }
                Event::CloseRequested => self.close_requested = true,
                Event::A11y(action) => {
                    if let A11yAction::Focus(id) = &action {
                        self.focus = Some(Id(*id));
                        self.keyboard_nav = true;
                    }
                    self.a11y_actions.push(action);
                }
                other => self.events.push(other),
            }
        }
        self.prev_hits = std::mem::take(&mut self.hits);
        let (hovered, top) = self.hit_test(&self.prev_hits);
        self.hovered = hovered;
        self.top_layer = top;
        self.focus_at_start = self.focus;
        self.focus_claimed = false;
        self.lost_reported = None;
        self.focusables.clear();
        self.focus_rect = None;
        self.popup_area = self.popup_area_next.take().or(self.popup_area);
        if let Some((button, pos, _)) = self.pressed {
            self.keyboard_nav = false;
            match button {
                Button::Primary => {
                    self.active = self.hovered;
                    self.drag_started = false;
                }
                Button::Secondary => self.active_secondary = self.hovered,
                Button::Middle => self.active_middle = self.hovered,
            }
            // A press outside an open popup closes it (its trigger toggles).
            if self.open_popup.is_some() && !self.popup_area.is_some_and(|r| r.contains(pos)) {
                self.memory
                    .insert(Id::new("popup-closing"), Box::new(self.open_popup));
                self.open_popup = None;
            }
        }
        self.list = DisplayList::default();
        self.a11y.clear();
        self.cursor = CursorIcon::Default;
        self.cursor_set = false;
        self.repaint_after = None;
        self.unstable = false;
        self.ime_rect = None;
        TIP.with(|t| *t.borrow_mut() = None);
    }

    fn hit_test(&self, hits: &[Hit]) -> (Option<Id>, Option<Layer>) {
        let Some(p) = self.pointer else {
            return (None, None);
        };
        let top = hits
            .iter()
            .filter(|h| h.rect.contains(p))
            .map(|h| h.layer)
            .max();
        let hovered = hits
            .iter()
            .rev()
            .filter(|h| Some(h.layer) == top && h.sense.interactive() && h.rect.contains(p))
            .max_by_key(|h| h.layer)
            .map(|h| h.id);
        (hovered, top)
    }

    pub fn end_frame(&mut self) -> FrameOutput {
        // An unconsumed Tab moves keyboard focus.
        let tab = self.events.iter().position(|e| {
            matches!(
                e,
                Event::Key {
                    key: Key::Tab,
                    pressed: true,
                    modifiers,
                } if !modifiers.ctrl && !modifiers.alt
            )
        });
        if let Some(index) = tab {
            let backwards =
                matches!(self.events[index], Event::Key { modifiers, .. } if modifiers.shift);
            self.events.remove(index);
            if !self.focusables.is_empty() {
                let current = self
                    .focus
                    .and_then(|f| self.focusables.iter().position(|x| *x == f));
                let n = self.focusables.len();
                let next = match (current, backwards) {
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                    (None, false) => 0,
                    (None, true) => n - 1,
                };
                self.focus = Some(self.focusables[next]);
                self.keyboard_nav = true;
                self.unstable = true;
            }
        }
        // A press that no widget claimed focus with clears it.
        if self.pressed.is_some_and(|(b, _, _)| b == Button::Primary)
            && !self.focus_claimed
            && self.focus.is_some()
            && self.hovered != self.focus
        {
            self.focus = None;
        }
        if let Some(lost) = self.focus_at_start
            && self.focus != Some(lost)
            && self.lost_reported != Some(lost)
        {
            self.lost_pending = Some(lost);
            self.unstable = true;
        } else {
            self.lost_pending = None;
        }
        if self.released.is_some() {
            self.active = None;
            self.active_secondary = None;
            self.active_middle = None;
        }
        // Tooltip after a short hover.
        let tip = TIP.with(|t| t.borrow_mut().take());
        match tip {
            Some((id, rect, text)) => {
                let since = match self.tip_id {
                    Some((current, since)) if current == id => since,
                    _ => {
                        self.tip_id = Some((id, self.time));
                        self.time
                    }
                };
                let waited = self.time - since;
                if waited >= TOOLTIP_DELAY && self.active.is_none() {
                    self.paint_tooltip(rect, &text);
                } else {
                    self.request_repaint_after(TOOLTIP_DELAY - waited);
                }
            }
            None => self.tip_id = None,
        }
        // The focus ring after keyboard navigation.
        if self.keyboard_nav
            && let Some(rect) = self.focus_rect
        {
            let mut p = self.painter(Layer::Tooltip, Rect::EVERYTHING, 1.0);
            p.rect_stroke(
                rect.expand(2.0),
                8.0,
                Stroke::new(2.0, Color32::from_rgba_unmultiplied(76, 154, 255, 89)),
            );
        }
        let (hovered, top) = self.hit_test(&self.hits);
        let rerun = self.unstable || hovered != self.hovered || top != self.top_layer;
        let focus = self.focus.map(|f| f.0);
        FrameOutput {
            list: std::mem::take(&mut self.list),
            cursor: self.cursor,
            repaint_after: self.repaint_after,
            rerun,
            a11y: Tree {
                nodes: std::mem::take(&mut self.a11y),
                focus,
            },
            ime_rect: self.ime_rect,
            close: !self.close_cancelled,
        }
    }

    fn paint_tooltip(&mut self, anchor: Rect, text: &str) {
        let font = crate::text::Font::regular(12.0);
        let max = 360.0;
        let lines = self.fonts.wrap(text, font, max);
        let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
        let lh = font.line_height();
        let size = Vec2 {
            x: width + 16.0,
            y: lines.len() as f32 * lh + 10.0,
        };
        let pointer = self.pointer.unwrap_or(anchor.left_bottom());
        let mut pos = Pos2 {
            x: pointer.x + 4.0,
            y: anchor.bottom().max(pointer.y + 16.0) + 4.0,
        };
        if pos.x + size.x > self.screen.x - 4.0 {
            pos.x = (self.screen.x - 4.0 - size.x).max(4.0);
        }
        if pos.y + size.y > self.screen.y - 4.0 {
            pos.y = (anchor.top() - 4.0 - size.y).max(4.0);
        }
        let rect = Rect::from_min_size(pos, size);
        let mut p = self.painter(Layer::Tooltip, Rect::EVERYTHING, 1.0);
        p.shadow(
            rect,
            6.0,
            Vec2 { x: 0.0, y: 4.0 },
            12.0,
            Color32::from_black_alpha(90),
        );
        p.rect(
            rect,
            6.0,
            Color32::from_rgb(0x26, 0x26, 0x29),
            Stroke::new(1.0, Color32::from_rgb(0x35, 0x35, 0x38)),
        );
        for (i, line) in lines.iter().enumerate() {
            p.text(
                Pos2 {
                    x: rect.left() + 8.0,
                    y: rect.top() + 5.0 + i as f32 * lh,
                },
                &text[line.range.clone()],
                font,
                Color32::from_rgb(0xe8, 0xe8, 0xea),
            );
        }
        self.a11y
            .push(Node::new(Id::new("tooltip").0, Role::ToolTip, text, rect));
    }

    // ---- queries ----------------------------------------------------------

    pub fn time(&self) -> f64 {
        self.time
    }

    pub fn screen_rect(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.screen)
    }

    pub fn pointer(&self) -> Option<Pos2> {
        self.pointer
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    pub fn primary_down(&self) -> bool {
        self.button_down[0]
    }

    /// The primary button went down this frame, and where.
    pub fn press(&self) -> Option<Pos2> {
        self.pressed
            .filter(|(b, _, _)| *b == Button::Primary)
            .map(|(_, p, _)| p)
    }

    pub fn any_press(&self) -> Option<Pos2> {
        self.pressed.map(|(_, p, _)| p)
    }

    pub fn is_dragging_something(&self) -> bool {
        self.active.is_some() && self.button_down[0]
    }

    pub fn focused(&self) -> Option<Id> {
        self.focus
    }

    pub fn focused_at_frame_start(&self) -> Option<Id> {
        self.focus_at_start
    }

    pub fn has_focus(&self, id: Id) -> bool {
        self.focus == Some(id)
    }

    pub fn request_focus(&mut self, id: Id) {
        self.focus = Some(id);
        self.focus_claimed = true;
    }

    pub fn surrender_focus(&mut self, id: Id) {
        if self.focus == Some(id) {
            self.focus = None;
            self.lost_reported = Some(id);
        }
    }

    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// Keeps the window open after a close request this frame.
    pub fn cancel_close(&mut self) {
        self.close_cancelled = true;
    }

    // ---- keyboard ---------------------------------------------------------

    /// Whether `key` was pressed this frame (with any modifiers), without
    /// consuming it.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, Event::Key { key: k, pressed: true, .. } if *k == key))
    }

    /// Takes a key press with exactly these modifiers (Shift ignored when
    /// `modifiers` doesn't ask for it and the key isn't a shortcut).
    pub fn consume_key(&mut self, modifiers: Modifiers, key: Key) -> bool {
        let index = self.events.iter().position(|e| {
            matches!(e, Event::Key { key: k, pressed: true, modifiers: m }
                if *k == key && m.ctrl == modifiers.ctrl && m.alt == modifiers.alt
                    && (m.shift == modifiers.shift || (!modifiers.ctrl && !modifiers.shift)))
        });
        if let Some(i) = index {
            self.events.remove(i);
            true
        } else {
            false
        }
    }

    /// The key and text events left for the focused widget.
    pub(crate) fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn put_back_events(&mut self, events: Vec<Event>) {
        self.events.extend(events);
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Takes the scroll this frame (for the innermost scroll area under
    /// the pointer, which runs first).
    pub fn take_wheel(&mut self, horizontal: bool, vertical: bool) -> Vec2 {
        let mut out = Vec2::ZERO;
        let mut wheel = self.wheel;
        // Shift turns a vertical wheel horizontal, as on Windows.
        if horizontal && !vertical && wheel.x == 0.0 {
            wheel = Vec2 { x: wheel.y, y: 0.0 };
            if self.wheel.y != 0.0 {
                out.x = wheel.x;
                self.wheel.y = 0.0;
                return out;
            }
        }
        if horizontal && self.modifiers.shift && wheel.x == 0.0 {
            out.x = wheel.y;
            self.wheel.y = 0.0;
            return out;
        }
        if horizontal {
            out.x = wheel.x;
            self.wheel.x = 0.0;
        }
        if vertical {
            out.y = wheel.y;
            self.wheel.y = 0.0;
        }
        out
    }

    pub fn wheel(&self) -> Vec2 {
        self.wheel
    }

    // ---- accessibility actions -------------------------------------------

    fn take_a11y(&mut self, id: Id, f: impl Fn(&A11yAction) -> bool) -> bool {
        if let Some(i) = self.a11y_actions.iter().position(|a| f(a)) {
            let _ = id;
            self.a11y_actions.remove(i);
            true
        } else {
            false
        }
    }

    pub(crate) fn take_a11y_value(&mut self, id: Id) -> Option<String> {
        let i = self
            .a11y_actions
            .iter()
            .position(|a| matches!(a, A11yAction::SetValue(x, _) if *x == id.0))?;
        match self.a11y_actions.remove(i) {
            A11yAction::SetValue(_, v) => Some(v),
            _ => None,
        }
    }

    pub fn a11y_node(&mut self, node: Node) {
        self.a11y.push(node);
    }

    // ---- interaction ------------------------------------------------------

    pub(crate) fn interact(
        &mut self,
        id: Id,
        rect: Rect,
        clip: Rect,
        layer: Layer,
        sense: Sense,
        enabled: bool,
    ) -> Response {
        let visible = rect.intersect(clip);
        if visible.is_positive() {
            self.hits.push(Hit {
                id,
                rect: visible,
                layer,
                sense,
            });
        }
        let contains = self.pointer.is_some_and(|p| visible.contains(p));
        let layer_ok = self.top_layer.is_none_or(|top| layer >= top);
        let mut hovered = contains
            && layer_ok
            && if sense.interactive() {
                self.hovered == Some(id)
            } else {
                true
            };
        if self.active.is_some_and(|a| a != id) && self.button_down[0] {
            hovered = false;
        }
        let mut r = Response {
            id,
            rect,
            enabled,
            hovered,
            ..Default::default()
        };
        if !sense.interactive() {
            return r;
        }
        if sense.focusable && enabled {
            self.focusables.push(id);
        }
        r.has_focus = self.focus == Some(id);
        if r.has_focus {
            self.focus_rect = Some(rect);
        }
        let is_active = self.active == Some(id);
        r.is_pointer_down = is_active && self.button_down[0];
        if let Some((button, _, double)) = self.pressed {
            if button == Button::Primary {
                if self.hovered == Some(id) && contains {
                    r.pressed = true;
                    r.double_clicked = double && enabled;
                } else {
                    r.clicked_elsewhere = true;
                }
            }
        }
        if let Some((button, pos)) = self.released {
            let over = visible.contains(pos);
            match button {
                Button::Primary => {
                    r.clicked = is_active && over && enabled;
                    if is_active && sense.drag && self.drag_started {
                        r.drag_stopped = true;
                        r.clicked = false;
                    }
                }
                Button::Secondary => {
                    r.secondary_clicked = self.active_secondary == Some(id) && over && enabled
                }
                Button::Middle => {
                    r.middle_clicked = self.active_middle == Some(id) && over && enabled
                }
            }
        }
        if sense.drag && is_active && self.button_down[0] && enabled {
            if let (Some(p), Some(last)) = (self.pointer, self.last_pointer) {
                let delta = p - last;
                if delta != Vec2::ZERO || self.drag_started {
                    if !self.drag_started {
                        self.drag_started = true;
                        r.drag_started = true;
                    }
                    r.dragged = true;
                    r.drag_delta = delta;
                }
            }
        }
        // Enter or Space on the focused control clicks it.
        if r.has_focus && enabled && sense.focusable {
            if self.consume_key(Modifiers::NONE, Key::Enter)
                || self.consume_key(Modifiers::NONE, Key::Space)
            {
                r.clicked = true;
            }
        }
        if self.take_a11y(
            id,
            |a| matches!(a, A11yAction::Invoke(x) | A11yAction::Toggle(x) if *x == id.0),
        ) && enabled
        {
            r.clicked = true;
        }
        if hovered && !self.cursor_set && !sense.plain_cursor {
            self.cursor = if enabled {
                CursorIcon::PointingHand
            } else {
                CursorIcon::NotAllowed
            };
        }
        r.lost_focus = self.lost_pending == Some(id) && self.focus != Some(id);
        r
    }

    /// Mark a focus change the widget reported itself (Enter, Escape).
    pub(crate) fn report_lost(&mut self, id: Id) {
        self.lost_reported = Some(id);
    }

    pub(crate) fn lost_pending(&self) -> Option<Id> {
        self.lost_pending
    }

    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.cursor = cursor;
        self.cursor_set = true;
    }

    pub(crate) fn set_ime_rect(&mut self, rect: Rect) {
        self.ime_rect = Some(rect);
    }

    // ---- repaint ----------------------------------------------------------

    pub fn request_repaint(&mut self) {
        self.repaint_after = Some(0.0);
    }

    pub fn request_repaint_after(&mut self, seconds: f64) {
        let s = seconds.max(0.0);
        self.repaint_after = Some(self.repaint_after.map_or(s, |r| r.min(s)));
    }

    /// Asks for another pass before presenting: a size measured this frame
    /// changes the layout.
    pub fn request_rerun(&mut self) {
        self.unstable = true;
    }

    // ---- memory -----------------------------------------------------------

    pub fn data<T: Clone + 'static>(&self, id: Id) -> Option<T> {
        self.memory.get(&id)?.downcast_ref::<T>().cloned()
    }

    pub fn data_mut<T: 'static>(&mut self, id: Id, default: impl FnOnce() -> T) -> &mut T {
        if !self.memory.get(&id).is_some_and(|v| v.is::<T>()) {
            self.memory.insert(id, Box::new(default()));
        }
        self.memory
            .get_mut(&id)
            .and_then(|v| v.downcast_mut::<T>())
            .expect("inserted above")
    }

    pub fn insert_data<T: 'static>(&mut self, id: Id, value: T) {
        self.memory.insert(id, Box::new(value));
    }

    pub fn remove_data(&mut self, id: Id) {
        self.memory.remove(&id);
    }

    // ---- popups -----------------------------------------------------------

    pub fn is_popup_open(&self, id: Id) -> bool {
        self.open_popup == Some(id)
    }

    /// Toggles a popup; a press that just closed it (outside its area, on
    /// its own trigger) doesn't reopen it.
    pub fn toggle_popup(&mut self, id: Id) {
        let just_closed = self.data::<Option<Id>>(Id::new("popup-closing")).flatten() == Some(id);
        self.remove_data(Id::new("popup-closing"));
        if self.open_popup == Some(id) || just_closed {
            self.open_popup = None;
        } else {
            self.open_popup = Some(id);
            self.popup_area_next = None;
            self.popup_area = None;
        }
    }

    pub fn open_popup(&mut self, id: Id) {
        self.open_popup = Some(id);
        self.popup_area = None;
    }

    pub fn close_popup(&mut self) {
        self.open_popup = None;
    }

    pub(crate) fn set_popup_area(&mut self, rect: Rect) {
        self.popup_area_next = Some(rect);
        self.popup_area = Some(rect);
    }

    // ---- painting ---------------------------------------------------------

    pub fn painter(&mut self, layer: Layer, clip: Rect, opacity: f32) -> Painter<'_> {
        Painter {
            list: &mut self.list,
            layer,
            clip,
            opacity,
            fonts: &self.fonts,
        }
    }

    pub fn copy_text(&self, text: &str) {
        self.platform.set_clipboard(text);
    }

    pub fn open_url(&self, url: &str) {
        self.platform.open_url(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::pos2;
    use crate::input::Event;
    use crate::text::MonoMetrics;

    fn ctx() -> Context {
        Context::new(
            Fonts::new(Box::new(MonoMetrics)),
            Rc::new(NullPlatform::default()),
        )
    }

    fn frame(
        ctx: &mut Context,
        events: Vec<Event>,
        f: impl FnMut(&mut Context) -> Response,
    ) -> Response {
        let mut f = f;
        ctx.begin_frame(RawInput {
            events,
            screen: Vec2 { x: 400.0, y: 300.0 },
            time: 0.0,
            modifiers: Modifiers::NONE,
        });
        let r = f(ctx);
        let out = ctx.end_frame();
        if out.rerun {
            ctx.begin_frame(RawInput {
                events: vec![],
                screen: Vec2 { x: 400.0, y: 300.0 },
                time: 0.0,
                modifiers: Modifiers::NONE,
            });
            let r = f(ctx);
            ctx.end_frame();
            return r;
        }
        r
    }

    fn button(ctx: &mut Context, id: &str, rect: Rect, layer: Layer) -> Response {
        ctx.interact(
            Id::new(id),
            rect,
            Rect::EVERYTHING,
            layer,
            Sense::CLICK,
            true,
        )
    }

    fn press(p: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: p,
            button: Button::Primary,
            pressed,
            double: false,
        }
    }

    #[test]
    fn a_click_needs_press_and_release_on_the_same_widget() {
        let mut c = ctx();
        let a = Rect::from_ltrb(0.0, 0.0, 50.0, 50.0);
        let b = Rect::from_ltrb(60.0, 0.0, 110.0, 50.0);
        let both = |c: &mut Context| {
            let ra = button(c, "a", a, Layer::Background);
            let _ = button(c, "b", b, Layer::Background);
            ra
        };
        let p = pos2(10.0, 10.0);
        frame(&mut c, vec![Event::PointerMoved(p)], both);
        let r = frame(&mut c, vec![press(p, true)], both);
        assert!(r.pressed && !r.clicked);
        let r = frame(&mut c, vec![press(p, false)], both);
        assert!(r.clicked);
        // Pressed on a, released on b: neither clicks.
        frame(&mut c, vec![press(p, true)], both);
        let q = pos2(70.0, 10.0);
        let r = frame(&mut c, vec![Event::PointerMoved(q), press(q, false)], both);
        assert!(!r.clicked);
    }

    #[test]
    fn higher_layers_block_widgets_underneath() {
        let mut c = ctx();
        let r = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let p = pos2(10.0, 10.0);
        let f = |c: &mut Context| {
            let under = button(c, "under", r, Layer::Background);
            // A hover-only popup background still blocks the layer below.
            let _ = c.interact(
                Id::new("popup"),
                r,
                Rect::EVERYTHING,
                Layer::Popup,
                Sense::HOVER,
                true,
            );
            under
        };
        let under = frame(&mut c, vec![Event::PointerMoved(p)], f);
        assert!(!under.hovered);
    }

    #[test]
    fn hover_is_correct_on_the_first_presented_frame() {
        let mut c = ctx();
        let p = pos2(10.0, 10.0);
        let r = frame(&mut c, vec![Event::PointerMoved(p)], |c| {
            button(
                c,
                "a",
                Rect::from_ltrb(0.0, 0.0, 50.0, 50.0),
                Layer::Background,
            )
        });
        assert!(
            r.hovered,
            "the rerun pass sees the new widget under the pointer"
        );
    }

    #[test]
    fn pressing_elsewhere_clears_focus_and_reports_it_next_frame() {
        let mut c = ctx();
        let field = Id::new("field");
        c.focus = Some(field);
        let f = |c: &mut Context| {
            c.interact(
                field,
                Rect::from_ltrb(0.0, 0.0, 50.0, 20.0),
                Rect::EVERYTHING,
                Layer::Background,
                Sense::CLICK,
                true,
            )
        };
        let q = pos2(200.0, 200.0);
        frame(&mut c, vec![Event::PointerMoved(q)], f);
        frame(&mut c, vec![press(q, true)], f);
        assert_eq!(c.focused(), None);
    }
}
