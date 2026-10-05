/**
 * `FORMAT.md` is written by hand, and keeps what the traces hold in blocks the
 * recorder rewrites, so that the document cannot go stale when a recording
 * changes: each worked example is a step of a trace (`<!-- example: NAME PATH -->`),
 * each file it shows is a file of the folder (`<!-- file: NAME -->`), and the
 * traces that depend on the platform they were recorded on are listed
 * (`<!-- list: posix -->`). `npm run goldens:daemon` rewrites them; the tests
 * hold the document to what `refreshed` makes of the checked-in files.
 */

const at = (value, path) => path.split('.').reduce((inside, key) => inside[key], value)
/** A fenced block, empty or not: the fence, what lies between, the fence that closes it. */
const FENCE = '\\n```(json|text)\\n(?:[\\s\\S]*?\\n)?```'

/** A command is found on Windows by an extension: these are the ones it takes. */
const EXTENSION = /\.(cmd|exe|bat|com)$/i

/**
 * The traces whose world holds a stand-in on `PATH` that only a POSIX machine
 * finds: a script named `claude` with no `claude.cmd` beside it.
 */
export function posixOnly(files) {
  return Object.keys(files)
    .filter((name) => name.endsWith('.json.gz'))
    .filter((name) =>
      JSON.parse(files[name]).steps.some((step) => {
        const paths = Object.keys(step.kind === 'world' ? (step.files ?? {}) : {})
        return paths.some(
          (path) =>
            path.startsWith('bin/') &&
            !EXTENSION.test(path) &&
            !paths.some((other) => other.startsWith(`${path}.`) && EXTENSION.test(other)),
        )
      }),
    )
    .map((name) => name.replace(/\.json\.gz$/, ''))
    .sort()
}

/**
 * `document` with each generated block as `files` (the folder as `record.mjs`
 * makes it: a trace is the text of its JSON) have it. Throws when a block
 * names a trace, a step or a file there is none of.
 */
export function refreshed(document, files) {
  const trace = (name) => {
    if (files[`${name}.json.gz`] === undefined)
      throw new Error(`the document names ${name}, which is no trace`)
    return JSON.parse(files[`${name}.json.gz`])
  }
  return document
    .replace(
      new RegExp(`(<!-- example: (\\S+) (\\S+) -->)${FENCE}`, 'g'),
      (_, marker, name, path) => {
        const step = at(trace(name), path)
        if (step === undefined)
          throw new Error(`the document shows ${name} ${path}, which is not there`)
        return `${marker}\n\`\`\`json\n${JSON.stringify(step, null, 2)}\n\`\`\``
      },
    )
    .replace(new RegExp(`(<!-- file: (\\S+) -->)${FENCE}`, 'g'), (_, marker, name) => {
      if (files[name] === undefined) throw new Error(`the document shows ${name}, which is no file`)
      return `${marker}\n\`\`\`json\n${files[name].trimEnd()}\n\`\`\``
    })
    .replace(
      new RegExp(`(<!-- list: posix -->)${FENCE}`, 'g'),
      (_, marker) => `${marker}\n\`\`\`text\n${posixOnly(files).join('\n')}\n\`\`\``,
    )
}
