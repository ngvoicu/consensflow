import { element } from '../dom.js'

/**
 * The diagrams of a project's specs (```` ```mermaid ```` fences), drawn by
 * Mermaid, which the page carries (`vendor/mermaid.js`, bundled as xterm is)
 * and loads only when a diagram is first shown: nothing is fetched, so they
 * draw offline. Mermaid draws at its strict level, in the page's own colours,
 * read from its variables as each diagram is drawn; a diagram cannot say other
 * colours, fonts or CSS of its own, and no click of one runs anything (the
 * page binds none).
 *
 * Mermaid lays a drawing out in the page itself, where an element of a label
 * or a line of its CSS (`classDef`, `style`) would load at once, before anything
 * here sees it. What keeps that from reaching anywhere is the page's own policy
 * (`app.security.csp` in `tauri.conf.json`): the page loads images, styles and
 * fonts from itself alone, and connects only to Tauri, however a diagram names
 * an address (a quoted key, an entity, a CSS escape). What is here is a second
 * lock behind it: each label cleaned of what loads or styles (`LABELS`,
 * Mermaid's own sanitizer given more to take out), a node's image given no
 * address, and what is drawn checked before it joins the page: no script, no
 * handler, no element that loads anything, no address but a place in the
 * drawing itself. One that cannot be drawn shows its source, and Mermaid's
 * words for why.
 */

/**
 * What a diagram may not set of Mermaid's settings: its own and Mermaid's. Of
 * the labels' two, Mermaid 11.17 already drops what a diagram says of its
 * sanitizer; they are kept here for a Mermaid that would not.
 */
const SECURE = [
  'secure',
  'securityLevel',
  'startOnLoad',
  'maxTextSize',
  'maxEdges',
  'suppressErrorRendering',
  'dompurifyConfig',
  'htmlLabels',
  'theme',
  'themeVariables',
  'themeCSS',
  'fontFamily',
  'altFontFamily',
  'darkMode',
]
/** What Mermaid's sanitizer takes out of a label, besides scripts: what loads, styles or takes input. */
const LABELS = {
  FORBID_TAGS: [
    'img',
    'image',
    'picture',
    'source',
    'video',
    'audio',
    'track',
    'iframe',
    'frame',
    'object',
    'embed',
    'link',
    'style',
    'svg',
    'math',
    'form',
    'input',
    'button',
    'textarea',
    'select',
    'meta',
    'base',
  ],
  FORBID_ATTR: [
    'style',
    'src',
    'srcset',
    'href',
    'xlink:href',
    'action',
    'formaction',
    'background',
    'poster',
  ],
}
/** A node's image as a diagram names it (`A@{ img: "…" }`, its key quoted or not): its address. */
const NODE_IMAGE = /((?:\bimg|"img"|'img')\s*:\s*)("[^"]*"|'[^']*'|[^,}\s]*)/gi
/** What is taken out of a drawing, with all it holds: what runs, loads, or takes input. */
const DROPPED = new Set([
  'script',
  'iframe',
  'frame',
  'object',
  'embed',
  'img',
  'image',
  'video',
  'audio',
  'source',
  'track',
  'link',
  'meta',
  'base',
  'form',
  'input',
  'button',
  'textarea',
  'select',
  'animate',
  'set',
  'animatemotion',
  'animatetransform',
])
/** The attributes that name an address. */
const ADDRESSES = new Set(['href', 'xlink:href', 'src', 'srcset', 'action', 'formaction', 'poster'])

let loaded = null
/** Mermaid, imported once, when the first diagram is drawn. */
const mermaid = () => {
  loaded ??= import('../vendor/mermaid.js').then((module) => module.default)
  return loaded
}

/** Diagrams are drawn one at a time: Mermaid draws in the page, and two at once measure each other. */
let queue = Promise.resolve()
let drawn = 0
/**
 * A turn of the page's own between two diagrams: Mermaid draws one without letting the page
 * do anything else, and what waits meanwhile (a click, the viewer shut) goes first.
 */
const turn = () => new Promise((later) => setTimeout(later, 0))

/**
 * A figure that shows the diagram `code` once it is drawn, or why it could not be. `signal`
 * says when what it is drawn for is gone (the viewer shut, another file chosen): a diagram
 * still waiting its turn then is never drawn, and one being drawn is not shown.
 */
export function diagram(code, signal) {
  const figure = element('figure', 'spec-diagram')
  figure.dataset.state = 'drawing'
  figure.append(element('p', 'spec-diagram-note', 'Drawing the diagram…'))
  queue = queue.then(turn).then(() => (signal?.aborted ? undefined : draw(figure, code, signal)))
  return figure
}

/** A diagram past what a document draws: its source, and one line that says why. */
export function undrawn(code, most) {
  const figure = element('figure', 'spec-diagram')
  figure.dataset.state = 'undrawn'
  const source = element('pre', 'spec-diagram-source')
  source.append(element('code', null, code))
  figure.append(
    element('p', 'spec-diagram-note', `Not drawn: a document draws its first ${most} diagrams.`),
    source,
  )
  return figure
}

async function draw(figure, code, signal) {
  drawn += 1
  const id = `spec-diagram-${drawn}`
  try {
    const drawer = await mermaid()
    if (signal?.aborted) return
    drawer.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      suppressErrorRendering: true,
      secure: SECURE,
      dompurifyConfig: LABELS,
      theme: 'base',
      themeVariables: colours(),
      fontFamily: variable('--ui'),
    })
    // A node's image keeps its place in the drawing, and loads nothing.
    const quieted = code.replace(/@\{[^}]*\}/g, (data) => data.replace(NODE_IMAGE, '$1""'))
    const { svg } = await drawer.render(id, quieted)
    if (signal?.aborted) return
    figure.replaceChildren(checked(svg))
    figure.dataset.state = 'drawn'
  } catch (cause) {
    // What Mermaid left in the page while it drew.
    document.getElementById(id)?.remove()
    document.getElementById(`d${id}`)?.remove()
    if (signal?.aborted) return
    const words = cause instanceof Error ? cause.message : String(cause)
    const source = element('pre', 'spec-diagram-source')
    source.append(element('code', null, code))
    figure.replaceChildren(
      element('p', 'spec-diagram-error', `This diagram could not be drawn. ${words}`.trim()),
      source,
    )
    figure.dataset.state = 'failed'
  }
}

/** The value of one of the page's variables, as it is now. */
const variable = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim()

/** Whether a colour (`#rrggbb`) is dark: what Mermaid draws shadows and contrasts by. */
function dark(colour) {
  const hex = /^#?([0-9a-f]{6})$/i.exec(colour)?.[1]
  if (hex === undefined) return true
  const [r, g, b] = [0, 2, 4].map((at) => Number.parseInt(hex.slice(at, at + 2), 16) / 255)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.5
}

/** Mermaid's colours, the page's: its surfaces, its lines and its words. */
function colours() {
  const [ink, deep, panel, raised, line, foam, muted, seafoam, dim, amber] = [
    '--ink',
    '--deep',
    '--panel',
    '--panel-raised',
    '--line',
    '--foam',
    '--muted',
    '--seafoam',
    '--seafoam-dim',
    '--amber',
  ].map(variable)
  return {
    darkMode: dark(ink),
    background: deep,
    fontFamily: variable('--ui'),
    fontSize: '13px',
    primaryColor: raised,
    primaryTextColor: foam,
    primaryBorderColor: seafoam,
    secondaryColor: panel,
    secondaryTextColor: foam,
    tertiaryColor: deep,
    tertiaryTextColor: foam,
    lineColor: muted,
    textColor: foam,
    titleColor: foam,
    mainBkg: raised,
    nodeBorder: seafoam,
    nodeTextColor: foam,
    clusterBkg: panel,
    clusterBorder: line,
    edgeLabelBackground: panel,
    noteBkgColor: panel,
    noteTextColor: foam,
    noteBorderColor: amber,
    actorBkg: raised,
    actorBorder: dim,
    actorTextColor: foam,
    actorLineColor: muted,
    signalColor: foam,
    signalTextColor: foam,
    labelBoxBkgColor: panel,
    labelBoxBorderColor: line,
    labelTextColor: foam,
    loopTextColor: muted,
    activationBkgColor: panel,
    activationBorderColor: seafoam,
    attributeBackgroundColorOdd: panel,
    attributeBackgroundColorEven: deep,
  }
}

/**
 * CSS that loads nothing: no import, no font of its own, no address but a place
 * in the drawing. Its escapes are read first (`u\72l(` is `url(`).
 */
const quiet = (css) =>
  css
    .replace(/\\([0-9a-f]{1,6})\s?|\\(.)/gis, (_, hex, character) => {
      if (hex === undefined) return character
      const point = Number.parseInt(hex, 16)
      return point > 0 && point <= 0x10ffff ? String.fromCodePoint(point) : ''
    })
    .replace(/@(import|font-face)\b[^;{]*(\{[^}]*\})?;?/gi, '')
    .replace(/\b(?:-webkit-)?image-set\s*\([^)]*\)/gi, 'none')
    .replace(/\burl\s*\(\s*(?!['"]?#)[^)]*\)/gi, 'none')

/**
 * The drawing (`svg`, as Mermaid writes one) as an element of the page, once
 * nothing in it may run or load: what Mermaid's strict level already keeps out,
 * checked again here, for the page has no policy of its own.
 */
export function checked(svg) {
  const parsed = new DOMParser().parseFromString(svg, 'text/html')
  const drawing = parsed.querySelector('svg')
  if (drawing === null) throw new Error('Mermaid drew nothing.')
  for (const node of [drawing, ...drawing.querySelectorAll('*')]) {
    const tag = node.localName.toLowerCase()
    if (DROPPED.has(tag)) {
      node.remove()
      continue
    }
    if (tag === 'a') {
      node.replaceWith(...node.childNodes)
      continue
    }
    if (tag === 'style') node.textContent = quiet(node.textContent)
    for (const { name, value } of [...node.attributes]) {
      const said = name.toLowerCase()
      if (said.startsWith('on')) node.removeAttribute(name)
      else if (ADDRESSES.has(said) && !value.trim().startsWith('#')) node.removeAttribute(name)
      else if (said === 'style') node.setAttribute(name, quiet(value))
    }
  }
  return document.importNode(drawing, true)
}
