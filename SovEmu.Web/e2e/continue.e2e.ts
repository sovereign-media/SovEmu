// Home's "Continue playing": a card opens Home's own panel (where the player left off, Continue,
// Game details) rather than starting the game, and Back comes back to it. The stand-in library
// has never been played, so the test puts one of its games in the row: the home feed's answer
// is the server's own, with that one game given a play and an auto save.

import { expect, type Page, test } from "@playwright/test";

/** A key held for a few frames, as a person presses it: the page polls held keys each frame. */
async function press(page: Page, key: string) {
  await page.keyboard.down(key);
  await page.waitForTimeout(80);
  await page.keyboard.up(key);
}

async function withAPlayedGame(page: Page) {
  await page.route("**/graphql", async (route) => {
    const query = String(route.request().postDataJSON()?.query ?? "");
    if (!query.includes("query Home")) return route.fallback();
    const response = await route.fetch();
    const json = await response.json();
    const game = json.data.home.added.find((g: { title: string }) => g.title === "Stand-in Quest");
    json.data.home.recent = [
      {
        ...game,
        playCount: 3,
        lastPlayedAt: "2026-01-01 12:00:00",
        autoSave: { slot: 0, savedAt: Date.now() / 1000 - 2 * 3600, screenshotUrl: null },
      },
    ];
    await route.fulfill({ response, json });
  });
}

test("a Continue playing card opens Home's panel, and Back comes back to it", async ({ page }) => {
  await withAPlayedGame(page);
  await page.goto("/");
  const row = page.locator("section", { hasText: "Continue playing" });
  await row.getByRole("button", { name: /Stand-in Quest/ }).click();

  const panel = page.getByRole("dialog", { name: "Stand-in Quest" });
  await expect(panel).toBeVisible();
  await expect(panel.getByText("Saved 2 h ago")).toBeVisible();
  await expect(panel.getByText(/Played 3×/)).toBeVisible();
  await expect(panel.getByRole("button", { name: "Continue" })).toBeVisible();
  await expect(page).toHaveURL(/\/\?game=\d+$/);
  await page.screenshot({ path: "test-results/continue-panel.png" });

  // Game details: the game's own panel, on its system's page.
  await panel.getByRole("button", { name: "Game details" }).click();
  await expect(page).toHaveURL(/\/system\/\?id=nes&game=\d+$/);
  await expect(
    page.getByRole("dialog", { name: "Stand-in Quest" }).getByText("Not played yet", { exact: false }),
  ).toBeVisible();

  // Back: Home, with the panel open again; Back again closes it.
  await page.goBack();
  await expect(page.getByRole("dialog", { name: "Stand-in Quest" })).toBeVisible();
  await press(page, "Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page).toHaveURL(/\/$/);
});

test("the pad moves between the panel's buttons and presses one", async ({ page }) => {
  await withAPlayedGame(page);
  await page.goto("/");
  await page
    .locator("section", { hasText: "Continue playing" })
    .getByRole("button", { name: /Stand-in Quest/ })
    .click();
  await expect(page.getByRole("dialog", { name: "Stand-in Quest" })).toBeVisible();
  await press(page, "ArrowRight");
  await press(page, "Enter");
  await expect(page).toHaveURL(/\/system\/\?id=nes&game=\d+$/);
});
