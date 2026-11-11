//! The UI-free part of the Windows and Linux desktop apps: the serialized
//! database worker and the self-updater. Each UI toolkit supplies a [`Wake`]
//! that schedules its next frame or main-loop turn when work finishes.

pub mod updates;
pub mod worker;

/// Called from background threads when results are waiting, so the UI
/// thread drains them.
pub type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;
