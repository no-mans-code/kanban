import { execSync } from 'node:child_process'

/** A project key that won't collide with any other spec file's. Board
 * project keys are letters only, so this can't just be a counter. */
export function uniqueKey(): string {
  const letters = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
  let s = ''
  let n = Date.now() % (letters.length ** 4) + Math.floor(Math.random() * 1000)
  for (let i = 0; i < 4; i++) {
    s = letters[n % letters.length] + s
    n = Math.floor(n / letters.length)
  }
  return s
}

/** The one-time setup code the board printed at startup. Only works before
 * the first admin account exists - see auth.setup.ts. */
export function readSetupCode(containerName: string): string {
  const logs = execSync(`docker logs ${containerName}`, { encoding: 'utf8' })
  const match = logs.match(/setup code: ([A-Z0-9]{4}-[A-Z0-9]{4})/)
  if (!match) throw new Error(`No setup code found in ${containerName}'s logs:\n${logs}`)
  return match[1]
}
