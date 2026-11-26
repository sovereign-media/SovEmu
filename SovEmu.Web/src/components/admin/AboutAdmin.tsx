// About and attributions: which build of the server this is, and what SovEmu is made with and
// draws on: the libretro cores and their licences (from the Cores page's query, so the licences
// live in one place), the game-information services and their terms, and the components the
// image ships; then, opened on request (it is long), every open-source package the server and
// the web client ship, by licence, with each licence's text.

import { useQuery } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { type ReactNode, useState } from "react";
import { buildText, shortCommit } from "../../lib/about";
import type { Notice } from "../../lib/api";
import { q } from "../../lib/data";
import { byLicence, packagesText } from "../../lib/notices";
import { Button } from "../ui/button";
import { Segmented } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote, Licence } from "./AdminPage";

type LinkProps = { href: string; children: ReactNode };

function Link({ href, children }: LinkProps) {
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

const SIDES = [
  { value: "server", label: "Server" },
  { value: "web", label: "Web client" },
] as const;

type NoticeListProps = { notices: Notice[] };

/** The packages one side ships, by licence; each licence's texts open on request. */
function NoticeList({ notices }: NoticeListProps) {
  if (notices.length === 0) {
    return <p className="mt-3 text-sm text-slate-400">None listed: this build was made without the list.</p>;
  }
  return (
    <ul aria-label="Licences" className="mt-3 divide-y divide-white/5">
      {byLicence(notices).map((g) => (
        <li key={g.id} className="py-2.5">
          <details>
            <summary className="flex cursor-pointer flex-wrap items-center gap-3">
              <span className="flex-1">
                {g.packages} {g.packages === 1 ? "package" : "packages"}
              </span>
              <Licence>{g.id}</Licence>
            </summary>
            {g.texts.map((n) => (
              <div key={n.text} className="mt-3">
                <p className="text-xs text-slate-300">{packagesText(n)}</p>
                <pre className="mt-1 max-h-64 overflow-auto rounded-lg bg-black/40 p-3 text-xs whitespace-pre-wrap text-slate-400">
                  {n.text}
                </pre>
              </div>
            ))}
          </details>
        </li>
      ))}
    </ul>
  );
}

function OpenSource() {
  const [open, setOpen] = useState(false);
  const [side, setSide] = useState<(typeof SIDES)[number]["value"]>("server");
  const notices = useQuery({ ...q.thirdPartyNotices(), enabled: open });
  return (
    <AdminSection
      title="Open-source packages"
      description="Every library the server and the web client are built from, with its licence: listed when each is built."
      actions={
        !open && (
          <Button variant="outline" onClick={() => setOpen(true)}>
            Show the packages
          </Button>
        )
      }
    >
      <ErrorNote error={notices.error} />
      {notices.data && (
        <>
          <div className="mt-3">
            <Segmented label="Packages of" value={side} options={[...SIDES]} onChange={setSide} />
          </div>
          <NoticeList notices={notices.data[side]} />
        </>
      )}
    </AdminSection>
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
            <p className="mt-2 flex flex-wrap items-center gap-2 text-sm text-slate-400">
              Free software: <Licence>{a.buildInfo.licence}</Licence>
              <Link href={a.buildInfo.source}>Source code</Link>
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

          <OpenSource />
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
