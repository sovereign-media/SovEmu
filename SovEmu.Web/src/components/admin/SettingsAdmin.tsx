// Emulator settings: each system's core options (rendering, resolution, frame skip …), which an
// admin can change. Changes apply to the next game started. Options set in the server's
// environment win and are shown locked; the admin's changes can be copied out as environment
// lines, so the configuration stays reproducible without this page.
import { Check, Copy, Lock, RotateCcw, Search, SlidersHorizontal, Terminal } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, type CoreSetting, type SystemSettings, type SystemSummary } from "../../lib/api";
import { choiceLabel, filterSettings, sourceBadge } from "../../lib/settings";
import Shell from "../Shell";
import TopBar from "../TopBar";
import { Button } from "../ui/button";
import { Chips, panel, Select, Tooltip } from "../ui/primitives";

function SettingRow({ s, onChange }: { s: CoreSetting; onChange: (key: string, value: string | null) => void }) {
  const badge = sourceBadge(s.source);
  const locked = s.source === "environment";
  const choices = s.choices.includes(s.value) || !s.choices.length ? s.choices : [...s.choices, s.value];
  return (
    <li className="flex flex-wrap items-center justify-between gap-3 py-3">
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2 font-semibold">
          {s.description}
          {badge && (
            <Tooltip content={badge.title}>
              <span
                className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-semibold ${locked ? "bg-amber-500/20 text-amber-200" : "bg-white/10 text-slate-300"}`}
              >
                {locked && <Lock className="size-3" />}
                {badge.text}
              </span>
            </Tooltip>
          )}
        </p>
        <p className="truncate font-mono text-xs text-slate-500">{s.key}</p>
      </div>
      <div className="flex items-center gap-2">
        {choices.length ? (
          <Select
            label={s.description}
            value={s.value}
            disabled={locked}
            options={choices.map((c) => ({ value: c, label: choiceLabel(c, s.default) }))}
            onChange={(v) => onChange(s.key, v)}
          />
        ) : (
          <code className="text-sm text-slate-300">{s.value}</code>
        )}
        {s.source === "admin" && (
          <Tooltip content="Go back to the default">
            <Button variant="ghost" size="sm" onClick={() => onChange(s.key, null)}>
              <RotateCcw /> Reset
            </Button>
          </Tooltip>
        )}
      </div>
    </li>
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
    api.status().then((s) => !s.user?.is_admin && location.replace("/"));
    api.systems().then((all) => {
      const offered = all.filter((s) => s.playable || s.needs_gpu);
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
      setMessage({ text: value === null ? `${key} reset. Applies to the next game started.` : "Saved. Applies to the next game started." });
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  };

  const copy = async () => {
    await navigator.clipboard.writeText(loaded?.environment.join("\n") ?? "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const shown = loaded ? filterSettings(loaded.settings, query) : [];
  return (
    <main className="mx-auto grid max-w-4xl gap-6 px-6 pt-24 pb-16">
      <section className={panel}>
        <h2 className="flex items-center gap-2 text-xl font-bold">
          <SlidersHorizontal className="size-5 text-slate-400" /> Emulator settings
        </h2>
        <p className="mt-2 text-sm text-slate-400">
          Each system's emulator options: rendering, resolution, frame skipping and more. Changes apply to the next game started.
        </p>
        <div className="mt-4">
          <Chips
            label="System"
            value={current?.id}
            items={systems.map((s) => ({ value: s.id, label: s.name, color: s.color }))}
            onChange={(id) => {
              const s = systems.find((x) => x.id === id);
              if (s) void select(s);
            }}
          />
        </div>
      </section>

      {current && (
        <section className={panel}>
          <div className="flex flex-wrap items-baseline justify-between gap-4">
            <div>
              <h2 className="text-xl font-bold">{current.name}</h2>
              <p className="text-sm text-slate-400">{loaded ? loaded.core : message?.error ? "" : "Reading the emulator's settings…"}</p>
            </div>
            <label className="relative">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-slate-500" />
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Filter settings"
                aria-label="Filter settings"
                className="w-64 max-w-full rounded-md border border-edge bg-black/30 py-1.5 pr-3 pl-8 text-sm outline-none focus:border-accent"
              />
            </label>
          </div>
          {message && <p className={`mt-4 text-sm ${message.error ? "text-red-300" : "text-emerald-300"}`}>{message.text}</p>}
          {loaded && !loaded.settings.length && <p className="mt-4 text-sm text-slate-400">This emulator has no settings.</p>}
          <ul className="mt-4 divide-y divide-white/5">
            {shown.map((s) => (
              <SettingRow key={s.key} s={s} onChange={change} />
            ))}
          </ul>
        </section>
      )}

      {loaded && loaded.environment.length > 0 && (
        <section className={panel}>
          <div className="flex items-center justify-between gap-4">
            <h2 className="flex items-center gap-2 text-xl font-bold">
              <Terminal className="size-5 text-slate-400" /> As environment variables
            </h2>
            <Button variant="outline" onClick={copy}>
              {copied ? <Check /> : <Copy />} {copied ? "Copied" : "Copy"}
            </Button>
          </div>
          <p className="mt-2 text-sm text-slate-400">
            The changes made here, as server settings. Put them in <code>.env</code> or the Helm values to make them part of the
            configuration; settings from the environment always win.
          </p>
          <pre className="mt-3 overflow-x-auto rounded-lg bg-black/40 p-3 text-xs">{loaded.environment.join("\n")}</pre>
        </section>
      )}
    </main>
  );
}

export default function SettingsAdmin() {
  return (
    <Shell>
      <TopBar crumb="Settings" />
      <Settings />
    </Shell>
  );
}
