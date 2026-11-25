// Signing in: the web client is the OpenID Connect web client of the server it is served from
// (its id is per install, asked of `bootstrapConfig` once), using the authorization code flow
// with PKCE (oidc-client-ts). The password goes only to
// the server's own sign-in form (`POST /oauth/signin`, on the /login/ page); this client is never
// given the password grant. API calls carry the access token as `Authorization: Bearer`.
//
// Tokens are kept in localStorage, so every tab shares one sign-in; the refresh token renews the
// access token before it expires (the server honours a just-used refresh token for a minute, so
// two tabs renewing at once is fine).
import { UserManager, WebStorageStateStore } from "oidc-client-ts";
import { anonymous } from "./graphql";
import { WebClientQuery } from "./queries";

let manager: Promise<UserManager> | null = null;

/** The sign-in, made once the server has said which client id this is. A failed ask is not
 *  kept, so the next call asks again. */
function users(): Promise<UserManager> {
  manager ??= create().catch((e: unknown) => {
    manager = null;
    throw e;
  });
  return manager;
}

async function create(): Promise<UserManager> {
  const { bootstrapConfig } = await anonymous(WebClientQuery);
  const clientId = bootstrapConfig.webClientId;
  if (!clientId) throw new Error(`signing in is not available at ${location.host}`);
  return new UserManager({
    authority: location.origin,
    client_id: clientId,
    redirect_uri: `${location.origin}/auth/callback/`,
    post_logout_redirect_uri: `${location.origin}/`,
    response_type: "code",
    scope: "openid profile offline_access",
    userStore: new WebStorageStateStore({ store: localStorage }),
    automaticSilentRenew: true,
    loadUserInfo: false,
    monitorSession: false,
  });
}

/** The last access token seen, for requests that cannot wait (sent as the page unloads). */
let latest: string | null = null;

/** The current access token, renewed first if it has expired; null when signed out. */
export async function accessToken(): Promise<string | null> {
  const user = await (await users()).getUser();
  if (user && !user.expired) return (latest = user.access_token);
  if (user?.refresh_token) return renew();
  return (latest = null);
}

export const cachedAccessToken = () => latest;

/** Renews the access token with the refresh token; null when the sign-in has ended. */
export async function renew(): Promise<string | null> {
  try {
    const user = await (await users()).signinSilent();
    return (latest = user?.access_token ?? null);
  } catch {
    await (await users()).removeUser();
    return (latest = null);
  }
}

/** Where a page may send the browser back to after signing in: a path on this site. */
export function safeReturn(path: string | null | undefined): string {
  return path?.startsWith("/") && !path.startsWith("//") && !path.startsWith("/auth/") ? path : "/";
}

/** Starts signing in (through the server's sign-in page if the browser is not signed in there),
 *  coming back to `returnTo`. */
export async function signIn(returnTo = location.pathname + location.search): Promise<void> {
  await (await users()).signinRedirect({ state: { returnTo: safeReturn(returnTo) } });
}

/** Finishes signing in on the callback page: where to go next. */
export async function finishSignIn(): Promise<string> {
  const user = await (await users()).signinRedirectCallback();
  latest = user.access_token;
  const state = user.state as { returnTo?: string } | undefined;
  return safeReturn(state?.returnTo);
}

/** Signs out: ends this app's sign-in (revokes its tokens) and the browser's at the server. */
export async function signOut(): Promise<void> {
  latest = null;
  try {
    await (await users()).revokeTokens();
  } catch {
    // Offline or already gone: the server forgets it when it expires.
  }
  await (await users()).signoutRedirect();
}
