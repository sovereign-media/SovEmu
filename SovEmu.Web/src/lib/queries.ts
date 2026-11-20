// Every GraphQL operation the web client sends. `npm run codegen` types them (src/graphql/).
import { graphql } from "../graphql";

graphql(`
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
    patch
    discs
  }
`);

graphql(`
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
`);

graphql(`
  fragment SaveFields on SaveSlot {
    slot
    savedAt
    screenshotUrl
  }
`);

graphql(`
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
`);

graphql(`
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
`);

graphql(`
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
`);

export const ViewerQuery = graphql(`
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
`);

export const SetupMutation = graphql(`
  mutation Setup($username: String!, $password: String!) {
    setup(username: $username, password: $password) {
      id
      username
      isAdmin
    }
  }
`);

export const LoginMutation = graphql(`
  mutation Login($username: String!, $password: String!) {
    login(username: $username, password: $password) {
      id
      username
      isAdmin
    }
  }
`);

export const LogoutMutation = graphql(`
  mutation Logout {
    logout
  }
`);

export const ClientConfigQuery = graphql(`
  query ClientConfig {
    clientConfig {
      iceServers
    }
  }
`);

export const HomeQuery = graphql(`
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
`);

export const PatchesQuery = graphql(`
  query Patches($id: Int!) {
    game(id: $id) {
      id
      patches {
        id
        name
        isDefault
      }
    }
  }
`);

export const SetPatchMutation = graphql(`
  mutation SetPatch($gameId: Int!, $patchId: Int) {
    setPatch(gameId: $gameId, patchId: $patchId)
  }
`);

export const SearchQuery = graphql(`
  query Search($text: String!) {
    search(text: $text) {
      ...GameFields
    }
  }
`);

export const CollectionsQuery = graphql(`
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
`);

export const CollectionIdsQuery = graphql(`
  query CollectionIds($id: Int!) {
    game(id: $id) {
      id
      collectionIds
    }
  }
`);

export const SetFavouriteMutation = graphql(`
  mutation SetFavourite($gameId: Int!, $favourite: Boolean!) {
    setFavourite(gameId: $gameId, favourite: $favourite)
  }
`);

export const CreateCollectionMutation = graphql(`
  mutation CreateCollection($name: String!) {
    createCollection(name: $name) {
      id
      name
      gameCount
    }
  }
`);

export const DeleteCollectionMutation = graphql(`
  mutation DeleteCollection($id: Int!) {
    deleteCollection(id: $id)
  }
`);

export const SetInCollectionMutation = graphql(`
  mutation SetInCollection($collectionId: Int!, $gameId: Int!, $member: Boolean!) {
    setInCollection(collectionId: $collectionId, gameId: $gameId, member: $member)
  }
`);

export const SystemsQuery = graphql(`
  query Systems {
    systems {
      ...SystemFields
    }
  }
`);

export const BiosQuery = graphql(`
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
`);

export const SystemGamesQuery = graphql(`
  query SystemGames($id: ID!) {
    system(id: $id) {
      ...SystemFields
      games {
        ...GameFields
      }
    }
  }
`);

export const GameQuery = graphql(`
  query Game($id: Int!) {
    game(id: $id) {
      ...GameFields
      saves {
        ...SaveFields
      }
      decoration {
        url
        width
        height
        top
        left
        bottom
        right
      }
    }
  }
`);

export const SavesQuery = graphql(`
  query Saves($id: Int!) {
    game(id: $id) {
      id
      saves {
        ...SaveFields
      }
    }
  }
`);

export const CheatsQuery = graphql(`
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
`);

export const AddCheatMutation = graphql(`
  mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {
    addCheat(gameId: $gameId, code: $code, description: $description) {
      id
      description
      code
      enabled
    }
  }
`);

export const SetCheatMutation = graphql(`
  mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {
    setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {
      id
      description
      code
      enabled
    }
  }
`);

export const ControllerMappingsQuery = graphql(`
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
`);

export const SaveControllerMappingMutation = graphql(`
  mutation SaveControllerMapping($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {
    saveControllerMapping(controller: $controller, system: $system, name: $name, bindings: $bindings) {
      controller
    }
  }
`);

export const PadLayoutFields = graphql(`
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
`);

export const SystemControlsQuery = graphql(`
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
`);

export const ControllerLayoutsQuery = graphql(`
  query ControllerLayouts {
    controllerLayouts {
      ...PadLayoutFields
    }
  }
`);

export const ControllerDefaultsQuery = graphql(`
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
`);

export const SaveControllerDefaultMutation = graphql(`
  mutation SaveControllerDefault($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {
    saveControllerDefault(controller: $controller, system: $system, name: $name, bindings: $bindings) {
      controller
    }
  }
`);

export const ResetControllerDefaultMutation = graphql(`
  mutation ResetControllerDefault($controller: String!, $system: ID) {
    resetControllerDefault(controller: $controller, system: $system)
  }
`);

export const ResetControllerMappingMutation = graphql(`
  mutation ResetControllerMapping($controller: String!, $system: ID) {
    resetControllerMapping(controller: $controller, system: $system)
  }
`);

export const StartSessionMutation = graphql(`
  mutation StartSession($gameId: Int!, $startId: String) {
    startSession(gameId: $gameId, startId: $startId) {
      ...SessionFields
    }
  }
`);

export const StartProgressQuery = graphql(`
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
`);

export const SessionOfferMutation = graphql(`
  mutation SessionOffer($id: ID!, $sdp: String!) {
    sessionOffer(id: $id, sdp: $sdp)
  }
`);

export const StopSessionMutation = graphql(`
  mutation StopSession($id: ID!) {
    stopSession(id: $id)
  }
`);

export const ResetSessionMutation = graphql(`
  mutation ResetSession($id: ID!) {
    resetSession(id: $id)
  }
`);

export const SetDiscMutation = graphql(`
  mutation SetDisc($sessionId: ID!, $disc: Int!) {
    setDisc(sessionId: $sessionId, disc: $disc)
  }
`);

export const SaveStateMutation = graphql(`
  mutation SaveState($sessionId: ID!, $slot: Int!) {
    saveState(sessionId: $sessionId, slot: $slot)
  }
`);

export const LoadStateMutation = graphql(`
  mutation LoadState($sessionId: ID!, $slot: Int!) {
    loadState(sessionId: $sessionId, slot: $slot)
  }
`);

export const ScanLibraryMutation = graphql(`
  mutation ScanLibrary {
    scanLibrary {
      found
      added
      removed
    }
  }
`);

export const SyncLibraryMutation = graphql(`
  mutation SyncLibrary {
    syncLibrary
  }
`);

export const AdminStatusQuery = graphql(`
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
`);

export const FetchMetadataMutation = graphql(`
  mutation FetchMetadata($system: ID, $force: Boolean!) {
    fetchMetadata(system: $system, force: $force) {
      ...MetadataFields
    }
  }
`);

export const EmulatorSettingsQuery = graphql(`
  query EmulatorSettings($system: ID!) {
    emulatorSettings(system: $system) {
      ...SettingsFields
    }
  }
`);

export const ChangeEmulatorSettingsMutation = graphql(`
  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {
    changeEmulatorSettings(system: $system, changes: $changes) {
      ...SettingsFields
    }
  }
`);
