// A system's sort and filters: the new sorts, the dropdown filters (one choice, or any number of
// values), and the Jump button, which stays put but is disabled outside title order. The stand-in
// library has two NES games and no game information, so the test widens the system's list: 16
// games made from the server's own first one, with ratings, age ratings, modes and series.

import { expect, type Page, test } from "@playwright/test";

const LETTERS = "ABCDEFGHIJKLMNOP";

async function withARatedLibrary(page: Page) {
  await page.route("**/graphql", async (route) => {
    const response = await route.fetch();
    const json = await response.json();
    const games = json.data?.system?.games;
    if (Array.isArray(games) && games.length) {
      const first = games[0];
      json.data.system.games = [...LETTERS].map((letter, i) => ({
        ...first,
        id: 9000 + i,
        title: `${letter} Stand-in`,
        rating: (i % 5) / 5 + 0.1,
        esrb: i % 2 ? "TEEN" : "EVERYONE",
        esrbBadgeUrl: null,
        genre: i % 3 ? "Platform" : "Puzzle, Action",
        modes: i % 4 ? ["Single player"] : ["Single player", "Co-op"],
        series: i < 4 ? ["Stand-in Saga"] : [],
        themes: [],
      }));
    }
    await route.fulfill({ response, json });
  });
}

const titles = (page: Page) => page.getByRole("main").locator("[data-game-card]").allInnerTexts();

test("sorting keeps the Jump button, disabled outside title order, and dropdowns filter the grid", async ({ page }) => {
  await withARatedLibrary(page);
  await page.goto("/system/?id=nes");
  const jump = page.getByRole("button", { name: "Jump" });
  await expect(jump).toBeVisible();
  await expect(jump).not.toHaveAttribute("aria-disabled", "true");

  await page.getByRole("button", { name: /^Sort:/ }).click();
  await page.getByRole("menuitemradio", { name: "Highest rated" }).click();
  await expect(jump).toBeVisible();
  await expect(jump).toHaveAttribute("aria-disabled", "true");
  await page.getByRole("button", { name: /^Sort:/ }).click();
  await page.getByRole("menuitemradio", { name: "Title" }).click();
  await expect(jump).not.toHaveAttribute("aria-disabled", "true");

  await page.getByRole("button", { name: /^Filters/ }).click();
  const drawer = page.getByRole("dialog", { name: "Filters" });
  await drawer.getByRole("button", { name: "Age rating: Any" }).click();
  await page.getByRole("menuitemcheckbox", { name: /Teen \(T\)/ }).click();
  await page.keyboard.press("Escape");
  await expect(drawer.getByRole("button", { name: "Age rating: Teen (T)" })).toBeVisible();

  await drawer.getByRole("button", { name: "Series: Any" }).click();
  await page.getByRole("menuitemcheckbox", { name: /Stand-in Saga/ }).click();
  await page.keyboard.press("Escape");
  await drawer.screenshot({ path: "test-results/browse-filters.png" });
  // The page polls held keys each frame: Back is a key held for a few frames, as a person presses it.
  await page.keyboard.down("Escape");
  await page.waitForTimeout(80);
  await page.keyboard.up("Escape");
  await expect(drawer).toBeHidden();
  // Teen (the odd ones) in the series (the first four): B and D.
  await expect
    .poll(async () => (await titles(page)).map((t) => t.split("\n")[0]))
    .toEqual(["B Stand-in", "D Stand-in"]);
  await expect(page.getByRole("button", { name: "Filters (2)" })).toBeVisible();
});

test("opening and closing a game's panel leaves the grid where it was", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 500 });
  await page.route("**/graphql", async (route) => {
    const response = await route.fetch();
    const json = await response.json();
    const games = json.data?.system?.games;
    if (Array.isArray(games) && games.length) {
      json.data.system.games = Array.from({ length: 40 }, (_, i) => ({
        ...games[0],
        id: 9100 + i,
        title: `Stand-in ${String(i).padStart(2, "0")}`,
      }));
    }
    await route.fulfill({ response, json });
  });
  await page.goto("/system/?id=nes");
  const card = page.locator("[data-game-card]").nth(24);
  await card.scrollIntoViewIfNeeded();
  const at = await page.evaluate(() => scrollY);
  expect(at).toBeGreaterThan(300);
  await card.click();
  await expect(page.getByRole("dialog", { name: "Stand-in 24" })).toBeVisible();
  expect(await page.evaluate(() => scrollY)).toBe(at);
  await page.keyboard.down("Escape");
  await page.waitForTimeout(80);
  await page.keyboard.up("Escape");
  await expect(page.getByRole("dialog", { name: "Stand-in 24" })).toBeHidden();
  expect(await page.evaluate(() => scrollY)).toBe(at);
});
