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

export function renderSidebar(
  container,
  tabs,
  { selection, onSelectSession, onSelectPane, onResume, onAttach, onRenameSession },
) {
  container.replaceChildren()

  for (const tab of tabs) {
    const session = document.createElement('li')
    session.className = 'tree-node'
    session.dataset.kind = 'session'
    session.dataset.testid = `session-${tab.id}`
    session.dataset.closed = tab.closed === true ? 'true' : 'false'
    session.setAttribute('role', 'treeitem')
    session.setAttribute('aria-level', '1')
    session.setAttribute('aria-expanded', 'true')

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
    if (tab.closed === true) {
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
      lead.setAttribute('role', 'treeitem')
      lead.setAttribute('aria-level', '2')
      lead.setAttribute('aria-expanded', 'true')
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
                if (pane.kind === 'worker' && (tab.closed === true || pane.alive === false)) {
                  onAttach(tab, pane)
                } else {
                  onSelectPane(tab, pane)
                }
              },
            ),
          ),
        )
        descendants.append(child)
      }
      lead.append(descendants)
      sessionChildren.append(lead)
    }
    session.append(sessionChildren)
    container.append(session)
  }
}
