// Signs in as the admin once, through the real sign-in (OpenID Connect against the server), and
// saves the browser's state for every test to start from.

import { expect, test as setup } from "@playwright/test";

const username = process.env.SOVEMU_E2E_USERNAME ?? "admin";
const password = process.env.SOVEMU_E2E_PASSWORD ?? "e2e-password";

setup("sign in as the admin", async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("navigation", { name: "Main navigation" }).first()).toBeVisible();
  await page.context().storageState({ path: "e2e/.auth/admin.json" });
});
