/* eslint-disable */
/** Internal type. DO NOT USE DIRECTLY. */
type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };
/** Internal type. DO NOT USE DIRECTLY. */
export type Incremental<T> = T | { [P in keyof T]?: P extends ' $fragmentName' | '__typename' ? T[P] : never };
import type { DocumentTypeDecoration } from '@graphql-typed-document-node/core';
/** One input driving one button. */
export type BindingInput = {
  button: PadButton;
  /** A controller input (`south`, `l1`, `up` …) or a keyboard's `KeyboardEvent.code`. */
  input: string;
};

/** One BIOS file. */
export type BiosFileState =
  /** A known dump, or the right name in the wrong case, but not where the core looks. */
  | 'MISPLACED'
  | 'MISSING'
  /** Where the core looks, and a known good dump. */
  | 'OK'
  /** Where the core looks, but not a dump the list knows: a bad dump, or a revision it lacks. */
  | 'UNVERIFIED';

/** How much a system depends on a BIOS file. */
export type BiosImportance =
  /** A boot logo, an add-on. */
  | 'OPTIONAL'
  /** The core has a built-in replacement that plays most games. */
  | 'RECOMMENDED'
  /** Games will not start without it. */
  | 'REQUIRED';

/** A system's BIOS files as a whole. */
export type BiosState =
  /** Playable, without a recommended file. */
  | 'INCOMPLETE'
  /** A required file is missing: games will not start. */
  | 'MISSING'
  /** Everything required and recommended is there and verified. */
  | 'READY'
  /** Playable, but a file is unverified or misplaced. */
  | 'WARNING';

/** How the library is read. */
export type LibraryMode =
  /** From the library, keeping local copies of what is used. */
  | 'CACHE'
  /** Straight from the library (local disk or a share on the LAN). */
  | 'DIRECT'
  /** From a full local copy kept in sync. */
  | 'MIRROR';

/** Where a mapping comes from. */
export type MappingSource =
  /** An admin's default, from the admin page. */
  | 'ADMIN'
  /** A default from `SOVEMU_CONTROLLERS__<n>__…`, which an admin cannot change (locked). */
  | 'ENVIRONMENT'
  /** The user's own. */
  | 'USER';

/** A RetroPad button (libretro's RETRO_DEVICE_ID_JOYPAD_*). */
export type PadButton =
  | 'A'
  | 'B'
  | 'DOWN'
  | 'L'
  | 'L2'
  | 'L3'
  | 'LEFT'
  | 'R'
  | 'R2'
  | 'R3'
  | 'RIGHT'
  | 'SELECT'
  | 'START'
  | 'UP'
  | 'X'
  | 'Y';

/** Where a 3D system renders on this server. */
export type Rendering =
  | 'GPU'
  | 'OFF'
  | 'SOFTWARE';

/** A change to one emulator setting. */
export type SettingChange = {
  key: string;
  /** The new value; null goes back to the default. */
  value?: string | null | undefined;
};

/** Where an emulator setting's value comes from. */
export type SettingSource =
  /** Changed by an admin. */
  | 'ADMIN'
  /** The emulator's own default. */
  | 'CORE'
  /** Set by `SOVEMU_SYSTEMS__<id>__OPTIONS__<name>`, which always wins (locked). */
  | 'ENVIRONMENT'
  /** SovEmu's default for the system. */
  | 'SOVEMU';

/** What a game start is doing. */
export type StartPhase =
  /** Reading the game. */
  | 'CONTENT'
  /** Opening the emulator. */
  | 'CORE'
  | 'FAILED'
  /** Starting the emulator's process. */
  | 'LAUNCHING'
  | 'READY'
  /** Saving and stopping the player's previous session. */
  | 'STOPPING';

export type GameFieldsFragment = { id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null };

export type SystemFieldsFragment = { id: string, name: string, short: string, manufacturer: string, year: number, color: string, gameCount: number, playable: boolean, needsGpu: boolean, rendering: Rendering | null, biosMissing: boolean };

export type SaveFieldsFragment = { slot: number, savedAt: number, screenshotUrl: string | null };

export type SessionFieldsFragment = { id: string, gameId: number, system: string, title: string, core: string, width: number, height: number, fps: number, connected: boolean, discs: number, disc: number };

export type SettingsFieldsFragment = { system: string, core: string, needsGame: boolean, environment: Array<string>, categories: Array<{ key: string, description: string, info: string }>, settings: Array<{ key: string, description: string, info: string, category: string | null, choices: Array<string>, labels: Array<string>, default: string, value: string, source: SettingSource }> };

export type MetadataFieldsFragment = { running: boolean, total: number, done: number, found: number, notFound: number, failed: number, sources: Array<string>, notes: Array<string>, artwork: { running: boolean, total: number, done: number, downloaded: number, failed: number } };

export type ViewerQueryVariables = Exact<{ [key: string]: never; }>;


export type ViewerQuery = { viewer: { setupRequired: boolean, user: { id: number, username: string, isAdmin: boolean } | null } };

export type SetupMutationVariables = Exact<{
  username: string;
  password: string;
}>;


export type SetupMutation = { setup: { id: number, username: string, isAdmin: boolean } };

export type LoginMutationVariables = Exact<{
  username: string;
  password: string;
}>;


export type LoginMutation = { login: { id: number, username: string, isAdmin: boolean } };

export type LogoutMutationVariables = Exact<{ [key: string]: never; }>;


export type LogoutMutation = { logout: boolean };

export type ClientConfigQueryVariables = Exact<{ [key: string]: never; }>;


export type ClientConfigQuery = { clientConfig: { iceServers: unknown } };

export type HomeQueryVariables = Exact<{ [key: string]: never; }>;


export type HomeQuery = { home: { featured: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null }>, recent: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null, autoSave: { slot: number, savedAt: number, screenshotUrl: string | null } | null }>, added: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null }>, favourites: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null }> } };

export type SearchQueryVariables = Exact<{
  text: string;
}>;


export type SearchQuery = { search: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null }> };

export type CollectionsQueryVariables = Exact<{ [key: string]: never; }>;


export type CollectionsQuery = { collections: Array<{ id: number, name: string, gameCount: number, games: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null }> }> };

export type CollectionIdsQueryVariables = Exact<{
  id: number;
}>;


export type CollectionIdsQuery = { game: { id: number, collectionIds: Array<number> } };

export type SetFavouriteMutationVariables = Exact<{
  gameId: number;
  favourite: boolean;
}>;


export type SetFavouriteMutation = { setFavourite: boolean };

export type CreateCollectionMutationVariables = Exact<{
  name: string;
}>;


export type CreateCollectionMutation = { createCollection: { id: number, name: string, gameCount: number } };

export type DeleteCollectionMutationVariables = Exact<{
  id: number;
}>;


export type DeleteCollectionMutation = { deleteCollection: boolean };

export type SetInCollectionMutationVariables = Exact<{
  collectionId: number;
  gameId: number;
  member: boolean;
}>;


export type SetInCollectionMutation = { setInCollection: boolean };

export type SystemsQueryVariables = Exact<{ [key: string]: never; }>;


export type SystemsQuery = { systems: Array<{ id: string, name: string, short: string, manufacturer: string, year: number, color: string, gameCount: number, playable: boolean, needsGpu: boolean, rendering: Rendering | null, biosMissing: boolean }> };

export type BiosQueryVariables = Exact<{ [key: string]: never; }>;


export type BiosQuery = { systems: Array<{ id: string, name: string, short: string, manufacturer: string, year: number, color: string, gameCount: number, playable: boolean, needsGpu: boolean, rendering: Rendering | null, biosMissing: boolean, bios: { state: BiosState, folder: string, groups: Array<{ label: string, importance: BiosImportance, help: string, anyOne: boolean, satisfied: boolean, files: Array<{ name: string, label: string, state: BiosFileState, foundAt: string | null, md5: string | null, size: number }> }> } | null }> };

export type SystemGamesQueryVariables = Exact<{
  id: string | number;
}>;


export type SystemGamesQuery = { system: { id: string, name: string, short: string, manufacturer: string, year: number, color: string, gameCount: number, playable: boolean, needsGpu: boolean, rendering: Rendering | null, biosMissing: boolean, games: Array<{ id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null }> } | null };

export type GameQueryVariables = Exact<{
  id: number;
}>;


export type GameQuery = { game: { id: number, system: string, title: string, fileTitle: string | null, tags: Array<string>, path: string, size: number, lastPlayedAt: string | null, playCount: number, description: string | null, releaseDate: string | null, developer: string | null, publisher: string | null, genre: string | null, players: string | null, rating: number | null, favourite: boolean, release: string | null, discs: number, image: string | null, thumb: string | null, video: string | null, saves: Array<{ slot: number, savedAt: number, screenshotUrl: string | null }> } };

export type SavesQueryVariables = Exact<{
  id: number;
}>;


export type SavesQuery = { game: { id: number, saves: Array<{ slot: number, savedAt: number, screenshotUrl: string | null }> } };

export type CheatsQueryVariables = Exact<{
  id: number;
}>;


export type CheatsQuery = { game: { id: number, cheats: Array<{ id: number, description: string, code: string, enabled: boolean }> } };

export type AddCheatMutationVariables = Exact<{
  gameId: number;
  code: string;
  description: string;
}>;


export type AddCheatMutation = { addCheat: Array<{ id: number, description: string, code: string, enabled: boolean }> };

export type SetCheatMutationVariables = Exact<{
  gameId: number;
  cheatId: number;
  enabled: boolean;
}>;


export type SetCheatMutation = { setCheatEnabled: Array<{ id: number, description: string, code: string, enabled: boolean }> };

export type ControllerMappingsQueryVariables = Exact<{
  system?: string | number | null | undefined;
}>;


export type ControllerMappingsQuery = { controllerMappings: Array<{ controller: string, system: string | null, name: string, source: MappingSource, bindings: Array<{ input: string, button: PadButton }> }> };

export type SaveControllerMappingMutationVariables = Exact<{
  controller: string;
  system?: string | number | null | undefined;
  name: string;
  bindings: Array<BindingInput> | BindingInput;
}>;


export type SaveControllerMappingMutation = { saveControllerMapping: { controller: string } };

export type PadLayoutFieldsFragment = { id: string, name: string, width: number, height: number, body: Array<{ x: number, y: number, w: number, h: number, r: number, tone: number }>, keys: Array<{ key: string, label: string, x: number, y: number, w: number, h: number, r: number }> };

export type SystemControlsQueryVariables = Exact<{
  id: string | number;
}>;


export type SystemControlsQuery = { system: { id: string, controls: { buttons: Array<{ button: PadButton, label: string }>, layout: { id: string, name: string, width: number, height: number, body: Array<{ x: number, y: number, w: number, h: number, r: number, tone: number }>, keys: Array<{ key: string, label: string, x: number, y: number, w: number, h: number, r: number }> } } } | null, controllerLayouts: Array<{ id: string, name: string, width: number, height: number, body: Array<{ x: number, y: number, w: number, h: number, r: number, tone: number }>, keys: Array<{ key: string, label: string, x: number, y: number, w: number, h: number, r: number }> }> };

export type ControllerLayoutsQueryVariables = Exact<{ [key: string]: never; }>;


export type ControllerLayoutsQuery = { controllerLayouts: Array<{ id: string, name: string, width: number, height: number, body: Array<{ x: number, y: number, w: number, h: number, r: number, tone: number }>, keys: Array<{ key: string, label: string, x: number, y: number, w: number, h: number, r: number }> }> };

export type ControllerDefaultsQueryVariables = Exact<{ [key: string]: never; }>;


export type ControllerDefaultsQuery = { controllerDefaults: { environment: Array<string>, mappings: Array<{ controller: string, system: string | null, name: string, source: MappingSource, bindings: Array<{ input: string, button: PadButton }> }> } };

export type SaveControllerDefaultMutationVariables = Exact<{
  controller: string;
  system?: string | number | null | undefined;
  name: string;
  bindings: Array<BindingInput> | BindingInput;
}>;


export type SaveControllerDefaultMutation = { saveControllerDefault: { controller: string } };

export type ResetControllerDefaultMutationVariables = Exact<{
  controller: string;
  system?: string | number | null | undefined;
}>;


export type ResetControllerDefaultMutation = { resetControllerDefault: boolean };

export type ResetControllerMappingMutationVariables = Exact<{
  controller: string;
  system?: string | number | null | undefined;
}>;


export type ResetControllerMappingMutation = { resetControllerMapping: boolean };

export type StartSessionMutationVariables = Exact<{
  gameId: number;
  startId?: string | null | undefined;
}>;


export type StartSessionMutation = { startSession: { id: string, gameId: number, system: string, title: string, core: string, width: number, height: number, fps: number, connected: boolean, discs: number, disc: number } };

export type StartProgressQueryVariables = Exact<{
  startId: string;
}>;


export type StartProgressQuery = { startProgress: { phase: StartPhase, read: number | null, total: number | null, elapsedMs: number, phaseMs: number, error: string | null } };

export type SessionOfferMutationVariables = Exact<{
  id: string | number;
  sdp: string;
}>;


export type SessionOfferMutation = { sessionOffer: string };

export type StopSessionMutationVariables = Exact<{
  id: string | number;
}>;


export type StopSessionMutation = { stopSession: boolean };

export type ResetSessionMutationVariables = Exact<{
  id: string | number;
}>;


export type ResetSessionMutation = { resetSession: boolean };

export type SetDiscMutationVariables = Exact<{
  sessionId: string | number;
  disc: number;
}>;


export type SetDiscMutation = { setDisc: boolean };

export type SaveStateMutationVariables = Exact<{
  sessionId: string | number;
  slot: number;
}>;


export type SaveStateMutation = { saveState: boolean };

export type LoadStateMutationVariables = Exact<{
  sessionId: string | number;
  slot: number;
}>;


export type LoadStateMutation = { loadState: boolean };

export type ScanLibraryMutationVariables = Exact<{ [key: string]: never; }>;


export type ScanLibraryMutation = { scanLibrary: { found: number, added: number, removed: number } };

export type SyncLibraryMutationVariables = Exact<{ [key: string]: never; }>;


export type SyncLibraryMutation = { syncLibrary: boolean };

export type AdminStatusQueryVariables = Exact<{ [key: string]: never; }>;


export type AdminStatusQuery = { libraryStatus: { mode: LibraryMode, games: number, cacheBytes: number | null, mirror: { running: boolean, filesTotal: number, filesDone: number, bytesTotal: number, bytesDone: number, removed: number, failed: number, finishedAt: number | null, error: string | null } | null }, metadataStatus: { running: boolean, total: number, done: number, found: number, notFound: number, failed: number, sources: Array<string>, notes: Array<string>, artwork: { running: boolean, total: number, done: number, downloaded: number, failed: number } } };

export type FetchMetadataMutationVariables = Exact<{
  system?: string | number | null | undefined;
  force: boolean;
}>;


export type FetchMetadataMutation = { fetchMetadata: { running: boolean, total: number, done: number, found: number, notFound: number, failed: number, sources: Array<string>, notes: Array<string>, artwork: { running: boolean, total: number, done: number, downloaded: number, failed: number } } };

export type EmulatorSettingsQueryVariables = Exact<{
  system: string | number;
}>;


export type EmulatorSettingsQuery = { emulatorSettings: { system: string, core: string, needsGame: boolean, environment: Array<string>, categories: Array<{ key: string, description: string, info: string }>, settings: Array<{ key: string, description: string, info: string, category: string | null, choices: Array<string>, labels: Array<string>, default: string, value: string, source: SettingSource }> } };

export type ChangeEmulatorSettingsMutationVariables = Exact<{
  system: string | number;
  changes: Array<SettingChange> | SettingChange;
}>;


export type ChangeEmulatorSettingsMutation = { changeEmulatorSettings: { system: string, core: string, needsGame: boolean, environment: Array<string>, categories: Array<{ key: string, description: string, info: string }>, settings: Array<{ key: string, description: string, info: string, category: string | null, choices: Array<string>, labels: Array<string>, default: string, value: string, source: SettingSource }> } };

export class TypedDocumentString<TResult, TVariables>
  extends String
  implements DocumentTypeDecoration<TResult, TVariables>
{
  __apiType?: NonNullable<DocumentTypeDecoration<TResult, TVariables>['__apiType']>;
  private value: string;
  public __meta__?: Record<string, any> | undefined;

  constructor(value: string, __meta__?: Record<string, any> | undefined) {
    super(value);
    this.value = value;
    this.__meta__ = __meta__;
  }

  override toString(): string & DocumentTypeDecoration<TResult, TVariables> {
    return this.value;
  }
}
export const GameFieldsFragmentDoc = new TypedDocumentString(`
    fragment GameFields on Game {
  id
  system
  title
  fileTitle
  tags
  path
  size
  lastPlayedAt
  playCount
  image: mediaUrl(kind: IMAGE)
  thumb: mediaUrl(kind: THUMB)
  video: mediaUrl(kind: VIDEO)
  description
  releaseDate
  developer
  publisher
  genre
  players
  rating
  favourite
  release
  discs
}
    `, {"fragmentName":"GameFields"}) as unknown as TypedDocumentString<GameFieldsFragment, unknown>;
export const SystemFieldsFragmentDoc = new TypedDocumentString(`
    fragment SystemFields on System {
  id
  name
  short
  manufacturer
  year
  color
  gameCount
  playable
  needsGpu
  rendering
  biosMissing
}
    `, {"fragmentName":"SystemFields"}) as unknown as TypedDocumentString<SystemFieldsFragment, unknown>;
export const SaveFieldsFragmentDoc = new TypedDocumentString(`
    fragment SaveFields on SaveSlot {
  slot
  savedAt
  screenshotUrl
}
    `, {"fragmentName":"SaveFields"}) as unknown as TypedDocumentString<SaveFieldsFragment, unknown>;
export const SessionFieldsFragmentDoc = new TypedDocumentString(`
    fragment SessionFields on GameSession {
  id
  gameId
  system
  title
  core
  width
  height
  fps
  connected
  discs
  disc
}
    `, {"fragmentName":"SessionFields"}) as unknown as TypedDocumentString<SessionFieldsFragment, unknown>;
export const SettingsFieldsFragmentDoc = new TypedDocumentString(`
    fragment SettingsFields on EmulatorSettings {
  system
  core
  needsGame
  environment
  categories {
    key
    description
    info
  }
  settings {
    key
    description
    info
    category
    choices
    labels
    default
    value
    source
  }
}
    `, {"fragmentName":"SettingsFields"}) as unknown as TypedDocumentString<SettingsFieldsFragment, unknown>;
export const MetadataFieldsFragmentDoc = new TypedDocumentString(`
    fragment MetadataFields on MetadataStatus {
  running
  total
  done
  found
  notFound
  failed
  sources
  notes
  artwork {
    running
    total
    done
    downloaded
    failed
  }
}
    `, {"fragmentName":"MetadataFields"}) as unknown as TypedDocumentString<MetadataFieldsFragment, unknown>;
export const PadLayoutFieldsFragmentDoc = new TypedDocumentString(`
    fragment PadLayoutFields on PadLayout {
  id
  name
  width
  height
  body {
    x
    y
    w
    h
    r
    tone
  }
  keys {
    key
    label
    x
    y
    w
    h
    r
  }
}
    `, {"fragmentName":"PadLayoutFields"}) as unknown as TypedDocumentString<PadLayoutFieldsFragment, unknown>;
export const ViewerDocument = new TypedDocumentString(`
    query Viewer {
  viewer {
    setupRequired
    user {
      id
      username
      isAdmin
    }
  }
}
    `) as unknown as TypedDocumentString<ViewerQuery, ViewerQueryVariables>;
export const SetupDocument = new TypedDocumentString(`
    mutation Setup($username: String!, $password: String!) {
  setup(username: $username, password: $password) {
    id
    username
    isAdmin
  }
}
    `) as unknown as TypedDocumentString<SetupMutation, SetupMutationVariables>;
export const LoginDocument = new TypedDocumentString(`
    mutation Login($username: String!, $password: String!) {
  login(username: $username, password: $password) {
    id
    username
    isAdmin
  }
}
    `) as unknown as TypedDocumentString<LoginMutation, LoginMutationVariables>;
export const LogoutDocument = new TypedDocumentString(`
    mutation Logout {
  logout
}
    `) as unknown as TypedDocumentString<LogoutMutation, LogoutMutationVariables>;
export const ClientConfigDocument = new TypedDocumentString(`
    query ClientConfig {
  clientConfig {
    iceServers
  }
}
    `) as unknown as TypedDocumentString<ClientConfigQuery, ClientConfigQueryVariables>;
export const HomeDocument = new TypedDocumentString(`
    query Home {
  home {
    featured {
      ...GameFields
    }
    recent {
      ...GameFields
      autoSave {
        ...SaveFields
      }
    }
    added {
      ...GameFields
    }
    favourites {
      ...GameFields
    }
  }
}
    fragment GameFields on Game {
  id
  system
  title
  fileTitle
  tags
  path
  size
  lastPlayedAt
  playCount
  image: mediaUrl(kind: IMAGE)
  thumb: mediaUrl(kind: THUMB)
  video: mediaUrl(kind: VIDEO)
  description
  releaseDate
  developer
  publisher
  genre
  players
  rating
  favourite
  release
  discs
}
fragment SaveFields on SaveSlot {
  slot
  savedAt
  screenshotUrl
}`) as unknown as TypedDocumentString<HomeQuery, HomeQueryVariables>;
export const SearchDocument = new TypedDocumentString(`
    query Search($text: String!) {
  search(text: $text) {
    ...GameFields
  }
}
    fragment GameFields on Game {
  id
  system
  title
  fileTitle
  tags
  path
  size
  lastPlayedAt
  playCount
  image: mediaUrl(kind: IMAGE)
  thumb: mediaUrl(kind: THUMB)
  video: mediaUrl(kind: VIDEO)
  description
  releaseDate
  developer
  publisher
  genre
  players
  rating
  favourite
  release
  discs
}`) as unknown as TypedDocumentString<SearchQuery, SearchQueryVariables>;
export const CollectionsDocument = new TypedDocumentString(`
    query Collections {
  collections {
    id
    name
    gameCount
    games {
      ...GameFields
    }
  }
}
    fragment GameFields on Game {
  id
  system
  title
  fileTitle
  tags
  path
  size
  lastPlayedAt
  playCount
  image: mediaUrl(kind: IMAGE)
  thumb: mediaUrl(kind: THUMB)
  video: mediaUrl(kind: VIDEO)
  description
  releaseDate
  developer
  publisher
  genre
  players
  rating
  favourite
  release
  discs
}`) as unknown as TypedDocumentString<CollectionsQuery, CollectionsQueryVariables>;
export const CollectionIdsDocument = new TypedDocumentString(`
    query CollectionIds($id: Int!) {
  game(id: $id) {
    id
    collectionIds
  }
}
    `) as unknown as TypedDocumentString<CollectionIdsQuery, CollectionIdsQueryVariables>;
export const SetFavouriteDocument = new TypedDocumentString(`
    mutation SetFavourite($gameId: Int!, $favourite: Boolean!) {
  setFavourite(gameId: $gameId, favourite: $favourite)
}
    `) as unknown as TypedDocumentString<SetFavouriteMutation, SetFavouriteMutationVariables>;
export const CreateCollectionDocument = new TypedDocumentString(`
    mutation CreateCollection($name: String!) {
  createCollection(name: $name) {
    id
    name
    gameCount
  }
}
    `) as unknown as TypedDocumentString<CreateCollectionMutation, CreateCollectionMutationVariables>;
export const DeleteCollectionDocument = new TypedDocumentString(`
    mutation DeleteCollection($id: Int!) {
  deleteCollection(id: $id)
}
    `) as unknown as TypedDocumentString<DeleteCollectionMutation, DeleteCollectionMutationVariables>;
export const SetInCollectionDocument = new TypedDocumentString(`
    mutation SetInCollection($collectionId: Int!, $gameId: Int!, $member: Boolean!) {
  setInCollection(collectionId: $collectionId, gameId: $gameId, member: $member)
}
    `) as unknown as TypedDocumentString<SetInCollectionMutation, SetInCollectionMutationVariables>;
export const SystemsDocument = new TypedDocumentString(`
    query Systems {
  systems {
    ...SystemFields
  }
}
    fragment SystemFields on System {
  id
  name
  short
  manufacturer
  year
  color
  gameCount
  playable
  needsGpu
  rendering
  biosMissing
}`) as unknown as TypedDocumentString<SystemsQuery, SystemsQueryVariables>;
export const BiosDocument = new TypedDocumentString(`
    query Bios {
  systems {
    ...SystemFields
    bios {
      state
      folder
      groups {
        label
        importance
        help
        anyOne
        satisfied
        files {
          name
          label
          state
          foundAt
          md5
          size
        }
      }
    }
  }
}
    fragment SystemFields on System {
  id
  name
  short
  manufacturer
  year
  color
  gameCount
  playable
  needsGpu
  rendering
  biosMissing
}`) as unknown as TypedDocumentString<BiosQuery, BiosQueryVariables>;
export const SystemGamesDocument = new TypedDocumentString(`
    query SystemGames($id: ID!) {
  system(id: $id) {
    ...SystemFields
    games {
      ...GameFields
    }
  }
}
    fragment GameFields on Game {
  id
  system
  title
  fileTitle
  tags
  path
  size
  lastPlayedAt
  playCount
  image: mediaUrl(kind: IMAGE)
  thumb: mediaUrl(kind: THUMB)
  video: mediaUrl(kind: VIDEO)
  description
  releaseDate
  developer
  publisher
  genre
  players
  rating
  favourite
  release
  discs
}
fragment SystemFields on System {
  id
  name
  short
  manufacturer
  year
  color
  gameCount
  playable
  needsGpu
  rendering
  biosMissing
}`) as unknown as TypedDocumentString<SystemGamesQuery, SystemGamesQueryVariables>;
export const GameDocument = new TypedDocumentString(`
    query Game($id: Int!) {
  game(id: $id) {
    ...GameFields
    saves {
      ...SaveFields
    }
  }
}
    fragment GameFields on Game {
  id
  system
  title
  fileTitle
  tags
  path
  size
  lastPlayedAt
  playCount
  image: mediaUrl(kind: IMAGE)
  thumb: mediaUrl(kind: THUMB)
  video: mediaUrl(kind: VIDEO)
  description
  releaseDate
  developer
  publisher
  genre
  players
  rating
  favourite
  release
  discs
}
fragment SaveFields on SaveSlot {
  slot
  savedAt
  screenshotUrl
}`) as unknown as TypedDocumentString<GameQuery, GameQueryVariables>;
export const SavesDocument = new TypedDocumentString(`
    query Saves($id: Int!) {
  game(id: $id) {
    id
    saves {
      ...SaveFields
    }
  }
}
    fragment SaveFields on SaveSlot {
  slot
  savedAt
  screenshotUrl
}`) as unknown as TypedDocumentString<SavesQuery, SavesQueryVariables>;
export const CheatsDocument = new TypedDocumentString(`
    query Cheats($id: Int!) {
  game(id: $id) {
    id
    cheats {
      id
      description
      code
      enabled
    }
  }
}
    `) as unknown as TypedDocumentString<CheatsQuery, CheatsQueryVariables>;
export const AddCheatDocument = new TypedDocumentString(`
    mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {
  addCheat(gameId: $gameId, code: $code, description: $description) {
    id
    description
    code
    enabled
  }
}
    `) as unknown as TypedDocumentString<AddCheatMutation, AddCheatMutationVariables>;
export const SetCheatDocument = new TypedDocumentString(`
    mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {
  setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {
    id
    description
    code
    enabled
  }
}
    `) as unknown as TypedDocumentString<SetCheatMutation, SetCheatMutationVariables>;
export const ControllerMappingsDocument = new TypedDocumentString(`
    query ControllerMappings($system: ID) {
  controllerMappings(system: $system) {
    controller
    system
    name
    source
    bindings {
      input
      button
    }
  }
}
    `) as unknown as TypedDocumentString<ControllerMappingsQuery, ControllerMappingsQueryVariables>;
export const SaveControllerMappingDocument = new TypedDocumentString(`
    mutation SaveControllerMapping($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {
  saveControllerMapping(
    controller: $controller
    system: $system
    name: $name
    bindings: $bindings
  ) {
    controller
  }
}
    `) as unknown as TypedDocumentString<SaveControllerMappingMutation, SaveControllerMappingMutationVariables>;
export const SystemControlsDocument = new TypedDocumentString(`
    query SystemControls($id: ID!) {
  system(id: $id) {
    id
    controls {
      buttons {
        button
        label
      }
      layout {
        ...PadLayoutFields
      }
    }
  }
  controllerLayouts {
    ...PadLayoutFields
  }
}
    fragment PadLayoutFields on PadLayout {
  id
  name
  width
  height
  body {
    x
    y
    w
    h
    r
    tone
  }
  keys {
    key
    label
    x
    y
    w
    h
    r
  }
}`) as unknown as TypedDocumentString<SystemControlsQuery, SystemControlsQueryVariables>;
export const ControllerLayoutsDocument = new TypedDocumentString(`
    query ControllerLayouts {
  controllerLayouts {
    ...PadLayoutFields
  }
}
    fragment PadLayoutFields on PadLayout {
  id
  name
  width
  height
  body {
    x
    y
    w
    h
    r
    tone
  }
  keys {
    key
    label
    x
    y
    w
    h
    r
  }
}`) as unknown as TypedDocumentString<ControllerLayoutsQuery, ControllerLayoutsQueryVariables>;
export const ControllerDefaultsDocument = new TypedDocumentString(`
    query ControllerDefaults {
  controllerDefaults {
    mappings {
      controller
      system
      name
      source
      bindings {
        input
        button
      }
    }
    environment
  }
}
    `) as unknown as TypedDocumentString<ControllerDefaultsQuery, ControllerDefaultsQueryVariables>;
export const SaveControllerDefaultDocument = new TypedDocumentString(`
    mutation SaveControllerDefault($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {
  saveControllerDefault(
    controller: $controller
    system: $system
    name: $name
    bindings: $bindings
  ) {
    controller
  }
}
    `) as unknown as TypedDocumentString<SaveControllerDefaultMutation, SaveControllerDefaultMutationVariables>;
export const ResetControllerDefaultDocument = new TypedDocumentString(`
    mutation ResetControllerDefault($controller: String!, $system: ID) {
  resetControllerDefault(controller: $controller, system: $system)
}
    `) as unknown as TypedDocumentString<ResetControllerDefaultMutation, ResetControllerDefaultMutationVariables>;
export const ResetControllerMappingDocument = new TypedDocumentString(`
    mutation ResetControllerMapping($controller: String!, $system: ID) {
  resetControllerMapping(controller: $controller, system: $system)
}
    `) as unknown as TypedDocumentString<ResetControllerMappingMutation, ResetControllerMappingMutationVariables>;
export const StartSessionDocument = new TypedDocumentString(`
    mutation StartSession($gameId: Int!, $startId: String) {
  startSession(gameId: $gameId, startId: $startId) {
    ...SessionFields
  }
}
    fragment SessionFields on GameSession {
  id
  gameId
  system
  title
  core
  width
  height
  fps
  connected
  discs
  disc
}`) as unknown as TypedDocumentString<StartSessionMutation, StartSessionMutationVariables>;
export const StartProgressDocument = new TypedDocumentString(`
    query StartProgress($startId: String!) {
  startProgress(startId: $startId) {
    phase
    read
    total
    elapsedMs
    phaseMs
    error
  }
}
    `) as unknown as TypedDocumentString<StartProgressQuery, StartProgressQueryVariables>;
export const SessionOfferDocument = new TypedDocumentString(`
    mutation SessionOffer($id: ID!, $sdp: String!) {
  sessionOffer(id: $id, sdp: $sdp)
}
    `) as unknown as TypedDocumentString<SessionOfferMutation, SessionOfferMutationVariables>;
export const StopSessionDocument = new TypedDocumentString(`
    mutation StopSession($id: ID!) {
  stopSession(id: $id)
}
    `) as unknown as TypedDocumentString<StopSessionMutation, StopSessionMutationVariables>;
export const ResetSessionDocument = new TypedDocumentString(`
    mutation ResetSession($id: ID!) {
  resetSession(id: $id)
}
    `) as unknown as TypedDocumentString<ResetSessionMutation, ResetSessionMutationVariables>;
export const SetDiscDocument = new TypedDocumentString(`
    mutation SetDisc($sessionId: ID!, $disc: Int!) {
  setDisc(sessionId: $sessionId, disc: $disc)
}
    `) as unknown as TypedDocumentString<SetDiscMutation, SetDiscMutationVariables>;
export const SaveStateDocument = new TypedDocumentString(`
    mutation SaveState($sessionId: ID!, $slot: Int!) {
  saveState(sessionId: $sessionId, slot: $slot)
}
    `) as unknown as TypedDocumentString<SaveStateMutation, SaveStateMutationVariables>;
export const LoadStateDocument = new TypedDocumentString(`
    mutation LoadState($sessionId: ID!, $slot: Int!) {
  loadState(sessionId: $sessionId, slot: $slot)
}
    `) as unknown as TypedDocumentString<LoadStateMutation, LoadStateMutationVariables>;
export const ScanLibraryDocument = new TypedDocumentString(`
    mutation ScanLibrary {
  scanLibrary {
    found
    added
    removed
  }
}
    `) as unknown as TypedDocumentString<ScanLibraryMutation, ScanLibraryMutationVariables>;
export const SyncLibraryDocument = new TypedDocumentString(`
    mutation SyncLibrary {
  syncLibrary
}
    `) as unknown as TypedDocumentString<SyncLibraryMutation, SyncLibraryMutationVariables>;
export const AdminStatusDocument = new TypedDocumentString(`
    query AdminStatus {
  libraryStatus {
    mode
    games
    cacheBytes
    mirror {
      running
      filesTotal
      filesDone
      bytesTotal
      bytesDone
      removed
      failed
      finishedAt
      error
    }
  }
  metadataStatus {
    ...MetadataFields
  }
}
    fragment MetadataFields on MetadataStatus {
  running
  total
  done
  found
  notFound
  failed
  sources
  notes
  artwork {
    running
    total
    done
    downloaded
    failed
  }
}`) as unknown as TypedDocumentString<AdminStatusQuery, AdminStatusQueryVariables>;
export const FetchMetadataDocument = new TypedDocumentString(`
    mutation FetchMetadata($system: ID, $force: Boolean!) {
  fetchMetadata(system: $system, force: $force) {
    ...MetadataFields
  }
}
    fragment MetadataFields on MetadataStatus {
  running
  total
  done
  found
  notFound
  failed
  sources
  notes
  artwork {
    running
    total
    done
    downloaded
    failed
  }
}`) as unknown as TypedDocumentString<FetchMetadataMutation, FetchMetadataMutationVariables>;
export const EmulatorSettingsDocument = new TypedDocumentString(`
    query EmulatorSettings($system: ID!) {
  emulatorSettings(system: $system) {
    ...SettingsFields
  }
}
    fragment SettingsFields on EmulatorSettings {
  system
  core
  needsGame
  environment
  categories {
    key
    description
    info
  }
  settings {
    key
    description
    info
    category
    choices
    labels
    default
    value
    source
  }
}`) as unknown as TypedDocumentString<EmulatorSettingsQuery, EmulatorSettingsQueryVariables>;
export const ChangeEmulatorSettingsDocument = new TypedDocumentString(`
    mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {
  changeEmulatorSettings(system: $system, changes: $changes) {
    ...SettingsFields
  }
}
    fragment SettingsFields on EmulatorSettings {
  system
  core
  needsGame
  environment
  categories {
    key
    description
    info
  }
  settings {
    key
    description
    info
    category
    choices
    labels
    default
    value
    source
  }
}`) as unknown as TypedDocumentString<ChangeEmulatorSettingsMutation, ChangeEmulatorSettingsMutationVariables>;