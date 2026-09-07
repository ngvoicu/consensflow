import { Menus } from './menus.js'
import { runSelftest } from './selftest.js'
import { orderedPanes, paneLabel, renderSidebar, sessionName } from './sidebar.js'
import { EmulatorRegistry, paneKey } from './term.js'
import { fits, focusOrder, gridTemplate } from './vendor/layout.js'
import { effectivePolicy } from './vendor/policy.js'

const MIN_PANE = { width: 260, height: 180 }
const GEOMETRY_KEY = 'consensflow.window.geometry.v1'
const READINESS_BLOCKERS = new Set(['draft open', 'lead busy', 'unbound'])

const app = document.querySelector('#app')
const stage = document.querySelector('#pane-stage')
const parking = document.querySelector('#pane-parking')
const tree = document.querySelector('#session-tree')
const workspace = document.querySelector('[data-testid="workspace"]')
const rosterPanel = document.querySelector('[data-testid="roster-panel"]')
const rosterFrame = document.querySelector('[data-testid="roster-frame"]')
const rosterToggle = document.querySelector('#roster-toggle')
const rosterClose = document.querySelector('#roster-close')
const deliveryInfo = document.querySelector('#delivery-info')
const sidebarToggle = document.querySelector('#sidebar-toggle')
const currentSession = document.querySelector('#current-session')
const currentDirectory = document.querySelector('#current-directory')
const tabPolicy = document.querySelector('#tab-policy')
const heldSend = document.querySelector('#held-send')
const newPane = document.querySelector('#new-pane')
const newConversation = document.querySelector('#new-conversation')
const focusNav = document.querySelector('#focus-nav')
const focusCount = document.querySelector('#focus-count')
const previousPane = document.querySelector('#previous-pane')
const nextPane = document.querySelector('#next-pane')
const status = document.querySelector('#status')

const tauri = window.__TAURI__ ?? {}
const invoke = tauri.core?.invoke
const Channel = tauri.core?.Channel
const listen = tauri.event?.listen

let viewState = emptyState()
let selection = { tabId: null, type: 'session', paneId: null }
const focusByTab = new Map()
let resizeFrame = null
let refreshInFlight = null
let refreshPending = false
let outputChannel = null
let unlistenStateChanged = null
const cards = new Map()
const provisionalKeys = new Set()
const outputChains = new Map()
const retiredKeys = new Set()
const retiringKeys = new Set()
const inputSequences = new Map()
// Set only by the packaged smoke's driver (`selftest.js`), which Rust only
// lets the page load when the app was started in self-test mode.
let ackObserver = null
let outputObserver = null

function emptyState() {
  return { available: false, tabs: [], agents: [], deliveries: [], held: [], roster: null }
}

function array(value) {
  return Array.isArray(value) ? value : []
}

function record(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function normalizeState(raw) {
  const outer = record(raw) ? raw : {}
  const inner = record(outer.state) ? outer.state : outer
  return {
    available: outer.available !== false && inner.available !== false && outer.ok !== false,
    tabs: array(inner.tabs ?? inner.sessions ?? outer.tabs ?? outer.sessions),
    agents: array(inner.agents ?? outer.agents),
    deliveries: array(inner.deliveries ?? outer.deliveries),
    held: array(inner.held ?? inner.heldDeliveries ?? outer.held ?? outer.heldDeliveries),
    roster: inner.roster ?? outer.roster ?? null,
  }
}

function report(message, tone = 'info') {
  if (typeof message !== 'string' || message.trim().length === 0) {
    status.hidden = true
    return
  }
  status.textContent = message
  status.dataset.tone = tone
  status.hidden = false
}

function resultFailure(result) {
  if (!record(result) || result.ok !== false) return null
  if (result.error === 'unknown-op' || result.error === 'not-available-yet') {
    return `${result.operation ?? 'This operation'} is not available yet`
  }
  return result.error ?? 'The operation failed'
}

async function rawInvoke(command, args = {}) {
  if (typeof invoke !== 'function') {
    return { ok: false, error: 'not-available-yet', operation: command }
  }
  return await invoke(command, args)
}

async function run(command, args = {}, { refresh: shouldRefresh = true } = {}) {
  let result
  try {
    result = await rawInvoke(command, args)
  } catch (cause) {
    report(cause instanceof Error ? cause.message : String(cause), 'error')
    return null
  }
  const failure = resultFailure(result)
  if (failure !== null) {
    report(failure, 'error')
    return result
  }
  if (shouldRefresh) await refresh()
  return result
}

function nextInputSequence(pane) {
  const key = paneKey(pane)
  const sequence = (inputSequences.get(key) ?? 0) + 1
  inputSequences.set(key, sequence)
  return sequence
}

async function enqueueTerminalInput(command, pane, data) {
  const sequence = nextInputSequence(pane)
  const bytes = [...new TextEncoder().encode(data)]
  const admitted = await run(
    command,
    { id: pane.id, generation: pane.generation, sequence, bytes },
    { refresh: false },
  )
  if (admitted?.ok !== true) return
  if (typeof admitted.ticket !== 'string' || admitted.ticket.length === 0) {
    report('pane-input-ticket-missing', 'error')
    return
  }
  await run('pane_input_wait', { ticket: admitted.ticket }, { refresh: false })
}

const menus = new Menus({ invoke: rawInvoke, run, report })

const registry = new EmulatorRegistry({
  createEmulator: tauri.test?.createEmulator,
  onData: (pane, data) => {
    void enqueueTerminalInput('pane_input_enqueue', pane, data)
  },
  onReply: (pane, data) => {
    void enqueueTerminalInput('pane_reply_enqueue', pane, data)
  },
  onResize: (pane, cols, rows) => {
    void run(
      'pane_resize',
      { id: pane.id, generation: pane.generation, cols, rows },
      { refresh: false },
    )
  },
})

if (record(tauri.test)) {
  tauri.test.inspectPane = (key) => ({
    emulator: registry.emulators.has(key),
    outputChain: outputChains.has(key),
    provisional: provisionalKeys.has(key),
    retired: retiredKeys.has(key),
    retiring: retiringKeys.has(key),
  })
}

function policySetting(value) {
  return value === 'auto' || value === 'manual' ? value : undefined
}

function fallbackPolicyInputs(pane) {
  const candidate = pane.effectivePolicy ?? pane.policyEffective ?? pane.policy
  if (!record(candidate)) return {}
  const mode = policySetting(candidate.mode ?? candidate.value)
  if (mode === undefined) return {}
  switch (candidate.source) {
    case 'tab':
    case 'tab-human':
      return { tab: mode }
    case 'pane':
    case 'pane-human':
      return { pane: mode }
    case 'lead':
      return { lead: mode }
    default:
      return {}
  }
}

function policy(pane, tab) {
  const fallback = fallbackPolicyInputs(pane)
  const row = record(pane.row)
    ? pane.row
    : record(pane.conversationRecord)
      ? pane.conversationRecord
      : {}
  const notifyPreference =
    policySetting(pane.notifyPreference) ?? policySetting(row.notifyPreference) ?? fallback.lead
  return effectivePolicy(
    { policy: policySetting(record(tab.policy) ? tab.policy.mode : tab.policy) ?? fallback.tab },
    { policy: typeof pane.policy === 'string' ? pane.policy : fallback.pane },
    { ...row, notifyPreference },
  )
}

function paneTitle(tab, pane) {
  if (pane.kind === 'shell') return 'shell'
  const name = paneLabel(tab, pane)
  const agent = pane.agent ?? pane.harness ?? tab.lead?.harness ?? 'agent'
  const effective = policy(pane, tab)
  return `${name} · @${agent} · Replies: ${effective.mode === 'manual' ? 'Manual' : 'Automatic'}`
}

function isLive(tab, pane) {
  return tab.closed !== true && pane.alive !== false
}

function deliveriesFor(tab, pane) {
  return viewState.deliveries.filter((delivery) => {
    const sameTab = delivery.tab === undefined || delivery.tab === tab.id
    const target = delivery.pane ?? delivery.targetPane ?? delivery.target?.pane
    return (
      sameTab && target === pane.id && ['pending', 'waiting'].includes(delivery.state ?? 'pending')
    )
  })
}

function addDeliveryBadges(titlebar, tab, pane) {
  for (const previous of titlebar.querySelectorAll('.delivery-badge')) previous.remove()
  for (const delivery of deliveriesFor(tab, pane)) {
    const badge = document.createElement('span')
    badge.className = 'delivery-badge'
    badge.dataset.testid = `delivery-${delivery.id}`
    const reason = document.createElement('span')
    reason.textContent = delivery.reason ?? 'waiting'
    const deliver = document.createElement('button')
    deliver.type = 'button'
    deliver.className = 'quiet-button'
    deliver.textContent = 'Deliver now'
    const blocker = READINESS_BLOCKERS.has(delivery.reason)
    deliver.disabled = blocker
    if (blocker) {
      deliver.title = `${delivery.reason} — readiness cannot be bypassed`
    } else {
      deliver.addEventListener('click', () =>
        run('deliver_now', { delivery: delivery.id, tab: tab.id }),
      )
    }
    badge.append(reason, deliver)
    titlebar.append(badge)
  }
}

function createCard(tab, pane) {
  const card = document.createElement('article')
  card.className = 'pane-card'
  card.dataset.testid = `pane-${pane.id}`
  card.dataset.paneId = pane.id
  card.dataset.generation = String(pane.generation)
  card.dataset.kind = pane.kind
  card.dataset.parked = 'true'

  const titlebar = document.createElement('header')
  titlebar.className = 'pane-titlebar'
  const kind = document.createElement('span')
  kind.className = 'pane-kind'
  kind.setAttribute('aria-hidden', 'true')
  const title = document.createElement('span')
  title.className = 'pane-title'
  titlebar.append(kind, title)
  const terminal = document.createElement('div')
  terminal.className = 'terminal-host'
  terminal.setAttribute('aria-label', `${paneLabel(tab, pane)} terminal`)
  card.append(titlebar, terminal)
  parking.append(card)

  card._context = { tab, pane }
  titlebar.addEventListener('contextmenu', (event) => {
    if (card._context.pane.kind === 'worker') {
      void menus.worker(event, card._context.tab, card._context.pane)
    }
  })
  card.addEventListener('pointerdown', () => {
    const context = card._context
    const panes = orderedPanes(context.tab)
    const at = panes.findIndex((candidate) => candidate.id === context.pane.id)
    if (at >= 0) focusByTab.set(context.tab.id, at)
  })

  registry.ensure(pane, terminal)
  return card
}

function updateCard(card, tab, pane) {
  card._context = { tab, pane }
  card.dataset.kind = pane.kind
  card.dataset.generation = String(pane.generation)
  const title = card.querySelector('.pane-title')
  title.textContent = paneTitle(tab, pane)
  if (pane.kind === 'shell') {
    title.removeAttribute('title')
  } else {
    const effective = policy(pane, tab)
    const source = {
      'tab-human': 'Set for this session.',
      'pane-human': 'Set for this worker.',
      lead: 'Requested by the lead.',
      default: 'Using the default setting.',
    }[effective.source]
    title.title = `Reply delivery: ${effective.mode === 'manual' ? 'Manual' : 'Automatic'}. ${source}`
  }
  addDeliveryBadges(card.querySelector('.pane-titlebar'), tab, pane)
}

async function drainAndRetirePane(key) {
  while (retiredKeys.has(key)) {
    const pending = outputChains.get(key)
    if (pending !== undefined) await pending
    if (!retiredKeys.has(key)) break
    if (outputChains.get(key) !== pending) continue

    outputChains.delete(key)
    registry.retire(key)
    cards.get(key)?.remove()
    cards.delete(key)
    break
  }
  retiringKeys.delete(key)
}

function retirePane(key) {
  retiredKeys.add(key)
  provisionalKeys.delete(key)
  if (retiringKeys.has(key)) return
  retiringKeys.add(key)
  void drainAndRetirePane(key)
}

function reconcileCards() {
  const liveKeys = new Set()
  const authoritativeKeys = new Set()
  const authoritativePaneIds = new Set()
  for (const tab of viewState.tabs) {
    for (const pane of orderedPanes(tab)) {
      const key = paneKey(pane)
      authoritativeKeys.add(key)
      authoritativePaneIds.add(pane.id)
      if (!isLive(tab, pane)) {
        retirePane(key)
        continue
      }
      liveKeys.add(key)
      retiredKeys.delete(key)
      let card = cards.get(key)
      if (card === undefined) {
        card = createCard(tab, pane)
        cards.set(key, card)
      }
      provisionalKeys.delete(key)
      card.dataset.provisional = 'false'
      updateCard(card, tab, pane)
    }
  }

  for (const [key, card] of cards) {
    const replacedGeneration =
      authoritativePaneIds.has(card.dataset.paneId) && !authoritativeKeys.has(key)
    if (replacedGeneration) retirePane(key)
  }

  for (const [key, card] of cards) {
    if (liveKeys.has(key) || provisionalKeys.has(key) || retiringKeys.has(key)) continue
    card.remove()
    cards.delete(key)
  }
  registry.reconcile(new Set([...liveKeys, ...provisionalKeys, ...retiringKeys]))
}

function activeTab() {
  return viewState.tabs.find((tab) => tab.id === selection.tabId) ?? null
}

function ensureSelection() {
  if (viewState.tabs.length === 0) {
    selection = { tabId: null, type: 'session', paneId: null }
    return
  }
  let tab = activeTab()
  if (tab === null) {
    tab = viewState.tabs.find((candidate) => candidate.closed !== true) ?? viewState.tabs[0]
    selection = { tabId: tab.id, type: 'session', paneId: null }
  }
  if (selection.type === 'pane') {
    const exists = orderedPanes(tab).some((pane) => pane.id === selection.paneId)
    if (!exists) selection = { tabId: tab.id, type: 'session', paneId: null }
  }
}

function parkCard(card) {
  if (card.parentElement !== parking) {
    parking.append(card)
  }
  card.dataset.parked = 'true'
  card.dataset.selected = 'false'
  card.style.removeProperty('grid-area')
  card.style.removeProperty('height')
  card.style.removeProperty('width')
}

function parkEveryCard() {
  for (const card of cards.values()) parkCard(card)
}

function focusedIndex(tab, panes) {
  if (selection.type === 'pane') {
    const selected = panes.findIndex((pane) => pane.id === selection.paneId)
    if (selected >= 0) return selected
  }
  const remembered = focusByTab.get(tab.id) ?? 0
  return Math.max(0, Math.min(remembered, panes.length - 1))
}

function cssPixels(value) {
  const parsed = Number.parseFloat(value)
  return Number.isFinite(parsed) ? parsed : 0
}

function gridContentArea(layout) {
  const styles = getComputedStyle(stage)
  const padding = cssPixels(styles.getPropertyValue('--pane-grid-padding'))
  const gap = cssPixels(styles.getPropertyValue('--pane-grid-gap'))
  return {
    width: Math.max(0, stage.clientWidth - 2 * padding - (layout.cols - 1) * gap),
    height: Math.max(0, stage.clientHeight - 2 * padding - (layout.rows - 1) * gap),
  }
}

function showCards(tab, panes, indices, mode, layout) {
  const desired = indices.map((index) => cards.get(paneKey(panes[index]))).filter(Boolean)
  const desiredSet = new Set(desired)
  for (const child of [...stage.children]) {
    if (child.classList.contains('pane-card') && !desiredSet.has(child)) parkCard(child)
    else if (!child.classList.contains('pane-card')) child.remove()
  }
  let cursor = stage.firstElementChild
  for (const card of desired) {
    if (card === cursor) {
      cursor = cursor.nextElementSibling
      continue
    }
    stage.insertBefore(card, cursor)
  }
  stage.dataset.empty = 'false'
  stage.dataset.mode = mode
  stage.dataset.layout = layout.areas
  stage.style.gridTemplateAreas = layout.areas
  stage.style.gridTemplateRows = `repeat(${layout.rows}, minmax(0, 1fr))`
  stage.style.gridTemplateColumns = `repeat(${layout.cols}, minmax(0, 1fr))`

  const cells = focusOrder(panes.length)
  for (const index of indices) {
    const pane = panes[index]
    const card = cards.get(paneKey(pane))
    if (card === undefined) continue
    card.dataset.parked = 'false'
    card.dataset.selected =
      mode === 'focused' ? 'true' : index === focusedIndex(tab, panes) ? 'true' : 'false'
    if (mode === 'grid') {
      card.style.removeProperty('width')
      card.style.removeProperty('height')
      card.style.gridArea = cells[index]
    } else {
      card.style.removeProperty('grid-area')
      card.style.width = '100%'
      card.style.height = '100%'
    }
    registry.fit(pane.id, pane.generation)
  }
}

function renderPaneView() {
  const tab = activeTab()
  const panes = tab === null ? [] : orderedPanes(tab).filter((pane) => isLive(tab, pane))
  if (tab === null || panes.length === 0) {
    parkEveryCard()
    stage.replaceChildren()
    stage.dataset.empty = 'true'
    stage.dataset.mode = 'grid'
    stage.dataset.layout = ''
    stage.removeAttribute('style')
    const empty = document.createElement('p')
    empty.className = 'empty-state'
    empty.textContent =
      tab?.closed === true
        ? 'Resume this session to reopen its lead.'
        : 'Open a conversation to start a lead pane.'
    stage.append(empty)
    focusNav.hidden = true
    return
  }

  const layout = gridTemplate(panes.length)
  const available = gridContentArea(layout)
  const mustFocus = selection.type === 'pane' || !fits(panes.length, available, MIN_PANE)
  if (!mustFocus) {
    showCards(
      tab,
      panes,
      panes.map((_, index) => index),
      'grid',
      layout,
    )
    focusNav.hidden = true
    return
  }

  const index = focusedIndex(tab, panes)
  focusByTab.set(tab.id, index)
  showCards(tab, panes, [index], 'focused', layout)
  focusCount.textContent = `${index + 1} / ${panes.length}`
  focusNav.hidden = panes.length < 2
  cards.get(paneKey(panes[index]))?.querySelector('.pane-titlebar')?.append(focusNav)
}

function renderHeader() {
  const tab = activeTab()
  currentSession.textContent = tab === null ? 'No session' : sessionName(tab)
  currentDirectory.textContent = tab?.directory ?? tab?.dir ?? ''
  tabPolicy.hidden = tab === null || tab.closed === true
  deliveryInfo.hidden = tabPolicy.hidden
  newPane.hidden = tab === null || tab.closed === true
  if (tab !== null) {
    const mode = record(tab.policy) ? tab.policy.mode : tab.policy
    tabPolicy.textContent = `Reply delivery: ${mode === 'manual' ? 'Manual' : 'Automatic'}`
  }
  const held = tab === null ? [] : viewState.held.filter((entry) => entry.tab === tab.id)
  heldSend.hidden = tab === null || held.length === 0 || tab.closed === true
}

function render() {
  ensureSelection()
  reconcileCards()
  renderSidebar(tree, viewState.tabs, {
    selection,
    onSelectSession: (tab) => {
      selection = { tabId: tab.id, type: 'session', paneId: null }
      render()
    },
    onSelectPane: (tab, pane) => {
      selection = { tabId: tab.id, type: 'pane', paneId: pane.id }
      const panes = orderedPanes(tab)
      focusByTab.set(
        tab.id,
        Math.max(
          0,
          panes.findIndex((candidate) => candidate.id === pane.id),
        ),
      )
      render()
    },
    onResume: (tab) => void run('tab_resume', { tab: tab.id }),
    onAttach: (tab, pane) =>
      void run('open_consult', {
        tab: tab.id,
        agent: pane.agent,
        conversation: pane.conversation ?? pane.name,
        task: null,
      }),
  })
  renderHeader()
  renderPaneView()
}

function applyRoster(roster) {
  if (!record(roster)) return
  const rawUrl = roster.url ?? roster.src
  if (typeof rawUrl !== 'string') return
  try {
    const url = new URL(rawUrl)
    if (url.protocol !== 'http:' || !['localhost', '127.0.0.1'].includes(url.hostname)) {
      throw new Error('the roster address is not loopback HTTP')
    }
    url.hostname = 'localhost'
    if (typeof roster.token === 'string' && roster.token.length > 0) {
      url.searchParams.set('token', roster.token)
    }
    rosterFrame.src = url.href
  } catch (cause) {
    report(cause instanceof Error ? cause.message : String(cause), 'error')
  }
}

async function refresh() {
  if (refreshInFlight !== null) {
    refreshPending = true
    return await refreshInFlight
  }
  refreshInFlight = (async () => {
    do {
      refreshPending = false
      let raw
      try {
        raw = await rawInvoke('list_state', {})
      } catch (cause) {
        raw = { ok: false, error: cause instanceof Error ? cause.message : String(cause) }
      }
      viewState = normalizeState(raw)
      applyRoster(viewState.roster)
      render()
      const failure = resultFailure(raw)
      if (failure !== null) report(failure, 'error')
      else if (!viewState.available) report('Session controls are not available yet', 'error')
    } while (refreshPending)
  })()
  try {
    await refreshInFlight
  } finally {
    refreshInFlight = null
    if (refreshPending) void refresh()
  }
}

function queueOutput(message) {
  outputObserver?.(message)
  if (!record(message)) return
  const key = `${message.id}:${message.generation}`
  if (
    retiredKeys.has(key) &&
    registry.get(message.id, message.generation) === null &&
    !outputChains.has(key)
  ) {
    return
  }
  const previous = outputChains.get(key) ?? Promise.resolve()
  const next = previous
    .then(async () => {
      const emulator = ensureOutputConsumer(message)
      await emulator.write(new Uint8Array(array(message.bytes)))
      await run(
        'pane_ack',
        { id: message.id, generation: message.generation, seq: message.seq },
        { refresh: false },
      )
      ackObserver?.(message)
    })
    .catch((cause) => report(cause instanceof Error ? cause.message : String(cause), 'error'))
  outputChains.set(key, next)
}

function ensureOutputConsumer(message) {
  const existing = registry.get(message.id, message.generation)
  if (existing !== null) return existing
  const pane = {
    id: message.id,
    generation: message.generation,
    kind: 'worker',
    order: Number.MAX_SAFE_INTEGER,
    conversation: message.id,
    agent: 'pending',
    alive: true,
  }
  const tab = { id: 'runtime', name: 'runtime', panes: [pane] }
  const key = paneKey(pane)
  const card = createCard(tab, pane)
  card.dataset.provisional = 'true'
  cards.set(key, card)
  provisionalKeys.add(key)
  return registry.get(message.id, message.generation)
}

function moveFocus(delta) {
  const tab = activeTab()
  if (tab === null) return
  const panes = orderedPanes(tab).filter((pane) => isLive(tab, pane))
  if (panes.length === 0) return
  const next = (focusedIndex(tab, panes) + delta + panes.length) % panes.length
  focusByTab.set(tab.id, next)
  selection = { tabId: tab.id, type: 'pane', paneId: panes[next].id }
  render()
}

function validGeometry(value) {
  return (
    record(value) &&
    [value.x, value.y, value.width, value.height].every(Number.isFinite) &&
    value.width >= 560 &&
    value.height >= 480
  )
}

async function installGeometryPersistence() {
  const api = tauri.window?.getCurrentWindow?.()
  if (api === undefined) return
  let saved = null
  try {
    saved = JSON.parse(localStorage.getItem(GEOMETRY_KEY))
  } catch {
    saved = null
  }

  if (validGeometry(saved)) {
    if (saved.maximized === true) {
      await api.maximize()
    } else {
      await api.unmaximize?.()
      const Size = tauri.dpi?.PhysicalSize
      const Position = tauri.dpi?.PhysicalPosition
      await api.setSize(
        Size === undefined ? { ...saved, type: 'Physical' } : new Size(saved.width, saved.height),
      )
      await api.setPosition(
        Position === undefined ? { ...saved, type: 'Physical' } : new Position(saved.x, saved.y),
      )
    }
  } else {
    await api.maximize()
  }

  let timer = null
  const save = () => {
    if (timer !== null) clearTimeout(timer)
    timer = setTimeout(async () => {
      try {
        const [position, size, maximized] = await Promise.all([
          api.outerPosition(),
          api.innerSize(),
          api.isMaximized(),
        ])
        localStorage.setItem(
          GEOMETRY_KEY,
          JSON.stringify({
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized,
          }),
        )
      } catch {
        // Geometry persistence is best-effort; pane ownership must stay live.
      }
    }, 30)
  }
  await api.onMoved(save)
  await api.onResized(save)
}

rosterToggle.addEventListener('click', () => rosterPanel.showModal())
rosterClose.addEventListener('click', () => rosterPanel.close())
rosterPanel.addEventListener('close', () => rosterToggle.focus())

sidebarToggle.addEventListener('click', () => {
  const collapsed = workspace.dataset.sidebarCollapsed !== 'true'
  workspace.dataset.sidebarCollapsed = String(collapsed)
  sidebarToggle.textContent = collapsed ? 'Expand sessions' : 'Collapse sessions'
  sidebarToggle.setAttribute('aria-expanded', String(!collapsed))
  scheduleLayout()
})

tabPolicy.addEventListener('click', () => {
  const tab = activeTab()
  if (tab !== null) menus.tabPolicy(tabPolicy, tab)
})

heldSend.addEventListener('click', () => {
  const tab = activeTab()
  if (tab !== null) void run('held_send', { tab: tab.id })
})

newPane.addEventListener('click', () => {
  const tab = activeTab()
  if (tab !== null) menus.newPane(newPane, tab, viewState.agents)
})

newConversation.addEventListener('click', () => void menus.newConversation(tauri.dialog))
previousPane.addEventListener('click', () => moveFocus(-1))
nextPane.addEventListener('click', () => moveFocus(1))

function scheduleLayout() {
  if (resizeFrame !== null) cancelAnimationFrame(resizeFrame)
  resizeFrame = requestAnimationFrame(() => {
    resizeFrame = null
    renderPaneView()
  })
}

new ResizeObserver(scheduleLayout).observe(stage)
window.addEventListener('beforeunload', () => {
  unlistenStateChanged?.()
  registry.dispose()
})

async function start() {
  try {
    await installGeometryPersistence()
  } catch (cause) {
    report(cause instanceof Error ? cause.message : String(cause), 'error')
  }
  if (typeof Channel === 'function') {
    outputChannel = new Channel()
    outputChannel.onmessage = queueOutput
    // Once, for the life of the page. Sending this with every refresh ended
    // the subscription instead of renewing it — see `subscribe_output`.
    await run('subscribe_output', { onOutput: outputChannel }, { refresh: false })
  }
  if (typeof listen === 'function') {
    // Rust emits `state-changed`: Tauri 2 refuses an event name with a dot in
    // it, and this rejection used to take the whole page down with it — the
    // pane area never appeared, because one optional listener failed. Live
    // refresh is a convenience; losing it must never cost the panes.
    try {
      unlistenStateChanged = await listen('state-changed', () => void refresh())
    } catch (cause) {
      report(cause instanceof Error ? cause.message : String(cause), 'error')
    }
  }
  await refresh()
  app.dataset.ready = 'true'
  const selftest = window.__CONSENSFLOW_SELFTEST__
  if (record(selftest)) {
    void runSelftest({
      config: selftest,
      invoke: rawInvoke,
      refresh,
      registry,
      sendInput: (pane, data) => enqueueTerminalInput('pane_input_enqueue', pane, data),
      onAck: (observer) => {
        ackObserver = observer
      },
      onOutput: (observer) => {
        outputObserver = observer
      },
    })
  }
}

void start()
