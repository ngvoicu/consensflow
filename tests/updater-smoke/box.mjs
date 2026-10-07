import { execFileSync } from 'node:child_process'
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * A whole machine for one case of the updater smoke: its own HOME, its own
 * ConsensFlow home (`state`), its own PATH holding stand-ins for the harnesses,
 * the folder the installed app sits in, and a second ConsensFlow home that
 * belongs to nothing that runs. Nothing of the real machine's is read or
 * written: every case runs in its own.
 *
 * `SHELL` is absent on purpose, as in the packaged smoke: the app asks the
 * login shell for its PATH when it has one and would replace the box's with it.
 */

/** The stand-in of a harness: it says it is alive, records its pid, and reads its input for ever. */
function standIn(box, name) {
  const version = name === 'claude' ? '2.1.266' : '0.0.0'
  return `#!/bin/sh
set -eu
if [ "\${1:-}" = "--version" ]; then
  printf '${version}\\n'
  exit 0
fi
printf '%s\\n' "$$" > '${join(box.pids, name)}'-$$.pid
printf 'CFUPDATER-ALIVE %s\\n' "$$"
while IFS= read -r _line; do
  :
done
`
}

export function sandbox(parent = tmpdir()) {
  // Tauri deliberately rejects relaunch paths with symlinked ancestors;
  // macOS /var is a symlink to /private/var.
  mkdirSync(parent, { recursive: true })
  const root = realpathSync(mkdtempSync(join(parent, 'cf-updater-smoke-')))
  const box = {
    root,
    apps: join(root, 'Applications'),
    copy: join(root, 'Applications', 'ConsensFlow.app'),
    home: join(root, 'home'),
    state: join(root, 'state'),
    other: join(root, 'other-state'),
    workspace: join(root, 'workspace'),
    bin: join(root, 'bin'),
    pids: join(root, 'pids'),
    tls: join(root, 'tls'),
    probe: join(root, 'probe'),
  }
  for (const path of [
    box.apps,
    box.home,
    box.state,
    box.other,
    box.workspace,
    join(box.workspace, '.consensflow-updater-second'),
    box.bin,
    box.pids,
    box.tls,
    box.probe,
  ]) {
    mkdirSync(path, { recursive: true })
  }
  for (const name of ['claude', 'codex', 'pi', 'opencode']) {
    const path = join(box.bin, name)
    writeFileSync(path, standIn(box, name), 'utf8')
    chmodSync(path, 0o755)
  }
  return box
}

/**
 * What a terminal on the box has: the box's PATH and homes, and whichever
 * ConsensFlow home the caller names (`state` unless it says another).
 */
export function terminalEnv(box, home = box.state) {
  return {
    PATH: `${box.bin}:/usr/bin:/bin:/usr/sbin:/sbin`,
    HOME: box.home,
    TMPDIR: box.root,
    CONSENSFLOW_HOME: home,
    CLAUDE_CONFIG_DIR: join(box.home, '.claude'),
    CODEX_HOME: join(box.home, '.codex'),
    XDG_CONFIG_HOME: join(box.home, '.config'),
    PI_CODING_AGENT_DIR: join(box.home, '.pi', 'agent'),
  }
}

/**
 * What the packaged app is started with: a terminal's, and the self-test's:
 * the folder its two projects are opened in, the version it is to find when the
 * update has been installed, the feed it asks, the certificate that feed's
 * address holds, and the public key its archives are signed by (the run's).
 */
export function appEnv(box, { feed, certificate, publicKeyFile, expected, deadlineMs }) {
  return {
    ...terminalEnv(box),
    CONSENSFLOW_SELFTEST: '1',
    CONSENSFLOW_SELFTEST_DIR: box.workspace,
    CONSENSFLOW_SELFTEST_UPDATER_EXPECTED: expected,
    CONSENSFLOW_SELFTEST_UPDATER_URL: feed,
    CONSENSFLOW_SELFTEST_UPDATER_CERT: certificate,
    CONSENSFLOW_SELFTEST_UPDATER_KEY: publicKeyFile,
    CONSENSFLOW_SELFTEST_DEADLINE_MS: String(deadlineMs),
  }
}

/** The pids the stand-in harnesses wrote: one file each. */
export function recordedPids(box) {
  return readdirSync(box.pids)
    .filter((name) => name.endsWith('.pid'))
    .map((name) => Number(readFileSync(join(box.pids, name), 'utf8').trim()))
    .filter((pid) => Number.isInteger(pid) && pid > 0)
}

/**
 * A FIFO the app reads as its input, which outlives the process Tauri restarts:
 * one of its own for each start of an app, since a case may start more than one.
 */
export function makeFifo(directory) {
  const file = join(mkdtempSync(join(directory, 'control-')), 'input.fifo')
  execFileSync('/usr/bin/mkfifo', [file])
  return file
}
