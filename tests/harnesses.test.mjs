import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { runnable } from '../src/harnesses.js'

/** How a harness CLI is run: a program as it is, a Windows .cmd through cmd.exe with its quoting. */
describe('runnable', () => {
  it('runs a program directly, with its arguments untouched', () => {
    const run = runnable('/usr/local/bin/codex', ['app-server', 'a b'])
    assert.deepEqual(run, {
      file: '/usr/local/bin/codex',
      args: ['app-server', 'a b'],
      options: {},
    })
  })

  it('runs a .cmd through cmd.exe, each argument quoted the way cmd.exe reads it', () => {
    const run = runnable(
      'C:\\Program Files\\nodejs\\codex.cmd',
      ['-c', 'developer_instructions="hi" & more', 'trailing\\'],
      { ComSpec: 'C:\\Windows\\System32\\cmd.exe' },
    )
    assert.equal(run.file, 'C:\\Windows\\System32\\cmd.exe')
    assert.deepEqual(run.options, { windowsVerbatimArguments: true })
    assert.deepEqual(run.args.slice(0, 3), ['/d', '/s', '/c'])
    // The exact line, as cross-spawn (npm's own runner) would write it: every
    // special character carries one caret for cmd.exe's reading of the line
    // and two more for the script's own reading; an inner quote is backslashed
    // for the program and a trailing backslash doubled.
    assert.equal(
      run.args[3],
      '"C:\\Program^ Files\\nodejs\\codex.cmd ^^^"-c^^^" ^^^"developer_instructions=\\^^^"hi\\^^^"^^^ ^^^&^^^ more^^^" ^^^"trailing\\\\^^^""',
    )
  })

  it('treats .bat the same and everything else as a program', () => {
    assert.equal(runnable('C:\\x\\tool.BAT', []).file, 'cmd.exe')
    assert.equal(runnable('C:\\x\\claude.exe', []).file, 'C:\\x\\claude.exe')
  })
})
