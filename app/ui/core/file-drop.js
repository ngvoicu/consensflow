/**
 * A file dropped on a terminal window types its path there, as a terminal
 * does. Tauri catches a file dropped on the app's window, so the page never
 * gets it as a drag of its own: Tauri says the drag in its events
 * (`tauri://drag-enter`, `-over` and `-leave`, with the paths and where the
 * pointer is), and the app says the drop itself (`file-drop`, as Tauri's
 * `tauri://drag-drop` with the number the app keeps it by, which the page asks
 * of it by: file_drop.rs). Hearing them takes no permission of its own: `listen`
 * is `core:event:allow-listen`, which `core:default` holds and the page's
 * other events already use. Which window is under the pointer, and how it
 * shows that a drop goes there, is the dock's (terminals.js); what a drop
 * types, and whether the window can take it as it is, is this module's. A
 * path is never typed changed: a drop that cannot type one as it is types
 * nothing, and says why.
 */
/** Whether this is Windows, where a path is quoted and the app's positions are in device pixels. */
const WINDOWS = /Windows/.test(globalThis.navigator?.userAgent ?? '')

/**
 * What a shell reads as more than itself in a word, and the space that ends
 * one: a path dropped on a Mac gets a backslash before each, as Terminal.app
 * writes one. Every other character stays as it is, non-ASCII too (a Mac's
 * screenshots have a narrow no-break space before AM, which no shell reads as
 * a space).
 */
const SHELL_SPECIAL = /[ !"#$&'()*;<>?[\\\]^`{|}~]/g

/**
 * Whether `character` is a control character (U+0000 to U+001F, and DEL),
 * which a Mac's file name may hold and no Windows one can. No window takes one
 * as part of a path: typed as it is, a line break or a carriage return ends the
 * line (a paste's line breaks reach the program as Enter), a tab completes, an
 * escape begins a key, a delete rubs out; and the windows are no shells, so no
 * quoting a shell reads (`$'…'`, a backslash before a line break) is read back
 * as the name by Pi, OpenCode, Codex or Claude Code.
 */
const control = (character) => character < ' ' || character === '\x7f'

/** The first of `paths` whose name holds a control character, if one does. */
export const withControl = (paths) => paths.find((path) => Array.from(path).some(control))

/** `/a/b␊c.png`: a path as the status line shows it, each control character by its picture. */
const shown = (path) =>
  Array.from(path, (character) => {
    if (!control(character)) return character
    return character === '\x7f' ? '␡' : String.fromCodePoint(0x2400 + character.charCodeAt(0))
  }).join('')

/**
 * One dropped path, written as the system's shell takes it as one word: on
 * Windows in double quotes if it has a space, as Windows Terminal does (a
 * Windows file name has no double quote); elsewhere with a backslash before
 * each character of `SHELL_SPECIAL`.
 */
function word(path, windows) {
  if (windows) return path.includes(' ') ? `"${path}"` : path
  return path.replace(SHELL_SPECIAL, '\\$&')
}

/**
 * What a drop of `paths` types: each path as one word of the system's shell,
 * a space between them and none after the last. Nothing else cuts or changes
 * a path. No paths type nothing.
 */
export function droppedText(paths, windows) {
  return paths.map((path) => word(path, windows)).join(' ')
}

/**
 * The characters of `path` a window on `harness` does not get as they are typed into it,
 * each once, on a system that is `windows` or not: on Windows a Codex window drops every
 * character outside the Basic Multilingual Plane (an emoji, two key events of the console's;
 * Codex 0.164.0 from the app's pane and from Windows Terminal alike, T-548). Every other
 * window, and Codex elsewhere, gets every character of a path whole. None where the path
 * reaches it as it is.
 */
export function untakeable(path, harness, windows) {
  if (!windows || harness !== 'codex') return []
  return [...new Set(path)].filter((character) => character.codePointAt(0) > 0xffff)
}

/** `— (U+2014), “ (U+201C)`: characters as the status line names them, those that show nothing too. */
const named = (characters) =>
  characters
    .map(
      (character) =>
        `${character} (U+${character.codePointAt(0).toString(16).toUpperCase().padStart(4, '0')})`,
    )
    .join(', ')

/**
 * Where a drop is in the page's own pixels, which is what `elementFromPoint`
 * takes. The app says it as a `PhysicalPosition` on every system, but from
 * the pointer's place in the webview as each system gave it: on Windows in
 * device pixels (wry's `ScreenToClient`), which the display's scale `ratio`
 * divides into the page's; on a Mac in points (wry's `draggingLocation`,
 * counted from the top), which are the page's pixels already, whatever the
 * display's scale.
 */
export function pagePoint({ x, y }, { windows, ratio }) {
  const scale = windows ? ratio : 1
  return { x: x / scale, y: y / scale }
}

/**
 * How often the page asks again for short names the app would not look up while
 * another lookup ran, and for how long at most (in ms): the one running may be a
 * page's before a reload, reading a share far away.
 */
const BUSY = { every: 250, within: 60_000 }

/**
 * What `ask()` answers once the app is free to look up: asked again every
 * `every` ms while the answer is `busy`, for `within` ms at most, after which the
 * last answer is the one. `wait(ms)` waits.
 */
export async function askedUntilFree(
  ask,
  { every, within } = BUSY,
  wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
) {
  for (let waited = 0; ; waited += every) {
    const answer = await ask()
    if (answer?.busy !== true || waited >= within) return answer
    await wait(every)
  }
}

/**
 * The paths as the window on `harness` takes them: each as it is where it can,
 * else by the short name Windows keeps for it (8.3, made of what the console
 * carries) where that one it can. The app looks them up
 * (`short_paths`), only for the paths of the drop it heard itself, which the
 * page names by the number `id` the app gave it, and asks again while the app is
 * busy with another lookup. `{ refused, why }` is the first it can take neither
 * way, and why its short name is of no use, in the app's words where the lookup
 * failed.
 */
async function keyed(id, paths, harness, invoke) {
  const untaken = (path) => untakeable(path, harness, WINDOWS)
  const changed = paths.filter((path) => untaken(path).length > 0)
  if (changed.length === 0) return { paths }
  const answer = await askedUntilFree(() =>
    invoke('short_paths', { id, paths: changed }).catch((cause) => ({
      ok: false,
      error: cause instanceof Error ? cause.message : String(cause),
    })),
  )
  const typed = []
  for (const path of paths) {
    const at = changed.indexOf(path)
    if (at === -1) {
      typed.push(path)
      continue
    }
    const found = answer?.ok === true ? answer.paths[at] : { error: answer?.error }
    if (typeof found?.short !== 'string') {
      return { refused: path, why: found?.error ?? 'the app did not answer for its short name' }
    }
    if (untaken(found.short).length > 0) {
      return { refused: path, why: 'its short name holds them too' }
    }
    typed.push(found.short)
  }
  return { paths: typed }
}

/**
 * Types the `paths` of the drop numbered `id` into the window `target` is (the
 * dock's `dropTarget`), or says why it types nothing: no window where the drop
 * was meant for one, a name with a control character, one the window would not
 * get as it is and has no usable short name for, or the window closed
 * meanwhile. `still()` says whether the human has done nothing since the drop:
 * only then does the window take the keyboard, and on Windows the app's window
 * come in front (the file came from another program, which the keyboard would
 * stay with, as Windows Terminal found); a drop typed after the human went on
 * elsewhere takes neither.
 */
async function type(target, { id, paths }, { invoke, focus, dock, report, still }) {
  if (target === null) return
  if (target.missed !== undefined) {
    report(`Nothing was typed: ${target.missed}.`)
    return
  }
  const controlled = withControl(paths)
  if (controlled !== undefined) {
    report(
      `Nothing was typed: the name of ${shown(controlled)} holds a control character, which no window can take.`,
    )
    return
  }
  const typed = await keyed(id, paths, target.harness, invoke)
  if (typed.refused !== undefined) {
    const characters = named(untakeable(typed.refused, target.harness, WINDOWS))
    report(
      `Nothing was typed: ${target.name}'s window cannot take ${characters} of ${typed.refused}, and ${typed.why}.`,
    )
    return
  }
  const focused = still()
  if (!dock.pasteInto(target.pane, droppedText(typed.paths, WINDOWS), { focus: focused })) {
    report(`Nothing was typed: ${target.name}'s window has closed.`)
    return
  }
  if (WINDOWS && focused) await focus(id)
}

/**
 * Hears the files dragged over the app's window and dropped on it. While they
 * are over a point of the page, the `dock` marks the running window there
 * (`markDrop(point)`, and `markDrop(null)` once the drag has ended, by a leave
 * or after the drop); a drop types their paths into the window it was on, in
 * the order the drops came, one waiting for the short names of the one before.
 * A drag that carries no file (text dragged from another program) marks nothing
 * and types nothing. `listen` and `invoke` are the app's, `focus(id)` brings its
 * window in front once the paths of the drop numbered `id` are typed, and
 * `report` says on the status line why nothing was typed.
 */
export async function listenForFileDrops({ listen, invoke, focus, dock, report }) {
  /** Whether the drag over the page carries files: its `over` events do not say. */
  let carrying = false
  /** The drops before this one, typed or not yet. */
  let typing = Promise.resolve()
  /** How often the human has acted since the page began: pressed a key or a button, come or gone. */
  let acts = 0
  const act = () => {
    acts += 1
  }
  for (const name of ['pointerdown', 'keydown']) globalThis.addEventListener(name, act, true)
  for (const name of ['focus', 'blur']) globalThis.addEventListener(name, act)
  const at = (position) =>
    pagePoint(position, { windows: WINDOWS, ratio: globalThis.devicePixelRatio ?? 1 })
  await listen('tauri://drag-enter', ({ payload }) => {
    carrying = payload.paths.length > 0
    if (carrying) dock.markDrop(at(payload.position))
  })
  await listen('tauri://drag-over', ({ payload }) => {
    if (carrying) dock.markDrop(at(payload.position))
  })
  await listen('tauri://drag-leave', () => {
    carrying = false
    dock.markDrop(null)
  })
  // A drop ends the drag with no leave before it, once the dock has said where
  // it went: a window that ended under the file is the drag's to say. The app
  // says it, not Tauri, with the number it keeps the drop by.
  await listen('file-drop', ({ payload }) => {
    carrying = false
    const target = dock.dropTarget(at(payload.position))
    dock.markDrop(null)
    if (payload.paths.length === 0) return
    const asOf = acts
    const still = () => acts === asOf
    const dropped = () => type(target, payload, { invoke, focus, dock, report, still })
    typing = typing.then(dropped).catch(report)
  })
}
