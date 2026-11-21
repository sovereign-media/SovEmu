import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { safeReturn } from "./auth";
import { ApiError, request, useSession } from "./graphql";
import { ViewerQuery } from "./queries";

const ok = { data: { viewer: { setupRequired: false, user: null } } };
const refused = { errors: [{ message: "sign in first", extensions: { code: "UNAUTHENTICATED" } }] };

/** A fetch that answers each call with the next body, recording the Authorization it was sent. */
function server(...answers: object[]) {
  const sent: (string | undefined)[] = [];
  const fetch = vi.fn(async (_url: string, init: RequestInit) => {
    sent.push((init.headers as Record<string, string>).Authorization);
    return new Response(JSON.stringify(answers.shift()));
  });
  vi.stubGlobal("fetch", fetch);
  return sent;
}

function session(token: string | null, renewed: string | null) {
  const s = {
    token: vi.fn(async () => token),
    renew: vi.fn(async () => renewed),
    cached: () => token,
    signIn: vi.fn(),
  };
  // biome-ignore lint/correctness/useHookAtTopLevel: installs the session; not a React hook.
  useSession(s);
  return s;
}

beforeEach(() => vi.stubGlobal("location", { pathname: "/system/nes/", search: "" }));
afterEach(() => vi.unstubAllGlobals());

describe("API requests", () => {
  it("carry the access token", async () => {
    const sent = server(ok);
    session("abc", null);
    await request(ViewerQuery);
    expect(sent).toEqual(["Bearer abc"]);
  });

  it("go without one when signed out", async () => {
    const sent = server(ok);
    session(null, null);
    await request(ViewerQuery);
    expect(sent).toEqual([undefined]);
  });

  it("renew a refused token once and try again", async () => {
    const sent = server(refused, ok);
    const s = session("old", "new");
    await expect(request(ViewerQuery)).resolves.toEqual(ok.data);
    expect(sent).toEqual(["Bearer old", "Bearer new"]);
    expect(s.renew).toHaveBeenCalledTimes(1);
    expect(s.signIn).not.toHaveBeenCalled();
  });

  it("send the browser to sign in when the sign-in has ended", async () => {
    const sent = server(refused);
    const s = session("old", null);
    await expect(request(ViewerQuery)).rejects.toEqual(new ApiError("UNAUTHENTICATED", "sign in first"));
    expect(sent).toHaveLength(1);
    expect(s.signIn).toHaveBeenCalledTimes(1);
  });

  it("never loop on the sign-in pages", async () => {
    vi.stubGlobal("location", { pathname: "/login/", search: "" });
    server(refused);
    const s = session(null, null);
    await expect(request(ViewerQuery)).rejects.toBeInstanceOf(ApiError);
    expect(s.signIn).not.toHaveBeenCalled();
  });
});

describe("the page to return to after signing in", () => {
  it("is a path on this site", () => {
    expect(safeReturn("/system/snes/?q=1")).toBe("/system/snes/?q=1");
    expect(safeReturn("//evil.test/")).toBe("/");
    expect(safeReturn("https://evil.test/")).toBe("/");
    expect(safeReturn("/auth/callback/")).toBe("/");
    expect(safeReturn(null)).toBe("/");
  });
});
