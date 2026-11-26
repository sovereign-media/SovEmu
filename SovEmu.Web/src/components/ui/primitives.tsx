// SovEmu's look over Radix UI primitives: dialogs, selects, progress bars, menus, tooltips,
// scroll areas and labels. Pages compose these; none hand-roll their own.

import { Check, ChevronDown, X } from "lucide-react";
import {
  AlertDialog as AlertPrimitive,
  Dialog as DialogPrimitive,
  Label as LabelPrimitive,
  DropdownMenu as MenuPrimitive,
  Progress as ProgressPrimitive,
  ScrollArea as ScrollPrimitive,
  Select as SelectPrimitive,
  ToggleGroup as TogglePrimitive,
  Tooltip as TooltipPrimitive,
} from "radix-ui";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "./cn";

export const panel = "rounded-2xl border border-white/10 bg-panel/80 p-6";

// --- Dialog ---------------------------------------------------------------------------------

type DialogProps = {
  open: boolean;
  onOpenChange?: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  className?: string;
  /** Darken what is behind (off for the player's splash, which is its own backdrop). */
  dim?: boolean;
  closeButton?: boolean;
  contentProps?: ComponentProps<typeof DialogPrimitive.Content>;
};

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  className,
  dim = true,
  closeButton = true,
  contentProps,
}: DialogProps) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className={cn("fixed inset-0 z-40", dim && "bg-black/70")} />
        <DialogPrimitive.Content
          className={cn(
            "fixed top-1/2 left-1/2 z-50 w-[26rem] max-w-[92vw] -translate-x-1/2 -translate-y-1/2 rounded-2xl border border-white/10 bg-panel/95 p-3 shadow-2xl backdrop-blur outline-none",
            className,
          )}
          {...contentProps}
        >
          <div className="flex items-center gap-2 px-3 pt-1 pb-2">
            <DialogPrimitive.Title className="min-w-0 flex-1 truncate text-sm font-normal text-slate-400">
              {title}
            </DialogPrimitive.Title>
            {closeButton && onOpenChange && (
              <DialogPrimitive.Close
                aria-label="Close"
                className="rounded p-1 text-slate-500 hover:bg-white/10 hover:text-white"
              >
                <X className="size-4" />
              </DialogPrimitive.Close>
            )}
          </div>
          {description ? (
            <DialogPrimitive.Description className="px-3 pb-2 text-sm text-slate-400">
              {description}
            </DialogPrimitive.Description>
          ) : (
            <DialogPrimitive.Description className="sr-only">{title}</DialogPrimitive.Description>
          )}
          {children}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

type ConfirmProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  action: string;
  onAction: () => void;
};

/** A yes/no question (in place of `confirm()`). */
export function Confirm({ open, onOpenChange, title, description, action, onAction }: ConfirmProps) {
  return (
    <AlertPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <AlertPrimitive.Portal>
        <AlertPrimitive.Overlay className="fixed inset-0 z-40 bg-black/70" />
        <AlertPrimitive.Content className="fixed top-1/2 left-1/2 z-50 grid w-[28rem] max-w-[92vw] -translate-x-1/2 -translate-y-1/2 gap-3 rounded-2xl border border-white/10 bg-panel p-6 shadow-2xl">
          <AlertPrimitive.Title className="text-lg font-bold">{title}</AlertPrimitive.Title>
          <AlertPrimitive.Description className="text-sm text-slate-400">{description}</AlertPrimitive.Description>
          <div className="mt-2 flex justify-end gap-2">
            <AlertPrimitive.Cancel className="h-9 rounded-md border border-edge px-3 text-sm hover:bg-white/10">
              Cancel
            </AlertPrimitive.Cancel>
            <AlertPrimitive.Action
              onClick={onAction}
              className="h-9 rounded-md bg-accent px-3 text-sm font-semibold hover:brightness-110"
            >
              {action}
            </AlertPrimitive.Action>
          </div>
        </AlertPrimitive.Content>
      </AlertPrimitive.Portal>
    </AlertPrimitive.Root>
  );
}

// --- Pill -----------------------------------------------------------------------------------

/** What a pill says about its subject, as a colour. */
export type PillTone = "neutral" | "muted" | "accent" | "good" | "warn" | "bad";

const PILL_TONES: Record<PillTone, string> = {
  neutral: "bg-white/10 text-slate-300",
  muted: "bg-white/10 text-slate-400",
  accent: "bg-accent/20 text-accent",
  good: "bg-emerald-500/15 text-emerald-300",
  warn: "bg-amber-500/15 text-amber-300",
  bad: "bg-red-500/20 text-red-300",
};

/** A short label for a state, a role or a licence. */
export function Pill({ tone = "neutral", className, ...props }: ComponentProps<"span"> & { tone?: PillTone }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap [&_svg]:size-3.5",
        PILL_TONES[tone],
        className,
      )}
      {...props}
    />
  );
}

// --- Select ---------------------------------------------------------------------------------

/** Radix refuses an empty item value, and a core option may have one: values are prefixed. */
const enc = (v: string) => `v:${v}`;
const dec = (v: string) => v.slice(2);

type SelectProps = {
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
  disabled?: boolean;
  label: string;
  className?: string;
};

export function Select({ value, options, onChange, disabled, label, className }: SelectProps) {
  return (
    <SelectPrimitive.Root value={enc(value)} onValueChange={(v) => onChange(dec(v))} disabled={disabled}>
      <SelectPrimitive.Trigger
        aria-label={label}
        className={cn(
          "inline-flex h-8 max-w-64 min-w-28 cursor-pointer items-center justify-between gap-2 rounded-md border border-edge bg-black/30 px-2.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent disabled:cursor-default disabled:opacity-60",
          className,
        )}
      >
        <span className="truncate">
          <SelectPrimitive.Value />
        </span>
        <SelectPrimitive.Icon>
          <ChevronDown className="size-4 text-slate-400" />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>
      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          position="popper"
          sideOffset={4}
          className="z-50 max-h-[min(24rem,var(--radix-select-content-available-height))] min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-lg border border-edge bg-panel shadow-2xl"
        >
          <SelectPrimitive.Viewport className="p-1">
            {options.map((o) => (
              <SelectPrimitive.Item
                key={o.value}
                value={enc(o.value)}
                className="relative flex cursor-pointer items-center rounded py-1.5 pr-3 pl-7 text-sm outline-none select-none data-[highlighted]:bg-accent data-[highlighted]:text-white"
              >
                <SelectPrimitive.ItemIndicator className="absolute left-2">
                  <Check className="size-3.5" />
                </SelectPrimitive.ItemIndicator>
                <SelectPrimitive.ItemText>{o.label}</SelectPrimitive.ItemText>
              </SelectPrimitive.Item>
            ))}
          </SelectPrimitive.Viewport>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}

// --- Multi-select ---------------------------------------------------------------------------

type MultiSelectProps<T extends string> = {
  label: string;
  options: { value: T; label: string; count?: number }[];
  selected: T[];
  onChange: (selected: T[]) => void;
  className?: string;
};

/** Pick any number of `options`: a dropdown of checkboxes (Radix's dropdown menu), which stays
 *  open while they are ticked. The trigger says what is picked: "Any", the one value, or how many.
 *  An option's `count` (games with it, say) shows beside it. */
export function MultiSelect<T extends string>({ label, options, selected, onChange, className }: MultiSelectProps<T>) {
  const named = (v: T) => options.find((o) => o.value === v)?.label ?? v;
  const summary =
    selected.length === 0 ? "Any" : selected.length === 1 ? named(selected[0]) : `${selected.length} selected`;
  const toggle = (v: T, on: boolean) => onChange(on ? [...selected, v] : selected.filter((x) => x !== v));
  return (
    <MenuPrimitive.Root modal={false}>
      <MenuPrimitive.Trigger
        aria-label={`${label}: ${summary}`}
        className={cn(
          "inline-flex h-9 w-full min-w-0 cursor-pointer items-center justify-between gap-2 rounded-md border bg-black/30 px-2.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent",
          selected.length ? "border-accent text-white" : "border-edge text-slate-300",
          className,
        )}
      >
        <span className="truncate">{summary}</span>
        <ChevronDown className="size-4 shrink-0 text-slate-400" />
      </MenuPrimitive.Trigger>
      <MenuPrimitive.Portal>
        <MenuPrimitive.Content
          align="start"
          sideOffset={4}
          aria-label={label}
          className="z-50 max-h-[min(24rem,var(--radix-dropdown-menu-content-available-height))] min-w-[var(--radix-dropdown-menu-trigger-width)] overflow-y-auto rounded-lg border border-edge bg-panel p-1 shadow-2xl"
        >
          {options.map((o) => (
            <MenuPrimitive.CheckboxItem
              key={o.value}
              checked={selected.includes(o.value)}
              onCheckedChange={(on) => toggle(o.value, on)}
              // Ticking one keeps the menu open, to tick more.
              onSelect={(e) => e.preventDefault()}
              className="relative flex cursor-pointer items-center gap-3 rounded py-1.5 pr-2 pl-7 text-sm text-slate-200 outline-none select-none data-[highlighted]:bg-white/10"
            >
              <MenuPrimitive.ItemIndicator className="absolute left-2">
                <Check className="size-3.5" />
              </MenuPrimitive.ItemIndicator>
              <span className="truncate">{o.label}</span>
              {o.count !== undefined && <span className="ml-auto text-xs text-slate-500 tabular-nums">{o.count}</span>}
            </MenuPrimitive.CheckboxItem>
          ))}
          {selected.length > 0 && (
            <>
              <MenuPrimitive.Separator className="my-1 h-px bg-white/10" />
              <MenuPrimitive.Item
                onSelect={(e) => {
                  e.preventDefault();
                  onChange([]);
                }}
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-slate-400 outline-none select-none data-[highlighted]:bg-white/10 data-[highlighted]:text-white"
              >
                <X className="size-3.5" /> Clear
              </MenuPrimitive.Item>
            </>
          )}
        </MenuPrimitive.Content>
      </MenuPrimitive.Portal>
    </MenuPrimitive.Root>
  );
}

// --- Progress -------------------------------------------------------------------------------

type ProgressProps = {
  value: number;
  color?: string;
  className?: string;
  label?: string;
};

/** `value` is 0 to 100; `color` overrides the accent (a system's colour). */
export function Progress({ value, color, className, label }: ProgressProps) {
  return (
    <ProgressPrimitive.Root
      value={value}
      aria-label={label}
      className={cn("h-2 overflow-hidden rounded-full bg-black/40", className)}
    >
      <ProgressPrimitive.Indicator
        className="h-full rounded-full bg-accent transition-[width] duration-300 ease-out"
        style={{ width: `${value}%`, background: color }}
      />
    </ProgressPrimitive.Root>
  );
}

// --- Segmented choice -----------------------------------------------------------------------

type SegmentedProps<T extends string> = {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  label: string;
};

/** One of a few options, side by side (a chart's time range). */
export function Segmented<T extends string>({ value, options, onChange, label }: SegmentedProps<T>) {
  return (
    <TogglePrimitive.Root
      type="single"
      value={value}
      aria-label={label}
      onValueChange={(v) => v && onChange(v as T)}
      className="inline-flex rounded-lg border border-edge bg-surface p-0.5"
    >
      {options.map((o) => (
        <TogglePrimitive.Item
          key={o.value}
          value={o.value}
          className="rounded-md px-3 py-1 text-sm text-slate-400 outline-none hover:text-white focus-visible:ring-2 focus-visible:ring-accent data-[state=on]:bg-accent data-[state=on]:text-white"
        >
          {o.label}
        </TogglePrimitive.Item>
      ))}
    </TogglePrimitive.Root>
  );
}

// --- Dropdown menu --------------------------------------------------------------------------

export const Menu = MenuPrimitive.Root;
export const MenuTrigger = MenuPrimitive.Trigger;

type MenuContentProps = { children: ReactNode };

export function MenuContent({ children }: MenuContentProps) {
  return (
    <MenuPrimitive.Portal>
      <MenuPrimitive.Content
        align="end"
        sideOffset={6}
        // Never taller than the room the screen has: a long menu (the admin's) scrolls.
        className="z-50 max-h-[var(--radix-dropdown-menu-content-available-height)] min-w-44 overflow-y-auto rounded-lg border border-edge bg-panel p-1 shadow-2xl"
      >
        {children}
      </MenuPrimitive.Content>
    </MenuPrimitive.Portal>
  );
}

export function MenuItem({ children, ...props }: ComponentProps<typeof MenuPrimitive.Item>) {
  return (
    <MenuPrimitive.Item
      className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-slate-200 outline-none select-none data-[highlighted]:bg-white/10 [&_svg]:size-4 [&_svg]:text-slate-400"
      {...props}
    >
      {children}
    </MenuPrimitive.Item>
  );
}

type MenuLabelProps = { children: ReactNode };

export const MenuLabel = ({ children }: MenuLabelProps) => (
  <MenuPrimitive.Label className="px-2 py-1.5 text-xs text-slate-500">{children}</MenuPrimitive.Label>
);
export const MenuSeparator = () => <MenuPrimitive.Separator className="my-1 h-px bg-white/10" />;

// --- Tooltip --------------------------------------------------------------------------------

export const TooltipProvider = TooltipPrimitive.Provider;

type TooltipProps = { content: ReactNode; children: ReactNode };

export function Tooltip({ content, children }: TooltipProps) {
  return (
    <TooltipPrimitive.Root delayDuration={300}>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          sideOffset={6}
          className="z-50 max-w-72 rounded-md bg-black px-2.5 py-1.5 text-xs text-slate-200 shadow-xl"
        >
          {content}
          <TooltipPrimitive.Arrow className="fill-black" />
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

// --- Scroll area ----------------------------------------------------------------------------

type ScrollAreaProps = {
  children: ReactNode;
  className?: string;
  viewportRef?: React.Ref<HTMLDivElement>;
  orientation?: "vertical" | "horizontal";
};

export function ScrollArea({ children, className, viewportRef, orientation = "vertical" }: ScrollAreaProps) {
  return (
    <ScrollPrimitive.Root type="hover" className={cn("relative overflow-hidden", className)}>
      <ScrollPrimitive.Viewport ref={viewportRef} className="size-full scroll-smooth">
        {children}
      </ScrollPrimitive.Viewport>
      <ScrollPrimitive.Scrollbar
        orientation={orientation}
        className={cn("flex touch-none p-0.5 select-none", orientation === "vertical" ? "w-2" : "h-2 flex-col")}
      >
        <ScrollPrimitive.Thumb className="relative flex-1 rounded-full bg-white/25" />
      </ScrollPrimitive.Scrollbar>
    </ScrollPrimitive.Root>
  );
}

// --- Form fields ----------------------------------------------------------------------------

type FieldProps = { label: string; children: ReactNode };

export function Field({ label, children }: FieldProps) {
  return (
    <LabelPrimitive.Root className="grid gap-1 text-sm">
      {label}
      {children}
    </LabelPrimitive.Root>
  );
}

export const inputClass = "rounded-md border border-edge bg-surface px-3 py-2 outline-none focus:border-accent";
