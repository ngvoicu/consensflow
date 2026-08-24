import assert from 'node:assert/strict'
import { execFile, spawn } from 'node:child_process'
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join } from 'node:path'
import { after, describe, it } from 'node:test'
import { promisify } from 'node:util'
import { rosterPath } from '../src/roster.js'
import { chooseCmuxMode, tempEnv } from './helpers.mjs'

const run = promisify(execFile)
const CF = join(import.meta.dirname, '..', 'bin', 'cf.mjs')
const FIXTURES = join(import.meta.dirname, 'fixtures')

async function cf(args, env) {
  try {
    const { stdout, stderr } = await run(process.execPath, [CF, ...args], {
      env,
      timeout: 30_000,
    })
    return { code: 0, stdout, stderr }
  } catch (cause) {
    return { code: cause.code ?? 1, stdout: cause.stdout ?? '', stderr: cause.stderr ?? '' }
  }
}

/** Like `cf`, but types into it — a REPL needs stdin. */
function cfTyping(args, env, lines) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [CF, ...args], { env, stdio: ['pipe', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', (chunk) => {
      stdout += chunk
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk
    })
    child.on('close', (code) => resolve({ code: code ?? 0, stdout, stderr }))
    child.stdin.end(lines.map((line) => `${line}\n`).join(''))
    setTimeout(() => child.kill(), 30_000).unref()
  })
}

function stubCli(t, name) {
  mkdirSync(t.env.PATH, { recursive: true })
  const path = join(t.env.PATH, name)
  writeFileSync(path, '#!/bin/sh\nexit 0\n')
  chmodSync(path, 0o755)
}

describe('cf manages the roster', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())

  it('adds, lists, edits and removes an agent', async () => {
    const added = await cf(
      [
        'agent',
        'add',
        'zeus',
        '--harness',
        'claude',
        '--model',
        'claude-opus-5',
        '--effort',
        'max',
      ],
      t.env,
    )
    assert.equal(added.code, 0)

    const listed = await cf(['agent', 'list'], t.env)
    assert.match(listed.stdout, /zeus/)
    assert.match(listed.stdout, /claude-opus-5/)

    const asJson = await cf(['agent', 'list', '--json'], t.env)
    assert.equal(JSON.parse(asJson.stdout).agents[0].effort, 'max')

    const edited = await cf(['agent', 'edit', 'zeus', '--model', 'claude-fable-5'], t.env)
    assert.equal(edited.code, 0)

    const removed = await cf(['agent', 'remove', 'zeus'], t.env)
    assert.equal(removed.code, 0)
    assert.doesNotMatch((await cf(['agent', 'list'], t.env)).stdout, /zeus/)
  })

  it('fails an unknown verb loudly', async () => {
    const out = await cf(['frobnicate'], t.env)
    assert.notEqual(out.code, 0)
  })

  it('prints its version', async () => {
    const out = await cf(['--version'], t.env)
    assert.match(out.stdout, /3\.0\.0/)
  })

  it('survives its output pipe closing early, like `cf … | head`', async () => {
    const { spawn } = await import('node:child_process')
    // `false` never reads: the pipe is closed before cf writes anything, so
    // every write EPIPEs. PIPESTATUS surfaces cf's own exit code.
    const child = spawn(
      '/bin/bash',
      ['-c', `"${process.execPath}" "${CF}" help | false; exit \${PIPESTATUS[0]}`],
      {
        env: { ...t.env, PATH: `${t.env.PATH}:/usr/bin:/bin` },
        stdio: ['ignore', 'ignore', 'pipe'],
      },
    )
    let stderr = ''
    child.stderr.on('data', (chunk) => {
      stderr += chunk
    })
    const code = await new Promise((resolve) => child.on('close', resolve))
    assert.doesNotMatch(stderr, /EPIPE/)
    assert.equal(code, 0)
  })
})

describe('roster changes keep installed skills current', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('skills install writes the generated skill into every detected harness', async () => {
    // No git on this fake PATH: the cmux fetch fails, which must not stop
    // the consensflow skill from installing.
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)
    const out = await cf(['skills', 'install'], t.env)
    assert.equal(out.code, 0)

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /zeus/)
    assert.match(installed, /claude-opus-5/)
  })

  it('editing an agent regenerates the installed skill everywhere', async () => {
    await cf(['agent', 'edit', 'zeus', '--model', 'claude-fable-5'], t.env)

    for (const dir of [t.env.CLAUDE_CONFIG_DIR, t.env.CODEX_HOME]) {
      const installed = readFileSync(join(dir, 'skills', 'consensflow', 'SKILL.md'), 'utf8')
      assert.match(installed, /claude-fable-5/)
    }
  })

  it('skills status reports every owned file, and uninstall clears them', async () => {
    const status = await cf(['skills', 'status'], t.env)
    assert.match(status.stdout, /consensflow\/SKILL\.md/)
    assert.match(status.stdout, /ok/)

    const out = await cf(['skills', 'uninstall'], t.env)
    assert.equal(out.code, 0)
    assert.equal((await cf(['skills', 'status'], t.env)).stdout.trim(), 'no skills installed')
  })
})

describe('the machine runs one mode, and cf keeps it that way', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('reports no mode before one is chosen', async () => {
    const out = await cf(['mode'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /cmux/)
    assert.match(out.stdout, /not set|none/i)
  })

  it('switches to cmux mode and says who can consult', async () => {
    await cf(['agent', 'add', 'zeus'], t.env)
    const out = await cf(['use', 'cmux'], t.env)

    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /codex/)
    assert.ok(
      existsSync(join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')),
      'codex got the generated skill',
    )
  })

  it('hand-installs the generated skill for the scope, in a host mode too', async () => {
    // It used to refuse here, which was right while `claude` was an
    // integration with a hand-written skill of its own. It is a scope over
    // this same skill now, so refusing would leave that harness the only one
    // that cannot have the only skill there is.
    writeFileSync(join(t.env.CONSENSFLOW_HOME, 'mode.json'), JSON.stringify({ mode: 'claude' }))
    // Earlier tests in this env ran in cmux mode, which reaches every harness.
    // Clear the out-of-scope one so this asserts what THIS install did.
    rmSync(join(t.env.CODEX_HOME, 'skills', 'consensflow'), { recursive: true, force: true })

    const out = await cf(['skills', 'install'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.ok(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      'the harness in scope gets it',
    )
    assert.equal(
      existsSync(join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')),
      false,
      'and nobody out of scope does',
    )
  })

  it('still refuses to install before any path has been chosen', async () => {
    rmSync(join(t.env.CONSENSFLOW_HOME, 'mode.json'), { force: true })

    const out = await cf(['skills', 'install'], t.env)

    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /no path chosen yet/)
  })

  it('names the modes when given one it does not have', async () => {
    const out = await cf(['use', 'emacs'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /claude, pi, cmux/)
  })
})

describe('the host-integration verbs are gone, not hidden', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())

  // `claude` and `pi` are scopes over one generated skill now, and `cf use`
  // is how you choose one. There is no separate integration to install, so
  // the verbs that installed one do not linger as aliases.
  for (const verb of ['hosts', 'install', 'uninstall']) {
    it(`no longer answers \`${verb}\``, async () => {
      const out = await cf([verb, 'claude'], t.env)
      assert.notEqual(out.code, 0)
      assert.match(out.stdout + out.stderr, /unknown command/)
    })
  }

  it('names the modes it does know when given one it does not', async () => {
    const out = await cf(['use', 'emacs'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /claude, pi, cmux/)
  })
})

describe('cf explains where it stands aside for a host integration', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())
  stubCli(t, 'claude')
  stubCli(t, 'codex')

  it('setup names the host that already has its own ConsensFlow', async () => {
    mkdirSync(join(t.env.HOME, '.claude', 'plugins', 'cache', 'consensflow-cc'), {
      recursive: true,
    })
    await cf(['agent', 'add', 'zeus'], t.env)

    const out = await cf(['setup'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /consensflow-cc/)
    assert.match(out.stdout, /--all/)
    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
    assert.ok(existsSync(join(t.env.CODEX_HOME, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('installs there anyway when asked with --all', async () => {
    const out = await cf(['skills', 'install', '--all'], t.env)
    assert.equal(out.code, 0)
    assert.ok(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')))
  })

  it('doctor reports the native integration too', async () => {
    const out = await cf(['doctor'], t.env)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /own consensflow|native/i)
  })
})

describe('the catalog turns a name into a working agent', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())

  it('lists ready-made agents per tool', async () => {
    const out = await cf(['catalog'], t.env)
    assert.equal(out.code, 0)
    assert.match(out.stdout, /claude/)
    assert.match(out.stdout, /zeus/)
    assert.match(out.stdout, /codex/)
    assert.match(out.stdout, /hyperion/)
    assert.match(out.stdout, /glm-5\.3/)
  })

  it('narrows to one tool on request, and answers JSON for scripts', async () => {
    const out = await cf(['catalog', '--harness', 'opencode'], t.env)
    assert.match(out.stdout, /mani/)
    assert.doesNotMatch(out.stdout, /hyperion/)

    const json = await cf(['catalog', '--json'], t.env)
    assert.ok(JSON.parse(json.stdout).catalog.pi.length > 0)
  })

  it('adds a catalog agent from its name alone', async () => {
    const out = await cf(['agent', 'add', 'hyperion'], t.env)
    assert.equal(out.code, 0)

    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const hyperion = listed.agents[0]
    assert.equal(hyperion.harness, 'codex')
    assert.equal(hyperion.model, 'gpt-5.6-sol')
    assert.equal(hyperion.effort, 'ultra')
  })

  it('still requires harness and model for a name it does not know', async () => {
    const out = await cf(['agent', 'add', 'nemo'], t.env)
    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /cf catalog|--harness/)
  })

  it('lets explicit flags override a catalog entry', async () => {
    await cf(['agent', 'add', 'diana', '--effort', 'low'], t.env)
    const listed = JSON.parse((await cf(['agent', 'list', '--json'], t.env)).stdout)
    const diana = listed.agents.find((p) => p.name === 'diana')
    assert.equal(diana.model, 'gpt-5.6-luna')
    assert.equal(diana.effort, 'low')
  })
})

describe('the skill heals itself when cc or pi edit the shared roster', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('any cf invocation regenerates a skill the roster has outrun', async () => {
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)

    // cc adds an agent behind v3's back: a raw write to the shared file.
    const rosterFile = rosterPath(t.env)
    const raw = JSON.parse(readFileSync(rosterFile, 'utf8'))
    raw.agents.push({
      id: 'apollo',
      name: 'Apollo',
      kind: 'codex',
      toolsPolicy: 'workspace-write',
      model: 'gpt-5.6-terra',
      effort: 'xhigh',
    })
    writeFileSync(rosterFile, JSON.stringify(raw, null, 2))

    // Any verb at all — not a skills verb — notices and heals.
    await cf(['doctor'], t.env)

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /apollo/)
  })

  it('never resurrects a skill the user uninstalled', async () => {
    await cf(['skills', 'uninstall'], t.env)

    const rosterFile = rosterPath(t.env)
    const raw = JSON.parse(readFileSync(rosterFile, 'utf8'))
    raw.agents[0].model = 'changed-again'
    writeFileSync(rosterFile, JSON.stringify(raw, null, 2))

    await cf(['doctor'], t.env)

    assert.equal(
      existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')),
      false,
    )
  })
})

describe('cf setup readies a machine in one command', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())
  stubCli(t, 'claude')

  it('a machine that already ran cc or pi gets its skill from the shared roster', async () => {
    // The cc/pi roster IS the roster: no import, no copy.
    mkdirSync(dirname(rosterPath(t.env)), { recursive: true })
    cpSync(join(FIXTURES, 'v1-agents.json'), rosterPath(t.env))

    const out = await cf(['setup'], t.env)
    assert.equal(out.code, 0)

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /hyperion/)
    // The image agent is in the skill too now: one verb spawns it like the rest.
    assert.match(installed, /pygmalion/)
    const listed = await cf(['agent', 'list'], t.env)
    assert.match(listed.stdout, /pygmalion/)
  })

  it('an agent added on top regenerates the installed skill', async () => {
    await cf(['agent', 'add', 'freya', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)

    const installed = readFileSync(
      join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md'),
      'utf8',
    )
    assert.match(installed, /freya/)
    assert.match(installed, /hyperion/)
  })

  it('is idempotent', async () => {
    const out = await cf(['setup'], t.env)
    assert.equal(out.code, 0)
  })
})

describe('the CLI can undo an install as completely as the app', () => {
  const t = tempEnv()
  chooseCmuxMode(t)
  after(() => t.cleanup())

  it('takes everything back but the roster', async () => {
    stubCli(t, 'claude')
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)
    await cf(['use', 'claude'], t.env)
    const skill = join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')
    assert.ok(existsSync(skill), 'the generated skill is installed for the chosen scope')

    const off = await cf(['off'], t.env)
    assert.equal(off.code, 0)
    assert.match(off.stdout, /off/i)

    assert.equal(existsSync(skill), false, 'the skill is taken back')
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'hosts')), false, 'no payload survives')
    assert.equal(existsSync(join(t.env.CLAUDE_CONFIG_DIR, 'commands', 'consensflow.md')), false)
    assert.equal(existsSync(join(t.env.CONSENSFLOW_HOME, 'mode.json')), false)

    // Agents are the user's, and outlive any install.
    const listed = await cf(['agent', 'list'], t.env)
    assert.match(listed.stdout, /zeus/)
  })
})

describe('cf run spawns one agent, in whatever mode this machine runs', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  /** A harness that answers instantly, so the test never touches a real CLI. */
  function stubCodex() {
    mkdirSync(t.env.PATH, { recursive: true })
    const path = join(t.env.PATH, 'codex')
    writeFileSync(
      path,
      `#!/bin/sh\necho '{"type":"item.completed","item":{"type":"agent_message","text":"answered"}}'\n`,
    )
    chmodSync(path, 0o755)
  }

  function packetOf() {
    // The payload writes artifacts under CONSENSFLOW_HOME when it is set —
    // the manager reads that variable as its state root, which is the known
    // collision between the two halves.
    const runs = join(t.env.CONSENSFLOW_HOME, 'workspaces')
    const found = []
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name)
        if (entry.isDirectory()) walk(full)
        else if (entry.name === 'packet.md') found.push(full)
      }
    }
    walk(runs)
    return readFileSync(found[found.length - 1], 'utf8')
  }

  it('carries the brief and the task, and streams the answer back', async () => {
    stubCodex()
    await cf(['agent', 'add', 'diana'], t.env)

    const out = await cf(
      ['run', '@diana', '--brief', 'You are reviewing this for GDPR.', 'check the export path'],
      t.env,
    )
    assert.equal(out.code, 0)
    assert.match(out.stdout, /answered/)

    const packet = packetOf()
    assert.match(packet, /## Your brief for this run/)
    assert.match(packet, /reviewing this for GDPR/)
    assert.match(packet, /check the export path/)
    // Nothing invents a persona, and no handoff is sent unless one is given.
    assert.doesNotMatch(packet, /You are Diana/)
    assert.doesNotMatch(packet, /## Handoff/)
  })

  it('prints the answer once, not once streamed and once again', async () => {
    stubCodex()
    const out = await cf(['run', '@diana', 'how are you'], t.env)

    const answer = 'answered'
    const times = out.stdout.split(answer).length - 1
    assert.equal(times, 1, `the answer appears ${times} times:\n${out.stdout}`)
    // Attribution still happens — it just stops repeating the text.
    assert.match(out.stdout, /@diana/)
  })

  it('sends the conversation only when the lead hands one over', async () => {
    const handoff = join(t.root, 'history.md')
    writeFileSync(handoff, 'user: we decided to drop the retention job\n')

    await cf(['run', '@diana', '--handoff-file', handoff, 'is that safe?'], t.env)
    const packet = packetOf()
    assert.match(packet, /## Handoff — current session/)
    assert.match(packet, /drop the retention job/)
  })

  it('names the agents you have when asked for one you do not', async () => {
    const out = await cf(['run', '@nobody', 'hello'], t.env)
    assert.equal(out.code, 1)
    assert.match(out.stderr + out.stdout, /no agent named/)
    assert.match(out.stderr + out.stdout, /diana/)
  })

  it('refuses to spawn from inside an agent run', async () => {
    const out = await cf(['run', '@diana', 'recurse'], { ...t.env, CONSENSFLOW_CHILD: '1' })
    assert.equal(out.code, 1)
    assert.match(out.stderr + out.stdout, /does not spawn agents/)
  })
})

describe('cf reset is the clean slate, and refuses until you say so', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  const skill = () => join(t.env.CLAUDE_CONFIG_DIR, 'skills', 'consensflow', 'SKILL.md')

  it('prints what it would destroy and touches nothing', async () => {
    stubCli(t, 'claude')
    await cf(['agent', 'add', 'zeus', '--harness', 'claude', '--model', 'claude-opus-5'], t.env)
    await cf(['use', 'claude'], t.env)
    mkdirSync(join(t.env.CONSENSFLOW_HOME, 'workspaces', 'proj', 'runs', 'ask-1'), {
      recursive: true,
    })
    assert.ok(existsSync(skill()))

    const out = await cf(['reset'], t.env)

    // The refusal IS the preview — same two numbers the page puts in its
    // dialog, printed while everything is still there.
    assert.notEqual(out.code, 0, 'a destructive default is not a default')
    assert.match(out.stdout + out.stderr, /1 agent and 1 run/)
    assert.match(out.stdout + out.stderr, /nothing was touched/)
    assert.ok(existsSync(skill()), 'the skill survives a refusal')
    assert.ok(existsSync(rosterPath(t.env)), 'and so does the roster')
  })

  it('removes everything once told, and says what went', async () => {
    const out = await cf(['reset', '--yes'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /1 agent and 1 run went with it/)
    assert.equal(existsSync(skill()), false)
    assert.equal(existsSync(t.env.CONSENSFLOW_HOME), false, 'the whole root is gone')
  })

  it('counts nothing, and still works, on a machine with nothing installed', async () => {
    const out = await cf(['reset', '--yes'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /0 agents and 0 runs/)
  })
})

describe('cf run continues a conversation instead of starting a new one', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  /**
   * A codex that emits a thread id, records the argv it was given, and fails
   * when asked to resume a thread it does not know — the three behaviours the
   * threading path has to cope with.
   */
  function stubCodex(known = [], emits = 'thread-alpha') {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, 'codex-argv.log')
    const path = join(t.env.PATH, 'codex')
    writeFileSync(
      path,
      `#!/bin/sh
echo "$@" >> "${log}"
for a in "$@"; do case "$a" in resume) IS_RESUME=1;; esac; done
if [ -n "$IS_RESUME" ]; then
  WANTED=$3
  case "${known.join(' ')} " in *"$WANTED "*) : ;; *) echo "no such session" >&2; exit 3 ;; esac
fi
echo '{"type":"thread.started","thread_id":"${emits}"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"answered"}}'
`,
    )
    chmodSync(path, 0o755)
    return log
  }

  const argv = (log) => (existsSync(log) ? readFileSync(log, 'utf8') : '')
  const threadsFile = () =>
    join(
      t.env.CONSENSFLOW_HOME,
      'workspaces',
      readdirSync(join(t.env.CONSENSFLOW_HOME, 'workspaces'))[0],
      'threads.json',
    )
  const threads = () => JSON.parse(readFileSync(threadsFile(), 'utf8'))

  /** A consult always comes from some lead session; these tests say which. */
  const asLead = (id) => ({ ...t.env, CLAUDE_CODE_SESSION_ID: id })
  const conversationIn = (text) => /conversation: ([a-z]+-[a-z]+)/.exec(text)?.[1]
  const stripLeads = () => {
    const all = threads()
    for (const row of Object.values(all)) delete row.lead
    writeFileSync(threadsFile(), JSON.stringify(all, null, 2))
  }

  it('names a new conversation on the first run and resumes it on the second', async () => {
    stubCli(t, 'claude')
    const log = stubCodex(['thread-alpha'])
    await cf(['agent', 'add', 'hyperion'], t.env)
    await cf(['use', 'cmux'], t.env)

    const first = await cf(['run', '@hyperion', 'hello', '--thread'], asLead('lead-first'))
    assert.equal(first.code, 0, first.stderr)
    const names = Object.keys(threads())
    assert.equal(names.length, 1, 'one conversation exists')
    assert.match(names[0], /^[a-z]+-[a-z]+$/, 'and it has a sayable name')
    assert.match(first.stdout, new RegExp(names[0]), 'which the run tells you')

    rmSync(log, { force: true })
    const second = await cf(['run', '@hyperion', 'again', '--thread'], asLead('lead-first'))

    assert.equal(second.code, 0, second.stderr)
    assert.match(argv(log), /exec resume thread-alpha/, 'the child was told to continue')
    assert.equal(Object.keys(threads()).length, 1, 'still one conversation, not two')
    assert.equal(threads()[names[0]].runs, 2)
  })

  it('--new starts a second conversation with its own name', async () => {
    const log = stubCodex(['thread-alpha'])
    rmSync(log, { force: true })
    const before = Object.keys(threads())

    const out = await cf(['run', '@hyperion', 'fresh start', '--thread', '--new'], t.env)

    assert.equal(out.code, 0, out.stderr)
    const after = Object.keys(threads())
    assert.equal(after.length, before.length + 1, 'a new conversation, not a reuse')
    assert.doesNotMatch(argv(log), /resume/, 'a new conversation does not resume anything')
  })

  it('--session targets one conversation by name', async () => {
    const log = stubCodex(['thread-alpha'])
    const [first] = Object.keys(threads())
    rmSync(log, { force: true })

    const out = await cf(['run', '@hyperion', 'to you', '--thread', '--session', first], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(argv(log), /exec resume thread-alpha/)
  })

  it('names the conversations you have when asked for one you do not', async () => {
    const out = await cf(['run', '@hyperion', 'hi', '--thread', '--session', 'no-such'], t.env)

    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /no-such/)
  })

  it('--no-thread runs one-shot even in cmux mode', async () => {
    const log = stubCodex(['thread-alpha'])
    rmSync(log, { force: true })

    const out = await cf(['run', '@hyperion', 'once', '--no-thread'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(argv(log), /--ephemeral/, 'one-shot keeps the session-refusing flag')
    assert.doesNotMatch(argv(log), /resume/)
  })

  it('a session the harness has forgotten starts a fresh one instead of failing', async () => {
    // The harness owns the session store; we never touch it. So a pruned or
    // cleared session is normal, and must cost one fresh start, not the run.
    const log = stubCodex([], 'thread-fresh') // knows nothing — every resume exits 3
    const [name] = Object.keys(threads())
    rmSync(log, { force: true })

    const out = await cf(['run', '@hyperion', 'still there?', '--thread', '--session', name], t.env)

    assert.equal(out.code, 0, `a forgotten session must not fail the run: ${out.stderr}`)
    assert.match(argv(log), /exec resume/, 'it tried to resume first')
    assert.match(out.stdout + out.stderr, /new conversation/i, 'and said what it did instead')
    assert.equal(threads()[name].sessionId, 'thread-fresh', 'the stale id was replaced')
  })

  it('a new lead starts its own conversation instead of joining what it finds', async () => {
    // Live 2026-08-24: a brand-new Claude Code session asked hyperion for a
    // joke and it silently became turn 4 of a conversation about something
    // else. The agent's most recent conversation in a directory was
    // everybody's; a conversation belongs to the lead that started it.
    stubCodex(['thread-alpha'])

    const first = await cf(['run', '@hyperion', 'hello', '--thread', '--new'], asLead('lead-A'))
    const mine = conversationIn(first.stdout)
    assert.ok(mine, `the first run names its conversation: ${first.stdout}`)

    const same = await cf(['run', '@hyperion', 'and again', '--thread'], asLead('lead-A'))
    assert.equal(conversationIn(same.stdout), mine, 'the same lead continues its own')

    const stranger = await cf(['run', '@hyperion', 'tell me a joke', '--thread'], asLead('lead-B'))

    assert.equal(stranger.code, 0, stranger.stderr)
    assert.ok(conversationIn(stranger.stdout), 'the new lead names one too')
    assert.notEqual(conversationIn(stranger.stdout), mine, 'but not the one it found here')
  })

  it('a lead it cannot identify starts fresh rather than joining', async () => {
    // Unidentified is nobody, not everybody. Two anonymous shells sharing one
    // conversation is the same bug, for the leads least able to notice it.
    stubCodex(['thread-alpha'])

    const first = await cf(['run', '@hyperion', 'anonymous', '--thread'], t.env)
    const second = await cf(['run', '@hyperion', 'anonymous again', '--thread'], t.env)

    assert.equal(second.code, 0, second.stderr)
    const [before, after] = [conversationIn(first.stdout), conversationIn(second.stdout)]
    assert.ok(before && after, 'both runs name a conversation')
    assert.notEqual(after, before, 'no identity means nothing to continue')
  })

  it('a conversation from before anyone recorded a lead is not inherited', async () => {
    // Rows an older version wrote carry no lead. They stay the user's, and
    // reachable by name — but nobody adopts one by standing next to it.
    stubCodex(['thread-alpha'])
    const orphaned = Object.keys(threads())
    stripLeads()

    const out = await cf(['run', '@hyperion', 'whose is this?', '--thread'], asLead('lead-C'))

    assert.equal(out.code, 0, out.stderr)
    const name = conversationIn(out.stdout)
    assert.ok(name, 'it named one')
    assert.ok(!orphaned.includes(name), 'and it was not one of the unowned ones')
  })

  it('--session reaches another lead conversation, because it was asked for', async () => {
    // Implicit is scoped, explicit is not: naming a conversation is the user
    // saying which one they mean, and no rule of ours overrules that.
    const log = stubCodex(['thread-alpha'])
    const started = await cf(['run', '@hyperion', 'ours', '--thread', '--new'], asLead('lead-D'))
    const name = conversationIn(started.stdout)
    rmSync(log, { force: true })

    const out = await cf(
      ['run', '@hyperion', 'yours', '--thread', '--session', name],
      asLead('lead-E'),
    )

    assert.equal(out.code, 0, out.stderr)
    assert.match(argv(log), /exec resume thread-alpha/, 'it resumed the conversation it was given')
  })

  it('names the conversation on every run, not only when it starts', async () => {
    // `cf last <name>`, `cf catchup <name>` and reusing that conversation's
    // pane all need the name. A lead told it only on run 1 has lost it by run
    // 2, and cannot tell it is continuing anything at all.
    stubCodex(['thread-alpha'])

    const first = await cf(['run', '@hyperion', 'one', '--thread', '--new'], asLead('lead-F'))
    const name = conversationIn(first.stdout)

    const second = await cf(['run', '@hyperion', 'two', '--thread'], asLead('lead-F'))

    assert.equal(second.code, 0, second.stderr)
    assert.match(second.stdout, new RegExp(`conversation: ${name}`), 'it says which one it joined')
    assert.match(second.stdout, /continu/i, 'and that it is continuing, not starting')
  })
})

describe('the lead reads a conversation that happened in another pane', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  function stubCodex() {
    mkdirSync(t.env.PATH, { recursive: true })
    const path = join(t.env.PATH, 'codex')
    writeFileSync(
      path,
      `#!/bin/sh
echo '{"type":"thread.started","thread_id":"thread-alpha"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"the answer you wanted"}}'
`,
    )
    chmodSync(path, 0o755)
  }

  it('lists nothing before anything has been asked', async () => {
    stubCli(t, 'claude')
    stubCodex()
    await cf(['agent', 'add', 'hyperion'], t.env)
    await cf(['use', 'cmux'], t.env)

    const out = await cf(['sessions'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /no conversations/i)
  })

  it('lists each conversation with its agent and run count', async () => {
    await cf(['run', '@hyperion', 'first question', '--thread'], t.env)

    const out = await cf(['sessions'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /hyperion/)
    assert.match(out.stdout, /[a-z]+-[a-z]+/, 'the conversation is named')
    assert.match(out.stdout, /\b1\b/, 'and counted')
  })

  it('cf last prints the answer and where the transcript is', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const [name] = Object.keys(JSON.parse(listed.stdout))

    const out = await cf(['last', name], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /the answer you wanted/)
    assert.match(out.stdout, /transcript\.md/, 'so the lead can read the whole run if it wants')
  })

  it('bare cf last means the newest conversation, like bare attach', async () => {
    // The three read verbs answer "which conversation?" through one rule now;
    // `cf last` was the odd one out, failing on an empty name.
    const out = await cf(['last'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /@hyperion/)
    assert.match(out.stdout, /transcript: /, 'a full answer, with its transcript path')
  })

  it('cf last @agent resolves that agent current conversation', async () => {
    const out = await cf(['last', '@hyperion'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /the answer you wanted/)
  })

  it('cf last --json is machine readable', async () => {
    const out = await cf(['last', '@hyperion', '--json'], t.env)

    assert.equal(out.code, 0, out.stderr)
    const payload = JSON.parse(out.stdout)
    assert.equal(payload.agent, 'hyperion')
    assert.match(payload.output, /the answer you wanted/)
    assert.ok(payload.transcriptPath.endsWith('transcript.md'))
  })

  it('names the conversations you have when asked for one you do not', async () => {
    const out = await cf(['last', 'never-happened'], t.env)

    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /never-happened/)
  })
})

describe('cf chat is the conversation, typed rather than commanded', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  function stubCodex() {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, 'chat-argv.log')
    const path = join(t.env.PATH, 'codex')
    writeFileSync(
      path,
      `#!/bin/sh
echo "$@" >> "${log}"
echo '{"type":"thread.started","thread_id":"thread-chat"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"reply to: '"$(cat)"'"}}'
`,
    )
    chmodSync(path, 0o755)
    return log
  }

  it('answers a line you type and names the conversation', async () => {
    stubCli(t, 'claude')
    stubCodex()
    await cf(['agent', 'add', 'hyperion'], t.env)
    await cf(['use', 'cmux'], t.env)

    const out = await cfTyping(['chat', '@hyperion'], t.env, ['tell me a joke', '/exit'])

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /[a-z]+-[a-z]+/, 'the conversation is named in the header')
    assert.match(out.stdout, /@hyperion/)
    assert.match(out.stdout, /reply to:/, 'the answer came back')
  })

  it('a second line continues the same conversation', async () => {
    const log = stubCodex()
    rmSync(log, { force: true })

    const out = await cfTyping(['chat', '@hyperion'], t.env, ['first', 'second', '/exit'])

    assert.equal(out.code, 0, out.stderr)
    const argv = readFileSync(log, 'utf8')
    assert.match(argv, /exec resume thread-chat/, 'the second turn resumed, not restarted')
    assert.equal((argv.match(/reply|exec/g) ?? []).length >= 2, true, 'two turns ran')
  })

  it('leaves on EOF as well as /exit', async () => {
    stubCodex()

    const out = await cfTyping(['chat', '@hyperion'], t.env, ['just one'])

    assert.equal(out.code, 0, 'closing stdin ends the chat cleanly')
  })

  it('ignores blank lines rather than consulting on nothing', async () => {
    const log = stubCodex()
    rmSync(log, { force: true })

    await cfTyping(['chat', '@hyperion'], t.env, ['', '   ', '/exit'])

    assert.equal(existsSync(log), false, 'an empty line must not spawn an agent')
  })

  it('an agent may not open a chat of its own', async () => {
    const out = await cfTyping(['chat', '@hyperion'], { ...t.env, CONSENSFLOW_CHILD: '1' }, ['hi'])

    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /already an agent run/)
  })
})

describe('cf attach hands the pane to the harness own window', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  function stubHarness(name) {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, `${name}-attach.log`)
    const path = join(t.env.PATH, name)
    writeFileSync(
      path,
      name === 'codex'
        ? `#!/bin/sh
echo "$@" >> "${log}"
if [ "$1" = "exec" ]; then
  echo '{"type":"thread.started","thread_id":"thread-attach"}'
  echo '{"type":"item.completed","item":{"type":"agent_message","text":"hi"}}'
fi
`
        : '#!/bin/sh\nexit 0\n',
    )
    chmodSync(path, 0o755)
    return log
  }

  it('prints the harness own interactive command for a conversation', async () => {
    stubHarness('claude')
    const log = stubHarness('codex')
    await cf(['agent', 'add', 'hyperion'], t.env)
    await cf(['use', 'cmux'], t.env)
    await cf(['run', '@hyperion', 'start it'], t.env)
    rmSync(log, { force: true })

    const out = await cf(['attach', '@hyperion', '--print'], t.env)

    assert.equal(out.code, 0, out.stderr)
    // `codex resume <id>` is the INTERACTIVE side of the same session — not
    // `exec resume`, which is the one-shot we use for a consult.
    assert.match(out.stdout, /codex resume thread-attach/)
    assert.doesNotMatch(out.stdout, /exec resume/)
    assert.equal(existsSync(log), false, '--print runs nothing')
  })

  it('actually hands the terminal over when not printing', async () => {
    const log = stubHarness('codex')
    rmSync(log, { force: true })

    const out = await cf(['attach', '@hyperion'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(readFileSync(log, 'utf8'), /^resume thread-attach/m)
  })

  it('refuses a conversation that has no session yet', async () => {
    const out = await cf(['attach', 'no-such-talk'], t.env)

    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /no-such-talk/)
  })

  it('an agent may not attach to anything', async () => {
    const out = await cf(['attach', '@hyperion'], { ...t.env, CONSENSFLOW_CHILD: '1' })

    assert.notEqual(out.code, 0)
    assert.match(out.stdout + out.stderr, /already an agent run/)
  })
})

describe('a consult can leave the pane open for you to continue in', () => {
  const t = tempEnv()
  after(() => t.cleanup())

  function stubCodex() {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, 'then-attach.log')
    const path = join(t.env.PATH, 'codex')
    writeFileSync(
      path,
      `#!/bin/sh
echo "$@" >> "${log}"
if [ "$1" = "exec" ]; then
  echo '{"type":"thread.started","thread_id":"thread-open"}'
  echo '{"type":"item.completed","item":{"type":"agent_message","text":"the consult answer"}}'
fi
`,
    )
    chmodSync(path, 0o755)
    return log
  }

  it('answers the lead first, then hands the terminal over', async () => {
    stubCli(t, 'claude')
    const log = stubCodex()
    await cf(['agent', 'add', 'hyperion'], t.env)
    await cf(['use', 'cmux'], t.env)

    const out = await cf(['run', '@hyperion', 'a question', '--attach'], t.env)

    assert.equal(out.code, 0, out.stderr)
    // The lead still gets a real, parsed answer — not a screen to read.
    assert.match(out.stdout, /the consult answer/)
    const argv = readFileSync(log, 'utf8')
    assert.match(argv, /^exec /m, 'the consult ran one-shot')
    assert.match(argv, /^resume thread-open/m, 'then the interactive window opened')
  })

  it('records the turn before handing over, so cf last works after', async () => {
    const out = await cf(['sessions', '--json'], t.env)
    const [name] = Object.keys(JSON.parse(out.stdout))

    const last = await cf(['last', name], t.env)

    assert.equal(last.code, 0, last.stderr)
    assert.match(last.stdout, /the consult answer/)
  })

  it('is a no-op when there is nothing to attach to', async () => {
    // A one-shot with threading off has no session, so --attach has no window
    // to open. It must not fail the consult over it.
    const out = await cf(['run', '@hyperion', 'quick one', '--no-thread', '--attach'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /the consult answer/)
  })
})
