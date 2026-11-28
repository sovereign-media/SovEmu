//! VP8 through the system libvpx.
//!
//! Tuned for interactive streaming rather than quality per bit: one pass, constant bitrate,
//! no frame lag (every input frame produces its output immediately), error-resilient mode so a
//! lost packet damages one frame rather than everything until the next keyframe, small rate
//! control buffers, and keyframes forced on request (a client asks after loss or on join).
//! A frame is encoded on a few threads, and cut into as many parts for a client's decoder to
//! do the same: at 640×448 four threads take about 3 ms a frame where one takes 5.

use std::ffi::{CStr, c_int};
use std::ptr::NonNull;

use crate::ffi;
use crate::yuv::{I420Planes, Source, chroma_len};
use crate::{EncodedFrame, Error};

/// Encoder settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vp8Config {
    /// Target bitrate in kbit/s.
    pub bitrate_kbps: u32,
    /// Frames per second of the source; used for timestamps and rate control.
    pub fps: u32,
    /// libvpx's speed (`VP8E_SET_CPUUSED`, -16..=16): negative is a fixed speed, a lower
    /// magnitude slower and better; positive lets libvpx pick its speed from how long frames
    /// take, which with frames this small settles at 4 whatever the value. The default, a fixed
    /// 2, gives a closer picture in the same or fewer bits for about 1 ms more a frame
    /// (`docs/research/encode-speed-2026-11.md`).
    pub cpu_used: i32,
    /// Longest gap between keyframes, in frames, when no client asks for one.
    pub keyframe_interval: u32,
    /// Threads a frame is encoded on. It is also cut into as many parts as there are threads
    /// (up to 8), which a decoder with several cores decodes side by side.
    pub threads: u32,
    /// libvpx's screen-content mode (`VP8E_SET_SCREEN_CONTENT_MODE`), tuned for flat colour and
    /// sharp edges: 0 off, 1 on, 2 on with more aggressive rate control (it may drop frames).
    /// 0 leaves the control unset, so the stream is byte for byte what it was without it.
    pub screen_content: u8,
    /// The loop filter's sharpness (`VP8E_SET_SHARPNESS`, 0..=7): higher smooths less across
    /// block edges. 0 leaves the control unset (libvpx's own 0).
    pub sharpness: u8,
}

impl Default for Vp8Config {
    fn default() -> Self {
        Self {
            bitrate_kbps: 2_500,
            fps: 60,
            cpu_used: -2,
            keyframe_interval: 600,
            threads: 4,
            screen_content: 0,
            sharpness: 0,
        }
    }
}

/// libvpx's token-partition setting for a thread count: the base-2 logarithm of how many parts
/// a frame's coefficients are cut into (1, 2, 4 or 8), one per thread.
fn token_partitions(threads: u32) -> i32 {
    match threads {
        0 | 1 => 0,
        2 => 1,
        3..=7 => 2,
        _ => 3,
    }
}

fn vpx_error(op: &'static str, err: ffi::vpx_codec_err_t::Type) -> Error {
    // SAFETY: vpx_codec_err_to_string returns a static C string for any value.
    let msg = unsafe { CStr::from_ptr(ffi::vpx_codec_err_to_string(err)) };
    Error::Vpx(op, msg.to_string_lossy().into_owned())
}

fn check(op: &'static str, err: ffi::vpx_codec_err_t::Type) -> Result<(), Error> {
    if err == ffi::vpx_codec_err_t::VPX_CODEC_OK { Ok(()) } else { Err(vpx_error(op, err)) }
}

/// A libvpx codec context plus the image it encodes from, both boxed so their addresses stay
/// fixed for libvpx.
struct Context {
    ctx: Box<ffi::vpx_codec_ctx_t>,
    image: NonNull<ffi::vpx_image_t>,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context").field("width", &self.width).field("height", &self.height).finish()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: both were initialised in `Vp8Encoder::open` and are destroyed exactly once.
        unsafe {
            ffi::vpx_img_free(self.image.as_ptr());
            ffi::vpx_codec_destroy(&mut *self.ctx);
        }
    }
}

/// A real-time VP8 encoder. It reopens itself if the frame size changes.
#[derive(Debug)]
pub struct Vp8Encoder {
    config: Vp8Config,
    context: Option<Context>,
    frames: i64,
    force_keyframe: bool,
}

// SAFETY: the libvpx context is only ever used through `&mut self`, from one thread at a time.
unsafe impl Send for Vp8Encoder {}

impl Vp8Encoder {
    pub fn new(config: Vp8Config) -> Self {
        Self { config, context: None, frames: 0, force_keyframe: true }
    }

    pub fn config(&self) -> &Vp8Config {
        &self.config
    }

    /// Makes the next frame a keyframe (a client joined, or lost packets and asked for one).
    pub fn request_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    /// Changes the target bitrate from the next frame on.
    pub fn set_bitrate(&mut self, kbps: u32) -> Result<(), Error> {
        self.config.bitrate_kbps = kbps;
        if let Some(c) = &mut self.context {
            let cfg = Self::enc_config(&self.config, c.width, c.height)?;
            // SAFETY: the context is initialised and `cfg` is a valid configuration.
            check("set bitrate", unsafe { ffi::vpx_codec_enc_config_set(&mut *c.ctx, &cfg) })?;
        }
        Ok(())
    }

    fn enc_config(
        config: &Vp8Config,
        width: u32,
        height: u32,
    ) -> Result<ffi::vpx_codec_enc_cfg_t, Error> {
        let mut cfg = ffi::vpx_codec_enc_cfg_t::default();
        // SAFETY: `cfg` is a valid out-parameter; the interface pointer is a static in libvpx.
        check("default config", unsafe {
            ffi::vpx_codec_enc_config_default(ffi::vpx_codec_vp8_cx(), &mut cfg, 0)
        })?;
        cfg.g_w = width;
        cfg.g_h = height;
        cfg.g_timebase = ffi::vpx_rational { num: 1, den: config.fps.max(1) as c_int };
        cfg.g_threads = config.threads.max(1);
        cfg.g_lag_in_frames = 0;
        cfg.g_pass = ffi::vpx_enc_pass::VPX_RC_ONE_PASS;
        cfg.g_error_resilient = ffi::VPX_ERROR_RESILIENT_DEFAULT;
        cfg.rc_end_usage = ffi::vpx_rc_mode::VPX_CBR;
        cfg.rc_target_bitrate = config.bitrate_kbps;
        cfg.rc_min_quantizer = 2;
        cfg.rc_max_quantizer = 52;
        cfg.rc_undershoot_pct = 100;
        cfg.rc_overshoot_pct = 15;
        // Small decoder-buffer model (milliseconds) keeps frame sizes even, so no frame
        // spends long on the wire.
        cfg.rc_buf_initial_sz = 200;
        cfg.rc_buf_optimal_sz = 300;
        cfg.rc_buf_sz = 500;
        cfg.kf_mode = ffi::vpx_kf_mode::VPX_KF_AUTO;
        cfg.kf_min_dist = 0;
        cfg.kf_max_dist = config.keyframe_interval;
        Ok(cfg)
    }

    fn open(&self, width: u32, height: u32) -> Result<Context, Error> {
        let cfg = Self::enc_config(&self.config, width, height)?;
        let mut ctx = Box::new(ffi::vpx_codec_ctx_t::default());
        // SAFETY: `ctx` is zeroed storage at a fixed address; `cfg` is a valid configuration.
        check("init", unsafe {
            ffi::vpx_codec_enc_init_ver(
                &mut *ctx,
                ffi::vpx_codec_vp8_cx(),
                &cfg,
                0,
                ffi::VPX_ENCODER_ABI_VERSION as c_int,
            )
        })?;
        let screen_content = (self.config.screen_content > 0).then_some((
            ffi::vp8e_enc_control_id::VP8E_SET_SCREEN_CONTENT_MODE,
            i32::from(self.config.screen_content.min(2)),
        ));
        let controls = [
            (ffi::vp8e_enc_control_id::VP8E_SET_CPUUSED, self.config.cpu_used),
            (ffi::vp8e_enc_control_id::VP8E_SET_NOISE_SENSITIVITY, 0),
            (ffi::vp8e_enc_control_id::VP8E_SET_STATIC_THRESHOLD, 1),
            (
                ffi::vp8e_enc_control_id::VP8E_SET_TOKEN_PARTITIONS,
                token_partitions(self.config.threads),
            ),
            // Keyframes may use at most 3x an average frame, so a refresh is not a burst.
            (ffi::vp8e_enc_control_id::VP8E_SET_MAX_INTRA_BITRATE_PCT, 300),
        ];
        let sharpness = (self.config.sharpness > 0).then_some((
            ffi::vp8e_enc_control_id::VP8E_SET_SHARPNESS,
            i32::from(self.config.sharpness.min(7)),
        ));
        for (id, value) in controls.into_iter().chain(screen_content).chain(sharpness) {
            // SAFETY: the context is initialised; each control takes one int.
            let err = unsafe { ffi::vpx_codec_control_(&mut *ctx, id as c_int, value as c_int) };
            if let Err(e) = check("control", err) {
                // SAFETY: the context was initialised above.
                unsafe { ffi::vpx_codec_destroy(&mut *ctx) };
                return Err(e);
            }
        }
        // SAFETY: allocates a fresh I420 image of the given size.
        let image = unsafe {
            ffi::vpx_img_alloc(
                std::ptr::null_mut(),
                ffi::vpx_img_fmt::VPX_IMG_FMT_I420,
                width,
                height,
                16,
            )
        };
        let Some(image) = NonNull::new(image) else {
            // SAFETY: the context was initialised above.
            unsafe { ffi::vpx_codec_destroy(&mut *ctx) };
            return Err(Error::Vpx("image alloc", "out of memory".into()));
        };
        Ok(Context { ctx, image, width, height })
    }

    /// Encodes one RGBA frame (`width * height * 4` bytes, tightly packed). Returns `None` if
    /// rate control dropped the frame.
    pub fn encode(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<EncodedFrame>, Error> {
        self.encode_source(Source::Rgba(rgba), width, height)
    }

    /// Encodes one frame already in I420 (packed planes, [`crate::yuv::i420_len`] bytes), as a
    /// GPU converts it: no conversion here, only a copy into the encoder's image.
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
        if self.context.as_ref().is_none_or(|c| (c.width, c.height) != (width, height)) {
            self.context = None;
            self.context = Some(self.open(width, height)?);
            self.force_keyframe = true;
        }
        let c = self.context.as_mut().expect("opened above");

        // SAFETY: the image was allocated for `width`x`height` I420, so each plane holds
        // `stride * rows` bytes, where chroma rows are rounded up.
        unsafe {
            let img = c.image.as_mut();
            let (w, h) = (width as usize, height as usize);
            let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
            let (ys, us, vs) =
                (img.stride[0] as usize, img.stride[1] as usize, img.stride[2] as usize);
            let planes = I420Planes {
                y: std::slice::from_raw_parts_mut(img.planes[0], ys * (h - 1) + w),
                y_stride: ys,
                u: std::slice::from_raw_parts_mut(img.planes[1], us * (ch - 1) + cw),
                u_stride: us,
                v: std::slice::from_raw_parts_mut(img.planes[2], vs * (ch - 1) + cw),
                v_stride: vs,
            };
            source.write_i420(w, h, planes);
        }

        let flags =
            if std::mem::take(&mut self.force_keyframe) { ffi::VPX_EFLAG_FORCE_KF as _ } else { 0 };
        let pts = self.frames;
        self.frames += 1;
        // SAFETY: the context and image are initialised and sized for this frame.
        check("encode", unsafe {
            ffi::vpx_codec_encode(
                &mut *c.ctx,
                c.image.as_ptr(),
                pts,
                1,
                flags,
                ffi::VPX_DL_REALTIME as _,
            )
        })?;

        let mut out: Option<EncodedFrame> = None;
        let mut iter: ffi::vpx_codec_iter_t = std::ptr::null();
        loop {
            // SAFETY: iterating the packets of the encode call above.
            let pkt = unsafe { ffi::vpx_codec_get_cx_data(&mut *c.ctx, &mut iter) };
            // SAFETY: libvpx returns null or a packet valid until the next call.
            let Some(pkt) = (unsafe { pkt.as_ref() }) else { break };
            if pkt.kind != ffi::vpx_codec_cx_pkt_kind::VPX_CODEC_CX_FRAME_PKT {
                continue;
            }
            // SAFETY: for a frame packet the `frame` member of the union is the active one.
            let frame = unsafe { pkt.data.frame };
            // SAFETY: `buf` holds `sz` bytes of compressed data.
            let data = unsafe { std::slice::from_raw_parts(frame.buf.cast::<u8>(), frame.sz) };
            let keyframe = frame.flags & ffi::VPX_FRAME_IS_KEY != 0;
            match &mut out {
                Some(o) => {
                    o.data.extend_from_slice(data);
                    o.keyframe |= keyframe;
                }
                None => out = Some(EncodedFrame { data: data.to_vec(), keyframe }),
            }
        }
        Ok(out)
    }
}

/// A VP8 decoder, used by tests (and later, perhaps, server-side checks).
pub struct Vp8Decoder {
    ctx: Box<ffi::vpx_codec_ctx_t>,
}

impl std::fmt::Debug for Vp8Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vp8Decoder").finish_non_exhaustive()
    }
}

// SAFETY: the libvpx context is only used through `&mut self`.
unsafe impl Send for Vp8Decoder {}

/// A decoded I420 picture: its size and its three planes, each packed with no row padding.
/// `luma` is `width * height` bytes; `u` and `v` are each `chroma_len(width) *
/// chroma_len(height)` (see [`crate::yuv::chroma_len`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

/// Copies `rows` rows of `width` bytes out of a plane whose rows are `stride` bytes apart.
///
/// # Safety
///
/// `plane` must be readable for `stride * (rows - 1) + width` bytes.
pub(crate) unsafe fn pack_plane(
    plane: *const u8,
    stride: usize,
    width: usize,
    rows: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * rows);
    for row in 0..rows {
        // SAFETY: the caller guarantees row `row` of `width` bytes is readable.
        out.extend_from_slice(unsafe {
            std::slice::from_raw_parts(plane.add(row * stride), width)
        });
    }
    out
}

impl Vp8Decoder {
    pub fn new() -> Result<Self, Error> {
        let mut ctx = Box::new(ffi::vpx_codec_ctx_t::default());
        // SAFETY: `ctx` is zeroed storage at a fixed address; a null config is allowed.
        check("decoder init", unsafe {
            ffi::vpx_codec_dec_init_ver(
                &mut *ctx,
                ffi::vpx_codec_vp8_dx(),
                std::ptr::null(),
                0,
                ffi::VPX_DECODER_ABI_VERSION as c_int,
            )
        })?;
        Ok(Self { ctx })
    }

    /// Decodes one compressed frame, returning the picture if one was produced.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedFrame>, Error> {
        // SAFETY: `data` is readable for its length; the context is initialised.
        check("decode", unsafe {
            ffi::vpx_codec_decode(
                &mut *self.ctx,
                data.as_ptr(),
                data.len() as _,
                std::ptr::null_mut(),
                0,
            )
        })?;
        let mut iter: ffi::vpx_codec_iter_t = std::ptr::null();
        // SAFETY: iterating the frames of the decode call above.
        let img = unsafe { ffi::vpx_codec_get_frame(&mut *self.ctx, &mut iter) };
        // SAFETY: libvpx returns null or an image valid until the next decode.
        let Some(img) = (unsafe { img.as_ref() }) else { return Ok(None) };
        let (w, h) = (img.d_w as usize, img.d_h as usize);
        let (cw, ch) = (chroma_len(w), chroma_len(h));
        let plane = |i: usize, width, rows| {
            // SAFETY: a VP8 image is I420: plane 0 holds `h` rows and planes 1 and 2 hold
            // `ch` rows, each of its stride, which is at least the plane's width.
            unsafe { pack_plane(img.planes[i], img.stride[i] as usize, width, rows) }
        };
        Ok(Some(DecodedFrame {
            width: img.d_w,
            height: img.d_h,
            luma: plane(0, w, h),
            u: plane(1, cw, ch),
            v: plane(2, cw, ch),
        }))
    }
}

impl Drop for Vp8Decoder {
    fn drop(&mut self) {
        // SAFETY: initialised in `new`, destroyed once.
        unsafe { ffi::vpx_codec_destroy(&mut *self.ctx) };
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

    #[test]
    fn encodes_and_decodes_at_native_resolution() {
        let mut enc = Vp8Encoder::new(Vp8Config::default());
        let mut dec = Vp8Decoder::new().unwrap();
        for n in 0..30 {
            let frame = enc.encode(&gradient(256, 224, n), 256, 224).unwrap().expect("a frame");
            assert_eq!(frame.keyframe, n == 0, "only the first frame is a keyframe");
            let pic = dec.decode(&frame.data).unwrap().expect("a picture");
            assert_eq!((pic.width, pic.height), (256, 224));
        }
    }

    #[test]
    fn an_i420_frame_encodes_as_its_rgba_does() {
        // One thread: on several, libvpx's output is not the same from run to run.
        let config = Vp8Config { threads: 1, ..Vp8Config::default() };
        let (mut from_rgba, mut from_i420) = (Vp8Encoder::new(config), Vp8Encoder::new(config));
        for n in 0..3 {
            let rgba = gradient(255, 223, n);
            let i420 = crate::yuv::packed_i420(&rgba, 255, 223);
            let a = from_rgba.encode(&rgba, 255, 223).unwrap();
            let b = from_i420.encode_i420(&i420, 255, 223).unwrap();
            assert_eq!(a, b, "frame {n}");
        }
        assert!(matches!(
            from_i420.encode_i420(&[0; 10], 4, 4),
            Err(Error::BadFrame { expected: 24, actual: 10 })
        ));
    }

    #[test]
    fn keyframes_on_request_and_on_resize() {
        let mut enc = Vp8Encoder::new(Vp8Config::default());
        assert!(enc.encode(&gradient(256, 224, 0), 256, 224).unwrap().unwrap().keyframe);
        assert!(!enc.encode(&gradient(256, 224, 1), 256, 224).unwrap().unwrap().keyframe);
        enc.request_keyframe();
        assert!(enc.encode(&gradient(256, 224, 2), 256, 224).unwrap().unwrap().keyframe);
        // A size change (e.g. SNES 256x224 -> 512x448 hi-res mode) reopens the encoder.
        let frame = enc.encode(&gradient(160, 144, 0), 160, 144).unwrap().unwrap();
        assert!(frame.keyframe);
        let pic = Vp8Decoder::new().unwrap().decode(&frame.data).unwrap().unwrap();
        assert_eq!((pic.width, pic.height), (160, 144));
    }

    #[test]
    fn odd_sizes_and_bitrate_changes_work() {
        let mut enc = Vp8Encoder::new(Vp8Config::default());
        let frame = enc.encode(&gradient(255, 223, 0), 255, 223).unwrap().unwrap();
        let pic = Vp8Decoder::new().unwrap().decode(&frame.data).unwrap().unwrap();
        assert_eq!((pic.width, pic.height), (255, 223));
        enc.set_bitrate(800).unwrap();
        assert!(enc.encode(&gradient(255, 223, 1), 255, 223).unwrap().is_some());
    }

    #[test]
    fn every_thread_count_makes_a_stream_that_decodes() {
        assert_eq!([0, 1, 2, 3, 4, 8, 64].map(token_partitions), [0, 0, 1, 2, 2, 3, 3]);
        for threads in [0, 1, 2, 4, 8] {
            let mut enc = Vp8Encoder::new(Vp8Config { threads, ..Vp8Config::default() });
            let mut dec = Vp8Decoder::new().unwrap();
            for n in 0..10 {
                let frame = enc.encode(&gradient(640, 448, n), 640, 448).unwrap().expect("a frame");
                let pic = dec.decode(&frame.data).unwrap().expect("a picture");
                assert_eq!((pic.width, pic.height), (640, 448), "{threads} threads");
            }
        }
    }

    /// Flat colour blocks with hard edges, sliding a pixel a frame: what screen-content mode is for.
    fn blocks(w: u32, h: u32, shift: u32) -> Vec<u8> {
        let colours = [[255, 0, 0], [0, 0, 255], [255, 255, 0], [0, 160, 0]];
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = colours[(((x + shift) / 16 + y / 16) % 4) as usize];
                out.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        out
    }

    fn stream(config: Vp8Config, frames: u32) -> Vec<Option<EncodedFrame>> {
        let mut enc = Vp8Encoder::new(config);
        (0..frames).map(|n| enc.encode(&blocks(256, 224, n), 256, 224).unwrap()).collect()
    }

    #[test]
    fn every_screen_content_mode_makes_a_stream_that_decodes() {
        for screen_content in [0, 1, 2] {
            let config = Vp8Config { screen_content, bitrate_kbps: 800, ..Vp8Config::default() };
            let mut dec = Vp8Decoder::new().unwrap();
            let mut pictures = 0;
            for frame in stream(config, 30).into_iter().flatten() {
                let pic = dec.decode(&frame.data).unwrap().expect("a picture");
                assert_eq!((pic.width, pic.height), (256, 224), "mode {screen_content}");
                pictures += 1;
            }
            assert!(pictures > 0, "mode {screen_content} sent nothing");
        }
    }

    #[test]
    fn screen_content_mode_0_is_the_stream_without_it() {
        // Mode 0 leaves the control unset: the same frames make the same bytes as an encoder
        // configured as before the setting existed. Screen-content mode changes them where the
        // bitrate is tight (with plenty to spare, this content is at the lowest quantizer either
        // way and the modes make the same stream).
        // A positive `cpu_used` lets libvpx change speed with how long frames take, so a loaded
        // machine encodes differently from run to run: a fixed speed (negative) and one thread
        // make the bytes repeatable.
        let tight =
            Vp8Config { bitrate_kbps: 800, cpu_used: -8, threads: 1, ..Vp8Config::default() };
        let off = stream(tight, 20);
        assert_eq!(off, stream(Vp8Config { screen_content: 0, ..tight }, 20));
        assert_ne!(off, stream(Vp8Config { screen_content: 1, ..tight }, 20));
    }

    #[test]
    fn sharpness_0_is_the_stream_without_it_and_7_decodes() {
        // As screen-content mode: 0 leaves the control unset, so a server that does not set it
        // sends what it did before; 7 filters block edges less, which changes the bytes where the
        // rate is tight, and still decodes.
        let tight =
            Vp8Config { bitrate_kbps: 800, cpu_used: -8, threads: 1, ..Vp8Config::default() };
        let off = stream(tight, 20);
        assert_eq!(off, stream(Vp8Config { sharpness: 0, ..tight }, 20));
        let sharp = stream(Vp8Config { sharpness: 7, ..tight }, 20);
        assert_ne!(off, sharp);
        let mut dec = Vp8Decoder::new().unwrap();
        for frame in sharp.into_iter().flatten() {
            let pic = dec.decode(&frame.data).unwrap().expect("a picture");
            assert_eq!((pic.width, pic.height), (256, 224));
        }
    }

    #[test]
    fn decoded_frames_carry_both_chroma_planes() {
        // A solid colour comes back with its own chroma in every sample of each plane.
        let (w, h) = (33, 17);
        let rgba = [255u8, 0, 0, 255].repeat((w * h) as usize);
        let frame = Vp8Encoder::new(Vp8Config::default()).encode(&rgba, w, h).unwrap().unwrap();
        let pic = Vp8Decoder::new().unwrap().decode(&frame.data).unwrap().unwrap();
        let chroma = chroma_len(w as usize) * chroma_len(h as usize);
        assert_eq!((pic.luma.len(), pic.u.len(), pic.v.len()), ((w * h) as usize, chroma, chroma));
        // BT.601 limited red is (82, 90, 240); allow the codec a little rounding.
        assert!(pic.u.iter().all(|&u| u.abs_diff(90) <= 3), "u {:?}", &pic.u[..4]);
        assert!(pic.v.iter().all(|&v| v.abs_diff(240) <= 3), "v {:?}", &pic.v[..4]);
    }

    #[test]
    fn rejects_a_wrong_sized_buffer() {
        let mut enc = Vp8Encoder::new(Vp8Config::default());
        assert!(matches!(enc.encode(&[0; 10], 4, 4), Err(Error::BadFrame { expected: 64, .. })));
    }
}
