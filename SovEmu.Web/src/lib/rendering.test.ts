import { describe, expect, it } from "vitest";
import type { GpuDevice, GpuStatus } from "./api";
import { deviceName, deviceState, probedText, verdict } from "./rendering";

const device = (over: Partial<GpuDevice>): GpuDevice => ({
  node: "/dev/dri/renderD128",
  driver: "amdgpu",
  kind: "hardware",
  vendor: "AMD",
  renderer: "AMD Radeon RX 6600",
  version: "4.6",
  works: true,
  error: null,
  chosen: false,
  ...over,
});

const status = (over: Partial<GpuStatus>): GpuStatus => ({
  setting: "auto",
  mode: "GPU",
  renderer: "AMD Radeon RX 6600",
  note: null,
  workerArg: "/dev/dri/renderD128",
  probedAt: 1_700_000_000,
  devices: [],
  ...over,
});

describe("the rendering page", () => {
  it("says where 3D systems render", () => {
    expect(verdict(status({})).tone).toBe("good");
    expect(verdict(status({ mode: "SOFTWARE", renderer: "llvmpipe (LLVM 19)" })).detail).toContain(
      "llvmpipe (LLVM 19)",
    );
    expect(verdict(status({ mode: "OFF", renderer: null })).title).toBe("3D systems are off");
  });

  it("names and judges each device", () => {
    expect(deviceName(device({}))).toBe("/dev/dri/renderD128");
    expect(deviceName(device({ node: null, kind: "software" }))).toBe("Software renderer");
    expect(deviceName(device({ node: null }))).toBe("GPU without a render node");
    expect(deviceState(device({ chosen: true })).label).toBe("In use");
    expect(deviceState(device({})).label).toBe("Works");
    expect(deviceState(device({ works: false, error: "no EGL" }))).toMatchObject({ label: "Failed", detail: "no EGL" });
  });

  it("says when the probe ran", () => {
    expect(probedText(null)).toContain("SOVEMU_GPU is off");
    expect(probedText(1_700_000_000)).toContain("needs a restart");
  });
});
