// Emulator settings: each system's core options, which an admin can change. A dropdown picks
// the system; its settings are grouped (the core's own categories, else Video, Audio, Input …,
// guessed in src/lib/settings.ts), with each setting's explanation from the core. Changes
// apply to the next game started. Options set in the server's environment win and are shown
// locked; the admin's changes can be copied out as environment lines, so the configuration
// stays reproducible without this page.
import { Check, Copy, Cpu, Gauge, Info, Lock, RotateCcw, Search, SlidersHorizontal, Terminal } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type CoreSetting, type SystemSettings, type SystemSummary } from "../../lib/api";
import { choiceLabel, filterSettings, groupSettings, runAheadChoices, sourceBadge } from "../../lib/settings";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { panel, Select, Tooltip } from "../ui/primitives";

function SettingRow({ s, onChange }: { s: CoreSetting; onChange: (key: string, value: string | null) => void }) {
  const badge = sourceBadge(s.source);
  const locked = s.source === "ENVIRONMENT";
  const choices = s.choices.includes(s.value) || !s.choices.length ? s.choices : [...s.choices, s.value];
  return (
    <li className="grid gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start">
      <div className="min-w-0">
        <p className="flex flex-wrap items-center gap-2 font-semibold">
          {s.description}
          {badge && (
            <Tooltip content={badge.title}>
              <span
                className={cn(
                  "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-semibold",
                  locked
                    ? "bg-amber-500/20 text-amber-200"
                    : s.source === "ADMIN"
                      ? "bg-accent/25 text-blue-200"
                      : "bg-white/10 text-slate-300",
                )}
              >
                {locked && <Lock className="size-3" />}
                {badge.text}
              </span>
            </Tooltip>
          )}
        </p>
        {s.info && <p className="mt-1 max-w-prose text-sm leading-relaxed text-slate-400">{s.info}</p>}
        <p className="mt-1 truncate font-mono text-[11px] text-slate-600">{s.key}</p>
      </div>
      <div className="flex items-center gap-2 sm:justify-end">
        {choices.length ? (
          <Select
            label={s.description}
            value={s.value}
            disabled={locked}
            options={choices.map((c) => ({ value: c, label: choiceLabel(s, c) }))}
            onChange={(v) => onChange(s.key, v)}
            className="w-56 max-w-full"
          />
        ) : (
          <code className="text-sm text-slate-300">{s.value}</code>
        )}
        {s.source === "ADMIN" && (
          <Tooltip content="Go back to the default">
            <Button
              variant="ghost"
              size="icon"
              aria-label={`Reset ${s.description}`}
              onClick={() => onChange(s.key, null)}
            >
              <RotateCcw />
            </Button>
          </Tooltip>
        )}
      </div>
    </li>
  );
}

/** Run-ahead: SovEmu's own setting, not the core's, so it sits above the core's groups. */
function RunAheadPanel({
  setting,
  onChange,
}: {
  setting: SystemSettings["runAhead"];
  onChange: (frames: number | null) => void;
}) {
  const badge = setting.source === "SOVEMU" ? null : sourceBadge(setting.source);
  const locked = setting.source === "ENVIRONMENT";
  return (
    <section className={cn(panel, "grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start")}>
      <div className="min-w-0">
        <h3 className="flex flex-wrap items-center gap-2 text-lg font-bold">
          <Gauge className="size-5 text-slate-400" /> Run-ahead
          {badge && (
            <Tooltip content={badge.title}>
              <span
                className={cn(
                  "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-semibold",
                  locked ? "bg-amber-500/20 text-amber-200" : "bg-accent/25 text-blue-200",
                )}
              >
                {locked && <Lock className="size-3" />}
                {badge.text}
              </span>
            </Tooltip>
          )}
        </h3>
        <p className="mt-1 max-w-prose text-sm leading-relaxed text-slate-400">
          {setting.supported
            ? "Hides the game's own input lag: the picture is shown a few frames ahead, so a press shows up that much sooner. Most games need 1 or 2; too many and the game visibly skips ahead. Each frame costs another frame of emulation."
            : "Not for this system: running its games ahead would cost more than a frame allows."}
        </p>
      </div>
      {setting.supported && (
        <div className="flex items-center gap-2 sm:justify-end">
          <Select
            label="Run-ahead"
            value={String(setting.frames)}
            disabled={locked}
            options={runAheadChoices(setting.max)}
            onChange={(v) => onChange(Number(v))}
            className="w-56 max-w-full"
          />
          {setting.source === "ADMIN" && (
            <Tooltip content="Turn run-ahead off">
              <Button variant="ghost" size="icon" aria-label="Reset run-ahead" onClick={() => onChange(null)}>
                <RotateCcw />
              </Button>
            </Tooltip>
          )}
        </div>
      )}
    </section>
  );
}

function Settings() {
  const [systems, setSystems] = useState<SystemSummary[]>([]);
  const [current, setCurrent] = useState<SystemSummary | null>(null);
  const [loaded, setLoaded] = useState<SystemSettings | null>(null);
  const [query, setQuery] = useState("");
  const [message, setMessage] = useState<{ text: string; error?: boolean } | null>(null);
  const [copied, setCopied] = useState(false);

  const select = useCallback(async (s: SystemSummary) => {
    setCurrent(s);
    setLoaded(null);
    setMessage(null);
    history.replaceState(null, "", `?id=${encodeURIComponent(s.id)}`);
    try {
      setLoaded(await api.systemSettings(s.id));
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  }, []);

  useEffect(() => {
    api.viewer().then((s) => !s.user?.isAdmin && location.replace("/"));
    api.systems().then((all) => {
      const offered = all.filter((s) => s.playable || s.needsGpu);
      setSystems(offered);
      const wanted = new URLSearchParams(location.search).get("id");
      const first = offered.find((s) => s.id === wanted) ?? offered[0];
      if (first) void select(first);
    });
  }, [select]);

  const change = async (key: string, value: string | null) => {
    if (!current) return;
    try {
      setLoaded(await api.changeSettings(current.id, { [key]: value }));
      setMessage({
        text:
          value === null
            ? "Back to the default. Applies to the next game started."
            : "Saved. Applies to the next game started.",
      });
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  };

  const changeRunAhead = async (frames: number | null) => {
    if (!current) return;
    try {
      setLoaded(await api.setRunAhead(current.id, frames));
      setMessage({
        text: frames ? "Saved. Applies to the next game started." : "Run-ahead off. Applies to the next game started.",
      });
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  };

  const copy = async () => {
    await navigator.clipboard.writeText(loaded?.environment.join("\n") ?? "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const groups = useMemo(
    () => (loaded ? groupSettings(filterSettings(loaded.settings, query), loaded.categories ?? []) : []),
    [loaded, query],
  );
  const changed = loaded?.settings.filter((s) => s.source === "ADMIN").length ?? 0;

  return (
    <main className="mx-auto grid max-w-6xl gap-6 px-6 pt-16 pb-16">
      <section className={panel}>
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h2 className="flex items-center gap-2 text-xl font-bold">
              <SlidersHorizontal className="size-5 text-slate-400" /> Emulator settings
            </h2>
            <p className="mt-1 text-sm text-slate-400">
              Rendering, resolution, sound, controls and more. Changes apply to the next game started.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Select
              label="System"
              value={current?.id ?? ""}
              options={systems.map((s) => ({ value: s.id, label: s.name }))}
              onChange={(id) => {
                const s = systems.find((x) => x.id === id);
                if (s) void select(s);
              }}
              className="h-9 w-64 text-base font-semibold"
            />
            <label className="relative">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-slate-500" />
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search settings"
                aria-label="Search settings"
                className="h-9 w-56 max-w-full rounded-md border border-edge bg-black/30 pr-3 pl-8 text-sm outline-none focus:border-accent"
              />
            </label>
          </div>
        </div>
        {current && (
          <p className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-slate-400">
            <span className="flex items-center gap-1.5">
              <Cpu className="size-4" />
              {loaded ? loaded.core : message?.error ? "" : "Reading the emulator's settings…"}
            </span>
            {loaded && (
              <span>
                {loaded.settings.length} settings{changed ? ` · ${changed} changed` : ""}
              </span>
            )}
          </p>
        )}
        {message && (
          <p className={cn("mt-3 text-sm", message.error ? "text-red-300" : "text-emerald-300")}>{message.text}</p>
        )}
      </section>

      {loaded && <RunAheadPanel setting={loaded.runAhead} onChange={changeRunAhead} />}

      {loaded && !loaded.settings.length && (
        <p className={cn(panel, "flex items-start gap-3 text-sm text-slate-300")}>
          <Info className="mt-0.5 size-4 shrink-0 text-slate-400" />
          {loaded.needsGame
            ? `This emulator lists its settings only once a game is running. Start any ${current?.name ?? ""} game once, and they will appear here.`
            : "This emulator has no settings."}
        </p>
      )}
      {loaded && loaded.settings.length > 0 && !groups.length && (
        <p className="text-sm text-slate-400">No settings match "{query}".</p>
      )}

      {groups.length > 0 && (
        <div className="grid gap-6 lg:grid-cols-[15rem_minmax(0,1fr)]">
          {/* The groups, to jump between them; it stays in view while the page scrolls. */}
          <nav aria-label="Setting groups" className="hidden lg:block">
            <ul className="sticky top-24 grid gap-0.5">
              {groups.map((g) => {
                const n = g.settings.filter((s) => s.source === "ADMIN").length;
                return (
                  <li key={g.id}>
                    <a
                      href={`#${g.id}`}
                      className="flex items-center justify-between gap-2 rounded-md px-3 py-1.5 text-sm text-slate-300 hover:bg-white/10 hover:text-white"
                    >
                      <span className="min-w-0 truncate">{g.title}</span>
                      <span className="flex shrink-0 items-center gap-1.5 text-xs text-slate-500 tabular-nums">
                        {n > 0 && <span className="size-1.5 rounded-full bg-accent" title={`${n} changed`} />}
                        {g.settings.length}
                      </span>
                    </a>
                  </li>
                );
              })}
            </ul>
          </nav>

          <div className="grid gap-6">
            {groups.map((g) => (
              <section key={g.id} id={g.id} className={cn(panel, "scroll-mt-24 py-4")}>
                <h3 className="text-lg font-bold">{g.title}</h3>
                {g.info && <p className="text-sm text-slate-400">{g.info}</p>}
                <ul className="mt-2 divide-y divide-white/5">
                  {g.settings.map((s) => (
                    <SettingRow key={s.key} s={s} onChange={change} />
                  ))}
                </ul>
              </section>
            ))}

            {loaded && loaded.environment.length > 0 && (
              <section className={panel}>
                <div className="flex items-center justify-between gap-4">
                  <h3 className="flex items-center gap-2 text-lg font-bold">
                    <Terminal className="size-5 text-slate-400" /> As environment variables
                  </h3>
                  <Button variant="outline" onClick={copy}>
                    {copied ? <Check /> : <Copy />} {copied ? "Copied" : "Copy"}
                  </Button>
                </div>
                <p className="mt-2 text-sm text-slate-400">
                  The changes made here, as server settings. Put them in <code>.env</code> or the Helm values to make
                  them part of the configuration; settings from the environment always win.
                </p>
                <pre className="mt-3 overflow-x-auto rounded-lg bg-black/40 p-3 text-xs">
                  {loaded.environment.join("\n")}
                </pre>
              </section>
            )}
          </div>
        </div>
      )}
    </main>
  );
}

export default function SettingsAdmin() {
  return (
    <Shell>
      <SideNav section="admin" />
      <div className={NAV_SPACE}>
        <Settings />
      </div>
    </Shell>
  );
}
