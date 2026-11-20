// The admin menu, as Sovereign's (`Sovereign.React/src/components/admin/adminNavItems.ts`): the gear
// in the dock opens a cascade of groups, each a submenu of pages. One list drives the menu, the
// page headings and the document titles, so a new page is one entry here.
import { Gamepad, Gamepad2, Library, SlidersHorizontal, Users, UsersRound, type LucideIcon } from "lucide-react";

export interface AdminNavItem {
  /** The page (with its trailing slash, as Astro serves it). */
  href: string;
  label: string;
  /** What the page is for, as its tooltip. */
  title: string;
  icon: LucideIcon;
}

export interface AdminNavGroup {
  id: string;
  label: string;
  icon: LucideIcon;
  items: AdminNavItem[];
}

export const ADMIN_NAV_GROUPS: AdminNavGroup[] = [
  {
    id: "library",
    label: "Library",
    icon: Library,
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
    icon: UsersRound,
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
    icon: Gamepad2,
    items: [
      {
        href: "/admin/settings/",
        label: "Emulator settings",
        title: "Each system's core and its options",
        icon: SlidersHorizontal,
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
