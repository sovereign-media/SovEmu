// What the screens read from the API, cached and shared by TanStack Query: one key and fetch per
// read, here, so every screen asks for "the systems" the same way and a change elsewhere can say
// what it made stale (`invalidate`). Screens use these with `useQuery`; the player, which loads
// imperatively, with `queryClient.fetchQuery`.
import { QueryClient, queryOptions } from "@tanstack/react-query";
import { api } from "./api";
import { ApiError } from "./graphql";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Moving between screens shows what was read a moment ago at once, and reads it again in
      // the background only once it is this old.
      staleTime: 30_000,
      // Signed out is handled by the API layer (it goes to sign-in); other failures, once more.
      retry: (failures, error) => !(error instanceof ApiError && error.code === "UNAUTHENTICATED") && failures < 1,
      refetchOnWindowFocus: false,
    },
  },
});

export const q = {
  viewer: () => queryOptions({ queryKey: ["viewer"], queryFn: api.viewer }),
  users: () => queryOptions({ queryKey: ["users"], queryFn: api.users }),
  home: () => queryOptions({ queryKey: ["home"], queryFn: api.home }),
  systems: () => queryOptions({ queryKey: ["systems"], queryFn: api.systems }),
  bios: () => queryOptions({ queryKey: ["bios"], queryFn: api.bios }),
  system: (id: string) => queryOptions({ queryKey: ["system", id], queryFn: () => api.system(id) }),
  game: (id: number) => queryOptions({ queryKey: ["game", id], queryFn: () => api.game(id) }),
  saves: (gameId: number) => queryOptions({ queryKey: ["game", gameId, "saves"], queryFn: () => api.saves(gameId) }),
  patches: (gameId: number) =>
    queryOptions({ queryKey: ["game", gameId, "patches"], queryFn: () => api.patches(gameId) }),
  collectionIds: (gameId: number) =>
    queryOptions({ queryKey: ["game", gameId, "collections"], queryFn: () => api.collectionIds(gameId) }),
  cheats: (gameId: number) => queryOptions({ queryKey: ["game", gameId, "cheats"], queryFn: () => api.cheats(gameId) }),
  collections: () => queryOptions({ queryKey: ["collections"], queryFn: api.collections }),
  search: (text: string) => queryOptions({ queryKey: ["search", text], queryFn: () => api.search(text) }),
  adminStatus: () => queryOptions({ queryKey: ["admin", "status"], queryFn: api.adminStatus }),
  jobs: () => queryOptions({ queryKey: ["admin", "jobs"], queryFn: api.jobs }),
  systemSettings: (system: string) =>
    queryOptions({ queryKey: ["admin", "settings", system], queryFn: () => api.systemSettings(system) }),
  controllerDefaults: () =>
    queryOptions({ queryKey: ["admin", "controllerDefaults"], queryFn: api.controllerDefaults }),
  controllerLayouts: () =>
    queryOptions({ queryKey: ["controllerLayouts"], queryFn: api.controllerLayouts, staleTime: Infinity }),
  iceServers: () => queryOptions({ queryKey: ["iceServers"], queryFn: api.iceServers, staleTime: Infinity }),
  controllerMappings: (system: string | null) =>
    queryOptions({ queryKey: ["controllerMappings", system], queryFn: () => api.controllerMappings(system) }),
  systemControls: (system: string) =>
    queryOptions({ queryKey: ["systemControls", system], queryFn: () => api.systemControls(system) }),
};

/** Marks reads stale after a change, so whatever shows them reads them again: `invalidate(["home"])`. */
export const invalidate = (...keys: readonly unknown[][]) =>
  Promise.all(keys.map((queryKey) => queryClient.invalidateQueries({ queryKey })));
