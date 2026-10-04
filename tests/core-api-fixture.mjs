import { spawn } from 'node:child_process'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { Credentials, startApi } from '../src/core/api.js'
import { openLedger } from '../src/ledger/index.js'

/**
 * The agents' API on a ledger of its own (TEST-BDC-11), for the tests of the
 * API and of `cf`: each window's token names one participant, the API decides
 * who may do what, and `cf` turns it into plain sentences and exit codes.
 */

/** The native `cf` npm run build:cf put beside cf.mjs: what a window runs. */
const CF = fileURLToPath(
  new URL(`../bin/${process.platform === 'win32' ? 'cf.exe' : 'cf'}`, import.meta.url),
)

/** `env` without anything of a window this test itself may run in. */
export const outsideAWindow = (env = process.env) =>
  Object.fromEntries(
    Object.entries(env).filter(([name]) => !/^(CONSENSFLOW_|CF_|CHISEL_)/.test(name)),
  )

/**
 * The native `cf` with `args`, `env` added to a clean environment and `input`
 * on its standard input: its exit code, and what it printed less the final
 * line break. Spawned, not run synchronously: the API it calls answers from
 * this same process.
 */
export function runCf(args, env, input = '') {
  return new Promise((resolve, reject) => {
    const child = spawn(CF, args, {
      env: { ...outsideAWindow(), ...env },
      stdio: ['pipe', 'pipe', 'pipe'],
    })
    let out = ''
    let err = ''
    child.stdout.setEncoding('utf8').on('data', (chunk) => {
      out += chunk
    })
    child.stderr.setEncoding('utf8').on('data', (chunk) => {
      err += chunk
    })
    child.on('error', reject)
    child.on('close', (code) =>
      resolve({ code, out: out.replace(/\n$/, ''), err: err.replace(/\n$/, '') }),
    )
    // A command that reads no input may have ended before it is written.
    child.stdin.on('error', () => {})
    child.stdin.end(input)
  })
}

export const participantId = (ledger, project, handle) =>
  ledger.project(project.id).participants.find((p) => p.handle === handle).id

export async function withApi(fn) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-core-api-'))
  const ledger = openLedger(path.join(dir, 'consensflow.db'))
  const credentials = new Credentials()
  let changes = 0
  const api = await startApi({
    ledger,
    credentials,
    changed: () => changes++,
    roster: (agent) =>
      agent === 'zeus'
        ? { id: 'zeus', kind: 'claude-code', model: 'claude-sonnet-5', effort: 'high' }
        : null,
  })
  const project = ledger.createProject({
    directory: '/work/app',
    name: 'app',
    chief: { harness: 'claude-code' },
  })
  ledger.addMember(project.id, {
    agent: 'zeus',
    harness: 'claude-code',
    role: 'worker',
    tier: 'standard',
  })
  const participant = (handle) =>
    ledger.project(project.id).participants.find((p) => p.handle === handle)
  const token = (handle) => credentials.issue({ participant: participant(handle), project })
  const call = async (who, method, route, body) => {
    const response = await fetch(`${api.url}${route}`, {
      method,
      headers: { authorization: `Bearer ${who}`, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    })
    return { status: response.status, body: await response.json() }
  }
  // A window's `cf`, as its participant: the native binary, against this API.
  const cf = (who, ...args) => {
    const options = typeof args.at(-1) === 'object' ? args.pop() : {}
    return runCf(args, { CONSENSFLOW_URL: api.url, CONSENSFLOW_TOKEN: who }, options.input)
  }
  try {
    await fn({ ledger, project, token, call, cf, credentials, changes: () => changes })
  } finally {
    await api.close()
    ledger.close()
    await rm(dir, { recursive: true, force: true })
  }
}

export const deliver = (ledger, message) => {
  ledger.beginDelivery(message.id)
  ledger.confirmDelivery(message.id, { item: 'test' })
}
