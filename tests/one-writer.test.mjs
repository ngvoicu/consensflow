import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { stageBundle } from './bundle.mjs'
import { chooseHome, startLine } from './choice.mjs'
import { tempEnv, windowsEnv } from './helpers.mjs'

/**
 * One writer for each file of a home: the implementation the home chooses, which
 * is the one the `use-node` file says (`cf_base::way_back`, `src/use-node.js`), the
 * native one without it, and the same for the daemon, a `cf` verb and `cf ui`.
 * Which one wrote is told by what ran, not by the file changing, which either
 * would change alike (`goldens:cli` holds their bytes equal): a Node process that
 * loaded `src/cli.js` is Node's write, and a verb with no Node process at all is
 * Rust's. The bundle is laid out as the app's is (tests/bundle.mjs), since the
 * native `cf` finds the Node of its own bundle for the way back.
 */

const SPY = fileURLToPath(new URL('./fixtures/module-spy.mjs', import.meta.url))
const NODE_SPY = fileURLToPath(new URL('./fixtures/node-spy.mjs', import.meta.url))

let bundle
before(() => {
  bundle = stageBundle()
})
after(() => bundle.cleanup())

/** A home of this test's own, the implementation it chooses made in it. */
function home(kind) {
  const t = tempEnv()
  chooseHome(kind, t.env.CONSENSFLOW_HOME)
  return t
}

/** The environment of a daemon: this machine's, but for what the test's home gives. */
function daemonEnv(t) {
  return {
    ...Object.fromEntries(
      Object.entries(process.env).filter(
        ([name]) => !/^(CONSENSFLOW_|CF_)/.test(name) && name.toUpperCase() !== 'PATH',
      ),
    ),
    ...t.env,
  }
}

/**
 * Runs the native `cf` of the bundle with `args` (a terminal's `cf`, as the launcher
 * runs it), and says which implementation wrote: the Node processes that started
 * and the modules of Node's CLI that loaded.
 */
function terminalCf(args, t) {
  const spy = join(t.root, 'spied')
  const options = [SPY, NODE_SPY].map((file) => `--import=${pathToFileURL(file).href}`).join(' ')
  const ran = spawnSync(bundle.cf, args, {
    encoding: 'utf8',
    env: { ...t.env, ...windowsEnv(), NODE_OPTIONS: options, CF_TEST_SPY: spy },
  })
  const spied = existsSync(spy) ? readFileSync(spy, 'utf8').split('\n').filter(Boolean) : []
  return {
    ...ran,
    // `spied` holds `<pid>\t<script>` for a Node process, and the module's name for a load.
    nodeProcesses: spied.filter((line) => line.includes('\t')).length,
    cliLoaded: spied.includes('cli'),
  }
}

describe('one writer for each file of a home', () => {
  it('writes a roster through Node with the file in the home, and through Rust without it', () => {
    for (const kind of ['node', 'native']) {
      const t = home(kind)
      try {
        const ran = terminalCf(
          ['agent', 'add', 'mine', '--harness', 'claude', '--model', 'a-model'],
          t,
        )
        assert.equal(ran.status, 0, ran.stderr)
        assert.equal(ran.stdout, 'mine  claude  a-model\n')
        const file = readFileSync(join(t.env.CONSENSFLOW_HOME, 'agents.json'), 'utf8')
        assert.match(file, /"id": "mine"/)
        // Who wrote it: Node's CLI ran in a Node process, or no Node process ran at all.
        assert.deepEqual(
          [ran.cliLoaded, ran.nodeProcesses > 0],
          kind === 'node' ? [true, true] : [false, false],
          kind,
        )
      } finally {
        t.cleanup()
      }
    }
  })

  it('has the daemon and the verbs of a home the same implementation, by whichever way the daemon is started', async () => {
    for (const kind of ['node', 'native']) {
      const t = home(kind)
      try {
        // The app starts what the home chooses (`daemon_command.rs`): Node's
        // runtime on cf.mjs for the file, the native cf without it. A terminal's
        // `cf ui` is the native cf, which chooses by the same file.
        const apps = kind === 'node' ? [bundle.node, [bundle.cfMjs]] : [bundle.cf, []]
        const terminals = [bundle.cf, []]
        for (const [label, [command, head]] of [
          ['the app', apps],
          ['a terminal', terminals],
        ]) {
          const daemon = await started(command, [...head, 'ui', '--json', '--no-open'], t)
          assert.equal(daemon.kind, kind, `${label}: the daemon of a ${kind} home`)
        }
        // And the verbs of that home are that implementation's too.
        const verb = terminalCf(['agent', 'list'], t)
        assert.deepEqual(
          [verb.cliLoaded, verb.nodeProcesses > 0],
          kind === 'node' ? [true, true] : [false, false],
          `the verbs of a ${kind} home`,
        )
      } finally {
        t.cleanup()
      }
    }
  })
})

/** Starts `command` as a daemon in `t`'s home, says which daemon its log names, and stops it. */
async function started(command, args, t) {
  const child = spawn(command, args, {
    env: daemonEnv(t),
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const exited = new Promise((resolve) => child.once('exit', resolve))
  let errors = ''
  child.stderr.on('data', (chunk) => {
    errors += chunk
  })
  try {
    await new Promise((resolve, reject) => {
      let said = ''
      child.stdout.on('data', (chunk) => {
        said += chunk
        if (said.includes('\n')) resolve()
      })
      child.once('exit', () => reject(new Error(`the daemon ended before it was ready: ${errors}`)))
    })
    // The home's last start line is this daemon's: each is stopped before the next starts
    // (and on Windows `cf` runs the Node it hands a command to as a child, so its pid is not ours).
    const log = readFileSync(join(t.env.CONSENSFLOW_HOME, 'daemon.log'), 'utf8')
    const start = startLine(log)
    assert.notEqual(start, null, `its log holds no start line: ${log}`)
    return start
  } finally {
    child.stdin.end()
    const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
    await exited
    clearTimeout(stuck)
  }
}
