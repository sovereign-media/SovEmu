// Search across every system: a field, and the matching games as a grid of covers. Typing (or a
// pad's on-screen keyboard) searches after a pause; down or Enter goes into the results, up from
// their top row comes back to the field; A opens a game's panel on its system's page. The query
// is kept in the URL, so coming back from that page shows the same results.
import { Search as SearchIcon, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api, type Game, type SystemSummary } from "../../lib/api";
import { listenNav } from "../../lib/nav";
import { FIELD, moveSearch, SEARCH_DELAY_MS, SEARCH_LIMIT } from "../../lib/search";
import HintBar from "../HintBar";
import { GameCard } from "../home/Row";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";

const browse = (g: Game) => (location.href = `/system/?id=${encodeURIComponent(g.system)}&game=${g.id}`);

/** Results laid out per row, read from the page (the grid fills the width it has). */
function columnsOf(grid: HTMLElement | null): number {
  const cards = grid?.children;
  if (!cards?.length) return 1;
  const top = (cards[0] as HTMLElement).offsetTop;
  let n = 0;
  while (n < cards.length && (cards[n] as HTMLElement).offsetTop === top) n++;
  return Math.max(1, n);
}

function SearchScreen() {
  const [text, setText] = useState(() => new URLSearchParams(location.search).get("q") ?? "");
  const [results, setResults] = useState<Game[] | null>(null);
  const [systems, setSystems] = useState<SystemSummary[]>([]);
  const [at, setAt] = useState(FIELD);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const grid = useRef<HTMLDivElement>(null);

  useEffect(
    () =>
      void api
        .systems()
        .then(setSystems)
        .catch(() => {}),
    [],
  );

  // Ask after a pause in typing; a later answer replaces an earlier one, never the reverse.
  useEffect(() => {
    const q = text.trim();
    history.replaceState(null, "", q ? `?q=${encodeURIComponent(q)}` : location.pathname);
    if (!q) return void setResults(null);
    let gone = false;
    const timer = setTimeout(() => {
      api
        .search(q)
        .then((found) => {
          if (gone) return;
          setResults(found);
          setError(null);
        })
        .catch((e) => !gone && setError((e as Error).message));
    }, SEARCH_DELAY_MS);
    return () => {
      gone = true;
      clearTimeout(timer);
    };
  }, [text]);

  // The field has the browser's focus while it is the focus; a result has the ring.
  useEffect(() => {
    if (at === FIELD) field.current?.focus();
    else {
      field.current?.blur();
      grid.current?.querySelector(`[data-col="${at}"]`)?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  }, [at]);

  const latest = useRef({ at, results });
  latest.current = { at, results };
  useEffect(() => {
    const nav = listenNav((action) => {
      const { at, results } = latest.current;
      const found = results ?? [];
      if (action === "accept") {
        if (at === FIELD) return found.length && setAt(0);
        return found[at] && browse(found[at]);
      }
      if (action === "back") {
        if (at !== FIELD) return setAt(FIELD);
        return void (location.href = "/");
      }
      setAt(moveSearch(at, action, found.length, columnsOf(grid.current)));
    });
    return nav.stop;
  }, []);

  const colorOf = (id: string) => systems.find((s) => s.id === id)?.color ?? "#4169e1";
  const shortOf = (id: string) => systems.find((s) => s.id === id)?.short ?? id.toUpperCase();
  const found = results ?? [];

  return (
    <main className="px-6 pt-10 pb-28 md:px-8">
      <h1 className="mb-6 text-3xl font-bold text-white">Search</h1>
      <div className="relative max-w-2xl">
        <SearchIcon className="pointer-events-none absolute top-1/2 left-4 size-5 -translate-y-1/2 text-slate-500" />
        <input
          ref={field}
          type="search"
          value={text}
          autoFocus
          enterKeyHint="search"
          aria-label="Search every system"
          placeholder="Search every system: titles, makers, genres"
          onChange={(e) => setText(e.target.value)}
          onFocus={() => setAt(FIELD)}
          onKeyDown={(e) => {
            if ((e.key === "ArrowDown" || e.key === "Enter") && found.length) {
              e.preventDefault();
              setAt(0);
            }
            if (e.key === "Escape" && text) {
              e.preventDefault();
              setText("");
            }
          }}
          className="h-12 w-full rounded-full border border-white/15 bg-black/30 pr-12 pl-12 text-base text-slate-100 outline-none placeholder:text-slate-500 focus:border-accent [&::-webkit-search-cancel-button]:hidden"
        />
        {text && (
          <button
            type="button"
            aria-label="Clear"
            onClick={() => {
              setText("");
              field.current?.focus();
            }}
            className="absolute top-1/2 right-3 grid size-8 -translate-y-1/2 cursor-pointer place-items-center rounded-full text-slate-400 hover:bg-white/10"
          >
            <X className="size-4" />
          </button>
        )}
      </div>
      <p className="mt-4 h-5 text-sm text-slate-400" aria-live="polite">
        {error ??
          (results === null
            ? ""
            : found.length === 0
              ? "No games match. Try fewer words."
              : `${found.length === SEARCH_LIMIT ? `The first ${SEARCH_LIMIT}` : found.length} game${found.length === 1 ? "" : "s"}`)}
      </p>
      <div ref={grid} className="mt-6 grid grid-cols-[repeat(auto-fill,10rem)] gap-x-4 gap-y-6">
        {found.map((g, i) => (
          <GameCard
            key={g.id}
            game={g}
            row={0}
            col={i}
            focused={at === i}
            color={colorOf(g.system)}
            short={shortOf(g.system)}
            onOpen={() => browse(g)}
          />
        ))}
      </div>
      <HintBar
        hints={[
          { button: "dpad", label: "Choose" },
          { button: "a", label: "Open" },
          { button: "b", label: at === FIELD ? "Home" : "Search" },
        ]}
      />
    </main>
  );
}

export default function Search() {
  return (
    <Shell>
      <SideNav section="search" />
      <div className={NAV_SPACE}>
        <SearchScreen />
      </div>
    </Shell>
  );
}
