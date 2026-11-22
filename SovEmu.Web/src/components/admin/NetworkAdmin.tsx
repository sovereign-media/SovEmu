// Network: how browsers and apps reach the server and its game streams: the address media goes
// to, the UDP ports and how many are held, the sign-in addresses, how this very request arrived
// (for checking a proxy), and the STUN/TURN servers. Read-only: these are settings, listed at the
// bottom as the Configuration page shows them.

import { useQuery } from "@tanstack/react-query";
import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { q } from "../../lib/data";
import { networkWarnings, portRange } from "../../lib/network";
import { Progress } from "../ui/primitives";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";
import SettingRow from "./SettingRow";

function Facts({ children }: { children: ReactNode }) {
  return <dl className="mt-3 grid grid-cols-[minmax(0,12rem)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">{children}</dl>;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-muted">{label}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
    </>
  );
}

const mono = "font-mono text-xs";

function Network() {
  // Ports come and go with sessions: read again every 10 s.
  const network = useQuery({ ...q.network(), refetchInterval: 10_000 });
  const n = network.data;
  return (
    <>
      <ErrorNote error={network.error} />
      {n && (
        <>
          {networkWarnings(n).map((w) => (
            <div
              key={w.title}
              role="alert"
              className="flex gap-3 rounded-2xl border border-amber-500/30 bg-amber-500/10 p-4"
            >
              <TriangleAlert aria-hidden className="mt-0.5 size-5 shrink-0 text-amber-400" />
              <div>
                <p className="font-semibold text-amber-200">{w.title}</p>
                <p className="mt-1 text-sm text-amber-100/80">{w.detail}</p>
              </div>
            </div>
          ))}

          <AdminSection title="Game streams">
            <Facts>
              <Fact label="Media goes to">
                <code className={mono}>{n.advertisedIp}</code>{" "}
                <span className="text-xs text-muted">
                  {n.advertisedIpDetected ? "(worked out: SOVEMU_ADVERTISE_IP is not set)" : ""}
                </span>
              </Fact>
              <Fact label="UDP ports">
                <code className={mono}>{portRange(n)}</code>, bound on <code className={mono}>{n.udpBind}</code>
              </Fact>
              <Fact label="In use">
                <div className="flex items-center gap-3">
                  <Progress
                    value={n.portCount ? (n.portsInUse / n.portCount) * 100 : 0}
                    label="Ports in use"
                    className="w-40"
                  />
                  <span className="text-sm">
                    {n.portsInUse} of {n.portCount} ({n.sessions} {n.sessions === 1 ? "game" : "games"} playing)
                  </span>
                </div>
              </Fact>
            </Facts>
            <p className="mt-3 text-xs text-muted">
              One port per game being played. Publish the whole range, UDP, from the host or container.
            </p>
          </AdminSection>

          <AdminSection title="Sign-in addresses">
            <ul className="mt-3 grid gap-1">
              {n.publicUrls.map((u) => (
                <li key={u}>
                  <code className={mono}>{u}</code>
                </li>
              ))}
            </ul>
            <p className="mt-3 text-xs text-muted">
              {n.publicUrlsConfigured
                ? "From SOVEMU_PUBLIC_URL."
                : "SOVEMU_PUBLIC_URL is not set, so these are this host's own: set it when people use a hostname or a proxy."}{" "}
              Cookies are {n.secureCookies ? "" : "not "}marked Secure (<code>SOVEMU_SECURE_COOKIES</code>).
            </p>
          </AdminSection>

          <AdminSection title="This request">
            <Facts>
              <Fact label="Address used">
                <code className={mono}>{n.requestHost || "(none)"}</code>
              </Fact>
              <Fact label="Your address">
                <code className={mono}>{n.clientIp ?? "(unknown)"}</code>
              </Fact>
              <Fact label="X-Forwarded-For">
                <code className={mono}>{n.forwardedFor ?? "(none)"}</code>
              </Fact>
              <Fact label="Trusted proxies">
                {n.trustedProxies.length ? (
                  <code className={mono}>{n.trustedProxies.join(", ")}</code>
                ) : (
                  <span className="text-muted">none</span>
                )}
              </Fact>
            </Facts>
            <p className="mt-3 text-xs text-muted">
              Behind a reverse proxy, "Your address" should be your own, not the proxy's: if it is the proxy's, add the
              proxy to <code>SOVEMU_TRUSTED_PROXIES</code>.
            </p>
          </AdminSection>

          <AdminSection title="STUN and TURN">
            {n.iceServers.length === 0 ? (
              <p className="mt-2 text-sm text-muted">
                None: browsers connect to the game ports directly, which works on the same network or with the ports
                forwarded.
              </p>
            ) : (
              <ul className="mt-3 grid gap-1">
                {n.iceServers.map((s) => (
                  <li key={s.urls.join(",")} className="text-sm">
                    <code className={mono}>{s.urls.join(", ")}</code>
                    {s.hasCredential && <span className="ml-2 text-xs text-muted">with credentials</span>}
                  </li>
                ))}
              </ul>
            )}
          </AdminSection>

          <AdminSection title="Settings">
            <ul className="mt-1 divide-y divide-white/5">
              {n.settings.map((s) => (
                <SettingRow key={s.name} setting={s} />
              ))}
            </ul>
          </AdminSection>
        </>
      )}
    </>
  );
}

export default function NetworkAdmin() {
  return (
    <AdminPage>
      <Network />
    </AdminPage>
  );
}
