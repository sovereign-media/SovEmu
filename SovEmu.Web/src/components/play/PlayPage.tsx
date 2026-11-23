// The player: a splash while the session starts (Continue / Start over when there is an auto
// save), then the stream full-screen, with an in-game menu on Escape, the gamepad's Home button,
// or Select+Start. The menu saves and loads states into slots with screenshots.
//
// The session, input, display filter and menu stack are the framework-free classes in
// src/lib/; this component sets them up once and renders their state.

import {
  Activity,
  ArrowLeft,
  CircleAlert,
  Disc3,
  FolderOpen,
  Gamepad2,
  Keyboard,
  LogOut,
  type LucideIcon,
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
} from "lucide-react";
import { type SubmitEvent, useEffect, useReducer, useRef, useState } from "react";
import {
  AUTO_SLOT,
  api,
  type ControllerMappingList,
  type Game,
  type PadLayout,
  type SaveSlot,
  type SessionInfo,
  type SystemControls,
  type SystemSummary,
} from "../../lib/api";
import { clientCapabilities } from "../../lib/capabilities";
import { clientName, reportClientEvent } from "../../lib/clientEvents";
import {
  defaultMapping,
  diagramKey,
  inputLabel,
  KEYBOARD,
  type Mapping,
  PAD_BUTTONS,
  rebind,
  toBindings,
  toMapping,
} from "../../lib/controllers";
import { invalidate, q, queryClient } from "../../lib/data";
import { type Decoration, parseAspect, shouldFrame, windowInsets } from "../../lib/decoration";
import { FILTERS, type FilterMode, VideoFilter } from "../../lib/filter";
import { go } from "../../lib/go";
import { captureInput, InputState } from "../../lib/input";
import { LatencyHistory, type LatencyPoint, twoColumns } from "../../lib/latency";
import { describeFirstPicture, describeStart, ForwardOnly, newStartId, type StartStatus } from "../../lib/launch";
import { type Menu, type MenuItem, MenuStack } from "../../lib/menu";
import { listenNav } from "../../lib/nav";
import type { QualityNotice } from "../../lib/pipeline";
import { type ButtonName, MAX_PORTS } from "../../lib/protocol";
import { Session } from "../../lib/session";
import { type StatItem, StatsSampler } from "../../lib/stats";
import { ago } from "../../lib/time";
import { PadDiagram } from "../PadDiagram";
import Shell from "../Shell";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { Dialog, Field, inputClass, Progress } from "../ui/primitives";
import { useToast } from "../ui/toast";
import { LatencyGraph } from "./LatencyGraph";
import { QualityChip } from "./QualityChip";

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
/** Systems whose picture keeps its own shape rather than a TV's 4:3: the handhelds, and the
 *  Vectrex's portrait tube. */
const HANDHELDS = new Set([
  "gb",
  "gbc",
  "gba",
  "gamegear",
  "lynx",
  "ngp",
  "ngpc",
  "wswan",
  "wswanc",
  "virtualboy",
  "vectrex",
]);
const FILTER_KEY = "sovemu.filter";
const player = (port: number) => `Player ${port + 1}`;

interface Loaded {
  game: Game;
  system: SystemSummary | undefined;
  /** The artwork to frame the picture with, if its system has any. */
  decoration: Decoration | null;
}

/** The window's shape, kept up to date: framing depends on it. */
function useScreen(): { width: number; height: number } {
  const [screen, setScreen] = useState({ width: innerWidth, height: innerHeight });
  useEffect(() => {
    const resize = () => setScreen({ width: innerWidth, height: innerHeight });
    addEventListener("resize", resize);
    return () => removeEventListener("resize", resize);
  }, []);
  return screen;
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
  const [progress, setProgress] = useState<{ label: string; detail: string; percent: number; failed: boolean } | null>(
    null,
  );
  const [splash, setSplash] = useState(true);
  const [playing, setPlaying] = useState(false);
  const [aspect, setAspect] = useState("4 / 3");
  const screen = useScreen();
  const [filtered, setFiltered] = useState(false);
  const [stats, setStats] = useState<StatItem[] | null>(null);
  /** The worker changed the stream's quality: the notice at the top of the screen, for a moment. */
  const [quality, setQuality] = useState<QualityNotice | null>(null);
  // The graph's last minute, a copy of `history`'s points per sample so React redraws it.
  const [graph, setGraph] = useState<{ points: LatencyPoint[]; max: number }>({ points: [], max: 50 });
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
    let qualityTimer = 0;
    let polling = true;
    let everConnected = false;
    let lastFrames = 0;
    let stalledFor = 0;
    const input = new InputState();
    const sampler = new StatsSampler();
    const history = new LatencyHistory();
    const filter = new VideoFilter(video.current!, canvas.current!);
    let filterMode = (localStorage.getItem(FILTER_KEY) as FilterMode) || "sharp";
    const timers: number[] = [];
    let stopped = false;
    /** The server's session has been asked to stop (Quit, or leaving the screen): once only. */
    let ended = false;
    /** After a game: its saves (the auto save), the home's recent row and its play count. */
    const played = () => invalidate(["game", gameId], ["home"], ["system"]);
    const end = () => {
      if (ended || !info) return;
      ended = true;
      void api
        .stopSession(info.id)
        .catch(() => {})
        .then(played);
    };
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
        // Through the cache: coming from the games screen, most of this is already known.
        queryClient.fetchQuery(q.game(gameId)),
        queryClient.fetchQuery(q.systems()),
        api.saves(gameId),
        queryClient.fetchQuery(q.iceServers()),
      ]);
      if (stopped) return;
      // The mappings in force on this system (and on every system, for the controls menu), and
      // the pads to draw there.
      const [mappings, everywhere, controls] = await Promise.all([
        queryClient.fetchQuery(q.controllerMappings(game.system)).catch(() => []),
        queryClient.fetchQuery(q.controllerMappings(null)).catch(() => []),
        queryClient.fetchQuery(q.systemControls(game.system)).catch(() => null),
      ]);
      if (stopped) return;
      pads = {
        system: game.system,
        short: systems.find((s) => s.id === game.system)?.short ?? game.system,
        controls: controls?.controls ?? null,
        layouts: controls?.layouts ?? [],
        everywhere,
      };
      const system = systems.find((s) => s.id === game.system);
      document.title = `${game.title} · SovEmu`;
      setLoaded({ game, system, decoration: game.decoration ?? null });
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
        setProgress({
          label: view.label,
          detail: view.detail,
          percent: Math.round(bar.next(view.fraction) * 100),
          failed: p.phase === "FAILED",
        });
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
        // The bar stays for the last stretch: the game's first picture (`begin`).
        info = s;
        // The player left while the game was loading: it is not wanted any more.
        if (stopped) end();
        setAspect(HANDHELDS.has(game.system) ? `${s.width} / ${s.height}` : "4 / 3");
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
        if (stopped) return;
        if (slot !== null) await api.loadState(s.id, slot).catch((e) => toast(`Could not load: ${e.message}`));
        stack.close();
        // The splash stays until the game has drawn: a console's BIOS can boot for seconds, and
        // a save is only loaded once the game is up. The bar counts the wait meanwhile.
        const waitingFrom = performance.now();
        const showWaiting = () => {
          const view = describeFirstPicture(performance.now() - waitingFrom);
          setProgress({ label: view.label, detail: view.detail, percent: 100, failed: false });
        };
        showWaiting();
        const waiting = window.setInterval(showWaiting, 500);
        timers.push(waiting);
        const firstPicture = () => {
          window.clearInterval(waiting);
          setProgress(null);
          setSplash(false);
        };
        const v = video.current!;
        if (v.requestVideoFrameCallback) v.requestVideoFrameCallback(firstPicture);
        else v.addEventListener("loadeddata", firstPicture, { once: true });
        session = new Session(
          async (sdp) => api.answer(s.id, sdp, clientName(), await clientCapabilities()),
          video.current!,
          input,
          {
            onState: (state) => {
              if (state === "failed" || state === "disconnected") {
                const lost = everConnected ? "CONNECTION_LOST" : "CONNECTION_FAILED";
                reportClientEvent(lost, `WebRTC ${state}`, s.id);
              }
              if (state === "connected") {
                everConnected = true;
                filter.start();
                toast(
                  "Menu: Escape or Select+Start · Hold Tab / Select+R to fast-forward, Backspace / Select+L to rewind",
                );
              }
              if (state === "failed" || state === "disconnected") toast(`Connection ${state}`);
            },
            onQuality: (notice) => {
              setQuality(notice);
              window.clearTimeout(qualityTimer);
              qualityTimer = window.setTimeout(() => setQuality(null), QUALITY_NOTICE_MS);
            },
          },
          { iceServers },
        );
        setPlaying(true);
        await session.connect();
      }

      async function quit() {
        session?.close();
        if (info && !ended) {
          ended = true;
          setStatus("Saving…");
          await api.stopSession(info.id).catch(() => {});
          void played();
        }
        go(`/system/?id=${encodeURIComponent(game.system)}`);
      }

      // --- Menus
      const slotItems = (
        slots: SaveSlot[],
        numbers: number[],
        onPick: (slot: number) => void,
        emptyEnabled: boolean,
      ): MenuItem[] =>
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
          items: slotItems(
            slots,
            SLOTS,
            async (slot) => {
              if (!info) return;
              try {
                await api.saveState(info.id, slot);
                void invalidate(["game", gameId, "saves"]);
                stack.close();
                toast(`Saved to slot ${slot}`);
              } catch (e) {
                toast(`Save failed: ${(e as Error).message}`);
              }
            },
            true,
          ),
        });
      }

      async function loadMenu() {
        const slots = await api.saves(gameId);
        stack.push({
          title: "Load state",
          items: slotItems(
            slots,
            [AUTO_SLOT, ...SLOTS],
            async (slot) => {
              if (!info) return;
              try {
                await api.loadState(info.id, slot);
                stack.close();
                toast(slot === AUTO_SLOT ? "Loaded the auto save" : `Loaded slot ${slot}`);
              } catch (e) {
                toast(`Load failed: ${(e as Error).message}`);
              }
            },
            false,
          ),
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
          return {
            title: "That controller is no longer connected",
            items: [{ label: "Back", icon: "back", run: () => stack.pop() }],
          };
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
        const where = !current
          ? "Built-in default"
          : current.source !== "USER"
            ? current.system
              ? `Default for ${pads.short}`
              : "Default"
            : current.system
              ? `Yours for ${pads.short}`
              : "Yours for all systems";
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
            void invalidate(["controllerMappings"]);
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
              const inputs = Object.entries(mapping)
                .filter(([, b]) => b === button)
                .map(([i]) => i);
              return {
                label,
                icon: "button",
                detail: inputs.map((i) => inputLabel(i, labels)).join(", ") || "Not bound",
                highlight: { system: [button], controller: inputs.map(diagramKey) },
                run: async () => {
                  capturing = true;
                  toast(
                    `Press ${keyboard ? "a key" : `a button on ${name}`} for ${label} · tap ${keyboard ? "Escape" : "Back"} to cancel, hold it to bind it`,
                  );
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
                toast(
                  `${name}: ${onSystem ? `${pads.short} back to your controls for all systems, or the default` : "back to the default"}`,
                );
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
            ? [
                {
                  label: "Change disc",
                  icon: "disc",
                  detail: `Disc ${info?.disc} of ${info?.discs}`,
                  run: () => stack.push(discMenu()),
                },
              ]
            : []),
          { label: "Cheats", icon: "cheats", run: () => cheatsMenu() },
          { label: "Controls", icon: "controls", run: () => stack.push(controlsMenu()) },
          {
            label: "Display filter",
            icon: "display",
            detail: FILTERS.find((f) => f.mode === filterMode)?.label,
            run: filterMenu,
          },
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
            detail: STATS_WARNING,
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
          if (!session) return;
          // A picture that stops while playing (not paused in the menu) is worth a report.
          const frames = video.current?.getVideoPlaybackQuality?.().totalVideoFrames ?? 0;
          const stalled = everConnected && !stack.open && frames === lastFrames;
          stalledFor = stalled ? stalledFor + 1 : 0;
          lastFrames = frames;
          if (stalledFor === STALL_SECONDS && info) {
            reportClientEvent("STREAM_STALLED", `no new picture for ${STALL_SECONDS} s`, info.id);
          }
          // Sampled whether or not the overlay is open: the worker adapts the video bitrate to
          // how this client keeps up, and logs its view beside its own health lines.
          const { items, numbers } = await sampler.sample(session);
          session.sendStats({ ...numbers, client: clientName() });
          if (!statsOn) return;
          setStats(items);
          history.push({ measured: numbers.input_to_frame_ms ?? NaN, estimate: numbers.estimate_ms ?? NaN });
          setGraph({ points: [...history.points], max: history.scaleMax });
        }, 1000),
      );

      // Leaving the page ends the session (which writes the auto save) rather than waiting for
      // the idle timeout.
      const onPageHide = () => {
        if (info && !ended) {
          ended = true;
          api.stopSessionOnUnload(info.id);
        }
      };
      addEventListener("pagehide", onPageHide);

      cleanup = () => {
        nav.stop();
        removeEventListener("keydown", onKey);
        removeEventListener("pagehide", onPageHide);
      };
    })().catch((e: unknown) => {
      // The game could not be read (an unknown or missing `?game`, or the server unreachable):
      // say so where "Starting…" was, as a failed start does.
      if (!stopped) setStatus(`Could not start: ${e instanceof Error ? e.message : String(e)}`);
    });

    // Leaving the screen (Back, a link) ends the game as closing the page would: the stream, the
    // listeners, and the server's session, which writes the auto save.
    return () => {
      stopped = true;
      polling = false;
      timers.forEach(clearTimeout);
      clearTimeout(qualityTimer);
      cleanup();
      session?.close();
      filter.stop();
      input.dispose();
      end();
    };
  }, []);

  const stack = menus.current;
  const top = stack.current;
  const diagrams =
    top?.menu.diagrams && (top.menu.diagrams.system || top.menu.diagrams.controller) ? top.menu.diagrams : null;
  const lit = top?.menu.items[top.index]?.highlight;
  const color = loaded?.system?.color ?? "#4169e1";
  const frame =
    loaded?.decoration && shouldFrame(screen.width, screen.height, parseAspect(aspect)) ? loaded.decoration : null;
  const submitCheat = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const code = String(data.get("code")).trim();
    if (code) cheatForm?.(code, String(data.get("description")).trim());
  };

  return (
    <main className="fixed inset-0 grid place-items-center bg-black">
      {frame ? (
        // The decoration fitted to the screen, the picture (its shape kept) in its window, the
        // artwork drawn over the picture's edges.
        <div
          className="relative"
          style={{
            aspectRatio: `${frame.width} / ${frame.height}`,
            width: `min(100vw, calc(100dvh * ${frame.width} / ${frame.height}))`,
          }}
        >
          <div className="absolute" style={windowInsets(frame)}>
            <video
              ref={video}
              autoPlay
              playsInline
              className={cn("pixelated absolute inset-0 size-full object-contain", filtered && "opacity-0")}
            />
            <canvas ref={canvas} className={cn("absolute inset-0 size-full object-contain", !filtered && "hidden")} />
          </div>
          <img
            src={frame.url}
            alt=""
            draggable={false}
            className="pointer-events-none absolute inset-0 size-full select-none"
          />
        </div>
      ) : (
        <div className="relative h-full max-h-dvh w-auto max-w-full" style={{ aspectRatio: aspect }}>
          <video
            ref={video}
            autoPlay
            playsInline
            className={cn("pixelated absolute inset-0 size-full object-fill", filtered && "opacity-0")}
          />
          <canvas ref={canvas} className={cn("absolute inset-0 size-full", !filtered && "hidden")} />
        </div>
      )}

      {splash && (
        <section
          className="absolute inset-0 grid content-start justify-items-center bg-surface pt-[12vh]"
          style={{
            background: `radial-gradient(ellipse at 50% 40%, color-mix(in srgb, ${color} 40%, transparent), #0b0d12 70%)`,
          }}
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
                <span
                  className={cn(
                    "flex items-center gap-1.5 font-semibold",
                    progress.failed ? "text-red-300" : "text-slate-200",
                  )}
                >
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
          <ul
            className="grid max-h-[70vh] content-start gap-1 overflow-y-auto"
            role="listbox"
            aria-label={top?.menu.title}
          >
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
                  className={cn(
                    "flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2.5",
                    item.disabled && "cursor-default opacity-40",
                    !on && "hover:bg-white/5",
                  )}
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
                <PadDiagram
                  layout={diagrams.system}
                  highlight={lit?.system}
                  color={color}
                  caption={`${loaded?.system?.short ?? ""} pad`}
                />
              )}
              {diagrams.controller && (
                <PadDiagram
                  layout={diagrams.controller}
                  highlight={lit?.controller}
                  color={color}
                  caption="Your controller"
                />
              )}
            </div>
          )}
        </div>
      </Dialog>

      <Dialog
        open={!!cheatForm}
        onOpenChange={(open) => !open && setCheatForm(null)}
        title="Add a cheat"
        className="w-[28rem] p-5"
      >
        <form onSubmit={submitCheat} className="grid gap-4 px-3 pb-2">
          <Field label="Cheat code">
            <input
              name="code"
              required
              autoFocus
              autoComplete="off"
              spellCheck={false}
              className={cn(inputClass, "font-mono")}
            />
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

      {quality && <QualityChip notice={quality} />}

      {stats && (
        <div className="pointer-events-none absolute top-4 left-4 w-max max-w-[calc(100%-2rem)] rounded-lg bg-black/70 px-3 py-2 font-mono text-xs">
          {/* Two columns, the first filled before the second, to keep the panel short. */}
          <dl className="grid grid-cols-[auto_auto_auto_auto] gap-x-3 gap-y-0.5">
            {twoColumns(stats).map(([left, right]) => (
              <div key={left.label} className="contents">
                <dt className="text-slate-400">{left.label}</dt>
                <dd className="pr-3 text-right whitespace-nowrap tabular-nums">{left.value}</dd>
                {right ? (
                  <>
                    <dt className="text-slate-400">{right.label}</dt>
                    <dd className="text-right whitespace-nowrap tabular-nums">{right.value}</dd>
                  </>
                ) : (
                  <div className="col-span-2" />
                )}
              </div>
            ))}
          </dl>
          <LatencyGraph points={graph.points} capacity={60} max={graph.max} />
          <p className="mt-1 font-sans text-[10px] text-slate-400">
            <span className="text-sky-300">━</span> measured button→frame <span className="text-slate-400">━</span>{" "}
            estimated button→pixel, last minute. {STATS_WARNING}
          </p>
        </div>
      )}
    </main>
  );
}

/** Shown where stats are turned on and on the overlay: they cost a little every second. */
/** How long a notice of a change in quality stays at the top of the screen (Sovereign's 5 s). */
const QUALITY_NOTICE_MS = 5000;

const STATS_WARNING = "Showing stats can affect game performance.";

/** Seconds without a new video frame, while playing, before it is reported as a stall. */
const STALL_SECONDS = 3;

export default function PlayPage() {
  return (
    <Shell>
      <Player />
    </Shell>
  );
}
