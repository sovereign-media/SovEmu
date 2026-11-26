// Search and filters for a system's games, in a drawer from the right (Sovereign's
// SearchFilterDrawer): search, played or not, favourites and, once game information has been
// fetched, players, rating, age rating, genre, play modes, series and themes. Each filter is a
// dropdown: one choice, or (for the lists) any number of values, each with how many games have it.
import { Search, SlidersHorizontal, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useMemo } from "react";
import type { Game } from "../../lib/api";
import { ESRB } from "../../lib/esrb";
import {
  activeFilters,
  esrbCounts,
  type GameFilters,
  knowsPlayers,
  knowsRatings,
  LIST_FILTERS,
  valueCounts,
} from "../../lib/games";
import { Field, MultiSelect, Select } from "../ui/primitives";

/** How many values a list filter offers at most: the most common. */
const MAX_VALUES = 60;

type FilterDrawerProps = {
  open: boolean;
  onClose: () => void;
  games: Game[];
  query: string;
  onQuery: (q: string) => void;
  filters: GameFilters;
  onFilters: (f: GameFilters) => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
};

export function FilterDrawer({
  open,
  onClose,
  games,
  query,
  onQuery,
  filters,
  onFilters,
  searchRef,
}: FilterDrawerProps) {
  const players = useMemo(() => knowsPlayers(games), [games]);
  const ratings = useMemo(() => knowsRatings(games), [games]);
  const esrb = useMemo(() => esrbCounts(games), [games]);
  const lists = useMemo(
    () =>
      LIST_FILTERS.map((f) => ({ ...f, counts: valueCounts(games, f.values).slice(0, MAX_VALUES) })).filter(
        (f) => f.counts.length > 0,
      ),
    [games],
  );
  const n = activeFilters(filters) + (query.trim() ? 1 : 0);
  const set = (change: Partial<GameFilters>) => onFilters({ ...filters, ...change });
  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/60 motion-safe:data-[state=open]:animate-fade-in motion-safe:data-[state=closed]:animate-fade-out" />
        <Dialog.Content
          // Escape in the search box closes the drawer; elsewhere the page's pad navigation does
          // (its Back), so one press is never handled twice.
          onEscapeKeyDown={(e) => !(e.target instanceof HTMLInputElement) && e.preventDefault()}
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            searchRef.current?.focus();
          }}
          className="fixed inset-y-0 right-0 z-50 flex w-[min(24rem,100vw)] flex-col overflow-hidden border-l border-white/10 bg-surface/95 shadow-2xl outline-none backdrop-blur-md motion-safe:data-[state=open]:animate-slide-in-right motion-safe:data-[state=closed]:animate-slide-out-right"
        >
          <div className="mx-5 flex items-center gap-3 border-b border-white/10 pt-5 pb-4">
            <SlidersHorizontal className="size-5 text-white" aria-hidden />
            <Dialog.Title className="text-lg font-semibold">Filters</Dialog.Title>
            {n > 0 && <span className="rounded-full bg-accent px-2.5 py-0.5 text-xs font-semibold">{n} on</span>}
            <Dialog.Close
              aria-label="Close"
              className="ml-auto grid size-9 place-items-center rounded-full text-slate-400 hover:bg-white/10 hover:text-white"
            >
              <X className="size-5" />
            </Dialog.Close>
          </div>
          <Dialog.Description className="sr-only">Search and filter this system's games.</Dialog.Description>
          <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-5 py-5">
            <label className="relative">
              <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-slate-500" />
              <input
                ref={searchRef}
                type="search"
                value={query}
                onChange={(e) => onQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && onClose()}
                placeholder="Search titles, makers, genres"
                aria-label="Search games"
                className="h-10 w-full rounded-full border border-white/15 bg-black/30 pr-4 pl-9 text-sm outline-none focus:border-accent"
              />
            </label>
            <div className="grid grid-cols-2 gap-x-3 gap-y-4">
              <Field label="Played">
                <Select
                  label="Played"
                  className="w-full max-w-none"
                  value={filters.played ?? ""}
                  onChange={(v) => set({ played: (v || undefined) as GameFilters["played"] })}
                  options={[
                    { value: "", label: "Any" },
                    { value: "played", label: "Played before" },
                    { value: "unplayed", label: "Not played yet" },
                  ]}
                />
              </Field>
              <Field label="Favourites">
                <Select
                  label="Favourites"
                  className="w-full max-w-none"
                  value={filters.favourites ? "only" : ""}
                  onChange={(v) => set({ favourites: v === "only" || undefined })}
                  options={[
                    { value: "", label: "Any" },
                    { value: "only", label: "Favourites only" },
                  ]}
                />
              </Field>
              {players && (
                <Field label="Players">
                  <Select
                    label="Players"
                    className="w-full max-w-none"
                    value={String(filters.minPlayers ?? "")}
                    onChange={(v) => set({ minPlayers: Number(v) || undefined })}
                    options={[
                      { value: "", label: "Any" },
                      { value: "2", label: "2 or more" },
                      { value: "3", label: "3 or more" },
                      { value: "4", label: "4 or more" },
                    ]}
                  />
                </Field>
              )}
              {ratings && (
                <Field label="Rating">
                  <Select
                    label="Rating"
                    className="w-full max-w-none"
                    value={String(filters.minStars ?? "")}
                    onChange={(v) => set({ minStars: Number(v) || undefined })}
                    options={[
                      { value: "", label: "Any" },
                      { value: "3", label: "3 stars or more" },
                      { value: "4", label: "4 stars or more" },
                      { value: "4.5", label: "4.5 stars or more" },
                    ]}
                  />
                </Field>
              )}
            </div>
            {esrb.length > 0 && (
              <Field label="Age rating (ESRB)">
                <MultiSelect
                  label="Age rating"
                  selected={filters.esrb ?? []}
                  onChange={(v) => set({ esrb: v.length ? v : undefined })}
                  options={esrb.map(({ value, count }) => ({
                    value,
                    label: `${ESRB[value].name} (${ESRB[value].code})`,
                    count,
                  }))}
                />
              </Field>
            )}
            {lists.map(({ key, label, counts }) => (
              <Field key={key} label={label}>
                <MultiSelect
                  label={label}
                  selected={filters[key] ?? []}
                  onChange={(v) => set({ [key]: v.length ? v : undefined })}
                  options={counts.map(({ value, count }) => ({ value, label: value, count }))}
                />
              </Field>
            ))}
            {!players && !ratings && esrb.length === 0 && lists.length === 0 && (
              <p className="text-sm text-slate-500">
                Filters for players, ratings, genres and more appear once game information has been fetched for these
                games.
              </p>
            )}
          </div>
          {n > 0 && (
            <div className="border-t border-white/10 px-5 py-4">
              <button
                type="button"
                onClick={() => {
                  onFilters({});
                  onQuery("");
                }}
                className="flex items-center gap-1.5 text-sm text-slate-400 hover:text-white"
              >
                <X className="size-4" /> Clear search and filters
              </button>
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
