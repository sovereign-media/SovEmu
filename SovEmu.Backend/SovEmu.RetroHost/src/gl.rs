//! Headless OpenGL for cores that render with the GPU (`RETRO_ENVIRONMENT_SET_HW_RENDER`).
//!
//! There is no window: an EGL context renders into a framebuffer object the core draws into,
//! and each frame is read back to system memory for the video encoder: as RGBA, or converted to
//! I420 on the GPU first ([`yuv`]), which reads back 1.5 bytes a pixel instead of 4 and leaves
//! the CPU no conversion to do. libEGL is opened at run time, so a machine without it (macOS, a
//! minimal container) simply has no GPU rendering.
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
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE_BINDING_2D: u32 = 0x8069;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_TEXTURE_WRAP_S: u32 = 0x2802;
const GL_TEXTURE_WRAP_T: u32 = 0x2803;
const GL_NEAREST: i32 = 0x2600;
const GL_CLAMP_TO_EDGE: i32 = 0x812F;

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
    ($(#[$meta:meta])* struct $name:ident {
        $($field:ident : $sym:literal => fn($($arg:ty),*) $(-> $ret:ty)?;)*
    }) => {
        $(#[$meta])*
        struct $name {
            $($field: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)*
        }

        impl $name {
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
    /// The GL entry points SovEmu itself calls (the core loads its own).
    struct Gl {
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
    gen_textures: "glGenTextures" => fn(i32, *mut u32);
    delete_textures: "glDeleteTextures" => fn(i32, *const u32);
    bind_texture: "glBindTexture" => fn(u32, u32);
    tex_image_2d: "glTexImage2D" => fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
    tex_parameter_i: "glTexParameteri" => fn(u32, u32, i32);
    framebuffer_texture_2d: "glFramebufferTexture2D" => fn(u32, u32, u32, u32, i32);
    get_integer_v: "glGetIntegerv" => fn(u32, *mut i32);
    }
}

// After `gl_fns!`, which it uses.
mod yuv;

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
    config: EglConfig,
    context: EglContext,
    /// The client API the context was made for (`EGL_OPENGL_API` or `EGL_OPENGL_ES_API`).
    api: u32,
    fbo: u32,
    /// The framebuffer's colour: a texture, so the I420 pass can sample it.
    color: u32,
    depth: u32,
    depth_format: Option<(u32, u32)>,
    width: u32,
    height: u32,
    /// The pass that converts frames to I420 on the GPU, once tried: why not, if it failed.
    yuv: Option<Result<yuv::YuvPass, String>>,
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
                config,
                context,
                api,
                fbo: 0,
                color: 0,
                depth: 0,
                depth_format,
                width: 0,
                height: 0,
                yuv: None,
                info,
            };
            (ctx.gl.gen_framebuffers)(1, &mut ctx.fbo);
            (ctx.gl.gen_textures)(1, &mut ctx.color);
            if depth_format.is_some() {
                (ctx.gl.gen_renderbuffers)(1, &mut ctx.depth);
            }
            // This constructor's own errors are its message alone: the caller names the GPU.
            ctx.resize(width.max(1), height.max(1)).map_err(|e| match e {
                crate::Error::Gpu(message) => message,
                other => other.to_string(),
            })?;
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
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), crate::Error> {
        if width <= self.width && height <= self.height {
            return Ok(());
        }
        self.make_current();
        let (w, h) = (width.max(self.width) as i32, height.max(self.height) as i32);
        // SAFETY: the context is current and the names were generated on it; the texture
        // binding is the core's, so it is put back.
        let status = unsafe {
            let mut bound = 0;
            (self.gl.get_integer_v)(GL_TEXTURE_BINDING_2D, &mut bound);
            (self.gl.bind_texture)(GL_TEXTURE_2D, self.color);
            alloc_rgba_texture(&self.gl, self.gles(), w, h, std::ptr::null());
            (self.gl.bind_texture)(GL_TEXTURE_2D, bound as u32);
            (self.gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (self.gl.framebuffer_texture_2d)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                self.color,
                0,
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
            return Err(crate::Error::Gpu(format!("framebuffer incomplete ({status:#x})")));
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

    /// Reads the bottom-left `width` × `height` of the framebuffer converted to I420 on the GPU
    /// (packed planes, as [`crate::PixelFormat::I420`] describes), `flip` as for [`Self::read`].
    /// Err, with why, when the GPU cannot: the pass is set up and checked on the first call, and
    /// a failure is kept, so every later call fails at once and the caller reads RGBA instead.
    pub fn read_i420(
        &mut self,
        width: u32,
        height: u32,
        flip: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), String> {
        let (w, h) = (width.min(self.width), height.min(self.height));
        self.make_current();
        if self.yuv.is_none() {
            let pass = yuv::YuvPass::create(self);
            self.make_current();
            match &pass {
                Ok(_) => tracing::info!("GPU frames are converted to I420 on the GPU"),
                Err(e) => {
                    tracing::warn!(error = %e, "GPU frames cannot be converted on the GPU; reading them back as RGBA")
                }
            }
            self.yuv = Some(pass);
        }
        let Some(Ok(pass)) = &mut self.yuv else {
            return Err(self
                .yuv
                .as_ref()
                .and_then(|p| p.as_ref().err())
                .cloned()
                .unwrap_or_default());
        };
        // SAFETY: the core's context is current. Its drawing must be done before the pass's
        // context samples the texture: commands in two contexts are not ordered otherwise.
        unsafe { (self.gl.finish)() };
        let result =
            pass.convert(&self.gl, self.color, (self.width, self.height), (w, h), flip, out);
        self.make_current();
        if let Err(e) = &result {
            tracing::warn!(error = %e, "converting a frame on the GPU failed; reading frames back as RGBA");
            self.yuv = Some(Err(e.clone()));
        }
        result
    }

    fn gles(&self) -> bool {
        self.api == EGL_OPENGL_ES_API
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
        // The pass's context goes first: it shares this one's objects.
        self.yuv = None;
        self.make_current();
        // SAFETY: the context is current; the names were generated on it; afterwards nothing
        // uses the context or display again.
        unsafe {
            (self.gl.delete_framebuffers)(1, &self.fbo);
            (self.gl.delete_textures)(1, &self.color);
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

/// Gives the bound `GL_TEXTURE_2D` RGBA8 storage of `w` × `h` (filled from `pixels`, or left
/// undefined if null), sampled texel for texel.
///
/// # Safety
///
/// A context is current with the texture bound; `pixels` is null or holds `w * h` RGBA pixels.
unsafe fn alloc_rgba_texture(gl: &Gl, gles: bool, w: i32, h: i32, pixels: *const c_void) {
    // OpenGL ES 2 takes no sized internal format; ES 3 and desktop GL make RGBA8 of either.
    let internal = if gles { GL_RGBA } else { GL_RGBA8 } as i32;
    // SAFETY: the caller guarantees the context, the binding and the pixels.
    unsafe {
        (gl.tex_image_2d)(GL_TEXTURE_2D, 0, internal, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
        (gl.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
        (gl.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
        (gl.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        (gl.tex_parameter_i)(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
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
pub fn probe() -> Result<Vec<GpuReport>, crate::Error> {
    let egl = egl().map_err(crate::Error::EglUnavailable)?;
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

    /// `SovEmu.Media`'s RGBA → I420 (BT.601 limited range, 2×2 chroma averages), packed.
    fn reference_i420(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            (i32::from(rgba[i]), i32::from(rgba[i + 1]), i32::from(rgba[i + 2]))
        };
        let mut out: Vec<u8> = (0..w * h)
            .map(|i| {
                let (r, g, b) = px(i % w, i / w);
                (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
            })
            .collect();
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let (mut u, mut v) = (Vec::new(), Vec::new());
        for cy in 0..ch {
            for cx in 0..cw {
                let (x0, y0) = (cx * 2, cy * 2);
                let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
                let (mut r, mut g, mut b) = (0, 0, 0);
                for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                    let (pr, pg, pb) = px(x, y);
                    (r, g, b) = (r + pr, g + pg, b + pb);
                }
                let (r, g, b) = ((r + 2) / 4, (g + 2) / 4, (b + 2) / 4);
                u.push((((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8);
                v.push((((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8);
            }
        }
        out.extend(u);
        out.extend(v);
        out
    }

    /// Needs libEGL with a working device (llvmpipe will do); elsewhere it says so and passes.
    #[test]
    fn converts_to_i420_on_the_gpu_exactly_as_the_cpu_does() {
        if !available() {
            eprintln!("skipped: no libEGL here");
            return;
        }
        let requests =
            [(GlApi::Gl, 0, 0), (GlApi::GlCore, 3, 3), (GlApi::Gles, 2, 0), (GlApi::Gles, 3, 0)];
        for (api, major, minor) in requests {
            let request = ContextRequest { api, major, minor, depth: true, stencil: false };
            let mut ctx = GlContext::create(&GpuChoice::Software, request, 64, 48)
                .unwrap_or_else(|e| panic!("{api:?} {major}.{minor}: {e}"));
            // Every channel takes values all over 0..=255: no two neighbours alike.
            let noise: Vec<u8> =
                (0..64 * 48 * 4u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
            ctx.make_current();
            // SAFETY: the context is current; `noise` fills the framebuffer's 64 × 48.
            unsafe {
                (ctx.gl.bind_texture)(GL_TEXTURE_2D, ctx.color);
                alloc_rgba_texture(&ctx.gl, ctx.gles(), 64, 48, noise.as_ptr().cast());
                (ctx.gl.bind_texture)(GL_TEXTURE_2D, 0);
            }
            for (w, h) in [(64, 48), (37, 23), (1, 1), (9, 2)] {
                for flip in [false, true] {
                    let mut rgba = Vec::new();
                    ctx.read(w, h, flip, &mut rgba);
                    let mut i420 = Vec::new();
                    ctx.read_i420(w, h, flip, &mut i420)
                        .unwrap_or_else(|e| panic!("{api:?} {major}.{minor}: {e}"));
                    let expected = reference_i420(&rgba, w as usize, h as usize);
                    assert!(
                        i420 == expected,
                        "{api:?} {major}.{minor}, {w}×{h}, flip {flip}: {} of {} bytes differ",
                        i420.iter().zip(&expected).filter(|(a, b)| a != b).count(),
                        expected.len()
                    );
                }
            }
        }
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
