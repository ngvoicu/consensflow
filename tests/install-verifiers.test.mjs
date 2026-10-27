// The installers' own check of a signature, written out in each script because
// no tool a Mac or Windows ships checks Ed25519: the JavaScript install.sh runs
// in macOS's osascript, and the PowerShell install.ps1 runs in Windows
// PowerShell 5.1 and PowerShell 7. Each is run here on the same battery:
// RFC 8032's Ed25519 vectors and their changed copies, BLAKE2b-512's known
// answers, signatures made by `tauri signer sign`, and a published release's
// signature by ConsensFlow's own update key (tests/fixtures/install, made by
// make-fixtures.mjs there). Every engine on this machine is run; the CI runs
// these on macOS and on Windows, where both PowerShells are.

import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { spawnSync } from 'node:child_process'
import crypto from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, before, describe, test } from 'node:test'

import {
  INSTALL_PS1,
  INSTALL_SH,
  makeKey,
  minisignProblem,
  ROOT,
  scriptKeyLine,
  signedName,
  updaterKeyLine,
} from '../install/kit.mjs'

const fixtures = path.join(ROOT, 'tests/fixtures/install')
const L = 2n ** 252n + 27742317777372353535851937790883648493n

function fixture(name) {
  return readFileSync(path.join(fixtures, name))
}

function flipHex(hex, at) {
  const bytes = Buffer.from(hex, 'hex')
  bytes[at] ^= 1
  return bytes.toString('hex')
}

/** The lines of a .sig file's text, and that text again from lines. */
function sigLines(sig) {
  return Buffer.from(sig.trim(), 'base64').toString('latin1').split('\n')
}
function sigOf(lines) {
  return Buffer.from(lines.join('\n'), 'latin1').toString('base64')
}
function flipBase64(base64, at) {
  const bytes = Buffer.from(base64, 'base64')
  bytes[at] ^= 1
  return bytes.toString('base64')
}

/** What every engine is given, and what it must answer. */
function battery() {
  const ed25519 = []
  for (const vector of JSON.parse(fixture('ed25519-rfc8032.json')).vectors) {
    ed25519.push({ ...vector, expect: true })
    ed25519.push({
      ...vector,
      name: `${vector.name}, R changed`,
      signature: flipHex(vector.signature, 0),
      expect: false,
    })
    ed25519.push({
      ...vector,
      name: `${vector.name}, S changed`,
      signature: flipHex(vector.signature, 40),
      expect: false,
    })
    ed25519.push({
      ...vector,
      name: `${vector.name}, key changed`,
      public: flipHex(vector.public, 0),
      expect: false,
    })
    if (vector.message)
      ed25519.push({
        ...vector,
        name: `${vector.name}, message changed`,
        message: flipHex(vector.message, 0),
        expect: false,
      })
    // S + L signs the same, and is refused: S must be below L (RFC 8032 5.1.7).
    const sig = Buffer.from(vector.signature, 'hex')
    let s = 0n
    for (let i = 63; i >= 32; i--) s = (s << 8n) | BigInt(sig[i])
    let big = s + L
    for (let i = 32; i < 64; i++) {
      sig[i] = Number(big & 255n)
      big >>= 8n
    }
    ed25519.push({
      ...vector,
      name: `${vector.name}, S + L`,
      signature: sig.toString('hex'),
      expect: false,
    })
  }
  // The global signature of a published release, by the key the scripts hold: Ed25519 over the
  // signature and its trusted comment.
  const released = sigLines(
    fixture('release-signature/ConsensFlow_3.0.6-alpha.2_aarch64.app.tar.gz.sig').toString('utf8'),
  )
  const signature = Buffer.from(released[1], 'base64').subarray(10)
  const comment = Buffer.from(released[2].slice('trusted comment: '.length), 'latin1')
  const updater = Buffer.from(updaterKeyLine(), 'base64').subarray(10)
  ed25519.push({
    name: "3.0.6-alpha.2's update archive, by ConsensFlow's update key",
    public: updater.toString('hex'),
    message: Buffer.concat([signature, comment]).toString('hex'),
    signature: Buffer.from(released[3], 'base64').toString('hex'),
    expect: true,
  })

  const blake2b = JSON.parse(fixture('blake2b-512.json')).vectors

  const tauri = JSON.parse(fixture('tauri-signed/key.json'))
  const sums = fixture(`tauri-signed/${tauri.file}`)
  const sig = fixture(`tauri-signed/${tauri.file}.sig`).toString('utf8')
  const lines = sigLines(sig)
  const minisign = [
    {
      name: "tauri signer's signature",
      key: tauri.keyLine,
      sig,
      file: sums,
      fileName: tauri.file,
      expect: null,
    },
    {
      name: "tauri signer's signature, for another release's name",
      key: tauri.keyLine,
      sig,
      file: sums,
      fileName: signedName('9.9.9'),
      expect:
        'the signature is for ConsensFlow_9.9.9-test.1_SHA256SUMS, not ConsensFlow_9.9.9_SHA256SUMS',
    },
    {
      name: 'a flipped bit in SHA256SUMS',
      key: tauri.keyLine,
      sig,
      file: Buffer.from(sums.map((byte, at) => (at === 5 ? byte ^ 1 : byte))),
      fileName: tauri.file,
      expect: 'the signature does not match the file',
    },
    {
      name: 'a flipped bit in the signature',
      key: tauri.keyLine,
      sig: sigOf([lines[0], flipBase64(lines[1], 30), lines[2], lines[3], lines[4]]),
      file: sums,
      fileName: tauri.file,
      expect: 'the signature does not match the file',
    },
    {
      name: 'a flipped bit in the trusted comment',
      key: tauri.keyLine,
      sig: sigOf([
        lines[0],
        lines[1],
        lines[2].replace('timestamp:1', 'timestamp:0'),
        lines[3],
        lines[4],
      ]),
      file: sums,
      fileName: tauri.file,
      expect: "the trusted comment's signature does not match",
    },
    {
      name: 'a flipped bit in the global signature',
      key: tauri.keyLine,
      sig: sigOf([lines[0], lines[1], lines[2], flipBase64(lines[3], 7), lines[4]]),
      file: sums,
      fileName: tauri.file,
      expect: "the trusted comment's signature does not match",
    },
    {
      name: 'the wrong key',
      key: updaterKeyLine(),
      sig,
      file: sums,
      fileName: tauri.file,
      expect: 'the signature was made by another key',
    },
    {
      name: 'a key line that is no minisign key',
      key: 'aGVsbG8=',
      sig,
      file: sums,
      fileName: tauri.file,
      expect: 'the public key is not a minisign Ed25519 key',
    },
    {
      name: 'an untrusted comment changed',
      key: tauri.keyLine,
      sig: sigOf(['untrusted comment: anything', ...lines.slice(1)]),
      file: sums,
      fileName: tauri.file,
      expect: null,
    },
    {
      name: 'a signature file with a line more',
      key: tauri.keyLine,
      sig: sigOf([...lines.slice(0, 4), 'extra', '']),
      file: sums,
      fileName: tauri.file,
      expect: 'the signature file is not a minisign signature',
    },
    {
      name: 'base64 with stray bits',
      key: tauri.keyLine,
      sig: sigOf([lines[0], `${lines[1].slice(0, -2)}B=`, ...lines.slice(2)]),
      file: sums,
      fileName: tauri.file,
      expect: 'the signature file is not a minisign signature',
    },
  ]
  // Signatures made here, in Tauri's shape: another release's, an update archive's, and one
  // whose trusted comment says more than the file.
  const key = makeKey()
  const made = Buffer.from(`${'0'.repeat(64)}  ConsensFlow_3.0.7_aarch64.dmg\n`)
  minisign.push(
    {
      name: 'a SUMS signature made here',
      key: key.keyLine,
      sig: key.sign(made, signedName('3.0.7')),
      file: made,
      fileName: signedName('3.0.7'),
      expect: null,
    },
    {
      name: "another release's SUMS",
      key: key.keyLine,
      sig: key.sign(made, signedName('3.0.7-alpha.1')),
      file: made,
      fileName: signedName('3.0.7'),
      expect:
        'the signature is for ConsensFlow_3.0.7-alpha.1_SHA256SUMS, not ConsensFlow_3.0.7_SHA256SUMS',
    },
    {
      name: "an update archive's signature",
      key: key.keyLine,
      sig: key.sign(made, 'ConsensFlow.app.tar.gz'),
      file: made,
      fileName: signedName('3.0.7'),
      expect: 'the signature is for ConsensFlow.app.tar.gz, not ConsensFlow_3.0.7_SHA256SUMS',
    },
    {
      name: 'a trusted comment with more after the file',
      key: key.keyLine,
      sig: key.sign(made, '', { comment: `timestamp:1\tfile:${signedName('3.0.7')}\tx` }),
      file: made,
      fileName: signedName('3.0.7'),
      expect: 'the signature is for',
    },
    {
      name: 'a trusted comment without a timestamp',
      key: key.keyLine,
      sig: key.sign(made, '', { comment: `file:${signedName('3.0.7')}` }),
      file: made,
      fileName: signedName('3.0.7'),
      expect: 'the signature is for something else',
    },
  )
  return {
    ed25519,
    blake2b,
    minisign: minisign.map((check) => ({
      ...check,
      file: Buffer.from(check.file).toString('hex'),
    })),
  }
}

/** The text between the line holding `begin` and the line holding `end` in `file`. */
function between(file, begin, end) {
  const lines = readFileSync(file, 'utf8').split('\n')
  const from = lines.findIndex((line) => line.includes(begin))
  const to = lines.findIndex((line, at) => at > from && line.includes(end))
  assert.ok(from >= 0 && to > from, `${path.basename(file)} marks its verifier`)
  return lines.slice(from + 1, to).join('\n')
}

const JXA_HARNESS = `
function run(argv) {
  const input = JSON.parse(argv[0]);
  return JSON.stringify({
    ed25519: input.ed25519.map((v) => ed25519Verify(fromHex(v.public), fromHex(v.message), fromHex(v.signature))),
    blake2b: input.blake2b.map((v) => Array.from(blake2b512(fromHex(v.message)), (b) => b.toString(16).padStart(2, "0")).join("")),
    minisign: input.minisign.map((v) => checkMinisign(v.key, v.sig, fromHex(v.file), v.fileName)),
  });
}
`

const PS_HARNESS = `
& {
    $ErrorActionPreference = 'Stop'
@VERIFIER@
    function FromHex([string]$hex) {
        $bytes = New-Object byte[] ($hex.Length / 2)
        for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [Convert]::ToByte($hex.Substring(2 * $i, 2), 16) }
        , $bytes
    }
    $battery = Get-Content -Raw -LiteralPath $args[0] | ConvertFrom-Json
    $ed = @(foreach ($v in $battery.ed25519) { [bool](Test-CfEd25519 (FromHex $v.public) (FromHex $v.message) (FromHex $v.signature)) })
    $blake = @(foreach ($v in $battery.blake2b) { $hash = Get-CfBlake2b512 (FromHex $v.message); -join ($hash | ForEach-Object { $_.ToString('x2') }) })
    $mini = @(foreach ($v in $battery.minisign) { $said = Test-CfMinisign $v.key $v.sig (FromHex $v.file) $v.fileName; if ($null -eq $said) { 'null' } else { $said } })
    [pscustomobject]@{ ed25519 = $ed; blake2b = $blake; minisign = $mini } | ConvertTo-Json -Compress -Depth 4
} @args
`

function onPath(program) {
  const ran = spawnSync(process.platform === 'win32' ? 'where' : 'which', [program], {
    encoding: 'utf8',
  })
  return ran.status === 0
}

const engines = [
  {
    // The battery's own check: what it expects is what OpenSSL answers, and the check the
    // release workflow makes of the signature it publishes (install/check-signature.mjs).
    name: "Node's crypto (OpenSSL), as the release workflow checks its signature",
    available: true,
    run(_, input) {
      const raw = (hex) =>
        crypto.createPublicKey({
          key: Buffer.concat([
            Buffer.from('302a300506032b6570032100', 'hex'),
            Buffer.from(hex, 'hex'),
          ]),
          format: 'der',
          type: 'spki',
        })
      const verify = (v) => {
        try {
          return crypto.verify(
            null,
            Buffer.from(v.message, 'hex'),
            raw(v.public),
            Buffer.from(v.signature, 'hex'),
          )
        } catch {
          return false
        }
      }
      const answers = {
        ed25519: input.ed25519.map(verify),
        blake2b: input.blake2b.map((v) =>
          crypto.createHash('blake2b512').update(Buffer.from(v.message, 'hex')).digest('hex'),
        ),
        minisign: input.minisign.map(
          (v) => minisignProblem(v.key, v.sig, Buffer.from(v.file, 'hex'), v.fileName) ?? 'null',
        ),
      }
      return { status: 0, stdout: JSON.stringify(answers), stderr: '' }
    },
  },
  {
    name: "install.sh's, in macOS's JavaScript (osascript)",
    available: process.platform === 'darwin',
    run(work, input) {
      const script = path.join(work, 'verify.js')
      writeFileSync(script, `${between(INSTALL_SH, "<<'VERIFIER'", 'VERIFIER')}\n${JXA_HARNESS}`)
      return spawnSync('osascript', ['-l', 'JavaScript', script, JSON.stringify(input)], {
        encoding: 'utf8',
        maxBuffer: 1 << 26,
      })
    },
  },
  ...[
    ['Windows PowerShell', 'powershell', process.platform === 'win32'],
    ['PowerShell 7', 'pwsh', onPath('pwsh')],
  ].map(([label, program, available]) => ({
    name: `install.ps1's, in ${label}`,
    available,
    run(work, input) {
      const data = path.join(work, 'battery.json')
      writeFileSync(data, JSON.stringify(input))
      const script = path.join(work, `verify-${program}.ps1`)
      writeFileSync(
        script,
        PS_HARNESS.replace('@VERIFIER@', () =>
          between(INSTALL_PS1, '# verifier: begin', '# verifier: end'),
        ),
      )
      return spawnSync(
        program,
        ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, data],
        { encoding: 'utf8', maxBuffer: 1 << 26 },
      )
    },
  })),
]

describe("the installers' signature check", () => {
  let work
  let input
  before(() => {
    work = mkdtempSync(path.join(os.tmpdir(), 'cf-install-verifiers-'))
    input = battery()
  })
  after(() => {
    if (work) rmSync(work, { recursive: true, force: true })
  })

  test("the release workflow's check takes no signature by another key than the one the installers pin", () => {
    const dir = mkdtempSync(path.join(work, 'signed-'))
    const sums = `${'0'.repeat(64)}  ConsensFlow_3.0.7_aarch64.dmg\n`
    writeFileSync(path.join(dir, 'SHA256SUMS'), sums)
    writeFileSync(
      path.join(dir, 'SHA256SUMS.sig'),
      makeKey().sign(Buffer.from(sums), signedName('3.0.7')),
    )
    const ran = spawnSync(
      process.execPath,
      [path.join(ROOT, 'install/check-signature.mjs'), '--dir', dir, '--version', '3.0.7'],
      { encoding: 'utf8' },
    )
    assert.equal(ran.status, 1, ran.stdout)
    assert.equal(
      ran.stderr,
      'check-signature: as Node checks it: the signature was made by another key\n',
    )
  })

  test("both scripts hold the update key the app's updater holds", () => {
    assert.equal(scriptKeyLine(readFileSync(INSTALL_SH, 'utf8')), updaterKeyLine())
    assert.equal(scriptKeyLine(readFileSync(INSTALL_PS1, 'utf8')), updaterKeyLine())
  })

  for (const engine of engines) {
    describe(engine.name, { skip: !engine.available && 'not on this machine' }, () => {
      let answers
      before(() => {
        const ran = engine.run(work, input)
        assert.equal(ran.status, 0, `${ran.stdout}\n${ran.stderr}`)
        answers = JSON.parse(ran.stdout.trim())
      })

      test("RFC 8032's Ed25519 vectors hold, and none changed by a bit, or with S + L, does", () => {
        assert.equal(answers.ed25519.length, input.ed25519.length)
        input.ed25519.forEach((vector, at) => {
          assert.equal(answers.ed25519[at], vector.expect, vector.name)
        })
      })

      test("BLAKE2b-512 gives RFC 7693's answer and OpenSSL's at every length around a block", () => {
        input.blake2b.forEach((vector, at) => {
          assert.equal(answers.blake2b[at], vector.hash, `${vector.message.length / 2} bytes`)
        })
      })

      test("tauri signer's signatures hold, and each changed one is refused, saying why", () => {
        input.minisign.forEach((check, at) => {
          const said = answers.minisign[at] === 'null' ? null : answers.minisign[at]
          if (check.expect === null) assert.equal(said, null, check.name)
          else
            assert.ok(
              typeof said === 'string' && said.startsWith(check.expect),
              `${check.name}: ${said}`,
            )
        })
      })
    })
  }
})
