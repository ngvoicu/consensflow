import assert from 'node:assert/strict'
import { existsSync, mkdirSync, writeFileSync } from 'node:fs'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import { forgetLaunch, sweepLaunches } from '../src/core/launch-files.js'

const A = '11111111-1111-4111-8111-111111111111'
const B = '22222222-2222-4222-8222-222222222222'

/** What a window leaves in the home goes with the window, or at the next start. */
describe('the files a launch leaves in the home', () => {
  it('go when their window closes, and every one at a start', async () => {
    const home = await mkdtemp(path.join(os.tmpdir(), 'cf-launch-files-'))
    try {
      for (const folder of [
        `integrations/claude/${A}`,
        `integrations/pi/${A}`,
        `integrations/opencode/${A}`,
        `integrations/claude/${B}`,
        'integrations/claude/not-a-launch',
        'extensions/pi',
      ]) {
        mkdirSync(path.join(home, folder), { recursive: true })
        writeFileSync(path.join(home, folder, 'file'), 'x')
      }
      forgetLaunch(home, A)
      assert.deepEqual(
        [`integrations/claude/${A}`, `integrations/pi/${A}`, `integrations/opencode/${A}`].map(
          (f) => existsSync(path.join(home, f)),
        ),
        [false, false, false],
      )
      assert.ok(existsSync(path.join(home, `integrations/claude/${B}`)), 'another launch stays')
      forgetLaunch(home, '..')
      forgetLaunch(home, null)
      assert.equal(sweepLaunches(home), 1, 'B went; nothing else is a launch')
      assert.ok(existsSync(path.join(home, 'integrations/claude/not-a-launch')))
      assert.ok(existsSync(path.join(home, 'extensions/pi')))
      assert.equal(sweepLaunches(path.join(home, 'nowhere')), 0)
    } finally {
      await rm(home, { recursive: true, force: true })
    }
  })
})
