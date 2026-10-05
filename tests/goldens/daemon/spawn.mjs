/**
 * `node:child_process` as the fixture that runs `cf` imports it: a run of the
 * native `cf` is written down whole (its arguments, the environment it was
 * given beyond the rig's, what was written to its standard input, what it
 * wrote to its standard output and error, how it ended). Any other program is
 * started as it would be.
 */
import * as real from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { text } from './exchange.mjs'
import * as session from './session.mjs'

export * from 'node:child_process'

/** The program a run of `cf` is: the native binary the build puts beside `cf.mjs`, unless a test names its own. */
const CF =
  process.env.CF_DAEMON_CF ??
  fileURLToPath(
    new URL(`../../../bin/${process.platform === 'win32' ? 'cf.exe' : 'cf'}`, import.meta.url),
  )

/**
 * What the rig puts in every run's environment of its own: this process's,
 * less what a window would have (`CONSENSFLOW_`, `CF_`, `CHISEL_` variables).
 */
const rig = () =>
  new Map(Object.entries(process.env).filter(([name]) => !/^(CONSENSFLOW_|CF_|CHISEL_)/.test(name)))

/** The variables a run was given, less the rig's. */
function beyond(env) {
  const base = rig()
  return Object.fromEntries(
    Object.entries(env ?? {}).filter(([name, value]) => base.get(name) !== value),
  )
}

const collect = (stream, into) =>
  stream.on('data', (chunk) => into.push(typeof chunk === 'string' ? Buffer.from(chunk) : chunk))

export function spawn(file, args, options) {
  const child = real.spawn(file, args, options)
  if (session.recording() === null || file !== CF) return child
  const step = {
    kind: 'run',
    id: session.next('run'),
    argv: args ?? [],
    env: beyond(options?.env),
    stdin: '',
    stdout: '',
    stderr: '',
    code: null,
  }
  const interval = session.begin(step, (other) => other.run === step.id)
  const written = []
  const out = []
  const err = []
  collect(child.stdout, out)
  collect(child.stderr, err)
  for (const method of ['write', 'end']) {
    const original = child.stdin[method]
    child.stdin[method] = function (chunk, ...rest) {
      if (typeof chunk === 'string' || chunk instanceof Uint8Array) written.push(Buffer.from(chunk))
      return original.call(this, chunk, ...rest)
    }
  }
  child.once('close', (code, signal) => {
    Object.assign(step, {
      stdin: text(written),
      stdout: text(out),
      stderr: text(err),
      code,
      ...(signal === null ? {} : { signal }),
    })
    session.end(interval, { run: step.id })
  })
  return child
}
