// Plain `<a href>` links between screens, followed by the router instead of a page load. Every
// screen's links stay ordinary anchors (Radix's `asChild` menus included); one listener on the
// document decides which clicks are the app's.

/** Paths the Rust server answers itself: always a real request. */
const SERVER_PATHS = ["/media/", "/oauth/", "/graphql", "/.well-known/", "/health"];

/** The path (with query and hash) to route to for a link to `href`, or null to leave it alone. */
export function inAppPath(href: string, origin: string, current: string): string | null {
  let url: URL;
  try {
    url = new URL(href, current);
  } catch {
    return null;
  }
  if (url.origin !== origin) return null;
  if (SERVER_PATHS.some((p) => url.pathname.startsWith(p))) return null;
  // A link within this screen (`#section`): the browser scrolls, nothing to route.
  const here = new URL(current);
  if (url.pathname === here.pathname && url.search === here.search && url.hash) return null;
  return url.pathname + url.search + url.hash;
}

/** Routes clicks on in-app links with `navigate`; returns the function that stops it. */
export function followLinks(navigate: (path: string) => void, doc: Document = document): () => void {
  const onClick = (e: MouseEvent) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target instanceof Element ? e.target.closest("a") : null;
    if (!a || !a.hasAttribute("href") || (a.target && a.target !== "_self") || a.hasAttribute("download")) return;
    const path = inAppPath(a.getAttribute("href")!, location.origin, location.href);
    if (path === null) return;
    e.preventDefault();
    navigate(path);
  };
  doc.addEventListener("click", onClick);
  return () => doc.removeEventListener("click", onClick);
}
