import { test as setup, expect } from '@playwright/test'
import { readSetupCode } from '../fixtures'

const authFile = 'e2e/.auth/admin.json'

const ADMIN_USER = 'admin'
const ADMIN_PASSWORD = 'correcthorsebattery'

setup('create the board admin once for the whole run', async ({ page, request }) => {
  // webServer's reuseExistingServer means a container left running from an
  // earlier local `playwright test` invocation is reused as-is, admin
  // account and all - so this must tolerate "already set up" rather than
  // assume a blank board every time.
  const { setup_required } = await (await request.get('/api/auth/status')).json()

  await page.goto('/')
  if (setup_required) {
    const code = readSetupCode('kanban-e2e-server')
    await expect(page.getByText('Set up this board')).toBeVisible()
    await page.getByPlaceholder('ABCD-1234').fill(code)
    await page.getByPlaceholder(/first name/).fill('Admin')
    const passwords = page.locator('input[type=password]')
    await passwords.nth(0).fill(ADMIN_PASSWORD)
    await passwords.nth(1).fill(ADMIN_PASSWORD)
    await page.getByRole('button', { name: 'Create administrator' }).click()
  } else {
    await page.getByLabel('Username').fill(ADMIN_USER)
    await page.getByLabel('Password').fill(ADMIN_PASSWORD)
    await page.getByRole('button', { name: 'Sign in' }).click()
  }

  await expect(page.locator('.topbar')).toBeVisible()
  await page.context().storageState({ path: authFile })
})
