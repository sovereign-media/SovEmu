// Finishes signing in: exchanges the code the server sent back for tokens, then goes on to the
// page the sign-in started from.

import { CircleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { finishSignIn, signIn } from "../lib/auth";
import { go } from "../lib/go";
import { Button } from "./ui/button";

export default function AuthCallback() {
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    finishSignIn()
      .then((next) => go(next, { replace: true }))
      .catch((e: unknown) => setError(e instanceof Error ? e.message : "Signing in failed."));
  }, []);

  if (!error) return null;
  return (
    <main className="grid min-h-dvh place-items-center p-6">
      <div className="grid max-w-sm gap-4 text-center">
        <p role="alert" className="flex items-center gap-2 rounded-md bg-red-500/15 px-3 py-2 text-sm text-red-300">
          <CircleAlert className="size-4 shrink-0" /> {error}
        </p>
        <Button onClick={() => void signIn("/")}>Try again</Button>
      </div>
    </main>
  );
}
