/* eslint-disable */
import * as types from './graphql';



/**
 * Map of all GraphQL operations in the project.
 *
 * This map has several performance disadvantages:
 * 1. It is not tree-shakeable, so it will include all operations in the project.
 * 2. It is not minifiable, so the string of a GraphQL query will be multiple times inside the bundle.
 * 3. It does not support dead code elimination, so it will add unused operations.
 *
 * Therefore it is highly recommended to use the babel or swc plugin for production.
 * Learn more about it here: https://the-guild.dev/graphql/codegen/plugins/presets/preset-client#reducing-bundle-size
 */
type Documents = {
    "\n  fragment GameFields on Game {\n    id\n    system\n    title\n    fileTitle\n    tags\n    path\n    size\n    lastPlayedAt\n    playCount\n    image: mediaUrl(kind: IMAGE)\n    thumb: mediaUrl(kind: THUMB)\n    video: mediaUrl(kind: VIDEO)\n    description\n    releaseDate\n    developer\n    publisher\n    genre\n    players\n    rating\n  }\n": typeof types.GameFieldsFragmentDoc,
    "\n  fragment SystemFields on System {\n    id\n    name\n    short\n    manufacturer\n    year\n    color\n    gameCount\n    playable\n    needsGpu\n    rendering\n    biosMissing\n  }\n": typeof types.SystemFieldsFragmentDoc,
    "\n  fragment SaveFields on SaveSlot {\n    slot\n    savedAt\n    screenshotUrl\n  }\n": typeof types.SaveFieldsFragmentDoc,
    "\n  fragment SessionFields on GameSession {\n    id\n    gameId\n    system\n    title\n    core\n    width\n    height\n    fps\n    connected\n  }\n": typeof types.SessionFieldsFragmentDoc,
    "\n  fragment SettingsFields on EmulatorSettings {\n    system\n    core\n    needsGame\n    environment\n    categories {\n      key\n      description\n      info\n    }\n    settings {\n      key\n      description\n      info\n      category\n      choices\n      labels\n      default\n      value\n      source\n    }\n  }\n": typeof types.SettingsFieldsFragmentDoc,
    "\n  fragment MetadataFields on MetadataStatus {\n    running\n    total\n    done\n    found\n    notFound\n    failed\n    sources\n    notes\n  }\n": typeof types.MetadataFieldsFragmentDoc,
    "\n  query Viewer {\n    viewer {\n      setupRequired\n      user {\n        id\n        username\n        isAdmin\n      }\n    }\n  }\n": typeof types.ViewerDocument,
    "\n  mutation Setup($username: String!, $password: String!) {\n    setup(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n": typeof types.SetupDocument,
    "\n  mutation Login($username: String!, $password: String!) {\n    login(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n": typeof types.LoginDocument,
    "\n  mutation Logout {\n    logout\n  }\n": typeof types.LogoutDocument,
    "\n  query ClientConfig {\n    clientConfig {\n      iceServers\n    }\n  }\n": typeof types.ClientConfigDocument,
    "\n  query Home {\n    home {\n      featured {\n        ...GameFields\n      }\n      recent {\n        ...GameFields\n        autoSave {\n          ...SaveFields\n        }\n      }\n      added {\n        ...GameFields\n      }\n    }\n  }\n": typeof types.HomeDocument,
    "\n  query Systems {\n    systems {\n      ...SystemFields\n    }\n  }\n": typeof types.SystemsDocument,
    "\n  query Bios {\n    systems {\n      ...SystemFields\n      bios {\n        state\n        folder\n        groups {\n          label\n          importance\n          help\n          anyOne\n          satisfied\n          files {\n            name\n            label\n            state\n            foundAt\n            md5\n            size\n          }\n        }\n      }\n    }\n  }\n": typeof types.BiosDocument,
    "\n  query SystemGames($id: ID!) {\n    system(id: $id) {\n      ...SystemFields\n      games {\n        ...GameFields\n      }\n    }\n  }\n": typeof types.SystemGamesDocument,
    "\n  query Game($id: Int!) {\n    game(id: $id) {\n      ...GameFields\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n": typeof types.GameDocument,
    "\n  query Saves($id: Int!) {\n    game(id: $id) {\n      id\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n": typeof types.SavesDocument,
    "\n  query Cheats($id: Int!) {\n    game(id: $id) {\n      id\n      cheats {\n        id\n        description\n        code\n        enabled\n      }\n    }\n  }\n": typeof types.CheatsDocument,
    "\n  mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {\n    addCheat(gameId: $gameId, code: $code, description: $description) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": typeof types.AddCheatDocument,
    "\n  mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {\n    setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": typeof types.SetCheatDocument,
    "\n  query ControllerMappings {\n    controllerMappings {\n      controller\n      name\n      source\n      bindings {\n        input\n        button\n      }\n    }\n  }\n": typeof types.ControllerMappingsDocument,
    "\n  mutation SaveControllerMapping($controller: String!, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerMapping(controller: $controller, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": typeof types.SaveControllerMappingDocument,
    "\n  query ControllerDefaults {\n    controllerDefaults {\n      mappings {\n        controller\n        name\n        source\n        bindings {\n          input\n          button\n        }\n      }\n      environment\n    }\n  }\n": typeof types.ControllerDefaultsDocument,
    "\n  mutation SaveControllerDefault($controller: String!, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerDefault(controller: $controller, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": typeof types.SaveControllerDefaultDocument,
    "\n  mutation ResetControllerDefault($controller: String!) {\n    resetControllerDefault(controller: $controller)\n  }\n": typeof types.ResetControllerDefaultDocument,
    "\n  mutation ResetControllerMapping($controller: String!) {\n    resetControllerMapping(controller: $controller)\n  }\n": typeof types.ResetControllerMappingDocument,
    "\n  mutation StartSession($gameId: Int!, $startId: String) {\n    startSession(gameId: $gameId, startId: $startId) {\n      ...SessionFields\n    }\n  }\n": typeof types.StartSessionDocument,
    "\n  query StartProgress($startId: String!) {\n    startProgress(startId: $startId) {\n      phase\n      read\n      total\n      elapsedMs\n      phaseMs\n      error\n    }\n  }\n": typeof types.StartProgressDocument,
    "\n  mutation SessionOffer($id: ID!, $sdp: String!) {\n    sessionOffer(id: $id, sdp: $sdp)\n  }\n": typeof types.SessionOfferDocument,
    "\n  mutation StopSession($id: ID!) {\n    stopSession(id: $id)\n  }\n": typeof types.StopSessionDocument,
    "\n  mutation ResetSession($id: ID!) {\n    resetSession(id: $id)\n  }\n": typeof types.ResetSessionDocument,
    "\n  mutation SaveState($sessionId: ID!, $slot: Int!) {\n    saveState(sessionId: $sessionId, slot: $slot)\n  }\n": typeof types.SaveStateDocument,
    "\n  mutation LoadState($sessionId: ID!, $slot: Int!) {\n    loadState(sessionId: $sessionId, slot: $slot)\n  }\n": typeof types.LoadStateDocument,
    "\n  mutation ScanLibrary {\n    scanLibrary {\n      found\n      added\n      removed\n    }\n  }\n": typeof types.ScanLibraryDocument,
    "\n  mutation SyncLibrary {\n    syncLibrary\n  }\n": typeof types.SyncLibraryDocument,
    "\n  query AdminStatus {\n    libraryStatus {\n      mode\n      games\n      cacheBytes\n      mirror {\n        running\n        filesTotal\n        filesDone\n        bytesTotal\n        bytesDone\n        removed\n        failed\n        finishedAt\n        error\n      }\n    }\n    metadataStatus {\n      ...MetadataFields\n    }\n  }\n": typeof types.AdminStatusDocument,
    "\n  mutation FetchMetadata($system: ID, $force: Boolean!) {\n    fetchMetadata(system: $system, force: $force) {\n      ...MetadataFields\n    }\n  }\n": typeof types.FetchMetadataDocument,
    "\n  query EmulatorSettings($system: ID!) {\n    emulatorSettings(system: $system) {\n      ...SettingsFields\n    }\n  }\n": typeof types.EmulatorSettingsDocument,
    "\n  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {\n    changeEmulatorSettings(system: $system, changes: $changes) {\n      ...SettingsFields\n    }\n  }\n": typeof types.ChangeEmulatorSettingsDocument,
};
const documents: Documents = {
    "\n  fragment GameFields on Game {\n    id\n    system\n    title\n    fileTitle\n    tags\n    path\n    size\n    lastPlayedAt\n    playCount\n    image: mediaUrl(kind: IMAGE)\n    thumb: mediaUrl(kind: THUMB)\n    video: mediaUrl(kind: VIDEO)\n    description\n    releaseDate\n    developer\n    publisher\n    genre\n    players\n    rating\n  }\n": types.GameFieldsFragmentDoc,
    "\n  fragment SystemFields on System {\n    id\n    name\n    short\n    manufacturer\n    year\n    color\n    gameCount\n    playable\n    needsGpu\n    rendering\n    biosMissing\n  }\n": types.SystemFieldsFragmentDoc,
    "\n  fragment SaveFields on SaveSlot {\n    slot\n    savedAt\n    screenshotUrl\n  }\n": types.SaveFieldsFragmentDoc,
    "\n  fragment SessionFields on GameSession {\n    id\n    gameId\n    system\n    title\n    core\n    width\n    height\n    fps\n    connected\n  }\n": types.SessionFieldsFragmentDoc,
    "\n  fragment SettingsFields on EmulatorSettings {\n    system\n    core\n    needsGame\n    environment\n    categories {\n      key\n      description\n      info\n    }\n    settings {\n      key\n      description\n      info\n      category\n      choices\n      labels\n      default\n      value\n      source\n    }\n  }\n": types.SettingsFieldsFragmentDoc,
    "\n  fragment MetadataFields on MetadataStatus {\n    running\n    total\n    done\n    found\n    notFound\n    failed\n    sources\n    notes\n  }\n": types.MetadataFieldsFragmentDoc,
    "\n  query Viewer {\n    viewer {\n      setupRequired\n      user {\n        id\n        username\n        isAdmin\n      }\n    }\n  }\n": types.ViewerDocument,
    "\n  mutation Setup($username: String!, $password: String!) {\n    setup(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n": types.SetupDocument,
    "\n  mutation Login($username: String!, $password: String!) {\n    login(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n": types.LoginDocument,
    "\n  mutation Logout {\n    logout\n  }\n": types.LogoutDocument,
    "\n  query ClientConfig {\n    clientConfig {\n      iceServers\n    }\n  }\n": types.ClientConfigDocument,
    "\n  query Home {\n    home {\n      featured {\n        ...GameFields\n      }\n      recent {\n        ...GameFields\n        autoSave {\n          ...SaveFields\n        }\n      }\n      added {\n        ...GameFields\n      }\n    }\n  }\n": types.HomeDocument,
    "\n  query Systems {\n    systems {\n      ...SystemFields\n    }\n  }\n": types.SystemsDocument,
    "\n  query Bios {\n    systems {\n      ...SystemFields\n      bios {\n        state\n        folder\n        groups {\n          label\n          importance\n          help\n          anyOne\n          satisfied\n          files {\n            name\n            label\n            state\n            foundAt\n            md5\n            size\n          }\n        }\n      }\n    }\n  }\n": types.BiosDocument,
    "\n  query SystemGames($id: ID!) {\n    system(id: $id) {\n      ...SystemFields\n      games {\n        ...GameFields\n      }\n    }\n  }\n": types.SystemGamesDocument,
    "\n  query Game($id: Int!) {\n    game(id: $id) {\n      ...GameFields\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n": types.GameDocument,
    "\n  query Saves($id: Int!) {\n    game(id: $id) {\n      id\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n": types.SavesDocument,
    "\n  query Cheats($id: Int!) {\n    game(id: $id) {\n      id\n      cheats {\n        id\n        description\n        code\n        enabled\n      }\n    }\n  }\n": types.CheatsDocument,
    "\n  mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {\n    addCheat(gameId: $gameId, code: $code, description: $description) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": types.AddCheatDocument,
    "\n  mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {\n    setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": types.SetCheatDocument,
    "\n  query ControllerMappings {\n    controllerMappings {\n      controller\n      name\n      source\n      bindings {\n        input\n        button\n      }\n    }\n  }\n": types.ControllerMappingsDocument,
    "\n  mutation SaveControllerMapping($controller: String!, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerMapping(controller: $controller, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": types.SaveControllerMappingDocument,
    "\n  query ControllerDefaults {\n    controllerDefaults {\n      mappings {\n        controller\n        name\n        source\n        bindings {\n          input\n          button\n        }\n      }\n      environment\n    }\n  }\n": types.ControllerDefaultsDocument,
    "\n  mutation SaveControllerDefault($controller: String!, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerDefault(controller: $controller, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": types.SaveControllerDefaultDocument,
    "\n  mutation ResetControllerDefault($controller: String!) {\n    resetControllerDefault(controller: $controller)\n  }\n": types.ResetControllerDefaultDocument,
    "\n  mutation ResetControllerMapping($controller: String!) {\n    resetControllerMapping(controller: $controller)\n  }\n": types.ResetControllerMappingDocument,
    "\n  mutation StartSession($gameId: Int!, $startId: String) {\n    startSession(gameId: $gameId, startId: $startId) {\n      ...SessionFields\n    }\n  }\n": types.StartSessionDocument,
    "\n  query StartProgress($startId: String!) {\n    startProgress(startId: $startId) {\n      phase\n      read\n      total\n      elapsedMs\n      phaseMs\n      error\n    }\n  }\n": types.StartProgressDocument,
    "\n  mutation SessionOffer($id: ID!, $sdp: String!) {\n    sessionOffer(id: $id, sdp: $sdp)\n  }\n": types.SessionOfferDocument,
    "\n  mutation StopSession($id: ID!) {\n    stopSession(id: $id)\n  }\n": types.StopSessionDocument,
    "\n  mutation ResetSession($id: ID!) {\n    resetSession(id: $id)\n  }\n": types.ResetSessionDocument,
    "\n  mutation SaveState($sessionId: ID!, $slot: Int!) {\n    saveState(sessionId: $sessionId, slot: $slot)\n  }\n": types.SaveStateDocument,
    "\n  mutation LoadState($sessionId: ID!, $slot: Int!) {\n    loadState(sessionId: $sessionId, slot: $slot)\n  }\n": types.LoadStateDocument,
    "\n  mutation ScanLibrary {\n    scanLibrary {\n      found\n      added\n      removed\n    }\n  }\n": types.ScanLibraryDocument,
    "\n  mutation SyncLibrary {\n    syncLibrary\n  }\n": types.SyncLibraryDocument,
    "\n  query AdminStatus {\n    libraryStatus {\n      mode\n      games\n      cacheBytes\n      mirror {\n        running\n        filesTotal\n        filesDone\n        bytesTotal\n        bytesDone\n        removed\n        failed\n        finishedAt\n        error\n      }\n    }\n    metadataStatus {\n      ...MetadataFields\n    }\n  }\n": types.AdminStatusDocument,
    "\n  mutation FetchMetadata($system: ID, $force: Boolean!) {\n    fetchMetadata(system: $system, force: $force) {\n      ...MetadataFields\n    }\n  }\n": types.FetchMetadataDocument,
    "\n  query EmulatorSettings($system: ID!) {\n    emulatorSettings(system: $system) {\n      ...SettingsFields\n    }\n  }\n": types.EmulatorSettingsDocument,
    "\n  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {\n    changeEmulatorSettings(system: $system, changes: $changes) {\n      ...SettingsFields\n    }\n  }\n": types.ChangeEmulatorSettingsDocument,
};

/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment GameFields on Game {\n    id\n    system\n    title\n    fileTitle\n    tags\n    path\n    size\n    lastPlayedAt\n    playCount\n    image: mediaUrl(kind: IMAGE)\n    thumb: mediaUrl(kind: THUMB)\n    video: mediaUrl(kind: VIDEO)\n    description\n    releaseDate\n    developer\n    publisher\n    genre\n    players\n    rating\n  }\n"): typeof import('./graphql').GameFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment SystemFields on System {\n    id\n    name\n    short\n    manufacturer\n    year\n    color\n    gameCount\n    playable\n    needsGpu\n    rendering\n    biosMissing\n  }\n"): typeof import('./graphql').SystemFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment SaveFields on SaveSlot {\n    slot\n    savedAt\n    screenshotUrl\n  }\n"): typeof import('./graphql').SaveFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment SessionFields on GameSession {\n    id\n    gameId\n    system\n    title\n    core\n    width\n    height\n    fps\n    connected\n  }\n"): typeof import('./graphql').SessionFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment SettingsFields on EmulatorSettings {\n    system\n    core\n    needsGame\n    environment\n    categories {\n      key\n      description\n      info\n    }\n    settings {\n      key\n      description\n      info\n      category\n      choices\n      labels\n      default\n      value\n      source\n    }\n  }\n"): typeof import('./graphql').SettingsFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment MetadataFields on MetadataStatus {\n    running\n    total\n    done\n    found\n    notFound\n    failed\n    sources\n    notes\n  }\n"): typeof import('./graphql').MetadataFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Viewer {\n    viewer {\n      setupRequired\n      user {\n        id\n        username\n        isAdmin\n      }\n    }\n  }\n"): typeof import('./graphql').ViewerDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation Setup($username: String!, $password: String!) {\n    setup(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n"): typeof import('./graphql').SetupDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation Login($username: String!, $password: String!) {\n    login(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n"): typeof import('./graphql').LoginDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation Logout {\n    logout\n  }\n"): typeof import('./graphql').LogoutDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query ClientConfig {\n    clientConfig {\n      iceServers\n    }\n  }\n"): typeof import('./graphql').ClientConfigDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Home {\n    home {\n      featured {\n        ...GameFields\n      }\n      recent {\n        ...GameFields\n        autoSave {\n          ...SaveFields\n        }\n      }\n      added {\n        ...GameFields\n      }\n    }\n  }\n"): typeof import('./graphql').HomeDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Systems {\n    systems {\n      ...SystemFields\n    }\n  }\n"): typeof import('./graphql').SystemsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Bios {\n    systems {\n      ...SystemFields\n      bios {\n        state\n        folder\n        groups {\n          label\n          importance\n          help\n          anyOne\n          satisfied\n          files {\n            name\n            label\n            state\n            foundAt\n            md5\n            size\n          }\n        }\n      }\n    }\n  }\n"): typeof import('./graphql').BiosDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query SystemGames($id: ID!) {\n    system(id: $id) {\n      ...SystemFields\n      games {\n        ...GameFields\n      }\n    }\n  }\n"): typeof import('./graphql').SystemGamesDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Game($id: Int!) {\n    game(id: $id) {\n      ...GameFields\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n"): typeof import('./graphql').GameDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Saves($id: Int!) {\n    game(id: $id) {\n      id\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n"): typeof import('./graphql').SavesDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Cheats($id: Int!) {\n    game(id: $id) {\n      id\n      cheats {\n        id\n        description\n        code\n        enabled\n      }\n    }\n  }\n"): typeof import('./graphql').CheatsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {\n    addCheat(gameId: $gameId, code: $code, description: $description) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n"): typeof import('./graphql').AddCheatDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {\n    setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n"): typeof import('./graphql').SetCheatDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query ControllerMappings {\n    controllerMappings {\n      controller\n      name\n      source\n      bindings {\n        input\n        button\n      }\n    }\n  }\n"): typeof import('./graphql').ControllerMappingsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SaveControllerMapping($controller: String!, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerMapping(controller: $controller, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n"): typeof import('./graphql').SaveControllerMappingDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query ControllerDefaults {\n    controllerDefaults {\n      mappings {\n        controller\n        name\n        source\n        bindings {\n          input\n          button\n        }\n      }\n      environment\n    }\n  }\n"): typeof import('./graphql').ControllerDefaultsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SaveControllerDefault($controller: String!, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerDefault(controller: $controller, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n"): typeof import('./graphql').SaveControllerDefaultDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ResetControllerDefault($controller: String!) {\n    resetControllerDefault(controller: $controller)\n  }\n"): typeof import('./graphql').ResetControllerDefaultDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ResetControllerMapping($controller: String!) {\n    resetControllerMapping(controller: $controller)\n  }\n"): typeof import('./graphql').ResetControllerMappingDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation StartSession($gameId: Int!, $startId: String) {\n    startSession(gameId: $gameId, startId: $startId) {\n      ...SessionFields\n    }\n  }\n"): typeof import('./graphql').StartSessionDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query StartProgress($startId: String!) {\n    startProgress(startId: $startId) {\n      phase\n      read\n      total\n      elapsedMs\n      phaseMs\n      error\n    }\n  }\n"): typeof import('./graphql').StartProgressDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SessionOffer($id: ID!, $sdp: String!) {\n    sessionOffer(id: $id, sdp: $sdp)\n  }\n"): typeof import('./graphql').SessionOfferDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation StopSession($id: ID!) {\n    stopSession(id: $id)\n  }\n"): typeof import('./graphql').StopSessionDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ResetSession($id: ID!) {\n    resetSession(id: $id)\n  }\n"): typeof import('./graphql').ResetSessionDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SaveState($sessionId: ID!, $slot: Int!) {\n    saveState(sessionId: $sessionId, slot: $slot)\n  }\n"): typeof import('./graphql').SaveStateDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation LoadState($sessionId: ID!, $slot: Int!) {\n    loadState(sessionId: $sessionId, slot: $slot)\n  }\n"): typeof import('./graphql').LoadStateDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ScanLibrary {\n    scanLibrary {\n      found\n      added\n      removed\n    }\n  }\n"): typeof import('./graphql').ScanLibraryDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SyncLibrary {\n    syncLibrary\n  }\n"): typeof import('./graphql').SyncLibraryDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query AdminStatus {\n    libraryStatus {\n      mode\n      games\n      cacheBytes\n      mirror {\n        running\n        filesTotal\n        filesDone\n        bytesTotal\n        bytesDone\n        removed\n        failed\n        finishedAt\n        error\n      }\n    }\n    metadataStatus {\n      ...MetadataFields\n    }\n  }\n"): typeof import('./graphql').AdminStatusDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation FetchMetadata($system: ID, $force: Boolean!) {\n    fetchMetadata(system: $system, force: $force) {\n      ...MetadataFields\n    }\n  }\n"): typeof import('./graphql').FetchMetadataDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query EmulatorSettings($system: ID!) {\n    emulatorSettings(system: $system) {\n      ...SettingsFields\n    }\n  }\n"): typeof import('./graphql').EmulatorSettingsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {\n    changeEmulatorSettings(system: $system, changes: $changes) {\n      ...SettingsFields\n    }\n  }\n"): typeof import('./graphql').ChangeEmulatorSettingsDocument;


export function graphql(source: string) {
  return (documents as any)[source] ?? {};
}
