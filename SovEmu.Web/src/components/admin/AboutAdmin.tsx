// About and attributions: which build of the server this is, and what SovEmu is made with and
// draws on: the libretro cores and their licences (from the Cores page's query, so the licences
// live in one place), the game-information services and their terms, and the components the
// image ships.

import { useQuery } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import type { ReactNode } from "react";
import { buildText, shortCommit } from "../../lib/about";
import { q } from "../../lib/data";
import { cn } from "../ui/cn";
import { panel } from "../ui/primitives";
import AdminPage, { ErrorNote } from "./AdminPage";

function Link({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 hover:underline [&_svg]:size-3.5 [&_svg]:text-slate-500"
    >
      {children} <ExternalLink aria-hidden />
    </a>
  );
}

function Licence({ children, nonCommercial = false }: { children: ReactNode; nonCommercial?: boolean }) {
  return (
    <span
      className={cn(
        "rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap",
        nonCommercial ? "bg-amber-500/15 text-amber-300" : "bg-white/10 text-slate-300",
      )}
    >
      {children}
    </span>
  );
}

function About() {
  const about = useQuery(q.about());
  const cores = useQuery(q.cores());
  const a = about.data;
  return (
    <>
      <ErrorNote error={about.error ?? cores.error} />
      {a && (
        <>
          <section className={panel} aria-labelledby="build-heading">
            <h2 id="build-heading" className="text-lg font-bold">
              SovEmu {buildText(a.buildInfo)}
            </h2>
            <p className="mt-1 text-sm text-slate-400">
              A self-hosted console emulation server, for personal and family use. It is not sold, which several of the
              emulators and services below require. Its cores come from libretro's {a.buildInfo.libretroRelease} stable
              release.
            </p>
            {a.buildInfo.revision && (
              <p className="mt-2 text-xs text-muted">
                Commit <code className="font-mono">{a.buildInfo.revision}</code>
              </p>
            )}
          </section>

          <section className={panel} aria-labelledby="cores-heading">
            <h2 id="cores-heading" className="text-lg font-bold">
              Emulators
            </h2>
            <p className="mt-1 text-sm text-slate-400">
              The libretro cores the image ships, as their own information files state their licences.
            </p>
            <ul className="mt-3 divide-y divide-white/5">
              {(cores.data?.cores ?? [])
                .filter((c) => c.licence)
                .map((c) => (
                  <li key={c.name} className="flex flex-wrap items-center gap-3 py-2.5">
                    <span className="min-w-0 flex-1">
                      {c.sourceUrl ? <Link href={c.sourceUrl}>{c.label ?? c.name}</Link> : (c.label ?? c.name)}
                    </span>
                    <Licence nonCommercial={c.nonCommercial}>{c.licence}</Licence>
                  </li>
                ))}
            </ul>
          </section>

          <section className={panel} aria-labelledby="services-heading">
            <h2 id="services-heading" className="text-lg font-bold">
              Game information
            </h2>
            <ul className="mt-3 divide-y divide-white/5">
              {a.attributions.services.map((s) => (
                <li key={s.name} className="grid gap-1 py-3">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="flex-1 font-semibold">
                      <Link href={s.url}>{s.name}</Link>
                    </span>
                    {s.licence &&
                      (s.licenceUrl ? (
                        <a href={s.licenceUrl} target="_blank" rel="noreferrer">
                          <Licence nonCommercial={/non-?commercial|NC/.test(s.licence)}>{s.licence}</Licence>
                        </a>
                      ) : (
                        <Licence>{s.licence}</Licence>
                      ))}
                  </div>
                  <p className="text-sm text-slate-300">{s.provides}</p>
                  <p className="text-xs text-muted">{s.terms}</p>
                  {s.name === "FinalBurn Neo" && (
                    <p className="text-xs text-muted">
                      DATs at commit <code className="font-mono">{shortCommit(a.attributions.fbneoCommit)}</code>
                    </p>
                  )}
                </li>
              ))}
            </ul>
          </section>

          <section className={panel} aria-labelledby="components-heading">
            <h2 id="components-heading" className="text-lg font-bold">
              Also in the image
            </h2>
            <ul className="mt-3 divide-y divide-white/5">
              {a.attributions.components.map((c) => (
                <li key={c.name} className="flex flex-wrap items-center gap-3 py-2.5">
                  <div className="min-w-0 flex-1">
                    <Link href={c.url}>{c.name}</Link>
                    <p className="text-xs text-muted">
                      {c.what}
                      {c.commit && (
                        <>
                          {" "}
                          · commit <code className="font-mono">{shortCommit(c.commit)}</code>
                        </>
                      )}
                    </p>
                  </div>
                  <Licence>{c.licence}</Licence>
                </li>
              ))}
            </ul>
            <p className="mt-3 text-xs text-muted">
              Debian's packages in the image (glibc, libva and the rest) keep their licences in{" "}
              <code>/usr/share/doc/*/copyright</code>.
            </p>
          </section>
        </>
      )}
    </>
  );
}

export default function AboutAdmin() {
  return (
    <AdminPage width="max-w-3xl">
      <About />
    </AdminPage>
  );
}
