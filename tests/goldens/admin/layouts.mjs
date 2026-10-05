/**
 * Every install layout of every harness, looked at and updated the way the
 * CLI got there: where its executable lives decides how it is called, which
 * command updates it and which feed says its latest release. Each layout is
 * found where `harnessPath` finds a CLI (on PATH, or where it installs itself),
 * once as the folder holds it, and, on systems that have links, once as the
 * installers make it: a link that leads to where it really lives.
 */
import { exe, onPath, release, said, WINDOWS } from './kit.mjs'

/** The layouts: which harness, what to call it, where it lives, what updates it. */
const FOLDERS = [
  ['codex', 'a Homebrew cask', '$ROOT/brew/Caskroom/codex/0.1/bin', 'brew upgrade --cask codex'],
  ['codex', 'a Homebrew formula', '$ROOT/brew/Cellar/codex/0.1/bin', 'brew upgrade codex'],
  ['codex', 'a formula of another name', '$ROOT/brew/Cellar/codex-cli/1/bin', null],
  [
    'codex',
    'a global npm install',
    '$ROOT/npm/lib/node_modules/@openai/codex/bin',
    'npm install -g @openai/codex@latest',
  ],
  ['codex', "its own installer's folder", '$ROOT/home/.codex/bin', 'codex update'],
  ['codex', 'a folder nobody recognizes', '$ROOT/bin', null],
  [
    'claude',
    'a Homebrew cask called claude-code',
    '$ROOT/brew/Caskroom/claude-code/2/bin',
    'brew upgrade --cask claude-code',
  ],
  [
    'claude',
    'a Homebrew cask called claude-code@latest',
    '$ROOT/brew/Caskroom/claude-code@latest/2/bin',
    'brew upgrade --cask claude-code@latest',
  ],
  ['claude', 'a cask called claude', '$ROOT/brew/Caskroom/claude/2/bin', null],
  [
    'claude',
    "the folder of Claude's own installer's versions",
    '$ROOT/home/.local/share/claude/versions',
    'claude update',
  ],
  [
    'claude',
    'a global npm install',
    '$ROOT/npm/lib/node_modules/@anthropic-ai/claude-code/bin',
    'npm install -g @anthropic-ai/claude-code@latest',
  ],
  ['claude', 'a folder nobody recognizes', '$ROOT/bin', null],
  ['opencode', 'a Homebrew formula', '$ROOT/brew/Cellar/opencode/1/bin', 'brew upgrade opencode'],
  [
    'opencode',
    'a Homebrew cask',
    '$ROOT/brew/Caskroom/opencode/1/bin',
    'brew upgrade --cask opencode',
  ],
  [
    'opencode',
    'a global npm install',
    '$ROOT/npm/lib/node_modules/opencode-ai/bin',
    'npm install -g opencode-ai@latest',
  ],
  ['opencode', "its own installer's folder", '$ROOT/home/.opencode/bin', 'opencode upgrade'],
  ['opencode', 'a folder nobody recognizes', '$ROOT/bin', null],
  [
    'pi',
    'a global npm install',
    '$ROOT/npm/lib/node_modules/@earendil-works/pi-coding-agent/dist',
    'npm install -g @earendil-works/pi-coding-agent@latest',
  ],
  ['pi', "its own installer's folder", '$ROOT/home/.pi/bin', 'pi update --self'],
  ['pi', 'a folder nobody recognizes', '$ROOT/bin', null],
  [
    'devin',
    "the folder of Devin's own installer",
    '$ROOT/home/.local/share/devin/cli/_versions/current/bin',
    'devin update',
  ],
  ['devin', 'the folder a link of its installer is in', '$ROOT/home/.local/bin', null],
  ['devin', 'a folder nobody recognizes', '$ROOT/bin', null],
]

/**
 * Layouts a link makes, as the installers make them: where the link is (on
 * PATH), what it leads to, and what updates it.
 */
const LINKS = [
  [
    'codex',
    'a link from Homebrew into its cask',
    '$ROOT/brew/bin',
    '$ROOT/brew/Caskroom/codex/0.1/bin/codex',
    'brew upgrade --cask codex',
  ],
  [
    'opencode',
    'a link from Homebrew into its formula',
    '$ROOT/brew/bin',
    '$ROOT/brew/Cellar/opencode/1/bin/opencode',
    'brew upgrade opencode',
  ],
  [
    'pi',
    "a link from npm's bin into the package",
    '$ROOT/npm/bin',
    '$ROOT/npm/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js',
    'npm install -g @earendil-works/pi-coding-agent@latest',
  ],
  [
    'claude',
    "a link from ~/.local/bin to one of Claude's versions",
    '$ROOT/home/.local/bin',
    '$ROOT/home/.local/share/claude/versions/2.1.280',
    'claude update',
  ],
  [
    'devin',
    "a link from ~/.local/bin into Devin's installer",
    '$ROOT/home/.local/bin',
    '$ROOT/home/.local/share/devin/cli/_versions/current/bin/devin',
    'devin update',
  ],
  [
    'codex',
    "a link from ~/.local/bin into the installer's folder",
    '$ROOT/home/.local/bin',
    '$ROOT/home/.codex/bin/codex',
    'codex update',
  ],
]

/** The name a program is called by in what a scenario scripts. */
const commandOf = (harness) => harness

/**
 * A scenario that looks at `harness`, then updates it: a version, a release,
 * and the program that updates it when the layout has one, after which the
 * version is a newer one.
 */
function lookAndUpdate({ harness, label, files, path, update }) {
  const probe = `${commandOf(harness)} --version`
  return {
    name: `${harness} as ${label}: looked at and updated the way it was installed`,
    files,
    env: { PATH: onPath(path) },
    effects: {
      run: {
        [probe]:
          update === null
            ? [said(`${harness} 5.0.0\n`)]
            : [said(`${harness} 5.0.0\n`), said(`${harness} 5.0.1\n`)],
        ...(update === null ? {} : { [update]: [said('done\n')] }),
      },
      latest: {
        [harness]: update === null ? [release('5.0.1')] : [release('5.0.1'), release('5.0.1')],
      },
    },
    steps: [
      { op: 'check', id: harness },
      { op: 'update', id: harness },
    ],
  }
}

export function layouts() {
  const folders = FOLDERS.map(([harness, label, folder, update]) =>
    lookAndUpdate({
      harness,
      label: `${label} (${folder.replace('$ROOT/', '')})`,
      files: [exe(`${folder}/${harness}`)],
      path: folder,
      update,
    }),
  )
  if (WINDOWS) return folders
  const links = LINKS.map(([harness, label, bin, target, update]) =>
    lookAndUpdate({
      harness,
      label,
      files: [
        { path: target, executable: true },
        { path: `${bin}/${harness}`, link: target },
      ],
      path: bin,
      update,
    }),
  )
  return [...folders, ...links]
}
