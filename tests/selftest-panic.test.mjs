import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { runSelftest } from '../app/ui/selftest.js'

/**
 * The page's half of the packaged smoke's case of a command's panic (T-1050): told to, the page
 * asks the app's command that panics and nothing else of the smoke, says what it was answered,
 * and asks a command after it. The smoke on a built app holds the rest
 * (crates/cf-e2e/tests/smoke/panic.rs).
 */
describe('the page of the smoke of a command that panics', () => {
  it('asks the commands that panic, says each answer, and asks a command after them', async () => {
    const asked = []
    const reports = []
    await runSelftest({
      config: { dir: '/workspace', tag: 'tag', panicProbe: true },
      invoke: async (name, body) => {
        if (name === 'selftest_report') {
          reports.push(body)
          return { ok: true }
        }
        asked.push(name)
        if (name.startsWith('selftest_panic')) return { ok: false, error: `${name} failed` }
        return { channel: 'stable' }
      },
      daemon: async () => assert.fail('the probe opens no project'),
    })
    assert.deepEqual(asked, ['selftest_panic', 'selftest_panic_async', 'update_status'])
    assert.deepEqual(reports, [
      { event: 'panic-answered', data: { ok: false, error: 'selftest_panic failed' } },
      {
        event: 'async-panic-answered',
        data: { ok: false, error: 'selftest_panic_async failed' },
      },
      { event: 'after-panic', data: { channel: 'stable' } },
    ])
  })

  it('says a refusal as what it was answered: a release has no such command', async () => {
    const reports = []
    await runSelftest({
      config: { panicProbe: true },
      invoke: async (name, body) => {
        if (name === 'selftest_report') {
          reports.push(body)
          return { ok: true }
        }
        throw `Command ${name} not found`
      },
    })
    assert.deepEqual(reports, [
      { event: 'panic-answered', data: { rejected: 'Command selftest_panic not found' } },
      {
        event: 'async-panic-answered',
        data: { rejected: 'Command selftest_panic_async not found' },
      },
      { event: 'after-panic', data: { rejected: 'Command update_status not found' } },
    ])
  })
})
