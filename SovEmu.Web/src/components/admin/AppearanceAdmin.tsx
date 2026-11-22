// Appearance: the background behind every screen, one for everyone, in every client (web, Android,
// TV). The admin picks it here; `SOVEMU_BACKGROUND` sets it from the environment instead, and then
// the page shows it locked. Backgrounds of one's own go in the background folders; system photos
// of one's own in `<data>/systems/`.

import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, Check, Lock, Shuffle } from "lucide-react";
import type { ReactNode } from "react";
import { api } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { EnvLine, ErrorNote } from "./AdminPage";

function Choice({
  label,
  chosen,
  locked,
  onChoose,
  children,
}: {
  label: string;
  chosen: boolean;
  locked: boolean;
  onChoose: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={chosen}
      aria-label={label}
      disabled={locked && !chosen}
      onClick={onChoose}
      className={cn(
        "group grid gap-2 rounded-xl p-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-accent",
        chosen ? "bg-accent/15 ring-2 ring-accent" : "hover:bg-white/5",
        locked && !chosen && "cursor-not-allowed opacity-50",
      )}
    >
      <div className="relative aspect-video overflow-hidden rounded-lg border border-white/10 bg-surface">
        {children}
        {chosen && (
          <span className="absolute top-2 right-2 grid size-6 place-items-center rounded-full bg-accent text-white [&_svg]:size-4">
            <Check aria-hidden />
          </span>
        )}
      </div>
      <span className="px-1 text-sm font-medium">{label}</span>
    </button>
  );
}

function Appearance() {
  const appearance = useQuery(q.appearance());
  const choose = useMutation({
    mutationFn: api.setBackground,
    // Every client reads it as it starts; this one at once.
    onSuccess: () => invalidate(["admin", "appearance"], ["background"]),
  });
  const a = appearance.data;
  const locked = !!a?.fromEnvironment;
  const pick = (id: string) => {
    if (!locked && a?.chosen !== id) choose.mutate(id);
  };

  return (
    <>
      <ErrorNote error={appearance.error ?? choose.error} />
      {a && (
        <section className={panel} aria-labelledby="background-heading">
          <h2 id="background-heading" className="text-lg font-bold">
            Background
          </h2>
          <p className="mt-1 text-sm text-slate-400">
            Behind every screen, for everyone, on the web, the Android app and the TV. Apps pick up a change when they
            next start.
          </p>
          {locked ? (
            <p className="mt-3 flex items-center gap-2 text-sm text-amber-200 [&_svg]:size-4">
              <Lock aria-hidden /> Set on the server: <EnvLine>{a.environment}</EnvLine>
            </p>
          ) : (
            <p className="mt-3 text-sm text-slate-400">
              To set it from the environment instead: <EnvLine>{a.environment}</EnvLine>
            </p>
          )}
          <div className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(12rem,1fr))] gap-3">
            <Choice
              label="A different one each visit"
              chosen={a.chosen === "random"}
              locked={locked}
              onChoose={() => pick("random")}
            >
              <span className="grid size-full place-items-center text-slate-500 [&_svg]:size-8">
                <Shuffle aria-hidden />
              </span>
            </Choice>
            {a.backgrounds.map((b) => (
              <Choice key={b.id} label={b.name} chosen={a.chosen === b.id} locked={locked} onChoose={() => pick(b.id)}>
                <img src={b.url} alt="" loading="lazy" className="absolute inset-0 size-full object-cover" />
                {!b.builtin && (
                  <span className="absolute bottom-2 left-2 rounded bg-black/60 px-1.5 py-0.5 text-[11px] text-slate-200">
                    your own
                  </span>
                )}
              </Choice>
            ))}
            <Choice label="None" chosen={a.chosen === "none"} locked={locked} onChoose={() => pick("none")}>
              <span className="grid size-full place-items-center text-slate-600 [&_svg]:size-8">
                <Ban aria-hidden />
              </span>
            </Choice>
          </div>
          <p className="mt-4 text-xs text-muted">
            Your own backgrounds: WebP, PNG or JPEG images in <code>backgrounds/</code> in the data folder (or a folder{" "}
            <code>SOVEMU_BACKGROUNDS_DIRS</code> names), offered by file name; one named like a built-in one replaces
            it. Your own system photos: <code>systems/&lt;system id&gt;.webp</code> in the data folder. Both are found
            by the next library scan.
          </p>
        </section>
      )}
    </>
  );
}

export default function AppearanceAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <Appearance />
    </AdminPage>
  );
}
