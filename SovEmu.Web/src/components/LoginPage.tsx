// Sign in, or on first run create the admin account.
import { CircleAlert, Gamepad2, LogIn, UserPlus } from "lucide-react";
import { useEffect, useState, type SubmitEvent } from "react";
import { api, ApiError } from "../lib/api";
import { Button } from "./ui/button";
import { Field, inputClass } from "./ui/primitives";

export default function LoginPage() {
  const next = new URLSearchParams(location.search).get("next") ?? "/";
  const safeNext = next.startsWith("/") && !next.startsWith("//") ? next : "/";
  const [setup, setSetup] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.viewer().then((s) => {
      if (s.user) location.replace(safeNext);
      setSetup(s.setupRequired);
    });
  }, []);

  const submit = async (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    const data = new FormData(e.currentTarget);
    const username = String(data.get("username"));
    const password = String(data.get("password"));
    setBusy(true);
    setError(null);
    try {
      await (setup ? api.setup(username, password) : api.login(username, password));
      location.replace(safeNext);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : "Something went wrong.");
      setBusy(false);
    }
  };

  return (
    <main className="grid min-h-dvh place-items-center bg-[radial-gradient(ellipse_at_top,#1e2a4a,transparent_60%)] p-6">
      <form onSubmit={submit} className="grid w-full max-w-sm gap-4 rounded-2xl border border-edge bg-panel/80 p-8 shadow-2xl backdrop-blur">
        <div>
          <h1 className="flex items-center gap-2 text-3xl font-black tracking-tight">
            <Gamepad2 className="size-7 text-accent" aria-hidden />
            <span>
              Sov<span className="text-accent">Emu</span>
            </span>
          </h1>
          <p className="mt-1 text-sm text-muted">{setup ? "First run: create the admin account." : "Sign in to play."}</p>
        </div>
        <Field label="Username">
          <input name="username" autoComplete="username" required autoFocus className={inputClass} />
        </Field>
        <Field label="Password">
          <input
            name="password"
            type="password"
            autoComplete={setup ? "new-password" : "current-password"}
            required
            minLength={8}
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
