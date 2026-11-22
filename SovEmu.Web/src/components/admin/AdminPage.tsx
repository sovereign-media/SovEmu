// The frame every admin page shares: the shell and side navigation, a heading and document title
// from the page's entry in the admin menu (`adminNav.ts`), and a non-admin sent home.

import { useQuery } from "@tanstack/react-query";
import { type ReactNode, useEffect, useId } from "react";
import { adminDocumentTitle, findAdminNavItem } from "../../lib/adminNav";
import { q } from "../../lib/data";
import { go } from "../../lib/go";
import Shell from "../Shell";
import SideNav, { NAV_SPACE } from "../SideNav";
import { cn } from "../ui/cn";
import { Pill, panel } from "../ui/primitives";

export function ErrorNote({ error }: { error: Error | null }) {
  if (!error) return null;
  return (
    <p role="alert" className="rounded-md bg-red-500/15 px-3 py-2 text-sm text-red-300">
      {error.message}
    </p>
  );
}

/** A setting as the line that sets it, for an admin to copy into the environment. */
export function EnvLine({ children }: { children: ReactNode }) {
  return (
    <code className="rounded bg-black/40 px-1.5 py-0.5 font-mono text-xs break-all text-slate-200">{children}</code>
  );
}

/** A licence's name, marked when it forbids commercial use. */
export function Licence({ children, nonCommercial = false }: { children: ReactNode; nonCommercial?: boolean }) {
  return <Pill tone={nonCommercial ? "warn" : "neutral"}>{children}</Pill>;
}

/**
 * One panel of an admin page: a region named by its heading, with an optional icon, a line on
 * what it shows, and actions beside the heading.
 */
export function AdminSection({
  title,
  icon,
  description,
  actions,
  className,
  children,
}: {
  title: ReactNode;
  icon?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
  children?: ReactNode;
}) {
  const id = useId();
  return (
    <section className={cn(panel, className)} aria-labelledby={id}>
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h2 id={id} className="flex items-center gap-2 text-lg font-bold [&_svg]:size-5 [&_svg]:text-slate-400">
          {icon}
          {title}
        </h2>
        {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
      </div>
      {description && <p className="mt-1 text-sm text-slate-400">{description}</p>}
      {children}
    </section>
  );
}

export default function AdminPage({
  description,
  width = "max-w-4xl",
  children,
}: {
  /** One line under the heading: what the page is for. */
  description?: ReactNode;
  width?: string;
  children: ReactNode;
}) {
  const viewer = useQuery(q.viewer());
  const item = findAdminNavItem(location.pathname);
  useEffect(() => {
    document.title = `${adminDocumentTitle(location.pathname)} · SovEmu`;
  }, []);
  useEffect(() => {
    if (viewer.data && !viewer.data.user?.isAdmin) go("/", { replace: true });
  }, [viewer.data]);
  const Icon = item?.icon;
  return (
    <Shell>
      <SideNav section="admin" />
      <div className={NAV_SPACE}>
        <main className={cn("mx-auto grid gap-6 p-6 pb-16", width)}>
          <header>
            <h1 className="flex items-center gap-2 text-2xl font-black tracking-tight [&_svg]:size-6 [&_svg]:text-slate-400">
              {Icon && <Icon aria-hidden />} {item?.label ?? "Admin"}
            </h1>
            <p className="mt-1 text-sm text-muted">{description ?? item?.title}</p>
          </header>
          {children}
        </main>
      </div>
    </Shell>
  );
}
