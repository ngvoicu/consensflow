import assert from 'node:assert/strict'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { join } from 'node:path'
import { after, describe, it } from 'node:test'
import { installSkill } from '../src/install.js'
import {
  applyMode,
  currentMode,
  MODES,
  modeLabel,
  modeReport,
  resetEverything,
  turnOff,
} from '../src/mode.js'
import { addAgent, removeAgent, rosterPath } from '../src/roster.js'
import { refreshInstalledSkill } from '../src/sync.js'
import { tempEnv } from './helpers.mjs'

function stubCli(t, name, script = '#!/bin/sh\nexit 0\n') {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, script)
  chmodSync(path, 0o755)
}

/** git that clones a two-file cmux skills tree. */
function stubGit(t, commit = 'cmux1234') {
  const fixture = join(t.root, 'cmux-repo')
  mkdirSync(join(fixture, 'skills', 'cmux-core'), { recursive: true })
  writeFileSync(join(fixture, 'skills', 'cmux-core', 'SKILL.md'), 'pane control\n')
  mkdirSync(t.env.PATH, { recursive: true })
  const git = join(t.env.PATH, 'git')
  writeFileSync(
    git,
    `#!/bin/sh
PATH=/usr/bin:/bin
for last do :; done
if [ "$1" = "clone" ]; then mkdir -p "$last"; cp -R "${fixture}/." "$last/"; exit 0; fi
case "$*" in *rev-parse*) echo "${commit}"; exit 0 ;; esac
exit 1
`,
  )
  chmodSync(git, 0o755)
}

/** How many cmux-sourced files the manifest owns right now. */
function cmuxFiles(t) {
  const manifest = join(t.env.CONSENSFLOW_HOME, 'skills-manifest.json')
  if (!existsSync(manifest)) return 0
  const recorded = JSON.parse(readFileSync(manifest, 'utf8')).files ?? {}
  return Object.values(recorded).filter((entry) => entry.source.startsWith('cmux@')).length
}

function bundle(t) {
  const root = join(t.root, 'bundled')
  mkdirSync(join(root, 'lib'), { recursive: true })
  mkdirSync(join(root, 'claude', 'skills', 'consensflow'), { recursive: true })
  mkdirSync(join(root, 'pi'), { recursive: true })
  writeFileSync(join(root, 'lib', 'runners.js'), '// engine\n')
  writeFileSync(join(root, 'claude', 'skills', 'consensflow', 'SKILL.md'), 'cc skill\n')
  writeFileSync(join(root, 'claude', 'hooks.json'), JSON.stringify({ hooks: {} }))
  writeFileSync(join(root, 'pi', 'index.ts'), '// pi\n')
  return root
}

describe('the machine runs exactly one ConsensFlow path', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  for (const cli of ['claude', 'codex', 'opencode', 'pi']) stubCli(t, cli)
  const bundled = bundle(t)
  addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, t.env)

  const generated = (dir) => join(dir, 'skills', 'consensflow', 'SKILL.md')

  it('offers exactly three modes and starts in none of them', () => {
    assert.deepEqual([...MODES].sort(), ['claude', 'cmux', 'pi'])
    assert.equal(currentMode(t.env), null)
  })

  it('labels the cmux mode with the harnesses it covers', () => {
    assert.equal(modeLabel('cmux'), 'cmux (pi, cc, codex, opencode)')
    assert.equal(modeLabel('claude'), 'claude')
  })

  it('still answers to the old name for it', () => {
    applyMode('standalone', t.env, { bundled })
    assert.equal(currentMode(t.env), 'cmux')
  })

  it('cmux mode puts the generated skill on every harness', () => {
    applyMode('cmux', t.env, { bundled })

    assert.equal(currentMode(t.env), 'cmux')
    for (const dir of [t.env.CLAUDE_CONFIG_DIR, t.env.CODEX_HOME]) {
      assert.ok(existsSync(generated(dir)), `${dir} has the generated skill`)
    }
  })

  it('switching to claude takes the generated skill away from everyone', () => {
    applyMode('claude', t.env, { bundled })

    assert.equal(currentMode(t.env), 'claude')
    // Claude Code has the generated skill — the same one every mode installs …
    assert.ok(existsSync(generated(t.env.CLAUDE_CONFIG_DIR)))
    // … and nothing beside it: no payload, no command, no hook …
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts')), false)
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')), false)
    // … and nothing else has a ConsensFlow path any more.
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)
    assert.equal(existsSync(generated(join(t.env.XDG_CONFIG_HOME, 'opencode'))), false)
  })

  it('says out loud which harnesses lose access in this mode', () => {
    const report = modeReport('claude', t.env)

    assert.match(report.join('\n'), /codex/)
    assert.match(report.join('\n'), /opencode/)
    assert.match(report.join('\n'), /no ConsensFlow|nothing/i)
  })

  it('switching to pi takes the skill back from Claude Code and gives it to pi', () => {
    applyMode('pi', t.env, { bundled })

    assert.equal(currentMode(t.env), 'pi')
    // A mode is a scope: the same skill, a different set of harnesses.
    assert.ok(existsSync(generated(join(t.env.HOME, '.pi', 'harness'))))
    assert.equal(existsSync(generated(t.env.CLAUDE_CONFIG_DIR)), false, 'claude gives it back')
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts')), false, 'no payloads anywhere')
  })

  it('going back to cmux mode removes the pi payload again', () => {
    applyMode('cmux', t.env, { bundled })

    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts', 'pi')), false)
    assert.ok(existsSync(generated(t.env.CODEX_HOME)))
  })

  it('adding an agent in a host mode does not smuggle the skill back', () => {
    applyMode('claude', t.env, { bundled })
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)

    // The mutation path must obey the mode, or the invariant only holds
    // until the next roster edit.
    addAgent({ name: 'apollo', harness: 'codex', model: 'gpt-5.6-terra' }, t.env)
    refreshInstalledSkill(t.env)

    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false, 'codex stays out in claude mode')
    removeAgent('apollo', t.env)
    applyMode('cmux', t.env, { bundled })
  })

  it('the first agent in a host mode installs the skill for that harness', () => {
    // A scope decides WHO gets the skill, not WHETHER anyone does. This path
    // returned early for every host mode while `claude` and `pi` installed a
    // hand-written skill through a payload of their own — so once they became
    // scopes, choosing `claude` and adding the first agent left the machine
    // with ConsensFlow on and no skill anywhere.
    const fresh = tempEnv()
    try {
      stubCli(fresh, 'claude')
      stubCli(fresh, 'codex')
      applyMode('claude', fresh.env, { bundled })
      assert.equal(
        existsSync(generated(fresh.env.CLAUDE_CONFIG_DIR)),
        false,
        'an empty roster installs nothing — there is no skill to generate yet',
      )

      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, fresh.env)
      refreshInstalledSkill(fresh.env)

      assert.ok(existsSync(generated(fresh.env.CLAUDE_CONFIG_DIR)), 'the chosen harness gets it')
      assert.equal(existsSync(generated(fresh.env.CODEX_HOME)), false, 'and nobody else does')

      // And every change after regenerates it, so the description keeps naming
      // the whole roster — which is what makes a harness reach for the skill.
      addAgent({ name: 'gaia', harness: 'codex', model: 'gpt-5.6-terra' }, fresh.env)
      refreshInstalledSkill(fresh.env)
      assert.match(readFileSync(generated(fresh.env.CLAUDE_CONFIG_DIR), 'utf8'), /gaia/)
    } finally {
      fresh.cleanup()
    }
  })

  it('keeps a skill when the harness CLI simply did not resolve this run', () => {
    // The app asks a NON-interactive login shell for PATH, and zsh -lc never
    // reads .zshrc — where per-tool bin dirs usually live. So the app saw a
    // narrower set than the terminal, decided a harness was out of scope, and
    // took its skill back on every mode apply while a terminal put it straight
    // back. Only a change of MODE may remove; a narrower PATH may not.
    const fresh = tempEnv()
    try {
      stubCli(fresh, 'claude')
      stubCli(fresh, 'codex')
      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, fresh.env)
      applyMode('cmux', fresh.env, { bundled })
      assert.ok(existsSync(generated(fresh.env.CODEX_HOME)), 'codex has it')

      // Same mode, same machine — but codex is invisible on this run.
      rmSync(join(fresh.env.PATH, 'codex'), { force: true })
      applyMode('cmux', fresh.env, { bundled })

      assert.ok(
        existsSync(generated(fresh.env.CODEX_HOME)),
        'a harness that did not resolve keeps what it has',
      )
    } finally {
      fresh.cleanup()
    }
  })

  it('still takes the skill back when the MODE stops covering a harness', () => {
    const fresh = tempEnv()
    try {
      stubCli(fresh, 'claude')
      stubCli(fresh, 'codex')
      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, fresh.env)
      applyMode('cmux', fresh.env, { bundled })
      assert.ok(existsSync(generated(fresh.env.CODEX_HOME)))

      applyMode('claude', fresh.env, { bundled })

      assert.equal(existsSync(generated(fresh.env.CODEX_HOME)), false, 'scope really narrowed')
      assert.ok(existsSync(generated(fresh.env.CLAUDE_CONFIG_DIR)), 'and claude still has it')
    } finally {
      fresh.cleanup()
    }
  })

  it('installs nothing until a mode is chosen', () => {
    // Adding an agent before picking a path used to install the generated
    // skill into every harness found — so ConsensFlow appeared in Claude Code
    // without anyone choosing Claude Code, and the one-path invariant was
    // broken before the user ever saw the switch.
    const fresh = tempEnv()
    try {
      stubCli(fresh, 'claude')
      stubCli(fresh, 'codex')
      assert.equal(currentMode(fresh.env), null, 'no mode yet')

      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, fresh.env)
      refreshInstalledSkill(fresh.env)

      assert.equal(
        existsSync(join(fresh.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
        false,
        'nothing is installed before a path is chosen',
      )
      assert.equal(
        existsSync(join(fresh.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')),
        false,
      )

      // Choosing the path is what installs it.
      stubGit(fresh)
      applyMode('cmux', fresh.env, { bundled })
      assert.ok(existsSync(join(fresh.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
    } finally {
      fresh.cleanup()
    }
  })

  it('puts `cf` on PATH in every mode, because every skill says `cf run`', () => {
    // The generated skill tells four harnesses to run `cf run`; if `cf` does
    // not resolve, every line in it is a lie.
    stubGit(t)
    applyMode('cmux', t.env, { bundled })
    const launcher = join(t.env.CONSENSFLOW_BIN_DIR, 'cf')
    assert.ok(existsSync(launcher), 'cmux mode installs the launcher')
    assert.match(readFileSync(launcher, 'utf8'), /Installed by ConsensFlow/)

    applyMode('claude', t.env, { bundled })
    assert.ok(existsSync(launcher), 'a host mode teaches the same line, so it keeps cf too')
  })

  it('installs no cmux skills in any mode — ConsensFlow ships one skill', () => {
    // The cloning era shipped ~300 files of cmux-development docs into every
    // harness for the sake of three pane commands the generated skill now
    // quotes itself. cmux mode is a scope over OUR skill, nothing more.
    applyMode('cmux', t.env, { bundled })

    assert.equal(cmuxFiles(t), 0, 'no cmux-sourced files in cmux mode either')
    assert.equal(existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core')), false)
  })

  it('takes back what the cloning era installed, whichever mode is chosen', () => {
    // A machine upgraded from the cloning era still holds cmux@… files; any
    // mode switch retires them, cache included.
    const planted = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'cmux-core', 'SKILL.md')
    installSkill(
      { relPath: 'cmux-core/SKILL.md', content: 'pane control\n', source: 'cmux@abc1234' },
      t.env,
    )
    assert.ok(existsSync(planted), 'fixture: the cloning era file is there')
    const cache = join(t.env.CONSENSFLOW_HOME, 'cache', 'cmux')
    mkdirSync(cache, { recursive: true })

    applyMode('cmux', t.env, { bundled })

    assert.equal(cmuxFiles(t), 0)
    assert.equal(existsSync(planted), false)
    assert.equal(existsSync(cache), false, 'the checkout cache goes with them')
  })

  it('does not reach for cmux at all in a host mode', () => {
    // git that fails loudly: a host mode must never clone, so the switch
    // succeeds without it. (Offline machines choose cc modes too.)
    mkdirSync(t.env.PATH, { recursive: true })
    const git = join(t.env.PATH, 'git')
    writeFileSync(git, '#!/bin/sh\necho "no network" >&2\nexit 1\n')
    chmodSync(git, 0o755)

    const applied = applyMode('claude', t.env, { bundled })
    assert.equal(applied.mode, 'claude')
    assert.equal(
      applied.report.some((line) => line.includes('cmux skills were not fetched')),
      false,
      'a host mode has no cmux skills to miss, so it must not report a failure',
    )
  })

  it('needs no git and no network to switch modes', () => {
    // The cloning era fetched cmux's skills on every switch; nothing fetches
    // anything now, so an offline machine with no git still gets its mode.
    const offline = tempEnv()
    try {
      stubCli(offline, 'codex')
      addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, offline.env)

      applyMode('cmux', offline.env, { bundled })

      assert.equal(currentMode(offline.env), 'cmux')
      assert.ok(existsSync(join(offline.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')))
    } finally {
      offline.cleanup()
    }
  })

  it('turns everything off: no path, no skills, no payloads, no mode', () => {
    stubGit(t)
    applyMode('cmux', t.env, { bundled })
    assert.ok(existsSync(generated(t.env.CODEX_HOME)))

    const outcome = turnOff(t.env)

    assert.equal(currentMode(t.env), null)
    assert.equal(existsSync(generated(t.env.CODEX_HOME)), false)
    assert.equal(existsSync(join(t.env.CODEX_HOME, 'skills', 'cmux-core', 'SKILL.md')), false)
    assert.ok(outcome.changes.length > 0)

    // "Off" means off: no bookkeeping left claiming state that is gone. The
    // roster is the one thing that stays — it is the user's, and it outlives
    // any install. (With CONSENSFLOW_HOME set, as here, it sits in the same
    // root as the state, which is the point of that variable.)
    const config = t.env.CONSENSFLOW_HOME
    const leftovers = (existsSync(config) ? readdirSync(config) : []).filter(
      (name) => name !== 'agents.json',
    )
    assert.deepEqual(leftovers, [], `nothing should remain, found ${leftovers.join(', ')}`)
    assert.ok(existsSync(rosterPath(t.env)), 'the roster survives being turned off')
  })

  it('refuses a mode it does not have, naming the ones it does', () => {
    assert.throws(() => applyMode('emacs', t.env, { bundled }), /claude, pi, cmux/)
  })
})

describe('reset leaves the machine as if ConsensFlow had never been installed', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  const bundled = bundle(t)
  const generated = (dir) => join(dir, 'skills', 'consensflow', 'SKILL.md')

  it('removes what off keeps: the roster and every run artifact', () => {
    stubCli(t, 'claude')
    addAgent({ name: 'zeus', harness: 'claude', model: 'claude-opus-5' }, t.env)
    applyMode('claude', t.env, { bundled })
    assert.ok(existsSync(generated(t.env.CLAUDE_CONFIG_DIR)), 'installed first')

    // A run artifact, of the kind that exists nowhere else.
    const runDir = join(t.env.CONSENSFLOW_HOME, 'workspaces', 'proj-abc', 'runs', 'ask-1')
    mkdirSync(runDir, { recursive: true })
    writeFileSync(join(runDir, 'packet.md'), '# ConsensFlow Packet\n')

    const outcome = resetEverything(t.env)

    assert.equal(outcome.removed.agents, 1, 'it counts what it destroyed')
    assert.equal(outcome.removed.runs, 1)
    assert.equal(existsSync(t.env.CONSENSFLOW_HOME), false, 'the whole root is gone')
    assert.equal(existsSync(generated(t.env.CLAUDE_CONFIG_DIR)), false, 'and the skill with it')
    assert.equal(currentMode(t.env), null, 'no mode survives')
  })

  it('off keeps the roster; reset is the only thing that does not', () => {
    stubCli(t, 'claude')
    addAgent({ name: 'apollo', harness: 'claude', model: 'claude-opus-5' }, t.env)
    applyMode('claude', t.env, { bundled })

    turnOff(t.env)
    assert.ok(existsSync(rosterPath(t.env)), "off is not a reset — agents are the user's")

    resetEverything(t.env)
    assert.equal(existsSync(rosterPath(t.env)), false, 'reset is')
  })

  it('takes a skill the user edited too, which off refuses to', () => {
    stubCli(t, 'claude')
    addAgent({ name: 'hermes', harness: 'claude', model: 'claude-opus-5' }, t.env)
    applyMode('claude', t.env, { bundled })
    writeFileSync(generated(t.env.CLAUDE_CONFIG_DIR), 'my own notes on top of the skill\n')

    // Drift is sacred so an install never clobbers an edit by accident. A
    // reset is not an accident — it is the operation that leaves nothing.
    resetEverything(t.env)

    assert.equal(existsSync(generated(t.env.CLAUDE_CONFIG_DIR)), false)
  })

  it("takes the desktop app's own data, but never the app bundle", () => {
    // These carry ConsensFlow's bundle identifier, so nothing else creates
    // them — a reset that left them would be leaving something behind. The
    // .app is deliberately not in that set: an application deleting its own
    // bundle mid-run is a bad idea, and on macOS that is a Finder gesture.
    const appData = join(t.env.HOME, 'Library', 'Caches', 'dev.ngvoicu.consensflow')
    const webkit = join(t.env.HOME, 'Library', 'WebKit', 'dev.ngvoicu.consensflow')
    const bundle = join(t.env.HOME, 'Applications', 'ConsensFlow.app')
    for (const dir of [appData, webkit, bundle]) mkdirSync(dir, { recursive: true })
    writeFileSync(join(appData, 'cached.bin'), 'webview cache\n')

    const outcome = resetEverything(t.env)

    assert.equal(existsSync(appData), false, 'the app cache goes')
    assert.equal(existsSync(webkit), false, 'and its webview data')
    assert.ok(existsSync(bundle), 'the bundle itself is not ours to delete')
    assert.ok(
      outcome.changes.some((change) => change.path === appData),
      'and it says so, rather than removing it silently',
    )
  })

  it('survives a machine with nothing installed', () => {
    const fresh = tempEnv()
    try {
      assert.doesNotThrow(() => resetEverything(fresh.env))
      assert.deepEqual(resetEverything(fresh.env).removed, { agents: 0, runs: 0 })
    } finally {
      fresh.cleanup()
    }
  })
})

describe('threading follows the mode, and can be overridden anywhere', () => {
  // The default lives in the run verb (`currentMode(env) === 'cmux'`), which
  // the CLI suite exercises end to end. This pins the rule itself so a future
  // refactor cannot quietly flip which modes converse.
  it('cmux is the mode that converses', () => {
    assert.equal(MODES.includes('cmux'), true)
    assert.deepEqual(
      MODES.filter((mode) => mode !== 'cmux'),
      ['claude', 'pi'],
      'the host modes stay one-shot; only cmux threads by default',
    )
  })
})
