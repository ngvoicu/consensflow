import { startCore } from '../../src/core/daemon.js'

// The new core's daemon for the integration suite.
await startCore(process.env)
