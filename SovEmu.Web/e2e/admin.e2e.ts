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

test("a game the server will not start is listed under the Sessions page's failures", async ({ page }) => {
  // The e2e server has no cores, so a start is refused before any emulator runs.
  const started = page.waitForResponse(
    (r) => r.url().endsWith("/graphql") && (r.request().postData() ?? "").includes("startSession"),
  );
  await page.goto("/play/?game=1");
  await started;
  await page.goto("/admin/sessions/");
  const failures = page.getByRole("region", { name: "Recent failures" });
  await expect(failures.getByText("Refused to start").first()).toBeVisible();
  await expect(failures.getByText(/^no core installed for /).first()).toBeVisible();
});

test("the Streaming page tests this browser's connection to a game port", async ({ page }) => {
  await page.goto("/admin/streaming/");
  await expect(page.getByRole("heading", { name: "Streaming", level: 1 })).toBeVisible();
  await expect(page.getByText("SOVEMU_VIDEO_KBPS", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Test this browser" }).click();
  // A real connection-test worker on a game port, over UDP on this machine.
  await expect(page.getByText("This browser reaches the game streams.")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByText(/^Direct to the game port/)).toBeVisible();
  await expect(page.getByText(/^\d+ of 10$/)).toBeVisible();
});

test("the Client errors page groups what the apps reported, and opens a group", async ({ page }) => {
  await page.goto("/admin/client-errors/");
  // Reported as an app does: signed in, through the API.
  const reported = await page.evaluate(async () => {
    const key = Object.keys(localStorage).find((k) => k.startsWith("oidc.user:"));
    const token = key ? JSON.parse(localStorage.getItem(key) ?? "{}").access_token : "";
    const response = await fetch("/graphql", {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
      body: JSON.stringify({
        query: 'mutation($e: [ClientEventInput!]!) { reportClientEvents(client: "tv 9.9 (Test TV)", events: $e) }',
        variables: { e: [{ kind: "STREAM_STALLED", message: "no new picture for 5 s" }] },
      }),
    });
    return (await response.json()).data?.reportClientEvents;
  });
  expect(reported).toBe(1);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Client errors", level: 1 })).toBeVisible();
  const row = page.getByRole("row").filter({ hasText: "tv 9.9" });
  await expect(row).toContainText("Picture stalled");
  await expect(row).toContainText("Test TV");
  await row.getByRole("button", { name: /^Show Picture stalled from tv 9\.9/ }).click();
  await expect(page.getByText("no new picture for 5 s").last()).toBeVisible();
  await page.getByRole("radio", { name: "Day" }).click();
  await expect(page.getByRole("radio", { name: "Day" })).toHaveAttribute("data-state", "on");
});

test("the Users page adds an account, changes its role and password, and removes it", async ({ page }) => {
  await page.goto("/admin/users/");
  await page.getByRole("button", { name: "Add a user" }).click();
  await page.getByLabel("Username").fill("e2e-player");
  await page.getByLabel(/^Password/).fill("password123");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  // The account's row, not the toast that says it was added.
  const row = page.getByRole("listitem").filter({ has: page.getByText("e2e-player", { exact: true }) });
  await expect(row).toContainText("Viewer");

  await row.getByRole("button", { name: "Actions for e2e-player" }).click();
  await page.getByRole("menuitem", { name: "Make an admin" }).click();
  await expect(row).toContainText("Admin");

  await row.getByRole("button", { name: "Actions for e2e-player" }).click();
  await page.getByRole("menuitem", { name: "Set a password…" }).click();
  await page.getByLabel(/^New password/).fill("another-password");
  await page.getByRole("button", { name: "Set the password" }).click();
  await expect(page.getByText("e2e-player's password is set").first()).toBeVisible();

  await row.getByRole("button", { name: "Actions for e2e-player" }).click();
  await page.getByRole("menuitem", { name: "Remove…" }).click();
  await page.getByRole("button", { name: "Remove", exact: true }).click();
  await expect(row).toHaveCount(0);

  // The signed-in admin cannot remove themselves.
  const me = page.getByRole("listitem").filter({ hasText: "(you)" });
  await me.getByRole("button", { name: /^Actions for / }).click();
  await expect(page.getByRole("menuitem", { name: "Remove…" })).toHaveAttribute("data-disabled", "");
});

test("the Activity page shows who did what, filters it, and exports it as CSV", async ({ page }) => {
  // An admin change to find: an account added through the Users page.
  await page.goto("/admin/users/");
  await page.getByRole("button", { name: "Add a user" }).click();
  await page.getByLabel("Username").fill("activity-kid");
  await page.getByLabel(/^Password/).fill("password123");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("activity-kid", { exact: true })).toBeVisible();

  await page.goto("/admin/activity/");
  // The e2e admin signed in through the real sign-in form, so that is in the log too.
  await expect(page.getByRole("cell", { name: "Signed in" }).first()).toBeVisible();
  const added = page.getByRole("row").filter({ hasText: "activity-kid" });
  await expect(added).toContainText("Added an account");

  await page.getByRole("radio", { name: "Sign-ins" }).click();
  await expect(page.getByRole("row").filter({ hasText: "activity-kid" })).toHaveCount(0);
  await page.getByRole("radio", { name: "Admin changes" }).click();
  await expect(page.getByRole("row").filter({ hasText: "activity-kid" })).toHaveCount(1);

  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export CSV" }).click();
  const csv = await (await download).createReadStream();
  const text = await new Promise<string>((resolve) => {
    let out = "";
    csv.on("data", (chunk) => {
      out += chunk;
    });
    csv.on("end", () => resolve(out));
  });
  expect(text.split("\r\n")[0]).toBe("time,user,what,subject,detail,address");
  expect(text).toContain("Added an account,activity-kid,viewer");
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

test("the Telemetry page says how to turn export on", async ({ page }) => {
  await page.goto("/admin/telemetry/");
  await expect(page.getByRole("heading", { name: "Telemetry", level: 1 })).toBeVisible();
  // The e2e server exports nothing.
  await expect(page.getByRole("heading", { name: "Nothing is exported" })).toBeVisible();
  await expect(page.getByText("SOVEMU_OTEL_ENDPOINT=http://otel-collector:4318")).toBeVisible();
  await expect(page.getByRole("listitem").filter({ hasText: "SOVEMU_LOG_FORMAT" })).toBeVisible();
});

test("the About page names the build and credits what SovEmu uses", async ({ page }) => {
  await page.goto("/admin/about/");
  await expect(page.getByRole("heading", { name: "About", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { name: /^SovEmu \d+\.\d+\.\d+/ })).toBeVisible();
  await expect(page.getByRole("listitem").filter({ hasText: "Snes9x" })).toContainText("Non-commercial");
  await expect(page.getByRole("listitem").filter({ hasText: "ScreenScraper" })).toContainText("CC BY-NC-SA 4.0");
  await expect(page.getByRole("listitem").filter({ hasText: "Mesa" })).toContainText("MIT");
});

test("the Appearance page chooses the background everyone sees", async ({ page }) => {
  await page.goto("/admin/appearance/");
  await expect(page.getByRole("heading", { name: "Appearance", level: 1 })).toBeVisible();
  const lattice = page.getByRole("button", { name: "Lattice" });
  await lattice.click();
  await expect(lattice).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByText("SOVEMU_BACKGROUND=lattice")).toBeVisible();
  // Every screen draws it, from the server.
  await page.goto("/");
  const background = page.locator("[data-background]");
  await expect(background).toHaveAttribute("data-background", "lattice");
  await expect(background).toHaveAttribute("style", /\/media\/backgrounds\/lattice\?Sov-/);
  // Back to a different one each visit, as before.
  await page.goto("/admin/appearance/");
  await page.getByRole("button", { name: "A different one each visit" }).click();
  await expect(page.getByRole("button", { name: "A different one each visit" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("system cards show the server's photos", async ({ page }) => {
  await page.goto("/");
  const photo = page.locator('img[src*="/media/systems/nes?"]').first();
  await expect(photo).toBeVisible();
  expect(await photo.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBeGreaterThan(0);
});
