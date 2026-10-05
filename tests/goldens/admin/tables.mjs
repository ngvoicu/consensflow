/**
 * The install layouts that are no more than a text: a path as a CLI is found
 * at, which `releaseSource` reads as a text, so that its answer is the same
 * on every system but for the separator `join` makes (what the JS test's
 * `posix()` takes out, as this does). A layout that needs a file on disk (a
 * link, Claude's settings, its copy on Windows) is a scenario of a platform's
 * own.
 */
import { existsSync } from 'node:fs'
import { sourceOf } from './world.mjs'

/** Every layout: the harness and where its CLI is. */
const SOURCES = [
  // Homebrew: a cask or a formula, named by its package.
  ['codex', '/opt/homebrew/Caskroom/codex/0.1/bin/codex'],
  ['opencode', '/opt/homebrew/Cellar/opencode/1/bin/opencode'],
  ['claude', '/opt/homebrew/Caskroom/claude-code@latest/2/bin/claude'],
  ['claude', '/opt/homebrew/Caskroom/claude-code/2/bin/claude'],
  ['claude', '/opt/homebrew/Cellar/claude-code/2/bin/claude'],
  ['claude', '/opt/homebrew/Caskroom/claude/2/bin/claude'],
  ['codex', '/opt/homebrew/Caskroom/claude-code/2/bin/codex'],
  ['codex', '/opt/homebrew/Cellar/codex-cli/1/bin/codex'],
  ['codex', '/usr/local/Cellar/codex/1.2/bin/codex'],
  ['codex', '/home/linuxbrew/.linuxbrew/Cellar/codex/1/bin/codex'],
  ['pi', '/opt/homebrew/Cellar/pi/1/bin/pi'],
  ['devin', '/opt/homebrew/Cellar/devin/1/bin/devin'],
  ['codex', '/a/Cellar/other/1/Caskroom/codex/2/bin/codex'],
  // The Caskroom folder named, and nothing after it; under a root no machine
  // has, as every path here must be (a Mac with Codex's cask has this folder).
  ['codex', '/no-such-root/homebrew/Caskroom/codex'],
  ['codex', '/no-such-root/homebrew/Caskroom/codex/'],
  ['codex', 'C:\\homebrew\\Cellar\\codex\\1\\bin\\codex.exe'],
  ['codex', '/opt/my brew/Caskroom/codex/0.1/bin/codex'],
  ['codex', '/opt/h\u00f6me/Caskroom/codex/0.1/bin/codex'],
  // Claude's own installer: its versions folder, or its copy on Windows.
  ['claude', '/Users/me/.local/share/claude/versions/2.1.280'],
  ['claude', '/Users/me/.local/share/claude/versions'],
  ['claude', '/Users/me/.local/share/claude/versions/'],
  ['claude', '/x/claude/versions/y/claude/versions/2'],
  ['codex', '/Users/me/.local/share/claude/versions/2.1.280'],
  ['claude', 'C:\\Users\\me\\.local\\share\\claude\\versions\\2.1.274\\claude.exe'],
  ['claude', 'C:\\Users\\me\\.local\\bin\\claude.exe'],
  ['claude', 'C:\\Users\\me\\.LOCAL\\BIN\\CLAUDE.EXE'],
  ['claude', '/Users/me/.claude/local/claude'],
  // A global npm install: the folder that holds `lib/node_modules`.
  ['pi', '/usr/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js'],
  ['codex', '/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js'],
  [
    'claude',
    '/Users/me/.nvm/versions/node/v22.1.0/lib/node_modules/@anthropic-ai/claude-code/cli.js',
  ],
  ['opencode', '/usr/local/lib/node_modules/opencode-ai/bin/opencode'],
  ['devin', '/usr/lib/node_modules/devin/bin/devin'],
  ['codex', '/a/lib/node_modules/x/lib/node_modules/@openai/codex/bin/codex.js'],
  ['codex', '/lib/node_modules/codex'],
  ['codex', 'C:\\Users\\me\\AppData\\Roaming\\npm\\node_modules\\@openai\\codex\\bin\\codex.js'],
  ['codex', 'C:\\Users\\me\\AppData\\Roaming\\npm\\codex.cmd'],
  // A harness's own installer, in either spelling.
  ['codex', '/Users/me/.codex/bin/codex'],
  ['codex', 'C:\\Users\\me\\.codex\\bin\\codex.exe'],
  ['opencode', '/Users/me/.opencode/bin/opencode'],
  ['opencode', 'C:\\Users\\me\\.opencode\\bin\\opencode.exe'],
  ['pi', '/Users/me/.pi/bin/pi'],
  ['pi', 'C:\\Users\\me\\.pi\\bin\\pi.exe'],
  ['devin', '/Users/me/.local/share/devin/cli/_versions/current/bin/devin'],
  ['devin', 'C:\\Users\\me\\AppData\\Local\\devin\\cli\\_versions\\current\\bin\\devin.exe'],
  ['claude', '/Users/me/.claude/bin/claude'],
  ['codex', '/Users/me/.codex/binx/codex'],
  ['codex', '/Users/me/.codex/bin'],
  // Where nothing is recognized.
  ['codex', '/somewhere/else/codex'],
  ['claude', '/x/y'],
  ['opencode', '/x/y'],
  ['pi', '/x/y'],
  ['devin', '/x/y'],
  // Which layout is read first: Homebrew's, then Claude's, npm's, the harness's own.
  ['codex', '/opt/homebrew/Caskroom/codex/1/lib/node_modules/@openai/codex/bin/codex'],
  ['codex', '/Users/me/.codex/bin/Caskroom/codex/1/codex'],
  ['claude', '/Users/me/lib/node_modules/@anthropic-ai/claude-code/claude/versions/claude'],
  ['codex', '/Users/me/lib/node_modules/@openai/codex/.codex/bin/codex'],
]

/** A source with every part of its command spelled with `/`, as the JS test's `posix()` does. */
function plain(source) {
  return { ...source, update: source.update?.map((part) => part.replaceAll('\\', '/')) ?? null }
}

/** Every table of the admin's goldens. */
export function tables() {
  // `releaseSource` reads where a path really leads: one that is there on the
  // machine recording would make the table that machine's.
  const there = SOURCES.filter(([, executable]) => existsSync(executable))
  if (there.length > 0)
    throw new Error(`these layouts exist on this machine: ${JSON.stringify(there)}`)
  return {
    sources: SOURCES.map(([id, executable]) => ({
      id,
      executable,
      source: plain(sourceOf(id, executable)),
    })),
  }
}
