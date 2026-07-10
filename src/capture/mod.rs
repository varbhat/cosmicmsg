use std::io::Write;

use crate::{
    Error,
    state::{AppData, CaptureResult},
};
use cosmic_client_toolkit::screencopy::{
    CaptureOptions, CaptureSource, Formats, Rect, ScreencopyFrameData, ScreencopyFrameDataExt,
    ScreencopySessionData, ScreencopySessionDataExt,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use wayland_client::{EventQueue, protocol::wl_shm};

// ── Protocol user-data newtypes ───────────────────────────────────────────────

#[derive(Default)]
pub struct SessionData(ScreencopySessionData);
impl ScreencopySessionDataExt for SessionData {
    fn screencopy_session_data(&self) -> &ScreencopySessionData {
        &self.0
    }
}

#[derive(Default)]
pub struct FrameData(ScreencopyFrameData);
impl ScreencopyFrameDataExt for FrameData {
    fn screencopy_frame_data(&self) -> &ScreencopyFrameData {
        &self.0
    }
}

// ── Supported shm formats, in preference order ───────────────────────────────

const PREFERRED_FORMATS: &[wl_shm::Format] = &[
    wl_shm::Format::Abgr8888,
    wl_shm::Format::Argb8888,
    wl_shm::Format::Xbgr8888,
    wl_shm::Format::Xrgb8888,
];

fn pick_format(formats: &Formats) -> Result<wl_shm::Format, Error> {
    PREFERRED_FORMATS
        .iter()
        .find(|f| formats.shm_formats.contains(f))
        .copied()
        .ok_or_else(|| {
            Error::Other(format!(
                "no supported shm format; compositor offers: {:?}",
                formats.shm_formats
            ))
        })
}

// ── Format conversion to RGBA8888 ─────────────────────────────────────────────
//
// Wayland/DRM fourcc names encode the 32-bit pixel from MSB to LSB.
// On little-endian x86, byte[0] is the LSB:
//
//   Abgr8888: A[31:24] B[23:16] G[15:8] R[7:0]  → mem: [R, G, B, A]  no swap
//   Argb8888: A[31:24] R[23:16] G[15:8] B[7:0]  → mem: [B, G, R, A]  swap R↔B
//   Xbgr8888: X[31:24] B[23:16] G[15:8] R[7:0]  → mem: [R, G, B, X]  no swap
//   Xrgb8888: X[31:24] R[23:16] G[15:8] B[7:0]  → mem: [B, G, R, X]  swap R↔B
//
// Force A=255: the compositor may send A=0 for composited-transparent areas.

fn to_rgba(mut pixels: Vec<u8>, fmt: wl_shm::Format) -> Vec<u8> {
    let needs_rb_swap = matches!(fmt, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888);
    for chunk in pixels.chunks_exact_mut(4) {
        if needs_rb_swap {
            chunk.swap(0, 2);
        }
        chunk[3] = 255;
    }
    pixels
}

// ── Event-loop helper ─────────────────────────────────────────────────────────

/// Pump the event loop up to `max_rounds` times until `predicate` returns
/// `Ok(true)` (done) or `Err(e)` (hard failure). Returns `Ok(())` in both the
/// success and timeout cases; callers verify the outcome themselves.
fn wait_for(
    app_data: &mut AppData,
    event_queue: &mut EventQueue<AppData>,
    max_rounds: usize,
    mut predicate: impl FnMut(&AppData) -> Result<bool, Error>,
) -> Result<(), Error> {
    for _ in 0..max_rounds {
        event_queue
            .blocking_dispatch(app_data)
            .map_err(|e| Error::Other(e.to_string()))?;
        match predicate(app_data) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ── Raw capture result ────────────────────────────────────────────────────────

/// Raw RGBA8888 pixels from a capture, before any PNG encoding.
///
/// Each pixel is 4 bytes in `[R, G, B, A]` order. Alpha is always 255.
/// Total buffer size is `width * height * 4` bytes.
#[allow(dead_code)] // used via the lib crate's capture_*_raw functions
pub struct RawCapture {
    pub width: u32,
    pub height: u32,
    /// Raw RGBA8888 pixel data, row-major, top-to-bottom.
    pub data: Vec<u8>,
}

#[allow(dead_code)] // methods used via the lib crate
impl RawCapture {
    /// Resize the capture by `scale` and return a new `RawCapture`.
    /// Equivalent to passing `--scale` on the CLI.
    pub fn scale(&self, factor: f64) -> Self {
        let (w, h, data) = maybe_scale(self.data.clone(), self.width, self.height, Some(factor));
        RawCapture {
            width: w,
            height: h,
            data,
        }
    }

    /// Encode the raw pixels as PNG, writing into `out`.
    pub fn encode_png(&self, out: &mut dyn Write) -> Result<(), Error> {
        encode_png(out, self.width, self.height, &self.data)
    }
}

// ── Capture pipeline ──────────────────────────────────────────────────────────

/// Capture a single frame from `source` and return raw RGBA8888 pixels.
///
/// Use this when you want to process the pixel data directly rather than
/// writing a PNG. Call [`RawCapture::encode_png`] or [`RawCapture::scale`]
/// on the result as needed.
#[allow(dead_code)] // used via the lib crate
pub fn capture_raw(
    app_data: &mut AppData,
    event_queue: &mut EventQueue<AppData>,
    source: CaptureSource,
    with_cursor: bool,
) -> Result<RawCapture, Error> {
    let (width, height, data) = capture_rgba(app_data, event_queue, source, with_cursor)?;
    Ok(RawCapture {
        width,
        height,
        data,
    })
}

/// Capture a single frame from `source` and write it as a PNG to `out`.
/// Pass `scale` to resize the image by that factor before encoding.
pub fn capture_to_png(
    app_data: &mut AppData,
    event_queue: &mut EventQueue<AppData>,
    source: CaptureSource,
    out: &mut dyn Write,
    with_cursor: bool,
    scale: Option<f64>,
) -> Result<(), Error> {
    let (width, height, rgba) = capture_rgba(app_data, event_queue, source, with_cursor)?;
    let (out_w, out_h, pixels) = maybe_scale(rgba, width, height, scale);
    encode_png(out, out_w, out_h, &pixels)
}

/// Run the full screencopy pipeline and return raw RGBA8888 pixels plus dimensions.
fn capture_rgba(
    app_data: &mut AppData,
    event_queue: &mut EventQueue<AppData>,
    source: CaptureSource,
    with_cursor: bool,
) -> Result<(u32, u32, Vec<u8>), Error> {
    let opts = if with_cursor {
        CaptureOptions::PaintCursors
    } else {
        CaptureOptions::empty()
    };
    let qh = event_queue.handle();
    let capturer = app_data.screencopy_state.capturer().clone();

    // ── 1. Open session, collect buffer constraints ───────────────────────────
    app_data.capture_formats = None;
    app_data.capture_result = None;

    let session = capturer
        .create_session(&source, opts, &qh, SessionData::default())
        .map_err(|e| Error::ProtocolNotAvailable(format!("capture source not supported: {e}")))?;

    wait_for(app_data, event_queue, 200, |d| {
        if d.capture_formats.is_some() {
            return Ok(true);
        }
        if matches!(d.capture_result, Some(CaptureResult::Stopped)) {
            return Err(Error::Other(
                "capture session stopped before constraints".into(),
            ));
        }
        Ok(false)
    })?;

    let formats = app_data
        .capture_formats
        .take()
        .ok_or_else(|| Error::Other("compositor never advertised buffer constraints".into()))?;

    let (width, height) = formats.buffer_size;
    if width == 0 || height == 0 {
        return Err(Error::Other("compositor reported zero-size buffer".into()));
    }

    // ── 2. Allocate shm buffer ────────────────────────────────────────────────
    let fmt = pick_format(&formats)?;
    let stride = width * 4;
    let buf_size = (stride * height) as usize;

    let mut pool = SlotPool::new(buf_size, &app_data.shm_state)
        .map_err(|e| Error::Other(format!("shm pool allocation failed: {e}")))?;

    let (buffer, canvas) = pool
        .create_buffer(width as i32, height as i32, stride as i32, fmt)
        .map_err(|e| Error::Other(format!("shm buffer creation failed: {e}")))?;

    // ── 3. Request and await the frame ────────────────────────────────────────
    app_data.capture_result = None;
    let _frame = session.capture(
        buffer.wl_buffer(),
        &[Rect {
            x: 0,
            y: 0,
            width: width as i32,
            height: height as i32,
        }],
        &qh,
        FrameData::default(),
    );

    wait_for(app_data, event_queue, 500, |d| match &d.capture_result {
        Some(CaptureResult::Ready) => Ok(true),
        Some(CaptureResult::Failed(msg)) => Err(Error::Other(format!("capture failed: {msg}"))),
        Some(CaptureResult::Stopped) => {
            Err(Error::Other("capture session stopped during frame".into()))
        }
        None => Ok(false),
    })?;

    if !matches!(app_data.capture_result, Some(CaptureResult::Ready)) {
        return Err(Error::Other("capture timed out waiting for frame".into()));
    }

    // ── 4. Read pixels and normalise to RGBA8888 ──────────────────────────────
    // Slice to exact size — SlotPool rounds allocations up to 64-byte boundaries.
    let rgba = to_rgba(canvas[..buf_size].to_vec(), fmt);

    Ok((width, height, rgba))
}

/// Resize RGBA pixels by `scale` if needed; otherwise pass through unchanged.
fn maybe_scale(rgba: Vec<u8>, width: u32, height: u32, scale: Option<f64>) -> (u32, u32, Vec<u8>) {
    match scale {
        Some(factor) if (factor - 1.0).abs() > 1e-9 => {
            let new_w = ((width as f64 * factor).round() as u32).max(1);
            let new_h = ((height as f64 * factor).round() as u32).max(1);
            (
                new_w,
                new_h,
                resize_bilinear(&rgba, width, height, new_w, new_h),
            )
        }
        _ => (width, height, rgba),
    }
}

/// Encode an RGBA8888 buffer as PNG into `out`.
fn encode_png(out: &mut dyn Write, width: u32, height: u32, pixels: &[u8]) -> Result<(), Error> {
    let mut encoder = png::Encoder::new(out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| Error::Other(format!("PNG header: {e}")))?;
    writer
        .write_image_data(pixels)
        .map_err(|e| Error::Other(format!("PNG encode: {e}")))?;
    Ok(())
}

// ── Bilinear resize ───────────────────────────────────────────────────────────

/// Bilinear resize of an RGBA8888 image.
fn resize_bilinear(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];

    let x_ratio = src_w as f64 / dst_w as f64;
    let y_ratio = src_h as f64 / dst_h as f64;

    for dy in 0..dst_h {
        // Pre-compute vertical blend weights — constant across the row.
        let sy = (dy as f64 + 0.5) * y_ratio - 0.5;
        let y0 = sy.floor() as i64;
        let fy = sy - y0 as f64;
        let cy0 = y0.clamp(0, src_h as i64 - 1) as u32;
        let cy1 = (y0 + 1).clamp(0, src_h as i64 - 1) as u32;

        for dx in 0..dst_w {
            let sx = (dx as f64 + 0.5) * x_ratio - 0.5;
            let x0 = sx.floor() as i64;
            let fx = sx - x0 as f64;
            let cx0 = x0.clamp(0, src_w as i64 - 1) as u32;
            let cx1 = (x0 + 1).clamp(0, src_w as i64 - 1) as u32;

            let p00 = pixel(src, src_w, cx0, cy0);
            let p10 = pixel(src, src_w, cx1, cy0);
            let p01 = pixel(src, src_w, cx0, cy1);
            let p11 = pixel(src, src_w, cx1, cy1);

            let w00 = (1.0 - fx) * (1.0 - fy);
            let w10 = fx * (1.0 - fy);
            let w01 = (1.0 - fx) * fy;
            let w11 = fx * fy;

            let out_off = ((dy * dst_w + dx) * 4) as usize;
            for c in 0..4usize {
                dst[out_off + c] = (p00[c] as f64 * w00
                    + p10[c] as f64 * w10
                    + p01[c] as f64 * w01
                    + p11[c] as f64 * w11)
                    .round() as u8;
            }
        }
    }

    dst
}

#[inline]
fn pixel(src: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let off = ((y * w + x) * 4) as usize;
    src[off..off + 4].try_into().unwrap()
}

// ── Named capture helpers ─────────────────────────────────────────────────────

pub fn capture_output(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    output: wayland_client::protocol::wl_output::WlOutput,
    out: &mut dyn std::io::Write,
    with_cursor: bool,
    scale: Option<f64>,
) -> Result<(), crate::Error> {
    capture_to_png(
        state,
        eq,
        cosmic_client_toolkit::screencopy::CaptureSource::Output(output),
        out,
        with_cursor,
        scale,
    )
}

pub fn capture_window(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
    out: &mut dyn std::io::Write,
    with_cursor: bool,
    scale: Option<f64>,
) -> Result<(), crate::Error> {
    let handle = crate::toplevels::resolve_toplevel_pub(state, selector)?
        .foreign_toplevel
        .clone();
    capture_to_png(
        state,
        eq,
        cosmic_client_toolkit::screencopy::CaptureSource::Toplevel(handle),
        out,
        with_cursor,
        scale,
    )
}

pub fn capture_workspace(
    state: &mut AppData,
    eq: &mut wayland_client::EventQueue<AppData>,
    selector: &str,
    out: &mut dyn std::io::Write,
    with_cursor: bool,
    scale: Option<f64>,
) -> Result<(), crate::Error> {
    let handle = crate::workspaces::resolve_workspace_pub(state, selector)?
        .handle
        .clone();
    capture_to_png(
        state,
        eq,
        cosmic_client_toolkit::screencopy::CaptureSource::Workspace(handle),
        out,
        with_cursor,
        scale,
    )
}
