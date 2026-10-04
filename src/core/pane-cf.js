import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** The bundle's own bin/: first on every window's PATH. */
export const BUNDLE_BIN = fileURLToPath(new URL('../../bin', import.meta.url))

/**
 * The `cf` a window runs, which its role text and its hooks name: the
 * native one in bin/ (crates/cf, which npm run build:cf and prepare-sidecar
 * build). On Windows its path has forward slashes, which Git Bash keeps
 * where it drops backslashes, and PowerShell reads alike.
 */
export const PANE_CF =
  process.platform === 'win32'
    ? join(BUNDLE_BIN, 'cf.exe').replaceAll('\\', '/')
    : join(BUNDLE_BIN, 'cf')
