// A steady tick that keeps going in a hidden tab (#302). Animation frames stop there, and a
// hidden page's own timers are held to about one a second, longer than the worker's half-second
// lapse on held input and hotkeys: the input heartbeat would stop, and a game paused under the
// menu would carry on unseen. A dedicated worker's timers are not held back, so the tick comes
// from one, by message; where workers are unavailable (tests, old browsers) it is a plain timer.

/** Calls `onTick` every `ms` until the returned function is called. */
export function startTicker(ms: number, onTick: () => void): () => void {
  if (typeof Worker === "undefined" || typeof Blob === "undefined" || typeof URL.createObjectURL !== "function") {
    const timer = setInterval(onTick, ms);
    return () => clearInterval(timer);
  }
  const source = `setInterval(() => postMessage(0), ${Math.max(1, Math.round(ms))});`;
  const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  let worker: Worker;
  try {
    worker = new Worker(url);
  } catch {
    URL.revokeObjectURL(url);
    const timer = setInterval(onTick, ms);
    return () => clearInterval(timer);
  }
  worker.onmessage = () => onTick();
  return () => {
    worker.terminate();
    URL.revokeObjectURL(url);
  };
}
