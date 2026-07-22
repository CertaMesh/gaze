import { defineConfig } from "@playwright/test";

/* Dev-only browser contract configuration. One worker: the fixture server
 * holds per-run mutable fixture state and the 44-state ledger is appended
 * sequentially. Screenshots are human evidence and are written OUTSIDE the
 * repository (GAZE_VISUAL_EVIDENCE_DIR or the OS temp dir); they are never
 * committed. */
export default defineConfig({
  testDir: ".",
  testMatch: ["dashboard.spec.mjs"],
  workers: 1,
  fullyParallel: false,
  retries: 0,
  timeout: 120000,
  expect: { timeout: 10000 },
  reporter: [["list"]],
  use: {
    browserName: "chromium",
    screenshot: "off",
    trace: "off",
    video: "off",
  },
});
