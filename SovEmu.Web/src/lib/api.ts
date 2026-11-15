// The SovEmu API (GraphQL), as the pages use it: one function per operation, returning the
// types codegen derived from the documents in queries.ts. Signed out, a call goes to sign-in.
import type {
  AdminStatusQuery,
  ControllerDefaultsQuery,
  CheatsQuery,
  EmulatorSettingsQuery,
  GameFieldsFragment,
  PadButton,
  PadLayoutFieldsFragment,
  SystemControlsQuery as SystemControlsData,
  HomeQuery,
  SaveFieldsFragment,
  SessionFieldsFragment,
  SettingSource as GqlSettingSource,
  StartProgressQuery,
  SystemFieldsFragment,
  ViewerQuery,
} from "../graphql/graphql";
import {
  AddCheatMutation,
  AdminStatusQuery as AdminStatusDocument,
  ChangeEmulatorSettingsMutation,
  ControllerDefaultsQuery as ControllerDefaultsDocument,
  ControllerLayoutsQuery,
  ControllerMappingsQuery,
  CheatsQuery as CheatsDocument,
  ClientConfigQuery,
  EmulatorSettingsQuery as EmulatorSettingsDocument,
  FetchMetadataMutation,
  GameQuery,
  HomeQuery as HomeDocument,
  LoadStateMutation,
  LoginMutation,
  LogoutMutation,
  ResetControllerDefaultMutation,
  ResetControllerMappingMutation,
  ResetSessionMutation,
  SaveControllerDefaultMutation,
  SaveControllerMappingMutation,
  SaveStateMutation,
  SavesQuery,
  ScanLibraryMutation,
  SessionOfferMutation,
  SetCheatMutation,
  SetupMutation,
  StartProgressQuery as StartProgressDocument,
  StartSessionMutation,
  StopSessionMutation,
  SyncLibraryMutation,
  SystemControlsQuery,
  SystemGamesQuery,
  SystemsQuery,
  ViewerQuery as ViewerDocument,
} from "./queries";
import { onUnload, request } from "./graphql";

export { ApiError } from "./graphql";

export type Viewer = ViewerQuery["viewer"];
export type User = NonNullable<Viewer["user"]>;
export type SystemSummary = SystemFieldsFragment;
export type Game = GameFieldsFragment;
export type SaveSlot = SaveFieldsFragment;
export type HomeFeed = HomeQuery["home"];
export type RecentGame = HomeFeed["recent"][number];
export type SessionInfo = SessionFieldsFragment;
export type StartStatus = StartProgressQuery["startProgress"];
export type Cheat = NonNullable<CheatsQuery["game"]>["cheats"][number];
export type SystemSettings = EmulatorSettingsQuery["emulatorSettings"];
export type CoreSetting = SystemSettings["settings"][number];
export type CoreCategory = SystemSettings["categories"][number];
export type SettingSource = GqlSettingSource;
export type LibraryStatus = AdminStatusQuery["libraryStatus"];
type Binding = { input: string; button: PadButton };
export type PadLayout = PadLayoutFieldsFragment;
export type SystemControls = NonNullable<NonNullable<SystemControlsData["system"]>["controls"]>;
export type ControllerMappingList = Awaited<ReturnType<typeof api.controllerMappings>>;
export type ControllerDefaults = ControllerDefaultsQuery["controllerDefaults"];
export type ControllerMappingInfo = ControllerDefaults["mappings"][number];
export type MetadataStatus = AdminStatusQuery["metadataStatus"];

/** Slot 0 is the auto save, written whenever a session ends. */
export const AUTO_SLOT = 0;

export const api = {
  viewer: async () => (await request(ViewerDocument)).viewer,
  setup: async (username: string, password: string) => (await request(SetupMutation, { username, password })).setup,
  login: async (username: string, password: string) => (await request(LoginMutation, { username, password })).login,
  logout: () => request(LogoutMutation),
  iceServers: async () => (await request(ClientConfigQuery)).clientConfig.iceServers as RTCIceServer[],
  home: async () => (await request(HomeDocument)).home,
  systems: async () => (await request(SystemsQuery)).systems,
  /** A system and its games, or null for an unknown or hidden system. */
  system: async (id: string) => (await request(SystemGamesQuery, { id })).system,
  game: async (id: number) => (await request(GameQuery, { id })).game,
  saves: async (gameId: number) => (await request(SavesQuery, { id: gameId })).game.saves,
  scan: async () => (await request(ScanLibraryMutation)).scanLibrary,
  /** Admin: library and game-information status together. */
  adminStatus: () => request(AdminStatusDocument),
  syncLibrary: () => request(SyncLibraryMutation),
  fetchMetadata: async (system?: string, force = false) =>
    (await request(FetchMetadataMutation, { system: system ?? null, force })).fetchMetadata,
  /** `startId` lets `startProgress` follow the start while this waits for it. */
  startSession: async (gameId: number, startId?: string) =>
    (await request(StartSessionMutation, { gameId, startId: startId ?? null })).startSession,
  startProgress: async (startId: string) => (await request(StartProgressDocument, { startId })).startProgress,
  /** The worker's answer (SDP) to a WebRTC offer for the session. */
  answer: async (sessionId: string, sdp: string) =>
    (await request(SessionOfferMutation, { id: sessionId, sdp })).sessionOffer,
  stopSession: (id: string) => request(StopSessionMutation, { id }),
  /** Stops a session as the page closes, so the auto save is written now. */
  stopSessionOnUnload: (id: string) => onUnload(StopSessionMutation, { id }),
  resetSession: (id: string) => request(ResetSessionMutation, { id }),
  saveState: (sessionId: string, slot: number) => request(SaveStateMutation, { sessionId, slot }),
  loadState: (sessionId: string, slot: number) => request(LoadStateMutation, { sessionId, slot }),
  cheats: async (gameId: number) => (await request(CheatsDocument, { id: gameId })).game.cheats,
  addCheat: async (gameId: number, code: string, description: string) =>
    (await request(AddCheatMutation, { gameId, code, description })).addCheat,
  setCheat: async (gameId: number, cheatId: number, enabled: boolean) =>
    (await request(SetCheatMutation, { gameId, cheatId, enabled })).setCheatEnabled,
  /** The mappings in force for this user on a system, or on every system with none (docs/CONTROLLERS.md). */
  controllerMappings: async (system: string | null = null) =>
    (await request(ControllerMappingsQuery, { system })).controllerMappings,
  /** Saves this user's mapping for a controller model on a system, or on every system with null. */
  saveControllerMapping: (controller: string, system: string | null, name: string, bindings: Binding[]) =>
    request(SaveControllerMappingMutation, { controller, system, name, bindings }),
  resetControllerMapping: (controller: string, system: string | null) =>
    request(ResetControllerMappingMutation, { controller, system }),
  /** A system's buttons and pad, and the player's controllers' pads, for diagrams. */
  systemControls: async (id: string) => {
    const data = await request(SystemControlsQuery, { id });
    return { controls: data.system?.controls ?? null, layouts: data.controllerLayouts };
  },
  controllerLayouts: async () => (await request(ControllerLayoutsQuery)).controllerLayouts,
  /** Admins: the default mappings every user starts from, and the admin's as environment lines. */
  controllerDefaults: async () => (await request(ControllerDefaultsDocument)).controllerDefaults,
  saveControllerDefault: (controller: string, system: string | null, name: string, bindings: Binding[]) =>
    request(SaveControllerDefaultMutation, { controller, system, name, bindings }),
  resetControllerDefault: (controller: string, system: string | null) =>
    request(ResetControllerDefaultMutation, { controller, system }),
  systemSettings: async (system: string) => (await request(EmulatorSettingsDocument, { system })).emulatorSettings,
  /** A value sets an option; null goes back to the default. */
  changeSettings: async (system: string, changes: Record<string, string | null>) =>
    (
      await request(ChangeEmulatorSettingsMutation, {
        system,
        changes: Object.entries(changes).map(([key, value]) => ({ key, value })),
      })
    ).changeEmulatorSettings,
};

/** Human-readable file size. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** "3 days ago" style, from an SQLite UTC timestamp ("2026-11-13 23:00:00"). */
export function formatAgo(sqliteUtc: string | null, now = Date.now()): string {
  if (!sqliteUtc) return "never";
  const then = Date.parse(sqliteUtc.replace(" ", "T") + "Z");
  const minutes = Math.round((now - then) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}
