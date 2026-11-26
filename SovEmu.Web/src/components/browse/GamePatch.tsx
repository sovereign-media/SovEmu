// A game's soft patch, on its panel: which patch it plays with (a translation, a fix, a hack;
// applied as it loads, never on disk), or the original. Shown only for a game that has patches.
// A patched game keeps its own saves, so changing it changes the saves listed.

import { useQuery } from "@tanstack/react-query";
import { FileDiff } from "lucide-react";
import { api, type Game, type Patch } from "../../lib/api";
import { invalidate, q } from "../../lib/data";
import { Select } from "../ui/primitives";
import { useToast } from "../ui/toast";

const ORIGINAL = "";

type GamePatchProps = { game: Game; onPatch: (name: string | null) => void };

export function GamePatch({ game, onPatch }: GamePatchProps) {
  const toast = useToast();
  const patches: Patch[] = useQuery(q.patches(game.id)).data ?? [];

  if (patches.length === 0) return null;
  const current = patches.find((p) => p.name === game.patch);

  const choose = async (value: string) => {
    const patch = patches.find((p) => String(p.id) === value) ?? null;
    const before = game.patch;
    onPatch(patch?.name ?? null);
    try {
      await api.setPatch(game.id, patch?.id ?? null);
      // A patched game's saves are its own; the list shows the patch.
      void invalidate(["game", game.id], ["system", game.system]);
    } catch (e) {
      onPatch(before);
      toast(`Could not change the patch: ${(e as Error).message}`);
    }
  };

  return (
    <div className="flex items-center gap-2 text-sm text-slate-300">
      <FileDiff className="size-4 text-slate-500" />
      <span>Patch</span>
      <Select
        label="Patch"
        value={current ? String(current.id) : ORIGINAL}
        options={[
          { value: ORIGINAL, label: "Original" },
          ...patches.map((p) => ({ value: String(p.id), label: p.isDefault ? `${p.name} (default)` : p.name })),
        ]}
        onChange={(v) => void choose(v)}
      />
    </div>
  );
}
