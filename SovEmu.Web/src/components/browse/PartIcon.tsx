// What a multi-part game's files are (discs, disks, tapes, sides, parts), as an icon, for the
// count badge on its cover (its words are `partsLabel`, in lib/games.ts).
import { CassetteTape, Disc3, Layers, Save } from "lucide-react";
import type { PartKind } from "../../graphql/graphql";

/** A disc, a floppy, a cassette or a stack. */
export function PartIcon({ kind, className }: { kind: PartKind | null | undefined; className?: string }) {
  switch (kind) {
    case "DISK":
    case "SIDE":
      return <Save className={className} />;
    case "TAPE":
      return <CassetteTape className={className} />;
    case "PART":
      return <Layers className={className} />;
    default:
      return <Disc3 className={className} />;
  }
}
