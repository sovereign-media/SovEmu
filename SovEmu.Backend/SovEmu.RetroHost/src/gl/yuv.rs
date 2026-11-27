//! RGBA → I420 on the GPU, so a GPU core's frame comes back already converted.
//!
//! A fragment shader draws the frame into one RGBA8 target, each output texel holding four
//! bytes of a plane: the first `height` rows are luma, four pixels a texel; the next
//! ⌈height/2⌉ rows hold a row of U in their left half and the same row of V in their right.
//! One `glReadPixels` brings that back (1.5 bytes a pixel, against RGBA's 4) and the rows are
//! packed into the planes. The flip a bottom-left-origin core needs is done in the shader too.
//!
//! The arithmetic is `SovEmu.Media`'s BT.601 integer formula, done exactly in floats (every
//! value is an integer below 2²⁴ and every division is by a power of two or 4), so the planes
//! match the CPU's conversion byte for byte. Chroma is the average of each 2×2 block, with the
//! last column or row averaged with itself at an odd size, as on the CPU.
//!
//! The pass runs in its own context, sharing the core's objects: the core's GL state is never
//! touched, as a core that asked for `RETRO_ENVIRONMENT_SET_HW_SHARED_CONTEXT` is promised. It
//! is checked on a known picture when it is made, so a driver that draws it wrong (no `highp`
//! in fragment shaders) is caught before a frame is shown, and the frames are read as RGBA.

use std::ffi::{CStr, c_char, c_void};

use super::{
    EGL_CONTEXT_MAJOR_VERSION, EGL_NONE, Egl, EglContext, EglDisplay, GL_COLOR_ATTACHMENT0,
    GL_FRAMEBUFFER, GL_FRAMEBUFFER_COMPLETE, GL_PACK_ALIGNMENT, GL_RGBA, GL_TEXTURE_2D,
    GL_UNSIGNED_BYTE, Gl, GlContext, alloc_rgba_texture, proc_address_with,
};

const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_STATIC_DRAW: u32 = 0x88E4;
const GL_FLOAT: u32 = 0x1406;
const GL_TRIANGLES: u32 = 0x0004;
const GL_TEXTURE0: u32 = 0x84C0;

gl_fns! {
    /// What the pass needs beyond [`Gl`]: shaders, a vertex buffer, uniforms.
    struct PassGl {
    create_shader: "glCreateShader" => fn(u32) -> u32;
    shader_source: "glShaderSource" => fn(u32, i32, *const *const c_char, *const i32);
    compile_shader: "glCompileShader" => fn(u32);
    get_shader_iv: "glGetShaderiv" => fn(u32, u32, *mut i32);
    get_shader_info_log: "glGetShaderInfoLog" => fn(u32, i32, *mut i32, *mut c_char);
    delete_shader: "glDeleteShader" => fn(u32);
    create_program: "glCreateProgram" => fn() -> u32;
    attach_shader: "glAttachShader" => fn(u32, u32);
    bind_attrib_location: "glBindAttribLocation" => fn(u32, u32, *const c_char);
    link_program: "glLinkProgram" => fn(u32);
    get_program_iv: "glGetProgramiv" => fn(u32, u32, *mut i32);
    get_program_info_log: "glGetProgramInfoLog" => fn(u32, i32, *mut i32, *mut c_char);
    use_program: "glUseProgram" => fn(u32);
    delete_program: "glDeleteProgram" => fn(u32);
    get_uniform_location: "glGetUniformLocation" => fn(u32, *const c_char) -> i32;
    uniform_1i: "glUniform1i" => fn(i32, i32);
    uniform_1f: "glUniform1f" => fn(i32, f32);
    uniform_2f: "glUniform2f" => fn(i32, f32, f32);
    gen_buffers: "glGenBuffers" => fn(i32, *mut u32);
    bind_buffer: "glBindBuffer" => fn(u32, u32);
    buffer_data: "glBufferData" => fn(u32, isize, *const c_void, u32);
    delete_buffers: "glDeleteBuffers" => fn(i32, *const u32);
    vertex_attrib_pointer: "glVertexAttribPointer" => fn(u32, i32, u32, u8, i32, *const c_void);
    enable_vertex_attrib_array: "glEnableVertexAttribArray" => fn(u32);
    draw_arrays: "glDrawArrays" => fn(u32, i32, i32);
    active_texture: "glActiveTexture" => fn(u32);
    }
}

const VERTEX: &str = "
attribute vec2 pos;
void main() { gl_Position = vec4(pos, 0.0, 1.0); }
";

/// GLSL ES 1.00 and GLSL 1.20 alike, after the version line.
const FRAGMENT: &str = "
uniform sampler2D src;
uniform vec2 src_size;
uniform vec2 frame;
uniform float flip;
uniform float half_width;

vec3 rgb(float x, float y) {
    x = min(x, frame.x - 1.0);
    y = min(y, frame.y - 1.0);
    float row = flip > 0.5 ? frame.y - 1.0 - y : y;
    vec3 c = texture2D(src, vec2(x + 0.5, row + 0.5) / src_size).rgb;
    return floor(c * 255.0 + 0.5);
}

float luma(vec3 c) {
    return floor((66.0 * c.r + 129.0 * c.g + 25.0 * c.b + 128.0) / 256.0) + 16.0;
}

float chroma(vec3 c, bool v) {
    float x = v ? 112.0 * c.r - 94.0 * c.g - 18.0 * c.b : -38.0 * c.r - 74.0 * c.g + 112.0 * c.b;
    return clamp(floor((x + 128.0) / 256.0) + 128.0, 0.0, 255.0);
}

float block(float cx, float cy, bool v) {
    float x0 = 2.0 * cx;
    float y0 = 2.0 * cy;
    float x1 = min(x0 + 1.0, frame.x - 1.0);
    float y1 = min(y0 + 1.0, frame.y - 1.0);
    vec3 sum = rgb(x0, y0) + rgb(x1, y0) + rgb(x0, y1) + rgb(x1, y1);
    return chroma(floor((sum + 2.0) / 4.0), v);
}

void main() {
    vec2 p = floor(gl_FragCoord.xy);
    vec4 o;
    if (p.y < frame.y) {
        float x = 4.0 * p.x;
        o = vec4(luma(rgb(x, p.y)), luma(rgb(x + 1.0, p.y)),
                 luma(rgb(x + 2.0, p.y)), luma(rgb(x + 3.0, p.y)));
    } else {
        bool v = p.x >= half_width;
        float x = 4.0 * (v ? p.x - half_width : p.x);
        float y = p.y - frame.y;
        o = vec4(block(x, y, v), block(x + 1.0, y, v), block(x + 2.0, y, v), block(x + 3.0, y, v));
    }
    gl_FragColor = o / 255.0;
}
";

/// The pass: its own context (sharing the core's), program and target.
pub(super) struct YuvPass {
    egl: &'static Egl,
    display: EglDisplay,
    context: EglContext,
    gles: bool,
    f: PassGl,
    program: u32,
    vbo: u32,
    fbo: u32,
    target: u32,
    target_size: (u32, u32),
    uniforms: Uniforms,
    /// The target as read back, before its rows are packed into planes.
    rows: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct Uniforms {
    src_size: i32,
    frame: i32,
    flip: i32,
    half_width: i32,
}

impl std::fmt::Debug for YuvPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuvPass").field("target_size", &self.target_size).finish()
    }
}

/// The packed I420 size of a `w` × `h` frame.
fn i420_len(w: usize, h: usize) -> usize {
    w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
}

impl YuvPass {
    /// Makes the pass beside `core`'s context and checks it on a known picture. Leaves the
    /// pass's context current (or none): the caller makes the core's current again.
    pub(super) fn create(core: &GlContext) -> Result<Self, String> {
        let egl = core.egl;
        let gles = core.gles();
        // SAFETY: the display and the core's context are live; the attribute list ends with
        // EGL_NONE; the new context is destroyed by `Drop` from here on.
        let context = unsafe {
            if (egl.f.bind_api)(core.api) == 0 {
                return Err(format!("eglBindAPI failed ({:#x})", (egl.f.get_error)()));
            }
            // OpenGL ES 2 is all the shader needs; desktop GL's default is a compatibility
            // context, which takes GLSL 1.20 whatever profile the core's is.
            let attrs: &[i32] =
                if gles { &[EGL_CONTEXT_MAJOR_VERSION, 2, EGL_NONE] } else { &[EGL_NONE] };
            let context =
                (egl.f.create_context)(core.display, core.config, core.context, attrs.as_ptr());
            if context.is_null() {
                return Err(format!("no context sharing the core's ({:#x})", (egl.f.get_error)()));
            }
            if (egl.f.make_current)(
                core.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                context,
            ) == 0
            {
                (egl.f.destroy_context)(core.display, context);
                return Err(format!("eglMakeCurrent failed ({:#x})", (egl.f.get_error)()));
            }
            context
        };
        let f = match PassGl::load(egl) {
            Ok(f) => f,
            Err(e) => {
                // SAFETY: the context was made above and nothing else refers to it.
                unsafe {
                    (egl.f.make_current)(
                        core.display,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    (egl.f.destroy_context)(core.display, context);
                }
                return Err(e);
            }
        };
        let mut pass = Self {
            egl,
            display: core.display,
            context,
            gles,
            f,
            program: 0,
            vbo: 0,
            fbo: 0,
            target: 0,
            target_size: (0, 0),
            uniforms: Uniforms { src_size: -1, frame: -1, flip: -1, half_width: -1 },
            rows: Vec::new(),
        };
        pass.build(&core.gl)?;
        pass.check(&core.gl)?;
        Ok(pass)
    }

    /// Compiles the program and sets up the triangle that covers the target.
    fn build(&mut self, gl: &Gl) -> Result<(), String> {
        let version = if self.gles {
            "#version 100\n#ifdef GL_FRAGMENT_PRECISION_HIGH\nprecision highp float;\n#else\n\
             precision mediump float;\n#endif\n"
        } else {
            "#version 120\n"
        };
        let f = &self.f;
        // SAFETY: the pass's context is current; sources are passed with their lengths;
        // out-parameters point to locals.
        unsafe {
            let vertex = compile(f, GL_VERTEX_SHADER, &[version, VERTEX])?;
            let fragment = match compile(f, GL_FRAGMENT_SHADER, &[version, FRAGMENT]) {
                Ok(s) => s,
                Err(e) => {
                    (f.delete_shader)(vertex);
                    return Err(e);
                }
            };
            self.program = (f.create_program)();
            (f.attach_shader)(self.program, vertex);
            (f.attach_shader)(self.program, fragment);
            (f.bind_attrib_location)(self.program, 0, c"pos".as_ptr());
            (f.link_program)(self.program);
            (f.delete_shader)(vertex);
            (f.delete_shader)(fragment);
            let mut ok = 0;
            (f.get_program_iv)(self.program, GL_LINK_STATUS, &mut ok);
            if ok == 0 {
                return Err(format!(
                    "the I420 shader does not link: {}",
                    program_log(f, self.program)
                ));
            }
            (f.use_program)(self.program);
            let at = |name: &CStr| (f.get_uniform_location)(self.program, name.as_ptr());
            (f.uniform_1i)(at(c"src"), 0);
            self.uniforms = Uniforms {
                src_size: at(c"src_size"),
                frame: at(c"frame"),
                flip: at(c"flip"),
                half_width: at(c"half_width"),
            };

            // One triangle past every edge covers the viewport with no seam.
            let triangle: [f32; 6] = [-1.0, -1.0, 3.0, -1.0, -1.0, 3.0];
            (f.gen_buffers)(1, &mut self.vbo);
            (f.bind_buffer)(GL_ARRAY_BUFFER, self.vbo);
            (f.buffer_data)(
                GL_ARRAY_BUFFER,
                size_of_val(&triangle) as isize,
                triangle.as_ptr().cast(),
                GL_STATIC_DRAW,
            );
            (f.vertex_attrib_pointer)(0, 2, GL_FLOAT, 0, 0, std::ptr::null());
            (f.enable_vertex_attrib_array)(0);
            (f.active_texture)(GL_TEXTURE0);

            (gl.gen_framebuffers)(1, &mut self.fbo);
            (gl.gen_textures)(1, &mut self.target);
        }
        Ok(())
    }

    /// Converts a picture whose answer is known: black, white, red and blue 2×2 blocks.
    fn check(&mut self, gl: &Gl) -> Result<(), String> {
        let colours: [[u8; 4]; 4] =
            [[0, 0, 0, 255], [255, 255, 255, 255], [255, 0, 0, 255], [0, 0, 255, 255]];
        let rgba: Vec<u8> =
            (0..2).flat_map(|_| colours.iter().flat_map(|c| [*c, *c]).flatten()).collect();
        let mut texture = 0;
        // SAFETY: the pass's context is current; `rgba` holds 8 × 2 RGBA pixels.
        unsafe {
            (gl.gen_textures)(1, &mut texture);
            (gl.bind_texture)(GL_TEXTURE_2D, texture);
            alloc_rgba_texture(gl, self.gles, 8, 2, rgba.as_ptr().cast());
        }
        let mut out = Vec::new();
        let result = self.convert(gl, texture, (8, 2), (8, 2), false, &mut out);
        // SAFETY: the texture was made above, in this context.
        unsafe { (gl.delete_textures)(1, &texture) };
        result?;
        let luma = [16, 16, 235, 235, 82, 82, 41, 41];
        let expected: Vec<u8> =
            [&luma[..], &luma, &[128, 128, 90, 240], &[128, 128, 240, 110]].concat();
        if out == expected {
            Ok(())
        } else {
            Err(format!("the I420 pass drew {out:?}, not {expected:?}"))
        }
    }

    /// Converts the bottom-left `frame` of texture `src` (`src_size`, its allocated size) into
    /// packed I420 in `out`. Makes the pass's context current and leaves it so.
    pub(super) fn convert(
        &mut self,
        gl: &Gl,
        src: u32,
        src_size: (u32, u32),
        (w, h): (u32, u32),
        flip: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), String> {
        let half = w.div_ceil(8);
        let (tw, th) = (2 * half, h + h.div_ceil(2));
        let f = &self.f;
        // SAFETY: the display and the pass's context are live; the names were generated in
        // it (or are shared with it: `src`); `rows` holds `tw * th` RGBA pixels when read.
        unsafe {
            (self.egl.f.make_current)(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                self.context,
            );
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            if self.target_size.0 < tw || self.target_size.1 < th {
                let size = (tw.max(self.target_size.0), th.max(self.target_size.1));
                (gl.bind_texture)(GL_TEXTURE_2D, self.target);
                alloc_rgba_texture(gl, self.gles, size.0 as i32, size.1 as i32, std::ptr::null());
                (gl.framebuffer_texture_2d)(
                    GL_FRAMEBUFFER,
                    GL_COLOR_ATTACHMENT0,
                    GL_TEXTURE_2D,
                    self.target,
                    0,
                );
                let status = (gl.check_framebuffer_status)(GL_FRAMEBUFFER);
                if status != GL_FRAMEBUFFER_COMPLETE {
                    return Err(format!("the I420 target is incomplete ({status:#x})"));
                }
                self.target_size = size;
            }
            (gl.viewport)(0, 0, tw as i32, th as i32);
            (gl.bind_texture)(GL_TEXTURE_2D, src);
            let u = self.uniforms;
            (f.uniform_2f)(u.src_size, src_size.0 as f32, src_size.1 as f32);
            (f.uniform_2f)(u.frame, w as f32, h as f32);
            (f.uniform_1f)(u.flip, if flip { 1.0 } else { 0.0 });
            (f.uniform_1f)(u.half_width, half as f32);
            (f.draw_arrays)(GL_TRIANGLES, 0, 3);
            self.rows.resize(tw as usize * th as usize * 4, 0);
            (gl.pixel_store_i)(GL_PACK_ALIGNMENT, 4);
            (gl.read_pixels)(
                0,
                0,
                tw as i32,
                th as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                self.rows.as_mut_ptr().cast(),
            );
            (gl.bind_texture)(GL_TEXTURE_2D, 0);
        }
        pack(&self.rows, tw as usize * 4, w as usize, h as usize, out);
        Ok(())
    }
}

/// Packs the target's rows (`stride` bytes each) into I420 planes: luma rows first, then each
/// chroma row's U half, then its V half.
fn pack(rows: &[u8], stride: usize, w: usize, h: usize, out: &mut Vec<u8>) {
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let v_at = stride / 2;
    out.clear();
    out.reserve(i420_len(w, h));
    for row in rows.chunks_exact(stride).take(h) {
        out.extend_from_slice(&row[..w]);
    }
    let chroma = || rows.chunks_exact(stride).skip(h).take(ch);
    for row in chroma() {
        out.extend_from_slice(&row[..cw]);
    }
    for row in chroma() {
        out.extend_from_slice(&row[v_at..v_at + cw]);
    }
}

/// # Safety
///
/// A context is current that `f` was loaded for.
unsafe fn compile(f: &PassGl, kind: u32, parts: &[&str]) -> Result<u32, String> {
    let pointers: Vec<*const c_char> = parts.iter().map(|p| p.as_ptr().cast()).collect();
    let lengths: Vec<i32> = parts.iter().map(|p| p.len() as i32).collect();
    // SAFETY: the caller guarantees the context; each source comes with its length.
    unsafe {
        let shader = (f.create_shader)(kind);
        (f.shader_source)(shader, parts.len() as i32, pointers.as_ptr(), lengths.as_ptr());
        (f.compile_shader)(shader);
        let mut ok = 0;
        (f.get_shader_iv)(shader, GL_COMPILE_STATUS, &mut ok);
        if ok != 0 {
            return Ok(shader);
        }
        let mut log = vec![0u8; 1024];
        let mut len = 0;
        (f.get_shader_info_log)(shader, log.len() as i32, &mut len, log.as_mut_ptr().cast());
        (f.delete_shader)(shader);
        log.truncate(len.clamp(0, 1024) as usize);
        Err(format!("the I420 shader does not compile: {}", String::from_utf8_lossy(&log).trim()))
    }
}

/// # Safety
///
/// A context is current that `f` was loaded for, and `program` is a program made in it.
unsafe fn program_log(f: &PassGl, program: u32) -> String {
    let mut log = vec![0u8; 1024];
    let mut len = 0;
    // SAFETY: the caller guarantees the context and program; the buffer's length is passed.
    unsafe {
        (f.get_program_info_log)(program, log.len() as i32, &mut len, log.as_mut_ptr().cast())
    };
    log.truncate(len.clamp(0, 1024) as usize);
    String::from_utf8_lossy(&log).trim().to_owned()
}

impl Drop for YuvPass {
    fn drop(&mut self) {
        let gl = Gl::load(self.egl).ok();
        // SAFETY: the display and context are live; the names were generated in the context;
        // afterwards nothing uses the context again.
        unsafe {
            let none = std::ptr::null_mut();
            (self.egl.f.make_current)(self.display, none, none, self.context);
            if self.program != 0 {
                (self.f.delete_program)(self.program);
            }
            if self.vbo != 0 {
                (self.f.delete_buffers)(1, &self.vbo);
            }
            if let Some(gl) = gl {
                (gl.delete_framebuffers)(1, &self.fbo);
                (gl.delete_textures)(1, &self.target);
            }
            (self.egl.f.make_current)(self.display, none, none, none);
            (self.egl.f.destroy_context)(self.display, self.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_rows_into_planes() {
        // A 6 × 3 frame: half = 1 texel, so rows of 2 texels (8 bytes); 3 luma rows, 2 chroma.
        let (w, h, stride) = (6, 3, 8);
        let mut rows = Vec::new();
        for y in 0..3u8 {
            rows.extend((0..8).map(|x| 10 * y + x));
        }
        for y in 0..2u8 {
            rows.extend((0..8).map(|x| 100 + 10 * y + x));
        }
        let mut out = Vec::new();
        pack(&rows, stride, w, h, &mut out);
        assert_eq!(out.len(), i420_len(w, h));
        let y: Vec<u8> = (0..3).flat_map(|r| (0..6).map(move |x| 10 * r + x)).collect();
        assert_eq!(&out[..18], &y[..]);
        assert_eq!(&out[18..24], &[100, 101, 102, 110, 111, 112], "U: each row's left half");
        assert_eq!(&out[24..], &[104, 105, 106, 114, 115, 116], "V: its right half");
    }
}
