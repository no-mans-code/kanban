import { test, expect, type APIRequestContext, type Page } from '@playwright/test'
import { uniqueKey } from '../fixtures'

/** `[value=...]` only matches the initial HTML attribute, not a Svelte-bound
 * input's live value - so finding "the row for this label" has to read
 * actual current values back instead. Null when no row currently matches. */
async function findLabelRow(page: Page, name: string) {
  const rows = page.locator('.row', { has: page.locator('input.input.grow') })
  const count = await rows.count()
  for (let i = 0; i < count; i++) {
    const row = rows.nth(i)
    if ((await row.locator('input.input.grow').inputValue()) === name) return row
  }
  return null
}

async function labelRow(page: Page, name: string) {
  const row = await findLabelRow(page, name)
  if (!row) throw new Error(`No label row found with the name "${name}"`)
  return row
}

// Labels have no project_id - they're shared board-wide (see
// server/migrations/0001_init.sql) - so create() is deliberately open to
// any writer (MCP's create_ticket relies on this to auto-create a missing
// label), but update()/delete() require a site admin: renaming or removing
// one reaches every project on the board, including ones a narrowly scoped
// token can't even see.

let key: string

test.beforeAll(async ({ browser }) => {
  key = uniqueKey()
  const page = await browser.newPage({ storageState: 'e2e/.auth/admin.json' })
  await page.goto('/')
  await page.getByRole('link', { name: 'New project' }).click()
  await page.locator('input.pk').fill(key)
  await page.getByPlaceholder('Project name').fill(`Labels test ${key}`)
  await page.locator('.content button.btn-primary', { hasText: 'Create' }).click()
  await expect(page).toHaveURL(new RegExp(`/p/${key}/`))
  await page.close()
})

test.describe('site admin', () => {
  test('can add, rename and delete a label from Settings', async ({ page }) => {
    await page.goto('/settings/labels')
    await expect(page.getByRole('heading', { name: 'Labels' })).toBeVisible()

    const name = `admin-${key}`
    await page.getByPlaceholder('New label').fill(name)
    await page.getByRole('button', { name: 'Add' }).click()
    await page.waitForTimeout(300)

    const row = await labelRow(page, name)
    const nameInput = row.locator('input.input.grow')
    await expect(nameInput).toBeEnabled()
    await expect(row.locator('button[title="Delete"]')).toBeVisible()

    await nameInput.fill(`${name}-renamed`)
    await nameInput.blur()
    await page.waitForTimeout(300)
    const renamedRow = await labelRow(page, `${name}-renamed`)

    await renamedRow.locator('button[title="Delete"]').click()
    await page.locator('[role=dialog] button.btn-danger').click() // confirm the "delete this label" dialog
    await page.waitForTimeout(300)
    expect(await findLabelRow(page, `${name}-renamed`)).toBeNull()
  })
})

test.describe('a project member who is not a site admin', () => {
  let memberToken: string
  let request: APIRequestContext

  test.beforeAll(async ({ playwright, browser }) => {
    const admin = await browser.newPage({ storageState: 'e2e/.auth/admin.json' })

    // An agent account, added as a plain member (not admin) of the project.
    await admin.goto('/settings/people')
    await admin.locator('label', { hasText: 'Agent' }).click()
    await admin.getByPlaceholder('username').fill(`member-bot-${key.toLowerCase()}`)
    await admin.getByRole('button', { name: 'Add agent' }).click()
    await admin.waitForTimeout(300)

    await admin.goto('/settings/projects')
    const projRow = admin.locator('.proj', { has: admin.locator('.pkey', { hasText: key }) })
    await projRow.locator('button[title="Show members"]').click()
    await projRow.locator('.add button.trigger').click()
    await projRow.locator('.add input.filter').fill(`member-bot-${key.toLowerCase()}`)
    await projRow.locator('.add .opt', { hasText: `member-bot-${key.toLowerCase()}` }).click()
    await admin.waitForTimeout(300)

    // A token for that account, scoped to just this one project.
    await admin.goto('/settings/tokens')
    const ownerSelect = admin.locator('.owner-picker select')
    const ownerValue = await ownerSelect
      .locator('option', { hasText: `member-bot-${key.toLowerCase()}` })
      .getAttribute('value')
    await ownerSelect.selectOption(ownerValue!)
    await admin.waitForTimeout(200)
    await admin.locator('.add-form input.input').fill('member-bot token')
    await admin.getByRole('button', { name: 'Create token' }).click()
    await expect(admin.locator('.reveal code')).toBeVisible()
    memberToken = (await admin.locator('.reveal code').innerText()).trim()
    expect(memberToken).toMatch(/^kbn_/)

    await admin.close()
    request = await playwright.request.newContext({
      baseURL: 'http://127.0.0.1:8610',
      extraHTTPHeaders: { authorization: `Bearer ${memberToken}` },
    })
  })

  test('can still create a label', async () => {
    const res = await request.post('/api/labels', { data: { name: `member-made-${key}` } })
    expect(res.status()).toBe(201)
  })

  test('cannot rename or delete a label', async () => {
    const created = await (await request.post('/api/labels', { data: { name: `to-attack-${key}` } })).json()

    const rename = await request.patch(`/api/labels/${created.id}`, { data: { name: 'hijacked' } })
    expect(rename.status()).toBe(403)

    const del = await request.delete(`/api/labels/${created.id}`)
    expect(del.status()).toBe(403)
  })

  test('the Labels settings page hides the actions it can’t use', async ({ browser }) => {
    // Agents don't sign into the UI at all (they only hold tokens), so this
    // checks what a human non-admin sees - the field must be disabled and
    // the delete button gone, or the UI would offer an action that 403s.
    const admin = await browser.newPage({ storageState: 'e2e/.auth/admin.json' })
    await admin.goto('/settings/people')
    await admin.locator('label', { hasText: 'Human' }).click()
    const username = `reviewer-${key.toLowerCase()}`
    await admin.getByPlaceholder('username').fill(username)
    await admin.getByPlaceholder(/password/i).fill('correcthorsebattery')
    await admin.getByRole('button', { name: 'Add human' }).click()
    // Confirm the account really was created before moving on, so a failure
    // here points straight at account creation instead of a mysterious
    // timeout three steps later trying to sign in as a user that doesn't exist.
    await expect(admin.locator('.row', { hasText: `@${username}` })).toBeVisible({ timeout: 5_000 })
    await admin.close()

    // The "chromium" project sets storageState: admin.json as a context
    // default, which browser.newPage() inherits even with no args - has to
    // be overridden explicitly to get a genuinely signed-out page.
    const reviewer = await browser.newPage({ storageState: { cookies: [], origins: [] } })
    await reviewer.goto('/')
    await expect(reviewer.getByRole('heading', { name: 'Sign in' })).toBeVisible({ timeout: 10_000 })
    await reviewer.getByLabel('Username').fill(username)
    await reviewer.getByLabel('Password').fill('correcthorsebattery')
    await reviewer.getByRole('button', { name: 'Sign in' }).click()
    await expect(reviewer.locator('.topbar')).toBeVisible()

    await reviewer.goto('/settings/labels')
    await expect(reviewer.getByText(/only a site administrator can rename/i)).toBeVisible()
    const anyLabelInput = reviewer.locator('.row input.input.grow').first()
    if (await anyLabelInput.count()) {
      await expect(anyLabelInput).toBeDisabled()
      await expect(reviewer.locator('button[title="Delete"]')).toHaveCount(0)
    }
    await reviewer.close()
  })
})
