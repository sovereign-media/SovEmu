// The SovEmu API (GraphQL), as the pages use it: one function per operation, returning the
// types gql.tada infers from the documents in queries.ts. Signed out, a call goes to sign-in.
// Signing in and out is auth.ts (OpenID Connect), not the API.
import type { Capabilities } from "./capabilities";
import type { Enum, ResultOf } from "./gql";
import { onUnload, request } from "./graphql";
import {
  AboutQuery,
  ActiveSessionsQuery,
  ActivityQuery,
  AddCheatMutation,
  AddUserMutation,
  AdminPatchesQuery,
  AdminStatusQuery as AdminStatusDocument,
  AppearanceQuery,
  ApproveDeviceCodeMutation,
  AttentionCountsQuery,
  BackgroundQuery,
  BackupsQuery,
  BiosDirQuery,
  BiosQuery,
  ChangeEmulatorSettingsMutation,
  CheatsQuery as CheatsDocument,
  ClearCacheMutation,
  ClientConfigQuery,
  ClientEventGroupsQuery,
  ClientEventsQuery,
  CollectionIdsQuery,
  CollectionsQuery as CollectionsDocument,
  ConfigurationQuery,
  ControllerDefaultsQuery as ControllerDefaultsDocument,
  ControllerLayoutsQuery,
  ControllerMappingsQuery,
  CoresQuery,
  CreateCollectionMutation,
  CreateSharedCollectionMutation,
  DecorationsQuery,
  DeleteBackupMutation,
  DeleteCollectionMutation,
  DeleteSharedCollectionMutation,
  EmulatorSettingsQuery as EmulatorSettingsDocument,
  EndSignInMutation,
  FetchMetadataMutation,
  type GameFields,
  GameQuery,
  GamesNeedingAttentionQuery,
  GpuQuery,
  HomeQuery as HomeDocument,
  JobsQuery,
  LoadStateMutation,
  LogsQuery,
  LookUpGameMutation,
  NetworkQuery,
  OverviewQuery,
  type PadLayoutFields,
  PatchesQuery as PatchesDocument,
  PlayStatisticsQuery,
  RemoveUserMutation,
  RenameSharedCollectionMutation,
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
  SessionFailuresQuery,
  type SessionFields,
  SessionOfferMutation,
  SetBackgroundMutation,
  SetCheatMutation,
  SetDiscMutation,
  SetFavouriteMutation,
  SetInCollectionMutation,
  SetInSharedCollectionMutation,
  SetPatchMutation,
  SetRunAheadMutation,
  SetUserPasswordMutation,
  SetUserRoleMutation,
  SetupMutation,
  SignInsQuery,
  SignOutEverywhereMutation,
  StartProgressQuery as StartProgressDocument,
  StartSessionMutation,
  StopSessionMutation,
  StorageQuery,
  StreamingQuery,
  SyncLibraryMutation,
  SystemControlsQuery,
  type SystemFields,
  SystemGamesQuery,
  SystemsQuery,
  TelemetryQuery,
  TestConnectionMutation,
  ThirdPartyNoticesQuery,
  UserDetailsQuery,
  UsersQuery,
  ViewerQuery as ViewerDocument,
} from "./queries";

// What each document answers (gql.tada infers it from the schema).
type AdminStatusQuery = ResultOf<typeof AdminStatusDocument>;
type BiosQueryResult = ResultOf<typeof BiosQuery>;
type JobsQueryResult = ResultOf<typeof JobsQuery>;
/** A game being played, as the admin's sessions page shows it. */
export type ActiveSession = ResultOf<typeof ActiveSessionsQuery>["activeSessions"][number];
/** A game that failed to start, or crashed, as the admin's sessions page shows it. */
/** What the admin Overview's cards read. */
export type Overview = ResultOf<typeof OverviewQuery>;
/** How much a log line says. */
export type LogLevel = Enum<"LogLevel">;
export type AttentionKind = Enum<"AttentionKind">;
export type EsrbRating = Enum<"EsrbRating">;
export type Notice = ResultOf<typeof ThirdPartyNoticesQuery>["thirdPartyNotices"]["server"][number];
export type Backups = ResultOf<typeof BackupsQuery>["backups"];
export type AttentionItem = ResultOf<typeof GamesNeedingAttentionQuery>["gamesNeedingAttention"][number];
export type AttentionCount = ResultOf<typeof AttentionCountsQuery>["attentionCounts"][number];
/** One line of the server's log. */
export type LogLine = ResultOf<typeof LogsQuery>["logs"]["lines"][number];
/** Which lines of the log a view asks for. */
export interface LogFilter {
  level: LogLevel | null;
  session: string | null;
  text: string | null;
}
/** The window the play statistics cover. */
export type StatsRange = Enum<"StatsRange">;
/** What was played over a window. */
export type PlayStatistics = ResultOf<typeof PlayStatisticsQuery>["playStatistics"];
/** What kind of thing happened, for the Activity page's filter. */
export type ActivityCategory = Enum<"ActivityCategory">;
/** One entry of the activity log. */
export type ActivityEntry = ResultOf<typeof ActivityQuery>["activity"]["entries"][number];
/** Which entries of the activity log a page asks for. */
export interface ActivityFilter {
  userId: number | null;
  category: ActivityCategory | null;
}
/** A browser or app signed in as an account. */
export type SignIn = ResultOf<typeof SignInsQuery>["signIns"][number];
/** What an account may do. */
export type Role = Enum<"Role">;
/** An account, as the admin's Users page lists it. */
export type Account = ResultOf<typeof UsersQuery>["users"][number];
/** What went wrong on a client, as the apps report it. */
export type ClientEventKind = Enum<"ClientEventKind">;
/** Client errors of one kind from one client. */
export type ClientEventGroup = ResultOf<typeof ClientEventGroupsQuery>["clientEventGroups"][number];
/** One client error. */
export type ClientEvent = ResultOf<typeof ClientEventsQuery>["clientEvents"][number];
export type SessionFailure = ResultOf<typeof SessionFailuresQuery>["sessionFailures"][number];
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
/** A collection's games' places, for its environment lines. */
export type CollectionGame = Collection["games"][number];
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
export type MetadataField = Enum<"MetadataField">;
export type JobState = JobsQueryResult["jobs"][number];
export type JobName = JobState["name"];
export type ServerResources = ServerResourcesResult["serverResources"];
export type ResourceHistory = ResourceHistoryResult["resourceHistory"];
export type ResourceRange = Enum<"ResourceRange">;
export type AppearanceSettings = ResultOf<typeof AppearanceQuery>["appearance"];
export type About = ResultOf<typeof AboutQuery>;
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
  /** Admin: every browser and app signed in, across accounts. */
  signIns: async () => (await request(SignInsQuery)).signIns,
  /** Admin: one account, with its sign-ins, what it has played and its saves. */
  userDetails: async (id: number) => (await request(UserDetailsQuery, { id })).user,
  /** Admin: signs one browser or app out. */
  endSignIn: (id: string) => request(EndSignInMutation, { id }),
  /** Signs a TV in as you: the code its *Sign in with your phone* screen shows. */
  approveDeviceCode: (userCode: string) => request(ApproveDeviceCodeMutation, { userCode }),
  /** Admin: adds an account. */
  addUser: (username: string, password: string, role: Role) =>
    request(AddUserMutation, { input: { username, password, role } }),
  /** Admin: makes an account an admin or a viewer. */
  setUserRole: (userId: number, role: Role) => request(SetUserRoleMutation, { userId, role }),
  /** Admin: sets an account's password, signing that user out everywhere. */
  setUserPassword: (userId: number, password: string) => request(SetUserPasswordMutation, { userId, password }),
  /** Admin: removes an account (its saves stay on disk). */
  removeUser: (userId: number) => request(RemoveUserMutation, { userId }),
  /** Admin: signs an account out of every browser and app. */
  signOutEverywhere: (userId: number) => request(SignOutEverywhereMutation, { userId }),
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
  /** The shared collections, then this user's own, each with its games (`shared` tells them apart). */
  collections: async () => {
    const r = await request(CollectionsDocument);
    return [...r.sharedCollections, ...r.collections];
  },
  /** A game's soft patches, the default first. */
  patches: async (gameId: number) => (await request(PatchesDocument, { id: gameId })).game.patches,
  /** Plays the game with a patch from now on, or the original (`null`). */
  setPatch: (gameId: number, patchId: number | null) => request(SetPatchMutation, { gameId, patchId }),
  /** Games from every system matching every word of `text`, best first. */
  search: async (text: string) => (await request(SearchQuery, { text })).search,
  /** The ids of this user's and the shared collections that hold the game (one table: never the same id). */
  collectionIds: async (gameId: number) => {
    const { game } = await request(CollectionIdsQuery, { id: gameId });
    return [...game.sharedCollectionIds, ...game.collectionIds];
  },
  setFavourite: (gameId: number, favourite: boolean) => request(SetFavouriteMutation, { gameId, favourite }),
  createCollection: async (name: string) => (await request(CreateCollectionMutation, { name })).createCollection,
  deleteCollection: (id: number) => request(DeleteCollectionMutation, { id }),
  setInCollection: (collectionId: number, gameId: number, member: boolean) =>
    request(SetInCollectionMutation, { collectionId, gameId, member }),
  /** Admins: makes a collection everyone sees. */
  createSharedCollection: async (name: string) =>
    (await request(CreateSharedCollectionMutation, { name })).createSharedCollection,
  renameSharedCollection: (id: number, name: string) => request(RenameSharedCollectionMutation, { id, name }),
  deleteSharedCollection: (id: number) => request(DeleteSharedCollectionMutation, { id }),
  setInSharedCollection: (collectionId: number, gameId: number, member: boolean) =>
    request(SetInSharedCollectionMutation, { collectionId, gameId, member }),
  scan: async () => (await request(ScanLibraryMutation)).scanLibrary,
  /** Admin: library and game-information status together. */
  adminStatus: () => request(AdminStatusDocument),
  syncLibrary: () => request(SyncLibraryMutation),
  /** Admin: the background jobs and how each last ran. */
  jobs: async () => (await request(JobsQuery)).jobs,
  /** The background behind every screen, for everyone (null: none). */
  background: async () => (await request(BackgroundQuery)).background,
  /** Admin: every background, and the one chosen. */
  appearance: async () => (await request(AppearanceQuery)).appearance,
  /** Admin: the background for everyone: an id, `random` or `none`. */
  setBackground: (id: string) => request(SetBackgroundMutation, { id }),
  /** Admin: the server's build, the services it uses and what the image ships. */
  about: () => request(AboutQuery),
  /** Admin: OpenTelemetry export, and how each signal's last export went. */
  telemetry: async () => (await request(TelemetryQuery)).telemetry,
  /** Admin: where the data folder's space goes (`refresh`: measure now). */
  storage: async (refresh = false) => (await request(StorageQuery, { refresh })).storage,
  /** Admin: deletes the game cache's copies, but those being played; the bytes freed. */
  clearCache: async () => (await request(ClearCacheMutation)).clearCache,
  /** Admin: how browsers and apps reach the server and its game streams. */
  network: async () => (await request(NetworkQuery)).network,
  /** Admin: everything the Overview's cards show, in one read. */
  overview: () => request(OverviewQuery),
  /** Admin: the server's recent log, the newest lines the filter keeps. */
  logs: async (filter: LogFilter) => (await request(LogsQuery, filter)).logs,
  /** Admin: one kind's games (or files) needing attention, one system's or all. */
  gamesNeedingAttention: async (kind: AttentionKind, system: string | null) =>
    (await request(GamesNeedingAttentionQuery, { kind, system })).gamesNeedingAttention,
  /** Admin: how many games (or files) of each kind need attention, per system. */
  attentionCounts: async () => (await request(AttentionCountsQuery)).attentionCounts,
  /** Admin: looks one game up again, in the background. */
  lookUpGame: async (gameId: number) => (await request(LookUpGameMutation, { gameId })).lookUpGame,
  /** Admin: the open-source packages the server and the web client ship, by licence. */
  thirdPartyNotices: async () => (await request(ThirdPartyNoticesQuery)).thirdPartyNotices,
  /** Admin: the backups kept, newest first, and how they are made. */
  backups: async () => (await request(BackupsQuery)).backups,
  deleteBackup: (name: string) => request(DeleteBackupMutation, { name }),
  /** Admin: what was played over the last week, month or year. */
  playStatistics: async (range: StatsRange) => (await request(PlayStatisticsQuery, { range })).playStatistics,
  /** Admin: a page of the activity log, newest first; `after` is the last page's `next`. */
  activity: async (filter: ActivityFilter, after: string | null, limit = 100) =>
    (await request(ActivityQuery, { ...filter, after, limit })).activity,
  /** Admin: the settings that shape every game's stream, and how long a connection test lasts. */
  streaming: async () => (await request(StreamingQuery)).streaming,
  /** Admin: answers this browser's offer with a connection-test peer on a game port. */
  testConnection: async (sdp: string) => (await request(TestConnectionMutation, { sdp })).testConnection,
  /** Admin: the client errors of the last `days` days, by kind and client. */
  clientEventGroups: async (days: number) => (await request(ClientEventGroupsQuery, { days })).clientEventGroups,
  /** Admin: one kind and client's errors of the last `days` days, newest first. */
  clientEvents: async (kind: ClientEventKind, client: string, days: number) =>
    (await request(ClientEventsQuery, { kind, client, days })).clientEvents,
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
  /** Admin: the games that failed most recently, newest first, with the core's last lines. */
  sessionFailures: async () => (await request(SessionFailuresQuery)).sessionFailures,
  runJob: async (job: JobName) => (await request(RunJobMutation, { job })).runJob,
  /** Admin: the host, and the container's latest CPU and memory use. */
  serverResources: async () => (await request(ServerResourcesQuery)).serverResources,
  resourceHistory: async (range: ResourceRange) => (await request(ResourceHistoryQuery, { range })).resourceHistory,
  /** Admin: looks up game information; `fields` refreshes only those (the rest are kept). */
  fetchMetadata: async (system?: string, force = false, fields: MetadataField[] = []) =>
    (await request(FetchMetadataMutation, { system: system ?? null, force, fields })).fetchMetadata,
  /** `startId` lets `startProgress` follow the start while this waits for it. */
  startSession: async (gameId: number, startId?: string) =>
    (await request(StartSessionMutation, { gameId, startId: startId ?? null })).startSession,
  startProgress: async (startId: string) => (await request(StartProgressDocument, { startId })).startProgress,
  /**
   * The worker's answer (SDP) to a WebRTC offer for the session; `client` names this browser and
   * `capabilities` says what it can decode and show (the whole document, every time).
   */
  answer: async (sessionId: string, sdp: string, client: string, capabilities: Capabilities) =>
    (await request(SessionOfferMutation, { id: sessionId, sdp, client, capabilities })).sessionOffer,
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
