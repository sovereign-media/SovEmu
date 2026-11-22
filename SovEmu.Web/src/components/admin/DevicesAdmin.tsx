// Devices: every browser and app signed in, across accounts, the most recently used first, each
// with a Sign out of its own. An account's own page lists just its own.

import { useQuery } from "@tanstack/react-query";
import { MonitorSmartphone } from "lucide-react";
import { q } from "../../lib/data";
import AdminPage, { AdminSection, ErrorNote } from "./AdminPage";
import SignInsTable from "./SignInsTable";

function Devices() {
  const signIns = useQuery({ ...q.signIns(), refetchInterval: 30_000 });
  return (
    <>
      <ErrorNote error={signIns.error} />
      {signIns.data && (
        <AdminSection
          icon={<MonitorSmartphone />}
          title={`${signIns.data.length} signed in`}
          description="A browser or app stays signed in until it is signed out, or goes unused until it lapses."
        >
          <SignInsTable signIns={signIns.data} showUser />
        </AdminSection>
      )}
    </>
  );
}

export default function DevicesAdmin() {
  return (
    <AdminPage width="max-w-4xl">
      <Devices />
    </AdminPage>
  );
}
