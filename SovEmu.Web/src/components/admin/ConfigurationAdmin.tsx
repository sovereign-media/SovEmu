// Configuration: every setting the server runs with, read-only, as the startup log prints it:
// grouped as the README's table, credentials masked, each marked with where its value came from.
// Settings are environment variables; "Copy settings" gives the ones this server was given as
// lines to keep.

import { useQuery } from "@tanstack/react-query";
import { Check, Copy, Search } from "lucide-react";
import { useState } from "react";
import { environmentLines, filterSettings, GROUP_LABELS, groupSettings } from "../../lib/configuration";
import { q } from "../../lib/data";
import { Button } from "../ui/button";
import { panel } from "../ui/primitives";
import AdminPage, { ErrorNote } from "./AdminPage";
import SettingRow from "./SettingRow";

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
              <SettingRow key={s.name} setting={s} />
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
