// Home: a hero of featured games over rows (continue playing, systems, recently added), laid out
// as Sovereign's home. A pad or the keyboard moves between rows (up/down) and along them
// (left/right); A plays or opens; Start rescans.
import { Gamepad2, History, Sparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Game, type HomeFeed, type SystemSummary } from "../../lib/api";
import { moveFocus, startFocus, type Focus, type RowShape } from "../../lib/home";
import { listenNav } from "../../lib/nav";
import HintBar from "../HintBar";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { useToast } from "../ui/toast";
import { Hero } from "./Hero";
import { GameCard, Row, SystemCard } from "./Row";

const HERO = 0;
const RECENT = 1;
const SYSTEMS = 2;
const ADDED = 3;

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
  const [focus, setFocus] = useState<Focus>({ row: 0, cols: [0, 0, 0, 0] });
  // Focus rings show once a pad or the keyboard is used, not for the mouse.
  const [navigating, setNavigating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [home, all] = await Promise.all([api.home(), api.systems()]);
      setFeed(home);
      setSystems(all.filter((s) => s.gameCount > 0));
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
      { length: systems.length, wraps: true },
      { length: feed?.added.length ?? 0, wraps: true },
    ],
    [feed, systems],
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
    },
    [feed, systems],
  );

  // Pad and keyboard. The handler reads the latest state through a ref.
  const latest = useRef({ focus, rows, accept });
  latest.current = { focus, rows, accept };
  useEffect(() => {
    const nav = listenNav(async (action) => {
      const { focus, rows, accept } = latest.current;
      if (action === "accept") return accept(focus);
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
        {systems.length > 0 && (
          <Row title="Systems" icon={<Gamepad2 />}>
            {systems.map((s, i) => (
              <SystemCard key={s.id} system={s} row={SYSTEMS} col={i} focused={focused(SYSTEMS, i)} onOpen={() => openSystem(s)} />
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
      </div>
      <HintBar hints={[{ button: "dpad", label: "Choose" }, { button: "a", label: "Open" }, { button: "start", label: "Rescan" }]} />
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
