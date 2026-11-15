// Sends GraphQL operations to the server (`POST /graphql`, same origin; the sign-in cookie comes
// along). Typed by the documents in queries.ts: the result and variables follow the operation.
import type { TypedDocumentString } from "../graphql/graphql";

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

/** The page to come back to after signing in. */
const here = () => location.pathname + location.search;

/** What an operation's errors mean for the caller: an `ApiError` for the first, or none. */
export function firstError(errors: GraphQLResponse<unknown>["errors"]): ApiError | null {
  const e = errors?.[0];
  return e ? new ApiError(e.extensions?.code ?? "INTERNAL", e.message) : null;
}

/** The JSON body for an operation. */
export const body = (query: TypedDocumentString<unknown, unknown>, variables?: unknown) =>
  JSON.stringify({ query: query.toString(), variables: variables ?? {} });

export async function request<TData, TVariables>(
  query: TypedDocumentString<TData, TVariables>,
  ...[variables]: TVariables extends Record<string, never> ? [] | [TVariables] : [TVariables]
): Promise<TData> {
  const res = await fetch("/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body(query as TypedDocumentString<unknown, unknown>, variables),
    credentials: "same-origin",
  });
  let json: GraphQLResponse<TData>;
  try {
    json = await res.json();
  } catch {
    throw new ApiError("INTERNAL", res.statusText || "the server did not answer");
  }
  const error = firstError(json.errors);
  if (error) {
    // Signed out (or the session expired): sign in, then come back here.
    if (error.code === "UNAUTHENTICATED" && !location.pathname.startsWith("/login")) {
      location.href = `/login/?next=${encodeURIComponent(here())}`;
    }
    throw error;
  }
  return json.data as TData;
}

/** Sends a mutation as the page goes away: `keepalive` lets the request outlive the page. */
export function onUnload<TData, TVariables>(query: TypedDocumentString<TData, TVariables>, variables: TVariables) {
  void fetch("/graphql", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body(query as TypedDocumentString<unknown, unknown>, variables),
    credentials: "same-origin",
    keepalive: true,
  }).catch(() => {});
}
