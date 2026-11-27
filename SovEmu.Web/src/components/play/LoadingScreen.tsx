// The screen while a game starts (#243), the same in the Android and iOS apps: a darkened
// picture behind (the save being continued, the game, its box blurred, or the system's colour),
// the bar centred with what the start is doing, the game bottom left, and a spinner bottom
// right, which a failure replaces with the alert. The Continue / Start over choice is the
// player's menu, drawn over this.

import { CircleAlert, LoaderCircle } from "lucide-react";
import type { LoadingBackground } from "../../lib/launch";
import { cn } from "../ui/cn";
import { Progress } from "../ui/primitives";

/** What the start is doing: the bar's label, its detail, how far it has got, and whether it failed. */
export interface LoadingProgress {
  label: string;
  detail: string;
  percent: number;
  failed: boolean;
}

type LoadingScreenProps = {
  title: string;
  /** The system's short name and colour, for the badge and the plain background. */
  short: string;
  color: string;
  background: LoadingBackground | null;
  /** The bar; null before the first word from the server. */
  progress: LoadingProgress | null;
  /** Why the game could not start, when it failed before the server reported a phase. */
  error: string;
  /** `FCEUmm · 256×224 @ 60.10 fps`, once the emulator is up. */
  status: string;
  /** How to leave a start that is slow, stuck or failed. */
  hint: string;
};

export function LoadingScreen({ title, short, color, background, progress, error, status, hint }: LoadingScreenProps) {
  const failed = progress?.failed || error !== "";
  const label = progress?.label ?? (error || `Starting ${title || "the game"}…`);
  return (
    <section className="absolute inset-0 overflow-hidden bg-surface">
      {background ? (
        <img
          src={background.url}
          alt=""
          className={cn(
            "absolute inset-0 size-full object-cover",
            background.pixelated && "pixelated",
            background.blurred && "scale-110 blur-2xl",
          )}
        />
      ) : (
        <div
          className="absolute inset-0"
          style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 25%, black))` }}
        />
      )}
      {/* An even scrim, deeper along the bottom where the text and the spinner sit. */}
      <div className="absolute inset-0 bg-black/55" />
      <div className="absolute inset-x-0 bottom-0 h-2/5 bg-gradient-to-t from-black/55 to-transparent" />

      <div className="absolute inset-x-0 bottom-36 grid justify-items-center gap-3 px-6 text-center sm:bottom-40">
        {progress && (
          <Progress
            value={progress.percent}
            color="var(--color-accent)"
            className="h-1.5 w-[25rem] max-w-full bg-white/15"
            label={progress.label}
          />
        )}
        <p
          className={cn("flex items-center gap-2 text-xl font-semibold", failed ? "text-red-300" : "text-slate-100")}
          role="status"
        >
          {label}
        </p>
        {progress?.detail && <p className="text-base text-slate-300 tabular-nums">{progress.detail}</p>}
        <p className="text-[13px] text-slate-400">{hint}</p>
      </div>

      <div className="absolute right-6 bottom-6 left-6 flex items-end justify-between gap-6 sm:right-10 sm:bottom-10 sm:left-10">
        <div className="grid min-w-0 justify-items-start gap-2">
          <span className="rounded-lg px-3 py-1 text-sm font-black" style={{ background: color }}>
            {short}
          </span>
          <h1 className="max-w-[60vw] truncate text-[2rem] leading-tight font-black">{title}</h1>
          {status && <p className="text-sm text-slate-300 tabular-nums">{status}</p>}
        </div>
        {failed ? (
          <CircleAlert className="size-10 shrink-0 text-red-300" aria-hidden />
        ) : (
          <LoaderCircle className="size-10 shrink-0 animate-spin text-white/80" aria-hidden />
        )}
      </div>
    </section>
  );
}
