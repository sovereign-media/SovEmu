// A system's BIOS check as a card: its folder, each group of files it reads, and each file's
// state. The BIOS page shows them to everyone; the admin BIOS page adds the good dumps' MD5s.

import { CircleAlert, CircleCheck, CircleDashed, CircleHelp, Copy, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { type BiosGroup, type BiosSystem, formatSize, type SystemBios } from "../lib/api";
import { FILE_STATES, fileHint, fileTone, IMPORTANCE, STATES, type Tone } from "../lib/bios";
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

export function Pill({ tone, children }: { tone: Tone; children: ReactNode }) {
  return (
    <span className={cn("rounded px-1.5 py-0.5 text-[11px] font-semibold whitespace-nowrap", PILL[tone])}>
      {children}
    </span>
  );
}

/** A file's good dumps, for an admin to compare a file against. */
function ExpectedMd5({ md5s }: { md5s: string[] }) {
  if (md5s.length === 0) return null;
  return (
    <div className="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-slate-500">
      MD5{md5s.length > 1 ? ", any of" : ""}
      {md5s.map((m) => (
        <span key={m} className="inline-flex items-center gap-1 rounded bg-black/40 py-0.5 pr-0.5 pl-1.5">
          <code className="font-mono text-slate-300 select-all">{m}</code>
          <button
            type="button"
            aria-label={`Copy ${m}`}
            className="rounded p-0.5 text-slate-500 hover:bg-white/10 hover:text-slate-200 [&_svg]:size-3"
            onClick={() => navigator.clipboard.writeText(m)}
          >
            <Copy />
          </button>
        </span>
      ))}
    </div>
  );
}

function Group({ group, folder, admin }: { group: BiosGroup; folder: string; admin: boolean }) {
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
                {admin && <ExpectedMd5 md5s={f.expectedMd5} />}
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

export function SystemCard({
  system,
  bios,
  current = false,
  admin = false,
  biosDir,
}: {
  system: BiosSystem;
  bios: SystemBios;
  current?: boolean;
  /** Shows each file's good dumps' MD5s. */
  admin?: boolean;
  /** The server's BIOS folder, to name the system's folder in full. */
  biosDir?: string;
}) {
  const state = STATES[bios.state];
  return (
    <section id={`bios-${system.id}`} className={cn(panel, "grid scroll-mt-16 gap-4", current && "ring-2 ring-accent")}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="flex items-center gap-3 text-lg font-bold">
          <span className="size-3 rounded-full" style={{ background: system.color }} />
          {system.name}
          <code className="text-xs font-normal break-all text-slate-500">
            {biosDir ? `${biosDir.replace(/\/$/, "")}/${bios.folder}/` : `bios/${bios.folder}/`}
          </code>
        </h2>
        <Tooltip content={state.help}>
          <span tabIndex={0}>
            <Pill tone={state.tone}>{state.text}</Pill>
          </span>
        </Tooltip>
      </div>
      {bios.groups.map((g) => (
        <Group key={g.label} group={g} folder={bios.folder} admin={admin} />
      ))}
    </section>
  );
}
