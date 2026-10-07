import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The updater key of one smoke run: a pair made for the run (`tauri signer
 * generate`), whose public half the apps are built with and whose private
 * half signs the archives the run serves. It never leaves the run's folder,
 * and nothing here reads, uses or copies the product's own key (in
 * `~/.tauri`) or any Apple key or certificate: what a build or a signer
 * inherits of those from the caller's shell is taken out first, and the
 * signer is told the one file it may read.
 */

/** The variables that carry an updater key, an Apple certificate or an identity to sign with. */
const SIGNING_VARIABLES = /^(TAURI_|APPLE_|CSC_)/

/**
 * The environment a build or a signer runs in: the caller's, without any
 * signing variable, and offline (cargo reads no network, so a crate not
 * already fetched fails the build, as `--offline` has it).
 */
export function cleanEnv(base = process.env) {
  const env = Object.fromEntries(
    Object.entries(base).filter(([name]) => !SIGNING_VARIABLES.test(name)),
  )
  env.CARGO_NET_OFFLINE = 'true'
  return env
}

/** The Tauri CLI a checkout has, a script `node` runs. */
export function tauriCli(checkout) {
  return join(checkout, 'app', 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
}

function tauri(checkout, args) {
  try {
    return execFileSync(process.execPath, [tauriCli(checkout), ...args], {
      cwd: checkout,
      env: cleanEnv(),
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  } catch (cause) {
    const said = cause?.stderr?.toString?.().trim() || cause.message
    throw new Error(`tauri ${args.slice(0, 2).join(' ')} failed: ${said}`)
  }
}

/**
 * A new key pair in `directory`, with no password: the private key and the
 * public key (`<key>.pub`, in the form `plugins.updater.pubkey` takes).
 */
export function generateKey(checkout, directory) {
  mkdirSync(directory, { recursive: true })
  const privateKey = join(directory, 'updater.key')
  tauri(checkout, ['signer', 'generate', '--ci', '--password', '', '--write-keys', privateKey])
  return {
    privateKey,
    publicKeyFile: `${privateKey}.pub`,
    publicKey: readFileSync(`${privateKey}.pub`, 'utf8').trim(),
  }
}

/** The signature of `file` by `privateKey`, as the feed carries it (the `.sig` file's one line). */
export function signFile(checkout, privateKey, file) {
  tauri(checkout, ['signer', 'sign', '--password', '', '--private-key-path', privateKey, file])
  return readFileSync(`${file}.sig`, 'utf8').trim()
}
