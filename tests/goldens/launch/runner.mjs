/**
 * Plays a launch scenario against Node's adapters and records what each
 * step did: the oracle `crates/cf-harness` is held to, step by step, by the
 * Rust player (`crates/cf-harness/tests/launch/scenarios.rs`).
 *
 * A scenario is data. Some steps set the scene and record nothing: stand-in
 * CLIs (`executable`; `node: true` for one a window starts), files, a
 * harness's own status files, what the engine tells a
 * window (`opened`, `follow`), and whether looks at a harness's record
 * wait to be released (`holdLooks`). The others are recorded:
 * - `prepare`, `observe`, `ready`, `deliver`, `started` begin that work on
 *   the adapter or its window, through a pane host that answers each
 *   request as the step scripts it (`answers`), at once or held
 *   (`{held: true}`), for the pane `p1-zeus` of generation 1 unless the
 *   step names its `pane`, and a peer on loopback that answers each `fetch`
 *   by its route as the step scripts it (`served`, `scriptedPeer`), and
 *   programs started as it scripts them (`children`), a prepare's too;
 * - `release` answers a held host request (`release: op, answer`), a held
 *   fetch (`release: 'GET /session', answer`) or a held look
 *   (`release: 'look'`); `releaseBody` ends a held body (`releaseBody:
 *   route, body`);
 * - `advance` moves the clock by that many milliseconds, firing the timers
 *   due on the way one time at a time;
 * - `close` closes the window as the engine does: its launch's files go,
 *   and the work still waiting on it keeps its hold.
 * After each, the work begun runs until it settles or waits on something a
 * step controls: a held request, a held look, a timer. What it waited on
 * the machine for (file work, a turn of the loop) is done by then.
 *
 * Nothing waits on the machine: the clock (`Date.now`, timers, the timers
 * of `node:timers/promises` and `AbortSignal.timeout`) starts at
 * 2026-09-19T12:00:00Z and moves only when a step advances it; randomness
 * (`randomUUID`, `randomBytes`) is the stream the Rust fakes hand out, byte
 * `i` being `(i * 7 + 3) % 256`; a free port on loopback is the next from
 * 41000 up, as Rust's `FixedPorts` hands them out; and `fetch` is the
 * scripted peer. A timeout signal's timer stands while a request or a body
 * waits under it, and is cleared once none does, as Rust's armed timer
 * goes with the waits it bounded. What an adapter makes with the system's
 * default modes comes out as the mask 022 leaves it, on every machine.
 *
 * Timers fire when due, one at a time, the first armed of those due
 * together first, each in a turn of the loop of its own, and the work runs
 * until it holds still before the next fires, its file work done: Rust does
 * file work where it is asked for, so no timer fires while it goes on
 * (`ManualTime` in `crates/cf-harness/src/testing.rs`), and here it takes
 * no time on the clock. Node itself fires timers due together in one go,
 * the continuations of each run before the next but nothing else, and
 * orders those of different lengths by its timer lists. A scenario where
 * that would differ is refused: timers of different lengths due together,
 * or a work's file request or `setImmediate` landing while a timer due with
 * the one fired last still waits. One difference stays, on purpose: on a
 * slow disk Node may fire a later timer while file work is in flight, and
 * Rust never does.
 *
 * A step's record is written so that it is the same on every run: every
 * path under the root is `$ROOT/…` (the root is as the system names it, so
 * a folder's real name is under it too), in a file URL, a URL's query and
 * JSON text as well, and the hash that names OpenCode's bundle `$HASH`; the
 * process this runs as is `$PID`,
 * one long dead `$DEAD`, a second live one a scenario names `$OTHER`; and
 * it lists the work that settled (its step, and what it answered or
 * threw), the work still waiting, the requests asked of the host and, when
 * there were any, of the peer (`fetches`), the size of every random draw,
 * and what the step did to the tree under the root.
 *
 * A step may carry `kept`: a difference Rust keeps from Node on purpose,
 * why, and how Rust's own work settles instead.
 */
import { AsyncLocalStorage, createHook } from 'node:async_hooks'
import { EventEmitter } from 'node:events'
import { existsSync, lstatSync, readdirSync, readFileSync, readlinkSync, statSync } from 'node:fs'
import fs from 'node:fs/promises'
import { createRequire, syncBuiltinESMExports } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import { PassThrough } from 'node:stream'
import { pathToFileURL } from 'node:url'
import { cachedAnswers } from '../../../hosts/lib/completion.js'
import { claudeCodeAdapter } from '../../../src/adapters/claude-code.js'
import { openCodeAdapter } from '../../../src/adapters/opencode.js'
import { piAdapter } from '../../../src/adapters/pi.js'
import { forgetLaunch } from '../../../src/core/launch-files.js'
import { fakeExecutable, fakeNodeExecutable } from '../../helpers.mjs'

const require = createRequire(import.meta.url)
const childProcess = require('node:child_process')
const crypto = require('node:crypto')
const net = require('node:net')
const timersPromises = require('node:timers/promises')

/** A process id no process has: macOS's pids stop below it, and Windows' are multiples of four. */
export const DEAD = 999_999

/** When a scenario's clock starts. */
const EPOCH = Date.parse('2026-09-19T12:00:00Z')

/** The longest a Node timer waits; it takes any other length as 1 ms. */
const TIMEOUT_MAX = 2 ** 31 - 1

/**
 * The async resources a wait on the machine makes, one each, from its start
 * until its callback or promise has run, probed on Node v26.8.1: a file
 * request's (`FSREQCALLBACK`, `FSREQPROMISE`, and `FILEHANDLECLOSEREQ` for a
 * file handle's close) and a `setImmediate`'s. A watcher's would last, and
 * none is used.
 */
const MACHINE_WAITS = new Set(['FSREQCALLBACK', 'FSREQPROMISE', 'FILEHANDLECLOSEREQ', 'Immediate'])

const ADAPTERS = { 'claude-code': claudeCodeAdapter, opencode: openCodeAdapter, pi: piAdapter }
const WINDOWS = process.platform === 'win32'

/** The mask files and folders are made under: the one nearly every machine has. */
const UMASK = 0o022

/** The machine's own timers, which the runner turns the loop with. */
const real = {
  setTimeout: globalThis.setTimeout,
  setImmediate: globalThis.setImmediate,
  spawn: childProcess.spawn,
  spawnSync: childProcess.spawnSync,
}

/** A path under the root with Windows' separators as POSIX's; on POSIX a backslash is a name's own. */
const posix = (text) => (WINDOWS ? text.replaceAll('\\', '/') : text)

/**
 * What the work begun waits on the machine for, which no step controls:
 * file work on Node's thread pool, and turns of the loop. Only a work's own
 * count, made in its async context (`owned`): the runner's turns do not.
 * How many are in flight, how many ever began, and `lands` told as each
 * one's callback or promise is about to run.
 */
function machineWork(owned, lands) {
  const inFlight = new Set()
  let begun = 0
  const hook = createHook({
    init(id, type) {
      if (!MACHINE_WAITS.has(type) || !owned()) return
      inFlight.add(id)
      begun += 1
    },
    before(id) {
      if (inFlight.has(id)) lands()
    },
    after: (id) => inFlight.delete(id),
    destroy: (id) => inFlight.delete(id),
  }).enable()
  return { inFlight: () => inFlight.size, begun: () => begun, stop: () => hook.disable() }
}

/**
 * A clock that moves only when told, and its timers, each the work of the
 * step that armed it (`owner`), its callback run in the async context it
 * was armed in, as Node runs it.
 */
function fakeClock(owner) {
  const timers = []
  let now = EPOCH
  let next = 1
  // When the timer fired last was due, and whether a work's wait on the
  // machine landed while another due then still waited: Node would have
  // fired that one first.
  let firing = null
  let raced = false
  const refuseRace = () => {
    if (raced) {
      throw new Error(
        "a work's file request or turn of the loop landed between two timers due together: Node fires the second before it lands",
      )
    }
  }
  const setTimeout = (callback, delay, ...args) => {
    const asked = Number(delay)
    const ms = asked >= 1 && asked <= TIMEOUT_MAX ? Math.trunc(asked) : 1
    const timer = {
      id: next++,
      due: now + ms,
      ms,
      callback: AsyncLocalStorage.bind(callback),
      args,
      work: owner(),
    }
    timers.push(timer)
    const handle = { unref: () => handle, ref: () => handle, hasRef: () => true, id: timer.id }
    handle[Symbol.toPrimitive] = () => timer.id
    return handle
  }
  const clearTimeout = (handle) => {
    const at = timers.findIndex((timer) => timer.id === (handle?.id ?? handle))
    if (at >= 0) timers.splice(at, 1)
  }
  return {
    now: () => now,
    setTimeout,
    clearTimeout,
    /** How long until each timer `work` armed is due, in the order armed. */
    waits: (work) => timers.filter((timer) => timer.work === work).map((timer) => timer.due - now),
    /** A wait on the machine landing: a race, if a timer due with the one fired last still waits. */
    machineLands() {
      if (timers.some((timer) => timer.due === firing)) raced = true
    },
    /**
     * Fires the timer due first, if by `until`, the one armed first of those
     * due together: whether there was one. The runner lets the work hold
     * still before the next. Throws where Node would fire them otherwise.
     */
    fireNext(until) {
      refuseRace()
      if (timers.length === 0) return false
      const due = Math.min(...timers.map((timer) => timer.due))
      if (due > until) return false
      const together = timers.filter((timer) => timer.due === due)
      if (new Set(together.map((timer) => timer.ms)).size > 1) {
        const lengths = together.map((timer) => `${timer.ms} ms`).join(', ')
        throw new Error(
          `timers of different lengths due together (${lengths}): Node orders them by its timer lists, not as armed`,
        )
      }
      now = Math.max(now, due)
      const [timer] = together
      timers.splice(timers.indexOf(timer), 1)
      firing = due
      timer.callback(...timer.args)
      return true
    },
    settleAt(until) {
      refuseRace()
      now = Math.max(now, until)
    },
  }
}

/** Puts the scenario's clock and randomness where the adapters read them; what puts them back. */
function install(context) {
  const clock = context.clock
  const saved = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    now: Date.now,
    abortTimeout: AbortSignal.timeout,
    abortAny: AbortSignal.any,
    fetch: globalThis.fetch,
    createServer: net.createServer,
    promisedTimeout: timersPromises.setTimeout,
    randomUUID: crypto.randomUUID,
    randomBytes: crypto.randomBytes,
  }
  globalThis.setTimeout = clock.setTimeout
  globalThis.clearTimeout = clock.clearTimeout
  Date.now = clock.now
  // A timeout signal's timer, and the signals a signal is made of: a wait
  // under a signal holds every timeout it comes from (`context.signals`).
  const timeouts = new Map()
  const sources = new Map()
  AbortSignal.timeout = (ms) => {
    const controller = new AbortController()
    const handle = clock.setTimeout(
      () =>
        controller.abort(
          new DOMException('The operation was aborted due to timeout', 'TimeoutError'),
        ),
      ms,
    )
    timeouts.set(controller.signal, { handle, waits: 0 })
    return controller.signal
  }
  AbortSignal.any = (signals) => {
    const made = saved.abortAny.call(AbortSignal, signals)
    sources.set(made, [...signals])
    return made
  }
  context.signals = {
    /** A wait under `signal` begun: what ends it, clearing each timeout no wait is under now. */
    hold(signal) {
      const held = []
      const walk = (each) => {
        if (timeouts.has(each)) held.push(timeouts.get(each))
        for (const source of sources.get(each) ?? []) walk(source)
      }
      if (signal !== undefined && signal !== null) walk(signal)
      for (const timeout of held) timeout.waits += 1
      let over = false
      return () => {
        if (over) return
        over = true
        for (const timeout of held) {
          timeout.waits -= 1
          if (timeout.waits === 0) clock.clearTimeout(timeout.handle)
        }
      }
    },
  }
  globalThis.fetch = (input, init) => context.peer.fetch(input, init)
  net.createServer = () => portServer(context)
  childProcess.spawn = (file, args = [], options = {}) =>
    context.children.spawn(file, args, options)
  childProcess.spawnSync = (file, args = [], options = {}) =>
    context.children.spawnSync(file, args, options)
  timersPromises.setTimeout = (ms, value) =>
    new Promise((resolve) => clock.setTimeout(() => resolve(value), ms))
  let drawn = 0
  const take = (count) => {
    const bytes = Buffer.alloc(count)
    for (let index = 0; index < count; index += 1) bytes[index] = ((drawn + index) * 7 + 3) % 256
    drawn += count
    context.draws.push(count)
    return bytes
  }
  crypto.randomBytes = (count) => take(count)
  crypto.randomUUID = () => {
    const bytes = take(16)
    bytes[6] = (bytes[6] & 0x0f) | 0x40
    bytes[8] = (bytes[8] & 0x3f) | 0x80
    const hex = bytes.toString('hex')
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
  }
  syncBuiltinESMExports()
  return () => {
    globalThis.setTimeout = saved.setTimeout
    globalThis.clearTimeout = saved.clearTimeout
    Date.now = saved.now
    AbortSignal.timeout = saved.abortTimeout
    AbortSignal.any = saved.abortAny
    globalThis.fetch = saved.fetch
    net.createServer = saved.createServer
    childProcess.spawn = real.spawn
    childProcess.spawnSync = real.spawnSync
    timersPromises.setTimeout = saved.promisedTimeout
    crypto.randomUUID = saved.randomUUID
    crypto.randomBytes = saved.randomBytes
    syncBuiltinESMExports()
  }
}

/** The process ids a scenario names, by their names. */
function pids(context) {
  const other = context.other === null ? {} : { $OTHER: context.other.pid }
  return { $PID: process.pid, $DEAD: DEAD, ...other }
}

/** `$ROOT/a/b` as a path under `root`, a named process as its id. */
function realValue(context, value) {
  if (Array.isArray(value)) return value.map((item) => realValue(context, item))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, realValue(context, item)]),
    )
  }
  if (typeof value !== 'string') return value
  if (Object.hasOwn(pids(context), value)) return pids(context)[value]
  if (!value.startsWith('$ROOT')) return value
  return path.join(context.root, ...value.slice('$ROOT'.length).split('/').filter(Boolean))
}

/** A file's text with the named processes in it: JSON that no object writes. */
function realText(context, text) {
  let filled = text
  for (const [name, pid] of Object.entries(pids(context)))
    filled = filled.replaceAll(name, `${pid}`)
  return filled
}

/**
 * The ways the root reads in the text a step records besides as itself: as a
 * file URL, as JSON writes it (Windows' backslashes escaped), and as a URL's
 * query holds it (`encodeURIComponent`; that with the parser's `%27` for a
 * quote; and a form's, with `+` for a space).
 */
function rootForms(root) {
  const component = encodeURIComponent(root)
  return {
    fileUrl: pathToFileURL(root).href,
    plain: [
      root,
      JSON.stringify(root).slice(1, -1),
      component,
      component.replaceAll("'", '%27'),
      new URLSearchParams({ d: root }).toString().slice(2),
    ],
  }
}

/**
 * OpenCode's bundle is named by the hash of its files and of its generated
 * `tui.json`, which names the folder it is published in, the root among its
 * parents: the hash is not the same in two runs, and is written `$HASH`.
 * (Its being what Node makes of those files is held where the bundle is,
 * `shared::private_bundle`.)
 */
const BUNDLE_HASH = /(extensions[\\/]opencode[\\/])[0-9a-f]{64}/g

/** What a step recorded, with the root and the live processes written as the scenario writes them. */
function written(context, value) {
  if (Array.isArray(value)) return value.map((item) => written(context, item))
  if (value !== null && typeof value === 'object') {
    // A key beginning with `$` gets another: the runner's own (`$utf16`) stand apart.
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key.startsWith('$') ? `$${key}` : key,
        written(context, item),
      ]),
    )
  }
  if (typeof value === 'number') {
    const live = Object.entries(pids(context)).find(
      ([name, pid]) => name !== '$DEAD' && pid === value,
    )
    return live === undefined ? value : live[0]
  }
  if (typeof value !== 'string') return value
  let text = value.replaceAll(context.rootForms.fileUrl, 'file://$ROOT')
  for (const form of context.rootForms.plain) text = text.replaceAll(form, '$ROOT')
  if (text.startsWith('$ROOT')) text = posix(text)
  text = text.replace(BUNDLE_HASH, '$1$$HASH')
  // Half a surrogate pair is no text every JSON reader holds, and Rust's
  // strings never do: such a string is written as its UTF-16 code units.
  if (!text.isWellFormed()) {
    return { $utf16: Array.from({ length: text.length }, (_, at) => text.charCodeAt(at)) }
  }
  return text
}

/**
 * Every file, folder and link under `root`, by its path there: a file's and
 * a folder's mode, a file's text, a link's target, never followed.
 */
function tree(root) {
  const found = new Map()
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const full = path.join(folder, entry.name)
      const relative = posix(path.relative(root, full))
      if (lstatSync(full).isSymbolicLink()) {
        found.set(relative, { link: readlinkSync(full) })
        continue
      }
      const mode = WINDOWS ? null : statSync(full).mode & 0o777
      if (entry.isDirectory()) {
        found.set(relative, { mode })
        walk(full)
      } else {
        found.set(relative, { mode, text: readFileSync(full, 'utf8') })
      }
    }
  }
  if (existsSync(root)) walk(root)
  return found
}

/** What a step did to the tree: each path made, changed or removed, in order. */
function changes(before, after) {
  const made = []
  for (const relative of new Set([...before.keys(), ...after.keys()])) {
    const was = before.get(relative)
    const entry = after.get(relative)
    if (entry === undefined) made.push({ path: `$ROOT/${relative}`, removed: true })
    else if (JSON.stringify(was) !== JSON.stringify(entry)) {
      made.push({ path: `$ROOT/${relative}`, ...entry })
    }
  }
  return made.sort((left, right) => (left.path < right.path ? -1 : left.path > right.path ? 1 : 0))
}

/** A host's response as a scenario writes it, or the error it throws (`{throws: message}`). */
function respond(context, given) {
  if (given?.throws !== undefined) throw Object.assign(new Error(given.throws), given)
  return realValue(context, given)
}

/**
 * A pane host that answers each request with the next answer scripted for
 * its operation, at once or held until released, and writes down what it
 * was asked. A request with no answer left fails.
 */
function scriptedHost(context) {
  const left = new Map()
  const held = []
  return {
    script(answers) {
      for (const [op, list] of Object.entries(answers ?? {})) {
        left.set(op, [...(left.get(op) ?? []), ...list])
      }
    },
    async request(op, body) {
      context.requests.push(written(context, { op, body }))
      const answer = left.get(op)?.shift()
      if (answer === undefined) throw new Error(`no answer for ${op}`)
      if (answer?.held === true) {
        const work = context.als.getStore()
        return new Promise((resolve, reject) => held.push({ op, work, resolve, reject }))
      }
      return respond(context, answer)
    },
    release(op, answer) {
      const at = held.findIndex((each) => each.op === op)
      if (at < 0) return false
      const [request] = held.splice(at, 1)
      try {
        request.resolve(respond(context, answer))
      } catch (cause) {
        request.reject(cause)
      }
      return true
    },
    held: () => held.length,
    waits: (work) => held.filter((each) => each.work === work).map((each) => each.op),
    unused(optional = []) {
      return [...left]
        .filter(([op, list]) => list.length > 0 && !optional.includes(op))
        .map(([op]) => op)
        .sort()
    },
  }
}

/** The looks at a harness's record, as the engine serves them, held until released when the scene says so. */
function scriptedLooks(context) {
  const answers = cachedAnswers()
  const held = []
  let hold = false
  return {
    answers: (...args) =>
      hold
        ? new Promise((resolve, reject) =>
            held.push({
              work: context.als.getStore(),
              // Released in the async context of the work that looked.
              release: AsyncLocalStorage.bind(() => answers(...args).then(resolve, reject)),
            }),
          )
        : answers(...args),
    hold(value) {
      hold = value
    },
    release() {
      const look = held.shift()
      look?.release()
      return look !== undefined
    },
    held: () => held.length,
    waits: (work) => held.filter((each) => each.work === work).length,
  }
}

/** The statuses a response has no body with. */
const NULL_BODY = new Set([101, 204, 205, 304])

/**
 * A peer on loopback the scene scripts, the twin of Rust's
 * `ScriptedLoopback` (`crates/cf-harness/src/testing/peer.rs`): `fetch`
 * answered by its route (`GET /session`: the method and the URL's path)
 * with the next answer scripted for it (`served`): a head at once
 * (`{status, body}`, the body its text, `{json: value}` as JSON with the
 * paths in it under the root (`$ROOT/…`) made whole, `{repeat: text, times}`
 * as that text that many times, `{held: true}` until a step ends it, or
 * `{cut: true}`), none (`{noHead: true}`), or held until a step releases
 * it (`{held: true}`). Each request is written down as its caller wrote it.
 * The failures are undici's (probed on Node v26.8.1): no head is `TypeError:
 * fetch failed`, a body broken off `TypeError: terminated`, and an abort of
 * the request's signal rejects the head or errors the body still coming
 * with the signal's own reason. A request with no answer left gets no head.
 */
function scriptedPeer(context) {
  const left = new Map()
  const heads = []
  const bodies = []
  const drop = (list, entry) => list.splice(list.indexOf(entry), 1)
  const failed = () => new TypeError('fetch failed')
  // The body as a stream that says when its reader is done with it.
  const response = (route, answer, signal, work, over) => {
    if (NULL_BODY.has(answer.status)) {
      over()
      return new Response(null, { status: answer.status })
    }
    let controller
    let rest = null
    const stream = new ReadableStream({
      start: (made) => {
        controller = made
      },
      // A stream pulls again from inside `enqueue`: the bytes are taken first.
      pull: () => {
        if (rest === null) return
        if (rest.length > 0) {
          const bytes = rest
          rest = new Uint8Array(0)
          controller.enqueue(bytes)
          return
        }
        controller.close()
        over()
      },
      cancel: over,
    })
    const body = answer.body
    if (typeof body === 'string') rest = new TextEncoder().encode(body)
    else if (body?.json !== undefined) {
      rest = new TextEncoder().encode(JSON.stringify(realValue(context, body.json)))
    } else if (body?.repeat !== undefined) {
      rest = new TextEncoder().encode(body.repeat.repeat(body.times))
    } else if (body?.cut === true) {
      controller.error(new TypeError('terminated'))
      over()
    } else {
      const entry = {
        route,
        work,
        send: (text) => {
          rest = new Uint8Array(0)
          controller.enqueue(new TextEncoder().encode(text))
        },
        cut: (cause) => {
          controller.error(cause)
          over()
        },
      }
      bodies.push(entry)
      signal?.addEventListener(
        'abort',
        () => {
          if (bodies.includes(entry)) {
            drop(bodies, entry)
            entry.cut(signal.reason)
          }
        },
        { once: true },
      )
    }
    return new Response(stream, { status: answer.status })
  }
  return {
    script(served) {
      for (const [route, list] of Object.entries(served ?? {})) {
        left.set(route, [...(left.get(route) ?? []), ...list])
      }
    },
    async fetch(input, init = {}) {
      const url = String(input instanceof Request ? input.url : input)
      const method = String(init.method ?? 'GET').toUpperCase()
      const route = `${method} ${new URL(url).pathname}`
      context.fetches.push(
        written(context, {
          route,
          url,
          headers: Object.entries(init.headers ?? {}),
          body: init.body ?? null,
        }),
      )
      const signal = init.signal
      signal?.throwIfAborted()
      const answer = left.get(route)?.shift()
      const over = context.signals.hold(signal)
      if (answer === undefined || answer.noHead === true) {
        // A request that fails has nothing left waiting under its timeout.
        over()
        throw failed()
      }
      const work = context.als.getStore()
      if (answer.held !== true) return response(route, answer, signal, work, over)
      return new Promise((resolve, reject) => {
        const entry = {
          route,
          work,
          answer: (given) => {
            if (given?.noHead === true) {
              over()
              reject(failed())
            } else resolve(response(route, given, signal, work, over))
          },
        }
        heads.push(entry)
        signal?.addEventListener(
          'abort',
          () => {
            if (heads.includes(entry)) {
              drop(heads, entry)
              over()
              reject(signal.reason)
            }
          },
          { once: true },
        )
      })
    },
    /** Answers the request to `route` held longest: whether one was. */
    release(route, answer) {
      const entry = heads.find((each) => each.route === route)
      if (entry === undefined) return false
      drop(heads, entry)
      entry.answer(answer)
      return true
    },
    /** Ends the body held longest of a reply to `route`, its text or `{cut: true}`: whether one was. */
    releaseBody(route, body) {
      const entry = bodies.find((each) => each.route === route)
      if (entry === undefined) return false
      drop(bodies, entry)
      if (body?.cut === true) entry.cut(new TypeError('terminated'))
      else entry.send(body)
      return true
    },
    /** The routes `work` waits on, a head or a body held, in the order asked. */
    waits: (work) =>
      [...heads, ...bodies].filter((entry) => entry.work === work).map((entry) => entry.route),
    unused(optional = []) {
      return [...left]
        .filter(([route, list]) => list.length > 0 && !optional.includes(route))
        .map(([route]) => route)
        .sort()
    },
  }
}

/**
 * What `net.createServer` gives `freeLoopbackPort` (`src/channels.js`): a
 * server that listens on the next free port from 41000 up, as Rust's
 * `FixedPorts` hands them out, and on nothing at all.
 */
function portServer(context) {
  let port = null
  const server = {
    once: () => server,
    listen: (_port, _host, listening) => {
      port = context.ports.shift()
      queueMicrotask(listening)
      return server
    },
    address: () => ({ address: '127.0.0.1', family: 'IPv4', port }),
    close: (closed) => {
      queueMicrotask(() => closed?.())
      return server
    },
  }
  return server
}

/**
 * A program a step scripts in place of `spawn`'s child, the twin of Rust's
 * `ScriptedProcesses` (`crates/cf-harness/src/testing/children.rs`): it
 * writes its scripted lines, keeps every line it is sent, and ends as its
 * script says (`itself` after its lines, `asked` when asked or forced,
 * `forced` only when forced, `never`; on Windows, where an end is always
 * forced, it ends). Its streams are those `stdio` asks for, its output
 * open until it ends, and its events are a Node child's (probed on Node
 * v26.8.1): `spawn`, then `exit` and `close` with its code, none after a
 * signal; one asked to end after its exit is not asked (`kill` says false).
 * One that is not there says `error`, then `close` with -2, and no `exit`.
 */
class ScriptedChild extends EventEmitter {
  #ends

  constructor(context, script, stdio) {
    super()
    this.#ends = script.ends ?? 'asked'
    this.pid = context.pids.shift()
    this.exitCode = null
    this.signalCode = null
    this.killed = false
    const [input, output, errors] = Array.isArray(stdio) ? stdio : [stdio, stdio, stdio]
    this.stdin = input === 'pipe' || input === undefined ? new PassThrough() : null
    this.stdout = output === 'pipe' || output === undefined ? new PassThrough() : null
    this.stderr = errors === 'pipe' || errors === undefined ? new PassThrough() : null
    let unended = ''
    this.stdin?.on('data', (chunk) => {
      unended += chunk
      for (let at = unended.indexOf('\n'); at >= 0; at = unended.indexOf('\n')) {
        context.written.push(unended.slice(0, at))
        unended = unended.slice(at + 1)
      }
    })
    if (script.missing === true) {
      queueMicrotask(() => {
        this.emit('error', script.failed)
        this.exitCode = -2
        this.emit('close', -2, null)
      })
      return
    }
    for (const line of script.lines ?? []) this.stdout?.write(`${line}\n`)
    queueMicrotask(() => {
      this.emit('spawn')
      if (this.#ends === 'itself') this.#exit(0, null)
    })
  }

  kill(signal = 'SIGTERM') {
    if (this.exitCode !== null || this.signalCode !== null) return false
    const forced = signal === 'SIGKILL'
    const ends =
      WINDOWS ||
      this.#ends === 'itself' ||
      this.#ends === 'asked' ||
      (this.#ends === 'forced' && forced)
    this.killed = true
    if (ends) this.#exit(null, signal)
    return true
  }

  /** It exits, its streams close, and it says `exit`, then `close` once they have. */
  #exit(code, signal) {
    if (this.exitCode !== null || this.signalCode !== null) return
    if (signal === null) this.exitCode = code
    else this.signalCode = signal
    const open = [this.stdout, this.stderr].filter((stream) => stream !== null)
    // Listening before the streams end: asked to end from a timer's callback,
    // a stream says it has ended in a tick that comes before a microtask.
    const ended = Promise.all(open.map((stream) => new Promise((done) => stream.once('end', done))))
    for (const stream of open) {
      stream.end()
      if (stream.listenerCount('data') === 0) stream.resume()
    }
    queueMicrotask(() => {
      this.emit('exit', this.exitCode, this.signalCode)
      ended.then(() => this.emit('close', this.exitCode, this.signalCode))
    })
  }

  /**
   * `taskkill /PID <pid> /T /F`, as `terminate` asks it on Windows: it ends
   * any process, and Node, which sent no signal, reads the exit code 1 the
   * system gives it.
   */
  forced() {
    this.#exit(1, null)
  }
}

/**
 * The programs a step scripts for `spawn` (`children`: by the program's
 * name, as its stand-in is named), each started one kept as Rust's fake
 * keeps it: its name and its arguments. On Windows a stand-in runs as its
 * shim's node and script (`runnable`), so the script names it.
 */
function scriptedChildren(context) {
  const left = new Map()
  const started = []
  const named = (file, args) => {
    const shim = WINDOWS && /\.mjs$/i.test(args[0] ?? '')
    if (WINDOWS && !shim) {
      throw new Error(`a spawned stand-in runs as a JavaScript one on Windows: ${file}`)
    }
    const [program, rest] = shim ? [args[0], args.slice(1)] : [file, args]
    return [path.basename(program).replace(/\.(mjs|cmd|bat|exe)$/i, ''), ...rest]
  }
  return {
    script(children) {
      for (const [name, list] of Object.entries(children ?? {})) {
        left.set(name, [...(left.get(name) ?? []), ...list])
      }
    },
    spawn(file, args, options) {
      const [name, ...rest] = named(file, args)
      const failed = Object.assign(new Error(`spawn ${file} ENOENT`), { code: 'ENOENT' })
      const script = left.get(name)?.shift() ?? { missing: true, failed }
      const child = new ScriptedChild(context, script, options.stdio)
      // One that is not there is not started, as Rust's fake keeps it.
      if (script.missing !== true) context.spawned.push([name, ...rest].join(' '))
      started.push(child)
      return child
    },
    spawnSync(file, args, options) {
      if (path.basename(file).toLowerCase().startsWith('taskkill')) {
        const pid = Number(args[args.indexOf('/PID') + 1])
        started.find((child) => child.pid === pid)?.forced()
        return { status: 0, signal: null, output: [], stdout: '', stderr: '' }
      }
      return real.spawnSync(file, args, options)
    },
    unused() {
      return [...left]
        .filter(([, list]) => list.length > 0)
        .map(([name]) => name)
        .sort()
    },
  }
}

/** A live process of the scenario's own besides this one: `$OTHER`. */
function startOther() {
  return WINDOWS
    ? real.spawn('ping', ['-n', '600', '127.0.0.1'], { stdio: 'ignore' })
    : real.spawn('sleep', ['600'], { stdio: 'ignore' })
}

/** Sets the scene as a step says: false for a step that is recorded. */
async function setUp(context, step) {
  if (step.executable !== undefined) {
    const file = path.join(context.root, 'bin', step.executable)
    // One a window starts (`node: true`) runs on Windows as its shim's node
    // and script, which the shim of an npm-installed CLI names (`runnable`).
    if (step.node === true) fakeNodeExecutable(file, '')
    else fakeExecutable(file)
    return true
  }
  if (step.write !== undefined) {
    const file = realValue(context, step.write)
    await fs.mkdir(path.dirname(file), { recursive: true })
    await fs.writeFile(file, realValue(context, step.text))
    return true
  }
  if (step.remove !== undefined) {
    await fs.rm(realValue(context, step.remove), { force: true })
    return true
  }
  if (step.status !== undefined || step.statusText !== undefined) {
    // Claude's own status of a live process (`sessions/<pid>.json`), or a
    // file of its by another name, or its text where JSON cannot hold it.
    const folder = path.join(context.env.CLAUDE_CONFIG_DIR, 'sessions')
    await fs.mkdir(folder, { recursive: true })
    if (step.statusText !== undefined) {
      await fs.writeFile(path.join(folder, step.file), realText(context, step.statusText))
      return true
    }
    const { pid, ...fields } = realValue(context, step.status)
    const file = step.file ?? `${pid}.json`
    await fs.writeFile(path.join(folder, file), JSON.stringify({ pid, ...fields }))
    return true
  }
  if (step.opened !== undefined) {
    // What the engine writes into a window's launch once its pane opened.
    const { pid } = realValue(context, step.opened)
    if (pid !== undefined) context.launch.pid = pid
    return true
  }
  if (step.follow !== undefined) {
    context.launch.nativeSession = realValue(context, step.follow)
    return true
  }
  if (step.holdLooks !== undefined) {
    context.looks.hold(step.holdLooks === true)
    return true
  }
  return false
}

/** Begins the work a step asks of the adapter or its window. */
function begin(context, index, step) {
  // Every wait the work makes is its own: the timers, requests and looks
  // it starts are told whose they are by the async context it runs in.
  context.als.run(index, () => beginOwn(context, index, step))
}

function beginOwn(context, index, step) {
  const settle = (work, written) =>
    work.then(
      (value) => ({ answer: written(value) }),
      (cause) => ({ throws: cause.message }),
    )
  // What the work asks of the host, the peer and the programs is answered as
  // the step scripts it, a prepare's (a window's first conversation is made on
  // a server of its own) as well as the others'.
  context.host.script(step.answers)
  context.peer.script(step.served)
  context.children.script(step.children)
  let work
  if (step.prepare !== undefined) {
    const launch = realValue(context, step.prepare)
    context.launchId = launch.launchId
    work = settle(context.adapter.prepare(launch), (plan) => {
      context.launch = plan.launch
      // The launch bag is the window's own state, which its later steps show.
      return {
        argv: plan.argv,
        env: Object.entries(plan.env),
        dropEnv: plan.dropEnv,
        nativeSession: plan.nativeSession ?? null,
      }
    })
  } else {
    const target = {
      launch: context.launch,
      pane: step.pane ?? { id: 'p1-zeus', generation: 1 },
      host: context.host,
    }
    const same = (value) => value ?? { undefined: true }
    if (step.observe !== undefined)
      work = settle(context.adapter.observe({ ...target, conversation: null }), same)
    else if (step.ready !== undefined) work = settle(context.adapter.ready(target), same)
    else if (step.deliver !== undefined)
      work = settle(context.adapter.deliver({ ...target, text: step.deliver }), same)
    else if (step.started !== undefined)
      work = settle(context.adapter.started(target), (started) => started.nativeSession ?? null)
    else throw new Error(`a step of no kind: ${JSON.stringify(step)}`)
  }
  const entry = { op: index, settled: null }
  work.then((outcome) => {
    entry.settled = outcome
  })
  context.work.push(entry)
}

/** The work still waiting, each with what it waits on: its timers, its held requests, its held looks. */
function pending(context) {
  return context.work
    .filter((entry) => entry.settled === null)
    .map((entry) => ({
      op: entry.op,
      waits: [
        ...context.clock.waits(entry.op).map((timer) => ({ timer })),
        ...context.host.waits(entry.op).map((request) => ({ request })),
        ...context.peer.waits(entry.op).map((fetch) => ({ fetch })),
        ...Array.from({ length: context.looks.waits(entry.op) }, () => ({ look: true })),
      ],
    }))
}

/** What the scene is now, which holds still once the work begun can move no further. */
function signature(context) {
  return JSON.stringify([
    context.work.map((entry) => entry.settled !== null),
    pending(context),
    context.requests.length,
    context.fetches.length,
    context.spawned.length,
    context.written.length,
    context.draws.length,
    context.machine.begun(),
  ])
}

/**
 * Turns the event loop until the scene holds still: nothing a work waits on
 * the machine for in flight, every work begun settled or waiting on
 * something a step controls, and nothing changed, two turns running. A file
 * request's callbacks and continuations run before the loop turns again, so
 * a turn that finds none in flight finds none about to begin. What it
 * cannot see is work waiting on a step and on something else besides that
 * is neither: no adapter's is. Work that does not hold still in ten seconds
 * fails the scenario.
 */
async function settleDown(context) {
  const started = performance.now()
  let still = null
  for (;;) {
    await new Promise((resolve) => real.setImmediate(resolve))
    const now = signature(context)
    const quiet =
      context.machine.inFlight() === 0 && pending(context).every((entry) => entry.waits.length > 0)
    if (quiet && still === now) return
    still = quiet ? now : null
    if (performance.now() - started > 10_000) {
      throw new Error(`work waits on nothing a step controls: ${JSON.stringify(pending(context))}`)
    }
  }
}

/**
 * Fires the next timer due by `until` in a turn of the loop of its own, as
 * Node fires a timer: what its callback queues with `process.nextTick` then
 * runs before its promises' continuations. Whether one fired.
 */
function fire(clock, until) {
  return new Promise((resolve, reject) =>
    real.setImmediate(() => {
      try {
        resolve(clock.fireNext(until))
      } catch (cause) {
        reject(cause)
      }
    }),
  )
}

/** A recorded step, played: its record. */
async function record(context, index, step) {
  const before = tree(context.root)
  context.requests = []
  context.fetches = []
  context.spawned = []
  context.written = []
  context.draws = []
  if (step.release !== undefined) {
    // A route has a space in it (`GET /session`), a host's operation none.
    const released =
      step.release === 'look'
        ? context.looks.release()
        : step.release.includes(' ')
          ? context.peer.release(step.release, step.answer)
          : context.host.release(step.release, step.answer)
    if (!released) throw new Error(`${step.release}: nothing held to release`)
  } else if (step.releaseBody !== undefined) {
    if (!context.peer.releaseBody(step.releaseBody, step.body)) {
      throw new Error(`${step.releaseBody}: no body held to end`)
    }
  } else if (step.advance !== undefined) {
    const until = context.clock.now() + step.advance
    await settleDown(context)
    while (await fire(context.clock, until)) await settleDown(context)
    context.clock.settleAt(until)
  } else if (step.close !== undefined) {
    // The engine closes the window: its launch's files go, and what it
    // still waits on keeps its own hold of the window.
    forgetLaunch(context.env.CONSENSFLOW_HOME, context.launchId)
  } else {
    begin(context, index, step)
  }
  await settleDown(context)
  const settled = context.work
    .filter((entry) => entry.settled !== null)
    .map((entry) => ({ ...entry.settled, op: entry.op }))
  context.work = context.work.filter((entry) => entry.settled === null)
  const unused = [
    ...context.host.unused(step.optional),
    ...context.peer.unused(step.optional),
    ...context.children.unused(),
  ]
  return written(context, {
    step: index,
    settled,
    pending: pending(context),
    requests: context.requests,
    ...(context.fetches.length > 0 ? { fetches: context.fetches } : {}),
    ...(context.spawned.length > 0 ? { spawned: context.spawned } : {}),
    ...(context.written.length > 0 ? { written: context.written } : {}),
    ...(unused.length > 0 ? { unused } : {}),
    draws: context.draws,
    tree: changes(before, tree(context.root)),
  })
}

/**
 * Plays `scenario` in a root of its own, against the adapter its harness
 * names among `adapters`: each recorded step's record.
 */
export async function play(scenario, adapters = ADAPTERS) {
  // The root as the system names it, which a harness that asks for a folder's
  // real name (OpenCode's `realpath`) is then told as the scenario writes it.
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'cf-launch-golden-')))
  const context = {
    root,
    rootForms: rootForms(root),
    other: null,
    launch: null,
    launchId: null,
    work: [],
    requests: [],
    fetches: [],
    spawned: [],
    written: [],
    draws: [],
    ports: Array.from({ length: 100 }, (_, index) => 41_000 + index),
    // A scripted child's pid, none a live process has (`DEAD` and on).
    pids: Array.from({ length: 100 }, (_, index) => DEAD - 1 - index),
    als: new AsyncLocalStorage(),
  }
  context.clock = fakeClock(() => context.als.getStore())
  context.machine = machineWork(
    () => context.als.getStore() !== undefined,
    () => context.clock.machineLands(),
  )
  const restore = install(context)
  const umask = process.umask(UMASK)
  try {
    if (JSON.stringify(scenario).includes('$OTHER')) context.other = startOther()
    context.env = realValue(context, scenario.env)
    await fs.mkdir(path.join(root, 'bin'), { recursive: true })
    context.host = scriptedHost(context)
    context.peer = scriptedPeer(context)
    context.children = scriptedChildren(context)
    context.looks = scriptedLooks(context)
    context.adapter = adapters[scenario.harness]({
      env: context.env,
      answers: context.looks.answers,
    })
    const records = []
    for (const [index, each] of scenario.steps.entries()) {
      if (await setUp(context, each)) continue
      const recorded = await record(context, index, each).catch((cause) => {
        throw new Error(`${scenario.name}, step ${index}: ${cause.message}`)
      })
      if (recorded.unused !== undefined) {
        throw new Error(`${scenario.name}, step ${index}: answers left unasked: ${recorded.unused}`)
      }
      records.push(recorded)
    }
    // Work still waiting at the end is the scenario's to declare.
    const left = context.work.map((entry) => entry.op)
    if (JSON.stringify(left) !== JSON.stringify(scenario.pendingAtEnd ?? [])) {
      throw new Error(`${scenario.name}: work left waiting at the end: ${JSON.stringify(left)}`)
    }
    return { ...scenario, records }
  } finally {
    process.umask(umask)
    restore()
    context.machine.stop()
    context.other?.kill()
    await fs.rm(root, { recursive: true, force: true })
  }
}
