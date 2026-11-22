// The admin's server pages: the menu that reaches them, the active sessions, the background jobs,
// and the resources.

import { expect, test } from "@playwright/test";

test("the admin menu lists every admin page, flat", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Admin", exact: true }).click();
  for (const name of [
    "Library",
    "Users",
    "Controllers",
    "Emulator settings",
    "Rendering",
    "Sessions",
    "Resources",
    "Jobs",
  ]) {
    await expect(page.getByRole("menuitem", { name, exact: true })).toBeVisible();
  }
  // Pages, not group submenus.
  await expect(page.getByRole("menuitem", { name: "Users & Devices" })).toHaveCount(0);
  await page.getByRole("menuitem", { name: "Jobs", exact: true }).click();
  await expect(page).toHaveURL(/\/admin\/jobs\/$/);
});

test("the Sessions page says when nothing is being played", async ({ page }) => {
  await page.goto("/admin/sessions/");
  await expect(page.getByRole("heading", { name: "Active sessions" })).toBeVisible();
  await expect(page.getByText("No games are being played.")).toBeVisible();
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

test("the Rendering page says where 3D systems render, and what the probe found", async ({ page }) => {
  await page.goto("/admin/rendering/");
  await expect(page.getByRole("heading", { name: "Rendering", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: /3D systems (render|are off)/ })).toBeVisible();
  await expect(page.getByText(/SOVEMU_GPU=/)).toBeVisible();
  await expect(page.getByRole("heading", { name: "Devices" })).toBeVisible();
});

test("the BIOS page shows each system's files with the MD5s they should have", async ({ page }) => {
  await page.goto("/admin/bios/?id=psx");
  await expect(page.getByRole("heading", { name: "BIOS", level: 1 })).toBeVisible();
  const psx = page.locator("section").filter({ has: page.getByRole("heading", { name: /psx\/$/ }) });
  await expect(psx).toBeVisible();
  await expect(psx.getByRole("button", { name: /^Copy [0-9a-f]{32}$/ }).first()).toBeVisible();
});

test("the Decorations page lists the sets and frames each system", async ({ page }) => {
  await page.goto("/admin/decorations/");
  await expect(page.getByRole("heading", { name: "Decorations", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sets" })).toBeVisible();
  await expect(page.getByText(/SOVEMU_DECORATION=/)).toBeVisible();
  await expect(page.getByRole("heading", { name: "Systems" })).toBeVisible();
  await expect(page.getByRole("listitem").filter({ hasText: "Nintendo Entertainment System" })).toBeVisible();
});

test("the Patches page explains how a patch becomes the default", async ({ page }) => {
  await page.goto("/admin/patches/");
  await expect(page.getByRole("heading", { name: "Patches", level: 1 })).toBeVisible();
  await expect(page.getByText("1 patch for 1 game")).toBeVisible();
  const game = page.getByRole("listitem").filter({ hasText: "Super Stand-in" }).first();
  await expect(game).toContainText("IPS");
  await expect(game).toContainText("Default");
  await expect(game).toContainText("snes/Super Stand-in (USA).ips");
});
