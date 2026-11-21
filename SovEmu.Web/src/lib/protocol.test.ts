import { describe, expect, it } from "vitest";
import { bit, decodeAck, encodeAnalog, encodeHotkeys, encodeInput, encodeStats } from "./protocol";

// The same vectors as `input_layout_is_stable` in SovEmu.Protocol/src/input.rs.
describe("input protocol", () => {
  it("encodes input exactly like the Rust reference", () => {
    const bytes = new Uint8Array(encodeInput({ seq: 0x04030201, clientTimeMs: 0x08070605, ports: [0x0a09] }));
    expect([...bytes]).toEqual([0x01, 1, 1, 2, 3, 4, 5, 6, 7, 8, 1, 0x09, 0x0a]);
  });

  it("encodes stats exactly like the Rust reference", () => {
    // `stats_are_json_after_the_header`: only the values known, in the Rust struct's order.
    const bytes = new Uint8Array(encodeStats({ packets_lost: 2, jitter_ms: 1.5, fps: NaN, kbps: undefined }));
    expect([...bytes.slice(0, 2)]).toEqual([0x04, 1]);
    expect(new TextDecoder().decode(bytes.slice(2))).toBe('{"jitter_ms":1.5,"packets_lost":2}');
  });

  it("encodes hotkeys exactly like the Rust reference", () => {
    expect([...new Uint8Array(encodeHotkeys({ fastForward: true, rewind: true }))]).toEqual([0x02, 1, 0b11]);
    expect([...new Uint8Array(encodeHotkeys({ fastForward: true, rewind: false }))]).toEqual([0x02, 1, 0b01]);
    expect([...new Uint8Array(encodeHotkeys({ fastForward: false, rewind: true }))]).toEqual([0x02, 1, 0b10]);
    expect([...new Uint8Array(encodeHotkeys({ fastForward: false, rewind: false, pause: true }))]).toEqual([
      0x02, 1, 0b100,
    ]);
  });

  it("encodes analog state exactly like the Rust reference", () => {
    // `analog_layout_is_stable_and_round_trips` in SovEmu.Protocol/src/input.rs.
    const bytes = new Uint8Array(
      encodeAnalog({ seq: 0x04030201, ports: [{ lx: -32768, ly: 32767, rx: 1, ry: -2, l2: 0, r2: 0x1234 }] }),
    );
    expect([...bytes]).toEqual([0x03, 1, 1, 2, 3, 4, 1, 0x00, 0x80, 0xff, 0x7f, 1, 0, 0xfe, 0xff, 0, 0, 0x34, 0x12]);
    // Out-of-range values clamp rather than wrap.
    const clamped = new DataView(
      encodeAnalog({ seq: 0, ports: [{ lx: 40000, ly: -40000, rx: 0, ry: 0, l2: 0, r2: 0 }] }),
    );
    expect([clamped.getInt16(7, true), clamped.getInt16(9, true)]).toEqual([32767, -32768]);
  });

  it("decodes the Rust reference ack", () => {
    const ack = new Uint8Array([0x81, 1, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
    expect(decodeAck(ack.buffer)).toEqual({ seq: 1, clientTimeMs: 2, frame: 3 });
  });

  it("rejects other messages", () => {
    expect(decodeAck(new Uint8Array([0x81, 1]).buffer)).toBeNull();
    expect(decodeAck(new Uint8Array([0x01, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).buffer)).toBeNull();
    expect(decodeAck(new Uint8Array([0x81, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).buffer)).toBeNull();
  });

  it("wraps sequence numbers and caps ports at four", () => {
    const bytes = new Uint8Array(encodeInput({ seq: 2 ** 32, clientTimeMs: 0, ports: [1, 2, 3, 4, 5] }));
    expect(bytes.slice(2, 6)).toEqual(new Uint8Array([0, 0, 0, 0]));
    expect(bytes[10]).toBe(4);
    expect(bytes.length).toBe(19);
  });

  it("numbers buttons as libretro does", () => {
    expect(bit("B")).toBe(1);
    expect(bit("START")).toBe(1 << 3);
    expect(bit("A")).toBe(1 << 8);
  });
});
