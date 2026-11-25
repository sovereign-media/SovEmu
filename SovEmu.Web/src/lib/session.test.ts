import { afterEach, describe, expect, it, vi } from "vitest";
import { iceGathered } from "./session";

/** A peer connection as far as gathering goes: its state, and the event that says it moved. */
function connection(state: RTCIceGatheringState) {
  const target = new EventTarget() as EventTarget & { iceGatheringState: RTCIceGatheringState };
  target.iceGatheringState = state;
  return {
    pc: target as unknown as RTCPeerConnection,
    finish() {
      target.iceGatheringState = "complete";
      target.dispatchEvent(new Event("icegatheringstatechange"));
    },
  };
}

describe("iceGathered", () => {
  afterEach(() => vi.useRealTimers());

  it("is done at once when gathering already is", async () => {
    await expect(iceGathered(connection("complete").pc)).resolves.toBeUndefined();
  });

  it("waits for gathering to finish, so the one offer carries every candidate", async () => {
    vi.useFakeTimers();
    const c = connection("gathering");
    let done = false;
    void iceGathered(c.pc).then(() => {
      done = true;
    });
    await vi.advanceTimersByTimeAsync(500);
    expect(done).toBe(false);
    c.finish();
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(true);
  });

  it("gives up waiting after 1.5 s and offers what it has", async () => {
    // A network with no STUN answer can gather for ever: the server answers one offer, so it
    // must go with what there is rather than never.
    vi.useFakeTimers();
    let done = false;
    void iceGathered(connection("gathering").pc).then(() => {
      done = true;
    });
    await vi.advanceTimersByTimeAsync(1499);
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(done).toBe(true);
  });
});
