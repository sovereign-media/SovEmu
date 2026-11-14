//! Headless OpenGL for cores that render with the GPU (`RETRO_ENVIRONMENT_SET_HW_RENDER`).
//!
//! There is no window: an EGL context renders into a framebuffer object the core draws into,
//! and each frame is read back to system memory for the video encoder. libEGL is opened at run
//! time, so a machine without it (macOS, a minimal container) simply has no GPU rendering.
//!
//! Devices come from `EGL_EXT_device_enumeration`: each `/dev/dri/renderD*` node a driver
//! serves, plus Mesa's software device (llvmpipe) when no GPU is usable. Like Sovereign's
//! codec probe, a device counts only once it has actually drawn a frame and read it back
//! ([`probe`]); listing it is not enough.

use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::OnceLock;

use libloading::Library;

type EglDisplay = *mut c_void;
type EglConfig = *mut c_void;
type EglContext = *mut c_void;
type EglDevice = *mut c_void;
type EglInt = i32;
type EglBool = u32;

const EGL_NONE: EglInt = 0x3038;
const EGL_EXTENSIONS: EglInt = 0x3055;
const EGL_RENDERABLE_TYPE: EglInt = 0x3040;
const EGL_SURFACE_TYPE: EglInt = 0x3033;
const EGL_OPENGL_BIT: EglInt = 0x0008;
const EGL_OPENGL_ES2_BIT: EglInt = 0x0004;
const EGL_OPENGL_ES3_BIT: EglInt = 0x0040;
const EGL_OPENGL_API: u32 = 0x30A2;
const EGL_OPENGL_ES_API: u32 = 0x30A0;
const EGL_CONTEXT_MAJOR_VERSION: EglInt = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: EglInt = 0x30FB;
const EGL_CONTEXT_OPENGL_PROFILE_MASK: EglInt = 0x30FD;
const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: EglInt = 0x1;
const EGL_CONTEXT_OPENGL_COMPATIBILITY_PROFILE_BIT: EglInt = 0x2;
const EGL_PLATFORM_DEVICE_EXT: u32 = 0x313F;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;
const EGL_DRM_DEVICE_FILE_EXT: EglInt = 0x3233;
const EGL_DRM_RENDER_NODE_FILE_EXT: EglInt = 0x377C;

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_DEPTH_ATTACHMENT: u32 = 0x8D00;
const GL_DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
const GL_RGBA8: u32 = 0x8058;
const GL_DEPTH24_STENCIL8: u32 = 0x88F0;
const GL_DEPTH_COMPONENT24: u32 = 0x81A6;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_PACK_ALIGNMENT: u32 = 0x0D05;
const GL_VENDOR: u32 = 0x1F00;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

macro_rules! fns {
    ($vis:vis struct $name:ident { $($field:ident : $sym:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)* }) => {
        $vis struct $name {
            $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }
    };
}

fns! {
    struct EglFns {
        get_proc_address: "eglGetProcAddress" => fn(*const c_char) -> *mut c_void;
        initialize: "eglInitialize" => fn(EglDisplay, *mut EglInt, *mut EglInt) -> EglBool;
        terminate: "eglTerminate" => fn(EglDisplay) -> EglBool;
        bind_api: "eglBindAPI" => fn(u32) -> EglBool;
        choose_config: "eglChooseConfig" => fn(EglDisplay, *const EglInt, *mut EglConfig, EglInt, *mut EglInt) -> EglBool;
        create_context: "eglCreateContext" => fn(EglDisplay, EglConfig, EglContext, *const EglInt) -> EglContext;
        destroy_context: "eglDestroyContext" => fn(EglDisplay, EglContext) -> EglBool;
        make_current: "eglMakeCurrent" => fn(EglDisplay, *mut c_void, *mut c_void, EglContext) -> EglBool;
        get_current_context: "eglGetCurrentContext" => fn() -> EglContext;
        get_error: "eglGetError" => fn() -> EglInt;
    }
}

struct Egl {
    _lib: Library,
    f: EglFns,
    get_platform_display:
        Option<unsafe extern "C" fn(u32, *mut c_void, *const EglInt) -> EglDisplay>,
    query_devices: Option<unsafe extern "C" fn(EglInt, *mut EglDevice, *mut EglInt) -> EglBool>,
    query_device_string: Option<unsafe extern "C" fn(EglDevice, EglInt) -> *const c_char>,
}

static EGL: OnceLock<Result<Egl, String>> = OnceLock::new();

fn egl() -> Result<&'static Egl, String> {
    EGL.get_or_init(|| {
        // SAFETY: libEGL's initialisers are trusted system code.
        let lib = unsafe { Library::new("libEGL.so.1") }
            .or_else(|_| unsafe { Library::new("libEGL.so") })
            .map_err(|e| format!("libEGL is not available: {e}"))?;
        macro_rules! sym {
            ($s:literal) => {
                // SAFETY: the symbol has the signature EGL 1.5 defines, matching the field.
                *unsafe { lib.get(concat!($s, "\0").as_bytes()) }
                    .map_err(|e| format!("libEGL lacks {}: {e}", $s))?
            };
        }
        let f = EglFns {
            get_proc_address: sym!("eglGetProcAddress"),
            initialize: sym!("eglInitialize"),
            terminate: sym!("eglTerminate"),
            bind_api: sym!("eglBindAPI"),
            choose_config: sym!("eglChooseConfig"),
            create_context: sym!("eglCreateContext"),
            destroy_context: sym!("eglDestroyContext"),
            make_current: sym!("eglMakeCurrent"),
            get_current_context: sym!("eglGetCurrentContext"),
            get_error: sym!("eglGetError"),
        };
        let ext = |name: &str| {
            let name = CString::new(name).expect("no NUL");
            // SAFETY: eglGetProcAddress takes a C string and returns null or the function.
            let p = unsafe { (f.get_proc_address)(name.as_ptr()) };
            (!p.is_null()).then_some(p)
        };
        // SAFETY: each pointer is the extension function of that name, whose signature the
        // EGL extension registry defines and the transmute target matches.
        let (get_platform_display, query_devices, query_device_string) = unsafe {
            (
                ext("eglGetPlatformDisplayEXT").map(|p| std::mem::transmute(p)),
                ext("eglQueryDevicesEXT").map(|p| std::mem::transmute(p)),
                ext("eglQueryDeviceStringEXT").map(|p| std::mem::transmute(p)),
            )
        };
        Ok(Egl { _lib: lib, f, get_platform_display, query_devices, query_device_string })
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Whether EGL can be loaded at all.
pub fn available() -> bool {
    egl().is_ok()
}

/// Which GPU to render with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GpuChoice {
    /// The first hardware device that works, else the software renderer.
    #[default]
    Auto,
    /// Mesa's software renderer (llvmpipe), even if a GPU is present.
    Software,
    /// This render node, e.g. `/dev/dri/renderD128`.
    Device(String),
}

impl std::str::FromStr for GpuChoice {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "" | "auto" => Self::Auto,
            "software" => Self::Software,
            path => Self::Device(path.to_owned()),
        })
    }
}

impl std::fmt::Display for GpuChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Software => f.write_str("software"),
            Self::Device(path) => f.write_str(path),
        }
    }
}

/// The client API a core asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlApi {
    /// Desktop OpenGL, compatibility profile.
    Gl,
    /// Desktop OpenGL, core profile.
    GlCore,
    /// OpenGL ES.
    Gles,
}

/// What a core asked for in `SET_HW_RENDER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextRequest {
    pub api: GlApi,
    pub major: u32,
    pub minor: u32,
    pub depth: bool,
    pub stencil: bool,
}

/// One device as EGL lists it.
struct Device {
    handle: EglDevice,
    /// The render node, or none for the software device.
    node: Option<String>,
    software: bool,
}

fn devices(egl: &Egl) -> Vec<Device> {
    let (Some(query), Some(string)) = (egl.query_devices, egl.query_device_string) else {
        return Vec::new();
    };
    let mut handles = [std::ptr::null_mut(); 16];
    let mut count = 0;
    // SAFETY: `handles` has room for 16 devices and `count` receives how many were written.
    if unsafe { query(16, handles.as_mut_ptr(), &mut count) } == 0 {
        return Vec::new();
    }
    let read = |dev: EglDevice, name: EglInt| {
        // SAFETY: a device EGL returned; the string is static in the driver, or null.
        let p = unsafe { string(dev, name) };
        // SAFETY: non-null strings from EGL are NUL-terminated.
        (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    };
    handles[..count.clamp(0, 16) as usize]
        .iter()
        .map(|&handle| {
            let extensions = read(handle, EGL_EXTENSIONS).unwrap_or_default();
            Device {
                handle,
                node: read(handle, EGL_DRM_RENDER_NODE_FILE_EXT)
                    .or_else(|| read(handle, EGL_DRM_DEVICE_FILE_EXT)),
                software: extensions.contains("EGL_MESA_device_software"),
            }
        })
        .collect()
}

macro_rules! gl_fns {
    ($($field:ident : $sym:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)*) => {
        /// The GL entry points SovEmu itself calls (the core loads its own).
        struct Gl {
            $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl Gl {
            fn load(egl: &Egl) -> Result<Self, String> {
                Ok(Self {
                    $($field: {
                        let p = proc_address_with(egl, $sym);
                        if p.is_null() {
                            return Err(format!("the GL driver lacks {}", $sym));
                        }
                        // SAFETY: the pointer is the GL function of that name, whose signature
                        // the GL specification gives as this field's type.
                        unsafe {
                            std::mem::transmute::<*const c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(p)
                        }
                    },)*
                })
            }
        }
    };
}

gl_fns! {
    gen_framebuffers: "glGenFramebuffers" => fn(i32, *mut u32);
    delete_framebuffers: "glDeleteFramebuffers" => fn(i32, *const u32);
    bind_framebuffer: "glBindFramebuffer" => fn(u32, u32);
    gen_renderbuffers: "glGenRenderbuffers" => fn(i32, *mut u32);
    delete_renderbuffers: "glDeleteRenderbuffers" => fn(i32, *const u32);
    bind_renderbuffer: "glBindRenderbuffer" => fn(u32, u32);
    renderbuffer_storage: "glRenderbufferStorage" => fn(u32, u32, i32, i32);
    framebuffer_renderbuffer: "glFramebufferRenderbuffer" => fn(u32, u32, u32, u32);
    check_framebuffer_status: "glCheckFramebufferStatus" => fn(u32) -> u32;
    read_pixels: "glReadPixels" => fn(i32, i32, i32, i32, u32, u32, *mut c_void);
    pixel_store_i: "glPixelStorei" => fn(u32, i32);
    get_string: "glGetString" => fn(u32) -> *const u8;
    clear_color: "glClearColor" => fn(f32, f32, f32, f32);
    clear: "glClear" => fn(u32);
    viewport: "glViewport" => fn(i32, i32, i32, i32);
    finish: "glFinish" => fn();
}

impl Gl {
    fn string(&self, name: u32) -> String {
        // SAFETY: a context is current; glGetString returns a static string or null.
        let p = unsafe { (self.get_string)(name) };
        if p.is_null() {
            return String::new();
        }
        // SAFETY: non-null GL strings are NUL-terminated.
        unsafe { CStr::from_ptr(p.cast()) }.to_string_lossy().into_owned()
    }
}

fn proc_address_with(egl: &Egl, name: &str) -> *const c_void {
    let Ok(name) = CString::new(name) else { return std::ptr::null() };
    // SAFETY: a C string in, a function pointer or null out.
    unsafe { (egl.f.get_proc_address)(name.as_ptr()) }.cast_const()
}

/// A GL function for the core (`retro_hw_render_callback::get_proc_address`).
pub(crate) fn proc_address(name: &CStr) -> *const c_void {
    match egl() {
        // SAFETY: a C string in, a function pointer or null out.
        Ok(egl) => unsafe { (egl.f.get_proc_address)(name.as_ptr()) }.cast_const(),
        Err(_) => std::ptr::null(),
    }
}

/// The renderer a context ended up on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererInfo {
    /// The render node, or none for the software renderer.
    pub node: Option<String>,
    pub vendor: String,
    pub renderer: String,
    pub version: String,
    /// A CPU renderer (llvmpipe, softpipe, swrast).
    pub software: bool,
}

/// A current, headless GL context with a framebuffer the core renders into.
pub struct GlContext {
    egl: &'static Egl,
    gl: Gl,
    display: EglDisplay,
    context: EglContext,
    fbo: u32,
    color: u32,
    depth: u32,
    depth_format: Option<(u32, u32)>,
    width: u32,
    height: u32,
    pub info: RendererInfo,
}

impl std::fmt::Debug for GlContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlContext").field("info", &self.info).field("fbo", &self.fbo).finish()
    }
}

// SAFETY: the context is only used by the thread that owns the `Core` (it is made current on
// whichever thread calls in, see `make_current`); the raw handles are just identifiers.
unsafe impl Send for GlContext {}

fn is_software_renderer(renderer: &str) -> bool {
    let r = renderer.to_ascii_lowercase();
    r.contains("llvmpipe") || r.contains("softpipe") || r.contains("swrast")
}

impl GlContext {
    /// Creates a context on the chosen device, framebuffer `width` × `height`.
    pub fn create(
        choice: &GpuChoice,
        request: ContextRequest,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let egl = egl()?;
        let all = devices(egl);
        let mut candidates: Vec<&Device> = match choice {
            GpuChoice::Device(path) => {
                all.iter().filter(|d| d.node.as_deref() == Some(path)).collect()
            }
            GpuChoice::Software => all.iter().filter(|d| d.software).collect(),
            GpuChoice::Auto => {
                let mut v: Vec<&Device> = all.iter().filter(|d| !d.software).collect();
                v.extend(all.iter().filter(|d| d.software));
                v
            }
        };
        if let GpuChoice::Device(path) = choice
            && candidates.is_empty()
        {
            return Err(format!("no EGL device serves {path}"));
        }
        let mut errors = Vec::new();
        for device in candidates.drain(..) {
            match Self::create_on(egl, Some(device), request, width, height) {
                Ok(ctx) if *choice == GpuChoice::Auto || !ctx.info.software || device.software => {
                    return Ok(ctx);
                }
                Ok(ctx) => errors.push(format!("{}: software only", ctx.info.renderer)),
                Err(e) => {
                    errors.push(format!("{}: {e}", device.node.as_deref().unwrap_or("software")))
                }
            }
        }
        // No device enumeration (old Mesa): Mesa's surfaceless platform picks for itself.
        if all.is_empty() && *choice != GpuChoice::Software {
            return Self::create_on(egl, None, request, width, height);
        }
        Err(if errors.is_empty() { "no EGL devices".to_owned() } else { errors.join("; ") })
    }

    fn create_on(
        egl: &'static Egl,
        device: Option<&Device>,
        request: ContextRequest,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let get_display = egl.get_platform_display.ok_or("EGL lacks eglGetPlatformDisplayEXT")?;
        let none = [EGL_NONE];
        // SAFETY: a platform enum with its matching native display (a device, or none).
        let display = unsafe {
            match device {
                Some(d) => get_display(EGL_PLATFORM_DEVICE_EXT, d.handle, none.as_ptr()),
                None => {
                    get_display(EGL_PLATFORM_SURFACELESS_MESA, std::ptr::null_mut(), none.as_ptr())
                }
            }
        };
        if display.is_null() {
            return Err("no EGL display".into());
        }
        // SAFETY: from here on, every EGL call gets the display just created; out-parameters
        // point to locals of the right type; attribute lists end with EGL_NONE.
        unsafe {
            let (mut major, mut minor) = (0, 0);
            if (egl.f.initialize)(display, &mut major, &mut minor) == 0 {
                return Err(format!("eglInitialize failed ({:#x})", (egl.f.get_error)()));
            }
            let fail = |what: &str| {
                let e = format!("{what} failed ({:#x})", (egl.f.get_error)());
                (egl.f.terminate)(display);
                Err(e)
            };
            let (api, bit) = match request.api {
                GlApi::Gles if request.major >= 3 => (EGL_OPENGL_ES_API, EGL_OPENGL_ES3_BIT),
                GlApi::Gles => (EGL_OPENGL_ES_API, EGL_OPENGL_ES2_BIT),
                GlApi::Gl | GlApi::GlCore => (EGL_OPENGL_API, EGL_OPENGL_BIT),
            };
            if (egl.f.bind_api)(api) == 0 {
                return fail("eglBindAPI");
            }
            // No surfaces at all (rendering goes to a framebuffer object), so no surface type:
            // the default asks for window surfaces, which a headless device has none of.
            let attrs = [EGL_RENDERABLE_TYPE, bit, EGL_SURFACE_TYPE, 0, EGL_NONE];
            let mut config = std::ptr::null_mut();
            let mut found = 0;
            if (egl.f.choose_config)(display, attrs.as_ptr(), &mut config, 1, &mut found) == 0
                || found == 0
            {
                // EGL_KHR_no_config_context: a context needs no config when it has no surface.
                config = std::ptr::null_mut();
            }
            let mut ctx_attrs = Vec::new();
            if request.major > 0 {
                ctx_attrs.extend([EGL_CONTEXT_MAJOR_VERSION, request.major as EglInt]);
                ctx_attrs.extend([EGL_CONTEXT_MINOR_VERSION, request.minor as EglInt]);
            }
            match request.api {
                GlApi::GlCore => ctx_attrs
                    .extend([EGL_CONTEXT_OPENGL_PROFILE_MASK, EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT]),
                GlApi::Gl if request.major >= 3 => ctx_attrs.extend([
                    EGL_CONTEXT_OPENGL_PROFILE_MASK,
                    EGL_CONTEXT_OPENGL_COMPATIBILITY_PROFILE_BIT,
                ]),
                _ => {}
            }
            ctx_attrs.push(EGL_NONE);
            let context =
                (egl.f.create_context)(display, config, std::ptr::null_mut(), ctx_attrs.as_ptr());
            if context.is_null() {
                return fail("eglCreateContext");
            }
            if (egl.f.make_current)(display, std::ptr::null_mut(), std::ptr::null_mut(), context)
                == 0
            {
                (egl.f.destroy_context)(display, context);
                return fail("eglMakeCurrent (surfaceless)");
            }
            let gl = match Gl::load(egl) {
                Ok(gl) => gl,
                Err(e) => {
                    (egl.f.destroy_context)(display, context);
                    (egl.f.terminate)(display);
                    return Err(e);
                }
            };
            let renderer = gl.string(GL_RENDERER);
            let info = RendererInfo {
                node: device.and_then(|d| d.node.clone()),
                vendor: gl.string(GL_VENDOR),
                software: device.is_some_and(|d| d.software) || is_software_renderer(&renderer),
                renderer,
                version: gl.string(GL_VERSION),
            };
            let depth_format = match (request.depth, request.stencil) {
                (_, true) => Some((GL_DEPTH24_STENCIL8, GL_DEPTH_STENCIL_ATTACHMENT)),
                (true, false) => Some((GL_DEPTH_COMPONENT24, GL_DEPTH_ATTACHMENT)),
                (false, false) => None,
            };
            let mut ctx = Self {
                egl,
                gl,
                display,
                context,
                fbo: 0,
                color: 0,
                depth: 0,
                depth_format,
                width: 0,
                height: 0,
                info,
            };
            (ctx.gl.gen_framebuffers)(1, &mut ctx.fbo);
            (ctx.gl.gen_renderbuffers)(1, &mut ctx.color);
            if depth_format.is_some() {
                (ctx.gl.gen_renderbuffers)(1, &mut ctx.depth);
            }
            ctx.resize(width.max(1), height.max(1))?;
            Ok(ctx)
        }
    }

    /// The framebuffer the core renders into (`get_current_framebuffer`).
    pub fn framebuffer(&self) -> u32 {
        self.fbo
    }

    /// Makes the context current on this thread, if it is not already.
    pub fn make_current(&self) {
        // SAFETY: the display and context are live; surfaceless, so no surfaces.
        unsafe {
            if (self.egl.f.get_current_context)() != self.context {
                (self.egl.f.make_current)(
                    self.display,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    self.context,
                );
            }
        }
    }

    /// Reallocates the framebuffer's storage for a larger frame (the framebuffer keeps its id).
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        if width <= self.width && height <= self.height {
            return Ok(());
        }
        self.make_current();
        let (w, h) = (width.max(self.width) as i32, height.max(self.height) as i32);
        // SAFETY: the context is current and the names were generated on it.
        let status = unsafe {
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.gl.bind_renderbuffer)(GL_RENDERBUFFER, self.color);
            (self.gl.renderbuffer_storage)(GL_RENDERBUFFER, GL_RGBA8, w, h);
            (self.gl.framebuffer_renderbuffer)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_RENDERBUFFER,
                self.color,
            );
            if let Some((format, attachment)) = self.depth_format {
                (self.gl.bind_renderbuffer)(GL_RENDERBUFFER, self.depth);
                (self.gl.renderbuffer_storage)(GL_RENDERBUFFER, format, w, h);
                (self.gl.framebuffer_renderbuffer)(
                    GL_FRAMEBUFFER,
                    attachment,
                    GL_RENDERBUFFER,
                    self.depth,
                );
            }
            (self.gl.bind_renderbuffer)(GL_RENDERBUFFER, 0);
            let status = (self.gl.check_framebuffer_status)(GL_FRAMEBUFFER);
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            status
        };
        if status != GL_FRAMEBUFFER_COMPLETE {
            return Err(format!("framebuffer incomplete ({status:#x})"));
        }
        (self.width, self.height) = (w as u32, h as u32);
        Ok(())
    }

    /// Reads the bottom-left `width` × `height` of the framebuffer as RGBA rows, top row first
    /// (`flip`: the core drew with a bottom-left origin, as GL does).
    pub fn read(&self, width: u32, height: u32, flip: bool, out: &mut Vec<u8>) {
        let (w, h) = (width.min(self.width), height.min(self.height));
        let row = w as usize * 4;
        out.resize(row * h as usize, 0);
        self.make_current();
        // SAFETY: the context is current; `out` holds exactly `h` rows of `w` RGBA pixels,
        // packed (pack alignment 1).
        unsafe {
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.gl.pixel_store_i)(GL_PACK_ALIGNMENT, 1);
            (self.gl.read_pixels)(
                0,
                0,
                w as i32,
                h as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                out.as_mut_ptr().cast(),
            );
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        }
        if flip {
            let rows = h as usize;
            for y in 0..rows / 2 {
                let (top, bottom) = out.split_at_mut((rows - 1 - y) * row);
                top[y * row..(y + 1) * row].swap_with_slice(&mut bottom[..row]);
            }
        }
    }

    /// Draws a known colour into the framebuffer and reads it back: proof the device renders.
    fn self_test(&self) -> Result<(), String> {
        let mut out = Vec::new();
        self.make_current();
        // SAFETY: the context is current; the framebuffer is complete.
        unsafe {
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.gl.viewport)(0, 0, 16, 16);
            (self.gl.clear_color)(0.25, 0.5, 0.75, 1.0);
            (self.gl.clear)(GL_COLOR_BUFFER_BIT);
            (self.gl.finish)();
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        }
        self.read(16, 16, false, &mut out);
        let px = &out[..4];
        let near = |a: u8, b: u8| a.abs_diff(b) <= 2;
        if near(px[0], 64) && near(px[1], 128) && near(px[2], 191) {
            Ok(())
        } else {
            Err(format!("drew (64, 128, 191) but read back {:?}", &px[..3]))
        }
    }
}

impl Drop for GlContext {
    fn drop(&mut self) {
        self.make_current();
        // SAFETY: the context is current; the names were generated on it; afterwards nothing
        // uses the context or display again.
        unsafe {
            (self.gl.delete_framebuffers)(1, &self.fbo);
            (self.gl.delete_renderbuffers)(1, &self.color);
            if self.depth != 0 {
                (self.gl.delete_renderbuffers)(1, &self.depth);
            }
            (self.egl.f.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            (self.egl.f.destroy_context)(self.display, self.context);
            (self.egl.f.terminate)(self.display);
        }
    }
}

/// What the GPU probe found for one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuReport {
    /// The render node; none for the software device.
    pub node: Option<String>,
    /// The kernel driver (`amdgpu`, `i915`, `virtio_gpu`, ...), from sysfs.
    pub driver: Option<String>,
    /// `hardware`, `virtual` (a hypervisor's adapter) or `software`.
    pub kind: &'static str,
    /// The renderer, once a context was made.
    pub renderer: Option<RendererInfo>,
    /// Drew a frame and read it back.
    pub works: bool,
    pub error: Option<String>,
}

/// The kernel driver behind a render node, from `/sys/class/drm/<node>/device/driver`.
fn kernel_driver(node: &str) -> Option<String> {
    let name = node.rsplit('/').next()?;
    let link = std::fs::read_link(format!("/sys/class/drm/{name}/device/driver")).ok()?;
    Some(link.file_name()?.to_string_lossy().into_owned())
}

/// Drivers of the display adapters hypervisors present: they render (some through the host),
/// but are no better than the software renderer for a game, so `Auto` skips them.
const VIRTUAL_DRIVERS: &[&str] = &[
    "virtio_gpu",
    "virtio-pci",
    "vmwgfx",
    "qxl",
    "bochs-drm",
    "bochs",
    "cirrus",
    "hyperv_drm",
    "vboxvideo",
];

/// Tries every EGL device: makes a context, draws a frame, reads it back. Err when EGL itself
/// is unavailable.
pub fn probe() -> Result<Vec<GpuReport>, String> {
    let egl = egl()?;
    let request =
        ContextRequest { api: GlApi::Gl, major: 0, minor: 0, depth: false, stencil: false };
    let mut reports = Vec::new();
    for device in devices(egl) {
        let driver = device.node.as_deref().and_then(kernel_driver);
        let kind = if device.software {
            "software"
        } else if driver.as_deref().is_some_and(|d| VIRTUAL_DRIVERS.contains(&d)) {
            "virtual"
        } else {
            "hardware"
        };
        let result = GlContext::create_on(egl, Some(&device), request, 16, 16).and_then(|ctx| {
            ctx.self_test()?;
            Ok(ctx.info.clone())
        });
        let (renderer, works, error) = match result {
            Ok(info) => (Some(info), true, None),
            Err(e) => (None, false, Some(e)),
        };
        let kind = if renderer.as_ref().is_some_and(|r| r.software) { "software" } else { kind };
        reports.push(GpuReport { node: device.node.clone(), driver, kind, renderer, works, error });
    }
    Ok(reports)
}

/// The device `Auto` would choose from a probe: the first working hardware GPU, else software.
pub fn best(reports: &[GpuReport]) -> Option<&GpuReport> {
    reports
        .iter()
        .find(|r| r.works && r.kind == "hardware")
        .or_else(|| reports.iter().find(|r| r.works && r.kind == "software"))
        .or_else(|| reports.iter().find(|r| r.works))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_choices_parse() {
        assert_eq!("auto".parse::<GpuChoice>().unwrap(), GpuChoice::Auto);
        assert_eq!("".parse::<GpuChoice>().unwrap(), GpuChoice::Auto);
        assert_eq!("software".parse::<GpuChoice>().unwrap(), GpuChoice::Software);
        assert_eq!(
            "/dev/dri/renderD129".parse::<GpuChoice>().unwrap(),
            GpuChoice::Device("/dev/dri/renderD129".into())
        );
        assert_eq!(
            GpuChoice::Device("/dev/dri/renderD129".into()).to_string(),
            "/dev/dri/renderD129"
        );
    }

    #[test]
    fn recognises_software_renderers() {
        assert!(is_software_renderer("llvmpipe (LLVM 19.1.7, 256 bits)"));
        assert!(!is_software_renderer("AMD Radeon RX 6600 (radeonsi, navi23, LLVM 19.1.7)"));
    }

    #[test]
    fn prefers_working_hardware_then_software() {
        let r = |kind, works| GpuReport {
            node: None,
            driver: None,
            kind,
            renderer: None,
            works,
            error: None,
        };
        let reports =
            [r("virtual", true), r("hardware", false), r("software", true), r("hardware", true)];
        assert_eq!(best(&reports).map(|r| r.kind), Some("hardware"));
        assert_eq!(best(&reports[..3]).map(|r| r.kind), Some("software"));
        assert!(best(&[r("hardware", false)]).is_none());
    }
}
