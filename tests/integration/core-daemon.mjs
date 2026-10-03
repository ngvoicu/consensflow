import { startDaemon } from '../../src/core/daemon.js'

// The daemon for the integration suite.
await startDaemon(process.env)
