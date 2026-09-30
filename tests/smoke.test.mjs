import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

/**
 * The packaged smoke: the REAL `.app`, not this checkout.
 *
 * Every other suite reaches into `src/` and `app/src-tauri/`. This one is the
 * only place that asks whether the thing Gabriel double-clicks works — the
 * bundle's own page, the bundle's own Node, the bundle's own CLI copy, the
 * production Tauri commands, and a real PTY child. So it resolves NOTHING
 * from the repository except this file, and every path it asserts on has to
 * live under `Contents/`.
 *
 * It is gated, not skipped-by-default-forever: without `CONSENSFLOW_SMOKE`
 * the tests skip so `npm test` stays a unit run, and WITH it a missing
 * bundle is a failure that names the build command. A smoke that quietly
 * passes because there was nothing to test is worse than no smoke.
 */

const REPO = dirname(dirname(fileURLToPath(import.meta.url)))
const REQUESTED = process.env.CONSENSFLOW_SMOKE === '1'
const BUNDLE_HINT = 'npm --prefix app run build -- --bundles app'
const HANDSHAKE_MS = Number(process.env.CONSENSFLOW_SMOKE_TIMEOUT_MS ?? 180_000)
const FLOOD_LINES = 4096
const FLOOD_WIDTH = 384

/**
 * What the flood is by CONSTRUCTION — not what xterm still holds.
 *
 * These lines wrap far past the emulator's 10 000-row scrollback, so the rows
 * on screen can never add up to the bytes sent. The size that matters is this
 * one: it is over the 1 MiB unacked-output window, so the LAST line can only
 * arrive if the page kept returning credit.
 */
const FLOOD_BYTES = FLOOD_LINES * (FLOOD_WIDTH + 'CFSMOKE-FLOOD 1234 '.length + 1)

/** The app writes its error output to <home>/app/app.log, not to its stderr. */
function appLog(box) {
  try {
    return readFileSync(join(box.env.CONSENSFLOW_HOME, 'app', 'app.log'), 'utf8').slice(-4000)
  } catch {
    return '(none)'
  }
}

function candidateApp() {
  const override = process.env.CONSENSFLOW_SMOKE_APP
  if (typeof override === 'string' && override.length > 0) return resolve(override)
  return join(REPO, 'app', 'src-tauri', 'target', 'release', 'bundle', 'macos', 'ConsensFlow.app')
}

/**
 * The bundle to test, or the reason there is none.
 *
 * `/Applications` is never a candidate: the installed app is whatever Gabriel
 * last installed, and a smoke that passes against it says nothing about the
 * source in this tree.
 */
function locateApp() {
  const app = candidateApp()
  if (!existsSync(app)) {
    return { app: null, why: `no built bundle at ${app} — build it with \`${BUNDLE_HINT}\`` }
  }
  // The executable is whatever the bundle SAYS it is. Tauri names the main
  // binary after the crate (`app`), not after `productName`, so guessing
  // `ConsensFlow` here would report a missing bundle for a bundle that is
  // sitting right there — which is how this check first went wrong.
  let executable
  try {
    executable = execFileSync(
      '/usr/bin/plutil',
      ['-extract', 'CFBundleExecutable', 'raw', '-o', '-', join(app, 'Contents', 'Info.plist')],
      { encoding: 'utf8' },
    ).trim()
  } catch (cause) {
    return { app: null, why: `the bundle at ${app} has no readable Info.plist: ${cause.message}` }
  }
  const binary = join(app, 'Contents', 'MacOS', executable)
  // `externalBin` sidecars land beside the executable; older layouts staged
  // them under Resources. Take whichever this bundle actually has.
  const sidecar = join(app, 'Contents', 'MacOS', 'node')
  const staged = join(app, 'Contents', 'Resources', 'binaries', 'node')
  const node = existsSync(sidecar) ? sidecar : staged
  const cli = join(app, 'Contents', 'Resources', 'cli', 'bin', 'cf.mjs')
  for (const [what, path] of [
    ['executable', binary],
    ['bundled node', node],
    ['bundled CLI', cli],
  ]) {
    if (!existsSync(path)) {
      return {
        app: null,
        why: `the bundle at ${app} has no ${what} (${path}) — rebuild with \`${BUNDLE_HINT}\``,
      }
    }
  }
  return { app, binary, node, cli, why: null }
}

/** Skips only when nobody asked for the smoke; a requested one never skips. */
function gate(t) {
  if (!REQUESTED) {
    t.skip('packaged smoke runs under `npm run smoke` (sets CONSENSFLOW_SMOKE=1)')
    return null
  }
  const found = locateApp()
  assert.equal(found.why, null, found.why ?? '')
  return found
}

const FAKE_HARNESS = String.raw`#!/bin/sh
# The smoke's stand-in harness. It exists to be recognisable on screen, to
# prove that what the human types reaches a real child — every line it reads
# comes back as hex, which no echo, no replay and no cached frame could
# produce — and, on request, to out-run the output window.
#
# It also keeps the two records a Claude window keeps, because the core reads
# them before it delivers and after: sessions/<pid>.json says the window is
# idle, and the transcript holds every line the window took as a user turn
# answered by an assistant turn. The human's Enter releases the typing latch
# on the first; a delivery from the board is confirmed by the second.
#
# The flood is asked for rather than printed at start-up: 1.5 MiB of wrapped
# lines pushes far more rows than xterm keeps, so a banner printed before it
# is gone by the time anything can look for it. The page says when it has
# seen the banner; only then does the flood run.
LC_ALL=C
export LC_ALL
echo $$ > "$CFSMOKE_PIDFILE"
session=''
seed=''
while [ $# -gt 0 ]; do
  case "$1" in
    --session-id|--resume) session="$2"; shift ;;
    # A worker's window opens with its brief as the last argument.
    '[ConsensFlow'*) seed="$1" ;;
  esac
  shift
done
config="${'$'}{CLAUDE_CONFIG_DIR:-$HOME/.claude}"
transcript="$config/projects/smoke/$session.jsonl"
status="$config/sessions/$$.json"
mkdir -p "$config/projects/smoke" "$config/sessions"
trap 'rm -f "$status"' EXIT
n=0
stamp() { date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo 1970-01-01T00:00:00Z; }
state() {
  printf '{"pid":%s,"sessionId":"%s","kind":"interactive","status":"%s"}' "$$" "$session" "$1" > "$status"
}
record() {
  n=$((n + 1))
  printf '{"sessionId":"%s","version":"2.1.277","timestamp":"%s","uuid":"%s-%s-%s",%s}\n' \
    "$session" "$(stamp)" "$session" "$$" "$n" "$1" >> "$transcript"
}
# One line read is one turn, the way the integration suite's fake agent does it.
turn() {
  state busy
  record "\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"$1\"}"
  record "\"type\":\"assistant\",\"message\":{\"id\":\"$session-message-$$-$n\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"noted\"}],\"stop_reason\":\"end_turn\"}"
  record "\"type\":\"system\",\"subtype\":\"stop_hook_summary\",\"preventedContinuation\":false,\"hookCount\":1"
  state idle
}
state idle
printf 'CFSMOKE-READY %s\n' "$CFSMOKE_TAG"
# Says whether the pane inherited a usable PATH. A chief whose PATH holds only
# ConsensFlow's own directories cannot run git, ripgrep or any of what a real
# harness shells out to, and every test that stubs the environment would still
# pass. One system command settles it.
if command -v uname >/dev/null 2>&1; then
  printf 'CFSMOKE-TOOLS ok\n'
else
  printf 'CFSMOKE-TOOLS missing\n'
fi
# A worker's first turn is its brief, answered at once: its header line is
# the record the core looks for, and "noted" is its result.
if [ -n "$seed" ]; then
  nl='
'
  turn "${'$'}{seed%%"$nl"*}"
fi
pad=''
n=0
while [ $n -lt ${FLOOD_WIDTH} ]; do
  pad="${'$'}{pad}x"
  n=$((n + 1))
done
n=0
esc=$(printf '\033')
while IFS= read -r line; do
  if [ "$line" = "BIGPASTE" ]; then
    saved=$(stty -g)
    stty raw -echo
    "$CFSMOKE_PASTE_NODE" "$CFSMOKE_PASTE_READER"
    stty "$saved"
  elif [ "$line" = "HANDOFF" ]; then
    # The chief puts a task on the board, the way a real chief does; a worker
    # window on this same stand-in does it, and the core delivers its result
    # into this window.
    cf task add --tier standard "SMOKE BRIEF"
    turn "HANDOFF"
  elif [ "$line" = "FLOOD" ]; then
    n=1
    while [ $n -le ${FLOOD_LINES} ]; do
      printf 'CFSMOKE-FLOOD %s %s\n' "$n" "$pad"
      n=$((n + 1))
    done
    printf 'CFSMOKE-FLOODED %s\n' "$CFSMOKE_TAG"
  else
    # A paste arrives bracketed; the record and the hex are of the text.
    line=${'$'}{line#"$esc[200~"}
    line=${'$'}{line%"$esc[201~"}
    # Shell builtins only, on purpose. A chief pane's PATH once carried just
    # ConsensFlow's own bin directories — this fixture is what found that,
    # by failing on a missing \`od\` — and it is fixed now. Keeping the hex in
    # the shell means this test measures the app, not the machine's coreutils.
    hex=''
    json=''
    rest=$line
    while [ -n "$rest" ]; do
      ch=${'$'}{rest%"${'$'}{rest#?}"}
      # Bytes above 0x7f come back sign-extended from printf; keep the byte.
      hex="$hex$(printf '%02x' $(( $(printf '%d' "'$ch") & 255 )))"
      case "$ch" in
        \\) json="$json\\\\" ;;
        \") json="$json\\\"" ;;
        *) json="$json$ch" ;;
      esac
      rest=${'$'}{rest#?}
    done
    turn "$json"
    printf 'CFSMOKE-HEX %s\n' "$hex"
  fi
done
`

/**
 * A whole machine for the app to live in: its own HOME, its own state root,
 * its own PATH with one harness on it.
 *
 * `SHELL` is deliberately absent. `commands.rs` asks the login shell for a
 * PATH when it has one and REPLACES the child's with the answer, which would
 * hand the app the real machine's harnesses. With no `SHELL` that lookup
 * returns nothing and the PATH built here is the one the app uses — the
 * isolation is the absence, so do not add `SHELL` back.
 */
function sandbox() {
  const root = mkdtempSync(join(tmpdir(), 'cf-smoke-'))
  const paths = {
    root,
    home: join(root, 'home'),
    state: join(root, 'state'),
    workspace: join(root, 'workspace'),
    bin: join(root, 'bin'),
    probe: join(root, 'probe'),
    pidFile: join(root, 'harness.pid'),
  }
  for (const dir of [paths.home, paths.state, paths.workspace, paths.bin, paths.probe]) {
    mkdirSync(dir, { recursive: true })
  }
  const tag = `smoke-${process.pid}-${Date.now()}`
  const harness = join(paths.bin, 'claude')
  writeFileSync(harness, FAKE_HARNESS, 'utf8')
  chmodSync(harness, 0o755)
  const pasteReader = join(paths.probe, 'paste-reader.mjs')
  writeFileSync(
    pasteReader,
    `
import { createHash } from 'node:crypto'
const chunks = []
process.stdout.write('CFSMOKE-PASTE-READY\\r\\n')
process.stdin.on('data', chunk => {
  chunks.push(chunk)
  const bytes = Buffer.concat(chunks)
  if (!bytes.subarray(-6).equals(Buffer.from('\\x1b[201~'))) return
  const hash = createHash('sha256').update(bytes).digest('base64')
  process.stdout.write('CFSMOKE-PASTE ' + bytes.length + ' ' + hash + '\\r\\n', () => process.exit(0))
})
`,
  )
  return {
    ...paths,
    tag,
    env: {
      PATH: `${paths.bin}:/usr/bin:/bin:/usr/sbin:/sbin`,
      HOME: paths.home,
      TMPDIR: paths.root,
      CONSENSFLOW_HOME: paths.state,
      CLAUDE_CONFIG_DIR: join(paths.home, '.claude'),
      CODEX_HOME: join(paths.home, '.codex'),
      XDG_CONFIG_HOME: join(paths.home, '.config'),
      CONSENSFLOW_SELFTEST: '1',
      CONSENSFLOW_SELFTEST_DIR: paths.workspace,
      CONSENSFLOW_SELFTEST_TAG: tag,
      CFSMOKE_PIDFILE: paths.pidFile,
      CFSMOKE_PASTE_READER: pasteReader,
      CFSMOKE_TAG: tag,
    },
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

/** A running app plus everything needed to read it and to be sure it died. */
function launch(binary, box) {
  const child = spawn(binary, [], {
    cwd: box.root,
    env: box.env,
    detached: true,
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const events = []
  const waiters = new Set()
  const stderr = []
  let stdout = ''
  child.stdout.setEncoding('utf8')
  child.stdout.on('data', (chunk) => {
    stdout += chunk
    let cut = stdout.indexOf('\n')
    while (cut !== -1) {
      const line = stdout.slice(0, cut)
      stdout = stdout.slice(cut + 1)
      if (line.startsWith('consensflow-selftest ')) {
        try {
          events.push(JSON.parse(line.slice('consensflow-selftest '.length)))
          for (const waiter of [...waiters]) waiter()
        } catch {
          // A malformed report is a failure of the assertion that wanted it,
          // not of the reader; keep draining so the app never blocks on us.
        }
      }
      cut = stdout.indexOf('\n')
    }
  })
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk) => stderr.push(chunk))

  const exited = new Promise((resolveExit) => {
    child.on('exit', (code, signal) => resolveExit({ code, signal }))
  })

  /** Everything the page said about going wrong, newest last. */
  function trouble() {
    const lines = events
      .filter((event) => ['page-error', 'page-rejection', 'failed', 'probe'].includes(event.event))
      .map((event) => `${event.event}: ${JSON.stringify(event.data)}`)
    // The last `waiting` is the picture of the page at the moment it gave up:
    // the terminal's size, what was actually on it, how many acks had flowed.
    const waiting = events.filter((event) => event.event === 'waiting').at(-1)
    if (waiting !== undefined) lines.push(`last waiting: ${JSON.stringify(waiting.data)}`)
    return lines
  }

  // WebKit reports "ResizeObserver loop completed with undelivered
  // notifications" as a window error when xterm refits inside a dock that is
  // still settling: a frame was skipped, nothing in the page failed.
  const fatal = (event) =>
    event.event === 'failed' ||
    event.event === 'page-rejection' ||
    (event.event === 'page-error' && !/ResizeObserver loop/.test(event.data?.message ?? ''))

  async function waitFor(name, timeoutMs = HANDSHAKE_MS) {
    const deadline = Date.now() + timeoutMs
    for (;;) {
      const found = events.find((event) => event.event === name)
      if (found !== undefined) return found
      // A driver that has already given up will never report anything else.
      // Waiting out the rest of the timeout only delays the same failure.
      const dead = events.find((event) => fatal(event))
      if (dead !== undefined) {
        throw new Error(
          `the app gave up before reporting "${name}".\n${trouble().join('\n')}\n` +
            `sandbox kept at ${box.root}\nstderr: ${stderr.join('').slice(-4000)}\n` +
            `app.log: ${appLog(box)}`,
        )
      }
      const left = deadline - Date.now()
      if (left <= 0) {
        // A timeout on its own says nothing useful. The page forwards its own
        // errors, rejections, give-ups and its view of the screen over the
        // same channel, so quote them here rather than making the next reader
        // launch the app by hand.
        const seen = trouble()
        throw new Error(
          `the app never reported "${name}" within ${timeoutMs} ms.\n` +
            `reported: ${events.map((event) => event.event).join(', ') || '(nothing)'}\n` +
            `${seen.length > 0 ? `${seen.join('\n')}\n` : ''}` +
            `sandbox kept at ${box.root}\n` +
            `stderr: ${stderr.join('').slice(-4000)}\n` +
            `app.log: ${appLog(box)}`,
        )
      }
      await new Promise((wake) => {
        const timer = setTimeout(wake, Math.min(250, left))
        const waiter = () => {
          clearTimeout(timer)
          waiters.delete(waiter)
          wake()
        }
        waiters.add(waiter)
      })
    }
  }

  return {
    child,
    events,
    stderr,
    waitFor,
    exited,
    /** The app's own quit path: stdin EOF, then its real `RunEvent::Exit`. */
    quit: () => child.stdin.end(),
    kill: () => {
      try {
        process.kill(-child.pid, 'SIGKILL')
      } catch {
        // Already gone, which is the outcome the caller wanted.
      }
    },
  }
}

function alive(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch (error) {
    return error?.code === 'EPERM'
  }
}

/** Runs a script with the BUNDLE's node, outside this checkout. */
function withBundledNode(node, box, script, extraEnv = {}) {
  const file = join(box.probe, `probe-${Math.random().toString(36).slice(2)}.mjs`)
  writeFileSync(file, script, 'utf8')
  return new Promise((done) => {
    const child = spawn(node, [file], {
      cwd: box.probe,
      env: { ...box.env, ...extraEnv },
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    let out = ''
    let err = ''
    child.stdout.setEncoding('utf8')
    child.stderr.setEncoding('utf8')
    child.stdout.on('data', (chunk) => {
      out += chunk
    })
    child.stderr.on('data', (chunk) => {
      err += chunk
    })
    child.on('exit', (code) => done({ code, out, err }))
  })
}

test('the built app opens a pane, renders a real child, takes input and exits clean', async (t) => {
  const found = gate(t)
  if (found === null) return

  const box = sandbox()
  box.env.CFSMOKE_PASTE_NODE = found.node
  const app = launch(found.binary, box)
  // A failed smoke leaves its machine behind on purpose: the harness, its pid
  // file, the state root and the app's own launchers are the evidence, and
  // rebuilding to look at them again costs minutes. A passing one tidies up.
  let finished = false
  t.after(() => {
    app.kill()
    if (finished && process.env.CONSENSFLOW_SMOKE_KEEP !== '1') box.cleanup()
  })

  // 1. The page came from the bundle, not from a dev server or the checkout.
  const boot = await app.waitFor('boot')
  assert.equal(boot.data.protocol, 'tauri:', `the page loaded over ${boot.data.protocol}`)
  for (const asset of boot.data.assets) {
    assert.ok(
      asset.startsWith('tauri://'),
      `the page loaded ${asset}, which did not come from the bundle`,
    )
  }

  // 2. A project, its chief window docked beside the board, and the fake
  //    harness's own first line drawn by the real xterm — through
  //    `project.open`, the production operation.
  const opened = await app.waitFor('project')
  assert.equal(opened.data.ok, true, `project.open refused: ${JSON.stringify(opened.data)}`)

  const rendered = await app.waitFor('rendered')
  assert.match(rendered.data.banner, new RegExp(`CFSMOKE-READY ${box.tag}`))
  assert.ok(rendered.data.rows > 0, 'the pane rendered no rows')
  // The pane's own PATH, reported by the child that has to live with it.
  assert.equal(
    rendered.data.tools,
    'ok',
    'the chief pane inherited a PATH with no system commands on it',
  )

  // 3. Input typed through the page's own path reached the child: it came
  //    back as hex, which only the child computes.
  const echoed = await app.waitFor('echo')
  assert.equal(
    echoed.data.hex,
    Buffer.from(echoed.data.typed, 'utf8').toString('hex'),
    'the child echoed something other than what was typed',
  )

  // The board both ways: the chief's task reached a worker window, and the
  // worker's result came back into the chief's window as a paste the child
  // hexed, header first.
  const board = await app.waitFor('board')
  assert.ok(Number.isInteger(board.data.result), 'no result came back from the worker')
  assert.match(
    Buffer.from(board.data.hex, 'hex').toString('utf8'),
    /^\[ConsensFlow m-\d+ · T-1 · result from @terpsichore-[a-z]+-[a-z]+\]/,
  )
  assert.equal(board.data.delivered, true, 'the core never confirmed the delivery from the record')

  const agentsWindow = await app.waitFor('agents-window')
  assert.deepEqual(
    [agentsWindow.data.first.ok, agentsWindow.data.first.label, agentsWindow.data.first.reused],
    [true, 'agents', false],
    JSON.stringify(agentsWindow.data.first),
  )
  assert.match(agentsWindow.data.first.url, /^http:\/\/localhost:\d+\/\?token=/)
  assert.deepEqual(
    [agentsWindow.data.again.ok, agentsWindow.data.again.reused],
    [true, true],
    'the second ask reuses the window',
  )
  assert.match(agentsWindow.data.again.url, /\/harnesses\?token=/)

  const pasted = await app.waitFor('large-paste')
  const expectedPaste = Buffer.from('\x1b[200~' + '漢字 résumé 🙂\r'.repeat(30_000) + '\x1b[201~')
  assert.equal(pasted.data.bytes, expectedPaste.length)
  assert.equal(pasted.data.hash, createHash('sha256').update(expectedPaste).digest('base64'))

  // 4. Acks flow. The flood is ~${FLOOD_BYTES} bytes by construction, well
  //    over the 1 MiB unacked-output window, so its LAST line can only be on
  //    screen if the page returned credit for everything before it. The ack
  //    count is the same fact from the page's side.
  const drained = await app.waitFor('drained')
  assert.ok(FLOOD_BYTES > 1024 * 1024, 'the flood no longer exceeds the output window')
  assert.equal(drained.data.lastFloodLine, FLOOD_LINES)
  assert.ok(drained.data.acks > 1, `only ${drained.data.acks} acks for ${FLOOD_BYTES} bytes`)

  const harnessPid = Number(readFileSync(box.pidFile, 'utf8').trim())
  assert.ok(Number.isInteger(harnessPid) && harnessPid > 0, 'the fake harness wrote no pid')
  assert.ok(alive(harnessPid), 'the fake harness was not running when it answered')

  // 5. The packaged pi extension resolves its dependency inside the bundle.
  //    Run by the bundle's own node, from a directory outside this checkout.
  const extension = join(
    found.app,
    'Contents',
    'Resources',
    'cli',
    'hosts',
    'pi-extension',
    'consensflow-delivery.mjs',
  )
  const hooks = join(box.probe, 'hooks.mjs')
  const sink = join(box.probe, 'resolved.json')
  // Node's own module-customization hooks, so the claim is what the RUNTIME
  // resolved rather than what the source appears to import.
  writeFileSync(
    hooks,
    `
    import { readFileSync, writeFileSync } from 'node:fs'
    let sink = null
    export async function initialize(data) {
      sink = data.sink
      writeFileSync(sink, '[]')
    }
    export async function resolve(specifier, context, nextResolve) {
      const result = await nextResolve(specifier, context)
      const seen = JSON.parse(readFileSync(sink, 'utf8'))
      seen.push({ specifier, url: result.url })
      writeFileSync(sink, JSON.stringify(seen))
      return result
    }
    `,
    'utf8',
  )
  const resolution = await withBundledNode(
    found.node,
    box,
    `
    import { register } from 'node:module'
    import { pathToFileURL } from 'node:url'
    import { writeFileSync } from 'node:fs'
    register(pathToFileURL(${JSON.stringify(hooks)}), { data: { sink: ${JSON.stringify(sink)} } })
    const module = await import(${JSON.stringify(extension)})
    writeFileSync(1, JSON.stringify({ default: typeof module.default }) + '\\n')
    `,
  )
  assert.equal(resolution.code, 0, `the bundled pi extension failed to load: ${resolution.err}`)
  assert.match(resolution.out, /"default":"function"/)

  const resolved = JSON.parse(readFileSync(sink, 'utf8'))
  const bundleRoot = join(found.app, 'Contents', 'Resources', 'cli')
  const outside = resolved.filter(
    (entry) =>
      entry.url.startsWith('file://') && !fileURLToPath(entry.url).startsWith(`${bundleRoot}/`),
  )
  assert.deepEqual(
    outside,
    [],
    `the packaged extension resolved files outside the bundle: ${JSON.stringify(outside)}`,
  )
  assert.ok(
    resolved.some((entry) => entry.url.endsWith('/hosts/lib/receiver.js')),
    'the packaged extension never resolved hosts/lib/receiver.js',
  )
  // Not "nothing under the repo": a locally built bundle LIVES under the
  // repo, so that would be trivially false. What must never be touched are
  // the checkout's live sources, which is where a path that escaped the
  // bundle would land.
  const checkout = ['src', 'hosts', 'bin', 'skill'].map((part) => `file://${join(REPO, part)}/`)
  const leaked = resolved.filter((entry) => checkout.some((root) => entry.url.startsWith(root)))
  assert.deepEqual(
    leaked,
    [],
    `the packaged extension resolved live sources from this checkout: ${JSON.stringify(leaked)}`,
  )

  // The packaged receiver must load without any checkout or global extension dependency.
  const receiver = await withBundledNode(
    found.node,
    box,
    `
    import assert from 'node:assert/strict'
    import { createReceiver } from ${JSON.stringify(join(bundleRoot, 'hosts/lib/receiver.js'))}
    const calls = []
    const receiver = createReceiver({ session: () => 'native-smoke', ready: () => true,
      request: async (op) => { calls.push(op); return op === 'state' ? null : op === 'register' ? { session:'native-smoke',lease:'smoke' } : null },
      insert: () => { throw new Error('empty inbox must never insert') },
    })
    await receiver.poll()
    await receiver.stop()
    assert.deepEqual(calls, ['state','register','claim','retire'])
    process.stdout.write('PACKAGED-RECEIVER-OK')
  `,
  )
  assert.equal(receiver.code, 0, receiver.err)
  assert.equal(receiver.out, 'PACKAGED-RECEIVER-OK')

  // 6. The ledger is the app's: its exclusive lock refuses the bundled node a
  //    second opening while the app runs.
  const lock = await withBundledNode(
    found.node,
    box,
    `
    import { openLedger } from ${JSON.stringify(join(bundleRoot, 'src', 'ledger', 'index.js'))}
    import { join } from 'node:path'
    import { writeFileSync } from 'node:fs'
    try {
      openLedger(join(process.env.CONSENSFLOW_HOME, 'consensflow.db'))
      writeFileSync(1, 'SECOND-OWNER\\n')
    } catch (error) {
      writeFileSync(1, 'REFUSED ' + error.code + ' ' + error.message + '\\n')
    }
    `,
  )
  assert.equal(lock.code, 0, `the lock probe crashed: ${lock.err}`)
  assert.match(
    lock.out,
    /^REFUSED ledger-locked another ConsensFlow has .*consensflow\.db open/m,
    `the bundled node opened the running app's ledger: ${lock.out}`,
  )

  // 7. The app's own exit: stdin EOF, `RunEvent::Exit`, and nothing left.
  const settled = await app.waitFor('settled')
  assert.equal(settled.data.terminalPreserved, true)
  app.quit()
  const ended = await app.exited
  assert.equal(ended.code, 0, `the app exited ${ended.code} / ${ended.signal}`)
  assert.equal(alive(harnessPid), false, 'the fake harness outlived the app')
  finished = true
})

test('built Agents catalog serves complete saved profiles and current browsing controls', async (t) => {
  const found = gate(t)
  if (found === null) return
  const box = sandbox()
  t.after(() => box.cleanup())
  const cli = join(found.app, 'Contents', 'Resources', 'cli')
  const result = await withBundledNode(
    found.node,
    box,
    `
    import assert from 'node:assert/strict'
    import { mkdirSync, readFileSync } from 'node:fs'
    import { join } from 'node:path'
    import { CATALOG, catalogEntry } from ${JSON.stringify(join(cli, 'src/catalog.js'))}
    import { agentsUi } from ${JSON.stringify(join(cli, 'src/core/agents-server.js'))}
    import { Credentials, startApi } from ${JSON.stringify(join(cli, 'src/core/api.js'))}
    import { openLedger } from ${JSON.stringify(join(cli, 'src/ledger/index.js'))}
    import { addAgent, listAgents, rosterPath } from ${JSON.stringify(join(cli, 'src/roster.js'))}
    assert.equal(Object.values(CATALOG).flat().length, 94, 'packaged preset count')
    assert.equal(catalogEntry('pygmalion').model, 'codex-image')
    // Every catalog agent is in the roster, as the catalog has it; the file keeps only your own.
    assert.equal(listAgents(process.env).length, 94)
    addAgent({ name: 'my-maia', harness: 'codex', model: 'gpt-6-astra', effort: 'low' }, process.env)
    // The agents pages the way the daemon serves them: behind its API, opened with the UI token.
    mkdirSync(process.env.CONSENSFLOW_HOME, { recursive: true })
    const ledger = openLedger(join(process.env.CONSENSFLOW_HOME, 'consensflow.db'))
    const token = 'smoke-ui-token'
    const server = await startApi({ ledger, credentials: new Credentials(), ui: agentsUi(process.env, { token }) })
    try {
      const headers = { authorization: 'Bearer ' + token }
      const data = await (await fetch(server.url + '/api/agents', { headers })).json()
      const stored = JSON.parse(readFileSync(rosterPath(process.env), 'utf8')).agents.find(a => a.id === 'my-maia')
      assert.deepEqual([stored.effort, stored.model, Object.hasOwn(stored, 'profile')], ['low', 'gpt-6-astra', false])
      const mine = data.agents.find(a => a.name === 'my-maia')
      assert.deepEqual([mine.effort, mine.custom, mine.profile.workTier], ['low', true, 'light'])
      assert.equal(data.agents.length, 95)
      const html = await (await fetch(server.url, { headers })).text()
      for (const text of ['aria-label="Agents"', 'Model and reasoning', 'My own agents', 'model-summary', 'model-group', 'value="model-reasoning" selected', 'Work tier', 'tier-pill', 'Important work only · No coding']) assert.ok(html.includes(text), text)
      for (const text of ['id="catalog-section"', 'Agent library', 'Your agents', 'PM candidate', 'name="tags"', 'category-pill', 'Chief of Staff candidate', 'name="category"', 'Name in use', 'offer__actions', 'Saved only', 'Sort by', 'benchmark', 'Artificial Analysis', 'AA ']) assert.ok(!html.includes(text), 'gone: ' + text)
      assert.equal((await fetch(server.url + '/api/agents/maia', { method: 'DELETE', headers })).status, 400)
      assert.equal((await fetch(server.url + '/api/agents/my-maia', { method: 'DELETE', headers })).status, 204)
      const after = await (await fetch(server.url + '/api/agents', { headers })).json()
      assert.equal(after.agents.length, 94)
      assert.equal(Object.hasOwn(after, 'catalog'), false)
      console.log('packaged catalog and saved profiles verified')
    } finally {
      await server.close()
      ledger.close()
    }
  `,
  )
  assert.equal(result.code, 0, result.err)
  assert.match(result.out, /packaged catalog and saved profiles verified/)
})
