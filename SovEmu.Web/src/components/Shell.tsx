// What every page's island sits in: Radix's tooltip and toast providers.
import type { ReactNode } from "react";
import { TooltipProvider } from "./ui/primitives";
import { Toasts } from "./ui/toast";

export default function Shell({ children }: { children: ReactNode }) {
  return (
    <TooltipProvider>
      <Toasts>{children}</Toasts>
    </TooltipProvider>
  );
}
