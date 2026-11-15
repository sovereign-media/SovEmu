// The navigation, after Sovereign's: on a monitor a rail down the whole left edge (icons at
// rest; on hover or keyboard focus it widens over the page, labels each entry and dims the page
// behind it), on a phone a tab bar along the bottom. The SovEmu wordmark sits at the top, beside
// the rail. Entries: Home; Systems, a menu nested by
// maker (Nintendo ▸ NES, SNES …); the account (sign out); and for admins the gear, a menu of
// Library and Emulator settings ▸ each system. Menus are Radix DropdownMenus.
import { ChevronRight, Gamepad2, House, Library, LogOut, Settings, SlidersHorizontal } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api, type SystemSummary, type User } from "../lib/api";
import { groupByMaker, initials, systemBadge } from "../lib/systems";
import { cn } from "./ui/cn";

/** Which section the page belongs to, lit in the dock. */
export type Section = "home" | "systems" | "admin" | "none";

const PANE = "z-50 min-w-52 overflow-hidden rounded-xl border border-edge bg-panel py-1 shadow-2xl outline-none";
const ROW =
  "flex w-full cursor-pointer items-center gap-3 px-4 py-2 text-left text-sm text-slate-300 outline-none select-none data-[highlighted]:bg-white/10 data-[highlighted]:text-white data-[state=open]:bg-white/10 data-[state=open]:text-white [&_svg]:size-4 [&_svg]:shrink-0";
const HEADING = "border-b border-edge px-4 pt-1.5 pb-2 text-xs font-semibold tracking-wider text-slate-500 uppercase";

// One dock entry: a 44px disc at rest, widening to a labelled pill when the dock opens.
const ENTRY =
  "flex h-11 w-11 cursor-pointer items-center overflow-hidden rounded-full outline-none transition-[width,background-color] duration-200 group-data-[open=true]/dock:w-48 focus-visible:ring-2 focus-visible:ring-white/60 focus-visible:ring-inset";
const IDLE = "text-slate-400 hover:bg-white/10 hover:text-white data-[state=open]:bg-white/10 data-[state=open]:text-white";
const CURRENT = "bg-accent text-white";
const GLYPH = "grid h-11 w-11 shrink-0 place-items-center [&_svg]:size-5";
const LABEL =
  "min-w-0 truncate pr-4 text-sm font-medium opacity-0 transition-opacity duration-200 group-data-[open=true]/dock:opacity-100";

/** A system's row: its colour, name and a status if it cannot play (or plays slowly). */
function SystemRow({ s, href }: { s: SystemSummary; href: string }) {
  const badge = systemBadge(s);
  return (
    <DropdownMenu.Item asChild className={ROW}>
      <a href={href}>
        <span className="size-2.5 shrink-0 rounded-full" style={{ background: s.color }} />
        <span className="min-w-0 flex-1 truncate">{s.name}</span>
        <span className="text-xs text-slate-500 tabular-nums">{badge ?? s.gameCount}</span>
      </a>
    </DropdownMenu.Item>
  );
}

/** Systems by maker: a submenu per maker, or the system itself where a maker has one. On a
 *  phone (`flat`), where a submenu would open off the screen, makers are headings instead. */
function SystemsByMaker({
  systems,
  href,
  flat = false,
}: {
  systems: SystemSummary[];
  href: (s: SystemSummary) => string;
  flat?: boolean;
}) {
  if (flat) {
    return (
      <>
        {groupByMaker(systems).map(({ maker, systems }) => (
          <DropdownMenu.Group key={maker}>
            <DropdownMenu.Label className="px-4 pt-2 pb-1 text-[11px] font-semibold tracking-wider text-slate-500 uppercase">
              {maker}
            </DropdownMenu.Label>
            {systems.map((s) => (
              <SystemRow key={s.id} s={s} href={href(s)} />
            ))}
          </DropdownMenu.Group>
        ))}
      </>
    );
  }
  return (
    <>
      {groupByMaker(systems).map(({ maker, systems }) =>
        systems.length === 1 ? (
          <SystemRow key={maker} s={systems[0]} href={href(systems[0])} />
        ) : (
          <DropdownMenu.Sub key={maker}>
            <DropdownMenu.SubTrigger className={ROW}>
              <span className="flex -space-x-1">
                {systems.slice(0, 3).map((s) => (
                  <span key={s.id} className="size-2.5 rounded-full ring-2 ring-panel" style={{ background: s.color }} />
                ))}
              </span>
              <span className="flex-1">{maker}</span>
              <ChevronRight className="text-slate-500" />
            </DropdownMenu.SubTrigger>
            <DropdownMenu.Portal>
              <DropdownMenu.SubContent sideOffset={4} collisionPadding={8} className={PANE}>
                {systems.map((s) => (
                  <SystemRow key={s.id} s={s} href={href(s)} />
                ))}
              </DropdownMenu.SubContent>
            </DropdownMenu.Portal>
          </DropdownMenu.Sub>
        ),
      )}
    </>
  );
}

/** A dock entry that opens a menu beside the dock (on a phone, above the tab bar). */
function MenuEntry({
  icon,
  label,
  current,
  shape,
  heading,
  children,
  onOpenChange,
}: {
  icon: ReactNode;
  label: string;
  current: boolean;
  shape: "dock" | "bar";
  heading: string;
  children: ReactNode;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <DropdownMenu.Root modal={false} onOpenChange={onOpenChange}>
      <DropdownMenu.Trigger asChild>
        {shape === "dock" ? (
          <button type="button" aria-label={label} title={label} className={cn(ENTRY, current ? CURRENT : IDLE)}>
            <span className={GLYPH}>{icon}</span>
            <span aria-hidden className={LABEL}>
              {label}
            </span>
          </button>
        ) : (
          <button type="button" className={cn(BAR_ITEM, current && "text-white")}>
            {icon}
            <span className="text-[10px] font-medium">{label}</span>
          </button>
        )}
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          side={shape === "dock" ? "right" : "top"}
          align={shape === "dock" ? "center" : "end"}
          sideOffset={shape === "dock" ? 18 : 12}
          collisionPadding={8}
          className={cn(PANE, shape === "bar" && "max-h-[70dvh] overflow-y-auto")}
        >
          <DropdownMenu.Label className={HEADING}>{heading}</DropdownMenu.Label>
          <div className="pt-1">{children}</div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

const BAR_ITEM =
  "flex min-w-0 flex-1 cursor-pointer flex-col items-center justify-center gap-0.5 rounded-lg py-1.5 text-slate-400 outline-none [&_svg]:size-5";

/** The entries, shared by the dock and the tab bar so the two cannot drift apart. */
function Entries({
  shape,
  section,
  user,
  systems,
  onMenu,
}: {
  shape: "dock" | "bar";
  section: Section;
  user: User | null;
  systems: SystemSummary[];
  onMenu: (open: boolean) => void;
}) {
  const withGames = systems.filter((s) => s.gameCount > 0);
  const configurable = systems.filter((s) => s.playable || s.needsGpu);
  const signOut = async () => {
    await api.logout();
    location.href = "/login/";
  };
  const home =
    shape === "dock" ? (
      <a href="/" aria-label="Home" title="Home" className={cn(ENTRY, section === "home" ? CURRENT : IDLE)}>
        <span className={GLYPH}>
          <House />
        </span>
        <span aria-hidden className={LABEL}>
          Home
        </span>
      </a>
    ) : (
      <a href="/" className={cn(BAR_ITEM, section === "home" && "text-white")}>
        <House />
        <span className="text-[10px] font-medium">Home</span>
      </a>
    );
  return (
    <>
      {home}
      <MenuEntry icon={<Gamepad2 />} label="Systems" heading="Systems" current={section === "systems"} shape={shape} onOpenChange={onMenu}>
        {withGames.length ? (
          <SystemsByMaker systems={withGames} flat={shape === "bar"} href={(s) => `/system/?id=${encodeURIComponent(s.id)}`} />
        ) : (
          <p className="px-4 py-2 text-sm text-slate-500">No games yet.</p>
        )}
      </MenuEntry>
      {/* On the rail, the account and admin entries sit at the bottom. */}
      {shape === "dock" && <div className="flex-1" />}
      {user && (
        <MenuEntry
          icon={
            <span className="grid size-7 place-items-center rounded-full bg-accent text-[10px] font-bold text-white">
              {initials(user.username)}
            </span>
          }
          label={user.username}
          heading={`Signed in as ${user.username}`}
          current={false}
          shape={shape}
          onOpenChange={onMenu}
        >
          <DropdownMenu.Item className={ROW} onSelect={signOut}>
            <LogOut /> Sign out
          </DropdownMenu.Item>
        </MenuEntry>
      )}
      {user?.isAdmin && (
        <MenuEntry icon={<Settings />} label="Admin" heading="Admin" current={section === "admin"} shape={shape} onOpenChange={onMenu}>
          <DropdownMenu.Item asChild className={ROW}>
            <a href="/admin/">
              <Library /> Library
            </a>
          </DropdownMenu.Item>
          {shape === "bar" ? (
            <DropdownMenu.Item asChild className={ROW}>
              <a href="/admin/settings/">
                <SlidersHorizontal /> Emulator settings
              </a>
            </DropdownMenu.Item>
          ) : (
            <DropdownMenu.Sub>
              <DropdownMenu.SubTrigger className={ROW}>
                <SlidersHorizontal />
                <span className="flex-1">Emulator settings</span>
                <ChevronRight className="text-slate-500" />
              </DropdownMenu.SubTrigger>
              <DropdownMenu.Portal>
                <DropdownMenu.SubContent sideOffset={4} collisionPadding={8} className={cn(PANE, "max-h-[70dvh] overflow-y-auto")}>
                  <SystemsByMaker systems={configurable} href={(s) => `/admin/settings/?id=${encodeURIComponent(s.id)}`} />
                </DropdownMenu.SubContent>
              </DropdownMenu.Portal>
            </DropdownMenu.Sub>
          )}
        </MenuEntry>
      )}
    </>
  );
}

/** How long the pointer rests on the dock before it opens, so crossing it does not flash it. */
const HOVER_INTENT_MS = 120;

export default function SideNav({ section }: { section: Section }) {
  const [user, setUser] = useState<User | null>(null);
  const [systems, setSystems] = useState<SystemSummary[]>([]);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [menus, setMenus] = useState(0);
  const intent = useRef(0);

  useEffect(() => {
    api.viewer().then((s) => setUser(s.user));
    api.systems().then(setSystems).catch(() => {});
  }, []);

  // Stays open while one of its menus is.
  const onMenu = (open: boolean) => setMenus((n) => Math.max(0, n + (open ? 1 : -1)));
  const open = hovered || focused || menus > 0;

  return (
    <>
      {/* The brand, at the top beside the rail. */}
      <a
        href="/"
        aria-label="SovEmu home"
        className="fixed top-4 left-20 z-30 hidden items-center gap-2 text-lg font-black tracking-tight text-white drop-shadow md:flex"
      >
        <Gamepad2 className="size-6 text-accent" aria-hidden />
        <span>
          Sov<span className="text-accent">Emu</span>
        </span>
      </a>

      {/* Dims the page while the rail is open; it widens over the page, never pushing it. */}
      <div
        aria-hidden
        className={cn(
          "pointer-events-none fixed inset-0 z-[35] hidden bg-black/60 transition-opacity duration-200 md:block",
          open ? "opacity-100" : "opacity-0",
        )}
      />

      {/* The rail: the full height of the left edge, square, flush with the screen. */}
      <nav
        aria-label="Main navigation"
        data-open={open}
        onMouseEnter={() => {
          clearTimeout(intent.current);
          intent.current = window.setTimeout(() => setHovered(true), HOVER_INTENT_MS);
        }}
        onMouseLeave={() => {
          clearTimeout(intent.current);
          setHovered(false);
        }}
        onFocus={(e) => setFocused(e.target.matches(":focus-visible"))}
        onBlur={(e) => !e.currentTarget.contains(e.relatedTarget) && setFocused(false)}
        className="group/dock fixed inset-y-0 left-0 z-40 hidden w-16 flex-col gap-1 border-r border-white/10 bg-surface px-2.5 py-3 transition-[width,box-shadow] duration-200 data-[open=true]:w-[13.25rem] data-[open=true]:shadow-2xl md:flex"
      >
        <Entries shape="dock" section={section} user={user} systems={systems} onMenu={onMenu} />
      </nav>

      {/* A phone: the same entries along the bottom. */}
      <nav
        aria-label="Main navigation"
        className="fixed inset-x-0 bottom-0 z-40 flex h-14 items-stretch gap-1 border-t border-white/10 bg-surface/95 px-1 py-1 backdrop-blur-md md:hidden"
      >
        <Entries shape="bar" section={section} user={user} systems={systems} onMenu={onMenu} />
      </nav>
    </>
  );
}

/** Space for the navigation: the dock's column on a monitor, the tab bar on a phone. */
export const NAV_SPACE = "md:pl-16 pb-14 md:pb-0";
