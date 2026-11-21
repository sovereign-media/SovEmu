// Moving between screens without reloading the page. The router installs itself here
// (`main.tsx`); until it has (or in tests), a plain page load does the same.

type Navigate = (href: string, replace: boolean) => void;

let navigate: Navigate = (href, replace) => (replace ? location.replace(href) : location.assign(href));

/** Used by the router to take over navigation. */
export function setNavigator(n: Navigate) {
  navigate = n;
}

/** Goes to `href` (`/system/?id=snes`), adding to history, or in place of the current entry. */
export function go(href: string, { replace = false }: { replace?: boolean } = {}) {
  navigate(href, replace);
}

/**
 * Which screen a URL shows: its path and query, less the parameters the screen keeps up to date
 * itself (the games screen's `game`, search's `q`), so its own history entries do not count as a
 * new screen.
 */
export function screenKey(href: string, own: readonly string[] = []): string {
  const url = new URL(href, "http://x");
  for (const p of own) url.searchParams.delete(p);
  url.searchParams.sort();
  return url.pathname + url.search;
}

/** The path of `href` (`/system/` for `/system/?id=snes`), with the router's trailing slash. */
export function pathOf(href: string): string {
  const path = new URL(href, "http://x").pathname;
  return path.endsWith("/") ? path : `${path}/`;
}
