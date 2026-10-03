import { startCore } from '../../src/core/daemon.js'

// The daemon for the integration suite.
await startCore(process.env)
