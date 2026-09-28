import { test, expect } from '@playwright/test'
import { uniqueKey } from '../fixtures'

let key: string

test.beforeAll(async ({ browser }) => {
  key = uniqueKey()
  const page = await browser.newPage({ storageState: 'e2e/.auth/admin.json' })
  await page.goto('/')
  await page.getByRole('link', { name: 'New project' }).click()
  await page.locator('input.pk').fill(key)
  await page.getByPlaceholder('Project name').fill(`Workflow test ${key}`)
  await page.locator('.content button.btn-primary', { hasText: 'Create' }).click()
  await expect(page).toHaveURL(new RegExp(`/p/${key}/`))
  await page.close()
})

async function openWorkflow(page: import('@playwright/test').Page) {
  await page.goto('/settings/workflow')
  const select = page.locator('select.select').first()
  const value = await select.locator('option', { hasText: `(${key})` }).getAttribute('value')
  await select.selectOption(value!)
  await expect(page.locator('.row.cat-todo')).toBeVisible()
}

test.describe('WIP limits', () => {
  test('a limit persists across a reload', async ({ page }) => {
    await openWorkflow(page)
    await page.locator('.row.cat-todo input[aria-label="WIP limit"]').fill('2')
    await page.locator('.row.cat-todo input[aria-label="WIP limit"]').blur()
    await page.waitForTimeout(300)

    await page.reload()
    const select = page.locator('select.select').first()
    const value = await select.locator('option', { hasText: `(${key})` }).getAttribute('value')
    await select.selectOption(value!)
    await expect(page.locator('.row.cat-todo input[aria-label="WIP limit"]')).toHaveValue('2')
  })

  test('exceeding the limit warns but does not block adding more tickets', async ({ page }) => {
    await page.goto(`/p/${key}/board`)
    for (const title of ['t1', 't2', 't3']) {
      await page.locator('.column.cat-todo button[title^="Add a ticket"]').click()
      await page.locator('.column.cat-todo input.quick').fill(title)
      await page.keyboard.press('Enter')
      await page.waitForTimeout(300)
    }
    await expect(page.locator('.column.cat-todo .card')).toHaveCount(3, { timeout: 5000 })
    await expect(page.locator('.column.cat-todo .count')).toHaveClass(/over/)
  })

  test('regression: a decimal or negative value is sanitized instead of sent raw', async ({ page }) => {
    // https://github.com/no-mans-code/kanban - wip_limit is an integer
    // column; a plain <input type=number min=1> still accepts "2.7" or
    // "-5" by keyboard, which used to be sent to the server as-is and
    // rejected with a raw deserialization error instead of anything a
    // user could act on.
    await openWorkflow(page)
    const input = page.locator('.row.cat-todo input[aria-label="WIP limit"]')

    await input.fill('2.7')
    await input.blur()
    await page.waitForTimeout(300)
    await expect(input).toHaveValue('3')

    await input.fill('-5')
    await input.blur()
    await page.waitForTimeout(300)
    await expect(input).toHaveValue('')

    await expect(page.locator('.toast.error')).toHaveCount(0)
  })
})
