import { defineConfig, devices } from '@playwright/test'

/**
 * UI tests against a real kanban-server: no mocks, the same Docker image
 * `docker compose up` builds. `start-board.mjs` builds and runs that image
 * as this config's webServer, so `npx playwright test` from `web/` is
 * enough on any OS with Docker - no local Rust toolchain needed, and (on
 * Windows) no risk of Smart App Control blocking a freshly-built binary.
 *
 * One board instance is shared across every spec file for speed; each file
 * uses its own uniquely-keyed project (see fixtures.ts) so different files
 * never collide. The one exception is setup.spec.ts, which needs the board
 * *before* first-run setup and so provisions its own throwaway instance.
 *
 * fullyParallel is deliberately off: tests within one file build on that
 * file's own shared project state (created once in beforeAll), so they run
 * in order within a file. Different files still run concurrently across
 * workers, since each owns an independent project.
 */
export default defineConfig({
  testDir: './e2e/tests',
  timeout: 45_000, // a few of these are multi-step account/token setup flows
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  use: {
    ...devices['Desktop Chrome'],
    // The system's Edge install, not Playwright's own bundled Chromium -
    // no browser download needed for this to run.
    channel: 'msedge',
    baseURL: 'http://127.0.0.1:8610',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: 'node e2e/start-board.mjs',
    url: 'http://127.0.0.1:8610/api/health',
    // Locally, a healthy container on :8610 is reused as-is rather than
    // rebuilt - fast for re-running tests, but it means an app-code change
    // (server or web/src) won't take effect until that container is gone.
    // `docker rm -f kanban-e2e-server` before the next run forces a rebuild.
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
    stdout: 'pipe',
  },
  projects: [
    { name: 'auth-setup', testMatch: /auth\.setup\.ts/ },
    // setup.spec.ts tests the pre-setup flow itself, so it provisions its
    // own throwaway board (see its own beforeAll) instead of using the
    // shared, already-set-up one - it must not depend on auth-setup or
    // reuse its storage state.
    { name: 'first-run', testMatch: /setup\.spec\.ts/ },
    {
      name: 'chromium',
      testIgnore: /setup\.spec\.ts/,
      use: { storageState: 'e2e/.auth/admin.json' },
      dependencies: ['auth-setup'],
    },
  ],
})
