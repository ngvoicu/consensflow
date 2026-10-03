/**
 * What every harness's reader shares: the shape of a reading, the native ids
 * and text of its items, the lookup of a session's file, and the reading on
 * of a JSONL file from where the last look stopped.
 */
import fs from 'node:fs/promises'
import path from 'node:path'

const describeError = (error) => (error instanceof Error ? error.message : String(error))
export const unreadable = (error) => ({
  unknown: true,
  reason: `unreadable: ${describeError(error)}`,
})
export const home = (env) => {
  const value = env.HOME ?? env.USERPROFILE
  if (typeof value !== 'string' || value.length === 0) throw new Error('missing home in env')
  return value
}

/** What a reading says, its items in the record's order. */
export function resultBase() {
  return {
    items: [],
    inFlight: false,
    // Its own question dialog is open: the window waits for the human's answer.
    asking: false,
    failed: false,
    quota: null,
    settlement: { state: 'unknown' },
  }
}

/**
 * An item as a reading returns it, without the reader's own fields: a copy,
 * so that a later look, which may still complete or grow the item it was
 * read from, never changes an earlier answer.
 */
export function emit(item) {
  const { id, role, text, complete, at } = item
  return item.commentary
    ? { id, role, text, complete, at, commentary: true }
    : { id, role, text, complete, at }
}

export function nativeId(value, kind, seq) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`missing native ${kind} id at record ${seq}`)
  }
  return value
}

export function visibleText(value) {
  if (typeof value === 'string') return value
  if (value === undefined || value === null) return ''
  if (Array.isArray(value)) {
    const text = value
      .map((part) => {
        if (typeof part === 'string') return part
        if (part && typeof part.text === 'string') return part.text
        return JSON.stringify(part)
      })
      .join('\n')
    return text
  }
  return JSON.stringify(value)
}

/** Depth-first lookup; all harness stores remain read-only. */
export async function findFile(root, matches, depth = 6) {
  if (depth < 0) return null
  let entries
  try {
    entries = await fs.readdir(root, { withFileTypes: true })
  } catch {
    return null
  }
  for (const entry of entries) {
    const full = path.join(root, entry.name)
    if (entry.isFile() && matches(entry.name)) return full
    if (entry.isDirectory()) {
      const found = await findFile(full, matches, depth - 1)
      if (found !== null) return found
    }
  }
  return null
}

function isJsonWhitespace(character) {
  return character === ' ' || character === '\t' || character === '\r' || character === '\n'
}

function isJsonBlank(source) {
  for (const character of source) {
    if (!isJsonWhitespace(character)) return false
  }
  return true
}

function jsonPrefixState(source) {
  const INCOMPLETE = Symbol('incomplete')
  const INVALID = Symbol('invalid')
  let at = 0

  const skipSpace = () => {
    while (at < source.length && isJsonWhitespace(source[at])) at += 1
  }
  const need = () => {
    if (at >= source.length) throw INCOMPLETE
  }
  const parseString = () => {
    need()
    if (source[at] !== '"') throw INVALID
    at += 1
    while (at < source.length) {
      const character = source[at]
      at += 1
      if (character === '"') return
      if (character.charCodeAt(0) < 0x20) throw INVALID
      if (character !== '\\') continue
      need()
      const escapeCode = source[at]
      at += 1
      if ('"\\/bfnrt'.includes(escapeCode)) continue
      if (escapeCode !== 'u') throw INVALID
      for (let digit = 0; digit < 4; digit += 1) {
        need()
        if (!/[0-9a-f]/i.test(source[at])) throw INVALID
        at += 1
      }
    }
    throw INCOMPLETE
  }
  const parseNumber = () => {
    if (source[at] === '-') {
      at += 1
      need()
    }
    if (source[at] === '0') {
      at += 1
      if (/[0-9]/.test(source[at] ?? '')) throw INVALID
    } else if (/[1-9]/.test(source[at] ?? '')) {
      while (/[0-9]/.test(source[at] ?? '')) at += 1
    } else {
      throw INVALID
    }
    if (source[at] === '.') {
      at += 1
      need()
      if (!/[0-9]/.test(source[at])) throw INVALID
      while (/[0-9]/.test(source[at] ?? '')) at += 1
    }
    if (source[at] === 'e' || source[at] === 'E') {
      at += 1
      need()
      if (source[at] === '+' || source[at] === '-') {
        at += 1
        need()
      }
      if (!/[0-9]/.test(source[at])) throw INVALID
      while (/[0-9]/.test(source[at] ?? '')) at += 1
    }
  }
  const parseLiteral = (literal) => {
    for (const expected of literal) {
      need()
      if (source[at] !== expected) throw INVALID
      at += 1
    }
  }
  const parseValue = () => {
    skipSpace()
    need()
    const character = source[at]
    if (character === '"') return parseString()
    if (character === '{') return parseObject()
    if (character === '[') return parseArray()
    if (character === 't') return parseLiteral('true')
    if (character === 'f') return parseLiteral('false')
    if (character === 'n') return parseLiteral('null')
    if (character === '-' || /[0-9]/.test(character)) return parseNumber()
    throw INVALID
  }
  const parseObject = () => {
    at += 1
    skipSpace()
    need()
    if (source[at] === '}') {
      at += 1
      return
    }
    while (true) {
      parseString()
      skipSpace()
      need()
      if (source[at] !== ':') throw INVALID
      at += 1
      parseValue()
      skipSpace()
      need()
      if (source[at] === '}') {
        at += 1
        return
      }
      if (source[at] !== ',') throw INVALID
      at += 1
      skipSpace()
      need()
    }
  }
  const parseArray = () => {
    at += 1
    skipSpace()
    need()
    if (source[at] === ']') {
      at += 1
      return
    }
    while (true) {
      parseValue()
      skipSpace()
      need()
      if (source[at] === ']') {
        at += 1
        return
      }
      if (source[at] !== ',') throw INVALID
      at += 1
      skipSpace()
      need()
    }
  }

  try {
    parseValue()
    skipSpace()
    return at === source.length ? 'complete' : 'invalid'
  } catch (error) {
    return error === INCOMPLETE ? 'incomplete' : 'invalid'
  }
}

// ================================================================= JSONL

const EMPTY = Buffer.alloc(0)
/** How many bytes before where a look stopped the next look checks are unchanged. */
const EDGE_BYTES = 1024
/** Thrown by a parser that must have its transcript's records again, from the first. */
export const REREAD = Symbol('read the transcript again')

/**
 * Reads on in a JSONL file from where an earlier look stopped (`seen`; null
 * reads from the start), handing each record found to `visit` with its place
 * among the file's records, without holding the file. A line is a record
 * once its newline is written. An unterminated last line that is already
 * whole JSON is visited too, and remembered, so that the newline which ends
 * it later adds nothing; one that is not whole yet waits for a later look,
 * and one that never can be fails, as a malformed whole line does. Returns
 * where the next look starts (`seen` itself when the file did not change),
 * or null when the file is not the one read so far: another file took its
 * place, or it no longer holds the bytes just before where the last look
 * stopped (it shrank, or was written over). With `only`, a line that does
 * not hold that text is passed over unparsed: a log of many conversations,
 * read for one, parses that one's lines alone.
 */
export async function readOn(file, seen, visit, { only = null } = {}) {
  const wanted = only === null ? null : Buffer.from(only)
  const handle = await fs.open(file, 'r')
  try {
    const { ino, size, mtimeMs } = await handle.stat()
    if (seen !== null) {
      if (ino !== seen.ino) return null
      if (size === seen.size && mtimeMs === seen.mtimeMs) return seen
      if (!(await holds(handle, seen.offset - seen.edge.length, seen.edge))) return null
      if (!(await holds(handle, seen.offset, seen.tail))) return null
    }
    let offset = seen?.offset ?? 0
    let tail = seen?.tail ?? EMPTY
    let records = seen?.records ?? 0
    // The line being read, in pieces; `visited` while it is the rest of `tail`'s line.
    let pieces = []
    let visited = tail.length > 0
    let at = offset + tail.length
    if (at < size) {
      const stream = handle.createReadStream({ start: at, end: size - 1, autoClose: false })
      for await (const chunk of stream) {
        let start = 0
        for (let newline = chunk.indexOf(10); newline !== -1; newline = chunk.indexOf(10, start)) {
          pieces.push(chunk.subarray(start, newline))
          const line = pieces.length === 1 ? pieces[0] : Buffer.concat(pieces)
          pieces = []
          if (visited) {
            // A record visited whole may be followed by nothing but whitespace.
            if (!isBlankBytes(line)) return null
            visited = false
            tail = EMPTY
          } else if (wanted === null || line.includes(wanted)) {
            records = consumeLine(line.toString('utf8'), records, visit)
          }
          offset = at + newline + 1
          start = newline + 1
        }
        pieces.push(chunk.subarray(start))
        at += chunk.length
      }
    }
    const rest = Buffer.concat(pieces)
    if (visited) {
      if (!isBlankBytes(rest)) return null
    } else if (!isBlankBytes(rest) && (wanted === null || rest.includes(wanted))) {
      const text = rest.toString('utf8')
      let record
      try {
        record = JSON.parse(text)
      } catch {
        // A live writer may have left only the final, unterminated append.
        if (jsonPrefixState(text) !== 'incomplete') {
          throw new Error(`malformed JSONL at record ${records}`)
        }
      }
      if (record !== undefined) {
        visit(record, records)
        records += 1
        tail = rest
      }
    }
    const edge =
      offset === seen?.offset
        ? seen.edge
        : await bytesAt(handle, Math.max(0, offset - EDGE_BYTES), offset)
    return { ino, size: at, mtimeMs, offset, edge, tail, records }
  } finally {
    await handle.close()
  }
}

/** One whole line: a record for `visit` at `index`, nothing when blank, a failure when malformed. */
function consumeLine(raw, index, visit) {
  const text = raw.endsWith('\r') ? raw.slice(0, -1) : raw
  if (isJsonBlank(text)) return index
  let record
  try {
    record = JSON.parse(text)
  } catch {
    throw new Error(`malformed JSONL at record ${index}`)
  }
  visit(record, index)
  return index + 1
}

function isBlankBytes(bytes) {
  for (const byte of bytes) {
    if (byte !== 0x20 && byte !== 0x09 && byte !== 0x0d && byte !== 0x0a) return false
  }
  return true
}

async function bytesAt(handle, from, to) {
  const bytes = Buffer.alloc(to - from)
  const { bytesRead } = await handle.read(bytes, 0, bytes.length, from)
  return bytes.subarray(0, bytesRead)
}

/** Whether the file still holds `bytes` at `at`. */
async function holds(handle, at, bytes) {
  if (bytes.length === 0) return true
  return (await bytesAt(handle, at, at + bytes.length)).equals(bytes)
}

const sameFile = (left, right) =>
  left.ino === right.ino && left.size === right.size && left.mtimeMs === right.mtimeMs

/**
 * A conversation's JSONL transcript, followed from look to look. `read()`
 * locates it with `locate` (again, once its file is gone), reads on from
 * where the last look stopped into the state `parse` makes, and says whether
 * it read anything; null when the harness keeps no transcript of the
 * conversation. A transcript that is not the one read so far is read again
 * from its start, into a fresh state. One that could not be read is not read
 * again until it changes.
 */
export function followedTranscript(locate, parse) {
  let file = null
  let seen = null
  let state = null
  let broken = null
  return {
    async read() {
      let stat = file === null ? null : await fs.stat(file).catch(() => null)
      if (stat === null) {
        const located = await locate()
        if (located !== file) {
          seen = null
          state = null
          broken = null
        }
        file = located
        if (file === null) return null
        stat = await fs.stat(file)
      }
      if (broken !== null && sameFile(broken.stat, stat)) throw broken.error
      broken = null
      for (;;) {
        state ??= parse()
        let next = null
        try {
          next = await readOn(file, seen, state.visit)
          if (next !== null) state.flush?.()
        } catch (error) {
          if (error !== REREAD) {
            seen = null
            state = null
            broken = { stat, error }
            throw error
          }
        }
        if (next !== null) {
          const changed = next !== seen
          seen = next
          return { file, state, changed }
        }
        seen = null
        state = null
      }
    },
  }
}

/**
 * The reader of a harness whose answer is its transcript's alone (Codex,
 * Claude Code): a look that read nothing new returns the previous answer.
 */
export function transcriptReader(locate, parse, missing) {
  const transcript = followedTranscript(locate, parse)
  let answer = null
  return async () => {
    try {
      const read = await transcript.read()
      if (read === null) {
        answer = null
        // A thread with no transcript yet is unknown: missing history alone proves nothing.
        return { unknown: true, reason: `unreadable: no ${missing}` }
      }
      if (read.changed || answer === null) answer = read.state.result()
      return answer
    } catch (error) {
      answer = null
      return unreadable(error)
    }
  }
}
