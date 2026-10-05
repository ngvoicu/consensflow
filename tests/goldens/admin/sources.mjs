/**
 * The install layouts that need something on disk to be read: Claude's
 * settings, which name the channel its installer follows, its copy on Windows,
 * which counts only with its versions beside it, and a link that leads nowhere.
 * (The layouts that are no more than a text are a table, `tables.mjs`.)
 */
import { WINDOWS } from './kit.mjs'

const VERSIONED = '$ROOT/home/.local/share/claude/versions/2.1.280'

/** The step that writes Claude's settings, and the one that says the source then. */
const settled = (settings) => [
  { op: 'write', path: '$ROOT/home/.claude/settings.json', text: settings },
  { op: 'source', id: 'claude', executable: VERSIONED },
]

export function sourceScenarios() {
  return [
    {
      name: "Claude's installer follows the channel its settings name, the latest unless they say stable",
      steps: [
        { op: 'source', id: 'claude', executable: VERSIONED },
        ...settled('{"autoUpdatesChannel":"stable"}'),
        ...settled('{"autoUpdatesChannel":"beta"}'),
        ...settled('{"autoUpdatesChannel":"Stable"}'),
        ...settled('{"autoUpdatesChannel":["stable"]}'),
        ...settled('{"autoUpdatesChannel":"\\u0073table"}'),
        ...settled('{\n  "model": "opus",\n  "autoUpdatesChannel": "stable"\n}\n'),
        ...settled('{"autoUpdatesChannel":"x","autoUpdatesChannel":"stable"}'),
        ...settled('{"autoUpdatesChannel":"stable","autoUpdatesChannel":"x"}'),
        ...settled('"stable"'),
        ...settled('null'),
        ...settled('[]'),
        ...settled('{}'),
        ...settled(''),
        ...settled('not json'),
        ...settled('﻿{"autoUpdatesChannel":"stable"}'),
      ],
    },
    {
      name: 'Claude settings are read where CLAUDE_CONFIG_DIR says, and nowhere else',
      env: { CLAUDE_CONFIG_DIR: '$ROOT/elsewhere' },
      steps: [
        {
          op: 'write',
          path: '$ROOT/home/.claude/settings.json',
          text: '{"autoUpdatesChannel":"stable"}',
        },
        { op: 'source', id: 'claude', executable: VERSIONED },
        {
          op: 'write',
          path: '$ROOT/elsewhere/settings.json',
          text: '{"autoUpdatesChannel":"stable"}',
        },
        { op: 'source', id: 'claude', executable: VERSIONED },
      ],
    },
    {
      name: "Claude's copy on Windows counts only with its versions beside it, whatever case its name is in",
      steps: [
        { op: 'write', path: '$ROOT/home/.LOCAL/BIN/CLAUDE.EXE', text: '' },
        { op: 'source', id: 'claude', executable: '$ROOT/home/.LOCAL/BIN/CLAUDE.EXE' },
        { op: 'write', path: '$ROOT/home/.local/bin/claude.exe', text: '' },
        { op: 'source', id: 'claude', executable: '$ROOT/home/.local/bin/claude.exe' },
        { op: 'write', path: '$ROOT/home/.local/share/claude/versions/2.1.274/claude', text: '' },
        { op: 'source', id: 'claude', executable: '$ROOT/home/.LOCAL/BIN/CLAUDE.EXE' },
        { op: 'source', id: 'claude', executable: '$ROOT/home/.local/bin/claude.exe' },
        { op: 'source', id: 'codex', executable: '$ROOT/home/.local/bin/claude.exe' },
      ],
    },
    ...(WINDOWS
      ? []
      : [
          {
            name: 'a link that leads nowhere is read as the path it is, and one that leads on is read where it leads',
            files: [
              { path: '$ROOT/home/.codex/bin/codex', link: '$ROOT/nowhere/codex' },
              { path: '$ROOT/brew/Caskroom/codex/1/bin/codex', executable: true },
              { path: '$ROOT/links/codex', link: '$ROOT/brew/Caskroom/codex/1/bin/codex' },
              { path: '$ROOT/links/again', link: '$ROOT/links/codex' },
              { path: '$ROOT/links/loop', link: '$ROOT/links/loop' },
            ],
            steps: [
              { op: 'source', id: 'codex', executable: '$ROOT/home/.codex/bin/codex' },
              { op: 'source', id: 'codex', executable: '$ROOT/links/codex' },
              { op: 'source', id: 'codex', executable: '$ROOT/links/again' },
              { op: 'source', id: 'codex', executable: '$ROOT/links/loop' },
            ],
          },
        ]),
  ]
}
