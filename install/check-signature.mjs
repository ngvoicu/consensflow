// The release workflow's check of the signature it made of a release's
// SHA256SUMS, before the signature is published (the `sign` job of
// .github/workflows/release.yml):
//
//   node install/check-signature.mjs --dir DIR --version VERSION
//
// DIR holds SHA256SUMS and SHA256SUMS.sig. The signature must hold under the key
// both installers pin, which must be the key the app's updater holds, for the
// name ConsensFlow_<VERSION>_SHA256SUMS: as Node's crypto checks it, and, on a
// Mac, as install.sh's own check does, in osascript, which must end with status 0
// and answer the hash the list gives the release's DMG. Status 0 when it holds,
// else 1 with what does not.

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { parseArgs } from 'node:util'

import {
  INSTALL_PS1,
  INSTALL_SH,
  jxaCheck,
  minisignProblem,
  scriptKeyLine,
  signedName,
  updaterKeyLine,
} from './kit.mjs'

const { values } = parseArgs({ options: { dir: { type: 'string' }, version: { type: 'string' } } })
if (!values.dir || !values.version) {
  console.error('check-signature: --dir and --version are needed')
  process.exit(2)
}

function refuse(words) {
  console.error(`check-signature: ${words}`)
  process.exit(1)
}

const key = scriptKeyLine(readFileSync(INSTALL_SH, 'utf8'))
if (scriptKeyLine(readFileSync(INSTALL_PS1, 'utf8')) !== key)
  refuse('install.sh and install.ps1 pin different keys')
if (updaterKeyLine() !== key) refuse("the installers pin another key than the app's updater holds")

const sums = readFileSync(path.join(values.dir, 'SHA256SUMS'))
const sig = readFileSync(path.join(values.dir, 'SHA256SUMS.sig'), 'utf8')
const name = signedName(values.version)
const problem = minisignProblem(key, sig, sums, name)
if (problem !== null) refuse(`as Node checks it: ${problem}`)

if (process.platform === 'darwin') {
  const dmg = `ConsensFlow_${values.version}_aarch64.dmg`
  const said = jxaCheck(key, sig, sums, name, dmg)
  if (said !== null) refuse(`as install.sh checks it: ${said}`)
}
console.log(
  `check-signature: SHA256SUMS is signed by the update key the installers pin, as ${name}`,
)
