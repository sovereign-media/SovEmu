// Search and filters for a system's games, in a drawer from the right (Sovereign's
// SearchFilterDrawer): search, played or not, players, and the library's tags.
import { Check, Search, SlidersHorizontal, X } from "lucide-react";
import { Dialog } from "radix-ui";
import { useMemo, type ReactNode } from "react";
import type { Game } from "../../lib/api";
import { activeFilters, tagCounts, type GameFilters } from "../../lib/games";
import { cn } from "../ui/cn";

function Toggle({ on, onClick, children }: { on: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={cn(
        "flex h-8 items-center gap-1.5 rounded-full border px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-white/60 [&_svg]:size-3.5",
        on ? "border-transparent bg-accent text-white" : "border-white/15 text-slate-300 hover:bg-white/10",
      )}
    >
      {on && <Check />}
      {children}
    </button>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="grid gap-2">
      <h3 className="text-xs font-semibold tracking-wider text-slate-500 uppercase">{title}</h3>
      <div className="flex flex-wrap gap-2">{children}</div>
    </section>
  );
}

export function FilterDrawer({
  open,
  onClose,
  games,
  query,
  onQuery,
  filters,
  onFilters,
  searchRef,
}: {
  open: boolean;
  onClose: () => void;
  games: Game[];
  query: string;
  onQuery: (q: string) => void;
  filters: GameFilters;
  onFilters: (f: GameFilters) => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
}) {
  const tags = useMemo(() => tagCounts(games).slice(0, 16), [games]);
  const n = activeFilters(filters) + (query.trim() ? 1 : 0);
  const toggleTag = (t: string) => {
    const has = filters.tags?.includes(t);
    onFilters({ ...filters, tags: has ? filters.tags!.filter((x) => x !== t) : [...(filters.tags ?? []), t] });
  };
  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/10" />
        <Dialog.Content
          // Escape in the search box closes the drawer; elsewhere the page's pad navigation does
          // (its Back), so one press is never handled twice.
          onEscapeKeyDown={(e) => !(e.target instanceof HTMLInputElement) && e.preventDefault()}
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            searchRef.current?.focus();
          }}
          className="fixed inset-y-3 right-3 z-50 flex w-[min(24rem,calc(100vw-1.5rem))] flex-col overflow-hidden rounded-[2rem] border border-white/10 bg-surface/95 shadow-2xl outline-none backdrop-blur-md"
        >
          <div className="mx-5 flex items-center gap-3 border-b border-white/10 pt-5 pb-4">
            <SlidersHorizontal className="size-5 text-white" aria-hidden />
            <Dialog.Title className="text-lg font-semibold">Filters</Dialog.Title>
            {n > 0 && <span className="rounded-full bg-accent px-2.5 py-0.5 text-xs font-semibold">{n} on</span>}
            <Dialog.Close aria-label="Close" className="ml-auto grid size-9 place-items-center rounded-full text-slate-400 hover:bg-white/10 hover:text-white">
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
            <Section title="Played">
              <Toggle on={filters.played === "played"} onClick={() => onFilters({ ...filters, played: filters.played === "played" ? undefined : "played" })}>
                Played before
              </Toggle>
              <Toggle on={filters.played === "unplayed"} onClick={() => onFilters({ ...filters, played: filters.played === "unplayed" ? undefined : "unplayed" })}>
                Not played yet
              </Toggle>
            </Section>
            <Section title="Players">
              <Toggle on={!!filters.multiplayer} onClick={() => onFilters({ ...filters, multiplayer: !filters.multiplayer || undefined })}>
                2 players or more
              </Toggle>
            </Section>
            {tags.length > 0 && (
              <Section title="Tags">
                {tags.map(({ tag, count }) => (
                  <Toggle key={tag} on={!!filters.tags?.includes(tag)} onClick={() => toggleTag(tag)}>
                    {tag}
                    <span className="text-xs opacity-60 tabular-nums">{count}</span>
                  </Toggle>
                ))}
              </Section>
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
