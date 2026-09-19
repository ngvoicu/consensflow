import { serveUi } from '../../src/ui.js'

// The live bench's editor: the production daemon with the REAL launch
// configuration (Claude settings, Pi extension, OpenCode plugin, Codex bridge),
// unlike the integration editor, which stubs every channel away.
await serveUi(process.env, {
  json: true,
  open: false,
  onOut: (line) => process.stdout.write(`${line}\n`),
})
