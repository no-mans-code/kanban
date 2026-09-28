// Builds and runs the same Docker image `docker compose up` does, on
// 127.0.0.1:8610, as this directory's Playwright webServer. Kept as plain
// Node (not a shell script) so it runs the same way on every OS Playwright
// itself supports.
import { execSync, spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const here = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(here, '..', '..')
const NAME = 'kanban-e2e-server'
const IMAGE = 'kanban-e2e-image'

try {
  execSync(`docker rm -f ${NAME}`, { stdio: 'ignore' })
} catch {
  // Nothing to remove.
}

console.log('==> Building the kanban-server image for e2e tests')
execSync(`docker build -t ${IMAGE} .`, { cwd: repoRoot, stdio: 'inherit' })

console.log(`==> Starting ${NAME} on http://127.0.0.1:8610`)
const proc = spawn('docker', ['run', '--rm', '--name', NAME, '-p', '127.0.0.1:8610:8610', IMAGE], {
  stdio: 'inherit',
})

for (const sig of ['SIGTERM', 'SIGINT']) {
  process.on(sig, () => {
    proc.kill(sig)
    try {
      execSync(`docker rm -f ${NAME}`, { stdio: 'ignore' })
    } catch {
      // Already gone.
    }
  })
}

proc.on('exit', (code) => process.exit(code ?? 0))
