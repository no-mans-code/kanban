import { test, expect } from '@playwright/test'
import { uniqueKey } from '../fixtures'

let key: string

test.beforeAll(async ({ browser }) => {
  key = uniqueKey()
  const page = await browser.newPage({ storageState: 'e2e/.auth/admin.json' })
  await page.goto('/')
  await page.getByRole('link', { name: 'New project' }).click()
  await page.locator('input.pk').fill(key)
  await page.getByPlaceholder('Project name').fill(`Board test ${key}`)
  await page.locator('.content button.btn-primary', { hasText: 'Create' }).click()
  await expect(page).toHaveURL(new RegExp(`/p/${key}/`))
  await page.close()
})

test.beforeEach(async ({ page }) => {
  await page.goto(`/p/${key}/board`)
  await expect(page.locator('.column.cat-todo')).toBeVisible()
})

test.describe('creating tickets', () => {
  test('the create-ticket modal supports a due date, and it shows on the card', async ({ page }) => {
    await page.locator('.topbar button.btn-primary').click()
    await expect(page.getByText('Create ticket')).toBeVisible()
    await page.getByPlaceholder('Short summary').fill('Ship the release notes')
    await page.locator('input[type=date]').fill('2020-01-01') // safely in the past: always "overdue"
    await page.locator('.modal button.btn-primary', { hasText: 'Create' }).click()
    await page.waitForTimeout(300)

    const card = page.locator('.card', { hasText: 'Ship the release notes' })
    await expect(card.locator('.chip.overdue')).toBeVisible()
  })

  test('quick-add creates a task in the column it was opened from', async ({ page }) => {
    await page.locator('.column.cat-todo button[title^="Add a ticket"]').click()
    await page.locator('.column.cat-todo input.quick').fill('Quick task in To Do')
    await page.keyboard.press('Enter')
    await expect(page.locator('.column.cat-todo .card', { hasText: 'Quick task in To Do' })).toBeVisible()
  })

  test('regression: switching the quick-add box to another column clears the draft', async ({ page }) => {
    // https://github.com/no-mans-code/kanban - a draft typed for one
    // column used to silently follow the quick-add box to whichever
    // column you clicked "+" on next, creating the ticket in the wrong
    // place if you hadn't submitted yet.
    await page.locator('.column.cat-todo button[title^="Add a ticket"]').click()
    await page.locator('.column.cat-todo input.quick').fill('Meant for To Do')

    await page.locator('.column[aria-label="In Progress"] button[title^="Add a ticket"]').click()
    await expect(page.locator('.column[aria-label="In Progress"] input.quick')).toHaveValue('')

    await page.locator('.column[aria-label="In Progress"] input.quick').fill('Actually in Progress')
    await page.keyboard.press('Enter')

    await expect(page.locator('.column[aria-label="In Progress"] .card', { hasText: 'Actually in Progress' })).toBeVisible()
    await expect(page.locator('.column.cat-todo .card', { hasText: 'Meant for To Do' })).toHaveCount(0)
  })
})

test.describe('filtering', () => {
  test('the Overdue toggle hides tickets that aren’t overdue and keeps ones that are', async ({ page }) => {
    // Uses distinctive titles rather than a total card count, since this
    // project accumulates tickets from tests earlier in the file (they
    // share one project - see the top-level beforeAll).
    await page.locator('.column.cat-todo button[title^="Add a ticket"]').click()
    await page.locator('.column.cat-todo input.quick').fill('Not due yet, ever')
    await page.keyboard.press('Enter')
    await page.waitForTimeout(300)

    await page.locator('.topbar button.btn-primary').click()
    await page.getByPlaceholder('Short summary').fill('Very overdue reminder')
    await page.locator('input[type=date]').fill('2020-01-01')
    await page.locator('.modal button.btn-primary', { hasText: 'Create' }).click()
    await page.waitForTimeout(300)
    await page.locator('button[title="Close (Esc)"]').click() // the new ticket's panel auto-opens

    await page.locator('button.overdue-toggle').click()
    await expect(page.locator('.card', { hasText: 'Very overdue reminder' })).toBeVisible()
    await expect(page.locator('.card', { hasText: 'Not due yet, ever' })).toHaveCount(0)
  })
})
