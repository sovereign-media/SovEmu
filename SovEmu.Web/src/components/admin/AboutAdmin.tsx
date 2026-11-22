// About and attributions: which build of the server this is, and what SovEmu is made with and
// draws on: the libretro cores and their licences (from the Cores page's query, so the licences
// live in one place), the game-information services and their terms, and the components the
// image ships.

import { useQuery } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import type { ReactNode } from "react";
import { buildText, shortCommit } from "../../lib/about";
import { q } from "../../lib/data";
import AdminPage, { AdminSection, ErrorNote, Licence } from "./AdminPage";

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

function About() {
  const about = useQuery(q.about());
  const cores = useQuery(q.cores());
  const a = about.data;
  return (
    <>
      <ErrorNote error={about.error ?? cores.error} />
      {a && (
        <>
          <AdminSection title={<>SovEmu {buildText(a.buildInfo)}</>}>
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
          </AdminSection>

          <AdminSection title="Emulators">
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
          </AdminSection>

          <AdminSection title="Game information">
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
          </AdminSection>

          <AdminSection title="Also in the image">
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
          </AdminSection>
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
