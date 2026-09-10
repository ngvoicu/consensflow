import { serveUi } from '../../src/ui.js'

// These integration cases exercise the generic PTY transport. The simulated
// Claude process implements a terminal, not Claude's native peer inbox.
await serveUi(process.env, {
  json: true,
  open: false,
  onOut: (line) => process.stdout.write(`${line}\n`),
  serverOptions: { prepareChannel: async () => ({ args: [], env: {}, channel: null }) },
})
