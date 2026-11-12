//! Input events a backend feeds the toolkit, in DIPs.

use crate::geom::{Pos2, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    A,
    C,
    N,
    T,
    V,
    W,
    X,
    Y,
    Z,
    F10,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };
    /// Ctrl on Windows: the platform's command modifier.
    pub const COMMAND: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };

    pub fn command(&self) -> bool {
        self.ctrl
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
}

/// A request from an assistive technology (UI Automation).
#[derive(Clone, Debug, PartialEq)]
pub enum A11yAction {
    Invoke(u64),
    Focus(u64),
    SetValue(u64, String),
    Toggle(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    PointerMoved(Pos2),
    PointerButton {
        pos: Pos2,
        button: Button,
        pressed: bool,
        /// The second press of a double click.
        double: bool,
    },
    PointerGone,
    /// Scroll distance in DIPs; positive y scrolls content down (wheel up).
    Wheel(Vec2),
    Key {
        key: Key,
        pressed: bool,
        modifiers: Modifiers,
    },
    Text(String),
    /// The window lost keyboard focus.
    FocusLost,
    /// The person asked to close the window.
    CloseRequested,
    A11y(A11yAction),
}

#[derive(Clone, Debug, Default)]
pub struct RawInput {
    pub events: Vec<Event>,
    /// The content area in DIPs.
    pub screen: Vec2,
    /// Seconds since the app started.
    pub time: f64,
    pub modifiers: Modifiers,
}
