// Every GraphQL operation the web client sends, typed from the schema by gql.tada (`gql.ts`).
// A fragment spread (`...GameFields`) passes its fragment as the second argument.
import { graphql } from "./gql";

export const GameFields = graphql(`
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
    partKind
  }
`);

export const SystemFields = graphql(`
  fragment SystemFields on System {
    id
    name
    short
    manufacturer
    year
    color
    photoUrl
    gameCount
    playable
    needsGpu
    rendering
    biosMissing
  }
`);

export const SaveFields = graphql(`
  fragment SaveFields on SaveSlot {
    slot
    savedAt
    screenshotUrl
  }
`);

export const SessionFields = graphql(`
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

export const SettingsFields = graphql(`
  fragment SettingsFields on EmulatorSettings {
    system
    core
    needsGame
    environment
    runAhead {
      frames
      max
      source
      supported
    }
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

export const MetadataFields = graphql(`
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
    current {
      gameId
      title
      system
      provider
      step
    }
    startedAt
    finishedAt
    etaSeconds
    systems {
      system
      total
      done
    }
    providers {
      id
      name
      asked
      found
      notFound
      errors
      quota {
        requestsToday
        maxRequestsPerDay
        missesToday
        maxMissesPerDay
      }
    }
    recent {
      gameId
      title
      system
      outcome
      sources
      detail
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
        role
        isAdmin
      }
    }
  }
`);

export const UsersQuery = graphql(`
  query Users {
    users {
      id
      username
      role
      declared
    }
  }
`);

export const SignInFields = graphql(`
  fragment SignInFields on SignInNode {
    id
    userId
    username
    client
    device
    signedInAt
    lastUsedAt
    expiresAt
  }
`);

export const SignInsQuery = graphql(
  `
  query SignIns {
    signIns {
      ...SignInFields
    }
  }
`,
  [SignInFields],
);

export const UserDetailsQuery = graphql(
  `
  query UserDetails($id: Int!) {
    user(id: $id) {
      user {
        id
        username
        role
        declared
      }
      joinedAt
      signIns {
        ...SignInFields
      }
      gamesPlayed
      starts
      lastPlayedAt
      saveBytes
      saveFiles
    }
  }
`,
  [SignInFields],
);

export const EndSignInMutation = graphql(`
  mutation EndSignIn($id: ID!) {
    endSignIn(id: $id)
  }
`);

export const AddUserMutation = graphql(`
  mutation AddUser($input: NewUserInput!) {
    addUser(input: $input) {
      id
    }
  }
`);

export const SetUserRoleMutation = graphql(`
  mutation SetUserRole($userId: Int!, $role: Role!) {
    setUserRole(userId: $userId, role: $role) {
      id
    }
  }
`);

export const SetUserPasswordMutation = graphql(`
  mutation SetUserPassword($userId: Int!, $password: String!) {
    setUserPassword(userId: $userId, password: $password)
  }
`);

export const RemoveUserMutation = graphql(`
  mutation RemoveUser($userId: Int!) {
    removeUser(userId: $userId)
  }
`);

export const SignOutEverywhereMutation = graphql(`
  mutation SignOutEverywhere($userId: Int!) {
    signOutEverywhere(userId: $userId)
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

export const ClientConfigQuery = graphql(`
  query ClientConfig {
    clientConfig {
      iceServers
    }
  }
`);

export const HomeQuery = graphql(
  `
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
`,
  [GameFields, SaveFields],
);

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

export const SearchQuery = graphql(
  `
  query Search($text: String!) {
    search(text: $text) {
      ...GameFields
    }
  }
`,
  [GameFields],
);

export const CollectionFields = graphql(
  `
  fragment CollectionFields on Collection {
    id
    name
    gameCount
    shared
    declared
    games {
      ...GameFields
    }
  }
`,
  [GameFields],
);

// The shared collections (an admin's, for everyone) and this user's own.
export const CollectionsQuery = graphql(
  `
  query Collections {
    sharedCollections {
      ...CollectionFields
    }
    collections {
      ...CollectionFields
    }
  }
`,
  [CollectionFields],
);

export const CollectionIdsQuery = graphql(`
  query CollectionIds($id: Int!) {
    game(id: $id) {
      id
      collectionIds
      sharedCollectionIds
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

export const CreateSharedCollectionMutation = graphql(`
  mutation CreateSharedCollection($name: String!) {
    createSharedCollection(name: $name) {
      id
      name
    }
  }
`);

export const RenameSharedCollectionMutation = graphql(`
  mutation RenameSharedCollection($id: Int!, $name: String!) {
    renameSharedCollection(id: $id, name: $name)
  }
`);

export const DeleteSharedCollectionMutation = graphql(`
  mutation DeleteSharedCollection($id: Int!) {
    deleteSharedCollection(id: $id)
  }
`);

export const SetInSharedCollectionMutation = graphql(`
  mutation SetInSharedCollection($collectionId: Int!, $gameId: Int!, $member: Boolean!) {
    setInSharedCollection(collectionId: $collectionId, gameId: $gameId, member: $member)
  }
`);

export const SystemsQuery = graphql(
  `
  query Systems {
    systems {
      ...SystemFields
    }
  }
`,
  [SystemFields],
);

export const BiosQuery = graphql(
  `
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
            expectedMd5
            size
          }
        }
      }
    }
  }
`,
  [SystemFields],
);

export const SystemGamesQuery = graphql(
  `
  query SystemGames($id: ID!) {
    system(id: $id) {
      ...SystemFields
      games {
        ...GameFields
      }
    }
  }
`,
  [SystemFields, GameFields],
);

export const GameQuery = graphql(
  `
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
`,
  [GameFields, SaveFields],
);

export const SavesQuery = graphql(
  `
  query Saves($id: Int!) {
    game(id: $id) {
      id
      saves {
        ...SaveFields
      }
    }
  }
`,
  [SaveFields],
);

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

export const SystemControlsQuery = graphql(
  `
  query SystemControls($id: ID!) {
    system(id: $id) {
      id
      controls {
        buttons {
          button
          label
        }
        sticks {
          stick
          label
          key
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
`,
  [PadLayoutFields],
);

export const ControllerLayoutsQuery = graphql(
  `
  query ControllerLayouts {
    controllerLayouts {
      ...PadLayoutFields
    }
  }
`,
  [PadLayoutFields],
);

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

export const StartSessionMutation = graphql(
  `
  mutation StartSession($gameId: Int!, $startId: String) {
    startSession(gameId: $gameId, startId: $startId) {
      ...SessionFields
    }
  }
`,
  [SessionFields],
);

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
  mutation SessionOffer($id: ID!, $sdp: String!, $client: String, $capabilities: ClientCapabilitiesInput) {
    sessionOffer(id: $id, sdp: $sdp, client: $client, capabilities: $capabilities)
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

export const AdminStatusQuery = graphql(
  `
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
`,
  [MetadataFields],
);

const JobFields = graphql(`
  fragment JobFields on JobState {
    name
    label
    description
    intervalMinutes
    nextRunAt
    running
    runningSince
    lastRun {
      trigger
      outcome
      startedAt
      finishedAt
      seconds
      message
    }
  }
`);

export const SessionFailuresQuery = graphql(`
  query SessionFailures {
    sessionFailures {
      at
      username
      gameId
      title
      system
      core
      whilePlaying
      reason
      message
      exitCode
      signal
      lastLines
      ranSeconds
    }
  }
`);

export const ActiveSessionsQuery = graphql(`
  query ActiveSessions {
    activeSessions {
      id
      username
      client
      gameId
      title
      system
      core
      startedAt
      connected
      codecChoice
      pipeline {
        renderer
        rendererSoftware
        videoCodec
        videoEncoder
        videoHardware
        fallbacks {
          encoder
          reason
        }
        width
        height
        fps
        audioCodec
        audioSourceHz
        audioKbps
        runAhead
      }
      health {
        fps
        overran
        paused
        tickMeanMs
        encodeMeanMs
        coreMeanMs
        readbackMeanMs
        videoKbps
        videoKbpsTarget
        estimateKbps
      }
      clientStats {
        estimateMs
        inputToFrameMs
        inputRttMs
        netRttMs
        decodeMs
        fps
        kbps
        packetsLost
        decoder
        lowLatencyDecode
      }
    }
  }
`);

export const JobsQuery = graphql(
  `
  query Jobs {
    jobs {
      ...JobFields
    }
  }
`,
  [JobFields],
);

export const RunJobMutation = graphql(
  `
  mutation RunJob($job: JobName!) {
    runJob(job: $job) {
      ...JobFields
    }
  }
`,
  [JobFields],
);

export const ServerResourcesQuery = graphql(`
  query ServerResources {
    serverResources {
      host {
        cpuModel
        arch
        hostCpus
        usableCpus
        maxMhz
        features
        memoryBytes
        kernel
      }
      sampleSeconds
      usage {
        sampledAt
        cpuPercent
        allowedCores
        cpuLimited
        memoryUsedBytes
        memoryLimitBytes
        memoryLimited
        memoryPercent
      }
      usageNote
    }
  }
`);

export const ResourceHistoryQuery = graphql(`
  query ResourceHistory($range: ResourceRange!) {
    resourceHistory(range: $range) {
      from
      to
      bucketSeconds
      series {
        resource
        points {
          at
          average
          max
        }
      }
    }
  }
`);

export const FetchMetadataMutation = graphql(
  `
  mutation FetchMetadata($system: ID, $force: Boolean!, $fields: [MetadataField!]!) {
    fetchMetadata(system: $system, force: $force, fields: $fields) {
      ...MetadataFields
    }
  }
`,
  [MetadataFields],
);

export const EmulatorSettingsQuery = graphql(
  `
  query EmulatorSettings($system: ID!) {
    emulatorSettings(system: $system) {
      ...SettingsFields
    }
  }
`,
  [SettingsFields],
);

export const ReportClientEventsMutation = graphql(`
  mutation ReportClientEvents($client: String!, $events: [ClientEventInput!]!) {
    reportClientEvents(client: $client, events: $events)
  }
`);

export const SetRunAheadMutation = graphql(
  `
  mutation SetRunAhead($system: ID!, $frames: Int) {
    setRunAhead(system: $system, frames: $frames) {
      ...SettingsFields
    }
  }
`,
  [SettingsFields],
);

export const ChangeEmulatorSettingsMutation = graphql(
  `
  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {
    changeEmulatorSettings(system: $system, changes: $changes) {
      ...SettingsFields
    }
  }
`,
  [SettingsFields],
);

export const GpuQuery = graphql(`
  query Gpu {
    gpu {
      setting
      mode
      renderer
      note
      workerArg
      probedAt
      devices {
        node
        driver
        kind
        vendor
        renderer
        version
        works
        error
        chosen
      }
    }
  }
`);

export const BiosDirQuery = graphql(`
  query BiosDir {
    biosDir
  }
`);

export const DecorationsQuery = graphql(`
  query Decorations {
    decorations {
      default
      folders
      sets {
        name
        builtin
        hasDefault
        systems
        games
      }
      systems {
        set
        perSystem
        system {
          id
          name
          color
          gameCount
        }
        preview {
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
  }
`);

export const AdminPatchesQuery = graphql(`
  query AdminPatches {
    patches {
      id
      name
      file
      format
      isDefault
      gameId
      gameTitle
      system
      chosenBy
      originalBy
    }
  }
`);

export const CoresQuery = graphql(`
  query Cores {
    libretroRelease
    cores {
      name
      label
      version
      file
      loaded
      error
      licence
      nonCommercial
      sourceUrl
      systems {
        id
        name
        chosen
      }
    }
  }
`);

export const ConfigSettingFields = graphql(`
  fragment ConfigSettingFields on ConfigSetting {
    group
    name
    value
    effective
    source
    secret
    default
    description
  }
`);

export const ConfigurationQuery = graphql(
  `
  query Configuration {
    configuration {
      ...ConfigSettingFields
    }
  }
`,
  [ConfigSettingFields],
);

export const NetworkQuery = graphql(
  `
  query Network {
    network {
      http
      udpFirst
      udpLast
      portCount
      portsInUse
      sessions
      udpBind
      advertisedIp
      advertisedIpDetected
      publicUrls
      publicUrlsConfigured
      signInAvailableHere
      requestHost
      clientIp
      forwardedFor
      trustedProxies
      secureCookies
      iceServers {
        urls
        hasCredential
      }
      settings {
        ...ConfigSettingFields
      }
    }
  }
`,
  [ConfigSettingFields],
);

export const StorageQuery = graphql(`
  query Storage($refresh: Boolean) {
    storage(refresh: $refresh) {
      measuredAt
      dataDir
      totalBytes
      freeBytes
      cacheLimitBytes
      areas {
        id
        path
        bytes
        files
      }
      saves {
        userId
        username
        bytes
        files
      }
    }
  }
`);

export const StreamingQuery = graphql(
  `
  query Streaming {
    streaming {
      testSeconds
      settings {
        ...ConfigSettingFields
      }
      encoderTest {
        tookMs
        budgetMs
        note
        testedAt
        encoders {
          codec
          encoder
          hardware
          threads
          works
          error
          timings {
            width
            height
            meanMs
            p95Ms
          }
        }
      }
    }
  }
`,
  [ConfigSettingFields],
);

export const TestConnectionMutation = graphql(`
  mutation TestConnection($sdp: String!) {
    testConnection(sdp: $sdp)
  }
`);

export const ClientEventGroupsQuery = graphql(`
  query ClientEventGroups($days: Int!) {
    clientEventGroups(days: $days) {
      kind
      client
      count
      users
      lastAt
      lastMessage
    }
  }
`);

export const ClientEventsQuery = graphql(`
  query ClientEvents($kind: ClientEventKind!, $client: String!, $days: Int!) {
    clientEvents(kind: $kind, client: $client, days: $days) {
      at
      clientAt
      username
      client
      kind
      message
      sessionId
      gameId
      title
      system
    }
  }
`);

export const ActivityQuery = graphql(`
  query Activity($userId: Int, $category: ActivityCategory, $after: String, $limit: Int!) {
    activity(userId: $userId, category: $category, after: $after, limit: $limit) {
      entries {
        at
        userId
        username
        kind
        category
        subject
        detail
        address
      }
      next
    }
  }
`);

export const PlayStatisticsQuery = graphql(`
  query PlayStatistics($range: StatsRange!) {
    playStatistics(range: $range) {
      from
      to
      totals {
        sessions
        seconds
        players
        games
      }
      days {
        day
        sessions
        seconds
      }
      games {
        gameId
        title
        system
        sessions
        seconds
        players
      }
      players {
        userId
        username
        sessions
        seconds
        games
      }
      systems {
        system
        sessions
        seconds
        players
      }
      favourites {
        gameId
        title
        system
        users
      }
    }
  }
`);

export const LogsQuery = graphql(`
  query Logs($level: LogLevel, $session: String, $text: String) {
    logs(level: $level, session: $session, text: $text) {
      enabled
      lines {
        id
        at
        level
        target
        message
        fields {
          key
          value
        }
        session
      }
      cursor
    }
  }
`);

// Games needing attention: one kind's games (or files), and how many of each kind per system.
export const GamesNeedingAttentionQuery = graphql(`
  query GamesNeedingAttention($kind: AttentionKind!, $system: ID) {
    gamesNeedingAttention(kind: $kind, system: $system) {
      gameId
      system
      title
      path
      detail
    }
  }
`);

export const AttentionCountsQuery = graphql(`
  query AttentionCounts {
    attentionCounts {
      kind
      system
      count
    }
  }
`);

export const LookUpGameMutation = graphql(`
  mutation LookUpGame($gameId: Int!) {
    lookUpGame(gameId: $gameId) {
      running
    }
  }
`);

export const BackupsQuery = graphql(`
  query Backups {
    backups {
      dir
      intervalHours
      keep
      restoreFrom
      files {
        name
        bytes
        created
        url
      }
    }
  }
`);

export const DeleteBackupMutation = graphql(`
  mutation DeleteBackup($name: String!) {
    deleteBackup(name: $name)
  }
`);

// The admin Overview: one read for every card, each summarising a page it links to.
export const OverviewQuery = graphql(`
  query Overview {
    network {
      portCount
      portsInUse
    }
    activeSessions {
      id
      username
      title
    }
    libraryStatus {
      mode
      games
    }
    jobs {
      name
      running
      lastRun {
        outcome
        finishedAt
        message
      }
    }
    metadataStatus {
      running
      total
      found
    }
    systems {
      id
      name
      biosMissing
    }
    gpu {
      mode
      renderer
    }
    storage {
      totalBytes
      freeBytes
    }
    buildInfo {
      version
      revision
    }
    serverResources {
      startedAt
    }
    sessionFailures {
      at
    }
    clientEventGroups(days: 1) {
      count
    }
  }
`);

export const ClearCacheMutation = graphql(`
  mutation ClearCache {
    clearCache
  }
`);

export const TelemetryQuery = graphql(
  `
  query Telemetry {
    telemetry {
      enabled
      endpoint
      logFormat
      serviceName
      metricIntervalMs
      headerNames
      traces {
        lastAt
        ok
        error
        exports
        failures
      }
      metrics {
        lastAt
        ok
        error
        exports
        failures
      }
      settings {
        ...ConfigSettingFields
      }
    }
  }
`,
  [ConfigSettingFields],
);

export const AboutQuery = graphql(`
  query About {
    buildInfo {
      version
      revision
      built
      libretroRelease
      licence
      source
    }
    attributions {
      fbneoCommit
      services {
        name
        provides
        url
        licence
        licenceUrl
        terms
      }
      components {
        name
        what
        licence
        url
        commit
      }
    }
  }
`);

// Every open-source package the server and the web client ship, with its licence: read when the
// About page's list is opened.
export const NoticeFields = graphql(`
  fragment NoticeFields on Notice {
    id
    packages {
      name
      version
      repository
    }
    text
  }
`);

export const ThirdPartyNoticesQuery = graphql(
  `
  query ThirdPartyNotices {
    thirdPartyNotices {
      server {
        ...NoticeFields
      }
      web {
        ...NoticeFields
      }
    }
  }
`,
  [NoticeFields],
);

export const BackgroundQuery = graphql(`
  query Background {
    background {
      id
      name
      url
    }
  }
`);

export const AppearanceQuery = graphql(`
  query Appearance {
    appearance {
      chosen
      fromEnvironment
      environment
      backgrounds {
        id
        name
        url
        builtin
      }
    }
  }
`);

export const SetBackgroundMutation = graphql(`
  mutation SetBackground($id: String!) {
    setBackground(id: $id)
  }
`);
