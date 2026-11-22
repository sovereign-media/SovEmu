// Every screen opens from its URL alone, as a bookmark, a reload or a pasted link would: the
// server answers with the page, the screen draws, and nothing throws. The paths are the router's
// own list (`SCREEN_PATHS`), so a new screen is covered without being listed here.

import { expect, type Page, test } from "@playwright/test";
import { SCREEN_PATHS } from "../src/lib/screens";

/** Opens `path` afresh, and returns what the page threw meanwhile. */
async function open(page: Page, path: string): Promise<string[]> {
  const thrown: string[] = [];
  page.on("pageerror", (e) => thrown.push(e.message));
  const response = await page.goto(path);
  expect(response?.status(), path).toBe(200);
  await expect(page.locator("#root"), path).not.toHaveText("");
  // Let the screen's first reads settle, so a failure among them is caught.
  await page.waitForLoadState("networkidle").catch(() => {});
  return thrown;
}

for (const path of SCREEN_PATHS) {
  test(`${path} opens from its URL`, async ({ page }) => {
    expect(await open(page, path)).toEqual([]);
  });

  if (path !== "/") {
    const bare = path.replace(/\/$/, "");
    test(`${bare} (no trailing slash) opens too`, async ({ page }) => {
      expect(await open(page, bare)).toEqual([]);
    });
  }
}

test("the player says so when its game cannot be found", async ({ page }) => {
  for (const path of ["/play/", "/play/?game=99999"]) {
    expect(await open(page, path)).toEqual([]);
    await expect(page.getByText(/^Could not start: /), path).toBeVisible();
  }
});
