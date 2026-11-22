// The admin's server pages: the menu that reaches them, the active sessions, the background jobs,
// and the resources.

import { expect, test } from "@playwright/test";
import { ADMIN_NAV_GROUPS } from "../src/lib/adminNav";

test("the admin menu lists every admin page, flat", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Admin", exact: true }).click();
  for (const name of ADMIN_NAV_GROUPS.flatMap((g) => g.items.map((i) => i.label))) {
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

test("the Cores page lists the shipped cores with their licences, and finds one", async ({ page }) => {
  await page.goto("/admin/cores/");
  await expect(page.getByRole("heading", { name: "Cores", level: 1 })).toBeVisible();
  await expect(page.getByText(/\d+ cores from libretro \d+\.\d+\.\d+/)).toBeVisible();
  const cores = page.getByRole("region", { name: "Cores" });
  await expect(cores.getByRole("listitem").filter({ hasText: "snes9x" })).toContainText("Non-commercial");
  await page.getByRole("searchbox", { name: "Find a core" }).fill("mpl");
  await expect(cores.getByRole("listitem")).toHaveCount(1);
  await expect(cores.getByRole("listitem")).toContainText("mgba");
});

test("the Configuration page shows every setting and where it came from", async ({ page }) => {
  await page.goto("/admin/configuration/");
  await expect(page.getByRole("heading", { name: "Configuration", level: 1 })).toBeVisible();
  for (const group of ["Accounts", "Library", "Game information", "Streaming", "Operations"]) {
    await expect(page.getByRole("heading", { name: group, exact: true })).toBeVisible();
  }
  // The e2e server is started with an admin from the environment, and its password redacted.
  const password = page.getByRole("listitem").filter({ hasText: "SOVEMU_ADMIN_PASSWORD" });
  await expect(password).toContainText("(set)");
  await page.getByRole("searchbox", { name: "Find a setting" }).fill("udp ports");
  await expect(page.getByRole("listitem").filter({ hasText: "SOVEMU_UDP_PORTS" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Accounts", exact: true })).toHaveCount(0);
});

test("the Network page shows where streams go and how this request arrived", async ({ page }) => {
  await page.goto("/admin/network/");
  await expect(page.getByRole("heading", { name: "Network", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Game streams" })).toBeVisible();
  await expect(page.getByRole("progressbar", { name: "Ports in use" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sign-in addresses" })).toBeVisible();
  // The e2e server is reached at its own address, so signing in works here: no warning about it.
  await expect(page.getByText(/Signing in does not work/)).toHaveCount(0);
  await expect(page.getByRole("listitem").filter({ hasText: "SOVEMU_UDP_PORTS" })).toBeVisible();
});

test("the Storage page shows where the space goes", async ({ page }) => {
  await page.goto("/admin/storage/");
  await expect(page.getByRole("heading", { name: "Storage", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: /free of/ })).toBeVisible();
  await expect(page.getByRole("img", { name: "How the data folder's disk is used" })).toBeVisible();
  await expect(page.getByRole("row", { name: /^Database / })).toBeVisible();
  // Direct mode: an empty cache, so nothing to clear.
  await expect(page.getByRole("button", { name: "Clear the cache" })).toBeDisabled();
  await page.getByRole("button", { name: "Refresh" }).click();
  await expect(page.getByRole("heading", { name: /free of/ })).toBeVisible();
});
