#!/usr/bin/env node

import { copyFile, mkdir } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { build } from 'esbuild'

const APP = dirname(dirname(fileURLToPath(import.meta.url)))
const REPO = dirname(APP)
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

await build({
  bundle: true,
  entryPoints: [join(REPO, 'src', 'layout.js')],
  format: 'esm',
  logLevel: 'warning',
  minify: true,
  outfile: join(VENDOR, 'layout.js'),
  platform: 'browser',
  target: ['safari15'],
})

await build({
  bundle: true,
  entryPoints: [join(REPO, 'hosts', 'lib', 'policy.js')],
  format: 'esm',
  logLevel: 'warning',
  minify: true,
  outfile: join(VENDOR, 'policy.js'),
  platform: 'browser',
  target: ['safari15'],
})

await copyFile(
  join(APP, 'node_modules', '@xterm', 'xterm', 'css', 'xterm.css'),
  join(VENDOR, 'xterm.css'),
)

process.stdout.write('ui: xterm 6, addon-fit, pane layout and delivery policy bundled locally\n')
