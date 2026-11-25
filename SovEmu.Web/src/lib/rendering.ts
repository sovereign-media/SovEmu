// The admin Rendering page's words: the server's GPU decision and each probed device, said
// plainly. No React, so Vitest can check them.

import type { GpuDevice, GpuStatus } from "./api";

export type Tone = "good" | "warn" | "off";

/** The verdict at the top of the page. */
export function verdict(gpu: GpuStatus): { tone: Tone; title: string; detail: string } {
  switch (gpu.mode) {
    case "GPU":
      return {
        tone: "good",
        title: "3D systems render on the GPU",
        detail: gpu.renderer ?? gpu.workerArg,
      };
    case "SOFTWARE":
      return {
        tone: "warn",
        title: "3D systems render in software",
        detail: `${gpu.renderer ?? "llvmpipe"}: playable but slow. Pass a GPU through to the container for full speed.`,
      };
    default:
      return {
        tone: "off",
        title: "3D systems are off",
        detail: "No renderer works, so N64, Dreamcast, GameCube and PS2 games cannot start.",
      };
  }
}

/** A device's name: its render node, or what it is. */
export function deviceName(d: GpuDevice): string {
  if (d.node) return d.node;
  return d.kind === "software" ? "Software renderer" : "GPU without a render node";
}

/** A device's state in a word, and why. */
export function deviceState(d: GpuDevice): { tone: Tone; label: string; detail: string | null } {
  if (d.chosen) return { tone: "good", label: "In use", detail: d.renderer };
  if (d.works) return { tone: "off", label: "Works", detail: d.renderer };
  return { tone: "warn", label: "Failed", detail: d.error ?? "It did not draw a frame." };
}

/** When the probe ran, as a local date and time. */
export function probedText(probedAt: number | null): string {
  if (probedAt == null) return "Not probed: SovEmu__Rendering__Gpu is off.";
  const when = new Date(probedAt * 1000).toLocaleString("en-GB", { dateStyle: "medium", timeStyle: "short" });
  return `Probed at startup, ${when}. A GPU added since needs a restart.`;
}
