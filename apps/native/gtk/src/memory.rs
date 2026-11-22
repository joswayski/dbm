//! Gives freed memory back to the system. glibc keeps freed blocks in its
//! heap: after a few 10,000-row results and their grids are closed, the
//! process held about 230 MB that nothing used. `malloc_trim` returns the
//! free pages.

use std::cell::Cell;
use std::time::Duration;

use gtk::glib;

/// How long after the last release to wait, so a burst of closes trims
/// once and work in progress reuses what it just freed.
const QUIET: Duration = Duration::from_secs(3);

thread_local! {
    /// Bumped by every request; only the newest one trims.
    static REQUEST: Cell<u64> = const { Cell::new(0) };
}

/// Trims the heap once the app has released nothing more for `QUIET`. Call
/// after dropping something large: a closed tab, a replaced result, a
/// popover's surface.
pub fn trim_soon() {
    let request = REQUEST.with(|current| {
        current.set(current.get() + 1);
        current.get()
    });
    glib::timeout_add_local_once(QUIET, move || {
        if REQUEST.with(Cell::get) == request {
            trim();
        }
    });
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
fn trim() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: glibc's malloc_trim takes no pointers and locks each arena
    // itself; it only releases pages that hold no live allocation.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim() {}
