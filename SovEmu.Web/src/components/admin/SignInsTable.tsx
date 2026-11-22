// Browsers and apps signed in, with a Sign out for each: every account's on the Devices page,
// one account's on its own page.

import { useMutation } from "@tanstack/react-query";
import { Globe, LogOut, MonitorSmartphone } from "lucide-react";
import { useState } from "react";
import { api, type SignIn } from "../../lib/api";
import { invalidate } from "../../lib/data";
import { CLIENT_LABELS, lapsesText, usedText } from "../../lib/signIns";
import { Button } from "../ui/button";
import { Confirm } from "../ui/primitives";
import { ErrorNote } from "./AdminPage";

export default function SignInsTable({ signIns, showUser }: { signIns: SignIn[]; showUser: boolean }) {
  const [ending, setEnding] = useState<SignIn | null>(null);
  const end = useMutation({
    mutationFn: (id: string) => api.endSignIn(id),
    onSettled: () => invalidate(["users"]),
  });
  if (signIns.length === 0) return <p className="mt-3 text-sm text-slate-400">Nothing is signed in.</p>;
  const who = (s: SignIn | null) => (showUser ? `${s?.username ?? "a deleted account"}'s ` : "this ");
  return (
    <>
      <ErrorNote error={end.error} />
      <ul className="mt-3 divide-y divide-white/5">
        {signIns.map((s) => (
          <li key={s.id} className="flex items-center gap-3 py-3">
            {s.client === "WEB" ? (
              <Globe aria-hidden className="size-5 shrink-0 text-slate-400" />
            ) : (
              <MonitorSmartphone aria-hidden className="size-5 shrink-0 text-slate-400" />
            )}
            <div className="min-w-0 flex-1">
              <div className="font-semibold">
                {CLIENT_LABELS[s.client]}
                {showUser && (
                  <>
                    {" · "}
                    {s.userId != null && s.username ? (
                      <a className="hover:underline" href={`/admin/users/?id=${s.userId}`}>
                        {s.username}
                      </a>
                    ) : (
                      "(deleted account)"
                    )}
                  </>
                )}
              </div>
              <div className="text-xs text-slate-400">
                {usedText(s)} · {lapsesText(s.expiresAt)}
              </div>
            </div>
            <Button
              variant="outline"
              size="sm"
              aria-label={`Sign out ${CLIENT_LABELS[s.client]}${showUser && s.username ? ` of ${s.username}` : ""}`}
              disabled={end.isPending && end.variables === s.id}
              onClick={() => setEnding(s)}
            >
              <LogOut /> Sign out
            </Button>
          </li>
        ))}
      </ul>
      <Confirm
        open={ending != null}
        onOpenChange={(open) => !open && setEnding(null)}
        title={`Sign out ${who(ending)}${ending ? CLIENT_LABELS[ending.client].toLowerCase() : ""}?`}
        description="It has to sign in again. Every other browser and app stays signed in."
        action="Sign out"
        onAction={() => ending && end.mutate(ending.id)}
      />
    </>
  );
}
