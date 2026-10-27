// The installers' own check of a signature, written out in each script because
// no tool a Mac or Windows ships checks Ed25519: the JavaScript install.sh runs
// in macOS's osascript, and the PowerShell install.ps1 runs in Windows
// PowerShell 5.1 and PowerShell 7. Each is run here on the same battery:
// RFC 8032's Ed25519 vectors and their changed copies, BLAKE2b-512's known
// answers, signatures made by `tauri signer sign`, and a published release's
// signature by ConsensFlow's own update key (tests/fixtures/install, made by
// make-fixtures.mjs there); and the edges: encodings that are no point, S = L,
// signatures of another length, SHA-512 at its padding boundaries, the 64-bit
// words BLAKE2b adds and rotates, every malformed shape of a signature file
// (Astraeus's crypto audit's among them, T-806), the line of SHA256SUMS a hash
// is read from, and the order of the pinned key. Every engine on this machine
// is run; the CI runs these on macOS and on Windows, where both PowerShells are.

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
  jxaCheck,
  listedHash,
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

/** `base64` whose last character before its padding has bits the bytes do not use. */
function strayBits(base64) {
  const at = base64.indexOf('=') - 1
  assert.ok(at > 0, 'base64 with padding')
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
  return base64.slice(0, at) + alphabet[alphabet.indexOf(base64[at]) ^ 1] + base64.slice(at + 1)
}

/** `hex` as 32 little-endian bytes made a number, plus `n`, as 32 bytes again. */
function plusLittleEndian(hex, n) {
  const bytes = Buffer.from(hex, 'hex')
  let v = 0n
  for (let i = bytes.length - 1; i >= 0; i--) v = (v << 8n) | BigInt(bytes[i])
  v += n
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = Number(v & 255n)
    v >>= 8n
  }
  return bytes.toString('hex')
}

const MASK64 = (1n << 64n) - 1n

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
  // The edges of Ed25519 (RFC 8032 5.1.7): R or A that is no point, S = L exactly, a signature a
  // byte short or long, and signatures whose R || A || M ends at SHA-512's padding boundaries.
  const fileEdges = JSON.parse(fixture('ed25519-edges.json'))
  const rfc = JSON.parse(fixture('ed25519-rfc8032.json')).vectors[0]
  const edges = []
  for (const point of fileEdges.points.filter((p) => !p.decodes)) {
    edges.push({ ...rfc, name: `A is ${point.name}`, public: point.point, expect: false })
    edges.push({
      ...rfc,
      name: `R is ${point.name}`,
      signature: point.point + rfc.signature.slice(64),
      expect: false,
    })
  }
  const sOfRfc = rfc.signature.slice(64)
  edges.push(
    {
      ...rfc,
      name: 'S = L exactly',
      signature: rfc.signature.slice(0, 64) + plusLittleEndian('00'.repeat(32), L),
      expect: false,
    },
    {
      ...rfc,
      name: 'S + L (the RFC test 1 signature)',
      signature: rfc.signature.slice(0, 64) + plusLittleEndian(sOfRfc, L),
      expect: false,
    },
    { ...rfc, name: 'a byte short', signature: rfc.signature.slice(0, 126), expect: false },
    { ...rfc, name: 'a byte long', signature: `${rfc.signature}00`, expect: false },
    { ...rfc, name: 'a key a byte short', public: rfc.public.slice(0, 62), expect: false },
  )
  for (const vector of fileEdges.boundaries) {
    edges.push({ ...vector, expect: true })
    edges.push({
      ...vector,
      name: `${vector.name}, its last byte changed`,
      message: flipHex(vector.message, vector.message.length / 2 - 1),
      expect: false,
    })
  }

  // 64-bit words as BLAKE2b adds and rotates them: every carry, and the rotations it uses.
  const words = []
  const big = [
    0n,
    1n,
    0xffffffffn,
    0x100000000n,
    0xffffffff00000000n,
    0x8000000000000000n,
    0x7fffffffffffffffn,
    MASK64,
    0x0123456789abcdefn,
    0xfedcba9876543210n,
  ]
  for (const a of big) {
    for (const b of big)
      words.push(['add', a.toString(), b.toString(), ((a + b) & MASK64).toString()])
    for (const n of [16n, 24n, 32n, 63n, 1n]) {
      const rotated = ((a >> n) | (a << (64n - n))) & MASK64
      words.push(['rotr', a.toString(), n.toString(), rotated.toString()])
    }
  }

  // Every malformed shape of a signature file, as the installers read it: a .sig made here with
  // an untrusted comment long enough that each layer of base64 is padded.
  let shaped
  let shapedLines
  for (let extra = 0; ; extra++) {
    shaped = key.sign(made, signedName('3.0.7'))
    shapedLines = sigLines(shaped)
    shapedLines[0] = `untrusted comment: ${'x'.repeat(extra)}`
    shaped = sigOf(shapedLines)
    if (shaped.endsWith('==')) break
  }
  const notMinisign = 'the signature file is not a minisign signature'
  const envelope = (name, sig, expect = notMinisign) => ({
    name,
    key: key.keyLine,
    sig,
    file: made,
    fileName: signedName('3.0.7'),
    expect,
  })
  const [l0, l1, l2, l3] = shapedLines
  const crossed = makeKey()
  const crossedLines = sigLines(crossed.sign(made, signedName('3.0.7')))
  const sameId = Buffer.from(crossedLines[1], 'base64')
  Buffer.from(l1, 'base64').copy(sameId, 2, 2, 10)
  const legacy = Buffer.from(l1, 'base64')
  legacy[1] = 0x64
  const envelopes = [
    envelope('the shape itself', shaped, null),
    envelope('outer whitespace around it', `\n  ${shaped} \r\n`, null),
    envelope('the untrusted comment missing', sigOf([l1, l2, l3, ''])),
    envelope('the signature line missing', sigOf([l0, l2, l3, ''])),
    envelope('the trusted comment missing', sigOf([l0, l1, l3, ''])),
    envelope('the global signature missing', sigOf([l0, l1, l2, ''])),
    envelope('no line feed at the end', sigOf([l0, l1, l2, l3])),
    envelope('an empty line more', sigOf([l0, l1, l2, l3, '', ''])),
    envelope(
      'CRLF line ends',
      Buffer.from(`${[l0, l1, l2, l3].join('\r\n')}\r\n`, 'latin1').toString('base64'),
    ),
    envelope('a space before the signature line', sigOf([l0, ` ${l1}`, l2, l3, ''])),
    envelope('a space after the global signature', sigOf([l0, l1, l2, `${l3} `, ''])),
    envelope('untrusted comment without its words', sigOf(['untrusted: x', l1, l2, l3, ''])),
    envelope(
      'trusted comment without its colon',
      sigOf([l0, l1, l2.replace('trusted comment: ', 'trusted comment '), l3, '']),
    ),
    envelope('outer base64 with a space inside', `${shaped.slice(0, 40)} ${shaped.slice(40)}`),
    envelope('outer base64 without its padding', shaped.replace(/=+$/, '')),
    envelope('outer base64 with padding more', `${shaped}=`),
    envelope('outer base64 with stray bits', strayBits(shaped)),
    envelope(
      'the signature line without its padding',
      sigOf([l0, l1.replace(/=+$/, ''), l2, l3, '']),
    ),
    envelope('the signature line with stray bits', sigOf([l0, strayBits(l1), l2, l3, ''])),
    envelope(
      'the global signature without its padding',
      sigOf([l0, l1, l2, l3.replace(/=+$/, ''), '']),
    ),
    envelope('the global signature with stray bits', sigOf([l0, l1, l2, strayBits(l3), ''])),
    envelope(
      'the signature line a byte short',
      sigOf([l0, Buffer.from(l1, 'base64').subarray(0, 73).toString('base64'), l2, l3, '']),
    ),
    envelope(
      'the global signature a byte short',
      sigOf([l0, l1, l2, Buffer.from(l3, 'base64').subarray(0, 63).toString('base64'), '']),
    ),
    envelope(
      "another key, signing under this key's id",
      sigOf([l0, sameId.toString('base64'), crossedLines[2], crossedLines[3], '']),
      'the signature does not match the file',
    ),
    envelope(
      "minisign's legacy signature of the file itself (Ed)",
      sigOf([l0, legacy.toString('base64'), l2, l3, '']),
      "the signature is not of minisign's prehashed kind (ED)",
    ),
  ]
  // Astraeus's cases (T-806): a NUL after the last line feed, which no key signs, and a signed
  // file name with a NUL or a soft hyphen after it, which a culture comparison takes as equal.
  const audit = JSON.parse(fixture('powershell-parser-cases.json'))
  const auditExpect = {
    'unsigned trailing NUL': notMinisign,
    'signed file name plus "\\u0000"': 'the signature is for',
    'signed file name plus "\u00ad"': 'the signature is for',
  }
  for (const c of audit) {
    assert.ok(c.name in auditExpect, `an audit case this battery knows: ${c.name}`)
    envelopes.push({ ...c, file: Buffer.from(c.file, 'hex'), expect: auditExpect[c.name] })
  }

  // The line of SHA256SUMS a hash is read from.
  const h1 = 'a'.repeat(64)
  const h2 = 'b'.repeat(64)
  const dmg = 'ConsensFlow_3.0.7_aarch64.dmg'
  const listed = [
    ['the line that names it', `${h1}  ${dmg}\n${h2}  ConsensFlow_3.0.7_x64-setup.exe\n`, h1],
    ["sha256sum's binary mode", `${h1} *${dmg}\n`, h1],
    ['a tab between', `${h1}\t${dmg}\n`, h1],
    ['spaces before the hash', `  ${h1}  ${dmg}\n`, h1],
    ['two lines that name it', `${h1}  ${dmg}\n${h2}  ${dmg}\n`, null],
    ['an upper-case hash', `${h1.toUpperCase()}  ${dmg}\n`, null],
    ['a hash a digit short', `${h1.slice(1)}  ${dmg}\n`, null],
    ['a field more', `${h1}  ${dmg} x\n`, null],
    ['a CRLF line end', `${h1}  ${dmg}\r\n`, null],
    ['the name with a NUL after it', `${h1}  ${dmg}\u0000\n`, null],
    ['the name with a soft hyphen after it', `${h1}  ${dmg}\u00ad\n`, null],
    ['a longer name', `${h1}  ${dmg}.bak\n`, null],
    ['no line that names it', `${h2}  ConsensFlow_3.0.7_x64-setup.exe\n`, null],
  ].map(([name, text, expect]) => ({
    name,
    file: Buffer.from(text, 'latin1').toString('hex'),
    artifact: dmg,
    expect,
  }))

  return {
    ed25519,
    edges,
    blake2b,
    sha512: JSON.parse(fixture('sha512.json')).vectors,
    points: fileEdges.points,
    words,
    listed,
    pinned: Buffer.from(updaterKeyLine(), 'base64').subarray(10).toString('hex'),
    minisign: minisign.map((check) => ({
      ...check,
      file: Buffer.from(check.file).toString('hex'),
    })),
    envelopes: envelopes.map((check) => ({
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
  const hex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  const identity = "01" + "00".repeat(31);
  const pinned = decode(fromHex(input.pinned));
  return JSON.stringify({
    ed25519: input.ed25519.map((v) => ed25519Verify(fromHex(v.public), fromHex(v.message), fromHex(v.signature))),
    edges: input.edges.map((v) => ed25519Verify(fromHex(v.public), fromHex(v.message), fromHex(v.signature))),
    blake2b: input.blake2b.map((v) => hex(blake2b512(fromHex(v.message)))),
    sha512: input.sha512.map((v) => hex(sha512(fromHex(v.message)))),
    points: input.points.map((v) => decode(fromHex(v.point)) !== null),
    words: input.words.map(([op, a, b]) => (op === "add" ? (BigInt(a) + BigInt(b)) & MASK64 : rotr(BigInt(a), BigInt(b))).toString()),
    listed: input.listed.map((v) => listedHash(fromHex(v.file), v.artifact)),
    pinned: { order: hex(encode(multiply(L, pinned))) === identity, small: hex(encode(multiply(8n, pinned))) === identity },
    minisign: input.minisign.map((v) => checkMinisign(v.key, v.sig, fromHex(v.file), v.fileName)),
    envelopes: input.envelopes.map((v) => checkMinisign(v.key, v.sig, fromHex(v.file), v.fileName)),
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
    function ToHex([byte[]]$bytes) { -join ($bytes | ForEach-Object { $_.ToString('x2') }) }
    $battery = Get-Content -Raw -LiteralPath $args[0] | ConvertFrom-Json
    $ed = @(foreach ($v in $battery.ed25519) { [bool](Test-CfEd25519 (FromHex $v.public) (FromHex $v.message) (FromHex $v.signature)) })
    $edges = @(foreach ($v in $battery.edges) { [bool](Test-CfEd25519 (FromHex $v.public) (FromHex $v.message) (FromHex $v.signature)) })
    $blake = @(foreach ($v in $battery.blake2b) { ToHex (Get-CfBlake2b512 (FromHex $v.message)) })
    $sha = [System.Security.Cryptography.SHA512]::Create()
    $sha512 = @(foreach ($v in $battery.sha512) { ToHex $sha.ComputeHash((FromHex $v.message)) })
    $points = @(foreach ($v in $battery.points) { $null -ne (CfDecode (FromHex $v.point)) })
    $words = @(foreach ($w in $battery.words) {
            if ($w[0] -eq 'add') { (CfAdd64 ([uint64]::Parse($w[1])) ([uint64]::Parse($w[2]))).ToString() }
            else { (CfRotr64 ([uint64]::Parse($w[1])) ([int]$w[2])).ToString() }
        })
    $listed = @(foreach ($v in $battery.listed) { $said = Get-CfListedHash (FromHex $v.file) $v.artifact; if ($null -eq $said) { 'null' } else { $said } })
    $identity = '01' + ('00' * 31)
    $pinnedPoint = CfDecode (FromHex $battery.pinned)
    $pinned = [pscustomobject]@{
        order = (ToHex (CfEncode (CfMultiply $CfL $pinnedPoint))) -eq $identity
        small = (ToHex (CfEncode (CfMultiply ([System.Numerics.BigInteger]8) $pinnedPoint))) -eq $identity
    }
    $mini = @(foreach ($v in $battery.minisign) { $said = Test-CfMinisign $v.key $v.sig (FromHex $v.file) $v.fileName; if ($null -eq $said) { 'null' } else { $said } })
    $env = @(foreach ($v in $battery.envelopes) { $said = Test-CfMinisign $v.key $v.sig (FromHex $v.file) $v.fileName; if ($null -eq $said) { 'null' } else { $said } })
    [pscustomobject]@{ ed25519 = $ed; edges = $edges; blake2b = $blake; sha512 = $sha512; points = $points; words = $words; listed = $listed; pinned = $pinned; minisign = $mini; envelopes = $env } | ConvertTo-Json -Compress -Depth 4
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
      const problem = (v) =>
        minisignProblem(v.key, v.sig, Buffer.from(v.file, 'hex'), v.fileName) ?? 'null'
      const answers = {
        ed25519: input.ed25519.map(verify),
        edges: input.edges.map(verify),
        blake2b: input.blake2b.map((v) =>
          crypto.createHash('blake2b512').update(Buffer.from(v.message, 'hex')).digest('hex'),
        ),
        sha512: input.sha512.map((v) =>
          crypto.createHash('sha512').update(Buffer.from(v.message, 'hex')).digest('hex'),
        ),
        // OpenSSL has no point arithmetic to ask: what decodes, and the order of the key, are
        // the scripts' own questions.
        points: null,
        pinned: null,
        words: input.words.map(([op, a, b]) =>
          (op === 'add'
            ? (BigInt(a) + BigInt(b)) & MASK64
            : ((BigInt(a) >> BigInt(b)) | (BigInt(a) << (64n - BigInt(b)))) & MASK64
          ).toString(),
        ),
        listed: input.listed.map((v) => listedHash(Buffer.from(v.file, 'hex'), v.artifact)),
        minisign: input.minisign.map(problem),
        envelopes: input.envelopes.map(problem),
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

  test("the release workflow's check of install.sh's verifier takes its word only from a clean exit", (t) => {
    if (process.platform !== 'darwin') return t.skip("osascript is the Mac's")
    const key = makeKey()
    const sums = Buffer.from(`${'0'.repeat(64)}  ConsensFlow_3.0.7_aarch64.dmg\n`)
    const sig = key.sign(sums, signedName('3.0.7'))
    const args = [key.keyLine, sig, sums, signedName('3.0.7'), 'ConsensFlow_3.0.7_aarch64.dmg']
    assert.equal(jxaCheck(...args), null)
    // An osascript that says the word and ends badly.
    const bin = mkdtempSync(path.join(work, 'bin-'))
    writeFileSync(path.join(bin, 'osascript'), `#!/bin/sh\necho "ok ${'0'.repeat(64)}"\nexit 3\n`, {
      mode: 0o755,
    })
    const said = jxaCheck(...args, { PATH: `${bin}:${process.env.PATH}` })
    assert.match(said ?? '', /ended with status 3/)
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

      test('Ed25519 refuses what RFC 8032 refuses: no point, S = L, another length; and holds at the padding boundaries', () => {
        assert.equal(answers.edges.length, input.edges.length)
        input.edges.forEach((vector, at) => {
          assert.equal(answers.edges[at], vector.expect, vector.name)
        })
      })

      test("SHA-512 gives OpenSSL's answer at every length 0 to 300 and around its blocks", () => {
        assert.equal(answers.sha512.length, input.sha512.length)
        input.sha512.forEach((vector, at) => {
          assert.equal(answers.sha512[at], vector.hash, `${vector.message.length / 2} bytes`)
        })
      })

      test('an encoding decodes as RFC 8032 5.1.3 says: no y at or above p, no x = 0 with its sign bit, no point off the curve', (t) => {
        if (answers.points === null) return t.skip('no point arithmetic to ask')
        input.points.forEach((point, at) => {
          assert.equal(answers.points[at], point.decodes, point.name)
        })
      })

      test("BLAKE2b's 64-bit words add with every carry and rotate as it needs", () => {
        assert.equal(answers.words.length, input.words.length)
        input.words.forEach(([op, a, b, expect], at) => {
          assert.equal(answers.words[at], expect, `${op} ${a} ${b}`)
        })
      })

      test('the hash of a file is read from the one line that names it exactly', () => {
        input.listed.forEach((check, at) => {
          const said = answers.listed[at] === 'null' ? null : answers.listed[at]
          assert.equal(said, check.expect, check.name)
        })
      })

      test('the pinned key has order L, and no small order', (t) => {
        if (answers.pinned === null) return t.skip('no point arithmetic to ask')
        assert.deepEqual(answers.pinned, { order: true, small: false })
      })

      test('every malformed shape of a signature file is refused, saying why', () => {
        assert.equal(answers.envelopes.length, input.envelopes.length)
        input.envelopes.forEach((check, at) => {
          const said = answers.envelopes[at] === 'null' ? null : answers.envelopes[at]
          if (check.expect === null) assert.equal(said, null, check.name)
          else
            assert.ok(
              typeof said === 'string' && said.startsWith(check.expect),
              `${check.name}: ${said}`,
            )
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
