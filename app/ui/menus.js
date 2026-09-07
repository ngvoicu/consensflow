function asArray(value) {
  return Array.isArray(value) ? value : []
}

function agentName(agent) {
  if (typeof agent === 'string') return agent
  return agent?.name ?? agent?.id ?? agent?.callsign ?? null
}

function menuButton(label, onClick, ariaLabel = label) {
  const button = document.createElement('button')
  button.type = 'button'
  button.setAttribute('role', 'menuitem')
  button.setAttribute('aria-label', ariaLabel)
  button.textContent = label
  button.addEventListener('click', onClick)
  return button
}

function heading(label) {
  const element = document.createElement('div')
  element.className = 'menu-heading'
  element.textContent = label
  return element
}

function separator() {
  const element = document.createElement('div')
  element.className = 'menu-separator'
  element.setAttribute('role', 'separator')
  return element
}

export class Menus {
  constructor({ invoke, run, report }) {
    this.invoke = invoke
    this.run = run
    this.report = report
    this.layer = document.querySelector('#menu-layer')
    this.conversationDialog = document.querySelector('#new-conversation-dialog')
    this.agentDialog = document.querySelector('#agent-dialog')
    this.directory = document.querySelector('#conversation-directory')
    this.leadHarness = document.querySelector('#lead-harness')
    this.agentPicker = document.querySelector('#agent-picker')
    this.agentTask = document.querySelector('#agent-task')
    this.pendingDirectory = null
    this.pendingTab = null

    document.addEventListener('pointerdown', (event) => {
      if (!this.layer.contains(event.target)) this.closeMenu()
    })
    window.addEventListener('blur', () => this.closeMenu())

    for (const dialog of [this.conversationDialog, this.agentDialog]) {
      dialog.querySelector('button[value="cancel"]').addEventListener('click', () => dialog.close())
    }
    this.conversationDialog
      .querySelector('form')
      .addEventListener('submit', (event) => this.submitConversation(event))
    this.agentDialog
      .querySelector('form')
      .addEventListener('submit', (event) => this.submitAgent(event))
  }

  closeMenu() {
    this.layer.replaceChildren()
  }

  place(menu, anchorOrEvent) {
    this.closeMenu()
    menu.className = 'menu'
    this.layer.append(menu)
    const point =
      typeof anchorOrEvent?.clientX === 'number'
        ? { x: anchorOrEvent.clientX, y: anchorOrEvent.clientY }
        : (() => {
            const bounds = anchorOrEvent.getBoundingClientRect()
            return { x: bounds.left, y: bounds.bottom + 5 }
          })()
    const bounds = menu.getBoundingClientRect()
    menu.style.left = `${Math.max(8, Math.min(point.x, innerWidth - bounds.width - 8))}px`
    menu.style.top = `${Math.max(8, Math.min(point.y, innerHeight - bounds.height - 8))}px`
    menu.querySelector('button')?.focus()
  }

  async worker(event, tab, pane) {
    event.preventDefault()
    const menu = document.createElement('div')
    menu.setAttribute('role', 'menu')
    menu.setAttribute('aria-label', 'Worker actions')
    menu.append(heading('Send reply to lead…'))
    const loading = document.createElement('div')
    loading.className = 'answer-row'
    loading.textContent = 'Reading transcript…'
    menu.append(loading)
    menu.append(separator())
    menu.append(heading('Reply delivery'))
    for (const mode of ['auto', 'manual', 'inherit']) {
      const label =
        mode === 'auto' ? 'Automatic' : mode === 'manual' ? 'Manual' : 'Inherit session setting'
      menu.append(
        menuButton(label, async () => {
          this.closeMenu()
          await this.run('set_policy', { scope: 'pane', id: pane.id, mode })
        }),
      )
    }
    this.place(menu, event)

    let result
    try {
      result = await this.invoke('answers_list', {
        tab: tab.id,
        pane: pane.id,
        conversation: pane.conversation ?? pane.name,
      })
    } catch (cause) {
      result = { ok: false, error: cause instanceof Error ? cause.message : String(cause) }
    }
    if (!menu.isConnected) return
    loading.remove()
    if (result?.ok === false) {
      const unavailable = document.createElement('div')
      unavailable.className = 'answer-row'
      unavailable.textContent =
        result.error === 'unknown-op' || result.error === 'not-available-yet'
          ? 'Answers are not available yet'
          : `Answers unavailable: ${result.error ?? 'unknown error'}`
      menu.insertBefore(unavailable, menu.querySelector('.menu-separator'))
      return
    }

    const answers = asArray(result?.answers ?? result?.items ?? result)
    if (answers.length === 0) {
      const empty = document.createElement('div')
      empty.className = 'answer-row'
      empty.textContent = 'No completed answers'
      menu.insertBefore(empty, menu.querySelector('.menu-separator'))
      return
    }
    for (const answer of answers) {
      const unfinished = answer.ready === false
      const row = document.createElement('div')
      row.className = 'answer-row'
      row.dataset.state = unfinished
        ? 'in-progress'
        : answer.uncertain === true
          ? 'uncertain'
          : answer.delivered === true
            ? 'delivered'
            : 'ready'
      const preview = document.createElement('span')
      preview.className = 'answer-preview'
      preview.textContent = answer.preview ?? answer.text ?? answer.id
      const mark = document.createElement('span')
      mark.className = 'answer-mark'
      mark.textContent = unfinished
        ? 'In progress'
        : answer.uncertain === true
          ? 'Uncertain'
          : answer.delivered === true
            ? 'Delivered'
            : 'Ready'
      const resend = answer.delivered === true || answer.uncertain === true
      const actionLabel = unfinished
        ? `Waiting for ${answer.id} to complete`
        : resend
          ? `Resend ${answer.id} to lead`
          : `Send ${answer.id} to lead`
      const action = menuButton(
        unfinished ? 'Waiting for completion' : resend ? 'Resend to lead' : 'Send to lead',
        async () => {
          this.closeMenu()
          await this.run('deliver_now', {
            tab: tab.id,
            conversation: pane.conversation ?? pane.name,
            answerId: answer.id,
            resend,
          })
        },
        actionLabel,
      )
      action.disabled = unfinished
      row.append(preview, mark, action)
      if (answer.partProgress !== undefined && answer.delivered !== true) {
        const progress = document.createElement('span')
        progress.className = 'answer-mark'
        const { total, uncovered } = answer.partProgress
        const missing = asArray(uncovered)
        progress.textContent = `${total - missing.length} of ${total} parts confirmed.${
          missing.length > 0 ? ` Not confirmed: ${missing.join(', ')}.` : ''
        }`
        row.insertBefore(progress, action)
      }
      menu.insertBefore(row, menu.querySelector('.menu-separator'))
    }
    this.place(menu, event)
  }

  tabPolicy(anchor, tab) {
    const menu = document.createElement('div')
    menu.setAttribute('role', 'menu')
    menu.setAttribute('aria-label', 'Session reply delivery')
    menu.append(heading('Reply delivery to this lead'))
    for (const mode of ['auto', 'manual']) {
      const label = mode === 'auto' ? 'Automatic' : 'Manual'
      menu.append(
        menuButton(label, async () => {
          this.closeMenu()
          await this.run('set_policy', { scope: 'tab', id: tab.id, mode })
        }),
      )
    }
    this.place(menu, anchor)
  }

  newPane(anchor, tab, agents) {
    const menu = document.createElement('div')
    menu.setAttribute('role', 'menu')
    menu.setAttribute('aria-label', 'New pane')
    menu.append(heading('Open beside the lead'))
    menu.append(
      menuButton('Shell', async () => {
        this.closeMenu()
        await this.run('open_shell', { tab: tab.id })
      }),
      menuButton('Agent', () => {
        this.closeMenu()
        this.openAgent(tab, agents)
      }),
    )
    this.place(menu, anchor)
  }

  openAgent(tab, agents) {
    this.pendingTab = tab
    this.agentPicker.replaceChildren()
    const names = [...new Set(asArray(agents).map(agentName).filter(Boolean))]
    for (const name of names) {
      const option = document.createElement('option')
      option.value = name
      option.textContent = `@${name}`
      this.agentPicker.append(option)
    }
    this.agentTask.value = ''
    if (names.length === 0) {
      const option = document.createElement('option')
      option.value = ''
      option.textContent = 'No agents configured'
      this.agentPicker.append(option)
    }
    this.agentDialog.showModal()
  }

  async newConversation(dialogApi) {
    if (typeof dialogApi?.open !== 'function') {
      this.report('The directory picker is not available yet', 'error')
      return
    }
    let directory
    try {
      directory = await dialogApi.open({
        title: 'Choose a working directory',
        directory: true,
        multiple: false,
      })
    } catch (cause) {
      this.report(cause instanceof Error ? cause.message : String(cause), 'error')
      return
    }
    if (typeof directory !== 'string' || directory.length === 0) return
    this.pendingDirectory = directory
    this.directory.value = directory
    this.conversationDialog.showModal()
  }

  async submitConversation(event) {
    event.preventDefault()
    if (this.pendingDirectory === null) return
    const directory = this.pendingDirectory
    const harness = this.leadHarness.value
    this.conversationDialog.close()
    await this.run('open_lead', { dir: directory, harness })
  }

  async submitAgent(event) {
    event.preventDefault()
    if (this.pendingTab === null || this.agentPicker.value.length === 0) return
    const tab = this.pendingTab
    const agent = this.agentPicker.value
    const task = this.agentTask.value.trim()
    if (task.length === 0) return
    this.agentDialog.close()
    await this.run('open_consult', { tab: tab.id, agent, task })
  }
}
