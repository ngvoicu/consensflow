import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
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

const FAKE_HARNESS = `#!/bin/sh
# The smoke's stand-in harness. It exists to be recognisable on screen, to
# prove that what the human types reaches a real child — every line it reads
# comes back as hex, which no echo, no replay and no cached frame could
# produce — and, on request, to out-run the output window.
#
# The flood is asked for rather than printed at start-up: 1.5 MiB of wrapped
# lines pushes far more rows than xterm keeps, so a banner printed before it
# is gone by the time anything can look for it. The page says when it has
# seen the banner; only then does the flood run.
echo $$ > "$CFSMOKE_PIDFILE"
printf 'CFSMOKE-READY %s\\n' "$CFSMOKE_TAG"
# Says whether the pane inherited a usable PATH. A lead whose PATH holds only
# ConsensFlow's own directories cannot run git, ripgrep or any of what a real
# harness shells out to, and every test that stubs the environment would still
# pass. One system command settles it.
if command -v uname >/dev/null 2>&1; then
  printf 'CFSMOKE-TOOLS ok\\n'
else
  printf 'CFSMOKE-TOOLS missing\\n'
fi
pad=''
n=0
while [ $n -lt ${FLOOD_WIDTH} ]; do
  pad="\${pad}x"
  n=$((n + 1))
done
while IFS= read -r line; do
  if [ "$line" = "FLOOD" ]; then
    n=1
    while [ $n -le ${FLOOD_LINES} ]; do
      printf 'CFSMOKE-FLOOD %s %s\\n' "$n" "$pad"
      n=$((n + 1))
    done
    printf 'CFSMOKE-FLOODED %s\\n' "$CFSMOKE_TAG"
  else
    # Shell builtins only, on purpose. A lead pane's PATH once carried just
    # ConsensFlow's own bin directories — this fixture is what found that,
    # by failing on a missing \`od\` — and it is fixed now. Keeping the hex in
    # the shell means this test measures the app, not the machine's coreutils.
    hex=''
    rest=$line
    while [ -n "$rest" ]; do
      ch=$\{rest%"$\{rest#?}"}
      hex="$hex$(printf '%02x' "'$ch")"
      rest=$\{rest#?}
    done
    printf 'CFSMOKE-HEX %s\\n' "$hex"
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

  async function waitFor(name, timeoutMs = HANDSHAKE_MS) {
    const deadline = Date.now() + timeoutMs
    for (;;) {
      const found = events.find((event) => event.event === name)
      if (found !== undefined) return found
      // A driver that has already given up will never report anything else.
      // Waiting out the rest of the timeout only delays the same failure.
      const dead = events.find((event) =>
        ['failed', 'page-error', 'page-rejection'].includes(event.event),
      )
      if (dead !== undefined) {
        throw new Error(
          `the app gave up before reporting "${name}".\n${trouble().join('\n')}\n` +
            `sandbox kept at ${box.root}\nstderr: ${stderr.join('').slice(-4000)}`,
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
            `stderr: ${stderr.join('').slice(-4000)}`,
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

  // 2. A tab, a lead pane, and the fake harness's own first line drawn by the
  //    real xterm — through `open_lead`, the production command.
  const opened = await app.waitFor('tab')
  assert.equal(opened.data.ok, true, `open_lead refused: ${JSON.stringify(opened.data)}`)

  const rendered = await app.waitFor('rendered')
  assert.match(rendered.data.banner, new RegExp(`CFSMOKE-READY ${box.tag}`))
  assert.ok(rendered.data.rows > 0, 'the pane rendered no rows')
  // The pane's own PATH, reported by the child that has to live with it.
  assert.equal(
    rendered.data.tools,
    'ok',
    'the lead pane inherited a PATH with no system commands on it',
  )

  // 3. Input typed through the page's own path reached the child: it came
  //    back as hex, which only the child computes.
  const echoed = await app.waitFor('echo')
  assert.equal(
    echoed.data.hex,
    Buffer.from(echoed.data.typed, 'utf8').toString('hex'),
    'the child echoed something other than what was typed',
  )

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
    (entry) => entry.url.startsWith('file://') && !entry.url.startsWith(`file://${bundleRoot}/`),
  )
  assert.deepEqual(
    outside,
    [],
    `the packaged extension resolved files outside the bundle: ${JSON.stringify(outside)}`,
  )
  assert.ok(
    resolved.some((entry) => entry.url.endsWith('/hosts/lib/deliveries.js')),
    'the packaged extension never resolved hosts/lib/deliveries.js',
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

  // 5b. …and the packaged extension actually DELIVERS. Importing it proves
  //     its dependencies resolve; only running its own entry point proves the
  //     bundled copy still works. This drives `consensflowDelivery(pi)` from
  //     the bundle with a stand-in Pi, a real record in a real inbox, and
  //     reads back the ack the extension writes.
  const delivery = await withBundledNode(
    found.node,
    box,
    `
    import { mkdir, readFile, writeFile } from 'node:fs/promises'
    import { writeFileSync } from 'node:fs'
    import { join } from 'node:path'
    import { envelope } from ${JSON.stringify(join(bundleRoot, 'hosts', 'lib', 'deliveries.js'))}
    import consensflowDelivery from ${JSON.stringify(extension)}

    const root = process.env.CF_DELIVERY_ROOT
    const dirs = ['inbox', 'ack', 'quarantine', 'settled', 'expired']
    for (const dir of dirs) await mkdir(join(root, dir), { recursive: true })

    const record = {
      id: 'd-1',
      conversation: 'smoke-worker',
      answerId: 'a-1',
      answer: 'the packaged extension delivered this',
      expiresAt: Date.now() + 30_000,
    }
    const text = envelope(record)
    await writeFile(join(root, 'inbox', 'd-1.json'), JSON.stringify({ ...record, text }), 'utf8')

    // A stand-in Pi: it records the send and then reports the message_start
    // the extension treats as proof of admission, exactly as Pi does.
    const handlers = new Map()
    const sent = []
    const pi = {
      on: (name, handler) => handlers.set(name, handler),
      sendUserMessage: (body) => {
        sent.push(body)
        const handler = handlers.get('message_start')
        if (handler !== undefined) {
          void handler({ message: { role: 'user', content: body } }, context)
        }
      },
    }
    const context = { isIdle: () => true, sessionManager: { getSessionId: () => 's-1', getLeafId: () => 'l-1' } }

    consensflowDelivery(pi)
    await handlers.get('session_start')({}, context)

    let ack = null
    for (let attempt = 0; attempt < 100 && ack === null; attempt += 1) {
      try {
        ack = JSON.parse(await readFile(join(root, 'ack', 'd-1.json'), 'utf8'))
      } catch {
        await new Promise((wake) => setTimeout(wake, 50))
      }
    }
    writeFileSync(1, JSON.stringify({ ack, sentMatchesEnvelope: sent[0] === text }) + '\\n')
    process.exit(0)
    `,
    {
      CF_DELIVERY_ROOT: box.probe,
      CF_DELIVERY_INBOX: join(box.probe, 'inbox'),
      CF_DELIVERY_ACK: join(box.probe, 'ack'),
      CF_DELIVERY_QUARANTINE: join(box.probe, 'quarantine'),
      CF_DELIVERY_SETTLED: join(box.probe, 'settled'),
      CF_DELIVERY_EXPIRED: join(box.probe, 'expired'),
      CF_DELIVERY_LAUNCH_ID: 'smoke-launch',
    },
  )
  assert.equal(delivery.code, 0, `the packaged extension could not deliver: ${delivery.err}`)
  const delivered = JSON.parse(delivery.out.trim().split('\n').at(-1))
  assert.equal(
    delivered.sentMatchesEnvelope,
    true,
    'the extension sent something other than the envelope',
  )
  assert.equal(
    delivered.ack?.admitted,
    true,
    `the packaged extension did not admit: ${JSON.stringify(delivered.ack)}`,
  )

  // 6. The state root is the app's, held by a kernel lock the bundled node
  //    cannot take a second time.
  const lock = await withBundledNode(
    found.node,
    box,
    `
    import { Store } from ${JSON.stringify(join(bundleRoot, 'src', 'store.js'))}
    import { writeFileSync } from 'node:fs'
    const store = new Store(process.env.CONSENSFLOW_HOME)
    try {
      await store.open()
      writeFileSync(1, 'SECOND-OWNER\\n')
    } catch (error) {
      writeFileSync(1, 'REFUSED ' + error.message + '\\n')
    }
    `,
  )
  assert.equal(lock.code, 0, `the lock probe crashed: ${lock.err}`)
  assert.match(
    lock.out,
    /^REFUSED another ConsensFlow instance holds .*instance\.lock/m,
    `the bundled node took a second lock on the running app's state root: ${lock.out}`,
  )

  // 7. The app's own exit: stdin EOF, `RunEvent::Exit`, and nothing left.
  await app.waitFor('settled')
  app.quit()
  const ended = await app.exited
  assert.equal(ended.code, 0, `the app exited ${ended.code} / ${ended.signal}`)
  assert.equal(alive(harnessPid), false, 'the fake harness outlived the app')
  finished = true
})
