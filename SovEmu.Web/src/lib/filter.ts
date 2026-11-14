// Display filters, drawn on a WebGL canvas over the (invisible) video element: the video keeps
// decoding and playing audio, and each new frame is uploaded as a texture.
//
// - sharp: nearest-neighbour, crisp pixels (the default)
// - smooth: bilinear
// - crt: scanlines between source rows and a faint aperture-grille mask

export type FilterMode = "sharp" | "smooth" | "crt";
export const FILTERS: { mode: FilterMode; label: string }[] = [
  { mode: "sharp", label: "Sharp pixels" },
  { mode: "smooth", label: "Smooth" },
  { mode: "crt", label: "CRT" },
];

const VERTEX = `
attribute vec2 pos;
varying vec2 uv;
void main() {
  uv = vec2(pos.x * 0.5 + 0.5, 0.5 - pos.y * 0.5);
  gl_Position = vec4(pos, 0.0, 1.0);
}`;

const FRAGMENT = `
precision mediump float;
uniform sampler2D frame;
uniform vec2 sourceSize;
uniform vec2 outputSize;
uniform int crt;
varying vec2 uv;
void main() {
  vec4 color = texture2D(frame, uv);
  if (crt == 1) {
    // Darken between source rows: brightest at a row's centre.
    float row = fract(uv.y * sourceSize.y);
    float scan = 0.55 + 0.45 * pow(sin(row * 3.14159), 0.8);
    // Aperture grille: every output column favours one of R, G, B.
    float column = mod(floor(uv.x * outputSize.x), 3.0);
    vec3 mask = vec3(0.85);
    if (column < 1.0) mask.r = 1.0; else if (column < 2.0) mask.g = 1.0; else mask.b = 1.0;
    color.rgb = color.rgb * scan * mask * 1.25;
  }
  gl_FragColor = vec4(color.rgb, 1.0);
}`;

export class VideoFilter {
  private gl: WebGLRenderingContext | null;
  private texture: WebGLTexture | null = null;
  private uniforms: Record<string, WebGLUniformLocation | null> = {};
  private mode: FilterMode = "sharp";
  private running = false;

  constructor(private readonly video: HTMLVideoElement, private readonly canvas: HTMLCanvasElement) {
    this.gl = canvas.getContext("webgl", { alpha: false, antialias: false, desynchronized: true });
    if (this.gl) this.setup(this.gl);
  }

  /** Whether WebGL is available (if not, show the video element itself). */
  get supported(): boolean {
    return this.gl !== null;
  }

  setMode(mode: FilterMode) {
    this.mode = mode;
    const gl = this.gl;
    if (!gl || !this.texture) return;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    const filter = mode === "sharp" ? gl.NEAREST : gl.LINEAR;
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
  }

  start() {
    if (!this.gl || this.running) return;
    this.running = true;
    const video = this.video as HTMLVideoElement & {
      requestVideoFrameCallback?: (cb: () => void) => number;
    };
    const draw = () => {
      if (!this.running) return;
      this.draw();
      if (video.requestVideoFrameCallback) video.requestVideoFrameCallback(draw);
      else requestAnimationFrame(draw);
    };
    draw();
  }

  stop() {
    this.running = false;
  }

  private setup(gl: WebGLRenderingContext) {
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      return s;
    };
    const program = gl.createProgram()!;
    gl.attachShader(program, compile(gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(program);
    gl.useProgram(program);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const pos = gl.getAttribLocation(program, "pos");
    gl.enableVertexAttribArray(pos);
    gl.vertexAttribPointer(pos, 2, gl.FLOAT, false, 0, 0);
    for (const name of ["frame", "sourceSize", "outputSize", "crt"]) {
      this.uniforms[name] = gl.getUniformLocation(program, name);
    }
    this.texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    this.setMode(this.mode);
  }

  private draw() {
    const gl = this.gl;
    const { videoWidth: vw, videoHeight: vh } = this.video;
    if (!gl || !vw || !vh) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(this.canvas.clientWidth * dpr);
    const h = Math.round(this.canvas.clientHeight * dpr);
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    gl.viewport(0, 0, w, h);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB, gl.RGB, gl.UNSIGNED_BYTE, this.video);
    gl.uniform1i(this.uniforms.frame, 0);
    gl.uniform2f(this.uniforms.sourceSize, vw, vh);
    gl.uniform2f(this.uniforms.outputSize, w, h);
    gl.uniform1i(this.uniforms.crt, this.mode === "crt" ? 1 : 0);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }
}
