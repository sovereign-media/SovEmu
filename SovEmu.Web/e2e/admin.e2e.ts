// The admin's server pages: the menu that reaches them, the background jobs, and the resources.

import { expect, test } from "@playwright/test";

test("the admin menu lists every admin page, flat", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Admin", exact: true }).click();
  for (const name of ["Library", "Users", "Controllers", "Emulator settings", "Resources", "Jobs"]) {
    await expect(page.getByRole("menuitem", { name, exact: true })).toBeVisible();
  }
  // Pages, not group submenus.
  await expect(page.getByRole("menuitem", { name: "Users & Devices" })).toHaveCount(0);
  await page.getByRole("menuitem", { name: "Jobs", exact: true }).click();
  await expect(page).toHaveURL(/\/admin\/jobs\/$/);
});

test("the Jobs page runs a job and shows how it went", async ({ page }) => {
  await page.goto("/admin/jobs/");
  const upkeep = page.getByRole("listitem").filter({ hasText: "Database upkeep" });
  await expect(page.getByRole("listitem").filter({ hasText: "Library scan" })).toContainText("Every 1 h");
  await expect(page.getByRole("listitem").filter({ hasText: "Game information" })).toContainText("After each scan");
  await upkeep.getByRole("button", { name: "Run now" }).click();
  await expect(upkeep).toContainText("Succeeded");
  await expect(upkeep).toContainText("expired sign-ins deleted");
});

test("the Resources page shows the host, its use and its history", async ({ page }) => {
  await page.goto("/admin/resources/");
  await expect(page.getByRole("heading", { name: "Host" })).toBeVisible();
  await expect(page.getByText("Memory", { exact: true }).first()).toBeVisible();
  // In a container the meters; elsewhere (macOS) why there are none.
  await expect(page.getByRole("progressbar", { name: "CPU" }).or(page.getByText(/No cgroup to read/))).toBeVisible();
  await page.getByRole("radio", { name: "Week" }).click();
  await expect(page.getByRole("radio", { name: "Week" })).toHaveAttribute("data-state", "on");
  await expect(page.getByRole("img", { name: /CPU over the window/ })).toBeVisible();
  await expect(page.getByRole("img", { name: /Memory over the window/ })).toBeVisible();
});
