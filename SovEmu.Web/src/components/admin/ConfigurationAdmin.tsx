// Configuration: every setting the server runs with, read-only, as the startup log prints it:
// grouped as the README's table, credentials masked, each marked with where its value came from.
// Settings are environment variables; "Copy settings" gives the ones this server was given as
// lines to keep.

import { useQuery } from "@tanstack/react-query";
import { Check, Copy, Search } from "lucide-react";
import { useState } from "react";
import type { ConfigSetting } from "../../lib/api";
import {
  codeSpans,
  environmentLines,
  filterSettings,
  GROUP_LABELS,
  groupSettings,
  LONG_VALUE,
  SOURCE_LABELS,
} from "../../lib/configuration";
import { q } from "../../lib/data";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { ErrorNote } from "./AdminPage";

const SOURCE_TONE: Record<ConfigSetting["source"], string> = {
  DEFAULT: "bg-white/5 text-slate-500",
  ENVIRONMENT: "bg-accent/20 text-accent",
  FILE: "bg-violet-500/20 text-violet-200",
  COMMAND_LINE: "bg-sky-500/20 text-sky-200",
};

function Value({ setting }: { setting: ConfigSetting }) {
  const [open, setOpen] = useState(false);
  const value = setting.value;
  if (value === null) return <span className="text-slate-500 italic">unset</span>;
  const long = value.length > LONG_VALUE;
  return (
    <span className="min-w-0">
      <code className={cn("font-mono text-xs break-all text-slate-200", setting.secret && "text-slate-400")}>
        {long && !open ? `${value.slice(0, LONG_VALUE)}…` : value}
      </code>
      {long && (
        <button
          type="button"
          onClick={() => setOpen(!open)}
          className="ml-2 text-xs text-slate-400 underline hover:text-slate-200"
        >
          {open ? "Less" : "Show all"}
        </button>
      )}
    </span>
  );
}

function Row({ setting }: { setting: ConfigSetting }) {
  return (
    <li className="grid gap-1 py-3 sm:grid-cols-[minmax(0,18rem)_minmax(0,1fr)_auto] sm:gap-4">
      <code className="font-mono text-xs break-all text-slate-300" title={setting.description || undefined}>
        {setting.name}
      </code>
      <div className="min-w-0">
        <Value setting={setting} />
        {setting.effective && (
          <div className="mt-0.5 text-xs text-slate-500">
            → <code className="font-mono break-all">{setting.effective}</code>
          </div>
        )}
        {setting.description && (
          <p className="mt-0.5 text-xs text-slate-500">
            {codeSpans(setting.description).map((part, i) =>
              part.code ? (
                // biome-ignore lint/suspicious/noArrayIndexKey: the spans of one fixed string
                <code key={i} className="font-mono text-slate-400">
                  {part.text}
                </code>
              ) : (
                part.text
              ),
            )}
          </p>
        )}
      </div>
      <span
        className={cn(
          "self-start rounded-full px-2 py-0.5 text-[11px] font-semibold whitespace-nowrap",
          SOURCE_TONE[setting.source],
        )}
      >
        {SOURCE_LABELS[setting.source]}
      </span>
    </li>
  );
}

function Configuration() {
  const config = useQuery(q.configuration());
  const [text, setText] = useState("");
  const [copied, setCopied] = useState(false);
  const all = config.data ?? [];
  const groups = groupSettings(filterSettings(all, text));
  const given = all.filter((s) => s.source !== "DEFAULT").length;

  const copy = async () => {
    await navigator.clipboard.writeText(environmentLines(all).join("\n"));
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <>
      <ErrorNote error={config.error} />
      <section className={panel}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="font-semibold">
            {config.data ? `${all.length} settings, ${given} given to this server` : "Reading…"}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <label className="flex items-center gap-2 rounded-lg border border-white/10 bg-black/30 px-3 py-1.5 text-sm focus-within:border-accent">
              <Search aria-hidden className="size-4 text-slate-500" />
              <input
                type="search"
                aria-label="Find a setting"
                placeholder="Name, value or description"
                value={text}
                onChange={(e) => setText(e.target.value)}
                className="w-56 bg-transparent outline-none placeholder:text-slate-600"
              />
            </label>
            <Button variant="outline" onClick={copy} disabled={!config.data}>
              {copied ? <Check /> : <Copy />} {copied ? "Copied" : "Copy settings"}
            </Button>
          </div>
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-400">
          Every setting is an environment variable (or a <code>--flag</code>), read at startup: change one and restart
          the server. Secrets show only whether they are set; a secret can also come from the file{" "}
          <code>&lt;NAME&gt;_FILE</code> names. Credentials inside other values are masked. "Copy settings" copies the
          ones given to this server, secrets left out, as lines for <code>.env</code>.
        </p>
      </section>
      {groups.map((g) => (
        <section key={g.group} className={panel} aria-labelledby={`config-${g.group}`}>
          <h2 id={`config-${g.group}`} className="text-lg font-bold">
            {GROUP_LABELS[g.group]}
          </h2>
          <ul className="mt-1 divide-y divide-white/5">
            {g.settings.map((s) => (
              <Row key={s.name} setting={s} />
            ))}
          </ul>
        </section>
      ))}
      {config.data && groups.length === 0 && <p className="text-sm text-muted">No setting matches.</p>}
    </>
  );
}

export default function ConfigurationAdmin() {
  return (
    <AdminPage width="max-w-5xl">
      <Configuration />
    </AdminPage>
  );
}
