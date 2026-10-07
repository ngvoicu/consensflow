/**
 * Records again the traces of the tests the Rust engine departs from Node's
 * on purpose: each test named in `DEPARTED`
 * (`crates/cf-engine/tests/dispatcher/traces.rs`) is run with
 * `CF_RERECORD_DEPARTED` set, and writes the trace of what the engine did into
 * `crates/cf-engine/tests/departures/`, in the shape of Node's, where it is
 * held to it. The rest of the suite runs as it does, held to Node's traces, so
 * a departure that spread shows.
 *
 * Node's traces are not touched: `npm run goldens:dispatcher` records them
 * from Node, which does not depart. When Node does what the engine does, a
 * departed test fails, saying so: take it off its `DEPARTED` and delete its
 * departure. (The ledger's traces have no recording of their own: a trace is
 * the calls Node's dispatcher made, which the engine's departure changes, and
 * `crates/cf-ledger/tests/replay.rs` says where each one departs.)
 *
 *   npm run goldens:departed
 */
import { spawnSync } from 'node:child_process'

const ran = spawnSync('cargo', ['test', '-p', 'cf-engine', '--test', 'dispatcher'], {
  stdio: 'inherit',
  env: { ...process.env, CF_RERECORD_DEPARTED: '1' },
})
process.exit(ran.status ?? 1)
