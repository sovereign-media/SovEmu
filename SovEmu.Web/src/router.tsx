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
import { SCREEN_PATHS, type ScreenPath } from "./lib/screens";

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
const ClientErrorsAdmin = lazy(() => import("./components/admin/ClientErrorsAdmin"));
const JobsAdmin = lazy(() => import("./components/admin/JobsAdmin"));
const SessionsAdmin = lazy(() => import("./components/admin/SessionsAdmin"));
const ResourcesAdmin = lazy(() => import("./components/admin/ResourcesAdmin"));
const RenderingAdmin = lazy(() => import("./components/admin/RenderingAdmin"));
const BiosAdmin = lazy(() => import("./components/admin/BiosAdmin"));
const DecorationsAdmin = lazy(() => import("./components/admin/DecorationsAdmin"));
const PatchesAdmin = lazy(() => import("./components/admin/PatchesAdmin"));
const CoresAdmin = lazy(() => import("./components/admin/CoresAdmin"));
const ConfigurationAdmin = lazy(() => import("./components/admin/ConfigurationAdmin"));
const NetworkAdmin = lazy(() => import("./components/admin/NetworkAdmin"));
const StorageAdmin = lazy(() => import("./components/admin/StorageAdmin"));
const TelemetryAdmin = lazy(() => import("./components/admin/TelemetryAdmin"));
const AboutAdmin = lazy(() => import("./components/admin/AboutAdmin"));
const AppearanceAdmin = lazy(() => import("./components/admin/AppearanceAdmin"));

/**
 * Each screen's title, component, and the URL parameters it updates itself (with `history`), by
 * its path. Keyed by every path in `SCREEN_PATHS`, no more and no fewer.
 */
const SCREENS: Record<ScreenPath, [title: string, Screen: ComponentType, own: string[]]> = {
  "/": ["Home", Home, []],
  "/login/": ["Sign in", LoginPage, []],
  "/auth/callback/": ["Signing in", AuthCallback, []],
  "/system/": ["Games", SystemPage, ["game"]],
  "/play/": ["Playing", PlayPage, []],
  "/search/": ["Search", Search, ["q"]],
  "/bios/": ["BIOS files", BiosPage, []],
  "/admin/": ["Library", LibraryAdmin, []],
  "/admin/users/": ["Users", UsersAdmin, []],
  "/admin/settings/": ["Settings", SettingsAdmin, ["id"]],
  "/admin/controllers/": ["Controllers", ControllersAdmin, []],
  "/admin/client-errors/": ["Client errors", ClientErrorsAdmin, []],
  "/admin/resources/": ["Resources", ResourcesAdmin, []],
  "/admin/sessions/": ["Sessions", SessionsAdmin, []],
  "/admin/jobs/": ["Jobs", JobsAdmin, []],
  "/admin/rendering/": ["Rendering", RenderingAdmin, []],
  "/admin/bios/": ["BIOS", BiosAdmin, []],
  "/admin/decorations/": ["Decorations", DecorationsAdmin, []],
  "/admin/patches/": ["Patches", PatchesAdmin, []],
  "/admin/cores/": ["Cores", CoresAdmin, []],
  "/admin/configuration/": ["Configuration", ConfigurationAdmin, []],
  "/admin/network/": ["Network", NetworkAdmin, []],
  "/admin/storage/": ["Storage", StorageAdmin, []],
  "/admin/telemetry/": ["Telemetry", TelemetryAdmin, []],
  "/admin/about/": ["About", AboutAdmin, []],
  "/admin/appearance/": ["Appearance", AppearanceAdmin, []],
};

const routes = SCREEN_PATHS.map((path) => {
  const [title, Screen, own] = SCREENS[path];
  return createRoute({ getParentRoute: () => root, path, component: screen(path, title, Screen, own) });
});

export const router = createRouter({
  routeTree: root.addChildren(routes),
  trailingSlash: "always",
  // An unknown path goes home, as the server's old 404 page would have been no use.
  defaultNotFoundComponent: () => {
    router.history.replace("/");
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
