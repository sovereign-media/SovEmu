// Buttons: one look per role, on Radix's Slot so a link can wear a button's style (`asChild`).
import { Slot } from "radix-ui";
import type { ButtonHTMLAttributes } from "react";
import { cn } from "./cn";

const VARIANTS = {
  primary: "bg-accent text-white hover:brightness-110",
  outline: "border border-edge text-slate-200 hover:bg-white/10",
  ghost: "text-slate-400 hover:bg-white/10 hover:text-white",
  light: "bg-white text-slate-950 hover:bg-white/85",
  glass: "bg-white/15 text-white backdrop-blur hover:bg-white/25",
} as const;

const SIZES = {
  sm: "h-7 gap-1.5 px-2 text-xs",
  md: "h-9 gap-2 px-3 text-sm",
  lg: "h-11 gap-2 px-5 text-base",
  icon: "size-9",
} as const;

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: keyof typeof VARIANTS;
  size?: keyof typeof SIZES;
  asChild?: boolean;
}

export function Button({ variant = "primary", size = "md", asChild, className, ...props }: ButtonProps) {
  const Component = asChild ? Slot.Root : "button";
  return (
    <Component
      className={cn(
        "inline-flex shrink-0 cursor-pointer items-center justify-center rounded-md font-semibold whitespace-nowrap transition outline-none focus-visible:ring-2 focus-visible:ring-white/70 disabled:pointer-events-none disabled:opacity-40 [&_svg]:size-[1.1em] [&_svg]:shrink-0",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...props}
    />
  );
}
