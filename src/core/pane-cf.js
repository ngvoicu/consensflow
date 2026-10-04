import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** The bundle's own bin/: first on every window's PATH. */
export const BUNDLE_BIN = fileURLToPath(new URL('../../bin', import.meta.url))

/**
 * The native `cf` in bin/ (crates/cf, which npm run build:cf and
 * prepare-sidecar build), its path as this platform spells it: what a
 * process is started as, a Codex window's supervisor (`cf codex-session`)
 * among them.
 */
export const BUNDLE_CF = join(BUNDLE_BIN, process.platform === 'win32' ? 'cf.exe' : 'cf')

/**
 * The `cf` a window runs, which its role text and its hooks name: the
 * bundle's. On Windows its path has forward slashes, which Git Bash keeps
 * where it drops backslashes, and PowerShell reads alike.
 */
export const PANE_CF = process.platform === 'win32' ? BUNDLE_CF.replaceAll('\\', '/') : BUNDLE_CF
