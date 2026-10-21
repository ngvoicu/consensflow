import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * The long run's project (`evals/fixtures/long-run`, ledgerline), held to working
 * as the run's staff will find it: its suite passes as it is, its build makes every
 * page and says the entries its planted CSV bug skips, and its dev server serves the
 * site and the API and does not exit. Each is run at no pace (`LONGRUN_PACE_MS=0`):
 * the run's staff run them at the pace they have, minutes each.
 */
const FIXTURE = fileURLToPath(new URL('../evals/fixtures/long-run/', import.meta.url))
// A suite run from inside this one would report to it and print nothing
// (`NODE_TEST_CONTEXT`): the fixture's is a suite of its own.
const { NODE_TEST_CONTEXT: _, ...OWN } = process.env
const QUICK = { ...OWN, LONGRUN_PACE_MS: '0' }

/** `args` of Node in the fixture, at no pace: what it said, and how it ended. */
function node(args) {
  return spawnSync(process.execPath, args, { cwd: FIXTURE, env: QUICK, encoding: 'utf8' })
}

describe('the long run’s project', () => {
  it('passes its own suite as it is, the planted bugs untested', () => {
    const ran = node(['--test', '--test-concurrency=1', '--test-reporter=tap'])
    assert.equal(ran.status, 0, `${ran.stdout}\n${ran.stderr}`)
    assert.match(ran.stdout, /# fail 0/)
    const files = readdirSync(join(FIXTURE, 'test')).filter((name) => name.endsWith('.test.mjs'))
    assert.ok(files.length >= 15, files.join(', '))
  })

  it('builds every page of its site, and says the seven entries the CSV bug skips', () => {
    const out = mkdtempSync(join(tmpdir(), 'ledgerline-dist-'))
    try {
      const ran = node(['scripts/longrun-build.mjs', out])
      assert.equal(ran.status, 0, `${ran.stdout}\n${ran.stderr}`)
      assert.match(ran.stdout, /^Built 23 pages in /m)
      assert.match(ran.stdout, /^skipped 7 invalid entries:/m)
      for (const page of ['index.html', '2025-03.html', 'account-6100.html', 'style.css']) {
        assert.ok(existsSync(join(out, page)), page)
      }
      assert.match(readFileSync(join(out, '2025-03.html'), 'utf8'), /<h1>March 2025<\/h1>/)
    } finally {
      rmSync(out, { recursive: true, force: true })
    }
  })

  it('serves its site and its API, and goes on serving until it is stopped', async () => {
    const out = mkdtempSync(join(tmpdir(), 'ledgerline-dev-'))
    const built = node(['scripts/longrun-build.mjs', out])
    assert.equal(built.status, 0, built.stderr)
    const server = spawn(process.execPath, ['scripts/longrun-dev-server.mjs', out], {
      cwd: FIXTURE,
      env: { ...QUICK, PORT: '0' },
    })
    try {
      const url = await new Promise((resolve, reject) => {
        let said = ''
        server.stdout.on('data', (chunk) => {
          said += chunk
          const found = /serving .+\/ on (http:\/\/127\.0\.0\.1:\d+)/.exec(said)
          if (found) resolve(found[1])
        })
        server.stderr.on('data', (chunk) => {
          said += chunk
        })
        server.on('exit', (code) => reject(new Error(`the server exited (${code}): ${said}`)))
      })
      const page = await fetch(`${url}/2025-03.html`)
      assert.equal(page.status, 200)
      assert.match(await page.text(), /<title>March 2025 · ledgerline<\/title>/)
      const balances = await fetch(`${url}/api/balances?month=2025-03`)
      assert.equal(balances.status, 200)
      assert.ok(Object.hasOwn(await balances.json(), '6100'))
      assert.equal((await fetch(`${url}/nothing.html`)).status, 404)
      await new Promise((resolve) => setTimeout(resolve, 500))
      assert.equal(server.exitCode, null, 'it still serves')
    } finally {
      server.kill()
      rmSync(out, { recursive: true, force: true })
    }
  })
})
