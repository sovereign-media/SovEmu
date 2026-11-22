// What every page's island sits in: the background, and Radix's tooltip and toast providers.
import type { ReactNode } from "react";
import { background } from "../lib/backgrounds";
import { reportUncaughtErrors } from "../lib/clientEvents";
import { TooltipProvider } from "./ui/primitives";
import { Toasts } from "./ui/toast";

// Every page has a shell, so every page reports its uncaught errors.
reportUncaughtErrors();

export default function Shell({ children }: { children: ReactNode }) {
  return (
    <TooltipProvider>
      {/* This visit's background, behind every screen; a screen that fills itself (the player) covers it. */}
      <div
        aria-hidden="true"
        data-background={background.id}
        className="pointer-events-none fixed inset-0 -z-10 bg-cover bg-center"
        style={{ backgroundImage: `url("${background.url}")` }}
      />
      <Toasts>{children}</Toasts>
    </TooltipProvider>
  );
}
