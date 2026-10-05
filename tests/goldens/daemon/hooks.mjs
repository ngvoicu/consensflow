/**
 * Loaded with `node --import`: what a suite imports of the daemon's surfaces
 * is put in the place of a wrapper that records it (the files beside this
 * one), and each wrapper imports the real module. The ledger is the 3.1
 * recording ledger, watched; `node:http` is replaced for the API's module
 * alone, `node:child_process` for the fixture that runs `cf`, `node:test` for
 * every suite, which gives each test its own trace.
 */
import { registerHooks } from 'node:module'

const here = (name) => new URL(name, import.meta.url).href
const source = (path) => new URL(`../../../src/${path}`, import.meta.url).href

const LEDGER = source('ledger/index.js')
const API = source('core/api.js')
const RECORDING = new URL('../ledger/recording.mjs', import.meta.url).href

/** The module a suite imports, and the wrapper it gets. */
const WRAPPED = new Map([
  [LEDGER, here('ledger.mjs')],
  [API, here('api.mjs')],
  [source('core/page.js'), here('page.mjs')],
  [source('core/agents-server.js'), here('agents.mjs')],
  [source('core/trace.js'), here('trace.mjs')],
  [source('core/log.js'), here('log.mjs')],
])

/** The wrappers and what they stand on: these reach the real modules. */
const OWN = new Set(
  [
    'ledger',
    'api',
    'page',
    'agents',
    'trace',
    'log',
    'http',
    'spawn',
    'test',
    'session',
    'exchange',
    'seam',
    'wire',
    'world',
    'mask',
    'lines',
  ]
    .map((name) => here(`${name}.mjs`))
    .concat(RECORDING),
)

/** The modules whose `cf` runs are recorded: the fixture that runs them, and any a recorder's own test names. */
const SPAWNERS = new Set([
  new URL('../../core-api-fixture.mjs', import.meta.url).href,
  ...JSON.parse(process.env.CF_DAEMON_SPAWNERS ?? '[]'),
])

registerHooks({
  resolve(specifier, context, nextResolve) {
    const resolved = nextResolve(specifier, context)
    const parent = context.parentURL ?? ''
    if (OWN.has(parent)) return resolved
    const wrapper = WRAPPED.get(resolved.url)
    if (wrapper !== undefined) return { ...resolved, url: wrapper, shortCircuit: true }
    const builtin = {
      'node:http': parent === API ? here('http.mjs') : undefined,
      'node:child_process': SPAWNERS.has(parent) ? here('spawn.mjs') : undefined,
      'node:test': parent.startsWith('file:') ? here('test.mjs') : undefined,
    }[resolved.url]
    return builtin === undefined ? resolved : { url: builtin, format: 'module', shortCircuit: true }
  },
})
