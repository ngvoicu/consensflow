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

// Mermaid, for the diagrams of a project's specs: one file the specs viewer imports only when
// it draws one, so the page loads without it and draws offline.
await build({
  bundle: true,
  format: 'esm',
  logLevel: 'warning',
  minify: true,
  outfile: join(VENDOR, 'mermaid.js'),
  platform: 'browser',
  stdin: {
    contents: "export { default } from 'mermaid'",
    resolveDir: APP,
    sourcefile: 'mermaid-entry.js',
  },
  target: ['safari15'],
})

process.stdout.write('ui: xterm 6, addon-fit and mermaid 11 bundled locally\n')
