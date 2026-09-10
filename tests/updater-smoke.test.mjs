import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  chmodSync,
  closeSync,
  constants,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
  writeSync,
} from 'node:fs'
import { createServer } from 'node:https'
import { tmpdir } from 'node:os'
import { basename, dirname, join, relative, resolve } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const REPO = dirname(dirname(fileURLToPath(import.meta.url)))
const REQUESTED = process.env.CONSENSFLOW_UPDATER_SMOKE === '1'
const TIMEOUT_MS = Number(process.env.CONSENSFLOW_UPDATER_SMOKE_TIMEOUT_MS ?? 180_000)
const SIGNER = join(REPO, 'app', 'node_modules', '.bin', 'tauri')

/**
 * This is the packaged acceptance gate, not a source or dev-server test.
 * Without the explicit opt-in it is one skipped test so the ordinary Node
 * suite stays cheap. Once requested, every missing or unsafe input fails with
 * the build-path the release driver needs to provide.
 */

function requestedPath(name) {
  const value = process.env[name]
  assert.ok(
    typeof value === 'string' && value.length > 0,
    `${name} is required; Root must provide the two built app paths`,
  )
  const path = realpathSync(resolve(value))
  assert.equal(path.endsWith('.app'), true, `${name} must point to a .app bundle: ${path}`)
  const protectedRoot = resolve('/Applications')
  const outside = relative(protectedRoot, path)
  assert.ok(
    outside === '..' || outside.startsWith('../') || outside.startsWith('/'),
    `${name} may not point into /Applications: ${path}`,
  )
  assert.ok(existsSync(path), `${name} bundle does not exist: ${path}`)
  return path
}

function commandOutput(command, args, options = {}) {
  try {
    return execFileSync(command, args, { encoding: 'utf8', ...options })
  } catch (cause) {
    const stderr = cause?.stderr?.toString?.() ?? ''
    throw new Error(`${command} ${args.join(' ')} failed: ${stderr.trim() || cause.message}`)
  }
}

function plist(app, field) {
  return commandOutput('/usr/bin/plutil', [
    '-extract',
    field,
    'raw',
    '-o',
    '-',
    join(app, 'Contents', 'Info.plist'),
  ]).trim()
}

function appInfo(app, label) {
  assert.ok(
    existsSync(join(app, 'Contents', 'Info.plist')),
    `${label} has no Contents/Info.plist: ${app}`,
  )
  const executable = plist(app, 'CFBundleExecutable')
  const binary = join(app, 'Contents', 'MacOS', executable)
  const sidecar = join(app, 'Contents', 'MacOS', 'node')
  const stagedNode = join(app, 'Contents', 'Resources', 'binaries', 'node')
  const cli = join(app, 'Contents', 'Resources', 'cli')
  const packageFile = join(cli, 'package.json')
  const bundledNode = existsSync(sidecar) ? sidecar : stagedNode
  for (const [what, path] of [
    ['native executable', binary],
    ['bundled Node runtime', bundledNode],
    ['bundled CLI package', packageFile],
    ['bundled CLI entrypoint', join(cli, 'bin', 'cf.mjs')],
    ['bundled CLI hosts', join(cli, 'hosts')],
    ['bundled CLI source', join(cli, 'src')],
  ]) {
    assert.ok(existsSync(path), `${label} has no ${what}: ${path}`)
  }
  const packageJson = JSON.parse(readFileSync(packageFile, 'utf8'))
  return {
    app,
    binary,
    bundledNode,
    executable,
    version: plist(app, 'CFBundleShortVersionString'),
    cliVersion: packageJson.version,
  }
}

function verifyBundle(info, expected, label) {
  assert.equal(
    info.version,
    expected,
    `${label} plist version is ${info.version}, expected ${expected}`,
  )
  assert.equal(
    info.cliVersion,
    expected,
    `${label} bundled CLI version is ${info.cliVersion}, expected ${expected}`,
  )
  commandOutput('/usr/bin/codesign', ['--verify', '--deep', '--strict', info.app])
}

function digestManifest(root) {
  const entries = []
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const path = join(directory, entry.name)
      const name = relative(root, path)
      if (entry.isDirectory()) {
        visit(path)
      } else if (entry.isSymbolicLink()) {
        entries.push({ name, link: readlinkSync(path) })
      } else if (entry.isFile()) {
        entries.push({
          name,
          sha256: createHash('sha256').update(readFileSync(path)).digest('hex'),
          mode: statSync(path).mode & 0o777,
        })
      } else {
        throw new Error(`unsupported bundle entry in digest manifest: ${path}`)
      }
    }
  }
  visit(root)
  return entries
}

function shellQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`
}

function fakeHarnesses(box) {
  const script = (name) => `#!/bin/sh
set -eu
if [ "\${1:-}" = "--version" ]; then
  if [ "${name}" = "claude" ]; then
    printf '2.1.266\\n'
  else
    printf '0.0.0\\n'
  fi
  exit 0
fi
printf '%s\\n' "$$" > ${shellQuote(join(box.pids, name))}-$$.pid
printf 'CFUPDATER-ALIVE %s\\n' "$$"
while IFS= read -r _line; do
  :
done
`
  for (const name of ['claude', 'codex', 'pi', 'opencode', 'kimi']) {
    const path = join(box.bin, name)
    writeFileSync(path, script(name), 'utf8')
    chmodSync(path, 0o755)
  }
}

function sandbox() {
  // Tauri deliberately rejects relaunch paths with symlinked ancestors;
  // macOS /var is a symlink to /private/var.
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'cf-updater-smoke-')))
  const box = {
    root,
    apps: join(root, 'Applications'),
    copy: join(root, 'Applications', 'ConsensFlow.app'),
    home: join(root, 'home'),
    state: join(root, 'state'),
    workspace: join(root, 'workspace'),
    secondWorkspace: join(root, 'workspace', '.consensflow-updater-second'),
    bin: join(root, 'bin'),
    pids: join(root, 'pids'),
    tls: join(root, 'tls'),
    probe: join(root, 'probe'),
  }
  for (const path of [
    box.apps,
    box.home,
    box.state,
    box.workspace,
    box.secondWorkspace,
    box.bin,
    box.pids,
    box.tls,
    box.probe,
  ])
    mkdirSync(path, { recursive: true })
  fakeHarnesses(box)
  return box
}

function openssl(box) {
  const caKey = join(box.tls, 'root.key')
  const caCert = join(box.tls, 'root.pem')
  const serverKey = join(box.tls, 'server.key')
  const serverCsr = join(box.tls, 'server.csr')
  const serverCert = join(box.tls, 'server.pem')
  const extensions = join(box.tls, 'server.ext')
  writeFileSync(
    extensions,
    [
      'subjectAltName=DNS:localhost,IP:127.0.0.1',
      'basicConstraints=critical,CA:FALSE',
      'keyUsage=critical,digitalSignature,keyEncipherment',
      'extendedKeyUsage=serverAuth',
      'subjectKeyIdentifier=hash',
      'authorityKeyIdentifier=keyid,issuer',
      '',
    ].join('\n'),
    'utf8',
  )
  commandOutput(
    'openssl',
    [
      'req',
      '-x509',
      '-newkey',
      'rsa:2048',
      '-nodes',
      '-keyout',
      caKey,
      '-out',
      caCert,
      '-days',
      '1',
      '-subj',
      '/CN=ConsensFlow updater smoke root',
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  )
  commandOutput(
    'openssl',
    [
      'req',
      '-newkey',
      'rsa:2048',
      '-nodes',
      '-keyout',
      serverKey,
      '-out',
      serverCsr,
      '-subj',
      '/CN=localhost',
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  )
  commandOutput(
    'openssl',
    [
      'x509',
      '-req',
      '-in',
      serverCsr,
      '-CA',
      caCert,
      '-CAkey',
      caKey,
      '-CAcreateserial',
      '-out',
      serverCert,
      '-days',
      '1',
      '-sha256',
      '-extfile',
      extensions,
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  )
  return {
    caCert,
    key: readFileSync(serverKey),
    cert: readFileSync(serverCert),
  }
}

function signArchive(box, archive) {
  assert.ok(existsSync(SIGNER), `packaged Tauri signer is missing: ${SIGNER}`)
  const key = join(box.tls, 'test-updater-key')
  const env = { ...process.env }
  delete env.TAURI_SIGNING_PRIVATE_KEY
  delete env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  commandOutput(SIGNER, ['signer', 'generate', '--ci', '--password', '', '--write-keys', key], {
    cwd: REPO,
    env,
    stdio: ['ignore', 'ignore', 'pipe'],
  })
  commandOutput(SIGNER, ['signer', 'sign', '--password', '', '--private-key-path', key, archive], {
    cwd: REPO,
    env,
    stdio: ['ignore', 'ignore', 'pipe'],
  })
  return {
    publicKeyPath: `${key}.pub`,
    signature: readFileSync(`${archive}.sig`, 'utf8').trim(),
  }
}

async function listen(server) {
  await new Promise((resolveListening, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolveListening)
  })
  const address = server.address()
  assert.equal(typeof address, 'object')
  return `https://127.0.0.1:${address.port}/feed`
}

function pidAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false
  try {
    process.kill(pid, 0)
    const state = execFileSync('/bin/ps', ['-p', String(pid), '-o', 'stat='], {
      encoding: 'utf8',
    }).trim()
    return state.length > 0 && !state.startsWith('Z')
  } catch {
    return false
  }
}

function recordedPids(box) {
  return readdirSync(box.pids)
    .filter((name) => name.endsWith('.pid'))
    .map((name) => Number(readFileSync(join(box.pids, name), 'utf8').trim()))
    .filter((pid) => Number.isInteger(pid) && pid > 0)
}

function killRecordedLeadPids(box) {
  for (const pid of new Set(recordedPids(box))) {
    if (!pidAlive(pid)) continue
    try {
      process.kill(pid, 'SIGKILL')
    } catch {
      // The recorded test child already exited.
    }
  }
}

function lockPid(box) {
  const file = join(box.state, 'app', 'instance.lock')
  if (!existsSync(file)) return null
  const first = readFileSync(file, 'utf8').split('\n', 1)[0]
  try {
    const value = JSON.parse(first).pid
    return Number.isInteger(value) && value > 0 ? value : null
  } catch {
    return null
  }
}

async function until(label, check, timeoutMs = TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const result = check()
    if (result !== null && result !== undefined && result !== false) return result
    if (Date.now() >= deadline) throw new Error(`${label} did not happen within ${timeoutMs} ms`)
    await new Promise((wake) => setTimeout(wake, 100))
  }
}

function launch(binary, env, cwd) {
  // Node destroys a spawned child's managed stdin pipe when that PID exits.
  // Keep a separate FIFO writer alive across Tauri's real process restart.
  const fifo = join(cwd, 'updater-control.fifo')
  commandOutput('/usr/bin/mkfifo', [fifo])
  const hold = openSync(fifo, constants.O_RDWR | constants.O_NONBLOCK)
  const input = openSync(fifo, 'r')
  const control = openSync(fifo, 'w')
  closeSync(hold)
  let controlClosed = false
  const child = spawn(binary, [], {
    cwd,
    env,
    detached: true,
    stdio: [input, 'pipe', 'pipe'],
  })
  closeSync(input)
  const events = []
  const failures = []
  const stderr = []
  let buffer = ''
  const appPids = new Set([child.pid])
  child.stdout.setEncoding('utf8')
  child.stdout.on('data', (chunk) => {
    buffer += chunk
    let cut = buffer.indexOf('\n')
    while (cut !== -1) {
      const line = buffer.slice(0, cut)
      buffer = buffer.slice(cut + 1)
      if (line.startsWith('consensflow-selftest ')) {
        try {
          const event = JSON.parse(line.slice('consensflow-selftest '.length))
          events.push(event)
          process.stdout.write(`updater probe ${JSON.stringify(event)}\n`)
          if (Number.isInteger(event.pid) && event.pid > 0) appPids.add(event.pid)
          if (
            ['update-failure', 'page-error', 'page-rejection', 'failed', 'deadline'].includes(
              event.event,
            )
          )
            failures.push(event)
        } catch {
          failures.push({ event: 'malformed-report', data: { line } })
        }
      }
      cut = buffer.indexOf('\n')
    }
  })
  child.stderr.setEncoding('utf8')
  child.stderr.on('data', (chunk) => stderr.push(chunk))

  async function waitFor(label, predicate) {
    return until(label, () => {
      const found = events.find(predicate)
      if (found !== undefined) return found
      if (failures.length > 0) {
        throw new Error(
          `packaged app reported failure: ${JSON.stringify(failures.at(-1))}\nstderr: ${stderr.join('').slice(-4000)}`,
        )
      }
      return null
    })
  }

  return {
    child,
    events,
    stderr,
    appPids,
    waitFor,
    continueUpdate() {
      writeSync(control, 'continue-updater\n')
    },
    closeInput() {
      if (!controlClosed) {
        closeSync(control)
        controlClosed = true
      }
    },
    killRecorded() {
      if (!controlClosed) {
        closeSync(control)
        controlClosed = true
      }
      for (const pid of appPids) {
        try {
          process.kill(-pid, 'SIGKILL')
        } catch {
          // The process or its group already exited.
        }
      }
    },
  }
}

function smokeEnvironment(box, feed, tls, publicKeyPath, expected) {
  return {
    PATH: `${box.bin}:/usr/bin:/bin:/usr/sbin:/sbin`,
    HOME: box.home,
    TMPDIR: box.root,
    CONSENSFLOW_HOME: box.state,
    CLAUDE_CONFIG_DIR: join(box.home, '.claude'),
    CODEX_HOME: join(box.home, '.codex'),
    XDG_CONFIG_HOME: join(box.home, '.config'),
    KIMI_CODE_HOME: join(box.home, '.kimi-code'),
    PI_CODING_AGENT_DIR: join(box.home, '.pi', 'agent'),
    CONSENSFLOW_SELFTEST: '1',
    CONSENSFLOW_SELFTEST_DIR: box.workspace,
    CONSENSFLOW_SELFTEST_UPDATER_EXPECTED: expected,
    CONSENSFLOW_SELFTEST_UPDATER_URL: feed,
    CONSENSFLOW_SELFTEST_UPDATER_CERT: tls.caCert,
    CONSENSFLOW_SELFTEST_UPDATER_KEY: publicKeyPath,
    CONSENSFLOW_SELFTEST_DEADLINE_MS: String(TIMEOUT_MS + 10_000),
  }
}

function archiveFor(box, target) {
  const archive = join(box.probe, `ConsensFlow-${target.version}_aarch64.app.tar.gz`)
  commandOutput(
    '/usr/bin/tar',
    ['-czf', archive, '-C', dirname(target.app), basename(target.app)],
    {
      cwd: REPO,
      env: { ...process.env, COPYFILE_DISABLE: '1' },
    },
  )
  return archive
}

function updaterFeed(target, signature) {
  const archiveName = `ConsensFlow-${target.version}_aarch64.app.tar.gz`
  return {
    version: target.version,
    notes: 'Packaged updater acceptance candidate.',
    pub_date: '2026-09-09T12:00:00Z',
    platforms: {
      'darwin-aarch64': {
        url: `https://github.com/ngvoicu/consensflow/releases/download/v${target.version}/${archiveName}`,
        signature,
      },
    },
  }
}

test('the packaged updater replaces only the isolated copy after pane admission closes', {
  timeout: TIMEOUT_MS + 20_000,
}, async (t) => {
  if (!REQUESTED) {
    t.skip('packaged updater smoke requires CONSENSFLOW_UPDATER_SMOKE=1')
    return
  }
  assert.equal(
    process.platform,
    'darwin',
    'the packaged updater smoke requires macOS bundles and codesign',
  )
  const fromApp = requestedPath('CONSENSFLOW_UPDATER_FROM_APP')
  const toApp = requestedPath('CONSENSFLOW_UPDATER_TO_APP')
  assert.notEqual(fromApp, toApp, 'FROM_APP and TO_APP must be distinct source bundles')

  const box = sandbox()
  let server
  let app
  t.after(async () => {
    app?.killRecorded()
    killRecordedLeadPids(box)
    if (server !== undefined) await new Promise((resolveClosed) => server.close(resolveClosed))
    if (process.env.CONSENSFLOW_UPDATER_SMOKE_KEEP !== '1') {
      rmSync(box.root, { recursive: true, force: true })
    }
  })

  const fromInfo = appInfo(fromApp, 'FROM_APP')
  const toInfo = appInfo(toApp, 'TO_APP')
  assert.notEqual(
    fromInfo.version,
    toInfo.version,
    'the two acceptance builds must have different versions',
  )
  assert.notEqual(
    fromInfo.cliVersion,
    toInfo.cliVersion,
    'the two acceptance builds must have different bundled CLI versions',
  )
  verifyBundle(fromInfo, fromInfo.version, 'FROM_APP')
  verifyBundle(toInfo, toInfo.version, 'TO_APP')
  const targetManifest = digestManifest(toApp)

  cpSync(fromApp, box.copy, { recursive: true })
  assert.deepEqual(
    digestManifest(box.copy),
    digestManifest(fromApp),
    'the test copy differs from FROM_APP',
  )
  verifyBundle(appInfo(box.copy, 'isolated copy'), fromInfo.version, 'isolated copy')

  const archive = archiveFor(box, toInfo)
  const signed = signArchive(box, archive)
  const tls = openssl(box)
  const feed = updaterFeed(toInfo, signed.signature)
  const feedBytes = Buffer.from(JSON.stringify(feed))
  const archiveBytes = readFileSync(archive)
  server = createServer({ key: tls.key, cert: tls.cert }, (request, response) => {
    if (request.url === '/feed') {
      response.writeHead(200, {
        'content-type': 'application/json',
        'content-length': feedBytes.length,
      })
      response.end(feedBytes)
      return
    }
    if (request.url === '/archive') {
      response.writeHead(200, {
        'content-type': 'application/gzip',
        'content-length': archiveBytes.length,
      })
      response.end(archiveBytes)
      return
    }
    response.writeHead(404).end('not found')
  })
  server.on('tlsClientError', (error) => process.stderr.write(`updater TLS: ${error.message}\n`))
  const feedUrl = await listen(server)
  const env = smokeEnvironment(box, feedUrl, tls, signed.publicKeyPath, toInfo.version)
  app = launch(appInfo(box.copy, 'launch copy').binary, env, box.root)

  const firstBoot = await app.waitFor('first update boot', (event) => event.event === 'update-boot')
  assert.equal(
    firstBoot.data.currentVersion,
    fromInfo.version,
    'first boot did not report the copied FROM version',
  )
  assert.notEqual(firstBoot.data.currentVersion, toInfo.version)
  const firstNativePid = firstBoot.pid
  assert.ok(pidAlive(firstNativePid), 'the first native app was not alive at update boot')
  const oldLockPid = await until('first Node process owns the state lock', () => {
    const failed = app.events.find((event) =>
      ['update-failure', 'page-error', 'page-rejection'].includes(event.event),
    )
    assert.equal(failed, undefined, JSON.stringify(failed))
    const pid = lockPid(box)
    return pid !== null && pidAlive(pid) ? pid : null
  })
  assert.ok(pidAlive(oldLockPid))

  const beforeInstall = await app.waitFor(
    'blocked update install report',
    (event) => event.event === 'update-blocked',
  )
  assert.equal(beforeInstall.data.phase, 'ready')
  assert.equal(
    beforeInstall.data.blockers.length,
    2,
    'the ready snapshot did not expose both open panes',
  )
  const leadPids = await until('two fake lead processes', () => {
    const pids = [...new Set(recordedPids(box))]
    return pids.length === 2 ? pids : null
  })
  assert.ok(
    leadPids.every(pidAlive),
    `fake leads were not alive before blocked install: ${leadPids.join(',')}`,
  )
  assert.ok(pidAlive(firstNativePid), 'blocked install changed the first app process')
  assert.ok(pidAlive(oldLockPid), 'blocked install changed the state-lock owner')

  app.continueUpdate()
  await until('two fake leads close', () => leadPids.every((pid) => !pidAlive(pid)))
  const secondBoot = await app.waitFor(
    'second update boot',
    (event) => event.event === 'update-boot' && event.pid !== firstNativePid,
  )
  assert.equal(
    secondBoot.data.currentVersion,
    toInfo.version,
    'second boot did not report the installed TO version',
  )
  const restarted = await app.waitFor(
    'successful updater restart',
    (event) => event.event === 'update-restarted',
  )
  assert.equal(
    restarted.data.currentVersion,
    toInfo.version,
    'restart did not report the TO version',
  )
  assert.equal(restarted.data.blockers, 0, 'restart reported open panes')
  assert.notEqual(restarted.pid, firstNativePid, 'updater restarted in the original native process')
  assert.ok(
    pidAlive(restarted.pid),
    'the restarted native app was not alive when it reported ready',
  )
  await until('first native app exits', () => !pidAlive(firstNativePid))
  const newLockPid = await until('new Node process owns the state lock', () => {
    const pid = lockPid(box)
    return pid !== null && pid !== oldLockPid && pidAlive(pid) ? pid : null
  })
  assert.notEqual(newLockPid, oldLockPid)
  assert.ok(pidAlive(newLockPid))

  const installed = appInfo(box.copy, 'installed isolated copy')
  verifyBundle(installed, toInfo.version, 'installed isolated copy')
  assert.deepEqual(
    digestManifest(box.copy),
    targetManifest,
    'installed copy bytes do not equal TO_APP',
  )
  assert.deepEqual(digestManifest(toApp), targetManifest, 'TO_APP was mutated by the smoke')
  assert.deepEqual(
    readdirSync(dirname(box.copy)).filter((name) => name.startsWith('.consensflow-update-')),
    [],
    'the updater left staging files beside the isolated copy',
  )

  app.closeInput()
  await until('all recorded app processes exit', () =>
    [...app.appPids].every((pid) => !pidAlive(pid)),
  )
  assert.deepEqual(recordedPids(box).filter(pidAlive), [], 'a fake lead survived app shutdown')
  assert.deepEqual(
    app.events.filter((event) => event.event === 'update-failure'),
    [],
    'the successful updater smoke reported a failure',
  )
})
