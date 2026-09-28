import { execSync } from 'node:child_process'
import { test, expect } from '@playwright/test'
import { readSetupCode } from '../fixtures'

// This is the one flow that genuinely needs a board that hasn't been set up
// yet, so unlike every other spec file it can't share the main instance -
// that one is already past setup by the time any test runs (auth.setup.ts
// uses it). A second, throwaway container on a different port stands in.
// The tests share that one instance and its one-time setup code, so they
// must run in order, not in parallel with each other.
const NAME = 'kanban-e2e-first-run'
const PORT = 8611
const BASE = `http://127.0.0.1:${PORT}`

test.describe.configure({ mode: 'serial' })

test.beforeAll(async () => {
  execSync(`docker rm -f ${NAME}`, { stdio: 'ignore' })
  execSync(`docker run -d --name ${NAME} -p 127.0.0.1:${PORT}:8610 kanban-e2e-image`, { stdio: 'ignore' })
  const deadline = Date.now() + 30_000
  for (;;) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) return
    } catch {
      // Not listening yet.
    }
    if (Date.now() > deadline) throw new Error(`${NAME} never became healthy on ${BASE}`)
    await new Promise((r) => setTimeout(r, 250))
  }
})

test.afterAll(() => {
  execSync(`docker rm -f ${NAME}`, { stdio: 'ignore' })
})

test.describe('first-run setup', () => {
  test('security headers are present even on the pre-setup page', async ({ page }) => {
    const res = await page.goto(BASE)
    const headers = res!.headers()
    expect(headers['x-content-type-options']).toBe('nosniff')
    expect(headers['x-frame-options']).toBe('DENY')
    expect(headers['content-security-policy']).toContain("script-src 'self'")
  })

  test('a mismatched confirmation password is rejected without a request', async ({ page }) => {
    await page.goto(BASE)
    await expect(page.getByText('Set up this board')).toBeVisible()
    await page.getByPlaceholder('ABCD-1234').fill('AAAA-AAAA')
    await page.getByPlaceholder(/first name/).fill('Someone')
    const passwords = page.locator('input[type=password]')
    await passwords.nth(0).fill('correcthorsebattery')
    await passwords.nth(1).fill('somethingelse')
    await page.getByRole('button', { name: 'Create administrator' }).click()
    await expect(page.locator('.error')).toContainText(/match/i)
  })

  test('rejects a wrong code, accepts the real one, and signs in as the new admin', async ({ page }) => {
    const code = readSetupCode(NAME)

    await page.goto(BASE)
    await page.getByPlaceholder('ABCD-1234').fill('WRONG-CODE')
    await page.getByPlaceholder(/first name/).fill('Admin')
    const passwords = page.locator('input[type=password]')
    await passwords.nth(0).fill('correcthorsebattery')
    await passwords.nth(1).fill('correcthorsebattery')
    await page.getByRole('button', { name: 'Create administrator' }).click()
    await expect(page.locator('.error')).toContainText(/wrong/i)

    await page.getByPlaceholder('ABCD-1234').fill(code)
    await page.getByRole('button', { name: 'Create administrator' }).click()
    await expect(page.locator('.topbar')).toBeVisible({ timeout: 10_000 })
  })

  test('setup cannot run a second time once an admin exists', async ({ page }) => {
    const res = await page.request.post(`${BASE}/api/auth/setup`, {
      data: { code: 'AAAA-AAAA', username: 'someone-else', password: 'correcthorsebattery' },
    })
    expect(res.status()).toBe(409)
    expect((await res.json()).error.message).toMatch(/already has an administrator/i)
  })
})
