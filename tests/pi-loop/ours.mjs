// ConsensFlow's extension as Pi loads it from the command line (`--extension`), with the options
// the driver gives it (`drive.mjs`): Pi loads this file through its own loader, as it loads the
// shipped one.
import { createDeliveryExtension } from '../../hosts/pi-extension/consensflow-delivery.mjs'

export default function ours(pi) {
  return createDeliveryExtension(pi, globalThis.__piLoop.ours)
}
