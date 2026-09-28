import { test, expect } from '@playwright/test'
import { uniqueKey } from '../fixtures'

let key: string

/** Creates a ticket via the modal (which opens its panel on success) and
 * returns its key, read back from the URL rather than assumed from number.
 * Closes any panel left open by a previous ticket first - it would
 * otherwise sit over the topbar and block the Create button. */
async function newTicket(page: import('@playwright/test').Page, title: string): Promise<string> {
  const closeBtn = page.locator('button[title="Close (Esc)"]')
  if (await closeBtn.isVisible().catch(() => false)) await closeBtn.click()
  await page.locator('.topbar button.btn-primary').click()
  await page.getByPlaceholder('Short summary').fill(title)
  await page.locator('.modal button.btn-primary', { hasText: 'Create' }).click()
  await page.waitForTimeout(300)
  const ticketKey = new URL(page.url()).searchParams.get('ticket')
  if (!ticketKey) throw new Error(`ticket panel did not open after creating "${title}"`)
  return ticketKey
}

test.beforeAll(async ({ browser }) => {
  key = uniqueKey()
  const page = await browser.newPage({ storageState: 'e2e/.auth/admin.json' })
  await page.goto('/')
  await page.getByRole('link', { name: 'New project' }).click()
  await page.locator('input.pk').fill(key)
  await page.getByPlaceholder('Project name').fill(`Panel test ${key}`)
  await page.locator('.content button.btn-primary', { hasText: 'Create' }).click()
  await expect(page).toHaveURL(new RegExp(`/p/${key}/`))
  await page.close()
})

test.beforeEach(async ({ page }) => {
  await page.goto(`/p/${key}/board`)
  await expect(page.locator('.column.cat-todo')).toBeVisible()
})

test('editing the title saves on blur', async ({ page }) => {
  const ticketKey = await newTicket(page, 'Original title')
  // newTicket's own creation already opened the panel (see CreateTicket.svelte).
  const title = page.locator('textarea.title')
  await expect(title).toHaveValue('Original title')
  await title.fill('Renamed title')
  await title.blur()

  // Assert against the API rather than waiting for the board's live-update
  // round trip to redraw the card - what matters here is that blur saves,
  // not how fast the board reflects it (board.spec.ts already covers that).
  await expect(async () => {
    const saved = await (await page.request.get(`/api/tickets/${ticketKey}`)).json()
    expect(saved.title).toBe('Renamed title')
  }).toPass({ timeout: 5_000 })
})

test('the due date field shows overdue styling only for a past date', async ({ page }) => {
  await newTicket(page, 'Due date field check')
  const due = page.locator('.due-input')
  await due.fill('2020-01-01')
  await due.dispatchEvent('change')
  await expect(due).toHaveClass(/overdue/)

  await due.fill('2099-01-01')
  await due.dispatchEvent('change')
  await expect(due).not.toHaveClass(/overdue/)
})

test('a comment posts and appears in the list', async ({ page }) => {
  await newTicket(page, 'Ticket with a comment')
  await page.getByPlaceholder(/Add a comment/).fill('Looks good to me.')
  await page.getByRole('button', { name: 'Comment', exact: true }).click()
  await expect(page.locator('.list')).toContainText('Looks good to me.')
})

test('regression: switching tickets while the title is focused cannot corrupt the wrong one', async ({ page }) => {
  // https://github.com/no-mans-code/kanban - the per-ticket reset effect
  // cleared editingDesc/pickingParent/tab on a ticket switch but not the
  // title field's focus flag, so navigating away without a real blur event
  // (browser back/forward is the common case) left one ticket's unsaved
  // draft sitting in the title box while the panel showed a different
  // ticket underneath it - blurring afterward could patch the WRONG
  // ticket's title with that leftover text.
  const keyA = await newTicket(page, 'Panel A')
  const keyB = await newTicket(page, 'Panel B')

  // Open A then B by URL (?ticket=KEY, exactly what a card click does under
  // the hood - see router.svelte.ts) rather than clicking their cards: the
  // 920px-wide panel covers most of the board at test viewport widths, and
  // fighting Playwright's actionability checks for a card click isn't what
  // this test is actually about. What matters is the switch itself.
  await page.goto(`/p/${key}/board?ticket=${keyA}`)
  await expect(page.locator('textarea.title')).toHaveValue('Panel A')
  await page.goto(`/p/${key}/board?ticket=${keyB}`)
  await expect(page.locator('textarea.title')).toHaveValue('Panel B')

  const title = page.locator('textarea.title')
  await title.click()
  await title.press('End')
  await title.type(' EDITED')
  await page.goBack() // never fires a blur event on the field, unlike a click

  await expect(title).toHaveValue('Panel A', { timeout: 5_000 })
  await page.waitForTimeout(300)

  const [ticketA, ticketB] = await Promise.all([
    page.request.get(`/api/tickets/${keyA}`).then((r) => r.json()),
    page.request.get(`/api/tickets/${keyB}`).then((r) => r.json()),
  ])
  expect(ticketA.title).toBe('Panel A')
  expect(ticketB.title).toBe('Panel B')
})
