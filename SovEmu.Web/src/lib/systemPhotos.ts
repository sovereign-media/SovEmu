// A photo of each system's hardware, for its card: `src/assets/systems/<system id>.webp`, a
// cut-out on a transparent background (their sources are in `src/assets/README.md`). A system
// with no photo gets a card with its short name alone.
const photos = import.meta.glob<string>("../assets/systems/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});

const byId = new Map(
  Object.entries(photos).map(([path, url]) => [path.slice(path.lastIndexOf("/") + 1, -".webp".length), url]),
);

/** The URL of a system's photo, if it has one. */
export function systemPhoto(id: string): string | undefined {
  return byId.get(id);
}
