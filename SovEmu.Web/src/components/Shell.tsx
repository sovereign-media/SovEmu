// What every page's island sits in: the background, and Radix's tooltip and toast providers.
import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { reportUncaughtErrors } from "../lib/clientEvents";
import { q } from "../lib/data";
import { TooltipProvider } from "./ui/primitives";
import { Toasts } from "./ui/toast";

// Every page has a shell, so every page reports its uncaught errors.
reportUncaughtErrors();

type ShellProps = { children: ReactNode };

export default function Shell({ children }: ShellProps) {
  // The server's background (the admin's choice), once signed in: the sign-in screen has the
  // surface colour alone, as asking signed out would send it to sign in.
  const signedIn = !!useQuery(q.viewer()).data?.user;
  const background = useQuery({ ...q.background(), enabled: signedIn }).data ?? null;
  return (
    <TooltipProvider>
      {/* Behind every screen; a screen that fills itself (the player) covers it. */}
      {background && (
        <div
          aria-hidden="true"
          data-background={background.id}
          className="pointer-events-none fixed inset-0 -z-10 bg-cover bg-center"
          style={{ backgroundImage: `url("${background.url}")` }}
        />
      )}
      <Toasts>{children}</Toasts>
    </TooltipProvider>
  );
}
