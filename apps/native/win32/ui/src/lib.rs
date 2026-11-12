//! Anybase's Windows toolkit. Apps build each frame with [`Ui`] (immediate
//! mode, like egui), which records a display list and an accessibility
//! tree; the Win32 backend (`win32`, Windows only) renders the list with
//! Direct2D and DirectWrite into a DirectComposition swap chain and serves
//! the tree through UI Automation.

pub mod a11y;
pub mod ctx;
pub mod geom;
pub mod icons;
pub mod input;
pub mod paint;
#[cfg(feature = "raster")]
pub mod raster;
pub mod scroll;
pub mod text;
pub mod text_edit;
pub mod theme;
pub mod ui;
pub mod widgets;
#[cfg(windows)]
pub mod win32;

pub use ctx::{Context, CursorIcon, FrameOutput, Id, Platform, Response, Sense};
pub use geom::{Color32, Pos2, Radius, Rect, Stroke, Vec2, pos2, vec2};
pub use input::{Event, Key, Modifiers, RawInput};
pub use paint::{DisplayList, Layer, Painter, Shape};
pub use scroll::ScrollArea;
pub use text::{Font, Fonts};
pub use ui::{Dir, Frame, Margin, Ui};
