// The page a TV's *Sign in with your phone* screen sends people to (the server's
// `verificationUri`): type the code the TV shows, and the TV signs in as you. The same approval
// as the phone apps' *Sign in a TV* (`approveDeviceCode`). It stands outside the console's frame,
// as the sign-in page does: whoever opens it is on a phone or laptop, setting up a TV.
import { useMutation, useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, Gamepad2, Tv } from "lucide-react";
import { type ReactNode, type SubmitEvent, useEffect, useState } from "react";
import { api } from "../lib/api";
import { signIn } from "../lib/auth";
import { q } from "../lib/data";
import { failureMessage, formatCode, isCompleteCode } from "../lib/signInTv";
import { Button } from "./ui/button";
import { Field, inputClass } from "./ui/primitives";

export default function LinkPage() {
  const viewer = useQuery(q.viewer());
  const user = viewer.data?.user;
  const signedOut = viewer.isSuccess && !user;
  // Signing in leaves the page and comes back to it, code and all.
  useEffect(() => {
    if (signedOut) void signIn();
  }, [signedOut]);

  // A link with the code in it (the old `/device?user_code=` address) fills the field.
  const [code, setCode] = useState(() => formatCode(new URLSearchParams(location.search).get("user_code") ?? ""));
  const approve = useMutation({ mutationFn: api.approveDeviceCode });

  if (!user) return null;

  const submit = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    if (isCompleteCode(code)) approve.mutate(code);
  };

  return (
    <Frame>
      {approve.isSuccess ? (
        <Approved />
      ) : (
        <form onSubmit={submit} className="grid gap-4">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold">
              <Tv className="size-6 text-accent" aria-hidden /> Sign in a TV
            </h1>
            <p className="mt-1 text-sm text-muted">Enter the code your TV is showing.</p>
          </div>
          <Field label="TV code">
            <input
              name="code"
              value={code}
              onChange={(e) => {
                setCode(formatCode(e.target.value));
                approve.reset();
              }}
              placeholder="XXXX-XXXX"
              autoComplete="one-time-code"
              autoCapitalize="characters"
              autoCorrect="off"
              spellCheck={false}
              required
              autoFocus
              className={`${inputClass} text-center font-mono text-xl tracking-widest`}
            />
          </Field>
          <p className="text-sm text-muted">
            The TV will be signed in as <span className="font-semibold text-white">{user.username}</span>.
          </p>
          {approve.isError && (
            <p role="alert" className="flex items-center gap-2 rounded-md bg-red-500/15 px-3 py-2 text-sm text-red-300">
              <CircleAlert className="size-4 shrink-0" /> {failureMessage(approve.error)}
            </p>
          )}
          <Button type="submit" size="lg" disabled={!isCompleteCode(code) || approve.isPending}>
            {approve.isPending ? "Approving…" : "Approve"}
          </Button>
          <p className="text-xs text-muted">
            Only enter a code your own TV is showing: approving it signs that TV in as you.
          </p>
        </form>
      )}
    </Frame>
  );
}

type FrameProps = { children: ReactNode };

/** The sign-in page's card, with the name above. */
function Frame({ children }: FrameProps) {
  return (
    <main className="grid min-h-dvh place-items-center bg-[radial-gradient(ellipse_at_top,#1e2a4a,transparent_60%)] p-6">
      <div className="grid w-full max-w-sm gap-6 rounded-2xl border border-edge bg-panel/80 p-8 shadow-2xl backdrop-blur">
        <p className="flex items-center gap-2 text-xl font-black tracking-tight">
          <Gamepad2 className="size-5 text-accent" aria-hidden />
          <span>
            Sov<span className="text-accent">Emu</span>
          </span>
        </p>
        {children}
      </div>
    </main>
  );
}

function Approved() {
  return (
    <div className="grid gap-4" role="status">
      <h1 className="flex items-center gap-2 text-2xl font-bold">
        <CircleCheck className="size-6 text-emerald-400" aria-hidden /> Your TV is signing in.
      </h1>
      <p className="text-sm text-muted">It will be ready in a moment. You can close this page.</p>
      <a href="/" className="cursor-pointer text-sm text-accent hover:underline">
        Go to SovEmu
      </a>
    </div>
  );
}
