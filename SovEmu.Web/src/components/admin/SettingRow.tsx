// A setting as the admin pages show it: its variable, its value (folded when long, credentials
// already masked by the server), what the server makes of it, its help, and where it came from.

import { useState } from "react";
import type { ConfigSetting } from "../../lib/api";
import { codeSpans, LONG_VALUE, SOURCE_LABELS } from "../../lib/configuration";
import { cn } from "../ui/cn";

const SOURCE_TONE: Record<ConfigSetting["source"], string> = {
  DEFAULT: "bg-white/5 text-slate-500",
  ENVIRONMENT: "bg-accent/20 text-accent",
  COMMAND_LINE: "bg-sky-500/20 text-sky-200",
};

type ValueProps = { setting: ConfigSetting };

function Value({ setting }: ValueProps) {
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

type SettingRowProps = { setting: ConfigSetting };

export default function SettingRow({ setting }: SettingRowProps) {
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
