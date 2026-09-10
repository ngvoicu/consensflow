function text(value, fallback) {
  return typeof value === 'string' && value.trim().length > 0 ? value : fallback
}

export function sessionName(tab) {
  if (typeof tab.name === 'string' && tab.name.trim().length > 0) return tab.name
  const directory = text(tab.directory, text(tab.dir, tab.id))
  return directory.split(/[\\/]/).filter(Boolean).at(-1) ?? tab.id
}

export function orderedPanes(tab) {
  const panes = Array.isArray(tab.panes) ? [...tab.panes] : []
  return panes.sort((left, right) => {
    if (left.kind === 'lead' && right.kind !== 'lead') return -1
    if (right.kind === 'lead' && left.kind !== 'lead') return 1
    return (left.order ?? 0) - (right.order ?? 0)
  })
}

export function paneLabel(tab, pane, workerNumber = null) {
  if (pane.kind === 'shell') return 'shell'
  if (pane.kind === 'lead') {
    if (tab.roleName) return `${tab.role === 'pm' ? 'pm' : 'lead'}: ${tab.roleName}`
    return text(pane.name, text(tab.lead?.name, text(tab.lead?.harness, 'lead')))
  }
  const name = text(pane.conversation, text(pane.name, text(pane.agent, 'worker')))
  return workerNumber === null ? name : `w${workerNumber} ${name}`
}

function button(label, testId, selected, onClick) {
  const element = document.createElement('button')
  element.type = 'button'
  element.className = 'tree-button'
  element.textContent = label
  element.dataset.testid = testId
  element.setAttribute('aria-current', selected ? 'true' : 'false')
  element.addEventListener('click', onClick)
  return element
}

function row(child) {
  const element = document.createElement('div')
  element.className = 'tree-row'
  element.append(child)
  return element
}

function group() {
  const element = document.createElement('ul')
  element.setAttribute('role', 'group')
  return element
}

function failedPane(pane) {
  return pane?.failure !== null && typeof pane?.failure === 'object' && !Array.isArray(pane.failure)
}

export function renderSidebar(
  container,
  tabs,
  {
    selection,
    onSelectSession,
    onSelectPane,
    onResume,
    onAttach,
    onRenameSession,
    onDeleteSession,
    onDeletePane,
    onOpenPm,
  },
) {
  container.replaceChildren()

  for (const tab of tabs.filter((tab) => tab.role !== 'pm')) {
    const session = document.createElement('li')
    session.className = 'tree-node'
    session.dataset.kind = 'session'
    session.dataset.testid = `session-${tab.id}`
    session.dataset.closed = tab.closed === true ? 'true' : 'false'
    session.setAttribute('role', 'treeitem')
    session.setAttribute('aria-level', '1')

    const sessionButton = button(
      sessionName(tab),
      `session-${tab.id}-button`,
      selection.tabId === tab.id && selection.type === 'session',
      () => onSelectSession(tab),
    )
    const sessionRow = row(sessionButton)
    const rename = document.createElement('button')
    rename.type = 'button'
    rename.className = 'rename-button'
    rename.dataset.testid = `rename-${tab.id}`
    rename.setAttribute('aria-label', 'Rename session')
    rename.textContent = 'Rename'
    rename.addEventListener('click', (event) => {
      event.stopPropagation()
      onRenameSession(tab)
    })
    sessionRow.append(rename)
    const remove = document.createElement('button')
    remove.type = 'button'
    remove.className = 'delete-button'
    remove.dataset.testid = `delete-${tab.id}`
    remove.setAttribute('aria-label', 'Delete session')
    remove.textContent = 'Delete'
    remove.addEventListener('click', (event) => {
      event.stopPropagation()
      onDeleteSession(tab)
    })
    sessionRow.append(remove)
    if (tab.closed === true && tab.deleting !== true) {
      const resume = document.createElement('button')
      resume.type = 'button'
      resume.className = 'resume-button'
      resume.dataset.testid = `resume-${tab.id}`
      resume.textContent = 'Resume'
      resume.addEventListener('click', (event) => {
        event.stopPropagation()
        onResume(tab)
      })
      sessionRow.append(resume)
    }
    session.append(sessionRow)

    const panes = orderedPanes(tab)
    const leadPane = panes.find((pane) => pane.kind === 'lead')
    const sessionChildren = group()
    if (leadPane !== undefined) {
      const lead = document.createElement('li')
      lead.className = 'tree-node'
      lead.dataset.kind = 'lead'
      lead.dataset.paneId = leadPane.id
      lead.dataset.state = failedPane(leadPane) ? 'failed' : 'live'
      lead.setAttribute('role', 'treeitem')
      lead.setAttribute('aria-level', '2')
      lead.append(
        row(
          button(
            paneLabel(tab, leadPane),
            `pane-node-${leadPane.id}`,
            selection.tabId === tab.id &&
              selection.type === 'pane' &&
              selection.paneId === leadPane.id,
            () => onSelectPane(tab, leadPane),
          ),
        ),
      )

      const descendants = group()
      let workerNumber = 0
      for (const pane of panes) {
        if (pane === leadPane) continue
        if (pane.kind === 'worker') workerNumber += 1
        const child = document.createElement('li')
        child.className = 'tree-node'
        child.dataset.kind = pane.kind
        child.dataset.paneId = pane.id
        child.dataset.closed = pane.alive === false || tab.closed === true ? 'true' : 'false'
        child.dataset.state = failedPane(pane) ? 'failed' : pane.closed === true ? 'closed' : 'live'
        child.setAttribute('role', 'treeitem')
        child.setAttribute('aria-level', '3')
        child.append(
          row(
            button(
              paneLabel(tab, pane, pane.kind === 'worker' ? workerNumber : null),
              `pane-node-${pane.id}`,
              selection.tabId === tab.id &&
                selection.type === 'pane' &&
                selection.paneId === pane.id,
              () => {
                if (
                  pane.kind === 'worker' &&
                  !failedPane(pane) &&
                  (tab.closed === true || pane.alive === false)
                ) {
                  onAttach(tab, pane)
                } else {
                  onSelectPane(tab, pane)
                }
              },
            ),
          ),
        )
        if (onDeletePane) {
          const remove = document.createElement('button')
          remove.type = 'button'
          remove.className = 'delete-button'
          remove.textContent = 'Delete'
          remove.setAttribute('aria-label', `Delete pane ${paneLabel(tab, pane)}`)
          remove.addEventListener('click', (event) => {
            event.stopPropagation()
            onDeletePane(tab, pane)
          })
          child.firstElementChild.append(remove)
        }
        descendants.append(child)
      }
      if (descendants.childElementCount > 0) {
        lead.setAttribute('aria-expanded', 'true')
        lead.append(descendants)
      }
      sessionChildren.append(lead)
    }
    if (onOpenPm) {
      const pm = tabs.find(
        (candidate) => candidate.role === 'pm' && candidate.parentTabId === tab.id,
      )
      const child = document.createElement('li')
      child.className = 'tree-node'
      child.dataset.kind = 'pm'
      child.setAttribute('role', 'treeitem')
      child.setAttribute('aria-level', '2')
      const open = button(
        pm ? `pm: ${pm.roleName}` : 'Add project manager',
        pm ? `pm-${pm.id}` : `add-pm-${tab.id}`,
        pm?.id === selection.tabId,
        () => onOpenPm(tab, pm, open),
      )
      child.append(row(open))
      sessionChildren.prepend(child)
    }
    if (sessionChildren.childElementCount > 0) {
      session.setAttribute('aria-expanded', 'true')
      session.append(sessionChildren)
    }
    container.append(session)
  }
}
