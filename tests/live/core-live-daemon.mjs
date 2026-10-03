import { startDaemon } from '../../src/core/daemon.js'

// The daemon for the live bench: real adapters, Claude's peer
// inbox where the platform has it.
await startDaemon(process.env)
