import { startCore } from '../../src/core/daemon.js'

// The new core's daemon for the integration suite. The fake agent speaks
// Claude's terminal and files but not its peer inbox, so messages are pasted.
await startCore(process.env, { peer: false })
