//! Direct2D/DirectWrite rendering into a DirectComposition swap chain.
//!
//! One D3D11 device (hardware, else WARP on machines without a GPU) backs a
//! flip-model composition swap chain that a DirectComposition visual shows
//! in the window. Each frame replays the toolkit's display list with
//! Direct2D in DIPs (`SetDpi`), text through DirectWrite with Space Mono
//! loaded from memory. Text layouts are cached between frames.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::*;
use windows_numerics::Vector2;

use crate::geom::{Color32, Pos2, Radius, Rect};
use crate::paint::{DisplayList, Shape};
use crate::text::{Font, FontBackend};

const SPACE_MONO: [&[u8]; 4] = [
    include_bytes!("../../../../workbench/assets/SpaceMono-Regular.ttf"),
    include_bytes!("../../../../workbench/assets/SpaceMono-Bold.ttf"),
    include_bytes!("../../../../workbench/assets/SpaceMono-Italic.ttf"),
    include_bytes!("../../../../workbench/assets/SpaceMono-BoldItalic.ttf"),
];

fn color(c: Color32) -> D2D1_COLOR_F {
    let [r, g, b, a] = c.to_unmultiplied_f32();
    D2D1_COLOR_F { r, g, b, a }
}

fn rectf(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.min.x,
        top: r.min.y,
        right: r.max.x,
        bottom: r.max.y,
    }
}

fn pt(p: Pos2) -> Vector2 {
    Vector2 { X: p.x, Y: p.y }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// DirectWrite: the Space Mono collection, text formats and glyph advances.
pub struct Text {
    pub dwrite: IDWriteFactory5,
    fonts: IDWriteFontCollection1,
    formats: RefCell<HashMap<(u32, u8), IDWriteTextFormat>>,
    faces: RefCell<HashMap<u8, (IDWriteFontFace, f32)>>,
    layouts: RefCell<LayoutCache>,
}

#[derive(Default)]
struct LayoutCache {
    map: HashMap<(String, u32, u8), (IDWriteTextLayout, u64)>,
    frame: u64,
}

fn style_bits(font: Font) -> u8 {
    font.bold as u8 | (font.italic as u8) << 1
}

impl Text {
    pub fn new() -> Result<Rc<Self>> {
        unsafe {
            let dwrite: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let loader = dwrite.CreateInMemoryFontFileLoader()?;
            dwrite.RegisterFontFileLoader(&loader)?;
            let builder = dwrite.CreateFontSetBuilder()?;
            for data in SPACE_MONO {
                let file = loader.CreateInMemoryFontFileReference(
                    &dwrite,
                    data.as_ptr().cast(),
                    data.len() as u32,
                    None,
                )?;
                builder.AddFontFile(&file)?;
            }
            let fonts = dwrite.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?;
            Ok(Rc::new(Text {
                dwrite,
                fonts,
                formats: RefCell::default(),
                faces: RefCell::default(),
                layouts: RefCell::default(),
            }))
        }
    }

    fn weight(font: Font) -> DWRITE_FONT_WEIGHT {
        if font.bold {
            DWRITE_FONT_WEIGHT_BOLD
        } else {
            DWRITE_FONT_WEIGHT_NORMAL
        }
    }

    fn style(font: Font) -> DWRITE_FONT_STYLE {
        if font.italic {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        }
    }

    fn format(&self, font: Font) -> Result<IDWriteTextFormat> {
        let key = (font.size.to_bits(), style_bits(font));
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let f = unsafe {
            let f = self.dwrite.CreateTextFormat(
                w!("Space Mono"),
                &self.fonts,
                Self::weight(font),
                Self::style(font),
                DWRITE_FONT_STRETCH_NORMAL,
                font.size,
                w!("en-us"),
            )?;
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            // Every line box is Space Mono's, whatever fallback font a run uses.
            f.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                font.line_height(),
                font.size * crate::text::ASCENT,
            )?;
            f
        };
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    fn face(&self, font: Font) -> Option<(IDWriteFontFace, f32)> {
        let key = style_bits(font);
        if let Some(f) = self.faces.borrow().get(&key) {
            return Some(f.clone());
        }
        let face = unsafe {
            let family = self.fonts.GetFontFamily(0).ok()?;
            let f = family
                .GetFirstMatchingFont(
                    Self::weight(font),
                    DWRITE_FONT_STRETCH_NORMAL,
                    Self::style(font),
                )
                .ok()?;
            let face = f.CreateFontFace().ok()?;
            let mut m = DWRITE_FONT_METRICS::default();
            face.GetMetrics(&mut m);
            (face, m.designUnitsPerEm as f32)
        };
        self.faces.borrow_mut().insert(key, face.clone());
        Some(face)
    }

    /// A cached single-line layout of `text`.
    pub fn layout(
        &self,
        text: &str,
        font: Font,
        underline: bool,
        strike: bool,
    ) -> Result<IDWriteTextLayout> {
        let bits = style_bits(font) | (underline as u8) << 2 | (strike as u8) << 3;
        let key = (text.to_owned(), font.size.to_bits(), bits);
        let mut cache = self.layouts.borrow_mut();
        let frame = cache.frame;
        if let Some((l, used)) = cache.map.get_mut(&key) {
            *used = frame;
            return Ok(l.clone());
        }
        let w = wide(text);
        let layout = unsafe {
            let l = self
                .dwrite
                .CreateTextLayout(&w, &self.format(font)?, 100_000.0, 1_000.0)?;
            let range = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: w.len() as u32,
            };
            if underline {
                l.SetUnderline(true, range)?;
            }
            if strike {
                l.SetStrikethrough(true, range)?;
            }
            l
        };
        cache.map.insert(key, (layout.clone(), frame));
        Ok(layout)
    }

    /// Drops layouts no frame used lately.
    pub fn end_frame(&self) {
        let mut cache = self.layouts.borrow_mut();
        cache.frame += 1;
        if cache.map.len() > 6000 {
            let keep = cache.frame.saturating_sub(2);
            cache.map.retain(|_, (_, used)| *used >= keep);
        }
    }
}

/// Glyph advances from Space Mono's design metrics; characters it lacks
/// are measured through DirectWrite's system fallback.
pub struct DWriteMetrics(pub Rc<Text>);

impl FontBackend for DWriteMetrics {
    fn advance(&self, ch: char, font: Font) -> f32 {
        let text = &self.0;
        if let Some((face, upem)) = text.face(font) {
            let cp = ch as u32;
            let mut index = 0u16;
            unsafe {
                if face.GetGlyphIndices(&cp, 1, &mut index).is_ok() && index != 0 {
                    let mut m = DWRITE_GLYPH_METRICS::default();
                    if face.GetDesignGlyphMetrics(&index, 1, &mut m, false).is_ok() {
                        return m.advanceWidth as f32 / upem * font.size;
                    }
                }
            }
        }
        let mut buf = [0u8; 4];
        text.layout(ch.encode_utf8(&mut buf), font, false, false)
            .ok()
            .map(|l| unsafe {
                let mut m = DWRITE_TEXT_METRICS::default();
                let _ = l.GetMetrics(&mut m);
                m.widthIncludingTrailingWhitespace
            })
            .unwrap_or(font.size * 0.612)
    }
}

pub struct Renderer {
    pub text: Rc<Text>,
    d3d: ID3D11Device,
    factory: ID2D1Factory,
    dc: ID2D1DeviceContext,
    swap: Option<IDXGISwapChain1>,
    dcomp: Option<IDCompositionDevice>,
    _target: Option<IDCompositionTarget>,
    _visual: Option<IDCompositionVisual>,
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    size: (u32, u32),
    pub software: bool,
}

impl Renderer {
    pub fn new(hwnd: Option<HWND>, text: Rc<Text>, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let (d3d, software) = create_device()?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let device = factory.CreateDevice(&dxgi)?;
            let dc = device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = dc.CreateSolidColorBrush(&color(Color32::WHITE), None)?;
            let factory0: ID2D1Factory = factory.cast()?;
            let round = factory0.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            let mut r = Renderer {
                text,
                d3d,
                factory: factory0,
                dc,
                swap: None,
                dcomp: None,
                _target: None,
                _visual: None,
                brush,
                round,
                size: (width.max(1), height.max(1)),
                software,
            };
            if let Some(hwnd) = hwnd {
                r.attach(hwnd)?;
            }
            Ok(r)
        }
    }

    /// Creates the composition swap chain and shows it in `hwnd`.
    unsafe fn attach(&mut self, hwnd: HWND) -> Result<()> {
        unsafe {
            let dxgi: IDXGIDevice = self.d3d.cast()?;
            let adapter = dxgi.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: self.size.0,
                Height: self.size.1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                Stereo: false.into(),
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: 0,
            };
            let swap = factory.CreateSwapChainForComposition(&self.d3d, &desc, None)?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;
            self.swap = Some(swap);
            self.dcomp = Some(dcomp);
            self._target = Some(target);
            self._visual = Some(visual);
            Ok(())
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if size == self.size {
            return;
        }
        self.size = size;
        if let Some(swap) = &self.swap {
            unsafe {
                self.dc.SetTarget(None);
                let _ = swap.ResizeBuffers(
                    0,
                    size.0,
                    size.1,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                );
            }
        }
    }

    fn target_props(scale: f32, target_only: bool) -> D2D1_BITMAP_PROPERTIES1 {
        D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
            },
            dpiX: 96.0 * scale,
            dpiY: 96.0 * scale,
            bitmapOptions: if target_only {
                D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW
            } else {
                D2D1_BITMAP_OPTIONS_TARGET
            },
            colorContext: std::mem::ManuallyDrop::new(None),
        }
    }

    /// Draws `list` into the swap chain and presents it.
    pub fn present(&mut self, list: &DisplayList, scale: f32, clear: Color32) -> Result<()> {
        let Some(swap) = self.swap.clone() else {
            return Ok(());
        };
        unsafe {
            let surface: IDXGISurface = swap.GetBuffer(0)?;
            let bmp = self
                .dc
                .CreateBitmapFromDxgiSurface(&surface, Some(&Self::target_props(scale, true)))?;
            self.dc.SetTarget(&bmp);
            self.draw(list, scale, clear);
            let result = self.dc.EndDraw(None, None);
            self.dc.SetTarget(None);
            result?;
            swap.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        self.text.end_frame();
        Ok(())
    }

    unsafe fn draw(&self, list: &DisplayList, scale: f32, clear: Color32) {
        let dc = &self.dc;
        unsafe {
            dc.SetDpi(96.0 * scale, 96.0 * scale);
            dc.BeginDraw();
            dc.SetTransform(&windows_numerics::Matrix3x2::identity());
            dc.Clear(Some(&color(clear)));
            let mut clip: Option<Rect> = None;
            for item in list.iter() {
                if clip != Some(item.clip) {
                    if clip.is_some() {
                        dc.PopAxisAlignedClip();
                    }
                    let c = item.clip.intersect(Rect::from_ltrb(-1e5, -1e5, 1e5, 1e5));
                    dc.PushAxisAlignedClip(&rectf(c), D2D1_ANTIALIAS_MODE_ALIASED);
                    clip = Some(item.clip);
                }
                self.shape(&item.shape);
            }
            if clip.is_some() {
                dc.PopAxisAlignedClip();
            }
        }
    }

    fn set(&self, c: Color32) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&color(c)) };
        &self.brush
    }

    unsafe fn shape(&self, shape: &Shape) {
        let dc = &self.dc;
        unsafe {
            match shape {
                Shape::Rect {
                    rect,
                    radius,
                    fill,
                    stroke,
                } => {
                    if !fill.is_transparent() {
                        self.rounded(
                            *rect,
                            *radius,
                            |g| dc.FillGeometry(g, self.set(*fill), None),
                            |rr| dc.FillRoundedRectangle(rr, self.set(*fill)),
                        );
                    }
                    if !stroke.is_none() {
                        // Inside the edge.
                        let r = rect.shrink(stroke.width / 2.0);
                        let rad = Radius {
                            nw: (radius.nw - stroke.width / 2.0).max(0.0),
                            ne: (radius.ne - stroke.width / 2.0).max(0.0),
                            sw: (radius.sw - stroke.width / 2.0).max(0.0),
                            se: (radius.se - stroke.width / 2.0).max(0.0),
                        };
                        let w = stroke.width;
                        self.rounded(
                            r,
                            rad,
                            |g| dc.DrawGeometry(g, self.set(stroke.color), w, None),
                            |rr| dc.DrawRoundedRectangle(rr, self.set(stroke.color), w, None),
                        );
                    }
                }
                Shape::Circle {
                    center,
                    radius,
                    fill,
                    stroke,
                } => {
                    let e = D2D1_ELLIPSE {
                        point: pt(*center),
                        radiusX: *radius,
                        radiusY: *radius,
                    };
                    if !fill.is_transparent() {
                        dc.FillEllipse(&e, self.set(*fill));
                    }
                    if !stroke.is_none() {
                        dc.DrawEllipse(&e, self.set(stroke.color), stroke.width, None);
                    }
                }
                Shape::Ellipse {
                    center,
                    radii,
                    stroke,
                } => {
                    let e = D2D1_ELLIPSE {
                        point: pt(*center),
                        radiusX: radii.x,
                        radiusY: radii.y,
                    };
                    dc.DrawEllipse(&e, self.set(stroke.color), stroke.width, None);
                }
                Shape::Path {
                    points,
                    closed,
                    stroke,
                } => {
                    if points.len() == 2 && !closed {
                        dc.DrawLine(
                            pt(points[0]),
                            pt(points[1]),
                            self.set(stroke.color),
                            stroke.width,
                            &self.round,
                        );
                    } else if let Some(g) = self.polyline(points, *closed, false) {
                        dc.DrawGeometry(&g, self.set(stroke.color), stroke.width, &self.round);
                    }
                }
                Shape::Polygon { points, fill } => {
                    if let Some(g) = self.polyline(points, true, true) {
                        dc.FillGeometry(&g, self.set(*fill), None);
                    }
                }
                Shape::Text {
                    pos,
                    text,
                    font,
                    color,
                    underline,
                    strikethrough,
                } => {
                    if let Ok(layout) = self.text.layout(text, *font, *underline, *strikethrough) {
                        dc.DrawTextLayout(
                            pt(*pos),
                            &layout,
                            self.set(*color),
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }
                }
                Shape::Shadow {
                    rect,
                    radius,
                    blur,
                    color: c,
                } => {
                    // A few widening, fading rounded rectangles: a soft edge
                    // without an effect graph.
                    let steps = 6;
                    for i in 0..steps {
                        let t = (i as f32 + 1.0) / steps as f32;
                        let grow = blur * 0.5 * t;
                        let alpha = 1.0 / steps as f32 * (1.0 - t * 0.6);
                        let rr = D2D1_ROUNDED_RECT {
                            rect: rectf(rect.expand(grow - blur * 0.25)),
                            radiusX: radius + grow,
                            radiusY: radius + grow,
                        };
                        dc.FillRoundedRectangle(&rr, self.set(c.gamma_multiply(alpha)));
                    }
                }
            }
        }
    }

    fn rounded(
        &self,
        r: Rect,
        radius: Radius,
        path: impl FnOnce(&ID2D1PathGeometry),
        uniform: impl FnOnce(&D2D1_ROUNDED_RECT),
    ) {
        if radius.is_uniform() {
            let rad = radius
                .nw
                .min(r.width() / 2.0)
                .min(r.height() / 2.0)
                .max(0.0);
            uniform(&D2D1_ROUNDED_RECT {
                rect: rectf(r),
                radiusX: rad,
                radiusY: rad,
            });
            return;
        }
        // Per-corner radii: a path of lines and arcs.
        let geo = unsafe {
            let Ok(geo) = self.factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = geo.Open() else { return };
            let m = r.width().min(r.height()) / 2.0;
            let (nw, ne, se, sw) = (
                radius.nw.min(m),
                radius.ne.min(m),
                radius.se.min(m),
                radius.sw.min(m),
            );
            let arc = |to: Pos2, rad: f32| D2D1_ARC_SEGMENT {
                point: pt(to),
                size: D2D_SIZE_F {
                    width: rad,
                    height: rad,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            };
            sink.BeginFigure(
                pt(Pos2 {
                    x: r.left() + nw,
                    y: r.top(),
                }),
                D2D1_FIGURE_BEGIN_FILLED,
            );
            sink.AddLine(pt(Pos2 {
                x: r.right() - ne,
                y: r.top(),
            }));
            if ne > 0.0 {
                sink.AddArc(&arc(
                    Pos2 {
                        x: r.right(),
                        y: r.top() + ne,
                    },
                    ne,
                ));
            }
            sink.AddLine(pt(Pos2 {
                x: r.right(),
                y: r.bottom() - se,
            }));
            if se > 0.0 {
                sink.AddArc(&arc(
                    Pos2 {
                        x: r.right() - se,
                        y: r.bottom(),
                    },
                    se,
                ));
            }
            sink.AddLine(pt(Pos2 {
                x: r.left() + sw,
                y: r.bottom(),
            }));
            if sw > 0.0 {
                sink.AddArc(&arc(
                    Pos2 {
                        x: r.left(),
                        y: r.bottom() - sw,
                    },
                    sw,
                ));
            }
            sink.AddLine(pt(Pos2 {
                x: r.left(),
                y: r.top() + nw,
            }));
            if nw > 0.0 {
                sink.AddArc(&arc(
                    Pos2 {
                        x: r.left() + nw,
                        y: r.top(),
                    },
                    nw,
                ));
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_err() {
                return;
            }
            geo
        };
        path(&geo);
    }

    fn polyline(&self, points: &[Pos2], closed: bool, filled: bool) -> Option<ID2D1PathGeometry> {
        let first = *points.first()?;
        unsafe {
            let geo = self.factory.CreatePathGeometry().ok()?;
            let sink = geo.Open().ok()?;
            sink.BeginFigure(
                pt(first),
                if filled {
                    D2D1_FIGURE_BEGIN_FILLED
                } else {
                    D2D1_FIGURE_BEGIN_HOLLOW
                },
            );
            let rest: Vec<Vector2> = points[1..].iter().map(|p| pt(*p)).collect();
            sink.AddLines(&rest);
            sink.EndFigure(if closed {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
            sink.Close().ok()?;
            Some(geo)
        }
    }

    /// Renders `list` off screen at `scale` and writes a PNG.
    pub fn snapshot(
        &self,
        list: &DisplayList,
        width: u32,
        height: u32,
        scale: f32,
        clear: Color32,
        path: &std::path::Path,
    ) -> Result<()> {
        unsafe {
            let props = Self::target_props(scale, false);
            let size = D2D_SIZE_U { width, height };
            let target = self.dc.CreateBitmap(size, None, 0, &props)?;
            self.dc.SetTarget(&target);
            self.draw(list, scale, clear);
            self.dc.EndDraw(None, None)?;
            self.dc.SetTarget(None);
            let cpu_props = D2D1_BITMAP_PROPERTIES1 {
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Self::target_props(scale, false)
            };
            let cpu = self.dc.CreateBitmap(size, None, 0, &cpu_props)?;
            cpu.CopyFromBitmap(None, &target, None)?;
            let mapped = cpu.Map(D2D1_MAP_OPTIONS_READ)?;
            let stride = mapped.pitch as usize;
            let data = std::slice::from_raw_parts(mapped.bits, stride * height as usize).to_vec();
            cpu.Unmap()?;
            write_png(&data, width, height, stride, path)
        }
        .map(|_| ())
        .inspect(|_| self.text.end_frame())
    }

    pub fn commit(&self) {
        if let Some(d) = &self.dcomp {
            unsafe {
                let _ = d.Commit();
            }
        }
    }
}

unsafe fn create_device() -> Result<(ID3D11Device, bool)> {
    let levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let force_warp = std::env::var_os("DBM_FORCE_WARP").is_some();
    for (driver, software) in [
        (D3D_DRIVER_TYPE_HARDWARE, false),
        (D3D_DRIVER_TYPE_WARP, true),
    ] {
        if force_warp && !software {
            continue;
        }
        let mut device = None;
        let ok = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        };
        if ok.is_ok()
            && let Some(device) = device
        {
            return Ok((device, software));
        }
    }
    Err(Error::from(E_FAIL))
}

unsafe fn write_png(
    bgra: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    path: &std::path::Path,
) -> Result<()> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = wic.CreateStream()?;
        let p = wide(&path.to_string_lossy());
        let mut z = p.clone();
        z.push(0);
        stream.InitializeFromFilename(PCWSTR(z.as_ptr()), GENERIC_WRITE.0)?;
        let encoder = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or_else(|| Error::from(E_FAIL))?;
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        // Opaque: alpha was ignored while drawing.
        let mut data = bgra.to_vec();
        for row in 0..height as usize {
            for px in 0..width as usize {
                data[row * stride + px * 4 + 3] = 255;
            }
        }
        frame.WritePixels(height, stride as u32, &data)?;
        frame.Commit()?;
        encoder.Commit()?;
        Ok(())
    }
}
