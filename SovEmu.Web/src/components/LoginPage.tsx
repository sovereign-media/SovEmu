// The server's sign-in page, or on first run the admin account's creation.
//
// Signing in is OpenID Connect: an app asks the server's /oauth/authorize, which sends a browser
// that is not signed in here with `?continue=<that request>`. The form posts the password
// straight to the server (`POST /oauth/signin`, a plain form post, so password managers see
// it), which signs the browser in and continues; a wrong password comes back here with `error`.
// Opened directly, the page starts signing the web client in, which brings it back here.
import { CircleAlert, LogIn, UserPlus } from "lucide-react";
import { type SubmitEvent, useEffect, useState } from "react";
import { ApiError, api } from "../lib/api";
import { signIn } from "../lib/auth";
import { Button } from "./ui/button";
import { Field, inputClass } from "./ui/primitives";

/** Where the form continues to: only the server's authorization endpoint. */
const continueTo = (value: string | null) => (value?.startsWith("/oauth/authorize?") ? value : null);

export default function LoginPage() {
  const params = new URLSearchParams(location.search);
  const next = continueTo(params.get("continue"));
  const [setup, setSetup] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(params.get("error"));

  useEffect(() => {
    api.viewer().then((s) => {
      setSetup(s.setupRequired);
      // Not asked by the server: start signing in, which returns here with `continue`.
      if (!s.setupRequired && !next) void signIn("/");
    });
  }, []);

  const createAdmin = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    setBusy(true);
    setError(null);
    try {
      // Creates the account and signs the browser in at the server; finishing the sign-in
      // then needs no password.
      await api.setup(String(data.get("username")), String(data.get("password")));
      if (next) location.assign(next);
      else await signIn("/");
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Something went wrong.");
      setBusy(false);
    }
  };

  if (setup === null || (!setup && !next)) return null;

  return (
    <main className="grid min-h-dvh place-items-center bg-[radial-gradient(ellipse_at_top,#1e2a4a,transparent_60%)] p-6">
      <form
        method="post"
        action="/oauth/signin"
        onSubmit={setup ? createAdmin : () => setBusy(true)}
        className="grid w-full max-w-sm gap-4 rounded-2xl border border-edge bg-panel/80 p-8 shadow-2xl backdrop-blur"
      >
        <div>
          <h1 className="flex items-center gap-2 text-3xl font-black tracking-tight">
            <img src="/sovemu-mark.svg" alt="" className="h-9 w-auto" />
            <span>
              Sov<span className="text-accent">Emu</span>
            </span>
          </h1>
          <p className="mt-1 text-sm text-muted">
            {setup ? "First run: create the admin account." : "Sign in to play."}
          </p>
        </div>
        {next && <input type="hidden" name="continue" value={next} />}
        <Field label="Username">
          <input
            name="username"
            autoComplete="username"
            required
            autoFocus
            defaultValue={params.get("username") ?? ""}
            className={inputClass}
          />
        </Field>
        <Field label="Password">
          <input
            name="password"
            type="password"
            autoComplete={setup ? "new-password" : "current-password"}
            required
            minLength={setup ? 8 : undefined}
            className={inputClass}
          />
        </Field>
        {error && (
          <p role="alert" className="flex items-center gap-2 rounded-md bg-red-500/15 px-3 py-2 text-sm text-red-300">
            <CircleAlert className="size-4 shrink-0" /> {error}
          </p>
        )}
        <Button type="submit" disabled={busy} size="lg">
          {setup ? <UserPlus /> : <LogIn />}
          {setup ? "Create admin account" : "Sign in"}
        </Button>
      </form>
    </main>
  );
}
