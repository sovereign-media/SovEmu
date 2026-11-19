// The player: a splash while the session starts (Continue / Start over when there is an auto
// save), then the stream full-screen, with an in-game menu on Escape, the gamepad's Home button,
// or Select+Start. The menu saves and loads states into slots with screenshots.
//
// The session, input, display filter and menu stack are the framework-free classes in
// src/lib/; this component sets them up once and renders their state.
import {
  Activity,
  Disc3,
  ArrowLeft,
  CircleAlert,
  FolderOpen,
  Gamepad2,
  Keyboard,
  LogOut,
  MonitorCog,
  Play,
  Plus,
  RotateCcw,
  Save,
  ToggleLeft,
  ToggleRight,
  Undo2,
  Users,
  Wand2,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useReducer, useRef, useState, type SubmitEvent } from "react";
import { AUTO_SLOT, api, type ControllerMappingList, type Game, type PadLayout, type SaveSlot, type SessionInfo, type SystemControls, type SystemSummary } from "../../lib/api";
import { FILTERS, VideoFilter, type FilterMode } from "../../lib/filter";
import { defaultMapping, diagramKey, inputLabel, KEYBOARD, type Mapping, PAD_BUTTONS, rebind, toBindings, toMapping } from "../../lib/controllers";
import { captureInput, InputState } from "../../lib/input";
import { ForwardOnly, describeStart, newStartId, type StartStatus } from "../../lib/launch";
import { MenuStack, type Menu, type MenuItem } from "../../lib/menu";
import { listenNav } from "../../lib/nav";
import { type ButtonName, MAX_PORTS } from "../../lib/protocol";
import { Session } from "../../lib/session";
import { StatsSampler, type StatItem } from "../../lib/stats";
import { PadDiagram } from "../PadDiagram";
import Shell from "../Shell";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Dialog, Field, inputClass, Progress } from "../ui/primitives";
import { useToast } from "../ui/toast";

const ICONS: Record<string, LucideIcon> = {
  play: Play,
  restart: RotateCcw,
  back: ArrowLeft,
  resume: Undo2,
  save: Save,
  load: FolderOpen,
  cheats: Wand2,
  controls: Keyboard,
  display: MonitorCog,
  reset: RotateCcw,
  stats: Activity,
  quit: LogOut,
  add: Plus,
  on: ToggleRight,
  off: ToggleLeft,
  button: Gamepad2,
  keyboard: Keyboard,
  players: Users,
  disc: Disc3,
};

const SLOTS = [1, 2, 3, 4];
const FILTER_KEY = "sovemu.filter";
const player = (port: number) => `Player ${port + 1}`;

const ago = (unix: number) => {
  const s = Math.max(0, Math.round(Date.now() / 1000 - unix));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(unix * 1000).toLocaleDateString();
};

interface Loaded {
  game: Game;
  system: SystemSummary | undefined;
}

function Player() {
  const toast = useToast();
  const gameId = Number(new URLSearchParams(location.search).get("game"));
  const video = useRef<HTMLVideoElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const menus = useRef<MenuStack | null>(null);
  menus.current ??= new MenuStack(rerender);

  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [status, setStatus] = useState("Starting…");
  const [progress, setProgress] = useState<{ label: string; detail: string; percent: number; failed: boolean } | null>(null);
  const [splash, setSplash] = useState(true);
  const [playing, setPlaying] = useState(false);
  const [aspect, setAspect] = useState("4 / 3");
  const [filtered, setFiltered] = useState(false);
  const [stats, setStats] = useState<StatItem[] | null>(null);
  // Adding a cheat: a form, while which menus ignore the pad.
  const [cheatForm, setCheatForm] = useState<null | ((code: string, description: string) => void)>(null);
  const formOpen = useRef(false);
  formOpen.current = cheatForm !== null;

  useEffect(() => {
    const stack = menus.current!;
    let info: SessionInfo | null = null;
    let session: Session | null = null;
    let capturing = false;
    let statsOn = false;
    let polling = true;
    const input = new InputState();
    const sampler = new StatsSampler();
    const filter = new VideoFilter(video.current!, canvas.current!);
    let filterMode = (localStorage.getItem(FILTER_KEY) as FilterMode) || "sharp";
    const timers: number[] = [];
    let stopped = false;
    let cleanup = () => {};
    /** This game's system, its buttons and pad, the controllers' pads, and the every-system mappings. */
    let pads: {
      system: string;
      short: string;
      controls: SystemControls | null;
      layouts: PadLayout[];
      everywhere: ControllerMappingList;
    } = { system: "", short: "", controls: null, layouts: [], everywhere: [] };

    void (async () => {
      const [game, systems, initialSlots, iceServers] = await Promise.all([
        api.game(gameId),
        api.systems(),
        api.saves(gameId),
        api.iceServers(),
      ]);
      // The mappings in force on this system (and on every system, for the controls menu), and
      // the pads to draw there.
      const [mappings, everywhere, controls] = await Promise.all([
        api.controllerMappings(game.system).catch(() => []),
        api.controllerMappings(null).catch(() => []),
        api.systemControls(game.system).catch(() => null),
      ]);
      pads = { system: game.system, short: systems.find((s) => s.id === game.system)?.short ?? game.system, controls: controls?.controls ?? null, layouts: controls?.layouts ?? [], everywhere };
      const system = systems.find((s) => s.id === game.system);
      document.title = `${game.title} · SovEmu`;
      setLoaded({ game, system });
      input.analogSticks = !!system?.needsGpu;
      input.setMappings(mappings);
      if (filter.supported) {
        setFiltered(true);
        filter.setMode(filterMode);
      }

      // --- The session: start at once (the splash's choices show meanwhile), and follow the
      // start on the bar.
      const startId = newStartId();
      const bar = new ForwardOnly();
      const showProgress = (p: StartStatus) => {
        const view = describeStart(p);
        setProgress({ label: view.label, detail: view.detail, percent: Math.round(bar.next(view.fraction) * 100), failed: p.phase === "FAILED" });
      };
      void (async () => {
        while (polling) {
          const p = await api.startProgress(startId).catch(() => null);
          if (!polling) break;
          if (p) showProgress(p);
          await new Promise((r) => setTimeout(r, 250));
        }
      })();
      const starting = api.startSession(gameId, startId).then((s) => {
        polling = false;
        showProgress({ phase: "READY", elapsedMs: 0, phaseMs: 0 });
        timers.push(window.setTimeout(() => setProgress(null), 600));
        info = s;
        setAspect(["gb", "gbc", "gba", "gamegear"].includes(game.system) ? `${s.width} / ${s.height}` : "4 / 3");
        setStatus(`${s.core} · ${s.width}×${s.height} @ ${s.fps.toFixed(2)} fps`);
        return s;
      });
      starting.catch((e) => {
        polling = false;
        showProgress({ phase: "FAILED", elapsedMs: 0, phaseMs: 0, error: (e as Error).message });
        setProgress((p) => p && { ...p, percent: 0 });
      });

      async function begin(slot: number | null) {
        let s: SessionInfo;
        try {
          s = await starting;
        } catch (e) {
          setStatus(`Could not start: ${(e as Error).message}`);
          return;
        }
        if (slot !== null) await api.loadState(s.id, slot).catch((e) => toast(`Could not load: ${e.message}`));
        stack.close();
        setSplash(false);
        session = new Session(
          (sdp) => api.answer(s.id, sdp),
          video.current!,
          input,
          {
            onState: (state) => {
              if (state === "connected") {
                filter.start();
                toast("Menu: Escape or Select+Start · Hold Tab / Select+R to fast-forward, Backspace / Select+L to rewind");
              }
              if (state === "failed" || state === "disconnected") toast(`Connection ${state}`);
            },
          },
          { iceServers },
        );
        setPlaying(true);
        await session.connect();
      }

      async function quit() {
        session?.close();
        if (info) {
          setStatus("Saving…");
          await api.stopSession(info.id).catch(() => {});
        }
        location.href = `/system/?id=${encodeURIComponent(game.system)}`;
      }

      // --- Menus
      const slotItems = (slots: SaveSlot[], numbers: number[], onPick: (slot: number) => void, emptyEnabled: boolean): MenuItem[] =>
        numbers.map((n) => {
          const s = slots.find((x) => x.slot === n);
          return {
            label: n === AUTO_SLOT ? "Auto save" : `Slot ${n}`,
            detail: s ? ago(s.savedAt) : "Empty",
            image: s?.screenshotUrl ?? "",
            disabled: !s && !emptyEnabled,
            run: () => onPick(n),
          };
        });

      async function saveMenu() {
        const slots = await api.saves(gameId);
        stack.push({
          title: "Save state",
          items: slotItems(slots, SLOTS, async (slot) => {
            if (!info) return;
            try {
              await api.saveState(info.id, slot);
              stack.close();
              toast(`Saved to slot ${slot}`);
            } catch (e) {
              toast(`Save failed: ${(e as Error).message}`);
            }
          }, true),
        });
      }

      async function loadMenu() {
        const slots = await api.saves(gameId);
        stack.push({
          title: "Load state",
          items: slotItems(slots, [AUTO_SLOT, ...SLOTS], async (slot) => {
            if (!info) return;
            try {
              await api.loadState(info.id, slot);
              stack.close();
              toast(slot === AUTO_SLOT ? "Loaded the auto save" : `Loaded slot ${slot}`);
            } catch (e) {
              toast(`Load failed: ${(e as Error).message}`);
            }
          }, false),
        });
      }

      const filterMenu = () =>
        stack.push({
          title: "Display filter",
          items: FILTERS.map(({ mode, label }) => ({
            label,
            detail: mode === filterMode ? "Selected" : "",
            run: () => {
              filterMode = mode;
              localStorage.setItem(FILTER_KEY, mode);
              filter.setMode(mode);
              stack.close();
              toast(label);
            },
          })),
        });

      // Controls: every connected device with its player; choosing one remaps it. Mappings are
      // the user's, per controller model, saved on the server (docs/CONTROLLERS.md).
      const controlsMenu = (): Menu => ({
        title: "Controls: choose a controller",
        items: input.devices().map((d) => ({
          label: d.info.name,
          icon: d.key === KEYBOARD ? "keyboard" : "button",
          detail: player(d.port),
          run: () => stack.push(deviceMenu(d.key)),
        })),
      });

      /** Which mappings a device's menu edits: this system's, or every system's. */
      const scopes = new Map<string, "system" | "all">();

      const deviceMenu = (key: string): Menu => {
        const device = input.devices().find((d) => d.key === key);
        if (!device) {
          return { title: "That controller is no longer connected", items: [{ label: "Back", icon: "back", run: () => stack.pop() }] };
        }
        const { id, name, family } = device.info;
        const keyboard = key === KEYBOARD;
        const labels = keyboard ? null : family;
        const scope = scopes.get(key) ?? "system";
        const onSystem = scope === "system";
        const forSystem = (m: { controller: string }) => m.controller === id;
        // The mapping being edited: in force on this system, or the every-system one.
        const current = onSystem ? input.inForce(id) : (pads.everywhere.find(forSystem) ?? null);
        const mapping: Mapping = current ? toMapping(current.bindings) : defaultMapping(id);
        const where =
          !current ? "Built-in default"
          : current.source !== "USER" ? (current.system ? `Default for ${pads.short}` : "Default")
          : current.system ? `Yours for ${pads.short}` : "Yours for all systems";
        const refresh = async () => {
          const [here, all] = await Promise.all([api.controllerMappings(pads.system), api.controllerMappings(null)]);
          input.setMappings(here);
          pads.everywhere = all;
        };
        const save = async (next: Mapping | null) => {
          try {
            const system = onSystem ? pads.system : null;
            if (next) await api.saveControllerMapping(id, system, name, toBindings(next));
            else await api.resetControllerMapping(id, system);
            await refresh();
          } catch (e) {
            toast(`Not saved: ${(e as Error).message}`);
          }
        };
        const buttons: [ButtonName, string][] =
          onSystem && pads.controls ? pads.controls.buttons.map((b) => [b.button, b.label]) : PAD_BUTTONS;
        const controllerPad = keyboard ? null : (pads.layouts.find((l) => l.id === family) ?? null);
        return {
          title: `Remap ${name}`,
          diagrams: { system: onSystem ? pads.controls?.layout : null, controller: controllerPad },
          items: [
            {
              label: onSystem ? `For ${pads.short} only` : "For all systems",
              icon: "controls",
              detail: `In force: ${where} · choose to change`,
              run: () => {
                scopes.set(key, onSystem ? "all" : "system");
                stack.replace(deviceMenu(key));
              },
            },
            {
              label: "Player",
              icon: "players",
              detail: `${player(device.port)}: choose to change`,
              run: () => {
                input.setPort(key, (device.port + 1) % MAX_PORTS);
                stack.replace(deviceMenu(key));
              },
            },
            ...buttons.map(([button, label]) => {
              const inputs = Object.entries(mapping).filter(([, b]) => b === button).map(([i]) => i);
              return {
                label,
                icon: "button",
                detail: inputs.map((i) => inputLabel(i, labels)).join(", ") || "Not bound",
                highlight: { system: [button], controller: inputs.map(diagramKey) },
                run: async () => {
                  capturing = true;
                  toast(`Press ${keyboard ? "a key" : `a button on ${name}`} for ${label} · tap ${keyboard ? "Escape" : "Back"} to cancel, hold it to bind it`);
                  const pressed = await captureInput(key);
                  // Let the press that bound the button finish before menus react again.
                  setTimeout(() => (capturing = false), 250);
                  if (pressed) await save(rebind(mapping, pressed, button));
                  stack.replace(deviceMenu(key));
                },
              };
            }),
            {
              label: onSystem ? `Reset ${pads.short}'s controls` : "Reset all systems' controls",
              icon: "reset",
              disabled: current?.source !== "USER" || (onSystem && !current.system),
              run: async () => {
                await save(null);
                stack.replace(deviceMenu(key));
                toast(`${name}: ${onSystem ? `${pads.short} back to your controls for all systems, or the default` : "back to the default"}`);
              },
            },
          ],
        };
      };

      async function cheatsMenu(replace = false) {
        const cheats = await api.cheats(gameId);
        const menu: Menu = {
          title: "Cheats: choose one to turn it on or off",
          items: [
            ...cheats.map((c) => ({
              label: c.description || c.code,
              icon: c.enabled ? "on" : "off",
              detail: c.description ? c.code : "",
              run: async () => {
                await api.setCheat(gameId, c.id, !c.enabled);
                await cheatsMenu(true);
              },
            })),
            {
              label: "Add a cheat…",
              icon: "add",
              detail: "Game Genie, Pro Action Replay or the core's own format",
              run: () =>
                setCheatForm(() => async (code: string, description: string) => {
                  setCheatForm(null);
                  try {
                    await api.addCheat(gameId, code, description);
                    toast("Cheat added");
                  } catch (e) {
                    toast(`Could not add: ${(e as Error).message}`);
                  }
                  await cheatsMenu(true);
                }),
            },
          ],
        };
        if (replace) stack.replace(menu);
        else stack.push(menu);
      }

      // A multi-disc game: the discs, the one in the tray marked; picking another swaps it.
      const discMenu = (): Menu => ({
        title: "Change disc",
        items: Array.from({ length: info?.discs ?? 1 }, (_, i) => i + 1).map((n) => ({
          label: `Disc ${n}`,
          icon: "disc",
          detail: info?.disc === n ? "In the console" : "",
          run: async () => {
            if (!info) return;
            try {
              await api.setDisc(info.id, n);
              info = { ...info, disc: n };
              stack.close();
              toast(`Disc ${n} is in`);
            } catch (e) {
              toast(`Could not change disc: ${(e as Error).message}`);
            }
          },
        })),
      });

      const gameMenu = (): Menu => ({
        title: game.title,
        items: [
          { label: "Resume", icon: "resume", run: () => stack.close() },
          { label: "Save state", icon: "save", run: saveMenu },
          { label: "Load state", icon: "load", run: loadMenu },
          ...((info?.discs ?? 1) > 1
            ? [{ label: "Change disc", icon: "disc", detail: `Disc ${info?.disc} of ${info?.discs}`, run: () => stack.push(discMenu()) }]
            : []),
          { label: "Cheats", icon: "cheats", run: () => cheatsMenu() },
          { label: "Controls", icon: "controls", run: () => stack.push(controlsMenu()) },
          { label: "Display filter", icon: "display", detail: FILTERS.find((f) => f.mode === filterMode)?.label, run: filterMenu },
          {
            label: "Reset",
            icon: "reset",
            run: async () => {
              if (info) await api.resetSession(info.id);
              stack.close();
              toast("Reset");
            },
          },
          {
            label: statsOn ? "Hide stats" : "Show stats",
            icon: "stats",
            run: () => {
              statsOn = !statsOn;
              setStats(statsOn ? [] : null);
              stack.close();
            },
          },
          { label: "Quit game", icon: "quit", detail: "Saves your progress", run: quit },
        ],
      });

      // The splash: continue from the auto save when there is one.
      const auto = initialSlots.find((s) => s.slot === AUTO_SLOT);
      stack.push({
        title: auto ? "Welcome back" : "Ready",
        onBack: () => void quit(),
        items: auto
          ? [
              {
                label: "Continue",
                icon: "play",
                detail: `Auto save, ${ago(auto.savedAt)}`,
                image: auto.screenshotUrl ?? "",
                run: () => begin(AUTO_SLOT),
              },
              { label: "Start over", icon: "restart", run: () => begin(null) },
              { label: "Back", icon: "back", run: quit },
            ]
          : [
              { label: "Start", icon: "play", run: () => begin(null) },
              { label: "Back", icon: "back", run: quit },
            ],
      });

      // Chosen already on the game list (?start=continue or ?start=new): skip the question.
      const chosen = new URLSearchParams(location.search).get("start");
      if (chosen === "continue" && auto) void begin(AUTO_SLOT);
      else if (chosen === "new" || chosen === "continue") void begin(null);

      // Menu navigation. While playing with no menu open, game keys must not drive menus.
      // Escape both opens the menu and means Back: the press that opened it must not also
      // close it on the next frame.
      let swallowBack = false;
      const nav = listenNav((action) => {
        if (action === "back" && swallowBack) return void (swallowBack = false);
        if (capturing || formOpen.current || (session && !stack.open)) return;
        stack.handle(action);
      });

      // Opening the menu while playing: Escape, gamepad Home, or Select+Start together.
      const onKey = (e: KeyboardEvent) => {
        if (session && !stack.open && e.code === "Escape") {
          e.preventDefault();
          swallowBack = true;
          stack.push(gameMenu());
        }
      };
      addEventListener("keydown", onKey);
      let comboHeld = false;
      const watchPads = () => {
        if (stopped) return;
        if (session) {
          const combo = [...(navigator.getGamepads?.() ?? [])].some(
            (p) => p && (p.buttons[16]?.pressed || (p.buttons[8]?.pressed && p.buttons[9]?.pressed)),
          );
          if (combo && !comboHeld && !stack.open) stack.push(gameMenu());
          comboHeld = combo;
          session.suspended = stack.open;
        }
        requestAnimationFrame(watchPads);
      };
      requestAnimationFrame(watchPads);

      timers.push(
        window.setInterval(async () => {
          if (!session || !statsOn) return;
          setStats(await sampler.sample(session));
        }, 1000),
      );

      // Leaving the page ends the session (which writes the auto save) rather than waiting for
      // the idle timeout.
      addEventListener("pagehide", () => {
        if (info) api.stopSessionOnUnload(info.id);
      });

      cleanup = () => {
        nav.stop();
        removeEventListener("keydown", onKey);
      };
    })();

    return () => {
      stopped = true;
      polling = false;
      timers.forEach(clearTimeout);
      cleanup();
    };
  }, []);

  const stack = menus.current;
  const top = stack.current;
  const diagrams = top?.menu.diagrams && (top.menu.diagrams.system || top.menu.diagrams.controller) ? top.menu.diagrams : null;
  const lit = top?.menu.items[top.index]?.highlight;
  const color = loaded?.system?.color ?? "#4169e1";
  const submitCheat = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const code = String(data.get("code")).trim();
    if (code) cheatForm?.(code, String(data.get("description")).trim());
  };

  return (
    <main className="fixed inset-0 grid place-items-center bg-black">
      <div className="relative h-full max-h-dvh w-auto max-w-full" style={{ aspectRatio: aspect }}>
        <video ref={video} autoPlay playsInline className={cn("pixelated absolute inset-0 size-full object-fill", filtered && "opacity-0")} />
        <canvas ref={canvas} className={cn("absolute inset-0 size-full", !filtered && "hidden")} />
      </div>

      {splash && (
        <section
          className="absolute inset-0 grid content-start justify-items-center bg-surface pt-[12vh]"
          style={{ background: `radial-gradient(ellipse at 50% 40%, color-mix(in srgb, ${color} 40%, transparent), #0b0d12 70%)` }}
        >
          <div className="grid justify-items-center gap-3 text-center">
            {loaded && (
              <span className="rounded-lg px-3 py-1 text-sm font-black" style={{ background: color }}>
                {loaded.system?.short ?? loaded.game.system}
              </span>
            )}
            <h1 className="max-w-3xl text-4xl font-black md:text-5xl">{loaded?.game.title}</h1>
            <p className="text-slate-400">{status}</p>
          </div>
          {/* The loading bar: what the start is doing, how far it has got, and for how long. */}
          {progress && (
            <div className="absolute inset-x-0 bottom-10 mx-auto grid w-[28rem] max-w-[86vw] gap-2">
              <div className="flex items-baseline justify-between gap-4 text-sm">
                <span className={cn("flex items-center gap-1.5 font-semibold", progress.failed ? "text-red-300" : "text-slate-200")}>
                  {progress.failed && <CircleAlert className="size-4" />}
                  {progress.label}
                </span>
                <span className="text-xs text-slate-500 tabular-nums">{progress.detail}</span>
              </div>
              <Progress value={progress.percent} color={color} className="h-1.5 bg-white/10" label={progress.label} />
            </div>
          )}
        </section>
      )}

      {/* The menu stack: splash choices, in-game menu, slot pickers. Navigation is the stack's
          (pad and keyboard through listenNav), so Radix's own Escape and outside clicks are off. */}
      <Dialog
        open={!!top && !cheatForm}
        title={top?.menu.title ?? ""}
        dim={playing}
        className={diagrams ? "w-[56rem]" : undefined}
        contentProps={{
          onEscapeKeyDown: (e) => e.preventDefault(),
          onInteractOutside: (e) => e.preventDefault(),
          onOpenAutoFocus: (e) => e.preventDefault(),
        }}
      >
        <div className={cn(diagrams && "grid gap-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]")}>
        <ul className="grid max-h-[70vh] content-start gap-1 overflow-y-auto" role="listbox" aria-label={top?.menu.title}>
          {top?.menu.items.map((item, i) => {
            const Icon = item.icon ? ICONS[item.icon] : null;
            const on = i === top.index;
            return (
              <li
                key={`${item.label}-${i}`}
                role="option"
                aria-selected={on}
                aria-disabled={item.disabled}
                onClick={() => stack.activate(i)}
                className={cn("flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2.5", item.disabled && "cursor-default opacity-40", !on && "hover:bg-white/5")}
                style={on ? { background: color } : undefined}
              >
                {item.image !== undefined ? (
                  <div className="pixelated grid h-12 w-16 shrink-0 place-items-center overflow-hidden rounded bg-black/50 text-xs text-slate-500">
                    {item.image ? <img src={item.image} alt="" className="pixelated size-full object-cover" /> : "—"}
                  </div>
                ) : (
                  Icon && <Icon className="size-5 shrink-0 opacity-80" />
                )}
                <div className="min-w-0">
                  <div className="truncate text-lg">{item.label}</div>
                  {item.detail && <div className="truncate text-xs text-slate-300">{item.detail}</div>}
                </div>
              </li>
            );
          })}
        </ul>
        {/* The console's pad and the player's controller, the selected button lit on both. */}
        {diagrams && (
          <div className="hidden content-start gap-3 md:grid" aria-hidden>
            {diagrams.system && (
              <PadDiagram layout={diagrams.system} highlight={lit?.system} color={color} caption={`${loaded?.system?.short ?? ""} pad`} />
            )}
            {diagrams.controller && (
              <PadDiagram layout={diagrams.controller} highlight={lit?.controller} color={color} caption="Your controller" />
            )}
          </div>
        )}
        </div>
      </Dialog>

      <Dialog open={!!cheatForm} onOpenChange={(open) => !open && setCheatForm(null)} title="Add a cheat" className="w-[28rem] p-5">
        <form onSubmit={submitCheat} className="grid gap-4 px-3 pb-2">
          <Field label="Cheat code">
            <input name="code" required autoFocus autoComplete="off" spellCheck={false} className={cn(inputClass, "font-mono")} />
          </Field>
          <Field label="What does it do? (optional)">
            <input name="description" autoComplete="off" className={inputClass} />
          </Field>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={() => setCheatForm(null)}>
              Cancel
            </Button>
            <Button type="submit">
              <Plus /> Add
            </Button>
          </div>
        </form>
      </Dialog>

      {stats && (
        <dl className="pointer-events-none absolute top-4 left-4 grid grid-cols-[auto_auto] gap-x-4 gap-y-0.5 rounded-lg bg-black/70 px-3 py-2 font-mono text-xs">
          {stats.map(({ label, value }) => (
            <div key={label} className="contents">
              <dt className="text-slate-400">{label}</dt>
              <dd className="text-right tabular-nums">{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </main>
  );
}

export default function PlayPage() {
  return (
    <Shell>
      <Player />
    </Shell>
  );
}
