// Signing in: the web client is the OpenID Connect client `sovemu-web` of the server it is served
// from, using the authorization code flow with PKCE (oidc-client-ts). The password goes only to
// the server's own sign-in form (`POST /oauth/signin`, on the /login/ page); this client is never
// given the password grant. API calls carry the access token as `Authorization: Bearer`.
//
// Tokens are kept in localStorage, so every tab shares one sign-in; the refresh token renews the
// access token before it expires (the server honours a just-used refresh token for a minute, so
// two tabs renewing at once is fine).
import { UserManager, WebStorageStateStore } from "oidc-client-ts";

export const CLIENT_ID = "sovemu-web";

let manager: UserManager | null = null;

function users(): UserManager {
  manager ??= new UserManager({
    authority: location.origin,
    client_id: CLIENT_ID,
    redirect_uri: `${location.origin}/auth/callback/`,
    post_logout_redirect_uri: `${location.origin}/`,
    response_type: "code",
    scope: "openid profile offline_access",
    userStore: new WebStorageStateStore({ store: localStorage }),
    automaticSilentRenew: true,
    loadUserInfo: false,
    monitorSession: false,
  });
  return manager;
}

/** The last access token seen, for requests that cannot wait (sent as the page unloads). */
let latest: string | null = null;

/** The current access token, renewed first if it has expired; null when signed out. */
export async function accessToken(): Promise<string | null> {
  const user = await users().getUser();
  if (user && !user.expired) return (latest = user.access_token);
  if (user?.refresh_token) return renew();
  return (latest = null);
}

export const cachedAccessToken = () => latest;

/** Renews the access token with the refresh token; null when the sign-in has ended. */
export async function renew(): Promise<string | null> {
  try {
    const user = await users().signinSilent();
    return (latest = user?.access_token ?? null);
  } catch {
    await users().removeUser();
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
  await users().signinRedirect({ state: { returnTo: safeReturn(returnTo) } });
}

/** Finishes signing in on the callback page: where to go next. */
export async function finishSignIn(): Promise<string> {
  const user = await users().signinRedirectCallback();
  latest = user.access_token;
  const state = user.state as { returnTo?: string } | undefined;
  return safeReturn(state?.returnTo);
}

/** Signs out: ends this app's sign-in (revokes its tokens) and the browser's at the server. */
export async function signOut(): Promise<void> {
  latest = null;
  try {
    await users().revokeTokens();
  } catch {
    // Offline or already gone: the server forgets it when it expires.
  }
  await users().signoutRedirect();
}
