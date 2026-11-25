// BIOS, for admins: the BIOS check with each file's good dumps' MD5s (copyable) and each
// system's folder in full on the server, in the same sortable, searchable table as `/bios/`.
// Read-only: files are added on the server (`SovEmu__Paths__Bios`). `?id=psx` starts the search on
// one system. Everyone else has `/bios/`.

import { useQuery } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useRef } from "react";
import { summary, withBios } from "../../lib/bios";
import { q } from "../../lib/data";
import { BiosTable } from "../BiosTable";
import { Button } from "../ui/button";
import { panel } from "../ui/primitives";
import AdminPage, { EnvLine, ErrorNote } from "./AdminPage";

function Bios() {
  // Checking hashes every file: read on demand, never polled.
  const bios = useQuery({ ...q.bios(), staleTime: 0 });
  const dir = useQuery(q.biosDir());
  const wanted = useRef(new URLSearchParams(location.search).get("id"));

  const shown = bios.data ? withBios(bios.data) : [];
  return (
    <>
      <ErrorNote error={bios.error ?? dir.error} />
      <section className={panel}>
        <div className="flex flex-wrap items-center justify-between gap-4">
          <p className="font-semibold">{bios.data ? summary(shown) : "Checking…"}</p>
          <Button onClick={() => bios.refetch()} disabled={bios.isFetching}>
            <RefreshCw className={bios.isFetching ? "animate-spin" : ""} /> Check again
          </Button>
        </div>
        <p className="mt-3 text-sm leading-relaxed text-slate-400">
          Files are found anywhere in the BIOS folder, in any layout (a Batocera <code>bios/</code> folder or a
          read-only share as it is): a known dump by its MD5 under any name, anything else by its name in any letter
          case. A folder per system, named like its folder of games, keeps them tidy.
        </p>
        <p className="mt-3 text-sm">
          BIOS folder: {dir.data ? <EnvLine>{dir.data}</EnvLine> : "…"}{" "}
          <span className="text-xs text-muted">
            (<code>SovEmu__Paths__Bios</code>; <code>bios/</code> in the data folder by default)
          </span>
        </p>
      </section>
      {bios.data && <BiosTable systems={bios.data} initialSystem={wanted.current} admin biosDir={dir.data} />}
    </>
  );
}

export default function BiosAdmin() {
  return (
    <AdminPage>
      <Bios />
    </AdminPage>
  );
}
