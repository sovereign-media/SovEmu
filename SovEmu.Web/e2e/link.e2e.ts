// Signing a TV in from the browser: the TV asks the server for a code, as a real one does, and
// someone signed in here approves it at the page the TV names (`/link/`).

import { type APIRequestContext, expect, test } from "@playwright/test";

type Boot = { deviceClientId: string; deviceAuthorizationEndpoint: string; tokenEndpoint: string };

/** What a TV asks for first: where to sign in, from the API. */
async function boot(request: APIRequestContext): Promise<Boot> {
  const query = "{ bootstrapConfig { deviceClientId deviceAuthorizationEndpoint tokenEndpoint } }";
  const response = await request.post("/graphql", { data: { query } });
  return (await response.json()).data.bootstrapConfig;
}

/** A TV's code, and the device code it polls with. */
async function tvCode(request: APIRequestContext, b: Boot) {
  const response = await request.post(b.deviceAuthorizationEndpoint, {
    form: { client_id: b.deviceClientId, scope: "openid profile offline_access" },
  });
  expect(response.ok()).toBe(true);
  return (await response.json()) as { user_code: string; device_code: string; verification_uri: string };
}

test("a TV's code is approved at the page it names, and the TV signs in", async ({ page, request }) => {
  const b = await boot(request);
  const grant = await tvCode(request, b);
  expect(new URL(grant.verification_uri).pathname).toBe("/link");

  await page.goto("/link");
  await expect(page.getByRole("heading", { name: "Sign in a TV" })).toBeVisible();
  await expect(page.getByText("The TV will be signed in as admin.")).toBeVisible();
  // Typed the way a phone keyboard types it: lower case, no dash.
  await page.getByLabel("TV code").fill(grant.user_code.replace("-", "").toLowerCase());
  await expect(page.getByLabel("TV code")).toHaveValue(grant.user_code);
  await page.getByRole("button", { name: "Approve" }).click();
  await expect(page.getByRole("status")).toContainText("Your TV is signing in.");
  await page.screenshot({ path: "test-results/link-approved.png" });

  // The TV's poll now gets its tokens.
  const tokens = await request.post(b.tokenEndpoint, {
    form: {
      grant_type: "urn:ietf:params:oauth:grant-type:device_code",
      client_id: b.deviceClientId,
      device_code: grant.device_code,
    },
  });
  expect(tokens.status(), await tokens.text()).toBe(200);
});

test("a code no TV is waiting with is said so, and the code stays to be fixed", async ({ page }) => {
  await page.goto("/link/");
  await page.getByLabel("TV code").fill("BCDF-GHJK");
  await page.getByRole("button", { name: "Approve" }).click();
  await expect(page.getByRole("alert")).toContainText("No TV is waiting with that code.");
  await expect(page.getByLabel("TV code")).toHaveValue("BCDF-GHJK");
  await page.screenshot({ path: "test-results/link-wrong-code.png" });
});

test("the old /device address opens the page with its code filled in", async ({ page }) => {
  await page.goto("/device?user_code=bcdf-ghjk");
  await expect(page).toHaveURL(/\/link\/\?user_code=bcdf-ghjk$/);
  await expect(page.getByLabel("TV code")).toHaveValue("BCDF-GHJK");
});

test.describe("signed out", () => {
  test.use({ storageState: { cookies: [], origins: [] } });

  test("signs in first, then comes back to the page with the code kept", async ({ page }) => {
    await page.goto("/link/?user_code=BCDF-GHJK");
    await page.getByLabel("Username").fill(process.env.SOVEMU_E2E_USERNAME ?? "admin");
    await page.getByLabel("Password").fill(process.env.SOVEMU_E2E_PASSWORD ?? "e2e-password");
    await page.getByRole("button", { name: "Sign in" }).click();
    await expect(page).toHaveURL(/\/link\/\?user_code=BCDF-GHJK$/);
    await expect(page.getByLabel("TV code")).toHaveValue("BCDF-GHJK");
  });
});
