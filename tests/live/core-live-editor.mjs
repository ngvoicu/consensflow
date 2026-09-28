import { startCore } from '../../src/core/daemon.js'

// The new core's daemon for the live bench: real adapters, Claude's peer
// inbox where the platform has it.
await startCore(process.env)
