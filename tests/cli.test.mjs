import assert from 'node:assert/strict'
import { execFile, spawn } from 'node:child_process'
import {
  appendFileSync,
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
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

async function cfIn(cwd, args, env) {
  try {
    const { stdout, stderr } = await run(process.execPath, [CF, ...args], {
      env,
      cwd,
      timeout: 30_000,
    })
    return { code: 0, stdout, stderr }
  } catch (cause) {
    return { code: cause.code ?? 1, stdout: cause.stdout ?? '', stderr: cause.stderr ?? '' }
  }
}

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

  it('cf mint names a conversation before it exists, so the lead can plan around it', async () => {
    // The lead composing a pane command needs the name for the tab title and
    // the read-back BEFORE the run prints it in a pane it cannot read.
    const minted = await cf(['mint'], t.env)
    assert.equal(minted.code, 0, minted.stderr)
    const name = minted.stdout.trim()
    assert.match(name, /^[a-z]+-[a-z]+$/, 'a sayable vocabulary name')

    stubCodex(['thread-alpha'])
    const run = await cf(
      ['run', '@hyperion', 'planned', '--thread', '--new', '--session', name],
      asLead('lead-M'),
    )

    assert.equal(run.code, 0, run.stderr)
    assert.match(run.stdout, new RegExp(`conversation: ${name} \\(new\\)`))
    assert.ok(threads()[name], 'created under exactly that name')
  })

  it('--new --session refuses an agent name, a taken name, and a shell-hostile one', async () => {
    const taken = Object.keys(threads())[0]
    for (const [bad, why] of [
      ['hyperion', /agent's name/],
      [taken, /already exists/],
      ['Has Spaces', /lowercase words and hyphens/],
    ]) {
      const out = await cf(['run', '@hyperion', 'x', '--thread', '--new', '--session', bad], t.env)
      assert.notEqual(out.code, 0, `${bad} must be refused`)
      assert.match(out.stdout + out.stderr, why)
    }
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

describe('in a terminal, a cmux consult IS the agent own window', () => {
  const t = tempEnv()
  after(() => t.cleanup())
  // CONSENSFLOW_TTY stands in for a real terminal: pipes are all a test has.
  const tty = (extra = {}) => ({
    ...t.env,
    CONSENSFLOW_TTY: '1',
    CLAUDE_CODE_SESSION_ID: 'lead-w',
    ...extra,
  })
  const argvOf = (log) => (existsSync(log) ? readFileSync(log, 'utf8') : '')
  const threadRows = () =>
    JSON.parse(
      readFileSync(
        join(
          t.env.CONSENSFLOW_HOME,
          'workspaces',
          readdirSync(join(t.env.CONSENSFLOW_HOME, 'workspaces'))[0],
          'threads.json',
        ),
        'utf8',
      ),
    )

  function recordingStub(name) {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, `${name}-window.log`)
    writeFileSync(join(t.env.PATH, name), `#!/bin/sh\necho "$@" >> "${log}"\n`)
    chmodSync(join(t.env.PATH, name), 0o755)
    return log
  }

  const COLD_CODEX_ID = '01a03068-3530-7173-a123-009f15591007'

  /** A kimi that streams a prompt-mode answer and hands its id back at the end. */
  function stubKimiWindow() {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, 'kimi-window.log')
    const path = join(t.env.PATH, 'kimi')
    writeFileSync(
      path,
      `#!/bin/sh
echo "$@" >> "${log}"
if [ "$1" = "-p" ]; then
  echo '{"role":"assistant","content":"the consult answer"}'
  echo '{"role":"meta","type":"session.resume_hint","session_id":"session-open"}'
fi
`,
    )
    chmodSync(path, 0o755)
    return log
  }

  /** A codex whose interactive form writes the rollout its id is read from. */
  function stubCodexCold() {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, 'codex-cold.log')
    const dir = join(t.env.CODEX_HOME, 'sessions', '2026', '08', '24')
    mkdirSync(dir, { recursive: true })
    const rollout = join(dir, `rollout-2026-08-24T00-00-00-${COLD_CODEX_ID}.jsonl`)
    const path = join(t.env.PATH, 'codex')
    writeFileSync(
      path,
      `#!/bin/sh
echo "$@" >> "${log}"
printf '%s\\n' '{"type":"session_meta","payload":{"cwd":"'"$PWD"'"}}' > "${rollout}"
`,
    )
    chmodSync(path, 0o755)
    return log
  }

  function stubCodexWindow() {
    mkdirSync(t.env.PATH, { recursive: true })
    const log = join(t.root, 'codex-window.log')
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

  it('kimi streams its first answer, then the pane becomes its window', async () => {
    // kimi is the one harness that cannot be seeded: `-p` is non-interactive
    // and it has no positional prompt. So turn 1 runs through the one-shot
    // machinery — which captures its id — and the window opens on that. The
    // lead still gets a parsed answer, never a screen.
    stubCli(t, 'claude')
    recordingStub('pi')
    recordingStub('opencode')
    const log = stubKimiWindow()
    await cf(['agent', 'add', 'ilmarinen'], t.env)
    await cf(['agent', 'add', 'hyperion'], t.env)
    await cf(['agent', 'add', 'zeus'], t.env)
    await cf(['agent', 'add', 'aether'], t.env)
    await cf(['agent', 'add', 'sunna'], t.env)
    await cf(['use', 'cmux'], t.env)

    const out = await cf(['run', '@ilmarinen', 'a question'], tty())

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /the consult answer/)
    const argv = argvOf(log)
    assert.match(argv, /^-p /m, 'turn 1 streamed')
    assert.match(argv, /^-S session-open/m, 'then the window opened on the captured id')
    assert.match(out.stdout, /kimi streams its first answer/, 'and it names the right harness')
  })

  it('a kimi run that dies before printing its id is still resumable', async () => {
    // Live 2026-08-24: a 24-minute rebuild hit the provider's rate limit on
    // its last step. kimi prints its id LAST, so the conversation was left
    // unresumable with all its work on disk. The store still knew.
    mkdirSync(t.env.PATH, { recursive: true })
    const sessions = join(t.env.HOME, '.kimi-code', 'sessions', 'wd_x', 'session_rescued')
    mkdirSync(sessions, { recursive: true })
    const path = join(t.env.PATH, 'kimi')
    writeFileSync(
      path,
      `#!/bin/sh
printf '%s' '{"id":"session_rescued","cwd":"'"$PWD"'","createdAt":99999999999999}' > "${join(sessions, 'state.json')}"
echo '{"role":"assistant","content":"got some of it done"}'
exit 1
`,
    )
    chmodSync(path, 0o755)

    const out = await cf(
      ['run', '@ilmarinen', 'a long job', '--thread', '--new'],
      asReader('lead-k'),
    )

    // `--new` means this is its own conversation; take the one it just named.
    const name = /conversation: ([a-z]+-[a-z]+)/.exec(out.stdout)?.[1]
    assert.ok(name, `the run named its conversation: ${out.stdout}`)
    assert.equal(threadRows()[name].sessionId, 'session_rescued', 'recovered from kimi own store')
  })

  it('codex opens its own window cold, and finds the id in its own store', async () => {
    // `codex [PROMPT]` opens the real window seeded with the packet. It
    // announces no id the way `exec --json` does, so the id is read back from
    // the rollout file it writes on the way up.
    const log = stubCodexCold()

    const out = await cf(['run', '@hyperion', 'review this'], tty())

    assert.equal(out.code, 0, out.stderr)
    const argv = argvOf(log)
    assert.doesNotMatch(argv, /^exec/m, 'nothing streamed — the window IS turn one')
    assert.match(argv, /review this/, 'seeded with the task')
    const row = Object.values(threadRows()).find((r) => r.agent === 'hyperion')
    assert.equal(row.sessionId, COLD_CODEX_ID, 'discovered from codex own rollout store')
  })

  it('records the turn before handing over, so cf last works after', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'ilmarinen',
    )[0]

    const last = await cf(['last', name], t.env)

    assert.equal(last.code, 0, last.stderr)
    assert.match(last.stdout, /the consult answer/)
  })

  it('claude opens fresh as its own window, on a uuid we minted and saved first', async () => {
    const log = recordingStub('claude')

    const out = await cf(['run', '@zeus', 'review the retry path'], tty())

    assert.equal(out.code, 0, out.stderr)
    const argv = argvOf(log)
    const uuid = /--session-id ([0-9a-f-]{36})/.exec(argv)?.[1]
    assert.ok(uuid, `claude was given a minted session id: ${argv}`)
    assert.match(argv, /review the retry path/, 'seeded with the task')
    const row = Object.values(threadRows()).find((r) => r.agent === 'zeus')
    assert.equal(row.sessionId, uuid, 'saved BEFORE the window, so a crash still resumes')
    assert.equal(row.runs, 0, 'window turns are not runs of ours')
    assert.match(out.stdout, /cf catchup/, 'and the lead is told how to read it')
  })

  it('the same lead follow-up resumes the window, seeded with only the new message', async () => {
    const log = recordingStub('claude')
    rmSync(log, { force: true })

    const out = await cf(['run', '@zeus', 'and the timeout?'], tty())

    assert.equal(out.code, 0, out.stderr)
    const argv = argvOf(log)
    assert.match(argv, /--resume /, 'the same session, resumed')
    assert.match(argv, /and the timeout\?/)
    assert.doesNotMatch(argv, /How to work/, 'no scene-setting on a follow-up')
  })

  it('pi opens fresh on the conversation own name', async () => {
    const log = recordingStub('pi')

    const out = await cf(['run', '@aether', 'hello'], tty())

    assert.equal(out.code, 0, out.stderr)
    const row = Object.entries(threadRows()).find(([, r]) => r.agent === 'aether')
    assert.match(argvOf(log), new RegExp(`--session-id ${row[0]}`), 'the name IS the id')
    assert.equal(row[1].sessionId, row[0])
  })

  it('opencode opens without an id, and the store tells us which one it minted', async () => {
    mkdirSync(t.env.PATH, { recursive: true })
    const dataHome = join(t.root, 'data')
    const store = join(dataHome, 'opencode', 'storage', 'session', 'proj')
    const path = join(t.env.PATH, 'opencode')
    // The stub plays the TUI: it mints its own session file, like the real one.
    // The stub's PATH is the stub dir alone, so external commands are out:
    // the store dir is made here, and the timestamp is any moment after the
    // spawn — the discovery only asks "born since?", so far-future is fine.
    mkdirSync(store, { recursive: true })
    writeFileSync(
      path,
      `#!/bin/sh
printf '{"id":"ses_window1","directory":"%s","time":{"created":99999999999999}}' "$PWD" > "${store}/ses_window1.json"
`,
    )
    chmodSync(path, 0o755)

    const out = await cf(['run', '@sunna', 'hello'], tty({ XDG_DATA_HOME: dataHome }))

    assert.equal(out.code, 0, out.stderr)
    const row = Object.values(threadRows()).find((r) => r.agent === 'sunna')
    assert.equal(row.sessionId, 'ses_window1', 'discovered from the store after launch')
  })

  it('a pipe cannot host a TUI: without a terminal the consult streams as before', async () => {
    const log = stubCodexWindow()
    rmSync(log, { force: true })

    const out = await cf(['run', '@hyperion', 'again', '--new'], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /the consult answer/)
    assert.doesNotMatch(argvOf(log), /^resume/m, 'no window opened for a pipe')
  })

  it('cf attach --print carries the billing guard as prose', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion',
    )[0]

    const out = await cf(['attach', name, '--print'], t.env)

    assert.equal(out.code, 0, out.stderr)
    // The printed line runs in someone else's shell, where our spawn-time
    // guard cannot reach — the guard has to travel inside the command.
    assert.match(out.stdout, /env -u OPENAI_API_KEY codex resume/)
  })

  const catchupWait = (name, extraEnv = {}) => {
    const child = spawn(process.execPath, [CF, 'catchup', name, '--wait'], {
      env: { ...t.env, CONSENSFLOW_WAIT_GRACE_MS: '600', ...extraEnv },
    })
    let stdout = ''
    child.stdout.on('data', (chunk) => {
      stdout += chunk
    })
    const guard = setTimeout(() => child.kill(), 20_000)
    guard.unref()
    return new Promise((resolve) =>
      child.on('close', (code) => {
        clearTimeout(guard)
        resolve({ code, stdout })
      }),
    )
  }

  /** A lead reading a conversation — its own identity, its own read mark. */
  const asReader = (id) => ({ ...t.env, CLAUDE_CODE_SESSION_ID: id })

  /** Plant a conversation row, for the states a run cannot reach on its own. */
  const writeThreadRow = (name, row) => {
    const file = join(
      t.env.CONSENSFLOW_HOME,
      'workspaces',
      readdirSync(join(t.env.CONSENSFLOW_HOME, 'workspaces'))[0],
      'threads.json',
    )
    const all = threadRows()
    all[name] = { createdAt: '2026-08-24T00:00:00.000Z', lastRunAt: null, lastRunId: null, ...row }
    writeFileSync(file, JSON.stringify(all, null, 2))
  }

  const rolloutFor = (id) => {
    const dir = join(t.env.CODEX_HOME, 'sessions', '2026', '08', '24')
    mkdirSync(dir, { recursive: true })
    return join(dir, `rollout-2026-08-24T00-00-00-${id}.jsonl`)
  }
  const turnLine = (role, text) =>
    `${JSON.stringify({ type: 'response_item', payload: { role, content: [{ text }] } })}\n`

  it('cf catchup --wait sits out an answer still being written', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]
    const file = rolloutFor('thread-open')
    writeFileSync(file, turnLine('user', 'a question'))
    setTimeout(() => appendFileSync(file, turnLine('assistant', 'the late answer')), 500).unref()

    const { code, stdout } = await catchupWait(name)

    assert.equal(code, 0, stdout)
    assert.match(stdout, /the late answer/)
    assert.match(stdout, /a question/, 'the question stays visible above its answer')
  })

  it('cf catchup --wait returns an answer that already landed, instead of hanging', async () => {
    // The race that bit live: a fast agent answered BEFORE --wait started, so
    // a baseline of "everything so far" contained the answer and --wait sat
    // out its whole timeout while the window plainly showed it.
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]
    writeFileSync(
      rolloutFor('thread-open'),
      turnLine('user', 'a quick question') + turnLine('assistant', 'already answered'),
    )

    const started = Date.now()
    const { code, stdout } = await catchupWait(name)

    assert.equal(code, 0, stdout)
    assert.match(stdout, /already answered/)
    assert.ok(Date.now() - started < 10_000, 'the standing answer returns within the grace')
  })

  it('cf catchup --wait prefers a question that lands during the grace over the stale answer', async () => {
    // The mirror-image race: the lead chains send-and-wait in one breath, so
    // --wait can start before the just-sent question reaches the store.
    // Returning the standing answer immediately would hand back the PREVIOUS
    // answer as if it were new.
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]
    const file = rolloutFor('thread-open')
    writeFileSync(file, turnLine('user', 'old question') + turnLine('assistant', 'old answer'))
    setTimeout(() => appendFileSync(file, turnLine('user', 'new question')), 200).unref()
    setTimeout(() => appendFileSync(file, turnLine('assistant', 'the new answer')), 900).unref()

    const { code, stdout } = await catchupWait(name, { CONSENSFLOW_WAIT_GRACE_MS: '2000' })

    assert.equal(code, 0, stdout)
    assert.match(stdout, /the new answer/)
    assert.doesNotMatch(stdout, /old answer/, 'the stale exchange is not what was asked for')
  })

  it('a long consult is findable while it runs, not only after it answers', async () => {
    // Live 2026-08-24: a site rebuild ran for ten minutes and `cf sessions`
    // showed nothing while `cf catchup <name>` said there was no such
    // conversation — for exactly the stretch the lead most wanted to follow.
    mkdirSync(t.env.PATH, { recursive: true })
    const slow = join(t.env.PATH, 'codex')
    const marker = join(t.root, 'slow-started')
    writeFileSync(
      slow,
      `#!/bin/sh
echo started > "${marker}"
while [ ! -f "${join(t.root, 'slow-release')}" ]; do :; done
echo '{"type":"thread.started","thread_id":"thread-slow"}'
echo '{"type":"item.completed","item":{"type":"agent_message","text":"eventually"}}'
`,
    )
    chmodSync(slow, 0o755)

    const run = cf(['run', '@hyperion', 'a long task', '--thread', '--new'], asReader('lead-slow'))
    // Wait for the child to be underway, then look: the conversation must
    // already be there.
    for (let i = 0; i < 200 && !existsSync(marker); i += 1) {
      await new Promise((resolve) => setTimeout(resolve, 25))
    }
    const listed = await cf(['sessions'], t.env)
    assert.match(listed.stdout, /working since/, 'sessions says which one is still working')
    const running = Object.entries(threadRows()).find(([, r]) => r.startedAt !== undefined)
    assert.ok(running, 'the row exists mid-run')
    assert.equal(running[1].sessionId, null, 'with no session id yet — it has not been given one')

    writeFileSync(join(t.root, 'slow-release'), '')
    const out = await run
    assert.equal(out.code, 0, out.stderr)
    assert.equal(threadRows()[running[0]].startedAt, undefined, 'and stops being marked running')
    assert.equal(threadRows()[running[0]].sessionId, 'thread-slow')
  })

  it('cf catchup --unread is only what was said since this lead last looked', async () => {
    // Live 2026-08-24: asked "can you see what other jokes he said?", the lead
    // had no way to ask for what was NEW, so it sent another request and
    // invented a third round instead of reading the second.
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]
    const file = rolloutFor('thread-open')
    writeFileSync(file, turnLine('user', 'round one?') + turnLine('assistant', 'first answer'))

    const first = await cf(['catchup', name], asReader('reader-1'))
    assert.equal(first.code, 0, first.stderr)
    assert.match(first.stdout, /first answer/)

    // The user then talks to the agent directly in its pane.
    appendFileSync(file, turnLine('user', 'more?') + turnLine('assistant', 'second answer'))

    const unread = await cf(['catchup', name, '--unread'], asReader('reader-1'))

    assert.equal(unread.code, 0, unread.stderr)
    assert.match(unread.stdout, /second answer/)
    assert.doesNotMatch(unread.stdout, /first answer/, 'what it already read is not new')
    assert.match(unread.stdout, /2 new turns/)

    // Reading is seeing: asking again shows nothing new.
    const again = await cf(['catchup', name, '--unread'], asReader('reader-1'))
    assert.match(again.stdout, /nothing new since you last looked/)
  })

  it('a full cf catchup marks where this lead memory stopped', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]
    appendFileSync(
      rolloutFor('thread-open'),
      turnLine('user', 'and again?') + turnLine('assistant', 'third answer'),
    )

    const out = await cf(['catchup', name], asReader('reader-1'))

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /first answer/, 'the whole conversation is still there')
    assert.match(out.stdout, /you had not seen/, 'with a line where its memory stopped')
  })

  it('another lead reads the same conversation with its own mark', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]

    const other = await cf(['catchup', name, '--unread'], asReader('reader-2'))

    assert.equal(other.code, 0, other.stderr)
    assert.match(other.stdout, /first answer/, 'a lead that never looked has read nothing')
  })

  it('a read mark survives the next run — writers keep fields they do not own', async () => {
    // recordTurn and saveWindowRow used to rebuild rows from a literal, which
    // silently wiped the marks the moment a consult followed a catchup.
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(
      ([, r]) => r.agent === 'hyperion' && r.sessionId === 'thread-open',
    )[0]
    const before = threadRows()[name].seen
    assert.ok(before && Object.keys(before).length > 0, 'fixture: marks exist')

    stubCodexWindow()
    await cf(['run', '@hyperion', 'a consult', '--thread', '--session', name], asReader('reader-1'))

    assert.deepEqual(threadRows()[name].seen, before, 'the marks are still there')
  })

  it('cf last on a window-only conversation points at catchup instead of failing', async () => {
    const listed = await cf(['sessions', '--json'], t.env)
    const name = Object.entries(JSON.parse(listed.stdout)).find(([, r]) => r.agent === 'zeus')[0]

    const out = await cf(['last', name], t.env)

    assert.equal(out.code, 0, out.stderr)
    assert.match(out.stdout, /cf catchup/)
  })
})
