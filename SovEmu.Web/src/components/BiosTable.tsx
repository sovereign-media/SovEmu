// The BIOS check as one table: a row per file each system reads, sortable by any column, with a
// search that keeps the rows whose system or file name matches. The BIOS page shows it to
// everyone; the admin BIOS page adds the good dumps' MD5s and the folders in full.

import { ArrowDown, ArrowUp, ArrowUpDown, Copy, Search } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import type { BiosSystem } from "../lib/api";
import {
  type BiosRow,
  type BiosSortKey,
  biosRows,
  filterBiosRows,
  IMPORTANCE,
  nextSort,
  type SortDirection,
  STATES,
  sortBiosRows,
  statusText,
  type Tone,
} from "../lib/bios";
import { formatBytes } from "../lib/numbers";
import { cn } from "./ui/cn";
import { Tooltip } from "./ui/primitives";

const PILL: Record<Tone, string> = {
  good: "bg-emerald-500/20 text-emerald-200",
  warn: "bg-amber-500/20 text-amber-200",
  info: "bg-sky-500/20 text-sky-200",
  bad: "bg-red-500/25 text-red-200",
  quiet: "bg-white/10 text-slate-400",
};

type PillProps = { tone: Tone; children: ReactNode };

export function Pill({ tone, children }: PillProps) {
  return (
    <span className={cn("rounded px-1.5 py-0.5 text-[11px] font-semibold whitespace-nowrap", PILL[tone])}>
      {children}
    </span>
  );
}

const NEED_TONE: Record<BiosRow["group"]["importance"], Tone> = {
  REQUIRED: "bad",
  RECOMMENDED: "info",
  OPTIONAL: "quiet",
};

type ExpectedMd5Props = { md5s: string[] };

/** A file's good dumps, for an admin to compare a file against. */
function ExpectedMd5({ md5s }: ExpectedMd5Props) {
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

const COLUMNS: { key: BiosSortKey; label: string; className?: string }[] = [
  { key: "system", label: "System" },
  { key: "file", label: "File" },
  { key: "description", label: "Description" },
  { key: "need", label: "Need" },
  { key: "size", label: "Size", className: "text-right" },
  { key: "status", label: "Status" },
];

type SortHeaderProps = {
  column: (typeof COLUMNS)[number];
  sort: { key: BiosSortKey; direction: SortDirection };
  onSort: (key: BiosSortKey) => void;
};

function SortHeader({ column, sort, onSort }: SortHeaderProps) {
  const active = sort.key === column.key;
  const Icon = !active ? ArrowUpDown : sort.direction === "asc" ? ArrowUp : ArrowDown;
  return (
    <th
      scope="col"
      aria-sort={active ? (sort.direction === "asc" ? "ascending" : "descending") : "none"}
      className={cn("px-3 py-2 font-semibold whitespace-nowrap", column.className)}
    >
      <button
        type="button"
        onClick={() => onSort(column.key)}
        className={cn(
          "inline-flex items-center gap-1 rounded hover:text-white focus-visible:outline-2 focus-visible:outline-accent",
          active ? "text-slate-100" : "text-slate-400",
        )}
      >
        {column.label}
        <Icon aria-hidden className={cn("size-3.5", !active && "opacity-50")} />
      </button>
    </th>
  );
}

type RowProps = { row: BiosRow; admin: boolean; biosDir?: string };

function Row({ row, admin, biosDir }: RowProps) {
  const quiet = row.tone === "quiet";
  const system = STATES[row.systemState];
  const folder = biosDir ? `${biosDir.replace(/\/$/, "")}/${row.folder}/` : `bios/${row.folder}/`;
  const status = <Pill tone={row.tone}>{statusText(row)}</Pill>;
  return (
    <tr className={cn("border-t border-white/5 align-top", quiet && "text-slate-400")}>
      <td className="px-3 py-2.5">
        <Tooltip content={`${system.text}: ${system.help}`}>
          <span tabIndex={0} className="flex items-center gap-2 font-semibold whitespace-nowrap text-slate-100">
            <span className="size-2.5 shrink-0 rounded-full" style={{ background: row.color }} />
            {row.systemName}
          </span>
        </Tooltip>
        <code className="mt-0.5 block text-xs break-all text-slate-500">{folder}</code>
      </td>
      <td className="px-3 py-2.5">
        <code className="font-mono break-all text-slate-200">{row.file.name}</code>
        {row.hint && !quiet && <p className="mt-0.5 max-w-xs text-xs text-slate-400">{row.hint}</p>}
        {admin && <ExpectedMd5 md5s={row.file.expectedMd5} />}
      </td>
      <td className="px-3 py-2.5">
        {row.file.label}
        <p className="mt-0.5 text-xs text-slate-500">
          {row.group.label}
          {row.group.anyOne && row.group.files.length > 1 ? " · any one is enough" : ""}
        </p>
      </td>
      <td className="px-3 py-2.5">
        <Tooltip content={row.group.help}>
          <span tabIndex={0}>
            <Pill tone={NEED_TONE[row.group.importance]}>{IMPORTANCE[row.group.importance]}</Pill>
          </span>
        </Tooltip>
      </td>
      <td className="px-3 py-2.5 text-right whitespace-nowrap text-slate-400 tabular-nums">
        {formatBytes(row.file.size)}
      </td>
      <td className="px-3 py-2.5">
        {row.file.md5 ? (
          <Tooltip content={<code className="font-mono">MD5 {row.file.md5}</code>}>
            <span tabIndex={0}>{status}</span>
          </Tooltip>
        ) : (
          status
        )}
      </td>
    </tr>
  );
}

type BiosTableProps = {
  systems: BiosSystem[];
  /** A system id (`?id=psx`): the search starts on its name. */
  initialSystem?: string | null;
  /** Shows each file's good dumps' MD5s. */
  admin?: boolean;
  /** The server's BIOS folder, to name each system's folder in full. */
  biosDir?: string;
};

export function BiosTable({ systems, initialSystem, admin = false, biosDir }: BiosTableProps) {
  const rows = useMemo(() => biosRows(systems), [systems]);
  const [text, setText] = useState(() => systems.find((s) => s.id === initialSystem)?.name ?? "");
  const [sort, setSort] = useState<{ key: BiosSortKey; direction: SortDirection }>({
    key: "system",
    direction: "asc",
  });
  const shown = useMemo(() => sortBiosRows(filterBiosRows(rows, text), sort.key, sort.direction), [rows, text, sort]);

  return (
    <section className="rounded-2xl border border-white/10 bg-panel/80" aria-label="BIOS files">
      <div className="flex flex-wrap items-center justify-between gap-3 px-6 pt-5 pb-3">
        <p className="text-sm text-slate-400">
          {shown.length === rows.length
            ? `${rows.length} file${rows.length === 1 ? "" : "s"}`
            : `${shown.length} of ${rows.length} files`}
        </p>
        <label className="flex items-center gap-2 rounded-lg border border-white/10 bg-black/30 px-3 py-1.5 text-sm focus-within:border-accent">
          <Search aria-hidden className="size-4 text-slate-500" />
          <input
            type="search"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="System or file name"
            aria-label="Search BIOS files by system or file name"
            className="w-56 bg-transparent outline-none placeholder:text-slate-600"
          />
        </label>
      </div>
      <div className="overflow-x-auto px-3 pb-3">
        <table className="w-full border-collapse text-left text-sm">
          <thead className="text-xs tracking-wide uppercase">
            <tr>
              {COLUMNS.map((c) => (
                <SortHeader key={c.key} column={c} sort={sort} onSort={(key) => setSort((s) => nextSort(s, key))} />
              ))}
            </tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <Row key={r.key} row={r} admin={admin} biosDir={biosDir} />
            ))}
          </tbody>
        </table>
        {shown.length === 0 && (
          <p className="px-3 py-6 text-center text-sm text-slate-400">
            {rows.length === 0 ? "None of your systems uses a BIOS." : `No system or file matches “${text}”.`}
          </p>
        )}
      </div>
    </section>
  );
}
