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

export async function initializeUpdates({ invoke, listen, getState } = {}) {
  const dialog = document.createElement('dialog')
  dialog.id = 'updates-dialog'
  dialog.setAttribute('aria-labelledby', 'updates-title')
  dialog.setAttribute('aria-describedby', 'updates-description')

  const form = document.createElement('form')
  form.className = 'updates-dialog-form'
  form.method = 'dialog'
  dialog.append(form)

  const header = document.createElement('header')
  header.className = 'updates-header'
  const brand = document.createElement('span')
  brand.className = 'updates-brand'
  brand.textContent = 'ConsensFlow'
  const title = document.createElement('h2')
  title.id = 'updates-title'
  title.textContent = 'Updates'
  const close = document.createElement('button')
  close.className = 'quiet-button'
  close.id = 'updates-close'
  close.type = 'button'
  close.autofocus = true
  close.setAttribute('aria-label', 'Close Updates')
  close.textContent = 'Close'
  header.append(brand, title, close)
  form.append(header)

  const content = document.createElement('div')
  content.className = 'updates-content'
  const description = document.createElement('p')
  description.id = 'updates-description'
  description.className = 'updates-description'
  description.textContent = 'Review signed releases and choose when to download or restart.'
  content.append(description)

  const metadata = document.createElement('div')
  metadata.className = 'updates-metadata'
  const installed = document.createElement('p')
  installed.className = 'updates-installed'
  installed.textContent = 'Installed: '
  const installedValue = document.createElement('strong')
  installedValue.id = 'updates-installed-version'
  installed.append(installedValue)
  const channelLabel = document.createElement('label')
  channelLabel.textContent = 'Channel'
  const channel = document.createElement('select')
  channel.id = 'updates-channel'
  channel.name = 'channel'
  for (const value of ['stable', 'alpha']) {
    const option = document.createElement('option')
    option.value = value
    option.textContent = value === 'alpha' ? 'Alpha' : 'Stable'
    channel.append(option)
  }
  channelLabel.append(channel)
  metadata.append(installed, channelLabel)
  content.append(metadata)

  const stateLine = document.createElement('p')
  stateLine.className = 'updates-state'
  stateLine.id = 'updates-state'
  stateLine.setAttribute('role', 'status')
  stateLine.setAttribute('aria-live', 'polite')
  content.append(stateLine)

  const feedback = document.createElement('p')
  feedback.className = 'updates-feedback'
  feedback.id = 'updates-feedback'
  feedback.setAttribute('role', 'status')
  feedback.setAttribute('aria-live', 'polite')
  content.append(feedback)

  const candidateSection = document.createElement('section')
  candidateSection.className = 'updates-candidate'
  const candidateHeading = document.createElement('h3')
  candidateHeading.textContent = 'Available version: '
  const candidateVersion = document.createElement('strong')
  candidateVersion.id = 'updates-candidate-version'
  candidateHeading.append(candidateVersion)
  const candidateDate = document.createElement('p')
  candidateDate.className = 'updates-date'
  const notesLabel = document.createElement('h4')
  notesLabel.textContent = 'Release notes'
  const notes = document.createElement('p')
  notes.className = 'updates-notes'
  notes.id = 'updates-notes'
  candidateSection.append(candidateHeading, candidateDate, notesLabel, notes)
  content.append(candidateSection)

  const progress = document.createElement('p')
  progress.className = 'updates-progress'
  progress.id = 'updates-progress'
  content.append(progress)

  const blockers = document.createElement('section')
  blockers.className = 'updates-blockers'
  const blockersHeading = document.createElement('h3')
  blockersHeading.textContent = 'Before installing'
  const blockersMessage = document.createElement('p')
  blockersMessage.id = 'updates-blockers-message'
  const blockerList = document.createElement('ul')
  blockerList.id = 'updates-blocker-list'
  blockers.append(blockersHeading, blockersMessage, blockerList)
  content.append(blockers)

  form.append(content)

  const actions = document.createElement('div')
  actions.className = 'updates-actions'
  const check = document.createElement('button')
  check.className = 'quiet-button'
  check.id = 'updates-check'
  check.type = 'button'
  check.textContent = 'Check for updates'
  const download = document.createElement('button')
  download.className = 'primary-button'
  download.id = 'updates-download'
  download.type = 'button'
  download.textContent = 'Download update'
  const later = document.createElement('button')
  later.className = 'quiet-button'
  later.id = 'updates-later'
  later.type = 'button'
  later.textContent = 'Later'
  const install = document.createElement('button')
  install.className = 'primary-button'
  install.id = 'updates-install'
  install.type = 'button'
  install.textContent = 'Install and restart'
  actions.append(check, download, later, install)
  form.append(actions)
  document.body.append(dialog)

  const banner = document.createElement('aside')
  banner.className = 'update-banner'
  banner.id = 'update-banner'
  banner.setAttribute('role', 'status')
  banner.setAttribute('aria-live', 'polite')
  const bannerText = document.createElement('span')
  const bannerOpen = document.createElement('button')
  bannerOpen.className = 'quiet-button'
  bannerOpen.type = 'button'
  bannerOpen.textContent = 'Review update'
  const bannerDismiss = document.createElement('button')
  bannerDismiss.className = 'icon-button'
  bannerDismiss.type = 'button'
  bannerDismiss.setAttribute('aria-label', 'Dismiss update notice')
  bannerDismiss.textContent = '×'
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
      const item = document.createElement('li')
      item.textContent = blockerName(blocker, getState)
      blockerList.append(item)
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
