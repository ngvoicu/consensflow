// ConsensFlow's extension as Pi loads it from the command line (`--extension`), with the options
// the driver gives it (`drive.mjs`): Pi loads this file through its own loader, as it loads the
// shipped one. The driver runs Pi's SDK, not its CLI, so the version is the SDK's own `VERSION`,
// which the driver passes; the CLI's read (`piVersionOf`) is proved apart (`tests/pi-extension`).
import { createDeliveryExtension } from '../../hosts/pi-extension/consensflow-delivery.mjs'

export default function ours(pi) {
  return createDeliveryExtension(pi, globalThis.__piLoop.ours)
}
