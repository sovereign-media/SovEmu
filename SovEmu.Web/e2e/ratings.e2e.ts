// What ScreenScraper says of a game beyond its description: Home's "Top rated" row, and the game
// panel's ESRB badge, stars, play modes, series and themes. The stand-in library has no game
// information (nothing is looked up in these tests), so the test gives one game some: every
// answer from the server is its own, with that game's fields filled in wherever it appears.

import { expect, type Page, test } from "@playwright/test";

const RATED = {
  rating: 0.8,
  esrb: "TEEN",
  modes: ["Single player", "Multiplayer (alternating)"],
  series: ["Stand-in Saga"],
  themes: ["Fantasy"],
};

/** `value` with the game titled `title` given RATED's fields, wherever it is. */
function rate(value: unknown, title: string): unknown {
  if (Array.isArray(value)) return value.map((v) => rate(v, title));
  if (value && typeof value === "object") {
    const entries = Object.entries(value).map(([k, v]) => [k, rate(v, title)]);
    const object = Object.fromEntries(entries) as Record<string, unknown>;
    return object.title === title && "esrb" in object ? { ...object, ...RATED } : object;
  }
  return value;
}

async function withARatedGame(page: Page) {
  await page.route("**/graphql", async (route) => {
    const response = await route.fetch();
    const json = await response.json();
    const query = String(route.request().postDataJSON()?.query ?? "");
    if (query.includes("query Home")) {
      const game = json.data.home.added.find((g: { title: string }) => g.title === "Stand-in Quest");
      json.data.home.topRated = [game];
    }
    await route.fulfill({ response, json: rate(json, "Stand-in Quest") });
  });
}

test("Home's top rated row opens a game whose panel shows its age rating and details", async ({ page }) => {
  await withARatedGame(page);
  await page.goto("/");
  const row = page.locator("section", { hasText: "Top rated" });
  await row.getByRole("button", { name: /Stand-in Quest/ }).click();

  const panel = page.getByRole("dialog", { name: "Stand-in Quest" });
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("img", { name: "ESRB: Teen (T)" })).toBeVisible();
  await expect(panel.getByText("4 out of 5")).toBeVisible();
  await expect(panel.getByText("Single player, Multiplayer (alternating)")).toBeVisible();
  await expect(panel.getByText("Stand-in Saga")).toBeVisible();
  await expect(panel.getByText("Fantasy")).toBeVisible();
  await panel.screenshot({ path: "test-results/ratings-panel.png" });
});
