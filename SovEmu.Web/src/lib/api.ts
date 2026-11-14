// Typed calls to the SovEmu API (same origin). A 401 sends the browser to the sign-in page.

export interface User {
  id: number;
  username: string;
  is_admin: boolean;
}

export interface AuthStatus {
  setup_required: boolean;
  user: User | null;
}

export interface SystemSummary {
  id: string;
  name: string;
  short: string;
  manufacturer: string;
  year: number;
  color: string;
  game_count: number;
  playable: boolean;
}

export interface Game {
  id: number;
  system: string;
  title: string;
  tags: string;
  path: string;
  size: number;
  last_played_at: string | null;
  play_count: number | null;
  /** Artwork available at mediaUrl(): "image", "thumb", "marquee", "video". */
  media: string[];
  /** The title from the file name, when metadata supplied `title`. */
  file_title: string | null;
  description: string | null;
  release_date: string | null;
  developer: string | null;
  publisher: string | null;
  genre: string | null;
  players: string | null;
  /** 0 to 1. */
  rating: number | null;
}

export interface SyncStatus {
  running: boolean;
  files_total: number;
  files_done: number;
  bytes_total: number;
  bytes_done: number;
  removed: number;
  failed: number;
  finished_at: number | null;
  error: string | null;
}

export interface LibraryStatus {
  mode: "direct" | "cache" | "mirror";
  games: number;
  cache_bytes: number | null;
  mirror: SyncStatus | null;
}

export interface MetadataStatus {
  running: boolean;
  total: number;
  done: number;
  found: number;
  not_found: number;
  failed: number;
  sources: string[];
  notes: string[];
}

export const mediaUrl = (gameId: number, kind: string) => `/api/games/${gameId}/media/${kind}`;

export interface SessionInfo {
  id: string;
  game_id: number;
  system: string;
  title: string;
  core: string;
  width: number;
  height: number;
  fps: number;
  connected: boolean;
}

export interface SaveSlot {
  slot: number;
  /** Unix seconds. */
  saved_at: number;
  screenshot: boolean;
}

/** Slot 0 is the auto state, written whenever a session ends. */
export const AUTO_SLOT = 0;

export interface Cheat {
  id: number;
  description: string;
  code: string;
  enabled: boolean;
}

export class ApiError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  if (res.status === 401 && !path.startsWith("/auth/")) {
    location.href = `/login/?next=${encodeURIComponent(location.pathname + location.search)}`;
    throw new ApiError(401, "sign in first");
  }
  if (!res.ok) {
    let message = res.statusText;
    try {
      message = (await res.json()).error ?? message;
    } catch { /* not JSON */ }
    throw new ApiError(res.status, message);
  }
  return res.status === 204 ? (undefined as T) : ((await res.json()) as T);
}

export const api = {
  status: () => call<AuthStatus>("GET", "/auth/status"),
  setup: (username: string, password: string) => call<User>("POST", "/auth/setup", { username, password }),
  login: (username: string, password: string) => call<User>("POST", "/auth/login", { username, password }),
  logout: () => call<void>("POST", "/auth/logout"),
  config: () => call<{ ice_servers: RTCIceServer[] }>("GET", "/config"),
  systems: () => call<SystemSummary[]>("GET", "/systems"),
  games: (system: string) => call<Game[]>("GET", `/systems/${encodeURIComponent(system)}/games`),
  game: (id: number) => call<Game>("GET", `/games/${id}`),
  scan: () => call<{ found: number; added: number; removed: number }>("POST", "/library/scan"),
  libraryStatus: () => call<LibraryStatus>("GET", "/library/status"),
  syncLibrary: () => call<void>("POST", "/library/sync"),
  metadataStatus: () => call<MetadataStatus>("GET", "/metadata/status"),
  fetchMetadata: (system?: string, force = false) => call<MetadataStatus>("POST", "/metadata/fetch", { system, force }),
  startSession: (gameId: number) => call<SessionInfo>("POST", "/sessions", { game_id: gameId }),
  currentSession: () => call<SessionInfo | null>("GET", "/sessions/current"),
  stopSession: (id: string) => call<void>("DELETE", `/sessions/${id}`),
  resetSession: (id: string) => call<void>("POST", `/sessions/${id}/reset`),
  states: (gameId: number) => call<SaveSlot[]>("GET", `/games/${gameId}/states`),
  saveState: (sessionId: string, slot: number) => call<void>("POST", `/sessions/${sessionId}/states/${slot}`),
  loadState: (sessionId: string, slot: number) => call<void>("POST", `/sessions/${sessionId}/states/${slot}/load`),
  deleteState: (gameId: number, slot: number) => call<void>("DELETE", `/games/${gameId}/states/${slot}`),
  cheats: (gameId: number) => call<Cheat[]>("GET", `/games/${gameId}/cheats`),
  addCheat: (gameId: number, code: string, description: string) =>
    call<Cheat[]>("POST", `/games/${gameId}/cheats`, { code, description }),
  setCheat: (gameId: number, id: number, enabled: boolean) =>
    call<Cheat[]>("PATCH", `/games/${gameId}/cheats/${id}`, { enabled }),
  deleteCheat: (gameId: number, id: number) => call<void>("DELETE", `/games/${gameId}/cheats/${id}`),
};

/** A save slot's screenshot, cache-busted by when it was saved. */
export const screenshotUrl = (gameId: number, s: SaveSlot) =>
  `/api/games/${gameId}/states/${s.slot}/screenshot?t=${s.saved_at}`;

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
