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
  await expect(page.getByRole("heading", { name: "Library" })).toBeVisible();
  await page.getByRole("button", { name: "Rescan" }).click();
  await expect(page.getByRole("button", { name: "Rescan" })).toBeEnabled();
  await expect(page.getByText("Games").locator("xpath=following-sibling::dd[1]")).toHaveText("4");
});
