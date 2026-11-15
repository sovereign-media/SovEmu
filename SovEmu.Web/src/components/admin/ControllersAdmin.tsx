// Controllers: the default button mapping for each controller model, which every user starts
// from until they remap it themselves in the player (docs/CONTROLLERS.md). A dropdown picks the
// model (the keyboard, the popular models, any connected controller and any model that already
// has a default); each console button then gets an input, chosen from the model's buttons
// (labelled as it prints them, so no controller needs to be connected) or, for the keyboard,
// by pressing the key. Defaults from the server's environment win and are shown locked; the
// admin's can be copied out as environment lines.
import { Check, Copy, Gamepad2, Info, Keyboard, Lock, RotateCcw, Save, Terminal } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type ControllerDefaults } from "../../lib/api";
import {
  defaultMapping,
  familyOf,
  identify,
  inputLabel,
  KEYBOARD,
  KNOWN_CONTROLLERS,
  type Mapping,
  PAD_BUTTONS,
  PAD_INPUTS,
  POSITIONS,
  rebind,
  toBindings,
} from "../../lib/controllers";
import { captureInput } from "../../lib/input";
import type { ButtonName } from "../../lib/protocol";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { Button } from "../ui/button";
import { cn } from "../ui/cn";
import { panel, Select, Tooltip } from "../ui/primitives";

const NOT_BOUND = "";

/** The controllers to offer: known models, connected pads, and anything with a default. */
function useChoices(defaults: ControllerDefaults | null) {
  const [connected, setConnected] = useState<{ id: string; name: string }[]>([]);
  useEffect(() => {
    const read = () =>
      setConnected(
        [...(navigator.getGamepads?.() ?? [])]
          .filter((p): p is Gamepad => !!p)
          .map((p) => identify(p.id))
          .map(({ id, name }) => ({ id, name })),
      );
    read();
    addEventListener("gamepadconnected", read);
    addEventListener("gamepaddisconnected", read);
    return () => {
      removeEventListener("gamepadconnected", read);
      removeEventListener("gamepaddisconnected", read);
    };
  }, []);
  return useMemo(() => {
    const out = new Map<string, string>(KNOWN_CONTROLLERS.map((c) => [c.id, c.name]));
    for (const c of connected) if (!out.has(c.id)) out.set(c.id, `${c.name} (connected)`);
    for (const m of defaults?.mappings ?? []) if (!out.has(m.controller)) out.set(m.controller, m.name || m.controller);
    return [...out].map(([value, label]) => ({ value, label }));
  }, [connected, defaults]);
}

function ButtonRow({
  button,
  label,
  mapping,
  controller,
  locked,
  onBind,
}: {
  button: ButtonName;
  label: string;
  mapping: Mapping;
  controller: string;
  locked: boolean;
  onBind: (input: string | null) => void;
}) {
  const keyboard = controller === KEYBOARD;
  const family = familyOf(controller);
  const inputs = Object.entries(mapping).filter(([, b]) => b === button).map(([i]) => i);
  const [waiting, setWaiting] = useState(false);
  const press = async () => {
    setWaiting(true);
    const key = await captureInput(KEYBOARD);
    setWaiting(false);
    if (key) onBind(key);
  };
  return (
    <li className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-2.5">
      <span className="font-medium">{label}</span>
      {keyboard ? (
        <span className="flex items-center gap-2">
          <Button variant="outline" size="sm" disabled={locked} onClick={press} className="min-w-28">
            <Keyboard /> {waiting ? "Press a key (tap Escape to cancel)…" : inputs.map((i) => inputLabel(i, null)).join(", ") || "Not bound"}
          </Button>
          {inputs.length > 0 && !locked && (
            <Tooltip content="Unbind">
              <Button variant="ghost" size="icon" aria-label={`Unbind ${label}`} onClick={() => onBind(null)}>
                <RotateCcw />
              </Button>
            </Tooltip>
          )}
        </span>
      ) : (
        <Select
          label={label}
          value={inputs[0] ?? NOT_BOUND}
          disabled={locked}
          className="w-56 max-w-full"
          options={[
            { value: NOT_BOUND, label: "Not bound" },
            ...PAD_INPUTS.map((i) => ({
              value: i,
              label: POSITIONS[i] ? `${inputLabel(i, family)} (${POSITIONS[i]})` : inputLabel(i, family),
            })),
          ]}
          onChange={(v) => onBind(v === NOT_BOUND ? null : v)}
        />
      )}
    </li>
  );
}

function Controllers() {
  const [defaults, setDefaults] = useState<ControllerDefaults | null>(null);
  const [controller, setController] = useState(KEYBOARD);
  const [draft, setDraft] = useState<Mapping | null>(null);
  const [message, setMessage] = useState<{ text: string; error?: boolean } | null>(null);
  const [copied, setCopied] = useState(false);
  const choices = useChoices(defaults);

  const load = useCallback(async () => {
    try {
      setDefaults(await api.controllerDefaults());
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  }, []);
  useEffect(() => void load(), [load]);

  const current = defaults?.mappings.find((m) => m.controller === controller) ?? null;
  const saved: Mapping = current
    ? (Object.fromEntries(current.bindings.map((b) => [b.input, b.button])) as Mapping)
    : defaultMapping(controller);
  const mapping = draft ?? saved;
  const locked = current?.source === "ENVIRONMENT";
  const name = choices.find((c) => c.value === controller)?.label.replace(/ \(connected\)$/, "") ?? controller;

  const choose = (id: string) => {
    setController(id);
    setDraft(null);
    setMessage(null);
  };
  const bind = (button: ButtonName, input: string | null) => {
    setDraft(input ? rebind(mapping, input, button) : Object.fromEntries(Object.entries(mapping).filter(([, b]) => b !== button)) as Mapping);
    setMessage(null);
  };
  const save = async () => {
    try {
      await api.saveControllerDefault(controller, name, toBindings(mapping));
      setDraft(null);
      await load();
      setMessage({ text: "Saved. Players without their own mapping get it in their next game." });
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  };
  const reset = async () => {
    try {
      await api.resetControllerDefault(controller);
      setDraft(null);
      await load();
      setMessage({ text: "Back to the built-in default." });
    } catch (e) {
      setMessage({ text: (e as Error).message, error: true });
    }
  };
  const copy = async () => {
    await navigator.clipboard.writeText(defaults?.environment.join("\n") ?? "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const source = locked ? "Set in the environment" : current?.source === "ADMIN" ? "Your default" : "Built-in default";

  return (
    <main className="mx-auto grid max-w-4xl grid-cols-1 gap-6 px-6 pt-16 pb-16">
      <section className={panel}>
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h2 className="flex items-center gap-2 text-xl font-bold">
              <Gamepad2 className="size-5 text-slate-400" /> Controllers
            </h2>
            <p className="mt-1 max-w-prose text-sm text-slate-400">
              The buttons every player starts with, per controller model. Players can still remap their own in a game's
              Controls menu, which wins over these.
            </p>
          </div>
          <Select label="Controller" value={controller} options={choices} onChange={choose} className="w-64" />
        </div>
        <p className="mt-4 flex items-center gap-2 text-sm text-slate-400">
          {locked && <Lock className="size-4 text-amber-300" />}
          <span className={cn(locked && "text-amber-200")}>{source}</span>
          <span className="font-mono text-xs text-slate-600">{controller}</span>
        </p>
        {locked && (
          <p className="mt-2 flex items-start gap-2 text-sm text-slate-400">
            <Info className="mt-0.5 size-4 shrink-0" /> This controller's default comes from the server's environment
            (SOVEMU_CONTROLLERS), which always wins. Change it there.
          </p>
        )}
        {message && <p className={cn("mt-3 text-sm", message.error ? "text-red-300" : "text-emerald-300")}>{message.text}</p>}
      </section>

      <section className={panel}>
        <ul className="divide-y divide-white/5">
          {PAD_BUTTONS.map(([button, label]) => (
            <ButtonRow
              key={button}
              button={button}
              label={label}
              mapping={mapping}
              controller={controller}
              locked={locked}
              onBind={(input) => bind(button, input)}
            />
          ))}
        </ul>
        {!locked && (
          // Stays in view at the bottom while the buttons scroll.
          <div className="sticky bottom-0 -mx-6 -mb-6 mt-4 flex flex-wrap gap-2 rounded-b-2xl border-t border-white/10 bg-panel/95 px-6 py-4 backdrop-blur">
            <Button onClick={save} disabled={!draft}>
              <Save /> Save as the default
            </Button>
            {draft && (
              <Button variant="ghost" onClick={() => setDraft(null)}>
                Discard changes
              </Button>
            )}
            {current?.source === "ADMIN" && (
              <Button variant="outline" onClick={reset}>
                <RotateCcw /> Back to the built-in default
              </Button>
            )}
          </div>
        )}
      </section>

      {defaults && defaults.environment.length > 0 && (
        <section className={panel}>
          <div className="flex items-center justify-between gap-4">
            <h3 className="flex items-center gap-2 text-lg font-bold">
              <Terminal className="size-5 text-slate-400" /> As environment variables
            </h3>
            <Button variant="outline" onClick={copy}>
              {copied ? <Check /> : <Copy />} {copied ? "Copied" : "Copy"}
            </Button>
          </div>
          <p className="mt-2 text-sm text-slate-400">
            The defaults set here, as server settings. Put them in <code>.env</code> or the Helm values to make them part of
            the configuration; defaults from the environment always win.
          </p>
          <pre className="mt-3 overflow-x-auto rounded-lg bg-black/40 p-3 text-xs">{defaults.environment.join("\n")}</pre>
        </section>
      )}
    </main>
  );
}

export default function ControllersAdmin() {
  return (
    <Shell>
      <SideNav section="admin" />
      <div className={NAV_SPACE}>
        <Controllers />
      </div>
    </Shell>
  );
}
