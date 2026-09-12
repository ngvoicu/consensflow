import { Menus } from './menus.js'
import { runSelftest } from './selftest.js'
import { orderedPanes, paneLabel, renderSidebar, sessionName } from './sidebar.js'
import { EmulatorRegistry, paneKey } from './term.js'
import { initializeUpdates } from './updates.js'
import { gridTemplate } from './vendor/layout.js'
import { effectivePolicy } from './vendor/policy.js'

const GEOMETRY_KEY = 'consensflow.window.geometry.v1'

const app = document.querySelector('#app')
const stage = document.querySelector('#pane-stage')
const parking = document.querySelector('#pane-parking')
const tree = document.querySelector('#session-tree')
const workspace = document.querySelector('[data-testid="workspace"]')
const rosterPanel = document.querySelector('[data-testid="roster-panel"]')
const rosterFrame = document.querySelector('[data-testid="roster-frame"]')
const rosterToggle = document.querySelector('#roster-toggle')
const rosterClose = document.querySelector('#roster-close')
const libraryPanel = document.querySelector('#library-dialog')
const libraryFrame = document.querySelector('[data-testid="library-frame"]')
const libraryToggle = document.querySelector('#library-toggle')
const libraryClose = document.querySelector('#library-close')
let libraryUrl = null
const harnessesPanel = document.querySelector('#harnesses-dialog')
const harnessesFrame = document.querySelector('[data-testid="harnesses-frame"]')
const harnessesToggle = document.querySelector('#harnesses-toggle')
const harnessesClose = document.querySelector('#harnesses-close')
let harnessesUrl = null
const deliveryInfo = document.querySelector('#delivery-info')
const sidebarToggle = document.querySelector('#sidebar-toggle')
const currentSession = document.querySelector('#current-session')
const currentDirectory = document.querySelector('#current-directory')
const tabPolicy = document.querySelector('#tab-policy')
const resultsToggle = document.querySelector('#results-toggle')
const resultsDialog = document.querySelector('#results-dialog')
const resultsList = document.querySelector('#results-list')
let resultsOwner = null
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
const selectionBySession = new Map()
const groupViews = document.querySelector('#group-views')
const pmView = document.querySelector('#view-pm')
const leadView = document.querySelector('#view-lead')
const focusByTab = new Map()
const scrollByTab = new Map()
const workerStage = document.createElement('div')
workerStage.className = 'worker-stage'
workerStage.addEventListener('scroll', () => {
  if (workerStage.dataset.tab) scrollByTab.set(workerStage.dataset.tab, workerStage.scrollLeft)
})
let resizeFrame = null
let refreshInFlight = null
let refreshPending = false
let outputChannel = null
let unlistenStateChanged = null
const cards = new Map()
const terminalSizes = new Map()
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
  return { available: false, tabs: [], agents: [], results: [], roster: null }
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
    results: array(inner.results ?? outer.results),
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
  if (command === 'tab_resume' && Array.isArray(result?.workers)) {
    const failed = result.workers.filter((worker) => worker.error)
    if (failed.length)
      report(failed.map((worker) => `${worker.conversation}: ${worker.error}`).join('; '), 'error')
  }
  if (
    ['open_lead', 'open_pm'].includes(command) &&
    result?.ok === true &&
    (result.outcome === 'opened' || command === 'open_pm') &&
    viewState.tabs.some((tab) => tab.id === result.tab && tab.closed !== true)
  ) {
    selection = {
      tabId: result.tab,
      type: 'session',
      paneId: null,
    }
    render()
    if (record(result.pane)) {
      cards.get(paneKey(result.pane))?.querySelector('.xterm-helper-textarea')?.focus()
    }
  }
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

const menus = new Menus({ invoke: rawInvoke, run, report, openResults: showResults })

async function syncTerminalSize(key) {
  // Fitting can precede native startup. Keep the size until the live pane
  // accepts it, even if xterm's geometry has not changed since its first fit.
  const update = terminalSizes.get(key)
  const context = cards.get(key)?._context
  if (
    !update ||
    !context ||
    update.running ||
    update.applied === update.desired ||
    provisionalKeys.has(key) ||
    !isLive(context.tab, context.pane) ||
    isFailed(context.pane)
  )
    return
  const { pane } = context
  const desired = update.desired
  update.running = true
  try {
    const result = await rawInvoke('pane_resize', {
      id: pane.id,
      generation: pane.generation,
      ...desired,
    })
    if (terminalSizes.get(key) !== update) return
    const failure = resultFailure(result)
    if (result?.ok === true) update.applied = desired
    else if (failure !== `pane ${pane.id} generation ${pane.generation} is not open`)
      report(failure, 'error')
  } catch (cause) {
    if (terminalSizes.get(key) === update)
      report(cause instanceof Error ? cause.message : String(cause), 'error')
  } finally {
    update.running = false
    if (terminalSizes.get(key) === update && update.desired !== desired) void syncTerminalSize(key)
  }
}

const registry = new EmulatorRegistry({
  createEmulator: tauri.test?.createEmulator,
  onData: (pane, data) => {
    if (isFailed(pane)) return
    void enqueueTerminalInput('pane_input_enqueue', pane, data)
  },
  onReply: (pane, data) => {
    if (isFailed(pane)) return
    void enqueueTerminalInput('pane_reply_enqueue', pane, data)
  },
  onResize: (pane, cols, rows) => {
    const key = paneKey(pane)
    if (isFailed(pane) || retiredKeys.has(key)) return
    const update = terminalSizes.get(key) ?? { running: false }
    update.desired = { cols, rows }
    terminalSizes.set(key, update)
    void syncTerminalSize(key)
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

function currentPane(pane) {
  for (const tab of viewState.tabs) {
    const match = array(tab.panes).find(
      (candidate) => candidate.id === pane.id && candidate.generation === pane.generation,
    )
    if (match !== undefined) return match
  }
  return pane
}

function paneFailure(pane) {
  const failure = currentPane(pane).failure
  return record(failure) ? failure : null
}

function paneStartupFailure(pane) {
  const progress = currentPane(pane).progress
  return record(progress) &&
    progress.state === 'failed' &&
    progress.pane === pane.id &&
    progress.generation === pane.generation
    ? progress
    : null
}

function isFailed(pane) {
  return paneFailure(pane) !== null
}

function isVisiblePane(tab, pane) {
  return tab.closed !== true && (isLive(tab, pane) || pane.starting === true || isFailed(pane))
}

function deliveriesFor(tab, pane) {
  if (!pane) return []
  return viewState.results.filter(
    (result) =>
      result.tab === tab.id &&
      (pane.kind === 'lead' || result.conversation === pane.conversation) &&
      ['waiting', 'collecting', 'uncertain'].includes(result.state),
  )
}

function showResults(tab, conversation = null) {
  menus.closeMenu()
  resultsOwner = { tab: tab.id, conversation }
  document.querySelector('#results-title').textContent =
    `${tab.role === 'pm' ? 'PM' : 'Lead'} results`
  resultsList.replaceChildren()
  if (!resultsDialog.open) resultsDialog.showModal()
  renderResults()
}

function renderResults() {
  if (!resultsDialog.open || !resultsOwner) return
  const tab = viewState.tabs.find((tab) => tab.id === resultsOwner.tab)
  if (!tab) {
    resultsDialog.close()
    return
  }
  const recipient = tab.role === 'pm' ? 'PM' : 'Lead'
  const results = viewState.results.filter(
    (r) =>
      r.tab === tab.id &&
      (!resultsOwner.conversation || r.conversation === resultsOwner.conversation),
  )
  for (const result of results) {
    let row = [...resultsList.children].find((row) => row.dataset.result === result.id)
    if (!row) {
      row = document.createElement('article')
      row.className = 'result-row'
      row.dataset.result = result.id
      const title = document.createElement('h3')
      title.textContent = `${result.conversation} · @${result.agent}`
      const meta = document.createElement('p')
      meta.className = 'result-meta'
      const preview = document.createElement('p')
      preview.textContent = result.preview ?? ''
      const body = document.createElement('pre')
      body.className = 'result-body'
      body.hidden = true
      const read = document.createElement('button')
      read.type = 'button'
      read.textContent = 'Read result'
      read.setAttribute('aria-label', `Read result ${result.id}`)
      read.addEventListener('click', async () => {
        if (body.dataset.loaded) {
          body.hidden = !body.hidden
          return
        }
        read.disabled = true
        try {
          let offset = 0,
            text = ''
          do {
            const chunk = await rawInvoke('result_body', { tab: tab.id, result: result.id, offset })
            if (chunk?.ok === false || typeof chunk?.text !== 'string')
              throw new Error(chunk?.error ?? 'Result unavailable')
            text += chunk.text
            if (chunk.next !== null && (!Number.isSafeInteger(chunk.next) || chunk.next <= offset))
              throw new Error('Invalid result page')
            offset = chunk.next
          } while (offset !== null)
          body.textContent = text
          body.dataset.loaded = 'true'
          body.hidden = false
        } catch (error) {
          report(error.message, 'error')
        } finally {
          read.disabled = false
        }
      })
      const collect = document.createElement('button')
      collect.type = 'button'
      collect.className = 'result-collect'
      collect.textContent = `Collect in ${recipient}`
      collect.addEventListener('click', () =>
        run('result_collect', { tab: tab.id, result: result.id }),
      )
      row.append(title, meta, preview, read, collect, body)
      resultsList.append(row)
    }
    row.dataset.state = result.state
    const label =
      {
        received: `Received by ${recipient}`,
        waiting: `Waiting for ${recipient}`,
        collecting: `Being collected by ${recipient}`,
        uncertain: 'Receipt unconfirmed',
        cancelled: 'Collection cancelled',
      }[result.state] ?? 'Receipt unconfirmed'
    const at = Number.isFinite(result.createdAt) ? new Date(result.createdAt).toLocaleString() : ''
    row.querySelector('.result-meta').textContent =
      `${label}${at ? ` · ${at}` : ''}${result.parts > 1 ? ` · ${result.receivedParts ?? 0}/${result.parts} parts confirmed` : ''}`
    row.querySelector('.result-collect').hidden = result.state !== 'waiting' || tab.closed === true
  }
  let empty = resultsList.querySelector('.results-empty')
  if (!results.length && !empty) {
    empty = document.createElement('p')
    empty.className = 'results-empty'
    empty.textContent = 'No completed results yet.'
    resultsList.append(empty)
  } else if (results.length) empty?.remove()
}

function addDeliveryBadges(titlebar, tab, pane) {
  for (const previous of titlebar.querySelectorAll('.result-summary')) previous.remove()
  const deliveries = deliveriesFor(tab, pane)
  if (deliveries.length === 0) return
  const summary = document.createElement('button')
  summary.type = 'button'
  summary.className = 'result-summary'
  summary.dataset.testid = `result-summary-${pane.id}`
  summary.textContent = `${deliveries.length} unconfirmed result${deliveries.length === 1 ? '' : 's'}`
  summary.setAttribute(
    'aria-label',
    `${deliveries.length} unconfirmed result${deliveries.length === 1 ? '' : 's'}`,
  )
  summary.addEventListener('click', () =>
    showResults(tab, pane.kind === 'worker' ? pane.conversation : null),
  )
  titlebar.append(summary)
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
  const close = document.createElement('button')
  close.type = 'button'
  close.className = 'pane-close-button'
  close.textContent =
    pane.kind === 'lead' ? `Suspend ${tab.role === 'pm' ? 'PM' : 'lead'}` : 'Close pane'
  close.setAttribute('aria-label', close.textContent)
  close.title =
    pane.kind === 'lead'
      ? 'Suspend this coordinator and keep its lead conversation for Resume'
      : 'Close this pane and keep its native conversation'
  close.addEventListener('click', (event) => {
    event.stopPropagation()
    void menus.closePane({
      id: card.dataset.paneId,
      generation: Number(card.dataset.generation),
    })
  })
  titlebar.append(kind, title, close)
  const terminal = document.createElement('div')
  terminal.className = 'terminal-host'
  terminal.setAttribute('aria-label', `${paneLabel(tab, pane)} terminal`)
  const failure = document.createElement('div')
  failure.className = 'pane-failure'
  failure.hidden = true
  const failureState = document.createElement('strong')
  failureState.className = 'pane-failure-state'
  const failureMessage = document.createElement('span')
  failureMessage.className = 'pane-failure-message'
  const failureExit = document.createElement('span')
  failureExit.className = 'pane-failure-exit'
  failure.append(failureState, failureMessage, failureExit)
  card.append(titlebar, failure, terminal)
  parking.append(card)

  card._context = { tab, pane }
  titlebar.addEventListener('contextmenu', (event) => {
    if (card._context.pane.kind === 'worker' && !isFailed(card._context.pane)) {
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
  const failure = paneFailure(pane)
  const startupFailure = paneStartupFailure(pane)
  const problem = failure ?? startupFailure
  card.dataset.state = problem === null ? 'live' : 'failed'
  card.dataset.failed = problem === null ? 'false' : 'true'
  const failureView = card.querySelector('.pane-failure')
  if (failureView !== null) {
    failureView.hidden = problem === null
    if (problem !== null) {
      failureView.querySelector('.pane-failure-state').textContent =
        failure === null ? 'Startup failed' : 'Ended'
      failureView.querySelector('.pane-failure-message').textContent =
        typeof problem.message === 'string' && problem.message.length > 0
          ? problem.message
          : 'Worker harness failed'
      failureView.querySelector('.pane-failure-exit').textContent =
        failure === null
          ? `Generation: ${problem.generation ?? pane.generation}`
          : `Exit code: ${failure.exitCode == null ? 'unknown' : String(failure.exitCode)}`
    }
  }
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
  terminalSizes.delete(key)
  provisionalKeys.delete(key)
  if (retiringKeys.has(key)) return
  retiringKeys.add(key)
  void drainAndRetirePane(key)
}

function reconcileCards() {
  const visibleKeys = new Set()
  const authoritativeKeys = new Set()
  const authoritativePaneIds = new Set()
  for (const tab of viewState.tabs) {
    for (const pane of orderedPanes(tab)) {
      const key = paneKey(pane)
      authoritativeKeys.add(key)
      authoritativePaneIds.add(pane.id)
      if (!isVisiblePane(tab, pane)) {
        retirePane(key)
        continue
      }
      visibleKeys.add(key)
      retiredKeys.delete(key)
      let card = cards.get(key)
      if (card === undefined) {
        card = createCard(tab, pane)
        cards.set(key, card)
      }
      provisionalKeys.delete(key)
      card.dataset.provisional = 'false'
      updateCard(card, tab, pane)
      void syncTerminalSize(key)
    }
  }

  for (const [key, card] of cards) {
    const replacedGeneration =
      authoritativePaneIds.has(card.dataset.paneId) && !authoritativeKeys.has(key)
    if (replacedGeneration) retirePane(key)
  }

  for (const [key] of cards) {
    if (visibleKeys.has(key) || provisionalKeys.has(key) || retiringKeys.has(key)) continue
    retirePane(key)
  }
  registry.reconcile(new Set([...visibleKeys, ...provisionalKeys, ...retiringKeys]))
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

function showCards(tab, panes, indices, mode, layout) {
  const desired = indices.map((index) => cards.get(paneKey(panes[index]))).filter(Boolean)
  const wanted = new Set(desired)
  for (const card of cards.values()) if (!wanted.has(card)) parkCard(card)
  for (const child of [...stage.children]) {
    if (!wanted.has(child) && child !== workerStage) child.remove()
  }
  stage.dataset.empty = 'false'
  stage.dataset.mode = mode
  stage.dataset.layout = layout.areas
  stage.style.removeProperty('grid-template-areas')
  stage.style.gridTemplateRows = 'minmax(0, 1fr)'
  if (mode === 'grid' && desired.length > 1) {
    const styles = getComputedStyle(stage)
    const padding = cssPixels(styles.getPropertyValue('--pane-grid-padding'))
    const gap = cssPixels(styles.getPropertyValue('--pane-grid-gap'))
    const visibleColumns = Math.min(2, Math.ceil((desired.length - 1) / 2))
    const width = Math.max(
      0,
      (stage.clientWidth - 2 * padding - visibleColumns * gap) / (2 + visibleColumns),
    )
    stage.style.gridTemplateColumns = `${2 * width}px minmax(0, 1fr)`
    workerStage.style.gridAutoColumns = `${width}px`
    workerStage.style.gridTemplateRows =
      desired.length === 2 ? 'minmax(0, 1fr)' : 'repeat(2, minmax(0, 1fr))'
    if (desired[0].parentElement !== stage) stage.prepend(desired[0])
    if (workerStage.parentElement !== stage) stage.append(workerStage)
    for (const card of desired.slice(1)) {
      if (card.parentElement !== workerStage) workerStage.append(card)
    }
    workerStage.dataset.tab = tab.id
    workerStage.scrollLeft = scrollByTab.get(tab.id) ?? 0
  } else {
    stage.style.gridTemplateColumns = 'minmax(0, 1fr)'
    for (const card of desired) if (card.parentElement !== stage) stage.append(card)
    workerStage.remove()
  }
  for (const index of indices) {
    const pane = panes[index]
    const card = cards.get(paneKey(pane))
    if (card === undefined) continue
    card.dataset.parked = 'false'
    card.dataset.selected =
      mode === 'focused' || index === focusedIndex(tab, panes) ? 'true' : 'false'
    card.style.removeProperty('grid-area')
    card.style.removeProperty('width')
    card.style.removeProperty('height')
    registry.fit(pane.id, pane.generation)
  }
}

function renderPaneView() {
  const tab = activeTab()
  const panes = tab === null ? [] : orderedPanes(tab).filter((pane) => isVisiblePane(tab, pane))
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
        : 'Open a session to start a lead pane.'
    stage.append(empty)
    focusNav.hidden = true
    return
  }

  const layout = gridTemplate(panes.length)
  if (selection.type !== 'pane') {
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
  const parent = tab?.role === 'pm' ? viewState.tabs.find((t) => t.id === tab.parentTabId) : tab
  currentSession.textContent = parent == null ? 'No session' : sessionName(parent)
  groupViews.hidden = parent == null
  const pm = viewState.tabs.find((t) => t.role === 'pm' && t.parentTabId === parent?.id)
  for (const [button, group, label] of [
    [pmView, pm, 'PM'],
    [leadView, parent, 'Lead'],
  ]) {
    button.setAttribute('aria-pressed', String(group?.id === tab?.id))
    const count = group
      ? deliveriesFor(
          group,
          orderedPanes(group).find((p) => p.kind === 'lead'),
        ).length
      : 0
    button.textContent = `${label}${count ? ` · ${count} pending` : ''}`
  }
  currentDirectory.textContent = tab?.directory ?? tab?.dir ?? ''
  tabPolicy.hidden = tab === null || tab.closed === true
  deliveryInfo.hidden = tabPolicy.hidden
  newPane.hidden = tab === null || tab.closed === true
  if (tab !== null) {
    const mode = record(tab.policy) ? tab.policy.mode : tab.policy
    tabPolicy.textContent = `Reply delivery: ${mode === 'manual' ? 'Manual' : 'Automatic'}`
  }
  resultsToggle.hidden = tab === null
}

function selectGroup(tab) {
  if (tab.closed) {
    if (tab.role === 'pm')
      return void run('open_pm', { tab: tab.parentTabId, harness: tab.lead.harness })
    return void run('tab_resume', { tab: tab.id })
  }
  selection = { tabId: tab.id, type: 'session', paneId: null }
  render()
}

function render() {
  ensureSelection()
  const current = activeTab()
  if (current) selectionBySession.set(current.parentTabId ?? current.id, { ...selection })
  reconcileCards()
  renderSidebar(tree, viewState.tabs, {
    results: viewState.results ?? [],
    onOpenResults: showResults,
    selection,
    onSelectGroup: selectGroup,
    onSelectSession: (tab) => {
      const previous = selectionBySession.get(tab.id)
      const active = activeTab()
      selection =
        (active?.parentTabId ?? active?.id) === tab.id
          ? { tabId: active.id, type: 'session', paneId: null }
          : (previous ?? { tabId: tab.id, type: 'session', paneId: null })
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
    onOpenPm: (tab, pm, anchor) => {
      if (!pm) return menus.newPm(tab, anchor)
      if (pm.closed) return void run('open_pm', { tab: tab.id, harness: pm.lead.harness })
      selectGroup(pm)
    },
    onDeletePane: (tab, pane) => menus.deletePane({ ...pane, name: paneLabel(tab, pane) }),
    onResume: (tab) => void run('tab_resume', { tab: tab.id }),
    onRenameSession: (tab) => menus.renameSession(tab),
    onDeleteSession: (tab) => menus.deleteSession(tab),
    onAttach: (tab, pane) =>
      void run('open_consult', {
        tab: tab.id,
        agent: pane.agent,
        conversation: pane.conversation ?? pane.name,
        task: null,
      }),
  })
  renderHeader()
  renderResults()
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
    if (rosterFrame.src !== url.href) rosterFrame.src = url.href
    url.pathname = '/library'
    libraryUrl = url.href
    if (
      (libraryPanel.open || libraryFrame.hasAttribute('src')) &&
      libraryFrame.src !== libraryUrl
    ) {
      libraryFrame.src = libraryUrl
    }
    url.pathname = '/harnesses'
    harnessesUrl = url.href
    if (
      (harnessesPanel.open || harnessesFrame.hasAttribute('src')) &&
      harnessesFrame.src !== harnessesUrl
    ) {
      harnessesFrame.src = harnessesUrl
    }
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

function refreshAgentFrame(frame) {
  if (frame.hasAttribute('src')) {
    frame.contentWindow.postMessage('consensflow:refresh-agents', new URL(frame.src).origin)
  }
}

rosterToggle.addEventListener('click', () => {
  rosterPanel.showModal()
  refreshAgentFrame(rosterFrame)
})
rosterClose.addEventListener('click', () => rosterPanel.close())
rosterPanel.addEventListener('close', () => rosterToggle.focus())

libraryToggle.addEventListener('click', () => {
  if (libraryUrl !== null && libraryFrame.src !== libraryUrl) {
    libraryFrame.src = libraryUrl
  } else {
    refreshAgentFrame(libraryFrame)
  }
  libraryPanel.showModal()
})
libraryClose.addEventListener('click', () => libraryPanel.close())
libraryPanel.addEventListener('close', () => libraryToggle.focus())

harnessesToggle.addEventListener('click', () => {
  if (harnessesUrl !== null && harnessesFrame.src !== harnessesUrl) {
    harnessesFrame.src = harnessesUrl
  }
  harnessesPanel.showModal()
})
harnessesClose.addEventListener('click', () => harnessesPanel.close())
harnessesPanel.addEventListener('close', () => harnessesToggle.focus())

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

resultsToggle.addEventListener('click', () => {
  const tab = activeTab()
  if (tab) showResults(tab)
})
document.querySelector('#results-close').addEventListener('click', () => resultsDialog.close())

pmView.addEventListener('click', () => {
  const active = activeTab()
  const parent =
    active?.role === 'pm' ? viewState.tabs.find((t) => t.id === active.parentTabId) : active
  if (!parent) return
  const pm = viewState.tabs.find((t) => t.role === 'pm' && t.parentTabId === parent.id)
  if (pm) selectGroup(pm)
  else menus.newPm(parent, pmView)
})
leadView.addEventListener('click', () => {
  const tab = activeTab()
  const parent = tab?.role === 'pm' ? viewState.tabs.find((t) => t.id === tab.parentTabId) : tab
  if (parent) selectGroup(parent)
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
  await initializeUpdates({
    invoke: rawInvoke,
    listen,
    getState: () => viewState,
  })
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
