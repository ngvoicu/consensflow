import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

/**
 * The Windows smoke (app/scripts/windows-smoke.mjs) drives the page the app ships, on a desktop
 * no CI job has, so what it names of that page goes stale without a test going red: the Agents
 * page was removed with the saved agents (T-1187) and the smoke waited for it for weeks (alpha.3's
 * copy and alpha.4's, T-1366). These hold what it names to what the page and the daemon take today.
 */
const smoke = readFileSync(new URL('../app/scripts/windows-smoke.mjs', import.meta.url), 'utf8')
const page = readFileSync(new URL('../app/ui/index.html', import.meta.url), 'utf8')

/** The words of the screens the Settings dialog opens, as the page labels its buttons. */
function settingsScreens(html) {
  const dialog = html.match(/<dialog id="settings-dialog"[\s\S]*?<\/dialog>/)?.[0] ?? ''
  return [...dialog.matchAll(/<button[^>]*\sdata-agents-page="[^"]*"[^>]*>([^<]+)<\/button>/g)].map(
    (entry) => entry[1].trim(),
  )
}

/** The screens the smoke opens from Settings: the list its loop is given. */
function smokeScreens(script) {
  const list = script.match(/for \(const screen of \[([^\]]*)\]\)/)?.[1]
  assert.ok(list, 'the smoke opens the screens of Settings in `for (const screen of [...])`')
  return [...list.matchAll(/'([^']+)'/g)].map((word) => word[1])
}

/** The bodies the smoke sends with `project.open`. */
function projectOpens(script) {
  return [...script.matchAll(/daemon\('project\.open',\s*\{([^}]*)\}\)/g)].map((call) => call[1])
}

test('the page has the screens the smoke opens from Settings, and the smoke opens each one', () => {
  const has = settingsScreens(page)
  assert.deepEqual(has, ['Harnesses'], 'the Settings dialog of app/ui/index.html')
  assert.deepEqual(smokeScreens(smoke), has)
})

test('the smoke opens its project as the New project dialog does: the chief by its harness and its model', () => {
  const opens = projectOpens(smoke)
  assert.equal(opens.length, 1, 'the smoke opens one project')
  const [body] = opens
  // `project.open` takes the chief's harness and model (chief_asked, require_chief_model in
  // crates/cf-daemon/src/page/); a saved agent's name is none of its fields since T-1187.
  assert.match(body, /\bharness\b/)
  assert.match(body, /\bmodel\b/)
  assert.doesNotMatch(body, /\bagent\b/)
})

test('the harness the smoke opens its project on is one it has drawn, and lists the model it names', () => {
  const model = smoke.match(/const SMOKE_MODEL = '([^']+)'/)?.[1]
  assert.ok(model, "the smoke names the chief's model in `const SMOKE_MODEL`")
  const literal = smoke.match(/const FAKE_CLAUDE = (`[\s\S]*?\n`)/)?.[1]
  assert.ok(literal, 'the smoke holds the stand-in claude in `const FAKE_CLAUDE`')
  // The stand-in is the script the smoke writes, as the smoke evaluates it. It answers the
  // read of the models (`claude -p`, an initialize control request, no turn) with the model the
  // chief is opened on: the daemon checks a pick against the list its harness gives.
  const folder = mkdtempSync(join(tmpdir(), 'cf-smoke-claude-'))
  try {
    const script = join(folder, 'fake-claude.mjs')
    writeFileSync(script, new Function('SMOKE_MODEL', `return ${literal}`)(model))
    const asked = {
      type: 'control_request',
      request_id: 'models',
      request: { subtype: 'initialize' },
    }
    const read = spawnSync(process.execPath, [script, '-p', '--input-format', 'stream-json'], {
      input: `${JSON.stringify(asked)}\n`,
      encoding: 'utf8',
      timeout: 20_000,
    })
    assert.equal(read.status, 0, read.stderr)
    const answer = JSON.parse(read.stdout.trim().split('\n').at(-1))
    assert.equal(answer.type, 'control_response')
    assert.equal(answer.response.subtype, 'success')
    assert.equal(answer.response.request_id, 'models')
    assert.deepEqual(
      answer.response.response.models.map((row) => row.resolvedModel),
      [model],
    )
    const version = spawnSync(process.execPath, [script, '--version'], { encoding: 'utf8' })
    assert.match(version.stdout, /^\d+\.\d+\.\d+ \(Claude Code\)/)
  } finally {
    rmSync(folder, { recursive: true, force: true })
  }
  // And it is the one harness found: a `claude.cmd` in the shape npm writes, which is the only
  // shape the daemon opens a window on, in a folder put first on the app's PATH.
  assert.match(smoke, /writeFileSync\(\s*join\(bin, 'claude\.cmd'\)/)
  assert.match(smoke, /"\$\{process\.execPath\}" "\$\{join\(bin, 'fake-claude\.mjs'\)\}" %\*/)
  assert.match(smoke, /const path = \[\s*bin,/)
})
