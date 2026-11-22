// End-to-end tests (`npm run e2e`): the built web client against a real sovemu-server, in
// Chromium. `e2e/server.mjs` builds and starts the server on a fresh data folder; `auth.setup.ts`
// signs in as the admin once and the tests reuse that sign-in. Tests are `e2e/*.e2e.ts` (Vitest
// owns `*.test.ts`).
//
// SOVEMU_E2E_URL points the tests at a server already running (`npm run dev`, a container)
// instead: nothing is started, and SOVEMU_E2E_USERNAME / SOVEMU_E2E_PASSWORD sign in.

import { defineConfig, devices } from "@playwright/test";

const port = process.env.SOVEMU_E2E_PORT ?? "18080";
const external = process.env.SOVEMU_E2E_URL;
const baseURL = external ?? `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "e2e",
  outputDir: "test-results/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "setup", testMatch: /auth\.setup\.ts/ },
    {
      name: "chromium",
      testMatch: /.*\.e2e\.ts/,
      dependencies: ["setup"],
      use: { ...devices["Desktop Chrome"], storageState: "e2e/.auth/admin.json" },
    },
  ],
  webServer: external
    ? undefined
    : {
        command: "node e2e/server.mjs",
        url: `${baseURL}/health`,
        // The first run compiles the server.
        timeout: 15 * 60 * 1000,
        reuseExistingServer: !process.env.CI,
        stdout: "pipe",
        stderr: "pipe",
      },
});
