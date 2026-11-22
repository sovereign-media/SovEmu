import { describe, expect, it } from "vitest";
import { decoderLabel, pipelineItems, renderingLabel, streamLabel } from "./pipeline";
import type { Pipeline } from "./protocol";

const pipeline = (over: Partial<Pipeline> = {}): Pipeline => ({
  renderer: null,
  rendererSoftware: false,
  video: { codec: "h264", encoder: "vaapi", hardware: true, fallbacks: [], width: 256, height: 224, fps: 60 },
  audio: { codec: "opus", sourceHz: 32040.5, kbps: 128 },
  runAhead: 1,
  ...over,
});

describe("pipeline rows", () => {
  it("names the stream, rendering, audio and run-ahead", () => {
    expect(pipelineItems(pipeline())).toEqual([
      { label: "Stream", value: "H.264 · VA-API (GPU)" },
      { label: "Rendering", value: "CPU" },
      { label: "Audio", value: "Opus 128 kbps · from 32,041 Hz" },
      { label: "Run-ahead", value: "1 frame" },
    ]);
  });

  it("says what it fell back from, and why", () => {
    const fellBack = pipeline({
      video: {
        ...pipeline().video,
        encoder: "x264",
        hardware: false,
        fallbacks: [
          { encoder: "vaapi", reason: "holds 1 frame(s) back" },
          { encoder: "nvenc", reason: "no driver" },
        ],
      },
      runAhead: 0,
    });
    const items = pipelineItems(fellBack);
    expect(items[0].value).toBe("H.264 · x264 (CPU)");
    expect(items[1]).toEqual({ label: "Fell back from", value: "VA-API: holds 1 frame(s) back; NVENC: no driver" });
    expect(items.at(-1)?.value).toBe("Off");
  });

  it("labels GPU, software and CPU rendering, and VP8", () => {
    expect(renderingLabel("Mesa Intel(R) Iris(R) Xe", false)).toBe("GPU · Mesa Intel(R) Iris(R) Xe");
    expect(renderingLabel("llvmpipe (LLVM 19)", true)).toBe("Software · llvmpipe (LLVM 19)");
    expect(renderingLabel(null, false)).toBe("CPU");
    expect(streamLabel("vp8", "libvpx", false)).toBe("VP8 · libvpx (CPU)");
  });
});

describe("decoder", () => {
  it("says hardware or software when the browser does", () => {
    expect(decoderLabel("ExternalDecoder", true)).toBe("ExternalDecoder (hardware)");
    expect(decoderLabel("libvpx", false)).toBe("libvpx (software)");
    expect(decoderLabel("FFmpeg", undefined)).toBe("FFmpeg");
    expect(decoderLabel(undefined, true)).toBeNull();
  });
});
