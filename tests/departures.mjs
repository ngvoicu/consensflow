/**
 * Records again the traces of the tests the Rust engine departs from Node's
 * on purpose: each test named in `DEPARTED`
 * (`crates/cf-engine/tests/dispatcher/traces.rs`) is run with
 * `CF_RERECORD_DEPARTED` set, and writes the trace of what the engine did into
 * `crates/cf-engine/tests/departures/`, in the shape of Node's, where it is
 * held to it. The rest of the suite runs as it does, held to Node's traces,
 * which are fixed since Node's dispatcher was deleted, so a departure that
 * spread shows. When the engine does what Node's trace has, a departed test
 * fails, saying so: take it off its `DEPARTED` and delete its departure.
 *
 *   npm run departures
 */
import { spawnSync } from 'node:child_process'

const ran = spawnSync('cargo', ['test', '-p', 'cf-engine', '--test', 'dispatcher'], {
  stdio: 'inherit',
  env: { ...process.env, CF_RERECORD_DEPARTED: '1' },
})
process.exit(ran.status ?? 1)
