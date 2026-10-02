import { element } from './dom.js'

const SIX_HOURS = 6 * 60 * 60 * 1_000
const QUIET_DELAY = 10_000
const BUSY_PHASES = new Set(['checking', 'downloading', 'installing'])
const CHANNELS = new Set(['alpha', 'stable'])
const DISMISSED_KEY = 'consensflow.dismissed-update-versions.v1'
// Blockers the backend synthesizes for panes already gone from its table, so
// they match no open pane and must explain themselves.
const CLEANUP_LABELS = [
  ['finishing-pane-cleanup-', 'A closed pane is still finishing cleanup; this clears on its own.'],
  [
    'unconfirmed-pane-cleanup-',
    'A closed pane never confirmed that it stopped. Quit and reopen ConsensFlow to install.',
  ],
]

function record(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function array(value) {
  return Array.isArray(value) ? value : []
}

function text(value, fallback = '') {
  return typeof value === 'string' ? value : fallback
}

function normalizeSnapshot(value) {
  if (!record(value) || value.ok === false) return null
  const candidate = record(value.available)
    ? {
        version: text(value.available.version, 'Unknown version'),
        notes: text(value.available.notes),
        date: text(value.available.date),
      }
    : null
  return {
    ok: true,
    currentVersion: text(value.currentVersion, 'Unknown'),
    channel: CHANNELS.has(value.channel) ? value.channel : 'stable',
    phase: text(value.phase, 'idle'),
    available: candidate,
    downloadedBytes: Number.isFinite(value.downloadedBytes) ? value.downloadedBytes : 0,
    totalBytes: Number.isFinite(value.totalBytes) ? value.totalBytes : null,
    lastChecked: value.lastChecked === null ? null : text(value.lastChecked),
    error: value.error === null ? null : text(value.error),
    blockers: array(value.blockers).filter(record),
  }
}

function commandFailure(result) {
  if (!record(result) || result.ok !== false) return null
  return text(result.error, 'The update operation failed')
}

function eventPayload(event) {
  return record(event?.payload) ? event.payload : event
}

function formatBytes(bytes) {
  return Number.isFinite(bytes) ? bytes.toLocaleString('en-US') : '0'
}

function blockerName(blocker, getState) {
  const id = text(blocker.id, 'unknown pane')
  const generation = Number.isSafeInteger(blocker.generation) ? blocker.generation : null
  for (const [prefix, label] of CLEANUP_LABELS) {
    if (id.startsWith(prefix)) return label
  }
  let state
  try {
    state = getState?.()
  } catch {
    state = null
  }
  for (const tab of array(state?.tabs)) {
    for (const pane of array(tab.panes)) {
      if (pane.id !== blocker.id || (generation !== null && pane.generation !== generation))
        continue
      const session = text(tab.name, text(tab.id, 'session'))
      const paneLabel = text(pane.name, text(pane.agent, text(pane.kind, id)))
      return `${session} / ${paneLabel}`
    }
  }
  return generation === null ? id : `${id} (generation ${generation})`
}

/** An element found by its `id`: by the dialog's own labels, and by the updater's tests. */
function named(tag, id, className, text) {
  const node = element(tag, className, text)
  node.id = id
  return node
}

/** A line read out when it changes. */
function liveLine(id) {
  const line = named('p', id, id)
  line.setAttribute('role', 'status')
  line.setAttribute('aria-live', 'polite')
  return line
}

/** A button of the updater's; what it does is wired once the dialog and banner are drawn. */
function updaterButton(className, text, id) {
  const node = element('button', className, text)
  node.type = 'button'
  if (id) node.id = id
  return node
}

export async function initializeUpdates({ invoke, listen, getState } = {}) {
  const dialog = named('dialog', 'updates-dialog')
  dialog.setAttribute('aria-labelledby', 'updates-title')
  dialog.setAttribute('aria-describedby', 'updates-description')
  const close = updaterButton('quiet-button', 'Close', 'updates-close')
  close.autofocus = true
  close.setAttribute('aria-label', 'Close Updates')
  const header = element('header', 'updates-header')
  header.append(
    element('span', 'updates-brand', 'ConsensFlow'),
    named('h2', 'updates-title', null, 'Updates'),
    close,
  )

  const installed = element('p', 'updates-installed', 'Installed: ')
  const installedValue = named('strong', 'updates-installed-version')
  installed.append(installedValue)
  const channel = named('select', 'updates-channel')
  channel.name = 'channel'
  for (const value of ['stable', 'alpha']) {
    const option = element('option', null, value === 'alpha' ? 'Alpha' : 'Stable')
    option.value = value
    channel.append(option)
  }
  const channelLabel = element('label', null, 'Channel')
  channelLabel.append(channel)
  const metadata = element('div', 'updates-metadata')
  metadata.append(installed, channelLabel)

  const candidateVersion = named('strong', 'updates-candidate-version')
  const candidateHeading = element('h3', null, 'Available version: ')
  candidateHeading.append(candidateVersion)
  const candidateDate = element('p', 'updates-date')
  const notes = named('p', 'updates-notes', 'updates-notes')
  const candidateSection = element('section', 'updates-candidate')
  candidateSection.append(
    candidateHeading,
    candidateDate,
    element('h4', null, 'Release notes'),
    notes,
  )

  const blockersMessage = named('p', 'updates-blockers-message')
  const blockerList = named('ul', 'updates-blocker-list')
  const blockers = element('section', 'updates-blockers')
  blockers.append(element('h3', null, 'Before installing'), blockersMessage, blockerList)

  const stateLine = liveLine('updates-state')
  const feedback = liveLine('updates-feedback')
  const progress = named('p', 'updates-progress', 'updates-progress')
  const content = element('div', 'updates-content')
  content.append(
    named(
      'p',
      'updates-description',
      'updates-description',
      'Review signed releases and choose when to download or restart.',
    ),
    metadata,
    stateLine,
    feedback,
    candidateSection,
    progress,
    blockers,
  )

  const check = updaterButton('quiet-button', 'Check for updates', 'updates-check')
  const download = updaterButton('primary-button', 'Download update', 'updates-download')
  const later = updaterButton('quiet-button', 'Later', 'updates-later')
  const install = updaterButton('primary-button', 'Install and restart', 'updates-install')
  const actions = element('div', 'updates-actions')
  actions.append(check, download, later, install)

  const form = element('form', 'updates-dialog-form')
  form.method = 'dialog'
  form.append(header, content, actions)
  dialog.append(form)
  document.body.append(dialog)

  // The notice that an update is out: its version, Review and Dismiss.
  const banner = named('aside', 'update-banner', 'update-banner')
  banner.setAttribute('role', 'status')
  banner.setAttribute('aria-live', 'polite')
  const bannerText = element('span')
  const bannerOpen = updaterButton('quiet-button', 'Review update')
  const bannerDismiss = updaterButton('icon-button', '×')
  bannerDismiss.setAttribute('aria-label', 'Dismiss update notice')
  banner.append(bannerText, bannerOpen, bannerDismiss)
  document.body.append(banner)

  let snapshot = null
  let feedbackText = ''
  let feedbackTone = 'info'
  let previousFocus = null
  let operation = null
  const dismissedVersions = loadDismissedVersions()

  function loadDismissedVersions() {
    try {
      const stored = JSON.parse(sessionStorage.getItem(DISMISSED_KEY) ?? '[]')
      return new Set(array(stored).filter((value) => typeof value === 'string'))
    } catch {
      return new Set()
    }
  }

  function saveDismissedVersions() {
    try {
      sessionStorage.setItem(DISMISSED_KEY, JSON.stringify([...dismissedVersions]))
    } catch {
      // Dismissal is a convenience; the updater remains usable without storage.
    }
  }

  function busy() {
    return snapshot !== null && BUSY_PHASES.has(snapshot.phase)
  }

  function candidateVersionValue() {
    return snapshot?.available?.version ?? null
  }

  function setFeedback(message, tone = 'info') {
    feedbackText = text(message)
    feedbackTone = tone
  }

  function renderBanner() {
    const version = candidateVersionValue()
    const visible = version !== null && !dismissedVersions.has(version)
    banner.hidden = !visible
    if (visible) bannerText.textContent = `Update available: ${version}`
  }

  function renderBlockers() {
    const current = snapshot ?? normalizeSnapshot({})
    const currentBlockers = current.blockers
    blockerList.replaceChildren()
    blockers.hidden = currentBlockers.length === 0
    if (currentBlockers.length === 0) return
    const count = currentBlockers.filter(
      (item) => !CLEANUP_LABELS.some(([prefix]) => item.id?.startsWith(prefix)),
    ).length
    blockersMessage.textContent =
      count === 0
        ? 'Installation is waiting for closed-pane cleanup.'
        : `Installation is blocked by ${count} open ${count === 1 ? 'pane' : 'panes'}. ` +
          'All open panes must be closed or suspended first, even if idle. ' +
          'ConsensFlow cannot safely infer unsent native-editor drafts.'
    for (const blocker of currentBlockers) {
      blockerList.append(element('li', null, blockerName(blocker, getState)))
    }
  }

  function render() {
    const current = snapshot
    installedValue.textContent = current?.currentVersion ?? 'Unknown'
    channel.value = current?.channel ?? 'stable'
    channel.disabled = current === null || busy()
    stateLine.textContent =
      current === null ? 'Update status is not available yet.' : stateText(current)
    feedback.textContent = feedbackText
    feedback.dataset.tone = feedbackTone
    feedback.hidden = feedbackText.length === 0

    const candidate = current?.available ?? null
    candidateSection.hidden = candidate === null
    if (candidate !== null) {
      candidateVersion.textContent = candidate.version
      candidateDate.textContent = candidate.date.length > 0 ? `Published ${candidate.date}` : ''
      notes.textContent = candidate.notes
    }

    const hasDownload = candidate !== null
    const isReady = current?.phase === 'ready'
    const bytes = current?.downloadedBytes ?? 0
    const total = current?.totalBytes
    progress.hidden = !hasDownload && !busy() && !isReady
    progress.textContent = progressText(current, bytes, total)
    check.disabled = current !== null && busy()
    download.disabled = !hasDownload || busy() || isReady
    install.disabled =
      !hasDownload || current?.phase !== 'ready' || (current?.blockers?.length ?? 0) !== 0
    renderBlockers()
    renderBanner()
  }

  function stateText(current) {
    if (current.phase === 'checking') return 'Checking for signed updates…'
    if (current.phase === 'downloading') return 'Downloading update…'
    if (current.phase === 'ready') return 'Update downloaded and ready to install.'
    if (current.phase === 'installing') return 'Installing update and preparing restart…'
    if (current.phase === 'error') return 'The last update operation failed.'
    if (current.available !== null) return 'A newer signed release is available.'
    return 'No update candidate is available.'
  }

  function progressText(current, bytes, total) {
    if (current === null || current.available === null) return ''
    if (total === null) return `Downloaded ${formatBytes(bytes)} bytes`
    return `Downloaded ${formatBytes(bytes)} of ${formatBytes(total)} bytes`
  }

  function applySnapshot(value) {
    const next = normalizeSnapshot(value)
    if (next === null) return false
    snapshot = next
    if (next.error !== null && next.phase === 'error') setFeedback(next.error, 'error')
    render()
    return true
  }

  async function call(command, args = {}) {
    if (typeof invoke !== 'function')
      return { ok: false, error: 'Update commands are not available yet' }
    try {
      return await invoke(command, args)
    } catch (cause) {
      return { ok: false, error: cause instanceof Error ? cause.message : String(cause) }
    }
  }

  async function refreshStatus({ explicit = false } = {}) {
    const result = await call('update_status')
    if (applySnapshot(result)) return true
    const failure = commandFailure(result)
    if (explicit && failure !== null) setFeedback(failure, 'error')
    render()
    return false
  }

  async function checkForUpdates({ quiet = false } = {}) {
    if (operation !== null) return
    operation = 'checking'
    const result = await call('update_check')
    operation = null
    if (applySnapshot(result)) {
      if (!quiet) {
        if (snapshot.error !== null) setFeedback(snapshot.error, 'error')
        else if (snapshot.available === null) setFeedback('You are up to date.', 'info')
        else setFeedback(`Update available: ${snapshot.available.version}`, 'info')
        render()
      }
      return
    }
    if (!quiet) {
      setFeedback(commandFailure(result) ?? 'The update check failed', 'error')
      render()
    }
  }

  async function openDialog({ checkNow = true } = {}) {
    previousFocus = document.activeElement
    if (!dialog.open) {
      dialog.showModal()
      close.focus()
    }
    render()
    await refreshStatus({ explicit: true })
    if (checkNow) await checkForUpdates()
    close.focus()
  }

  async function downloadUpdate() {
    if (snapshot?.available === null || busy() || snapshot?.phase === 'ready') return
    const result = await call('update_download')
    if (!applySnapshot(result)) {
      setFeedback(commandFailure(result) ?? 'The update download failed', 'error')
      render()
    }
  }

  async function changeChannel() {
    if (snapshot === null || busy()) return
    const selected = channel.value
    if (!CHANNELS.has(selected) || selected === snapshot.channel) return
    const previous = snapshot.channel
    const result = await call('update_channel', { channel: selected })
    if (!applySnapshot(result)) {
      channel.value = previous
      setFeedback(commandFailure(result) ?? 'The channel could not be changed', 'error')
      render()
      return
    }
    await checkForUpdates()
  }

  async function installUpdate() {
    if (!(await refreshStatus({ explicit: true }))) return
    const current = snapshot
    if (
      current === null ||
      current.phase !== 'ready' ||
      current.available === null ||
      current.blockers.length > 0
    ) {
      if (current?.blockers.length > 0) {
        setFeedback(
          'Installation is still blocked. Close or suspend all open panes first, even if idle.',
          'error',
        )
      } else {
        setFeedback('Install is available only when a downloaded update is ready.', 'error')
      }
      render()
      return
    }
    const result = await call('update_install')
    if (applySnapshot(result)) return
    setFeedback(`Installation was refused: ${commandFailure(result) ?? 'unknown error'}`, 'error')
    await refreshStatus()
    render()
  }

  function dismissBanner() {
    const version = candidateVersionValue()
    if (version === null) return
    dismissedVersions.add(version)
    saveDismissedVersions()
    renderBanner()
  }

  close.addEventListener('click', () => dialog.close())
  later.addEventListener('click', () => {
    dismissBanner()
    dialog.close()
  })
  bannerOpen.addEventListener('click', () => void openDialog())
  bannerDismiss.addEventListener('click', dismissBanner)
  check.addEventListener('click', () => void checkForUpdates())
  download.addEventListener('click', () => void downloadUpdate())
  channel.addEventListener('change', () => void changeChannel())
  install.addEventListener('click', () => void installUpdate())
  dialog.addEventListener('close', () => previousFocus?.focus?.())

  if (typeof listen === 'function') {
    try {
      await listen('update-state-changed', (event) => applySnapshot(eventPayload(event)))
    } catch {
      // Update events are a convenience; explicit status commands still work.
    }
    try {
      await listen('check-updates', () => void openDialog())
    } catch {
      // Quiet checks and update notices remain available if the menu event is absent.
    }
  }

  if (!window.__CONSENSFLOW_SELFTEST__) {
    const schedule = (delay) => {
      window.setTimeout(async () => {
        await checkForUpdates({ quiet: true })
        schedule(SIX_HOURS)
      }, delay)
    }
    schedule(QUIET_DELAY)
  }
  render()
}
