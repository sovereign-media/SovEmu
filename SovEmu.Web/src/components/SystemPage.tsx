// One system's games: a list on the left, the focused game's details on the right. Up/down
// choose, left/right (and the shoulder buttons) page, A plays, B goes home.
import { CalendarDays, Clock, FileArchive, HardDrive, Play, Repeat, Tag, Users } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, formatAgo, formatSize, mediaUrl, type Game, type SystemSummary } from "../lib/api";
import { listenNav } from "../lib/nav";
import HintBar from "./HintBar";
import Shell from "./Shell";
import TopBar from "./TopBar";
import { Button } from "./ui/button";
import { ScrollArea } from "./ui/primitives";

const PAGE = 10;

/** Box art (or the composite image) at once; the video snap after a short pause, as on a
 *  console frontend, so scrolling quickly does not start a dozen videos. */
function Art({ game, color, short }: { game: Game; color: string; short: string }) {
  const [video, setVideo] = useState(false);
  const [playing, setPlaying] = useState(false);
  useEffect(() => {
    setVideo(false);
    setPlaying(false);
    if (!game.media.includes("video")) return;
    const t = setTimeout(() => setVideo(true), 900);
    return () => clearTimeout(t);
  }, [game.id]);
  const still = game.media.includes("image") ? "image" : game.media.includes("thumb") ? "thumb" : null;
  return (
    <div
      className="relative mb-6 grid aspect-[4/3] place-items-center overflow-hidden rounded-xl text-6xl font-black text-white/90"
      style={{ background: `linear-gradient(145deg, ${color}, color-mix(in srgb, ${color} 30%, black))` }}
    >
      <span>{short}</span>
      {still && <img key={game.id} src={mediaUrl(game.id, still)} alt="" className="absolute inset-0 size-full object-contain" />}
      {video && (
        <video
          src={mediaUrl(game.id, "video")}
          autoPlay
          muted
          loop
          playsInline
          onPlaying={() => setPlaying(true)}
          className={`absolute inset-0 size-full object-contain ${playing ? "" : "hidden"}`}
        />
      )}
    </div>
  );
}

function Fact({ icon, label, children }: { icon: ReactNode; label: string; children: ReactNode }) {
  return (
    <>
      <dt className="flex items-center gap-2 text-slate-400 [&_svg]:size-4">
        {icon}
        {label}
      </dt>
      <dd className="min-w-0">{children}</dd>
    </>
  );
}

function Games() {
  const systemId = new URLSearchParams(location.search).get("id") ?? "";
  const rememberKey = `sovemu.game.${systemId}`;
  const [system, setSystem] = useState<SystemSummary | null>(null);
  const [games, setGames] = useState<Game[]>([]);
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    Promise.all([api.systems(), api.games(systemId)]).then(([systems, games]) => {
      const s = systems.find((x) => x.id === systemId);
      if (!s) return location.replace("/");
      document.title = `${s.name} · SovEmu`;
      setSystem(s);
      setGames(games);
      setIndex(Math.max(0, games.findIndex((g) => String(g.id) === localStorage.getItem(rememberKey))));
    });
  }, []);

  const select = (i: number) => setIndex(Math.min(Math.max(i, 0), Math.max(0, games.length - 1)));
  const play = (g?: Game) => g && (location.href = `/play/?game=${g.id}`);

  useEffect(() => {
    const g = games[index];
    if (!g) return;
    localStorage.setItem(rememberKey, String(g.id));
    list.current?.children[index]?.scrollIntoView({ block: "nearest" });
  }, [index, games]);

  const latest = useRef({ index, games });
  latest.current = { index, games };
  useEffect(() => {
    const nav = listenNav((action) => {
      const { index, games } = latest.current;
      const to = (i: number) => setIndex(Math.min(Math.max(i, 0), Math.max(0, games.length - 1)));
      if (action === "up") to(index - 1);
      if (action === "down") to(index + 1);
      if (action === "pageUp" || action === "left") to(index - PAGE);
      if (action === "pageDown" || action === "right") to(index + PAGE);
      if (action === "accept") play(games[index]);
      if (action === "back") location.href = "/";
    });
    return nav.stop;
  }, []);

  if (!system) return null;
  const color = system.color;
  const g = games[index];
  const year = g?.release_date?.slice(0, 4);
  return (
    <>
      <div
        className="fixed inset-0"
        style={{ background: `radial-gradient(ellipse at 0% 0%, color-mix(in srgb, ${color} 35%, transparent), #0b0d12 60%)` }}
      />
      <main className="relative mx-auto grid h-dvh max-w-7xl grid-cols-1 gap-8 px-8 pt-24 pb-24 md:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]">
        <section className="flex min-h-0 flex-col">
          <h1 className="mb-4 flex items-baseline gap-3">
            <span className="rounded-lg px-2.5 py-1 text-sm font-black" style={{ background: color }}>
              {system.short}
            </span>
            <span className="text-2xl font-bold">{system.name}</span>
            <span className="text-sm text-slate-400">
              {games.length} game{games.length === 1 ? "" : "s"}
            </span>
          </h1>
          <ScrollArea className="min-h-0 flex-1 rounded-xl bg-black/30">
            <ol ref={list} className="p-2">
              {games.map((x, i) => (
                <li
                  key={x.id}
                  onClick={() => (i === index ? play(x) : select(i))}
                  className={`flex cursor-pointer items-center gap-3 rounded-lg px-4 py-2.5 text-lg transition-colors ${i === index ? "font-semibold" : "text-slate-300 hover:bg-white/5"}`}
                  style={i === index ? { background: color } : undefined}
                >
                  <span className="truncate">{x.title}</span>
                  {x.tags && <span className="ml-auto shrink-0 text-xs text-slate-400">{x.tags}</span>}
                </li>
              ))}
            </ol>
          </ScrollArea>
        </section>

        {g && (
          <aside className="hidden self-center rounded-2xl border border-white/10 bg-panel/70 p-8 shadow-2xl backdrop-blur md:block">
            <Art game={g} color={color} short={system.short} />
            <h2 className="text-3xl leading-tight font-black">{g.title}</h2>
            <p className="mt-1 text-sm text-slate-400">
              {[year, g.developer, g.publisher && g.publisher !== g.developer ? g.publisher : null].filter(Boolean).join(" · ")}
            </p>
            {g.tags && (
              <div className="mt-3 flex flex-wrap gap-2">
                {g.tags
                  .split(",")
                  .map((t) => t.trim())
                  .filter(Boolean)
                  .map((t) => (
                    <span key={t} className="rounded-full bg-white/10 px-2.5 py-0.5 text-xs">
                      {t}
                    </span>
                  ))}
              </div>
            )}
            {g.description && <p className="mt-4 line-clamp-5 text-sm leading-relaxed text-slate-300">{g.description}</p>}
            <dl className="mt-6 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
              {g.genre && <Fact icon={<Tag />} label="Genre">{g.genre}</Fact>}
              {g.players && <Fact icon={<Users />} label="Players">{g.players}</Fact>}
              {year && <Fact icon={<CalendarDays />} label="Released">{g.release_date}</Fact>}
              <Fact icon={<Clock />} label="Last played">{formatAgo(g.last_played_at)}</Fact>
              <Fact icon={<Repeat />} label="Times played">{g.play_count ?? 0}</Fact>
              <Fact icon={<FileArchive />} label="File">
                <span className="block truncate font-mono text-xs leading-5">{g.path}</span>
              </Fact>
              <Fact icon={<HardDrive />} label="Size">{formatSize(g.size)}</Fact>
            </dl>
            <Button className="mt-6" size="lg" onClick={() => play(g)}>
              <Play className="fill-current" /> Play
            </Button>
          </aside>
        )}
      </main>
      <HintBar hints={[{ button: "dpad", label: "Choose" }, { button: "a", label: "Play" }, { button: "b", label: "Home" }]} />
    </>
  );
}

export default function SystemPage() {
  return (
    <Shell>
      <TopBar crumb="Games" />
      <Games />
    </Shell>
  );
}
