// The SovEmu API (GraphQL), as the pages use it: one function per operation, returning the
// types gql.tada infers from the documents in queries.ts. Signed out, a call goes to sign-in.
// Signing in and out is auth.ts (OpenID Connect), not the API.
import type { Enum, ResultOf } from "./gql";
import { onUnload, request } from "./graphql";
import {
  ActiveSessionsQuery,
  AddCheatMutation,
  AdminPatchesQuery,
  AdminStatusQuery as AdminStatusDocument,
  BiosDirQuery,
  BiosQuery,
  ChangeEmulatorSettingsMutation,
  CheatsQuery as CheatsDocument,
  ClearCacheMutation,
  ClientConfigQuery,
  CollectionIdsQuery,
  CollectionsQuery as CollectionsDocument,
  ConfigurationQuery,
  ControllerDefaultsQuery as ControllerDefaultsDocument,
  ControllerLayoutsQuery,
  ControllerMappingsQuery,
  CoresQuery,
  CreateCollectionMutation,
  DecorationsQuery,
  DeleteCollectionMutation,
  EmulatorSettingsQuery as EmulatorSettingsDocument,
  FetchMetadataMutation,
  type GameFields,
  GameQuery,
  GpuQuery,
  HomeQuery as HomeDocument,
  JobsQuery,
  LoadStateMutation,
  NetworkQuery,
  type PadLayoutFields,
  PatchesQuery as PatchesDocument,
  ResetControllerDefaultMutation,
  ResetControllerMappingMutation,
  ResetSessionMutation,
  ResourceHistoryQuery,
  RunJobMutation,
  SaveControllerDefaultMutation,
  SaveControllerMappingMutation,
  type SaveFields,
  SaveStateMutation,
  SavesQuery,
  ScanLibraryMutation,
  SearchQuery,
  ServerResourcesQuery,
  type SessionFields,
  SessionOfferMutation,
  SetCheatMutation,
  SetDiscMutation,
  SetFavouriteMutation,
  SetInCollectionMutation,
  SetPatchMutation,
  SetRunAheadMutation,
  SetupMutation,
  StartProgressQuery as StartProgressDocument,
  StartSessionMutation,
  StopSessionMutation,
  StorageQuery,
  SyncLibraryMutation,
  SystemControlsQuery,
  type SystemFields,
  SystemGamesQuery,
  SystemsQuery,
  TelemetryQuery,
  UsersQuery,
  ViewerQuery as ViewerDocument,
} from "./queries";

// What each document answers (gql.tada infers it from the schema).
type AdminStatusQuery = ResultOf<typeof AdminStatusDocument>;
type BiosQueryResult = ResultOf<typeof BiosQuery>;
type JobsQueryResult = ResultOf<typeof JobsQuery>;
/** A game being played, as the admin's sessions page shows it. */
export type ActiveSession = ResultOf<typeof ActiveSessionsQuery>["activeSessions"][number];
type ServerResourcesResult = ResultOf<typeof ServerResourcesQuery>;
type ResourceHistoryResult = ResultOf<typeof ResourceHistoryQuery>;
type CheatsQuery = ResultOf<typeof CheatsDocument>;
type CollectionsQuery = ResultOf<typeof CollectionsDocument>;
type ControllerDefaultsQuery = ResultOf<typeof ControllerDefaultsDocument>;
type EmulatorSettingsQuery = ResultOf<typeof EmulatorSettingsDocument>;
type HomeQuery = ResultOf<typeof HomeDocument>;
type PatchesQuery = ResultOf<typeof PatchesDocument>;
type StartProgressQuery = ResultOf<typeof StartProgressDocument>;
type SystemControlsData = ResultOf<typeof SystemControlsQuery>;
type ViewerQuery = ResultOf<typeof ViewerDocument>;
type GameFieldsFragment = ResultOf<typeof GameFields>;
type PadLayoutFieldsFragment = ResultOf<typeof PadLayoutFields>;
type SaveFieldsFragment = ResultOf<typeof SaveFields>;
type SessionFieldsFragment = ResultOf<typeof SessionFields>;
type SystemFieldsFragment = ResultOf<typeof SystemFields>;
type GqlSettingSource = Enum<"SettingSource">;
type PadButton = Enum<"PadButton">;

export { ApiError } from "./graphql";

export type Viewer = ViewerQuery["viewer"];
export type User = NonNullable<Viewer["user"]>;
export type SystemSummary = SystemFieldsFragment;
export type Game = GameFieldsFragment;
export type SaveSlot = SaveFieldsFragment;
export type HomeFeed = HomeQuery["home"];
export type Collection = CollectionsQuery["collections"][number];
export type Patch = PatchesQuery["game"]["patches"][number];
export type RecentGame = HomeFeed["recent"][number];
export type SessionInfo = SessionFieldsFragment;
export type StartStatus = StartProgressQuery["startProgress"];
export type Cheat = NonNullable<CheatsQuery["game"]>["cheats"][number];
export type SystemSettings = EmulatorSettingsQuery["emulatorSettings"];
export type CoreSetting = SystemSettings["settings"][number];
export type CoreCategory = SystemSettings["categories"][number];
export type SettingSource = GqlSettingSource;
export type BiosSystem = BiosQueryResult["systems"][number];
export type SystemBios = NonNullable<BiosSystem["bios"]>;
export type BiosGroup = SystemBios["groups"][number];
export type BiosFile = BiosGroup["files"][number];
export type LibraryStatus = AdminStatusQuery["libraryStatus"];
type Binding = { input: string; button: PadButton };
export type PadLayout = PadLayoutFieldsFragment;
export type SystemControls = NonNullable<NonNullable<SystemControlsData["system"]>["controls"]>;
export type ControllerMappingList = Awaited<ReturnType<typeof api.controllerMappings>>;
export type ControllerDefaults = ControllerDefaultsQuery["controllerDefaults"];
export type ControllerMappingInfo = ControllerDefaults["mappings"][number];
export type MetadataStatus = AdminStatusQuery["metadataStatus"];
export type JobState = JobsQueryResult["jobs"][number];
export type JobName = JobState["name"];
export type ServerResources = ServerResourcesResult["serverResources"];
export type ResourceHistory = ResourceHistoryResult["resourceHistory"];
export type ResourceRange = Enum<"ResourceRange">;
export type TelemetryReport = ResultOf<typeof TelemetryQuery>["telemetry"];
export type StorageReport = ResultOf<typeof StorageQuery>["storage"];
export type NetworkReport = ResultOf<typeof NetworkQuery>["network"];
export type ConfigSetting = ResultOf<typeof ConfigurationQuery>["configuration"][number];
export type CoreReport = ResultOf<typeof CoresQuery>["cores"][number];
export type AdminPatch = ResultOf<typeof AdminPatchesQuery>["patches"][number];
export type DecorationsReport = ResultOf<typeof DecorationsQuery>["decorations"];
export type GpuStatus = ResultOf<typeof GpuQuery>["gpu"];
export type GpuDevice = GpuStatus["devices"][number];

/** Slot 0 is the auto save, written whenever a session ends. */
export const AUTO_SLOT = 0;

export const api = {
  viewer: async () => (await request(ViewerDocument)).viewer,
  /** Admins: every account, by name. */
  users: async () => (await request(UsersQuery)).users,
  setup: async (username: string, password: string) => (await request(SetupMutation, { username, password })).setup,
  iceServers: async () => (await request(ClientConfigQuery)).clientConfig.iceServers as RTCIceServer[],
  home: async () => (await request(HomeDocument)).home,
  systems: async () => (await request(SystemsQuery)).systems,
  /** Every system with its BIOS files checked (null for one that uses none). */
  bios: async () => (await request(BiosQuery)).systems,
  /** Admins: the server's BIOS folder. */
  biosDir: async () => (await request(BiosDirQuery)).biosDir,
  /** A system and its games, or null for an unknown or hidden system. */
  system: async (id: string) => (await request(SystemGamesQuery, { id })).system,
  game: async (id: number) => (await request(GameQuery, { id })).game,
  saves: async (gameId: number) => (await request(SavesQuery, { id: gameId })).game.saves,
  /** This user's collections, each with its games. */
  collections: async () => (await request(CollectionsDocument)).collections,
  /** A game's soft patches, the default first. */
  patches: async (gameId: number) => (await request(PatchesDocument, { id: gameId })).game.patches,
  /** Plays the game with a patch from now on, or the original (`null`). */
  setPatch: (gameId: number, patchId: number | null) => request(SetPatchMutation, { gameId, patchId }),
  /** Games from every system matching every word of `text`, best first. */
  search: async (text: string) => (await request(SearchQuery, { text })).search,
  /** The ids of this user's collections that hold the game. */
  collectionIds: async (gameId: number) => (await request(CollectionIdsQuery, { id: gameId })).game.collectionIds,
  setFavourite: (gameId: number, favourite: boolean) => request(SetFavouriteMutation, { gameId, favourite }),
  createCollection: async (name: string) => (await request(CreateCollectionMutation, { name })).createCollection,
  deleteCollection: (id: number) => request(DeleteCollectionMutation, { id }),
  setInCollection: (collectionId: number, gameId: number, member: boolean) =>
    request(SetInCollectionMutation, { collectionId, gameId, member }),
  scan: async () => (await request(ScanLibraryMutation)).scanLibrary,
  /** Admin: library and game-information status together. */
  adminStatus: () => request(AdminStatusDocument),
  syncLibrary: () => request(SyncLibraryMutation),
  /** Admin: the background jobs and how each last ran. */
  jobs: async () => (await request(JobsQuery)).jobs,
  /** Admin: OpenTelemetry export, and how each signal's last export went. */
  telemetry: async () => (await request(TelemetryQuery)).telemetry,
  /** Admin: where the data folder's space goes (`refresh`: measure now). */
  storage: async (refresh = false) => (await request(StorageQuery, { refresh })).storage,
  /** Admin: deletes the game cache's copies, but those being played; the bytes freed. */
  clearCache: async () => (await request(ClearCacheMutation)).clearCache,
  /** Admin: how browsers and apps reach the server and its game streams. */
  network: async () => (await request(NetworkQuery)).network,
  /** Admin: the server's effective configuration, credentials masked. */
  configuration: async () => (await request(ConfigurationQuery)).configuration,
  /** Admin: every core with its licence, and the libretro release they come from. */
  cores: () => request(CoresQuery),
  /** Admin: every soft patch in the library, with its game and who chose it. */
  adminPatches: async () => (await request(AdminPatchesQuery)).patches,
  /** Admin: the decoration sets, and the set each system's games are framed with. */
  decorations: async () => (await request(DecorationsQuery)).decorations,
  /** Admin: where 3D systems render, and every device the startup probe tried. */
  gpu: async () => (await request(GpuQuery)).gpu,
  activeSessions: async () => (await request(ActiveSessionsQuery)).activeSessions,
  runJob: async (job: JobName) => (await request(RunJobMutation, { job })).runJob,
  /** Admin: the host, and the container's latest CPU and memory use. */
  serverResources: async () => (await request(ServerResourcesQuery)).serverResources,
  resourceHistory: async (range: ResourceRange) => (await request(ResourceHistoryQuery, { range })).resourceHistory,
  fetchMetadata: async (system?: string, force = false) =>
    (await request(FetchMetadataMutation, { system: system ?? null, force })).fetchMetadata,
  /** `startId` lets `startProgress` follow the start while this waits for it. */
  startSession: async (gameId: number, startId?: string) =>
    (await request(StartSessionMutation, { gameId, startId: startId ?? null })).startSession,
  startProgress: async (startId: string) => (await request(StartProgressDocument, { startId })).startProgress,
  /** The worker's answer (SDP) to a WebRTC offer for the session; `client` names this browser. */
  answer: async (sessionId: string, sdp: string, client: string) =>
    (await request(SessionOfferMutation, { id: sessionId, sdp, client })).sessionOffer,
  stopSession: (id: string) => request(StopSessionMutation, { id }),
  /** Stops a session as the page closes, so the auto save is written now. */
  stopSessionOnUnload: (id: string) => onUnload(StopSessionMutation, { id }),
  resetSession: (id: string) => request(ResetSessionMutation, { id }),
  saveState: (sessionId: string, slot: number) => request(SaveStateMutation, { sessionId, slot }),
  /** Changes a multi-disc game's disc (from 1). */
  setDisc: (sessionId: string, disc: number) => request(SetDiscMutation, { sessionId, disc }),
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
  /** Frames a system's pictures run ahead of its games; null goes back to off. */
  setRunAhead: async (system: string, frames: number | null) =>
    (await request(SetRunAheadMutation, { system, frames })).setRunAhead,
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
  const then = Date.parse(`${sqliteUtc.replace(" ", "T")}Z`);
  const minutes = Math.round((now - then) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}
