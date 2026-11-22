// The admin menu: the gear in the dock opens one flat list of the admin pages, a separator
// between groups. One list drives the menu, the page headings and the document titles, so a new
// page is one entry here.
import { Gamepad, Library, ListChecks, type LucideIcon, SlidersHorizontal, Users } from "lucide-react";

export interface AdminNavItem {
  /** The page (with its trailing slash, as the router's paths have it). */
  href: string;
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
    ],
  },
  {
    id: "server",
    label: "Server",
    items: [
      {
        href: "/admin/jobs/",
        label: "Jobs",
        title: "Scans, syncs, lookups and upkeep: when each runs and how it last went",
        icon: ListChecks,
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
