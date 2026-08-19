import assert from 'node:assert/strict'
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { installCmuxSkills } from '../src/cmux-skills.js'
import { skillsStatus } from '../src/install.js'
import { tempEnv } from './helpers.mjs'

/**
 * git is PATH-shimmed: `clone` copies a local fixture tree, `rev-parse`
 * prints a fixed commit. No network, no real git history.
 */
function stubGit(t, commit) {
  const fixture = join(t.root, 'cmux-fixture')
  mkdirSync(join(fixture, 'skills', 'cmux-core'), { recursive: true })
  mkdirSync(join(fixture, 'skills', 'cmux-browser'), { recursive: true })
  writeFileSync(join(fixture, 'skills', 'cmux-core', 'SKILL.md'), `core skill @${commit}\n`)
  writeFileSync(join(fixture, 'skills', 'cmux-core', 'reference.md'), 'extra file\n')
  writeFileSync(join(fixture, 'skills', 'cmux-browser', 'SKILL.md'), `browser skill @${commit}\n`)

  mkdirSync(t.env.PATH, { recursive: true })
  const git = join(t.env.PATH, 'git')
  writeFileSync(
    git,
    `#!/bin/sh
PATH=/usr/bin:/bin
for last do :; done
if [ "$1" = "clone" ]; then
  mkdir -p "$last"
  cp -R "${fixture}/." "$last/"
  exit 0
fi
case "$*" in *rev-parse*) echo "${commit}"; exit 0 ;; esac
exit 1
`,
  )
  chmodSync(git, 0o755)
}

function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('cmux skills are fetched with git and owned like our own', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubGit(t, 'abc1234')
  stubCli(t, 'claude')

  it('installs every fixture skill file into the detected agents', () => {
    const outcome = installCmuxSkills(t.env)

    assert.equal(outcome.commit, 'abc1234')
    const core = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')
    assert.equal(readFileSync(core, 'utf8'), 'core skill @abc1234\n')
    // Non-SKILL files travel with their skill.
    assert.equal(
      readFileSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'reference.md'), 'utf8'),
      'extra file\n',
    )

    const status = skillsStatus(t.env)
    assert.ok(status.every((row) => row.source === 'cmux@abc1234'))
    assert.equal(status.length, 3)
  })

  it('update rewrites unchanged files under the new commit', () => {
    stubGit(t, 'def5678')
    const outcome = installCmuxSkills(t.env)

    assert.equal(outcome.commit, 'def5678')
    const core = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')
    assert.equal(readFileSync(core, 'utf8'), 'core skill @def5678\n')
    assert.ok(skillsStatus(t.env).every((row) => row.source === 'cmux@def5678'))
  })

  it('refuses to overwrite a user-edited cmux skill without force', () => {
    const browser = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-browser', 'SKILL.md')
    writeFileSync(browser, 'MY TWEAKS\n')
    stubGit(t, 'aaa9999')

    const outcome = installCmuxSkills(t.env)
    const row = outcome.report.find((r) => r.path === browser)
    assert.equal(row.action, 'refused-drifted')
    assert.equal(readFileSync(browser, 'utf8'), 'MY TWEAKS\n')
  })

  it('says plainly when git is not available', () => {
    const bare = tempEnv()
    try {
      stubCli(bare, 'claude')
      assert.throws(() => installCmuxSkills(bare.env), /git/)
    } finally {
      bare.cleanup()
    }
  })
})
