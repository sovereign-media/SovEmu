// Logs: the server's recent log, its own lines and its game sessions', newest at the bottom,
// read again every 2 s while following (Pause holds the view). Filtered by level, by one game
// session (a session's id from its line, or the Sessions page), and by text. Only a view: the
// lines go to stderr as ever, and SOVEMU_LOG_BUFFER_LINES=0 keeps none.

import { useQuery } from "@tanstack/react-query";
import { Pause, Play, ScrollText } from "lucide-react";
import { useState } from "react";
import type { LogFilter, LogLevel } from "../../lib/api";
import { q } from "../../lib/data";
import { fieldsText, LEVEL_WORDS, LEVELS, timeText } from "../../lib/logs";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { inputClass, Segmented } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";

const LEVEL_TONE: Record<LogLevel, string> = {
  TRACE: "text-slate-500",
  DEBUG: "text-slate-400",
  INFO: "text-sky-300",
  WARN: "text-amber-300",
  ERROR: "text-red-300",
};

function Logs() {
  const [level, setLevel] = useState<LogLevel>("INFO");
  const [session, setSession] = useState("");
  const [text, setText] = useState("");
  const [following, setFollowing] = useState(true);
  const filter: LogFilter = { level, session: session.trim() || null, text: text.trim() || null };
  const logs = useQuery({
    ...q.logs(filter),
    refetchInterval: following ? 2000 : false,
    placeholderData: (previous) => previous,
  });
  const page = logs.data;

  return (
    <AdminSection
      icon={<ScrollText />}
      title="Recent log"
      description="The server's own lines and its game sessions', newest at the bottom."
      actions={
        <Button variant="outline" onClick={() => setFollowing(!following)}>
          {following ? <Pause /> : <Play />} {following ? "Pause" : "Follow"}
        </Button>
      }
    >
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Segmented label="Level" value={level} options={LEVELS} onChange={setLevel} />
        <input
          type="search"
          value={session}
          onChange={(e) => setSession(e.target.value)}
          placeholder="Session id"
          aria-label="Session id"
          className={cn(inputClass, "h-9 w-48 font-mono text-sm")}
        />
        <input
          type="search"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Text"
          aria-label="Text"
          className={cn(inputClass, "h-9 w-56 text-sm")}
        />
      </div>
      <div className="mt-3">
        <ErrorNote error={logs.error} />
      </div>
      {page && !page.enabled && (
        <p className="mt-4 text-sm text-slate-400">
          The server keeps no log for this view: <code>SOVEMU_LOG_BUFFER_LINES</code> is 0. Its lines still go to its
          standard error.
        </p>
      )}
      {page?.enabled &&
        (page.lines.length === 0 ? (
          <p className="mt-4 text-sm text-slate-400">No lines match.</p>
        ) : (
          <ol
            aria-label="Log lines"
            className="mt-4 max-h-[70vh] overflow-auto rounded-lg bg-black/40 p-3 font-mono text-xs leading-relaxed"
          >
            {page.lines.map((l) => (
              <li key={l.id} className="break-words whitespace-pre-wrap">
                <span className="text-slate-500">{timeText(l.at)}</span>{" "}
                <span className={cn("font-semibold", LEVEL_TONE[l.level])}>{LEVEL_WORDS[l.level].padEnd(5)}</span>{" "}
                <span className="text-slate-500">{l.target}</span>{" "}
                {l.session && (
                  <button
                    type="button"
                    className="text-violet-300 hover:underline"
                    title="Show only this session"
                    onClick={() => setSession(l.session ?? "")}
                  >
                    [{l.session.slice(0, 8)}]
                  </button>
                )}{" "}
                <span className="text-slate-200">{l.message}</span>{" "}
                {l.fields.length > 0 && <span className="text-slate-400">{fieldsText(l.fields)}</span>}
              </li>
            ))}
          </ol>
        ))}
    </AdminSection>
  );
}

export default function LogsAdmin() {
  return (
    <AdminPage width="max-w-6xl">
      <Logs />
    </AdminPage>
  );
}
