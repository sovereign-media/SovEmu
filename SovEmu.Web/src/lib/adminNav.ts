// The admin menu: the gear in the dock opens one flat list of the admin pages, a separator
// between groups. One list drives the menu, the page headings and the document titles, so a new
// page is one entry here.
import {
  Activity,
  Bug,
  FileDiff,
  Frame,
  Gamepad,
  Gauge,
  HardDrive,
  History,
  Library,
  ListChecks,
  type LucideIcon,
  Microchip,
  MonitorCog,
  MonitorPlay,
  MonitorSmartphone,
  Network,
  Package,
  Palette,
  Radio,
  ScrollText,
  Settings2,
  SlidersHorizontal,
  Users,
} from "lucide-react";
import type { ScreenPath } from "./screens";

export interface AdminNavItem {
  /** The page: one of the router's paths (`screens.ts`). */
  href: ScreenPath;
  label: string;
  /** What the page is for, as its tooltip. */
  title: string;
  icon: LucideIcon;
}

export interface AdminNavGroup {
  id: string;
  /** Names the group in its pages' document titles (`Users & Devices · Users`). */
  label: string;
  items: AdminNavItem[];
}

export const ADMIN_NAV_GROUPS: AdminNavGroup[] = [
  {
    id: "library",
    label: "Library",
    items: [
      {
        href: "/admin/",
        label: "Library",
        title: "Scanning, the mirror or cache, and game information",
        icon: Library,
      },
      {
        href: "/admin/patches/",
        label: "Patches",
        title: "The soft patches found beside games, and which apply by default",
        icon: FileDiff,
      },
    ],
  },
  {
    id: "users-devices",
    label: "Users & Devices",
    items: [
      { href: "/admin/users/", label: "Users", title: "Accounts on this server and their roles", icon: Users },
      {
        href: "/admin/controllers/",
        label: "Controllers",
        title: "The default button mapping for each controller model",
        icon: Gamepad,
      },
      {
        href: "/admin/devices/",
        label: "Devices",
        title: "Every browser and app signed in, and signing one out",
        icon: MonitorSmartphone,
      },
      {
        href: "/admin/client-errors/",
        label: "Client errors",
        title: "What went wrong in the apps, by kind and by app and device",
        icon: Bug,
      },
    ],
  },
  {
    id: "emulation",
    label: "Emulation",
    items: [
      {
        href: "/admin/settings/",
        label: "Emulator settings",
        title: "Each system's core and its options",
        icon: SlidersHorizontal,
      },
      {
        href: "/admin/cores/",
        label: "Cores",
        title: "The emulators installed: versions, systems and licences",
        icon: Package,
      },
      {
        href: "/admin/rendering/",
        label: "Rendering",
        title: "Where 3D systems render: the GPU, or software",
        icon: MonitorCog,
      },
      {
        href: "/admin/streaming/",
        label: "Streaming",
        title: "Test this browser's connection to the game streams, and the stream settings",
        icon: Radio,
      },
      {
        href: "/admin/bios/",
        label: "BIOS",
        title: "The BIOS files each system needs, where they go, and their MD5s",
        icon: Microchip,
      },
      {
        href: "/admin/decorations/",
        label: "Decorations",
        title: "The bezel sets found, and which frames each system's games",
        icon: Frame,
      },
    ],
  },
  {
    id: "server",
    label: "Server",
    items: [
      {
        href: "/admin/sessions/",
        label: "Sessions",
        title: "Games being played now, and how each stream is made",
        icon: MonitorPlay,
      },
      {
        href: "/admin/resources/",
        label: "Resources",
        title: "The host's CPU and memory, and how much the server is using",
        icon: Gauge,
      },
      {
        href: "/admin/activity/",
        label: "Activity",
        title: "Who did what, and when: sign-ins, games played, admin changes",
        icon: History,
      },
      {
        href: "/admin/jobs/",
        label: "Jobs",
        title: "Scans, syncs, lookups and upkeep: when each runs and how it last went",
        icon: ListChecks,
      },
      {
        href: "/admin/configuration/",
        label: "Configuration",
        title: "Every setting the server runs with, and where each came from",
        icon: Settings2,
      },
      {
        href: "/admin/network/",
        label: "Network",
        title: "How browsers and apps reach the server and its game streams",
        icon: Network,
      },
      {
        href: "/admin/storage/",
        label: "Storage",
        title: "Where the data folder's space goes, and clearing the game cache",
        icon: HardDrive,
      },
      {
        href: "/admin/telemetry/",
        label: "Telemetry",
        title: "Whether traces and metrics are exported, and how the last export went",
        icon: Activity,
      },
      {
        href: "/admin/appearance/",
        label: "Appearance",
        title: "The background behind every screen, for everyone",
        icon: Palette,
      },
      {
        href: "/admin/about/",
        label: "About",
        title: "This build, and the software and services SovEmu is made with",
        icon: ScrollText,
      },
    ],
  },
];

/** A path as the menu compares it: with its trailing slash, without a query. */
const normalize = (pathname: string) => (pathname.endsWith("/") ? pathname : `${pathname}/`);

/** The page a path is, if it is one of the menu's. */
export function findAdminNavItem(pathname: string): AdminNavItem | null {
  const path = normalize(pathname);
  for (const group of ADMIN_NAV_GROUPS) {
    const item = group.items.find((i) => i.href === path);
    if (item) return item;
  }
  return null;
}

/** The group a path's page is in, lit as the current one in the menu. */
export function findAdminNavGroup(pathname: string): AdminNavGroup | null {
  const path = normalize(pathname);
  return ADMIN_NAV_GROUPS.find((g) => g.items.some((i) => i.href === path)) ?? null;
}

/** The document title for an admin page: `Group · Page`, or the page alone when they match. */
export function adminDocumentTitle(pathname: string): string {
  const group = findAdminNavGroup(pathname);
  const item = findAdminNavItem(pathname);
  if (!group || !item) return "Admin";
  return group.label === item.label ? item.label : `${group.label} · ${item.label}`;
}
