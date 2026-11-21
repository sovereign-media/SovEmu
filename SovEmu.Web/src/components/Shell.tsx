// What every page's island sits in: Radix's tooltip and toast providers.
import type { ReactNode } from "react";
import { TooltipProvider } from "./ui/primitives";
import { Toasts } from "./ui/toast";
import { reportUncaughtErrors } from "../lib/clientEvents";

// Every page has a shell, so every page reports its uncaught errors.
reportUncaughtErrors();

export default function Shell({ children }: { children: ReactNode }) {
  return (
    <TooltipProvider>
      <Toasts>{children}</Toasts>
    </TooltipProvider>
  );
}
