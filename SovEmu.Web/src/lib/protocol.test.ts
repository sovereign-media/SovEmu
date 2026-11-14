import { describe, expect, it } from "vitest";
import { bit, decodeAck, encodeHotkeys, encodeInput } from "./protocol";

// The same vectors as `input_layout_is_stable` in SovEmu.Protocol/src/input.rs.
describe("input protocol", () => {
  it("encodes input exactly like the Rust reference", () => {
    const bytes = new Uint8Array(encodeInput({ seq: 0x04030201, clientTimeMs: 0x08070605, ports: [0x0a09] }));
    expect([...bytes]).toEqual([0x01, 1, 1, 2, 3, 4, 5, 6, 7, 8, 1, 0x09, 0x0a]);
  });

  it("encodes hotkeys exactly like the Rust reference", () => {
    expect([...new Uint8Array(encodeHotkeys({ fastForward: true, rewind: true }))]).toEqual([0x02, 1, 0b11]);
    expect([...new Uint8Array(encodeHotkeys({ fastForward: true, rewind: false }))]).toEqual([0x02, 1, 0b01]);
    expect([...new Uint8Array(encodeHotkeys({ fastForward: false, rewind: true }))]).toEqual([0x02, 1, 0b10]);
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
