/**
 * How the page builds what it shows: elements with their text set as text,
 * never markup, so an agent-written title cannot become HTML, and a redraw
 * that leaves alone whatever did not change.
 */

export function element(tag, className, text) {
  const node = document.createElement(tag)
  if (className) node.className = className
  if (text !== undefined && text !== null) node.textContent = text
  return node
}

/** A button that does `action`; `label`, when given, names it in place of its text. */
export function button(text, className, action, label) {
  const node = element('button', className, text)
  node.type = 'button'
  if (label) node.setAttribute('aria-label', label)
  node.addEventListener('click', action)
  return node
}

const SVG = 'http://www.w3.org/2000/svg'

/** A line icon drawn from `paths` (path data on a 24-unit grid), hidden from assistive technology. */
export function icon(paths) {
  const svg = document.createElementNS(SVG, 'svg')
  svg.setAttribute('class', 'icon')
  svg.setAttribute('viewBox', '0 0 24 24')
  svg.setAttribute('aria-hidden', 'true')
  for (const d of paths) {
    const path = document.createElementNS(SVG, 'path')
    path.setAttribute('d', d)
    svg.append(path)
  }
  return svg
}

/**
 * A button drawn as an icon: `tip` shows beside it on hover and keyboard
 * focus, `label` names it, and `className` adds to how it looks.
 */
export function iconButton(paths, tip, action, label, className) {
  const node = button('', className ? `icon-button ${className}` : 'icon-button', action, label)
  node.dataset.tip = tip
  node.append(icon(paths))
  return node
}

/**
 * Draws `nodes` into `parent`, leaving whatever comes out the same where it
 * is: replacing it would take the keyboard from it, and lose a click whose
 * press came before the redraw and its release after. A changed element is
 * replaced whole, unless its own markup is the same and it holds only
 * elements: then it is drawn the same way inside. A changed button is
 * always replaced whole, its handler with it; a kept one acts on what its
 * own markup shows, or looks up the rest when it is clicked. If the element
 * that had the keyboard went all the same, the one drawn in its place takes
 * it back.
 */
export function redraw(parent, nodes) {
  const focused = parent.contains(document.activeElement) ? document.activeElement : null
  const key = focused === null ? null : focusKey(focused, parent)
  patch(parent, nodes)
  if (focused === null || focused.isConnected) return
  const again = [...parent.querySelectorAll('button, summary')].find(
    (node) => focusKey(node, parent) === key,
  )
  again?.focus({ preventScroll: true })
}

function patch(parent, nodes) {
  for (const [at, node] of nodes.entries()) {
    let shown = parent.children[at]
    // Keyed elements (`data-key`) that came are put in before the next one that stays, and ones
    // that went are taken out: those that stay are never taken out and put back, so a part the
    // page keeps in one of them (`data-keep`) keeps the keyboard while rows come and go around it.
    while (keyed(shown, node) && shown.dataset.key !== node.dataset.key) {
      if (nodes.slice(at + 1).some((later) => later.dataset?.key === shown.dataset.key)) break
      shown.remove()
      shown = parent.children[at]
    }
    if (keyed(shown, node) && shown.dataset.key !== node.dataset.key) {
      parent.insertBefore(node, shown)
      continue
    }
    if (shown === undefined) parent.append(node)
    else if (shown.isEqualNode(node) || kept(shown, node)) continue
    else if (opens(shown, node)) patch(shown, [...node.children])
    else shown.replaceWith(node)
  }
  while (parent.children.length > nodes.length) parent.lastElementChild.remove()
}

/** Whether both elements say what they are by a key of their own. */
const keyed = (shown, node) => shown?.dataset?.key !== undefined && node.dataset?.key !== undefined

/**
 * A place the page fills itself (`data-keep`), such as the member form a dialog moves into a
 * slot's row: drawn again with the same mark, it is left as it is, whatever it holds by then.
 */
const kept = (shown, node) =>
  shown.dataset?.keep !== undefined && shown.dataset.keep === node.dataset?.keep

const opens = (shown, node) =>
  shown.tagName === node.tagName &&
  shown.tagName !== 'BUTTON' &&
  shown.attributes.length === node.attributes.length &&
  [...shown.attributes].every(({ name, value }) => node.getAttribute(name) === value) &&
  [shown, node].every((element) => element.childNodes.length === element.children.length)

/**
 * What tells an element from the others across redraws: a card its task,
 * wherever it moved, a cell's stack its column, whatever its count, any
 * other control its label; and what it sits in.
 */
function focusKey(node, root) {
  const path = [
    node.dataset.task ?? node.dataset.stack ?? node.getAttribute('aria-label') ?? node.textContent,
  ]
  for (let at = node; at !== root; at = at.parentElement) {
    const { task, message, handle, role, project } = at.dataset
    path.push([at.tagName, task, message, handle, role, project].join(':'))
  }
  return path.join('/')
}
