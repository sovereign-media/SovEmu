// The screens load against a real server, signed in as the admin.

import { expect, test } from "@playwright/test";

test("home and the Systems menu list the library's systems", async ({ page }) => {
  await page.goto("/");
  for (const name of ["Nintendo Entertainment System", "Super Nintendo", "Game Boy"]) {
    await expect(page.getByText(name, { exact: true })).toBeVisible();
  }
  // A maker with more than one system is a submenu.
  await page.getByRole("button", { name: "Systems" }).first().click();
  await page.getByRole("menuitem", { name: "Nintendo" }).click();
  await expect(page.getByRole("menuitem", { name: /Nintendo Entertainment System/ })).toBeVisible();
});

test("the admin can rescan the library", async ({ page }) => {
  await page.goto("/admin/");
  await expect(page.getByRole("heading", { name: "Library", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Rescan" }).click();
  await expect(page.getByRole("button", { name: "Rescan" })).toBeEnabled();
  await expect(page.getByText("Games").locator("xpath=following-sibling::dd[1]")).toHaveText("4");
});

test("every screen has one of the backgrounds, and a system's card its photo", async ({ page }) => {
  await page.goto("/");
  const background = page.locator("[data-background]");
  await expect(background).toHaveAttribute("data-background", /^(dither|lattice|screens)$/);
  // The image is there to draw, not just named.
  const url = await background.evaluate((el) => getComputedStyle(el).backgroundImage.slice(5, -2));
  expect((await page.request.get(url)).ok()).toBe(true);

  const photo = page.getByRole("button", { name: /Super Nintendo/ }).locator("img");
  await expect(photo).toBeVisible();
  expect(await photo.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(0);
  await page.screenshot({ path: "test-results/home-background.png" });
});
