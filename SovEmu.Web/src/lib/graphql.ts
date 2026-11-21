// Sends GraphQL operations to the server (`POST /graphql`, same origin) with the signed-in user's
// access token. Typed by the documents in queries.ts: the result and variables follow the
// operation.
import type { TypedDocumentString } from "../graphql/graphql";
import * as auth from "./auth";

/** How requests are signed in: the browser's sign-in (`auth.ts`), replaced in tests. */
export interface Session {
  /** The access token, or null when signed out. */
  token(): Promise<string | null>;
  /** A fresh access token after the server refused one, or null when the sign-in has ended. */
  renew(): Promise<string | null>;
  /** The token as last seen, without waiting (for requests sent as the page goes away). */
  cached(): string | null;
  /** Sends the browser to sign in, then back here. */
  signIn(): void;
}

let session: Session = {
  token: auth.accessToken,
  renew: auth.renew,
  cached: auth.cachedAccessToken,
  signIn: () => void auth.signIn(),
};

export function useSession(s: Session) {
  session = s;
}

/** A failed operation: the server's `extensions.code` (`UNAUTHENTICATED`, `NOT_FOUND`,
 *  `BAD_REQUEST` …) and its message, which is written for people. */
export class ApiError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
  }
}

interface GraphQLResponse<T> {
  data?: T | null;
  errors?: { message: string; extensions?: { code?: string } }[];
}

/** What an operation's errors mean for the caller: an `ApiError` for the first, or none. */
export function firstError(errors: GraphQLResponse<unknown>["errors"]): ApiError | null {
  const e = errors?.[0];
  return e ? new ApiError(e.extensions?.code ?? "INTERNAL", e.message) : null;
}

/** The JSON body for an operation. */
export const body = (query: TypedDocumentString<unknown, unknown>, variables?: unknown) =>
  JSON.stringify({ query: query.toString(), variables: variables ?? {} });

const headers = (token: string | null): Record<string, string> =>
  token ? { "Content-Type": "application/json", Authorization: `Bearer ${token}` } : { "Content-Type": "application/json" };

async function send<TData>(json: string, token: string | null): Promise<GraphQLResponse<TData>> {
  // Same-origin credentials so the browser keeps the cookie `setup` sets (the browser's sign-in
  // at the server, which /oauth/authorize reads). The API itself ignores cookies.
  const res = await fetch("/graphql", { method: "POST", headers: headers(token), body: json, credentials: "same-origin" });
  try {
    return await res.json();
  } catch {
    throw new ApiError("INTERNAL", res.statusText || "the server did not answer");
  }
}

/** Pages where being signed out is expected: they never send the browser to sign in. */
const signInPage = () => location.pathname.startsWith("/login") || location.pathname.startsWith("/auth/");

export async function request<TData, TVariables>(
  query: TypedDocumentString<TData, TVariables>,
  ...[variables]: TVariables extends Record<string, never> ? [] | [TVariables] : [TVariables]
): Promise<TData> {
  const json = body(query as TypedDocumentString<unknown, unknown>, variables);
  const token = await session.token();
  let response = await send<TData>(json, token);
  let error = firstError(response.errors);
  // The token was refused (expired, or the sign-in ended): renew it once and try again.
  if (error?.code === "UNAUTHENTICATED" && token) {
    const fresh = await session.renew();
    if (fresh) {
      response = await send<TData>(json, fresh);
      error = firstError(response.errors);
    }
  }
  if (error) {
    // Signed out: sign in, then come back here.
    if (error.code === "UNAUTHENTICATED" && !signInPage()) session.signIn();
    throw error;
  }
  return response.data as TData;
}

/**
 * Sends a mutation only if signed in already, without renewing the token or going to sign in,
 * and ignores the answer: for reports that must never get in the player's way.
 */
export function quietly<TData, TVariables>(query: TypedDocumentString<TData, TVariables>, variables: TVariables) {
  const token = session.cached();
  if (!token) return;
  void fetch("/graphql", {
    method: "POST",
    headers: headers(token),
    body: body(query as TypedDocumentString<unknown, unknown>, variables),
    keepalive: true,
  }).catch(() => {});
}

/** Sends a mutation as the page goes away: `keepalive` lets the request outlive the page. */
export function onUnload<TData, TVariables>(query: TypedDocumentString<TData, TVariables>, variables: TVariables) {
  void fetch("/graphql", {
    method: "POST",
    headers: headers(session.cached()),
    body: body(query as TypedDocumentString<unknown, unknown>, variables),
    keepalive: true,
  }).catch(() => {});
}
