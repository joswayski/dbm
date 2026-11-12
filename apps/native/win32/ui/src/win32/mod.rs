//! The Win32 backend: one top-level window composed with DirectComposition,
//! a message loop that renders only when something changed (input, a
//! wake from a background thread, or a timer the frame asked for), and a
//! UI Automation provider over the frame's accessibility tree.

mod render;
mod uia;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Threading::INFINITE;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::a11y::Tree;
use crate::ctx::{Context, CursorIcon, Platform};
use crate::geom::{Color32, Vec2, pos2, vec2};
use crate::input::{Button, Event, Key, Modifiers, RawInput};
use crate::paint::Layer;
use crate::text::Fonts;
use crate::ui::Ui;

pub use render::wide;

/// What an app built on the toolkit implements.
pub trait App {
    /// Builds one frame.
    fn update(&mut self, ui: &mut Ui);
    /// Asked after each presented frame: a PNG to write of it (`--snapshot-dir`).
    fn snapshot_path(&mut self) -> Option<PathBuf> {
        None
    }
    /// Asked after each frame: quit now (snapshots done).
    fn should_exit(&self) -> bool {
        false
    }
    /// The window is closing; save state.
    fn on_exit(&mut self) {}
}

pub struct Options {
    pub title: String,
    /// Client size in DIPs.
    pub size: Vec2,
    pub min_size: Vec2,
    /// Where the window's position and size persist between launches.
    pub placement_file: Option<PathBuf>,
    /// Bring the window to the front (after a self-update restart).
    pub focus: bool,
    /// Off-screen rendering only (snapshots without showing a window).
    pub hidden: bool,
}

/// Wakes the UI thread from any thread: posts a message to the window.
#[derive(Clone, Default)]
pub struct Waker(Arc<AtomicIsize>);

impl Waker {
    pub fn wake(&self) {
        let hwnd = self.0.load(Ordering::Acquire);
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_WAKE, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn callback(&self) -> Arc<dyn Fn() + Send + Sync> {
        let w = self.clone();
        Arc::new(move || w.wake())
    }
}

const WM_WAKE: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const BACKGROUND: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1e);

struct WinPlatform {
    hwnd: RefCell<HWND>,
}

impl Platform for WinPlatform {
    fn set_clipboard(&self, text: &str) {
        unsafe {
            let hwnd = *self.hwnd.borrow();
            if OpenClipboard(Some(hwnd)).is_err() {
                return;
            }
            let _ = EmptyClipboard();
            let mut w = wide(text);
            w.push(0);
            let bytes = w.len() * 2;
            if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
                let ptr = GlobalLock(mem) as *mut u16;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(w.as_ptr(), ptr, w.len());
                    let _ = GlobalUnlock(mem);
                    let _ = SetClipboardData(13 /* CF_UNICODETEXT */, Some(HANDLE(mem.0)));
                }
            }
            let _ = CloseClipboard();
        }
    }

    fn clipboard(&self) -> Option<String> {
        unsafe {
            let hwnd = *self.hwnd.borrow();
            OpenClipboard(Some(hwnd)).ok()?;
            let out = (|| {
                let h = GetClipboardData(13).ok()?;
                let mem = HGLOBAL(h.0);
                let ptr = GlobalLock(mem) as *const u16;
                if ptr.is_null() {
                    return None;
                }
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                let _ = GlobalUnlock(mem);
                Some(s)
            })();
            let _ = CloseClipboard();
            out
        }
    }

    fn open_url(&self, url: &str) {
        let mut w = wide(url);
        w.push(0);
        unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }
}

pub(crate) struct State {
    hwnd: HWND,
    ctx: Context,
    app: Box<dyn App>,
    renderer: render::Renderer,
    _platform: Rc<WinPlatform>,
    events: Vec<Event>,
    start: Instant,
    scale: f32,
    size_px: (u32, u32),
    cursor: CursorIcon,
    pub(crate) tree: Tree,
    dirty: bool,
    next_frame: Option<Instant>,
    high_surrogate: Option<u16>,
    tracking: bool,
    min_size: Vec2,
    closing: bool,
    placement_file: Option<PathBuf>,
    frames: u64,
    /// `DBM_FRAME_LOG`: a file holding the presented frame count, rewritten
    /// at most every 250 ms (for the benchmark).
    frame_log: Option<(PathBuf, f64)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs `f` with the app state, unless it is already borrowed (re-entrant
/// window messages during a frame are then handled by DefWindowProc).
pub(crate) fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

/// Creates the window and runs the message loop until it closes.
pub fn run(options: Options, make: impl FnOnce(Waker) -> Box<dyn App>) -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let instance = GetModuleHandleW(None)?;
        let class = w!("AnybaseWindow");
        let icon = LoadIconW(Some(HINSTANCE(instance.0)), PCWSTR(1 as _)).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE(instance.0),
            hIcon: icon,
            hIconSm: icon,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let waker = Waker::default();
        let mut title = wide(&options.title);
        title.push(0);
        let style = WS_OVERLAPPEDWINDOW;
        let placement = options.placement_file.as_deref().and_then(read_placement);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_APPWINDOW,
            class,
            PCWSTR(title.as_ptr()),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )?;
        let dark: BOOL = true.into();
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&dark as *const BOOL).cast(), 4);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let scale = dpi as f32 / 96.0;
        // Size the client area in DIPs, centred on the window's monitor,
        // unless a saved placement fits on screen.
        let mut wr = RECT {
            left: 0,
            top: 0,
            right: (options.size.x * scale) as i32,
            bottom: (options.size.y * scale) as i32,
        };
        let _ = AdjustWindowRectExForDpi(&mut wr, style, false, WS_EX_NOREDIRECTIONBITMAP, dpi);
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut mi);
        let work = mi.rcWork;
        let (w, h) = (
            (wr.right - wr.left).min(work.right - work.left),
            (wr.bottom - wr.top).min(work.bottom - work.top),
        );
        let mut show = SW_SHOWNORMAL;
        match placement {
            Some((x, y, pw, ph, maximized)) if pw >= 200 && ph >= 150 && on_screen(x, y, pw, ph) => {
                let _ = SetWindowPos(hwnd, None, x, y, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE);
                if maximized {
                    show = SW_SHOWMAXIMIZED;
                }
            }
            _ => {
                let x = work.left + (work.right - work.left - w) / 2;
                let y = work.top + (work.bottom - work.top - h) / 2;
                let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let size_px = ((client.right - client.left).max(1) as u32, (client.bottom - client.top).max(1) as u32);
        let text = render::Text::new()?;
        let renderer = render::Renderer::new(Some(hwnd), text.clone(), size_px.0, size_px.1)?;
        let platform = Rc::new(WinPlatform { hwnd: RefCell::new(hwnd) });
        let ctx = Context::new(Fonts::new(Box::new(render::DWriteMetrics(text))), platform.clone());
        waker.0.store(hwnd.0 as isize, Ordering::Release);
        let app = make(waker);
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                ctx,
                app,
                renderer,
                _platform: platform,
                events: Vec::new(),
                start: Instant::now(),
                scale,
                size_px,
                cursor: CursorIcon::Default,
                tree: Tree::default(),
                dirty: true,
                next_frame: None,
                high_surrogate: None,
                tracking: false,
                min_size: options.min_size,
                closing: false,
                placement_file: options.placement_file.clone(),
                frames: 0,
                frame_log: std::env::var_os("DBM_FRAME_LOG").map(|p| (PathBuf::from(p), 0.0)),
            })
        });
        // The first frame renders before the window shows, so it never
        // flashes empty.
        with_state(|s| s.frame());
        if !options.hidden {
            let _ = ShowWindow(hwnd, show);
            if options.focus {
                let _ = SetForegroundWindow(hwnd);
            }
        }
        let mut msg = MSG::default();
        loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let wait = with_state(|s| {
                if s.closing {
                    return None;
                }
                let now = Instant::now();
                if s.next_frame.is_some_and(|t| t <= now) {
                    s.next_frame = None;
                    s.dirty = true;
                }
                if s.dirty {
                    s.frame();
                }
                if s.app.should_exit() {
                    s.closing = true;
                    let _ = DestroyWindow(s.hwnd);
                    return Some(0);
                }
                Some(match s.next_frame {
                    _ if s.dirty => 0,
                    Some(t) => t.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32,
                    None => INFINITE,
                })
            });
            let Some(wait) = wait.flatten() else {
                continue;
            };
            if wait > 0 {
                MsgWaitForMultipleObjectsEx(None, wait, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
        }
    }
}

fn on_screen(x: i32, y: i32, w: i32, h: i32) -> bool {
    unsafe {
        let r = RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        !MonitorFromRect(&r, MONITOR_DEFAULTTONULL).is_invalid()
    }
}

fn read_placement(path: &std::path::Path) -> Option<(i32, i32, i32, i32, bool)> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Vec<i32> = text.split_whitespace().filter_map(|s| s.parse().ok()).collect();
    (v.len() == 5).then(|| (v[0], v[1], v[2], v[3], v[4] != 0))
}

impl State {
    fn frame(&mut self) {
        self.dirty = false;
        let screen = vec2(self.size_px.0 as f32 / self.scale, self.size_px.1 as f32 / self.scale);
        let mut events = std::mem::take(&mut self.events);
        let close_requested = events.iter().any(|e| matches!(e, Event::CloseRequested));
        let mut output = None;
        for pass in 0..4 {
            let input = RawInput {
                events: std::mem::take(&mut events),
                screen,
                time: self.start.elapsed().as_secs_f64(),
                modifiers: modifiers(),
            };
            self.ctx.begin_frame(input);
            {
                let mut ui = Ui::root(&mut self.ctx, crate::geom::Rect::from_min_size(pos2(0.0, 0.0), screen), Layer::Background);
                self.app.update(&mut ui);
            }
            let out = self.ctx.end_frame();
            let rerun = out.rerun;
            let close = out.close;
            output = Some(out);
            if close_requested && pass == 0 && close {
                self.close();
                return;
            }
            if !rerun {
                break;
            }
        }
        let Some(out) = output else { return };
        if let Err(e) = self.renderer.present(&out.list, self.scale, BACKGROUND) {
            eprintln!("present failed: {e}");
        }
        self.frames += 1;
        let now = self.start.elapsed().as_secs_f64();
        if let Some((path, written)) = &mut self.frame_log
            && now - *written >= 0.25
        {
            *written = now;
            let _ = std::fs::write(path, self.frames.to_string());
        }
        if let Some(path) = self.app.snapshot_path() {
            let (w, h) = self.size_px;
            if let Err(e) = self.renderer.snapshot(&out.list, w, h, self.scale, BACKGROUND, &path) {
                eprintln!("snapshot {} failed: {e}", path.display());
            }
            self.dirty = true;
        }
        if out.cursor != self.cursor {
            self.cursor = out.cursor;
            unsafe { set_cursor(self.cursor) };
        }
        if let Some(r) = out.ime_rect {
            unsafe {
                let himc = ImmGetContext(self.hwnd);
                if !himc.is_invalid() {
                    let form = COMPOSITIONFORM {
                        dwStyle: CFS_POINT,
                        ptCurrentPos: POINT {
                            x: (r.min.x * self.scale) as i32,
                            y: (r.min.y * self.scale) as i32,
                        },
                        ..Default::default()
                    };
                    let _ = ImmSetCompositionWindow(himc, &form);
                    let _ = ImmReleaseContext(self.hwnd, himc);
                }
            }
        }
        let focus_changed = out.a11y.focus != self.tree.focus;
        self.tree = out.a11y;
        if focus_changed {
            uia::focus_changed(&self.tree);
        }
        self.next_frame = out
            .repaint_after
            .map(|s| Instant::now() + std::time::Duration::from_secs_f64(s.max(0.0)));
        if out.repaint_after == Some(0.0) {
            self.dirty = true;
        }
    }

    fn close(&mut self) {
        self.app.on_exit();
        if let Some(path) = &self.placement_file {
            unsafe {
                let mut wp = WINDOWPLACEMENT {
                    length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                    ..Default::default()
                };
                if GetWindowPlacement(self.hwnd, &mut wp).is_ok() {
                    let r = wp.rcNormalPosition;
                    let maximized = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32;
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(path, format!("{} {} {} {} {}", r.left, r.top, r.right - r.left, r.bottom - r.top, maximized as i32));
                }
            }
        }
        self.closing = true;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.dirty = true;
    }

    fn dip(&self, lp: LPARAM) -> crate::geom::Pos2 {
        let x = (lp.0 & 0xffff) as i16 as f32;
        let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
        pos2(x / self.scale, y / self.scale)
    }

    pub(crate) fn screen_origin(&self) -> (f64, f64) {
        let mut p = POINT::default();
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut p);
        }
        (p.x as f64, p.y as f64)
    }

    pub(crate) fn scale(&self) -> f32 {
        self.scale
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub(crate) fn a11y_action(&mut self, action: crate::input::A11yAction) {
        self.push(Event::A11y(action));
    }
}

unsafe fn set_cursor(c: CursorIcon) {
    let id = match c {
        CursorIcon::Default => IDC_ARROW,
        CursorIcon::PointingHand => IDC_HAND,
        CursorIcon::Text => IDC_IBEAM,
        CursorIcon::ResizeHorizontal => IDC_SIZEWE,
        CursorIcon::ResizeVertical => IDC_SIZENS,
        CursorIcon::NotAllowed => IDC_NO,
    };
    unsafe {
        if let Ok(h) = LoadCursorW(None, id) {
            SetCursor(Some(h));
        }
    }
}

fn modifiers() -> Modifiers {
    unsafe {
        let down = |vk: VIRTUAL_KEY| (GetKeyState(vk.0 as i32) as u16 & 0x8000) != 0;
        Modifiers {
            shift: down(VK_SHIFT),
            ctrl: down(VK_CONTROL),
            alt: down(VK_MENU),
        }
    }
}

fn map_key(vk: u16) -> Option<Key> {
    Some(match VIRTUAL_KEY(vk) {
        VK_RETURN => Key::Enter,
        VK_ESCAPE => Key::Escape,
        VK_TAB => Key::Tab,
        VK_BACK => Key::Backspace,
        VK_DELETE => Key::Delete,
        VK_SPACE => Key::Space,
        VK_LEFT => Key::ArrowLeft,
        VK_RIGHT => Key::ArrowRight,
        VK_UP => Key::ArrowUp,
        VK_DOWN => Key::ArrowDown,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_F10 => Key::F10,
        _ => match vk as u8 {
            b'A' => Key::A,
            b'C' => Key::C,
            b'N' => Key::N,
            b'T' => Key::T,
            b'V' => Key::V,
            b'W' => Key::W,
            b'X' => Key::X,
            b'Y' => Key::Y,
            b'Z' => Key::Z,
            _ => return None,
        },
    })
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_GETOBJECT => {
                // Outside the state borrow: UIA calls straight back into the providers.
                if lp.0 as i32 == UiaRootObjectId {
                    return UiaReturnRawElementProvider(hwnd, wp, lp, &uia::root());
                }
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            WM_DESTROY => {
                let _ = UiaReturnRawElementProvider(hwnd, WPARAM(0), LPARAM(0), None);
                PostQuitMessage(0);
                return LRESULT(0);
            }
            WM_SYSCOMMAND if (wp.0 & 0xFFF0) as u32 == SC_KEYMENU => return LRESULT(0),
            WM_ERASEBKGND => return LRESULT(1),
            _ => {}
        }
        let handled = with_state(|s| handle(s, msg, wp, lp));
        match handled {
            Some(Some(r)) => r,
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe fn handle(s: &mut State, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    unsafe {
        let button = |s: &mut State, button: Button, pressed: bool, double: bool| {
            let pos = s.dip(lp);
            if pressed {
                SetCapture(s.hwnd);
                let _ = SetFocus(Some(s.hwnd));
            } else {
                let _ = ReleaseCapture();
            }
            s.push(Event::PointerButton { pos, button, pressed, double });
            Some(LRESULT(0))
        };
        match msg {
            WM_WAKE => {
                s.dirty = true;
                Some(LRESULT(0))
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(s.hwnd, &mut ps);
                let _ = EndPaint(s.hwnd, &ps);
                s.dirty = true;
                Some(LRESULT(0))
            }
            WM_SIZE => {
                let w = (lp.0 & 0xffff) as u32;
                let h = ((lp.0 >> 16) & 0xffff) as u32;
                if w > 0 && h > 0 {
                    s.size_px = (w, h);
                    s.renderer.resize(w, h);
                    // Resizing runs a modal loop: draw now.
                    s.frame();
                }
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                s.scale = ((wp.0 & 0xffff) as f32 / 96.0).max(1.0);
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(s.hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                s.dirty = true;
                Some(LRESULT(0))
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(lp.0 as *mut MINMAXINFO);
                let dpi = GetDpiForWindow(s.hwnd).max(96);
                let mut r = RECT {
                    left: 0,
                    top: 0,
                    right: (s.min_size.x * s.scale) as i32,
                    bottom: (s.min_size.y * s.scale) as i32,
                };
                let _ = AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPEDWINDOW, false, WS_EX_NOREDIRECTIONBITMAP, dpi);
                info.ptMinTrackSize = POINT {
                    x: r.right - r.left,
                    y: r.bottom - r.top,
                };
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                if !s.tracking {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: s.hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                    s.tracking = true;
                }
                let p = s.dip(lp);
                s.push(Event::PointerMoved(p));
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                s.tracking = false;
                s.push(Event::PointerGone);
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => button(s, Button::Primary, true, false),
            WM_LBUTTONDBLCLK => button(s, Button::Primary, true, true),
            WM_LBUTTONUP => button(s, Button::Primary, false, false),
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => button(s, Button::Secondary, true, false),
            WM_RBUTTONUP => button(s, Button::Secondary, false, false),
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => button(s, Button::Middle, true, false),
            WM_MBUTTONUP => button(s, Button::Middle, false, false),
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0 * crate::scroll::WHEEL_STEP;
                // The wheel's point is in screen coordinates.
                let mut p = POINT {
                    x: (lp.0 & 0xffff) as i16 as i32,
                    y: ((lp.0 >> 16) & 0xffff) as i16 as i32,
                };
                let _ = ScreenToClient(s.hwnd, &mut p);
                s.push(Event::PointerMoved(pos2(p.x as f32 / s.scale, p.y as f32 / s.scale)));
                let v = if msg == WM_MOUSEWHEEL { vec2(0.0, delta) } else { vec2(-delta, 0.0) };
                s.push(Event::Wheel(v));
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                if (lp.0 & 0xffff) as u32 == HTCLIENT {
                    set_cursor(s.cursor);
                    Some(LRESULT(1))
                } else {
                    None
                }
            }
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
                let pressed = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                if let Some(key) = map_key(wp.0 as u16) {
                    s.push(Event::Key { key, pressed, modifiers: modifiers() });
                    // Alt+F4 and other system keys still reach DefWindowProc.
                    if msg == WM_SYSKEYDOWN || msg == WM_SYSKEYUP {
                        return None;
                    }
                    return Some(LRESULT(0));
                }
                None
            }
            WM_CHAR => {
                let unit = wp.0 as u16;
                let text = if (0xD800..0xDC00).contains(&unit) {
                    s.high_surrogate = Some(unit);
                    return Some(LRESULT(0));
                } else if (0xDC00..0xE000).contains(&unit) {
                    match s.high_surrogate.take() {
                        Some(high) => String::from_utf16_lossy(&[high, unit]),
                        None => return Some(LRESULT(0)),
                    }
                } else {
                    String::from_utf16_lossy(&[unit])
                };
                let ctrl = modifiers().ctrl && !modifiers().alt;
                // Control characters arrive as keys; Ctrl+letter is a shortcut.
                if !ctrl && text.chars().all(|c| !c.is_control()) {
                    s.push(Event::Text(text));
                } else if text == "\r" {
                    // Enter is a key event.
                }
                Some(LRESULT(0))
            }
            WM_KILLFOCUS => {
                s.push(Event::FocusLost);
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                s.push(Event::CloseRequested);
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}
