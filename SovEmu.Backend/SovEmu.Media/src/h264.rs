//! H.264 through FFmpeg's libavcodec.
//!
//! One API over several encoders, tried in order until one opens: the GPU's own (VA-API on
//! Intel and AMD, NVENC on NVIDIA), then x264 in software. Which one works is only known by
//! opening it, so [`H264Encoder`] opens lazily on the first frame and falls back on its own;
//! [`H264Encoder::backend`] says which one it got. A hardware encoder that fails mid-stream is
//! dropped for the next one, so the picture never stops, and so is one that holds frames back:
//! at console resolutions x264 takes about a millisecond a frame, so a GPU encoder that
//! returns each frame one late (a whole frame period) would only add latency. (VideoToolbox,
//! on a Mac, does exactly that, so it is not offered; Macs, for development, use x264.)
//!
//! Every encoder is set up the same way for interactive streaming: Constrained Baseline (what
//! every WebRTC client decodes, the `42e01f` profile the worker negotiates), no B-frames and no
//! lookahead (each frame comes out as soon as it goes in), constant bitrate with a small
//! buffer (frame sizes stay even, so none spends long on the wire), the parameter sets
//! repeated before every keyframe, and keyframes forced on request.
//!
//! Frames are encoded at the console's native resolution, rounded up to even sizes (4:2:0
//! needs them) by repeating the last column or row.

use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char, c_int};
use std::ptr;

use crate::ffi;
use crate::vp8::{DecodedFrame, pack_plane};
use crate::yuv::{I420Planes, PixelLayout, Source, chroma_len};
use crate::{EncodedFrame, Error};

/// Threads x264 encodes a frame on.
pub const X264_THREADS: u32 = 4;

/// An H.264 encoder libavcodec can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum H264Backend {
    /// VA-API: Intel (iHD) and AMD (Mesa) GPUs, on a render node.
    Vaapi,
    /// NVIDIA's NVENC, through the driver the container toolkit injects.
    Nvenc,
    /// x264 on the CPU.
    Software,
}

impl H264Backend {
    /// The name used in settings and logs.
    pub fn name(self) -> &'static str {
        match self {
            Self::Vaapi => "vaapi",
            Self::Nvenc => "nvenc",
            Self::Software => "x264",
        }
    }

    /// Whether it encodes on a GPU.
    pub fn is_hardware(self) -> bool {
        self != Self::Software
    }

    fn codec_name(self) -> &'static CStr {
        match self {
            Self::Vaapi => c"h264_vaapi",
            Self::Nvenc => c"h264_nvenc",
            Self::Software => c"libx264",
        }
    }
}

/// Which encoder to use (`SovEmu__Streaming__Video__Encoder`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum H264Preference {
    /// The GPU's encoder if one opens, else x264.
    #[default]
    Auto,
    /// This one, falling back to x264 if it does not open.
    Only(H264Backend),
}

impl std::str::FromStr for H264Preference {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "" | "auto" => Self::Auto,
            "vaapi" => Self::Only(H264Backend::Vaapi),
            "nvenc" => Self::Only(H264Backend::Nvenc),
            "software" | "x264" => Self::Only(H264Backend::Software),
            other => {
                return Err(format!(
                    "unknown video encoder {other:?} (auto, vaapi, nvenc or software)"
                ));
            }
        })
    }
}

impl H264Preference {
    /// The encoders to try, in order.
    fn candidates(self) -> Vec<H264Backend> {
        match self {
            Self::Auto if cfg!(target_os = "linux") => {
                vec![H264Backend::Vaapi, H264Backend::Nvenc, H264Backend::Software]
            }
            Self::Auto => vec![H264Backend::Software],
            Self::Only(H264Backend::Software) => vec![H264Backend::Software],
            Self::Only(backend) => vec![backend, H264Backend::Software],
        }
    }
}

/// Encoder settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264Config {
    /// Target bitrate in kbit/s.
    pub bitrate_kbps: u32,
    /// Frames per second of the source; used for timestamps and rate control.
    pub fps: u32,
    /// Longest gap between keyframes, in frames, when no client asks for one.
    pub keyframe_interval: u32,
    pub preference: H264Preference,
    /// The render node VA-API encodes on (`/dev/dri/renderD128`); none for libva's default.
    pub device: Option<String>,
}

impl Default for H264Config {
    fn default() -> Self {
        Self {
            bitrate_kbps: 2_500,
            fps: 60,
            keyframe_interval: 600,
            preference: H264Preference::Auto,
            device: None,
        }
    }
}

/// `AVERROR(EAGAIN)`: the encoder wants more input before it has output.
const AVERROR_EAGAIN: c_int = -(ffi::EAGAIN as c_int);
/// `AVERROR_EOF`: `-MKTAG('E', 'O', 'F', ' ')`.
const AVERROR_EOF: c_int = -0x2046_4F45;

fn av_error(op: &'static str, err: c_int) -> Error {
    let mut buf = [0 as c_char; 128];
    // SAFETY: `buf` is writable for its length; av_strerror always NUL-terminates it.
    let msg = unsafe {
        ffi::av_strerror(err, buf.as_mut_ptr(), buf.len());
        CStr::from_ptr(buf.as_ptr())
    };
    Error::Av(op, msg.to_string_lossy().into_owned())
}

fn check(op: &'static str, err: c_int) -> Result<c_int, Error> {
    if err < 0 { Err(av_error(op, err)) } else { Ok(err) }
}

/// Quietens FFmpeg's own logging (x264 prints its settings on every open) to errors, once.
fn quiet_ffmpeg() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: sets a process-wide integer; no other state is touched.
    ONCE.call_once(|| unsafe { ffi::av_log_set_level(ffi::AV_LOG_ERROR as c_int) });
}

/// Sets a codec option (the encoder's private ones included) before it is opened.
///
/// # Safety
///
/// `ctx` must be an allocated, not yet opened codec context.
unsafe fn set_option(ctx: *mut ffi::AVCodecContext, name: &CStr, value: &str) -> Result<(), Error> {
    let value = CString::new(value).map_err(|_| Error::Av("option", "NUL in value".into()))?;
    // SAFETY: the caller guarantees `ctx`; both strings are NUL-terminated.
    let err = unsafe {
        ffi::av_opt_set(
            ctx.cast(),
            name.as_ptr(),
            value.as_ptr(),
            ffi::AV_OPT_SEARCH_CHILDREN as c_int,
        )
    };
    check("option", err).map(drop).map_err(|e| match e {
        Error::Av(_, msg) => Error::Av("option", format!("{}: {msg}", name.to_string_lossy())),
        other => other,
    })
}

/// An owned `AVBufferRef` (a hardware device or frame pool).
struct BufferRef(*mut ffi::AVBufferRef);

impl BufferRef {
    fn new_ref(&self) -> *mut ffi::AVBufferRef {
        // SAFETY: `self.0` is a valid reference; av_buffer_ref returns a new one (or null).
        unsafe { ffi::av_buffer_ref(self.0) }
    }
}

impl Drop for BufferRef {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a reference this value owns; av_buffer_unref nulls it.
        unsafe { ffi::av_buffer_unref(&mut self.0) };
    }
}

/// An open codec context and what it encodes from.
struct Context {
    backend: H264Backend,
    ctx: *mut ffi::AVCodecContext,
    /// The frame `rgba` is converted into: I420, or NV12 to upload for VA-API.
    frame: *mut ffi::AVFrame,
    packet: *mut ffi::AVPacket,
    /// VA-API's surface pool; frames are uploaded into it.
    hw_frames: Option<BufferRef>,
    /// The encoded size: the picture's, rounded up to even.
    width: u32,
    height: u32,
    /// The picture's size, before rounding.
    source: (u32, u32),
    bitrate_kbps: u32,
    /// Frames sent since it opened.
    frames_sent: u32,
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: each was allocated in `open` and is freed exactly once (the free functions
        // take a pointer to the pointer and null it).
        unsafe {
            ffi::av_packet_free(&mut self.packet);
            ffi::av_frame_free(&mut self.frame);
            ffi::avcodec_free_context(&mut self.ctx);
        }
        // `hw_frames` is dropped after the codec context that referenced it.
    }
}

/// A real-time H.264 encoder over whichever backend opens. It reopens itself if the frame size
/// changes, and, on a hardware encoder, when the bitrate changes (x264 changes it in place).
pub struct H264Encoder {
    config: H264Config,
    context: Option<Context>,
    /// The VA-API device, opened once and kept across reopens.
    vaapi_device: Option<BufferRef>,
    /// Backends that failed to open or to encode, in order, with why; not tried again.
    failed: Vec<(H264Backend, String)>,
    frames: i64,
    force_keyframe: bool,
    /// Packets received but not yet returned: one per frame, oldest first.
    pending: VecDeque<EncodedFrame>,
    /// Frames sent minus packets received: an encoder that holds frames back adds latency.
    lag: u32,
}

impl std::fmt::Debug for H264Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H264Encoder")
            .field("config", &self.config)
            .field("backend", &self.backend())
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

// SAFETY: the libavcodec state is only ever used through `&mut self`, from one thread at a
// time; none of it is tied to the thread that made it.
unsafe impl Send for H264Encoder {}

impl H264Encoder {
    pub fn new(config: H264Config) -> Self {
        quiet_ffmpeg();
        Self {
            config,
            context: None,
            vaapi_device: None,
            failed: Vec::new(),
            frames: 0,
            force_keyframe: true,
            pending: VecDeque::new(),
            lag: 0,
        }
    }

    pub fn config(&self) -> &H264Config {
        &self.config
    }

    /// The encoders given up so far, in order, and why: one that did not open, failed, or held
    /// frames back.
    pub fn fallbacks(&self) -> impl Iterator<Item = (H264Backend, &str)> {
        self.failed.iter().map(|(b, why)| (*b, why.as_str()))
    }

    /// The encoder in use, once the first frame has opened one.
    pub fn backend(&self) -> Option<H264Backend> {
        self.context.as_ref().map(|c| c.backend)
    }

    /// Frames the encoder holds back before returning them (0 for every backend kept; more
    /// means it adds a frame of latency per frame held).
    pub fn lag(&self) -> u32 {
        self.lag
    }

    /// Makes the next frame a keyframe (a client joined, or lost packets and asked for one).
    pub fn request_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    /// Changes the target bitrate from the next frame on.
    pub fn set_bitrate(&mut self, kbps: u32) -> Result<(), Error> {
        self.config.bitrate_kbps = kbps;
        match &mut self.context {
            Some(c) if c.backend == H264Backend::Software => {
                // libx264 reconfigures itself when these change between frames.
                // SAFETY: the context is open and only touched from this thread.
                unsafe { set_rate(c.ctx, kbps) };
                c.bitrate_kbps = kbps;
            }
            // The hardware encoders take their rate control at open: reopen (with a keyframe).
            Some(_) => self.context = None,
            None => {}
        }
        Ok(())
    }

    /// Opens the first backend that works for this size.
    fn open(&mut self, width: u32, height: u32) -> Result<Context, Error> {
        let mut tried = Vec::new();
        for backend in self.config.preference.candidates() {
            if self.failed.iter().any(|(b, _)| *b == backend) {
                continue;
            }
            match self.open_backend(backend, width, height) {
                Ok(context) => {
                    if !tried.is_empty() {
                        tracing::info!(
                            encoder = backend.name(),
                            tried = %tried.join("; "),
                            "H.264 encoder fell back"
                        );
                    }
                    return Ok(context);
                }
                Err(e) => {
                    tracing::debug!(encoder = backend.name(), error = %e, "H.264 encoder did not open");
                    tried.push(format!("{}: {e}", backend.name()));
                    self.failed.push((backend, e.to_string()));
                }
            }
        }
        Err(Error::NoEncoder(if tried.is_empty() {
            "none left to try".into()
        } else {
            tried.join("; ")
        }))
    }

    fn open_backend(
        &mut self,
        backend: H264Backend,
        width: u32,
        height: u32,
    ) -> Result<Context, Error> {
        if backend == H264Backend::Vaapi {
            if self.vaapi_device.is_none() {
                self.vaapi_device = Some(open_vaapi_device(self.config.device.as_deref())?);
            }
            // Low power (the fixed-function VDEnc) is all Intel's free driver offers, and the
            // faster path where both exist; Mesa (AMD) has only the other, so try both.
            let device = self.vaapi_device.as_ref().expect("opened above");
            return open_context(&self.config, backend, width, height, Some((device, true)))
                .or_else(|_| {
                    open_context(&self.config, backend, width, height, Some((device, false)))
                });
        }
        open_context(&self.config, backend, width, height, None)
    }

    /// Encodes one RGBA frame (`width * height * 4` bytes, tightly packed). Returns `None` if
    /// the encoder produced nothing for it.
    pub fn encode(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, Error> {
        self.encode_pixels(rgba, PixelLayout::Rgba8888, width, height)
    }

    /// Encodes one frame of packed pixels in `layout` (`width * height` pixels, no row padding),
    /// converted straight from it. Returns `None` if the encoder produced nothing for it.
    pub fn encode_pixels(
        &mut self,
        pixels: &[u8],
        layout: PixelLayout,
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, Error> {
        self.encode_source(Source::Packed(pixels, layout), width, height)
    }

    /// Encodes one frame already in I420 (packed planes, [`crate::yuv::i420_len`] bytes), as a
    /// GPU converts it: copied (or, for VA-API, interleaved into NV12), not converted.
    pub fn encode_i420(
        &mut self,
        i420: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, Error> {
        self.encode_source(Source::I420(i420), width, height)
    }

    fn encode_source(
        &mut self,
        source: Source<'_>,
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, Error> {
        let (expected, actual) = source.sizes(width as usize, height as usize);
        if actual != expected {
            return Err(Error::BadFrame { expected, actual });
        }
        match self.encode_once(source, width, height) {
            Err(e) if self.backend().is_some_and(H264Backend::is_hardware) => {
                // A GPU encoder that fails mid-stream (a reset, a driver bug) or holds frames
                // back is dropped for the next one rather than stopping or delaying the picture.
                let backend = self.backend().expect("checked above");
                tracing::warn!(encoder = backend.name(), error = %e, "H.264 encoder failed; switching");
                self.failed.push((backend, e.to_string()));
                self.context = None;
                self.encode_once(source, width, height)
            }
            other => other,
        }
    }

    fn encode_once(
        &mut self,
        source: Source<'_>,
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, Error> {
        let (even_w, even_h) = (width.next_multiple_of(2), height.next_multiple_of(2));
        let reopen = self.context.as_ref().is_none_or(|c| {
            (c.width, c.height) != (even_w, even_h) || c.bitrate_kbps != self.config.bitrate_kbps
        });
        if reopen {
            self.context = None;
            self.pending.clear();
            self.lag = 0;
            self.context = Some(self.open(even_w, even_h)?);
            self.force_keyframe = true;
        }
        let c = self.context.as_mut().expect("opened above");
        c.source = (width, height);

        // SAFETY: the context and frame were opened for `even_w`x`even_h`; `fill` checks the
        // planes' sizes against the frame's strides.
        unsafe { fill(c, source)? };

        let keyframe = std::mem::take(&mut self.force_keyframe);
        let pts = self.frames;
        self.frames += 1;
        // SAFETY: the context is open, its frame is filled and owned by it.
        unsafe { send(c, pts, keyframe)? };
        self.lag += 1;

        loop {
            // SAFETY: the context is open; the packet is owned by it and unreferenced below.
            let err = unsafe { ffi::avcodec_receive_packet(c.ctx, c.packet) };
            if err == AVERROR_EAGAIN || err == AVERROR_EOF {
                break;
            }
            check("receive packet", err)?;
            // SAFETY: a received packet holds `size` bytes at `data` until it is unreferenced.
            let frame = unsafe {
                let p = &*c.packet;
                let data = std::slice::from_raw_parts(p.data, p.size as usize).to_vec();
                let keyframe = p.flags & ffi::AV_PKT_FLAG_KEY as c_int != 0;
                ffi::av_packet_unref(c.packet);
                EncodedFrame { data, keyframe }
            };
            self.lag = self.lag.saturating_sub(1);
            self.pending.push_back(frame);
        }
        if self.lag > 0 && c.backend.is_hardware() && c.frames_sent >= LAG_GRACE_FRAMES {
            return Err(Error::Av("encode", format!("holds {} frame(s) back", self.lag)));
        }
        c.frames_sent += 1;
        Ok(self.pending.pop_front())
    }
}

/// Frames a hardware encoder may take to settle (its first output may come late) before one
/// that still holds frames back is dropped for x264.
const LAG_GRACE_FRAMES: u32 = 3;

/// Sets the target, the cap and the buffer for a bitrate: constant bitrate with half a second
/// of buffer, so a keyframe may be a few frames' worth but no frame is a burst.
///
/// # Safety
///
/// `ctx` must be an allocated codec context only touched from this thread.
unsafe fn set_rate(ctx: *mut ffi::AVCodecContext, kbps: u32) {
    let bps = i64::from(kbps) * 1000;
    // SAFETY: the caller guarantees `ctx`.
    unsafe {
        (*ctx).bit_rate = bps;
        (*ctx).rc_max_rate = bps;
        (*ctx).rc_buffer_size = (bps / 2).min(i64::from(c_int::MAX)) as c_int;
    }
}

fn open_vaapi_device(node: Option<&str>) -> Result<BufferRef, Error> {
    let node = node.map(CString::new).transpose().map_err(|_| Error::Av("device", "NUL".into()))?;
    let mut device = ptr::null_mut();
    // SAFETY: `device` is an out-parameter; the node is NUL-terminated or null (libva's
    // default device).
    let err = unsafe {
        ffi::av_hwdevice_ctx_create(
            &mut device,
            ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
            node.as_ref().map_or(ptr::null(), |n| n.as_ptr()),
            ptr::null_mut(),
            0,
        )
    };
    check("VA-API device", err)?;
    Ok(BufferRef(device))
}

/// Allocates and opens one backend's codec context, with its frame and packet.
fn open_context(
    config: &H264Config,
    backend: H264Backend,
    width: u32,
    height: u32,
    vaapi: Option<(&BufferRef, bool)>,
) -> Result<Context, Error> {
    // SAFETY: the name is a NUL-terminated literal; a missing encoder is a null result.
    let codec = unsafe { ffi::avcodec_find_encoder_by_name(backend.codec_name().as_ptr()) };
    if codec.is_null() {
        return Err(Error::Av("find encoder", "not in this libavcodec".into()));
    }
    // SAFETY: `codec` is a valid encoder; a null result is checked.
    let ctx = unsafe { ffi::avcodec_alloc_context3(codec) };
    if ctx.is_null() {
        return Err(Error::Av("alloc context", "out of memory".into()));
    }
    // From here the context (and the frame, packet and pool as they are made) are owned by
    // `context`, whose Drop frees them on any early return.
    let mut context = Context {
        backend,
        ctx,
        frame: ptr::null_mut(),
        packet: ptr::null_mut(),
        hw_frames: None,
        width,
        height,
        source: (width, height),
        bitrate_kbps: config.bitrate_kbps,
        frames_sent: 0,
    };
    let fps = config.fps.max(1) as c_int;
    let sw_format = if vaapi.is_some() {
        ffi::AVPixelFormat::AV_PIX_FMT_NV12
    } else {
        ffi::AVPixelFormat::AV_PIX_FMT_YUV420P
    };

    // SAFETY: `ctx` is allocated and not yet opened; every field set here is plain data.
    unsafe {
        let c = &mut *ctx;
        c.width = width as c_int;
        c.height = height as c_int;
        c.time_base = ffi::AVRational { num: 1, den: fps };
        c.framerate = ffi::AVRational { num: fps, den: 1 };
        c.gop_size = config.keyframe_interval.max(1).min(c_int::MAX as u32) as c_int;
        c.max_b_frames = 0;
        c.pix_fmt = sw_format;
        // BT.601, limited range: what `yuv` produces, and what decoders assume anyway.
        c.colorspace = ffi::AVColorSpace::AVCOL_SPC_SMPTE170M;
        c.color_primaries = ffi::AVColorPrimaries::AVCOL_PRI_SMPTE170M;
        c.color_trc = ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE170M;
        c.color_range = ffi::AVColorRange::AVCOL_RANGE_MPEG;
        set_rate(ctx, config.bitrate_kbps);
    }

    // SAFETY: `ctx` is allocated and not yet opened.
    unsafe {
        match backend {
            H264Backend::Software => {
                set_option(ctx, c"preset", "superfast")?;
                set_option(ctx, c"tune", "zerolatency")?;
                set_option(ctx, c"profile", "baseline")?;
                set_option(ctx, c"forced-idr", "1")?;
                // zerolatency slices each frame across threads; a few are plenty at console
                // resolutions and leave cores for the emulator.
                (*ctx).thread_count = X264_THREADS as c_int;
            }
            H264Backend::Vaapi => {
                let (device, low_power) = vaapi.expect("VA-API needs its device");
                set_option(ctx, c"profile", "constrained_baseline")?;
                set_option(ctx, c"rc_mode", "CBR")?;
                set_option(ctx, c"low_power", if low_power { "1" } else { "0" })?;
                // Hand each frame back before taking the next: no frames in flight.
                set_option(ctx, c"async_depth", "1")?;
                set_option(ctx, c"sei", "0")?;
                context.hw_frames = Some(vaapi_frames(device, width, height)?);
                (*ctx).pix_fmt = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
                (*ctx).hw_frames_ctx = context.hw_frames.as_ref().expect("made above").new_ref();
            }
            H264Backend::Nvenc => {
                set_option(ctx, c"preset", "p1")?;
                set_option(ctx, c"tune", "ull")?;
                set_option(ctx, c"zerolatency", "1")?;
                set_option(ctx, c"delay", "0")?;
                set_option(ctx, c"rc", "cbr")?;
                set_option(ctx, c"profile", "baseline")?;
                set_option(ctx, c"forced-idr", "1")?;
            }
        }
        check("open", ffi::avcodec_open2(ctx, codec, ptr::null_mut()))?;

        context.packet = ffi::av_packet_alloc();
        context.frame = ffi::av_frame_alloc();
        if context.packet.is_null() || context.frame.is_null() {
            return Err(Error::Av("alloc frame", "out of memory".into()));
        }
        let f = &mut *context.frame;
        f.format = sw_format;
        f.width = width as c_int;
        f.height = height as c_int;
        check("frame buffer", ffi::av_frame_get_buffer(context.frame, 0))?;
    }
    Ok(context)
}

/// A pool of VA-API NV12 surfaces for this size.
fn vaapi_frames(device: &BufferRef, width: u32, height: u32) -> Result<BufferRef, Error> {
    // SAFETY: `device` is a valid VA-API device reference; a null result is checked.
    let frames = unsafe { ffi::av_hwframe_ctx_alloc(device.0) };
    if frames.is_null() {
        return Err(Error::Av("VA-API frames", "out of memory".into()));
    }
    let frames = BufferRef(frames);
    // SAFETY: the buffer's data is the AVHWFramesContext it was allocated as, not yet
    // initialised, so its fields may be set.
    unsafe {
        let fc = &mut *(*frames.0).data.cast::<ffi::AVHWFramesContext>();
        fc.format = ffi::AVPixelFormat::AV_PIX_FMT_VAAPI;
        fc.sw_format = ffi::AVPixelFormat::AV_PIX_FMT_NV12;
        fc.width = width as c_int;
        fc.height = height as c_int;
        fc.initial_pool_size = 4;
        check("VA-API frames", ffi::av_hwframe_ctx_init(frames.0))?;
    }
    Ok(frames)
}

/// Writes `source` (the context's source size) into the context's frame, repeating the last
/// column and row if the encoded size was rounded up.
///
/// # Safety
///
/// `c` must be an open context whose frame has buffers for its size and format.
unsafe fn fill(c: &mut Context, source: Source<'_>) -> Result<(), Error> {
    // SAFETY: the frame is allocated; make_writable copies it if the encoder still holds it.
    check("frame writable", unsafe { ffi::av_frame_make_writable(c.frame) })?;
    // SAFETY: the frame's buffers were allocated for `c.width`x`c.height` of its format, so
    // each plane holds `linesize * rows` bytes (chroma rows halved).
    let f = unsafe { &mut *c.frame };
    let (w, h) = (c.source.0 as usize, c.source.1 as usize);
    let (ew, eh) = (c.width as usize, c.height as usize);
    let plane = |i: usize, rows: usize| {
        let stride = f.linesize[i] as usize;
        // SAFETY: see above: plane `i` holds `rows` rows of `stride` bytes.
        (unsafe { std::slice::from_raw_parts_mut(f.data[i], stride * rows) }, stride)
    };
    // Each plane's slice lives only inside its own block, so no two ever alias.
    if f.format == ffi::AVPixelFormat::AV_PIX_FMT_NV12 {
        let (y, ys) = plane(0, eh);
        let (uv, uvs) = plane(1, eh / 2);
        source.write_nv12(w, h, y, ys, uv, uvs);
    } else {
        let (y, ys) = plane(0, eh);
        let (u, us) = plane(1, eh / 2);
        let (v, vs) = plane(2, eh / 2);
        source.write_i420(w, h, I420Planes { y, y_stride: ys, u, u_stride: us, v, v_stride: vs });
    }
    // Odd sizes: chroma already covers the extra column and row (it rounds up); repeat luma's.
    let (y, ys) = plane(0, eh);
    if ew > w {
        for row in 0..h {
            y[row * ys + w] = y[row * ys + w - 1];
        }
    }
    if eh > h {
        y.copy_within((h - 1) * ys..(h - 1) * ys + ew, h * ys);
    }
    Ok(())
}

/// Sends the context's frame (uploaded to a surface first on VA-API).
///
/// # Safety
///
/// `c` must be an open context with its frame filled.
unsafe fn send(c: &mut Context, pts: i64, keyframe: bool) -> Result<(), Error> {
    let picture = if keyframe {
        ffi::AVPictureType::AV_PICTURE_TYPE_I
    } else {
        ffi::AVPictureType::AV_PICTURE_TYPE_NONE
    };
    // SAFETY: the caller guarantees an open context and a filled frame; the uploaded frame is
    // freed after sending (the encoder keeps its own reference).
    unsafe {
        (*c.frame).pts = pts;
        (*c.frame).pict_type = picture;
        let Some(pool) = &c.hw_frames else {
            return check("send frame", ffi::avcodec_send_frame(c.ctx, c.frame)).map(drop);
        };
        let mut surface = ffi::av_frame_alloc();
        if surface.is_null() {
            return Err(Error::Av("alloc frame", "out of memory".into()));
        }
        let result = check("surface", ffi::av_hwframe_get_buffer(pool.0, surface, 0))
            .and_then(|_| check("upload", ffi::av_hwframe_transfer_data(surface, c.frame, 0)))
            .and_then(|_| {
                (*surface).pts = pts;
                (*surface).pict_type = picture;
                check("send frame", ffi::avcodec_send_frame(c.ctx, surface))
            });
        ffi::av_frame_free(&mut surface);
        result.map(drop)
    }
}

/// libavcodec's H.264 decoder, used by tests (as [`crate::Vp8Decoder`] is for VP8).
pub struct H264Decoder {
    ctx: *mut ffi::AVCodecContext,
    packet: *mut ffi::AVPacket,
    frame: *mut ffi::AVFrame,
}

impl std::fmt::Debug for H264Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H264Decoder").finish_non_exhaustive()
    }
}

// SAFETY: the libavcodec state is only used through `&mut self`.
unsafe impl Send for H264Decoder {}

impl H264Decoder {
    pub fn new() -> Result<Self, Error> {
        quiet_ffmpeg();
        // SAFETY: the name is a NUL-terminated literal; null results are checked, and what was
        // allocated is owned by `decoder`, whose Drop frees it on an early return.
        unsafe {
            let codec = ffi::avcodec_find_decoder_by_name(c"h264".as_ptr());
            if codec.is_null() {
                return Err(Error::Av("find decoder", "not in this libavcodec".into()));
            }
            let decoder = Self {
                ctx: ffi::avcodec_alloc_context3(codec),
                packet: ffi::av_packet_alloc(),
                frame: ffi::av_frame_alloc(),
            };
            if decoder.ctx.is_null() || decoder.packet.is_null() || decoder.frame.is_null() {
                return Err(Error::Av("alloc decoder", "out of memory".into()));
            }
            // One thread: frame threading would hold pictures back.
            (*decoder.ctx).thread_count = 1;
            check("open decoder", ffi::avcodec_open2(decoder.ctx, codec, ptr::null_mut()))?;
            Ok(decoder)
        }
    }

    /// Decodes one access unit (Annex B), returning the picture if one was produced.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedFrame>, Error> {
        // SAFETY: the packet borrows `data` only for the send call and is cleared after it; a
        // received frame is 4:2:0 (all a Constrained Baseline stream can be): its luma plane holds
        // `height` rows of `linesize[0]` bytes, each chroma plane half as many rows, until unref.
        unsafe {
            (*self.packet).data = data.as_ptr().cast_mut();
            (*self.packet).size = data.len() as c_int;
            let sent = ffi::avcodec_send_packet(self.ctx, self.packet);
            (*self.packet).data = ptr::null_mut();
            (*self.packet).size = 0;
            check("send packet", sent)?;
            let err = ffi::avcodec_receive_frame(self.ctx, self.frame);
            if err == AVERROR_EAGAIN {
                return Ok(None);
            }
            check("receive frame", err)?;
            let f = &*self.frame;
            let (w, h) = (f.width as usize, f.height as usize);
            let (cw, ch) = (chroma_len(w), chroma_len(h));
            let plane =
                |i: usize, width, rows| pack_plane(f.data[i], f.linesize[i] as usize, width, rows);
            let picture = DecodedFrame {
                width: w as u32,
                height: h as u32,
                luma: plane(0, w, h),
                u: plane(1, cw, ch),
                v: plane(2, cw, ch),
            };
            ffi::av_frame_unref(self.frame);
            Ok(Some(picture))
        }
    }
}

impl Drop for H264Decoder {
    fn drop(&mut self) {
        // SAFETY: each is null or was allocated in `new`; the free functions accept null and
        // null the pointer.
        unsafe {
            ffi::av_frame_free(&mut self.frame);
            ffi::av_packet_free(&mut self.packet);
            ffi::avcodec_free_context(&mut self.ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, shift: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&[(x + shift) as u8, (y + shift) as u8, 128, 255]);
            }
        }
        out
    }

    fn size(picture: Result<Option<DecodedFrame>, Error>) -> Option<(u32, u32)> {
        picture.unwrap().map(|p| (p.width, p.height))
    }

    fn software() -> H264Encoder {
        H264Encoder::new(H264Config {
            preference: H264Preference::Only(H264Backend::Software),
            ..H264Config::default()
        })
    }

    /// NAL unit types in an Annex B stream.
    fn nal_types(data: &[u8]) -> Vec<u8> {
        let mut types = Vec::new();
        let mut i = 0;
        while i + 3 < data.len() {
            if data[i..i + 3] == [0, 0, 1] {
                types.push(data[i + 3] & 0x1f);
                i += 3;
            } else {
                i += 1;
            }
        }
        types
    }

    #[test]
    fn every_frame_comes_out_at_once_and_decodes() {
        let mut enc = software();
        let mut dec = H264Decoder::new().unwrap();
        for n in 0..30 {
            let frame = enc.encode(&gradient(256, 224, n), 256, 224).unwrap().expect("a frame");
            assert_eq!(frame.keyframe, n == 0, "only the first frame is a keyframe");
            assert_eq!(
                size(dec.decode(&frame.data)),
                Some((256, 224)),
                "frame {n} decodes at once"
            );
        }
        assert_eq!(enc.backend(), Some(H264Backend::Software));
        assert_eq!(enc.lag(), 0);
    }

    #[test]
    fn an_i420_frame_encodes_as_its_rgba_does() {
        // Odd: the encoded size is rounded up and the last column and row repeated, either way.
        let (mut from_rgba, mut from_i420) = (software(), software());
        for n in 0..3 {
            let rgba = gradient(255, 223, n);
            let i420 = crate::yuv::packed_i420(&rgba, 255, 223);
            let a = from_rgba.encode(&rgba, 255, 223).unwrap();
            let b = from_i420.encode_i420(&i420, 255, 223).unwrap();
            assert_eq!(a, b, "frame {n}");
        }
    }

    #[test]
    fn keyframes_carry_their_parameter_sets_and_come_on_request_and_on_resize() {
        let mut enc = software();
        let first = enc.encode(&gradient(256, 224, 0), 256, 224).unwrap().unwrap();
        let types = nal_types(&first.data);
        assert!(types.contains(&7) && types.contains(&8) && types.contains(&5), "{types:?}");
        assert!(!enc.encode(&gradient(256, 224, 1), 256, 224).unwrap().unwrap().keyframe);
        enc.request_keyframe();
        let forced = enc.encode(&gradient(256, 224, 2), 256, 224).unwrap().unwrap();
        assert!(forced.keyframe);
        assert!(nal_types(&forced.data).contains(&5), "a forced keyframe is an IDR");
        // A size change (e.g. SNES 256x224 -> 512x448 hi-res mode) reopens the encoder.
        let frame = enc.encode(&gradient(160, 144, 0), 160, 144).unwrap().unwrap();
        assert!(frame.keyframe);
        assert_eq!(size(H264Decoder::new().unwrap().decode(&frame.data)), Some((160, 144)));
    }

    #[test]
    fn odd_sizes_round_up_and_bitrate_changes_keep_going() {
        let mut enc = software();
        let frame = enc.encode(&gradient(255, 223, 0), 255, 223).unwrap().unwrap();
        assert_eq!(size(H264Decoder::new().unwrap().decode(&frame.data)), Some((256, 224)));
        enc.set_bitrate(800).unwrap();
        let next = enc.encode(&gradient(255, 223, 1), 255, 223).unwrap().unwrap();
        assert!(!next.keyframe, "x264 changes its bitrate without a keyframe");
    }

    #[test]
    fn decoded_frames_carry_both_chroma_planes() {
        // A solid colour comes back with its own chroma in every sample of each plane; the
        // stream rounds 255x223 up to whole macroblock pairs, and so do the planes.
        let rgba = [0u8, 0, 255, 255].repeat(255 * 223);
        let frame = software().encode(&rgba, 255, 223).unwrap().unwrap();
        let pic = H264Decoder::new().unwrap().decode(&frame.data).unwrap().unwrap();
        assert_eq!((pic.width, pic.height), (256, 224));
        assert_eq!((pic.luma.len(), pic.u.len(), pic.v.len()), (256 * 224, 128 * 112, 128 * 112));
        // BT.601 limited blue is (41, 240, 110); allow the codec a little rounding.
        assert!(pic.u.iter().all(|&u| u.abs_diff(240) <= 3), "u {:?}", &pic.u[..4]);
        assert!(pic.v.iter().all(|&v| v.abs_diff(110) <= 3), "v {:?}", &pic.v[..4]);
    }

    #[test]
    fn the_default_picks_whatever_opens() {
        let mut enc = H264Encoder::new(H264Config::default());
        let frame = enc.encode(&gradient(256, 224, 0), 256, 224).unwrap().expect("a frame");
        assert!(frame.keyframe);
        let backend = enc.backend().expect("some encoder opened");
        let mut dec = H264Decoder::new().unwrap();
        // A hardware encoder may hand the first frame back a frame late; it must still decode.
        let mut decoded = size(dec.decode(&frame.data)).is_some();
        for n in 1..5 {
            if let Some(f) = enc.encode(&gradient(256, 224, n), 256, 224).unwrap() {
                decoded |= size(dec.decode(&f.data)).is_some();
            }
        }
        assert!(decoded, "{} produced a stream that decodes", backend.name());
    }

    #[test]
    fn preferences_parse_and_always_end_in_software() {
        assert_eq!("".parse(), Ok(H264Preference::Auto));
        assert_eq!("vaapi".parse(), Ok(H264Preference::Only(H264Backend::Vaapi)));
        assert_eq!("software".parse(), Ok(H264Preference::Only(H264Backend::Software)));
        assert!("quicksync".parse::<H264Preference>().is_err());
        for p in [H264Preference::Auto, H264Preference::Only(H264Backend::Nvenc)] {
            assert_eq!(p.candidates().last(), Some(&H264Backend::Software));
        }
    }

    #[test]
    fn a_backend_that_cannot_open_falls_back_to_software() {
        // NVENC never opens without NVIDIA's driver (and not at all on a Mac).
        let mut enc = H264Encoder::new(H264Config {
            preference: H264Preference::Only(H264Backend::Nvenc),
            ..H264Config::default()
        });
        if enc.encode(&gradient(64, 64, 0), 64, 64).unwrap().is_some()
            && enc.backend() == Some(H264Backend::Nvenc)
        {
            return; // a machine with an NVIDIA GPU: nothing to fall back from
        }
        assert_eq!(enc.backend(), Some(H264Backend::Software));
        let fallbacks: Vec<_> = enc.fallbacks().collect();
        assert_eq!(fallbacks.len(), 1);
        assert_eq!(fallbacks[0].0, H264Backend::Nvenc);
        assert!(!fallbacks[0].1.is_empty(), "with why");
    }

    #[test]
    fn rejects_a_wrong_sized_buffer() {
        let mut enc = software();
        assert!(matches!(enc.encode(&[0; 10], 4, 4), Err(Error::BadFrame { expected: 64, .. })));
    }
}
