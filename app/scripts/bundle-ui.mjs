#!/usr/bin/env node

import { copyFile, mkdir } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { build } from 'esbuild'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const VENDOR = join(APP, 'ui', 'vendor')

await mkdir(VENDOR, { recursive: true })

await build({
  bundle: true,
  format: 'esm',
  logLevel: 'warning',
  minify: true,
  outfile: join(VENDOR, 'xterm.js'),
  platform: 'browser',
  stdin: {
    contents:
      "export { Terminal } from '@xterm/xterm'; export { FitAddon } from '@xterm/addon-fit';",
    resolveDir: APP,
    sourcefile: 'xterm-entry.js',
  },
  target: ['safari15'],
})

await copyFile(
  join(APP, 'node_modules', '@xterm', 'xterm', 'css', 'xterm.css'),
  join(VENDOR, 'xterm.css'),
)

// The daemon's own module, for what the human types into a Devin or Codex window.
await copyFile(join(APP, '..', 'src', 'console-text.js'), join(VENDOR, 'console-text.js'))

process.stdout.write('ui: xterm 6, addon-fit and console-text bundled locally\n')
