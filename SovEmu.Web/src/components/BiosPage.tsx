// The BIOS check, reached from the Systems menu: for each system that uses a BIOS, its folder
// (`bios/<system>/`, named like its ROM folder) and each file it reads, checked on the server
// by name and MD5. `?id=psx` scrolls to one system. B goes home.
import { CircleAlert, CircleCheck, CircleDashed, CircleHelp, Microchip, RefreshCw, TriangleAlert } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api, type BiosGroup, type BiosSystem, formatSize, type SystemBios } from "../lib/api";
import { FILE_STATES, fileHint, fileTone, IMPORTANCE, STATES, summary, type Tone, withBios } from "../lib/bios";
import { listenNav } from "../lib/nav";
import HintBar from "./HintBar";
import Shell from "./Shell";
import SideNav, { NAV_SPACE } from "./SideNav";
import { Button } from "./ui/button";
import { cn } from "./ui/cn";
import { panel, Tooltip } from "./ui/primitives";

const PILL: Record<Tone, string> = {
  good: "bg-emerald-500/20 text-emerald-200",
  warn: "bg-amber-500/20 text-amber-200",
  info: "bg-sky-500/20 text-sky-200",
  bad: "bg-red-500/25 text-red-200",
  quiet: "bg-white/10 text-slate-400",
};

const ICON: Record<Tone, ReactNode> = {
  good: <CircleCheck className="text-emerald-400" />,
  warn: <TriangleAlert className="text-amber-400" />,
  info: <CircleHelp className="text-sky-400" />,
  bad: <CircleAlert className="text-red-400" />,
  quiet: <CircleDashed className="text-slate-600" />,
};

function Pill({ tone, children }: { tone: Tone; children: ReactNode }) {
  return (
    <span className={cn("rounded px-1.5 py-0.5 text-[11px] font-semibold whitespace-nowrap", PILL[tone])}>
      {children}
    </span>
  );
}

function Group({ group, folder }: { group: BiosGroup; folder: string }) {
  return (
    <div className="border-t border-white/5 pt-4">
      <p className="flex flex-wrap items-center gap-2 font-semibold">
        {group.label}
        <Pill tone={group.importance === "REQUIRED" ? "bad" : group.importance === "RECOMMENDED" ? "info" : "quiet"}>
          {IMPORTANCE[group.importance]}
        </Pill>
        {group.anyOne && <span className="text-xs font-normal text-slate-500">any one is enough</span>}
      </p>
      <p className="mt-1 max-w-prose text-sm leading-relaxed text-slate-400">{group.help}</p>
      <ul className="mt-3 grid gap-2">
        {group.files.map((f) => {
          const tone = fileTone(f, group);
          const hint = fileHint(f, folder);
          return (
            <li
              key={`${f.name}:${f.foundAt}`}
              className={cn("flex items-start gap-3 text-sm", tone === "quiet" && "opacity-60")}
            >
              <span className="mt-0.5 [&_svg]:size-4">{ICON[tone]}</span>
              <div className="min-w-0 flex-1">
                <p className="flex flex-wrap items-baseline gap-x-2">
                  <code className="font-mono text-slate-200">{f.name}</code>
                  <span className="text-slate-400">{f.label}</span>
                  <span className="text-xs text-slate-600">{formatSize(f.size)}</span>
                </p>
                {hint && tone !== "quiet" && <p className="mt-0.5 text-xs text-slate-400">{hint}</p>}
              </div>
              {f.state !== "MISSING" &&
                (f.md5 ? (
                  <Tooltip content={<code className="font-mono">MD5 {f.md5}</code>}>
                    <span tabIndex={0}>
                      <Pill tone={tone}>{FILE_STATES[f.state].text}</Pill>
                    </span>
                  </Tooltip>
                ) : (
                  <Pill tone={tone}>{FILE_STATES[f.state].text}</Pill>
                ))}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function SystemCard({ system, bios, current }: { system: BiosSystem; bios: SystemBios; current: boolean }) {
  const state = STATES[bios.state];
  return (
    <section id={`bios-${system.id}`} className={cn(panel, "grid scroll-mt-16 gap-4", current && "ring-2 ring-accent")}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="flex items-center gap-3 text-lg font-bold">
          <span className="size-3 rounded-full" style={{ background: system.color }} />
          {system.name}
          <code className="text-xs font-normal text-slate-500">bios/{bios.folder}/</code>
        </h2>
        <Tooltip content={state.help}>
          <span tabIndex={0}>
            <Pill tone={state.tone}>{state.text}</Pill>
          </span>
        </Tooltip>
      </div>
      {bios.groups.map((g) => (
        <Group key={g.label} group={g} folder={bios.folder} />
      ))}
    </section>
  );
}

function Bios() {
  const [systems, setSystems] = useState<BiosSystem[] | null>(null);
  const [checking, setChecking] = useState(false);
  const wanted = useRef(new URLSearchParams(location.search).get("id"));

  const check = useCallback(async () => {
    setChecking(true);
    try {
      setSystems(await api.bios());
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    void check();
    const nav = listenNav((action) => {
      if (action === "back") location.href = "/";
    });
    return () => nav.stop();
  }, [check]);

  // The system the link asked for, once it is shown.
  useEffect(() => {
    if (systems && wanted.current)
      document.getElementById(`bios-${wanted.current}`)?.scrollIntoView({ block: "start" });
  }, [systems]);

  if (!systems) return null;
  const shown = withBios(systems);
  return (
    <main className="mx-auto grid max-w-4xl gap-6 px-6 pt-16 pb-24">
      <section className={panel}>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <h1 className="flex items-center gap-2 text-2xl font-bold [&_svg]:size-6 [&_svg]:text-slate-400">
            <Microchip /> BIOS files
          </h1>
          <Button onClick={check} disabled={checking}>
            <RefreshCw className={checking ? "animate-spin" : ""} /> Check again
          </Button>
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-400">
          Each system has its own folder in the server's BIOS folder (<code>SOVEMU_BIOS_DIR</code>, <code>bios/</code>{" "}
          in the data folder by default), named like its folder of games: <code>bios/psx/</code>, <code>bios/ps2/</code>
          , <code>bios/dreamcast/</code>. Files are checked by name, which is case-sensitive, and by MD5 against
          libretro's list of known dumps. Systems not listed need no BIOS.
        </p>
        <p className="mt-3 text-sm font-semibold">{summary(shown)}</p>
      </section>
      {shown.map((s) => (
        <SystemCard key={s.id} system={s} bios={s.bios} current={s.id === wanted.current} />
      ))}
      <HintBar hints={[{ button: "b", label: "Home" }]} />
    </main>
  );
}

export default function BiosPage() {
  return (
    <Shell>
      <SideNav section="systems" />
      <div className={NAV_SPACE}>
        <Bios />
      </div>
    </Shell>
  );
}
