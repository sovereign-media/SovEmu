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
    "\n  fragment GameFields on Game {\n    id\n    system\n    title\n    fileTitle\n    tags\n    path\n    size\n    lastPlayedAt\n    playCount\n    image: mediaUrl(kind: IMAGE)\n    thumb: mediaUrl(kind: THUMB)\n    video: mediaUrl(kind: VIDEO)\n    description\n    releaseDate\n    developer\n    publisher\n    genre\n    players\n    rating\n    favourite\n    release\n    patch\n    discs\n    partKind\n  }\n": typeof types.GameFieldsFragmentDoc,
    "\n  fragment SystemFields on System {\n    id\n    name\n    short\n    manufacturer\n    year\n    color\n    gameCount\n    playable\n    needsGpu\n    rendering\n    biosMissing\n  }\n": typeof types.SystemFieldsFragmentDoc,
    "\n  fragment SaveFields on SaveSlot {\n    slot\n    savedAt\n    screenshotUrl\n  }\n": typeof types.SaveFieldsFragmentDoc,
    "\n  fragment SessionFields on GameSession {\n    id\n    gameId\n    system\n    title\n    core\n    width\n    height\n    fps\n    connected\n    discs\n    disc\n  }\n": typeof types.SessionFieldsFragmentDoc,
    "\n  fragment SettingsFields on EmulatorSettings {\n    system\n    core\n    needsGame\n    environment\n    runAhead {\n      frames\n      max\n      source\n      supported\n    }\n    categories {\n      key\n      description\n      info\n    }\n    settings {\n      key\n      description\n      info\n      category\n      choices\n      labels\n      default\n      value\n      source\n    }\n  }\n": typeof types.SettingsFieldsFragmentDoc,
    "\n  fragment MetadataFields on MetadataStatus {\n    running\n    total\n    done\n    found\n    notFound\n    failed\n    sources\n    notes\n    artwork {\n      running\n      total\n      done\n      downloaded\n      failed\n    }\n  }\n": typeof types.MetadataFieldsFragmentDoc,
    "\n  query Viewer {\n    viewer {\n      setupRequired\n      user {\n        id\n        username\n        role\n        isAdmin\n      }\n    }\n  }\n": typeof types.ViewerDocument,
    "\n  query Users {\n    users {\n      id\n      username\n      role\n    }\n  }\n": typeof types.UsersDocument,
    "\n  mutation Setup($username: String!, $password: String!) {\n    setup(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n": typeof types.SetupDocument,
    "\n  query ClientConfig {\n    clientConfig {\n      iceServers\n    }\n  }\n": typeof types.ClientConfigDocument,
    "\n  query Home {\n    home {\n      featured {\n        ...GameFields\n      }\n      recent {\n        ...GameFields\n        autoSave {\n          ...SaveFields\n        }\n      }\n      added {\n        ...GameFields\n      }\n      favourites {\n        ...GameFields\n      }\n    }\n  }\n": typeof types.HomeDocument,
    "\n  query Patches($id: Int!) {\n    game(id: $id) {\n      id\n      patches {\n        id\n        name\n        isDefault\n      }\n    }\n  }\n": typeof types.PatchesDocument,
    "\n  mutation SetPatch($gameId: Int!, $patchId: Int) {\n    setPatch(gameId: $gameId, patchId: $patchId)\n  }\n": typeof types.SetPatchDocument,
    "\n  query Search($text: String!) {\n    search(text: $text) {\n      ...GameFields\n    }\n  }\n": typeof types.SearchDocument,
    "\n  query Collections {\n    collections {\n      id\n      name\n      gameCount\n      games {\n        ...GameFields\n      }\n    }\n  }\n": typeof types.CollectionsDocument,
    "\n  query CollectionIds($id: Int!) {\n    game(id: $id) {\n      id\n      collectionIds\n    }\n  }\n": typeof types.CollectionIdsDocument,
    "\n  mutation SetFavourite($gameId: Int!, $favourite: Boolean!) {\n    setFavourite(gameId: $gameId, favourite: $favourite)\n  }\n": typeof types.SetFavouriteDocument,
    "\n  mutation CreateCollection($name: String!) {\n    createCollection(name: $name) {\n      id\n      name\n      gameCount\n    }\n  }\n": typeof types.CreateCollectionDocument,
    "\n  mutation DeleteCollection($id: Int!) {\n    deleteCollection(id: $id)\n  }\n": typeof types.DeleteCollectionDocument,
    "\n  mutation SetInCollection($collectionId: Int!, $gameId: Int!, $member: Boolean!) {\n    setInCollection(collectionId: $collectionId, gameId: $gameId, member: $member)\n  }\n": typeof types.SetInCollectionDocument,
    "\n  query Systems {\n    systems {\n      ...SystemFields\n    }\n  }\n": typeof types.SystemsDocument,
    "\n  query Bios {\n    systems {\n      ...SystemFields\n      bios {\n        state\n        folder\n        groups {\n          label\n          importance\n          help\n          anyOne\n          satisfied\n          files {\n            name\n            label\n            state\n            foundAt\n            md5\n            size\n          }\n        }\n      }\n    }\n  }\n": typeof types.BiosDocument,
    "\n  query SystemGames($id: ID!) {\n    system(id: $id) {\n      ...SystemFields\n      games {\n        ...GameFields\n      }\n    }\n  }\n": typeof types.SystemGamesDocument,
    "\n  query Game($id: Int!) {\n    game(id: $id) {\n      ...GameFields\n      saves {\n        ...SaveFields\n      }\n      decoration {\n        url\n        width\n        height\n        top\n        left\n        bottom\n        right\n      }\n    }\n  }\n": typeof types.GameDocument,
    "\n  query Saves($id: Int!) {\n    game(id: $id) {\n      id\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n": typeof types.SavesDocument,
    "\n  query Cheats($id: Int!) {\n    game(id: $id) {\n      id\n      cheats {\n        id\n        description\n        code\n        enabled\n      }\n    }\n  }\n": typeof types.CheatsDocument,
    "\n  mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {\n    addCheat(gameId: $gameId, code: $code, description: $description) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": typeof types.AddCheatDocument,
    "\n  mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {\n    setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": typeof types.SetCheatDocument,
    "\n  query ControllerMappings($system: ID) {\n    controllerMappings(system: $system) {\n      controller\n      system\n      name\n      source\n      bindings {\n        input\n        button\n      }\n    }\n  }\n": typeof types.ControllerMappingsDocument,
    "\n  mutation SaveControllerMapping($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerMapping(controller: $controller, system: $system, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": typeof types.SaveControllerMappingDocument,
    "\n  fragment PadLayoutFields on PadLayout {\n    id\n    name\n    width\n    height\n    body {\n      x\n      y\n      w\n      h\n      r\n      tone\n    }\n    keys {\n      key\n      label\n      x\n      y\n      w\n      h\n      r\n    }\n  }\n": typeof types.PadLayoutFieldsFragmentDoc,
    "\n  query SystemControls($id: ID!) {\n    system(id: $id) {\n      id\n      controls {\n        buttons {\n          button\n          label\n        }\n        layout {\n          ...PadLayoutFields\n        }\n      }\n    }\n    controllerLayouts {\n      ...PadLayoutFields\n    }\n  }\n": typeof types.SystemControlsDocument,
    "\n  query ControllerLayouts {\n    controllerLayouts {\n      ...PadLayoutFields\n    }\n  }\n": typeof types.ControllerLayoutsDocument,
    "\n  query ControllerDefaults {\n    controllerDefaults {\n      mappings {\n        controller\n        system\n        name\n        source\n        bindings {\n          input\n          button\n        }\n      }\n      environment\n    }\n  }\n": typeof types.ControllerDefaultsDocument,
    "\n  mutation SaveControllerDefault($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerDefault(controller: $controller, system: $system, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": typeof types.SaveControllerDefaultDocument,
    "\n  mutation ResetControllerDefault($controller: String!, $system: ID) {\n    resetControllerDefault(controller: $controller, system: $system)\n  }\n": typeof types.ResetControllerDefaultDocument,
    "\n  mutation ResetControllerMapping($controller: String!, $system: ID) {\n    resetControllerMapping(controller: $controller, system: $system)\n  }\n": typeof types.ResetControllerMappingDocument,
    "\n  mutation StartSession($gameId: Int!, $startId: String) {\n    startSession(gameId: $gameId, startId: $startId) {\n      ...SessionFields\n    }\n  }\n": typeof types.StartSessionDocument,
    "\n  query StartProgress($startId: String!) {\n    startProgress(startId: $startId) {\n      phase\n      read\n      total\n      elapsedMs\n      phaseMs\n      error\n    }\n  }\n": typeof types.StartProgressDocument,
    "\n  mutation SessionOffer($id: ID!, $sdp: String!) {\n    sessionOffer(id: $id, sdp: $sdp)\n  }\n": typeof types.SessionOfferDocument,
    "\n  mutation StopSession($id: ID!) {\n    stopSession(id: $id)\n  }\n": typeof types.StopSessionDocument,
    "\n  mutation ResetSession($id: ID!) {\n    resetSession(id: $id)\n  }\n": typeof types.ResetSessionDocument,
    "\n  mutation SetDisc($sessionId: ID!, $disc: Int!) {\n    setDisc(sessionId: $sessionId, disc: $disc)\n  }\n": typeof types.SetDiscDocument,
    "\n  mutation SaveState($sessionId: ID!, $slot: Int!) {\n    saveState(sessionId: $sessionId, slot: $slot)\n  }\n": typeof types.SaveStateDocument,
    "\n  mutation LoadState($sessionId: ID!, $slot: Int!) {\n    loadState(sessionId: $sessionId, slot: $slot)\n  }\n": typeof types.LoadStateDocument,
    "\n  mutation ScanLibrary {\n    scanLibrary {\n      found\n      added\n      removed\n    }\n  }\n": typeof types.ScanLibraryDocument,
    "\n  mutation SyncLibrary {\n    syncLibrary\n  }\n": typeof types.SyncLibraryDocument,
    "\n  query AdminStatus {\n    libraryStatus {\n      mode\n      games\n      cacheBytes\n      mirror {\n        running\n        filesTotal\n        filesDone\n        bytesTotal\n        bytesDone\n        removed\n        failed\n        finishedAt\n        error\n      }\n    }\n    metadataStatus {\n      ...MetadataFields\n    }\n  }\n": typeof types.AdminStatusDocument,
    "\n  mutation FetchMetadata($system: ID, $force: Boolean!) {\n    fetchMetadata(system: $system, force: $force) {\n      ...MetadataFields\n    }\n  }\n": typeof types.FetchMetadataDocument,
    "\n  query EmulatorSettings($system: ID!) {\n    emulatorSettings(system: $system) {\n      ...SettingsFields\n    }\n  }\n": typeof types.EmulatorSettingsDocument,
    "\n  mutation SetRunAhead($system: ID!, $frames: Int) {\n    setRunAhead(system: $system, frames: $frames) {\n      ...SettingsFields\n    }\n  }\n": typeof types.SetRunAheadDocument,
    "\n  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {\n    changeEmulatorSettings(system: $system, changes: $changes) {\n      ...SettingsFields\n    }\n  }\n": typeof types.ChangeEmulatorSettingsDocument,
};
const documents: Documents = {
    "\n  fragment GameFields on Game {\n    id\n    system\n    title\n    fileTitle\n    tags\n    path\n    size\n    lastPlayedAt\n    playCount\n    image: mediaUrl(kind: IMAGE)\n    thumb: mediaUrl(kind: THUMB)\n    video: mediaUrl(kind: VIDEO)\n    description\n    releaseDate\n    developer\n    publisher\n    genre\n    players\n    rating\n    favourite\n    release\n    patch\n    discs\n    partKind\n  }\n": types.GameFieldsFragmentDoc,
    "\n  fragment SystemFields on System {\n    id\n    name\n    short\n    manufacturer\n    year\n    color\n    gameCount\n    playable\n    needsGpu\n    rendering\n    biosMissing\n  }\n": types.SystemFieldsFragmentDoc,
    "\n  fragment SaveFields on SaveSlot {\n    slot\n    savedAt\n    screenshotUrl\n  }\n": types.SaveFieldsFragmentDoc,
    "\n  fragment SessionFields on GameSession {\n    id\n    gameId\n    system\n    title\n    core\n    width\n    height\n    fps\n    connected\n    discs\n    disc\n  }\n": types.SessionFieldsFragmentDoc,
    "\n  fragment SettingsFields on EmulatorSettings {\n    system\n    core\n    needsGame\n    environment\n    runAhead {\n      frames\n      max\n      source\n      supported\n    }\n    categories {\n      key\n      description\n      info\n    }\n    settings {\n      key\n      description\n      info\n      category\n      choices\n      labels\n      default\n      value\n      source\n    }\n  }\n": types.SettingsFieldsFragmentDoc,
    "\n  fragment MetadataFields on MetadataStatus {\n    running\n    total\n    done\n    found\n    notFound\n    failed\n    sources\n    notes\n    artwork {\n      running\n      total\n      done\n      downloaded\n      failed\n    }\n  }\n": types.MetadataFieldsFragmentDoc,
    "\n  query Viewer {\n    viewer {\n      setupRequired\n      user {\n        id\n        username\n        role\n        isAdmin\n      }\n    }\n  }\n": types.ViewerDocument,
    "\n  query Users {\n    users {\n      id\n      username\n      role\n    }\n  }\n": types.UsersDocument,
    "\n  mutation Setup($username: String!, $password: String!) {\n    setup(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n": types.SetupDocument,
    "\n  query ClientConfig {\n    clientConfig {\n      iceServers\n    }\n  }\n": types.ClientConfigDocument,
    "\n  query Home {\n    home {\n      featured {\n        ...GameFields\n      }\n      recent {\n        ...GameFields\n        autoSave {\n          ...SaveFields\n        }\n      }\n      added {\n        ...GameFields\n      }\n      favourites {\n        ...GameFields\n      }\n    }\n  }\n": types.HomeDocument,
    "\n  query Patches($id: Int!) {\n    game(id: $id) {\n      id\n      patches {\n        id\n        name\n        isDefault\n      }\n    }\n  }\n": types.PatchesDocument,
    "\n  mutation SetPatch($gameId: Int!, $patchId: Int) {\n    setPatch(gameId: $gameId, patchId: $patchId)\n  }\n": types.SetPatchDocument,
    "\n  query Search($text: String!) {\n    search(text: $text) {\n      ...GameFields\n    }\n  }\n": types.SearchDocument,
    "\n  query Collections {\n    collections {\n      id\n      name\n      gameCount\n      games {\n        ...GameFields\n      }\n    }\n  }\n": types.CollectionsDocument,
    "\n  query CollectionIds($id: Int!) {\n    game(id: $id) {\n      id\n      collectionIds\n    }\n  }\n": types.CollectionIdsDocument,
    "\n  mutation SetFavourite($gameId: Int!, $favourite: Boolean!) {\n    setFavourite(gameId: $gameId, favourite: $favourite)\n  }\n": types.SetFavouriteDocument,
    "\n  mutation CreateCollection($name: String!) {\n    createCollection(name: $name) {\n      id\n      name\n      gameCount\n    }\n  }\n": types.CreateCollectionDocument,
    "\n  mutation DeleteCollection($id: Int!) {\n    deleteCollection(id: $id)\n  }\n": types.DeleteCollectionDocument,
    "\n  mutation SetInCollection($collectionId: Int!, $gameId: Int!, $member: Boolean!) {\n    setInCollection(collectionId: $collectionId, gameId: $gameId, member: $member)\n  }\n": types.SetInCollectionDocument,
    "\n  query Systems {\n    systems {\n      ...SystemFields\n    }\n  }\n": types.SystemsDocument,
    "\n  query Bios {\n    systems {\n      ...SystemFields\n      bios {\n        state\n        folder\n        groups {\n          label\n          importance\n          help\n          anyOne\n          satisfied\n          files {\n            name\n            label\n            state\n            foundAt\n            md5\n            size\n          }\n        }\n      }\n    }\n  }\n": types.BiosDocument,
    "\n  query SystemGames($id: ID!) {\n    system(id: $id) {\n      ...SystemFields\n      games {\n        ...GameFields\n      }\n    }\n  }\n": types.SystemGamesDocument,
    "\n  query Game($id: Int!) {\n    game(id: $id) {\n      ...GameFields\n      saves {\n        ...SaveFields\n      }\n      decoration {\n        url\n        width\n        height\n        top\n        left\n        bottom\n        right\n      }\n    }\n  }\n": types.GameDocument,
    "\n  query Saves($id: Int!) {\n    game(id: $id) {\n      id\n      saves {\n        ...SaveFields\n      }\n    }\n  }\n": types.SavesDocument,
    "\n  query Cheats($id: Int!) {\n    game(id: $id) {\n      id\n      cheats {\n        id\n        description\n        code\n        enabled\n      }\n    }\n  }\n": types.CheatsDocument,
    "\n  mutation AddCheat($gameId: Int!, $code: String!, $description: String!) {\n    addCheat(gameId: $gameId, code: $code, description: $description) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": types.AddCheatDocument,
    "\n  mutation SetCheat($gameId: Int!, $cheatId: Int!, $enabled: Boolean!) {\n    setCheatEnabled(gameId: $gameId, cheatId: $cheatId, enabled: $enabled) {\n      id\n      description\n      code\n      enabled\n    }\n  }\n": types.SetCheatDocument,
    "\n  query ControllerMappings($system: ID) {\n    controllerMappings(system: $system) {\n      controller\n      system\n      name\n      source\n      bindings {\n        input\n        button\n      }\n    }\n  }\n": types.ControllerMappingsDocument,
    "\n  mutation SaveControllerMapping($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerMapping(controller: $controller, system: $system, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": types.SaveControllerMappingDocument,
    "\n  fragment PadLayoutFields on PadLayout {\n    id\n    name\n    width\n    height\n    body {\n      x\n      y\n      w\n      h\n      r\n      tone\n    }\n    keys {\n      key\n      label\n      x\n      y\n      w\n      h\n      r\n    }\n  }\n": types.PadLayoutFieldsFragmentDoc,
    "\n  query SystemControls($id: ID!) {\n    system(id: $id) {\n      id\n      controls {\n        buttons {\n          button\n          label\n        }\n        layout {\n          ...PadLayoutFields\n        }\n      }\n    }\n    controllerLayouts {\n      ...PadLayoutFields\n    }\n  }\n": types.SystemControlsDocument,
    "\n  query ControllerLayouts {\n    controllerLayouts {\n      ...PadLayoutFields\n    }\n  }\n": types.ControllerLayoutsDocument,
    "\n  query ControllerDefaults {\n    controllerDefaults {\n      mappings {\n        controller\n        system\n        name\n        source\n        bindings {\n          input\n          button\n        }\n      }\n      environment\n    }\n  }\n": types.ControllerDefaultsDocument,
    "\n  mutation SaveControllerDefault($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerDefault(controller: $controller, system: $system, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n": types.SaveControllerDefaultDocument,
    "\n  mutation ResetControllerDefault($controller: String!, $system: ID) {\n    resetControllerDefault(controller: $controller, system: $system)\n  }\n": types.ResetControllerDefaultDocument,
    "\n  mutation ResetControllerMapping($controller: String!, $system: ID) {\n    resetControllerMapping(controller: $controller, system: $system)\n  }\n": types.ResetControllerMappingDocument,
    "\n  mutation StartSession($gameId: Int!, $startId: String) {\n    startSession(gameId: $gameId, startId: $startId) {\n      ...SessionFields\n    }\n  }\n": types.StartSessionDocument,
    "\n  query StartProgress($startId: String!) {\n    startProgress(startId: $startId) {\n      phase\n      read\n      total\n      elapsedMs\n      phaseMs\n      error\n    }\n  }\n": types.StartProgressDocument,
    "\n  mutation SessionOffer($id: ID!, $sdp: String!) {\n    sessionOffer(id: $id, sdp: $sdp)\n  }\n": types.SessionOfferDocument,
    "\n  mutation StopSession($id: ID!) {\n    stopSession(id: $id)\n  }\n": types.StopSessionDocument,
    "\n  mutation ResetSession($id: ID!) {\n    resetSession(id: $id)\n  }\n": types.ResetSessionDocument,
    "\n  mutation SetDisc($sessionId: ID!, $disc: Int!) {\n    setDisc(sessionId: $sessionId, disc: $disc)\n  }\n": types.SetDiscDocument,
    "\n  mutation SaveState($sessionId: ID!, $slot: Int!) {\n    saveState(sessionId: $sessionId, slot: $slot)\n  }\n": types.SaveStateDocument,
    "\n  mutation LoadState($sessionId: ID!, $slot: Int!) {\n    loadState(sessionId: $sessionId, slot: $slot)\n  }\n": types.LoadStateDocument,
    "\n  mutation ScanLibrary {\n    scanLibrary {\n      found\n      added\n      removed\n    }\n  }\n": types.ScanLibraryDocument,
    "\n  mutation SyncLibrary {\n    syncLibrary\n  }\n": types.SyncLibraryDocument,
    "\n  query AdminStatus {\n    libraryStatus {\n      mode\n      games\n      cacheBytes\n      mirror {\n        running\n        filesTotal\n        filesDone\n        bytesTotal\n        bytesDone\n        removed\n        failed\n        finishedAt\n        error\n      }\n    }\n    metadataStatus {\n      ...MetadataFields\n    }\n  }\n": types.AdminStatusDocument,
    "\n  mutation FetchMetadata($system: ID, $force: Boolean!) {\n    fetchMetadata(system: $system, force: $force) {\n      ...MetadataFields\n    }\n  }\n": types.FetchMetadataDocument,
    "\n  query EmulatorSettings($system: ID!) {\n    emulatorSettings(system: $system) {\n      ...SettingsFields\n    }\n  }\n": types.EmulatorSettingsDocument,
    "\n  mutation SetRunAhead($system: ID!, $frames: Int) {\n    setRunAhead(system: $system, frames: $frames) {\n      ...SettingsFields\n    }\n  }\n": types.SetRunAheadDocument,
    "\n  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {\n    changeEmulatorSettings(system: $system, changes: $changes) {\n      ...SettingsFields\n    }\n  }\n": types.ChangeEmulatorSettingsDocument,
};

/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment GameFields on Game {\n    id\n    system\n    title\n    fileTitle\n    tags\n    path\n    size\n    lastPlayedAt\n    playCount\n    image: mediaUrl(kind: IMAGE)\n    thumb: mediaUrl(kind: THUMB)\n    video: mediaUrl(kind: VIDEO)\n    description\n    releaseDate\n    developer\n    publisher\n    genre\n    players\n    rating\n    favourite\n    release\n    patch\n    discs\n    partKind\n  }\n"): typeof import('./graphql').GameFieldsFragmentDoc;
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
export function graphql(source: "\n  fragment SessionFields on GameSession {\n    id\n    gameId\n    system\n    title\n    core\n    width\n    height\n    fps\n    connected\n    discs\n    disc\n  }\n"): typeof import('./graphql').SessionFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment SettingsFields on EmulatorSettings {\n    system\n    core\n    needsGame\n    environment\n    runAhead {\n      frames\n      max\n      source\n      supported\n    }\n    categories {\n      key\n      description\n      info\n    }\n    settings {\n      key\n      description\n      info\n      category\n      choices\n      labels\n      default\n      value\n      source\n    }\n  }\n"): typeof import('./graphql').SettingsFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment MetadataFields on MetadataStatus {\n    running\n    total\n    done\n    found\n    notFound\n    failed\n    sources\n    notes\n    artwork {\n      running\n      total\n      done\n      downloaded\n      failed\n    }\n  }\n"): typeof import('./graphql').MetadataFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Viewer {\n    viewer {\n      setupRequired\n      user {\n        id\n        username\n        role\n        isAdmin\n      }\n    }\n  }\n"): typeof import('./graphql').ViewerDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Users {\n    users {\n      id\n      username\n      role\n    }\n  }\n"): typeof import('./graphql').UsersDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation Setup($username: String!, $password: String!) {\n    setup(username: $username, password: $password) {\n      id\n      username\n      isAdmin\n    }\n  }\n"): typeof import('./graphql').SetupDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query ClientConfig {\n    clientConfig {\n      iceServers\n    }\n  }\n"): typeof import('./graphql').ClientConfigDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Home {\n    home {\n      featured {\n        ...GameFields\n      }\n      recent {\n        ...GameFields\n        autoSave {\n          ...SaveFields\n        }\n      }\n      added {\n        ...GameFields\n      }\n      favourites {\n        ...GameFields\n      }\n    }\n  }\n"): typeof import('./graphql').HomeDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Patches($id: Int!) {\n    game(id: $id) {\n      id\n      patches {\n        id\n        name\n        isDefault\n      }\n    }\n  }\n"): typeof import('./graphql').PatchesDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SetPatch($gameId: Int!, $patchId: Int) {\n    setPatch(gameId: $gameId, patchId: $patchId)\n  }\n"): typeof import('./graphql').SetPatchDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Search($text: String!) {\n    search(text: $text) {\n      ...GameFields\n    }\n  }\n"): typeof import('./graphql').SearchDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query Collections {\n    collections {\n      id\n      name\n      gameCount\n      games {\n        ...GameFields\n      }\n    }\n  }\n"): typeof import('./graphql').CollectionsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query CollectionIds($id: Int!) {\n    game(id: $id) {\n      id\n      collectionIds\n    }\n  }\n"): typeof import('./graphql').CollectionIdsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SetFavourite($gameId: Int!, $favourite: Boolean!) {\n    setFavourite(gameId: $gameId, favourite: $favourite)\n  }\n"): typeof import('./graphql').SetFavouriteDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation CreateCollection($name: String!) {\n    createCollection(name: $name) {\n      id\n      name\n      gameCount\n    }\n  }\n"): typeof import('./graphql').CreateCollectionDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation DeleteCollection($id: Int!) {\n    deleteCollection(id: $id)\n  }\n"): typeof import('./graphql').DeleteCollectionDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SetInCollection($collectionId: Int!, $gameId: Int!, $member: Boolean!) {\n    setInCollection(collectionId: $collectionId, gameId: $gameId, member: $member)\n  }\n"): typeof import('./graphql').SetInCollectionDocument;
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
export function graphql(source: "\n  query Game($id: Int!) {\n    game(id: $id) {\n      ...GameFields\n      saves {\n        ...SaveFields\n      }\n      decoration {\n        url\n        width\n        height\n        top\n        left\n        bottom\n        right\n      }\n    }\n  }\n"): typeof import('./graphql').GameDocument;
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
export function graphql(source: "\n  query ControllerMappings($system: ID) {\n    controllerMappings(system: $system) {\n      controller\n      system\n      name\n      source\n      bindings {\n        input\n        button\n      }\n    }\n  }\n"): typeof import('./graphql').ControllerMappingsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SaveControllerMapping($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerMapping(controller: $controller, system: $system, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n"): typeof import('./graphql').SaveControllerMappingDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  fragment PadLayoutFields on PadLayout {\n    id\n    name\n    width\n    height\n    body {\n      x\n      y\n      w\n      h\n      r\n      tone\n    }\n    keys {\n      key\n      label\n      x\n      y\n      w\n      h\n      r\n    }\n  }\n"): typeof import('./graphql').PadLayoutFieldsFragmentDoc;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query SystemControls($id: ID!) {\n    system(id: $id) {\n      id\n      controls {\n        buttons {\n          button\n          label\n        }\n        layout {\n          ...PadLayoutFields\n        }\n      }\n    }\n    controllerLayouts {\n      ...PadLayoutFields\n    }\n  }\n"): typeof import('./graphql').SystemControlsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query ControllerLayouts {\n    controllerLayouts {\n      ...PadLayoutFields\n    }\n  }\n"): typeof import('./graphql').ControllerLayoutsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  query ControllerDefaults {\n    controllerDefaults {\n      mappings {\n        controller\n        system\n        name\n        source\n        bindings {\n          input\n          button\n        }\n      }\n      environment\n    }\n  }\n"): typeof import('./graphql').ControllerDefaultsDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation SaveControllerDefault($controller: String!, $system: ID, $name: String!, $bindings: [BindingInput!]!) {\n    saveControllerDefault(controller: $controller, system: $system, name: $name, bindings: $bindings) {\n      controller\n    }\n  }\n"): typeof import('./graphql').SaveControllerDefaultDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ResetControllerDefault($controller: String!, $system: ID) {\n    resetControllerDefault(controller: $controller, system: $system)\n  }\n"): typeof import('./graphql').ResetControllerDefaultDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ResetControllerMapping($controller: String!, $system: ID) {\n    resetControllerMapping(controller: $controller, system: $system)\n  }\n"): typeof import('./graphql').ResetControllerMappingDocument;
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
export function graphql(source: "\n  mutation SetDisc($sessionId: ID!, $disc: Int!) {\n    setDisc(sessionId: $sessionId, disc: $disc)\n  }\n"): typeof import('./graphql').SetDiscDocument;
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
export function graphql(source: "\n  mutation SetRunAhead($system: ID!, $frames: Int) {\n    setRunAhead(system: $system, frames: $frames) {\n      ...SettingsFields\n    }\n  }\n"): typeof import('./graphql').SetRunAheadDocument;
/**
 * The graphql function is used to parse GraphQL queries into a document that can be used by GraphQL clients.
 */
export function graphql(source: "\n  mutation ChangeEmulatorSettings($system: ID!, $changes: [SettingChange!]!) {\n    changeEmulatorSettings(system: $system, changes: $changes) {\n      ...SettingsFields\n    }\n  }\n"): typeof import('./graphql').ChangeEmulatorSettingsDocument;


export function graphql(source: string) {
  return (documents as any)[source] ?? {};
}
