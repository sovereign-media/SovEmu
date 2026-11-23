// The BIOS check, reached from the Systems menu: a table of every file the systems with games
// read, each in its system's folder (`bios/<system>/`, named like its ROM folder), checked on the
// server by name and MD5. Sortable by any column, and searchable by system or file name;
// `?id=psx` starts the search on one system. B goes home.

import { useQuery } from "@tanstack/react-query";
import { Microchip, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import type { BiosSystem } from "../lib/api";
import { summary, withBios } from "../lib/bios";
import { q } from "../lib/data";
import { go } from "../lib/go";
import { listenNav } from "../lib/nav";
import { BiosTable } from "./BiosTable";
import HintBar from "./HintBar";
import Shell from "./Shell";
import SideNav, { NAV_SPACE } from "./SideNav";
import { Button } from "./ui/button";
import { panel } from "./ui/primitives";

function Bios() {
  const bios = useQuery(q.bios());
  const systems: BiosSystem[] | null = bios.data ?? null;
  const checking = bios.isFetching;
  const wanted = useRef(new URLSearchParams(location.search).get("id"));
  const check = useCallback(() => bios.refetch(), [bios.refetch]);

  useEffect(() => {
    const nav = listenNav((action) => {
      if (action === "back") go("/");
    });
    return () => nav.stop();
  }, []);

  if (!systems) return null;
  const shown = withBios(systems);
  return (
    <main className="mx-auto grid max-w-6xl gap-6 px-6 pt-16 pb-24">
      <section className={panel}>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <h1 className="flex items-center gap-2 text-2xl font-bold [&_svg]:size-6 [&_svg]:text-slate-400">
            <Microchip /> BIOS files
          </h1>
          <Button onClick={check} disabled={checking}>
            <RefreshCw className={checking ? "animate-spin" : ""} /> Check again
          </Button>
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-400">
          Each system has its own folder in the server's BIOS folder (<code>SOVEMU_BIOS_DIR</code>, <code>bios/</code>{" "}
          in the data folder by default), named like its folder of games: <code>bios/psx/</code>, <code>bios/ps2/</code>
          , <code>bios/dreamcast/</code>. Files are checked by name, which is case-sensitive, and by MD5 against
          libretro's list of known dumps. Only systems with games are listed; those not listed need no BIOS.
        </p>
        <p className="mt-3 text-sm font-semibold">{summary(shown)}</p>
      </section>
      <BiosTable systems={systems} initialSystem={wanted.current} />
      <HintBar hints={[{ button: "b", label: "Home" }]} />
    </main>
  );
}

export default function BiosPage() {
  return (
    <Shell>
      <SideNav section="systems" />
      <div className={NAV_SPACE}>
        <Bios />
      </div>
    </Shell>
  );
}
