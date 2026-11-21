// Home: a hero of featured games over rows (continue playing, systems, recently added), laid out
// as Sovereign's home. A pad or the keyboard moves between rows (up/down) and along them
// (left/right); A plays or opens; X (or /) searches every system; Start rescans.
import { FolderHeart, Gamepad2, Heart, History, Sparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Collection, type Game, type HomeFeed, type SystemSummary } from "../../lib/api";
import { type Focus, moveFocus, type RowShape, startFocus } from "../../lib/home";
import { listenNav } from "../../lib/nav";
import HintBar from "../HintBar";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { useToast } from "../ui/toast";
import { Hero } from "./Hero";
import { GameCard, Row, SystemCard } from "./Row";

const HERO = 0;
const RECENT = 1;
const FAVOURITES = 2;
const SYSTEMS = 3;
const ADDED = 4;
/** Each collection's row follows, in name order. */
const COLLECTIONS = 5;

const play = (g: Game) => (location.href = `/play/?game=${g.id}`);
/** Opens the game's system with its side panel open (its own URL). */
const browse = (g: Game) => (location.href = `/system/?id=${encodeURIComponent(g.system)}&game=${g.id}`);
/** Opens the game's system. */
const browseSystem = (g: Game) => (location.href = `/system/?id=${encodeURIComponent(g.system)}`);
const openSystem = (s: SystemSummary) => (location.href = `/system/?id=${encodeURIComponent(s.id)}`);

function HomeScreen() {
  const toast = useToast();
  const [feed, setFeed] = useState<HomeFeed | null>(null);
  const [systems, setSystems] = useState<SystemSummary[]>([]);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [focus, setFocus] = useState<Focus>({ row: 0, cols: [0, 0, 0, 0, 0] });
  // Focus rings show once a pad or the keyboard is used, not for the mouse.
  const [navigating, setNavigating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [home, all, lists] = await Promise.all([api.home(), api.systems(), api.collections().catch(() => [])]);
      setFeed(home);
      setSystems(all.filter((s) => s.gameCount > 0));
      setCollections(lists.filter((c) => c.games.length > 0));
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);
  useEffect(() => void load(), [load]);

  const rows: RowShape[] = useMemo(
    () => [
      { length: feed?.featured.length ?? 0, wraps: true },
      // Every row loops, like the hero: right on the last card comes back to the first.
      { length: feed?.recent.length ?? 0, wraps: true },
      { length: feed?.favourites.length ?? 0, wraps: true },
      { length: systems.length, wraps: true },
      { length: feed?.added.length ?? 0, wraps: true },
      ...collections.map((c) => ({ length: c.games.length, wraps: true })),
    ],
    [feed, systems, collections],
  );
  // Start on the first row with something in it once the feed arrives.
  useEffect(() => {
    if (feed) setFocus(startFocus(rows));
  }, [feed]); // eslint-disable-line react-hooks/exhaustive-deps

  const heroIndex = focus.cols[HERO] ?? 0;
  const setHeroIndex = (i: number) => setFocus((f) => ({ ...f, cols: f.cols.map((c, r) => (r === HERO ? i : c)) }));

  const accept = useCallback(
    (f: Focus) => {
      if (!feed) return;
      const col = f.cols[f.row] ?? 0;
      if (f.row === HERO && feed.featured[col]) play(feed.featured[col]);
      if (f.row === RECENT && feed.recent[col]) play(feed.recent[col]);
      if (f.row === SYSTEMS && systems[col]) openSystem(systems[col]);
      if (f.row === ADDED && feed.added[col]) browse(feed.added[col]);
      if (f.row === FAVOURITES && feed.favourites[col]) browse(feed.favourites[col]);
      const list = collections[f.row - COLLECTIONS];
      if (list?.games[col]) browse(list.games[col]);
    },
    [feed, systems, collections],
  );

  // Pad and keyboard. The handler reads the latest state through a ref.
  const latest = useRef({ focus, rows, accept });
  latest.current = { focus, rows, accept };
  useEffect(() => {
    const nav = listenNav(async (action) => {
      const { focus, rows, accept } = latest.current;
      if (action === "accept") return accept(focus);
      if (action === "search") return void (location.href = "/search/");
      if (action === "menu") {
        toast("Rescanning the library…");
        try {
          const r = await api.scan();
          toast(`Library rescanned: ${r.added} added, ${r.removed} removed`);
        } catch {
          toast("Only an admin can rescan the library");
        }
        return void load();
      }
      setNavigating(true);
      setFocus(moveFocus(focus, action, rows));
    });
    return nav.stop;
  }, [load, toast]);

  // Keep the focused card on screen.
  useEffect(() => {
    if (!navigating) return;
    if (focus.row === HERO) return void scrollTo({ top: 0, behavior: "smooth" });
    document
      .querySelector(`[data-row="${focus.row}"][data-col="${focus.cols[focus.row]}"]`)
      ?.scrollIntoView({ block: "center", inline: "nearest", behavior: "smooth" });
  }, [focus, navigating]);

  const colorOf = (id: string) => systems.find((s) => s.id === id)?.color ?? "#4169e1";
  const shortOf = (id: string) => systems.find((s) => s.id === id)?.short ?? id.toUpperCase();
  const focused = (row: number, col: number) => navigating && focus.row === row && focus.cols[row] === col;

  if (error) return <p className="grid min-h-dvh place-items-center text-red-300">{error}</p>;
  if (!feed) return <div className="h-[60vh] animate-pulse bg-panel/40" />;

  const empty = systems.length === 0;
  return (
    <main className="pb-28">
      <Hero
        games={feed.featured}
        index={heroIndex}
        onIndex={setHeroIndex}
        focused={navigating && focus.row === HERO}
        color={colorOf}
        short={shortOf}
        onPlay={play}
        onBrowse={browseSystem}
        empty={empty}
      />
      <div className="relative z-10 -mt-8 grid grid-cols-1 gap-10">
        {feed.recent.length > 0 && (
          <Row title="Continue playing" icon={<History />}>
            {feed.recent.map((g, i) => (
              <GameCard
                key={g.id}
                game={g}
                row={RECENT}
                col={i}
                wide
                focused={focused(RECENT, i)}
                color={colorOf(g.system)}
                short={shortOf(g.system)}
                onOpen={() => play(g)}
              />
            ))}
          </Row>
        )}
        {feed.favourites.length > 0 && (
          <Row title="Favourites" icon={<Heart />}>
            {feed.favourites.map((g, i) => (
              <GameCard
                key={g.id}
                game={g}
                row={FAVOURITES}
                col={i}
                focused={focused(FAVOURITES, i)}
                color={colorOf(g.system)}
                short={shortOf(g.system)}
                onOpen={() => browse(g)}
              />
            ))}
          </Row>
        )}
        {systems.length > 0 && (
          <Row title="Systems" icon={<Gamepad2 />}>
            {systems.map((s, i) => (
              <SystemCard
                key={s.id}
                system={s}
                row={SYSTEMS}
                col={i}
                focused={focused(SYSTEMS, i)}
                onOpen={() => openSystem(s)}
              />
            ))}
          </Row>
        )}
        {feed.added.length > 0 && (
          <Row title="Recently added" icon={<Sparkles />}>
            {feed.added.map((g, i) => (
              <GameCard
                key={g.id}
                game={g}
                row={ADDED}
                col={i}
                focused={focused(ADDED, i)}
                color={colorOf(g.system)}
                short={shortOf(g.system)}
                onOpen={() => browse(g)}
              />
            ))}
          </Row>
        )}
        {collections.map((c, n) => (
          <Row key={c.id} title={c.name} icon={<FolderHeart />}>
            {c.games.map((g, i) => (
              <GameCard
                key={g.id}
                game={g}
                row={COLLECTIONS + n}
                col={i}
                focused={focused(COLLECTIONS + n, i)}
                color={colorOf(g.system)}
                short={shortOf(g.system)}
                onOpen={() => browse(g)}
              />
            ))}
          </Row>
        ))}
      </div>
      <HintBar
        hints={[
          { button: "dpad", label: "Choose" },
          { button: "a", label: "Open" },
          { button: "x", label: "Search" },
          { button: "start", label: "Rescan" },
        ]}
      />
    </main>
  );
}

export default function Home() {
  return (
    <Shell>
      <SideNav section="home" />
      <div className={NAV_SPACE}>
        <HomeScreen />
      </div>
    </Shell>
  );
}
