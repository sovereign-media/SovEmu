// On-screen controls for a phone or tablet with nothing else to play with, laid out as the
// Android app's (`TouchController` in Player.kt): the D-pad bottom left, X/Y/A/B bottom right,
// Select and Start bottom centre, L and R along the top, and the menu button top centre, the way
// out of a game on a phone. Each button is held while a finger is on it; several at once work,
// as each follows its own pointer.

import { Menu } from "lucide-react";
import { type CSSProperties, useEffect, useRef, useState } from "react";
import type { ButtonName } from "../../lib/protocol";
import { cn } from "../ui/cn";

type PadProps = {
  label: string;
  button: ButtonName;
  onButton: (button: ButtonName, down: boolean) => void;
  round?: boolean;
  width?: number;
  height?: number;
};

/**
 * A faint dark edge and a shadowed label, not on Android: Select, Start and the menu button sit
 * over the game, and Android's translucent white disappears over a white title screen.
 */
const EDGE = "ring-1 ring-black/25 [text-shadow:0_1px_2px_rgb(0_0_0/0.6)]";

/** One button: 18% white at rest and 45% while held, as on Android. */
function Pad({ label, button, onButton, round = false, width = 56, height = 56 }: PadProps) {
  const [pressed, setPressed] = useState(false);
  const held = useRef(false);
  const set = (down: boolean) => {
    if (held.current === down) return;
    held.current = down;
    setPressed(down);
    onButton(button, down);
  };
  // A button still held as the controls go (the menu opened, a pad connected) is let go.
  useEffect(
    () => () => {
      if (held.current) onButton(button, false);
    },
    [button, onButton],
  );
  const size: CSSProperties = { width, height };
  return (
    <button
      type="button"
      // For fingers: kept out of the tab order, so a keyboard's Tab never lands on it.
      tabIndex={-1}
      aria-label={label}
      style={size}
      className={cn(
        "grid touch-none place-items-center text-sm font-bold text-white/90 select-none",
        EDGE,
        round ? "rounded-full" : "rounded-[10px]",
        pressed ? "bg-white/45" : "bg-white/[0.18]",
      )}
      onPointerDown={(e) => {
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        set(true);
      }}
      onPointerUp={() => set(false)}
      onPointerCancel={() => set(false)}
      onLostPointerCapture={() => set(false)}
      onContextMenu={(e) => e.preventDefault()}
    >
      {label}
    </button>
  );
}

/** The empty middle of the D-pad's and the face buttons' crosses. */
const GAP = <div className="size-14" />;

type TouchControllerProps = {
  onButton: (button: ButtonName, down: boolean) => void;
  onMenu: () => void;
};

export function TouchController({ onButton, onMenu }: TouchControllerProps) {
  return (
    <div className="pointer-events-none absolute inset-0 z-10 p-6 select-none [&>*]:pointer-events-auto">
      <div className="absolute top-6 left-6">
        <Pad label="L" button="L" onButton={onButton} width={88} height={40} />
      </div>
      <div className="absolute top-6 right-6">
        <Pad label="R" button="R" onButton={onButton} width={88} height={40} />
      </div>
      <button
        type="button"
        aria-label="Menu"
        onClick={onMenu}
        className={cn(
          "absolute top-6 left-1/2 grid size-11 -translate-x-1/2 place-items-center rounded-full bg-white/[0.18] text-white/90",
          EDGE,
        )}
      >
        <Menu className="size-5 drop-shadow" />
      </button>
      <div className="absolute bottom-6 left-6 grid grid-cols-3 place-items-center">
        {GAP}
        <Pad label="▲" button="UP" onButton={onButton} />
        {GAP}
        <Pad label="◀" button="LEFT" onButton={onButton} />
        {GAP}
        <Pad label="▶" button="RIGHT" onButton={onButton} />
        {GAP}
        <Pad label="▼" button="DOWN" onButton={onButton} />
        {GAP}
      </div>
      <div className="absolute bottom-6 left-1/2 flex -translate-x-1/2 gap-4">
        <Pad label="Select" button="SELECT" onButton={onButton} width={72} height={32} />
        <Pad label="Start" button="START" onButton={onButton} width={72} height={32} />
      </div>
      <div className="absolute right-6 bottom-6 grid grid-cols-3 place-items-center">
        {GAP}
        <Pad label="X" button="X" onButton={onButton} round />
        {GAP}
        <Pad label="Y" button="Y" onButton={onButton} round />
        {GAP}
        <Pad label="A" button="A" onButton={onButton} round />
        {GAP}
        <Pad label="B" button="B" onButton={onButton} round />
        {GAP}
      </div>
    </div>
  );
}
