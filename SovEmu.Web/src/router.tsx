// The screens, one route each, at the same paths (trailing slash and all) the site has always
// used, so links, bookmarks and the sign-in callback (`/auth/callback/`) keep working.
//
// Each screen reads what it needs from the URL as it mounts, and is mounted afresh whenever the
// router's location changes: the same start every screen had when each was its own page, without
// reloading the page (no new HTML, scripts, token or controller setup between screens).
import { createRootRoute, createRoute, createRouter, Outlet, useRouterState } from "@tanstack/react-router";
import { type ComponentType, lazy, Suspense, useLayoutEffect, useRef } from "react";
import { pathOf, screenKey, setNavigator } from "./lib/go";
import { followLinks } from "./lib/links";

/** A screen, titled, and remounted when the URL changes (see above), but for `own` parameters. */
function screen(path: string, title: string, Screen: ComponentType, own: readonly string[]) {
  return function Titled() {
    const href = useRouterState({ select: (s) => s.location.href });
    // Only a URL on this screen's own path counts: while the router moves to another screen,
    // this one still renders once with the new URL, and must not remount against it (its
    // mount would read, and rewrite, the next screen's URL).
    const key = useRef("");
    if (key.current === "" || pathOf(href) === path) key.current = screenKey(href, own);
    // Before the screen's own effects, so a screen that names itself (a game) has the last word.
    useLayoutEffect(() => {
      document.title = `${title} · SovEmu`;
    }, []);
    return (
      <Suspense fallback={null}>
        <Screen key={key.current} />
      </Suspense>
    );
  };
}

const root = createRootRoute({ component: Outlet });

// Each screen is its own chunk, loaded on first visit: home need not wait for the player.
const Home = lazy(() => import("./components/home/Home"));
const LoginPage = lazy(() => import("./components/LoginPage"));
const AuthCallback = lazy(() => import("./components/AuthCallback"));
const SystemPage = lazy(() => import("./components/SystemPage"));
const PlayPage = lazy(() => import("./components/play/PlayPage"));
const Search = lazy(() => import("./components/search/Search"));
const BiosPage = lazy(() => import("./components/BiosPage"));
const LibraryAdmin = lazy(() => import("./components/admin/LibraryAdmin"));
const UsersAdmin = lazy(() => import("./components/admin/UsersAdmin"));
const SettingsAdmin = lazy(() => import("./components/admin/SettingsAdmin"));
const ControllersAdmin = lazy(() => import("./components/admin/ControllersAdmin"));
const JobsAdmin = lazy(() => import("./components/admin/JobsAdmin"));

/** Path, title, screen, and the URL parameters the screen updates itself (with `history`). */
const SCREENS: [path: string, title: string, Screen: ComponentType, own: string[]][] = [
  ["/", "Home", Home, []],
  ["/login/", "Sign in", LoginPage, []],
  ["/auth/callback/", "Signing in", AuthCallback, []],
  ["/system/", "Games", SystemPage, ["game"]],
  ["/play/", "Playing", PlayPage, []],
  ["/search/", "Search", Search, ["q"]],
  ["/bios/", "BIOS files", BiosPage, []],
  ["/admin/", "Library", LibraryAdmin, []],
  ["/admin/users/", "Users", UsersAdmin, []],
  ["/admin/settings/", "Settings", SettingsAdmin, ["id"]],
  ["/admin/controllers/", "Controllers", ControllersAdmin, []],
  ["/admin/jobs/", "Jobs", JobsAdmin, []],
];

const routes = SCREENS.map(([path, title, Screen, own]) =>
  createRoute({ getParentRoute: () => root, path, component: screen(path, title, Screen, own) }),
);

export const router = createRouter({
  routeTree: root.addChildren(routes),
  trailingSlash: "always",
  // An unknown path goes home, as the server's old 404 page would have been no use.
  defaultNotFoundComponent: () => {
    router.navigate({ to: "/", replace: true });
    return null;
  },
});

setNavigator((href, replace) => (replace ? router.history.replace(href) : router.history.push(href)));
followLinks((path) => router.history.push(path));

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
