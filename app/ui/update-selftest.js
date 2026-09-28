const SECOND_WORKSPACE = '.consensflow-updater-second'

function record(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function failure(result, operation) {
  if (!record(result)) return `${operation} returned no snapshot`
  if (result.ok === false) return `${operation} refused: ${result.error ?? 'unknown error'}`
  return null
}

async function report(invoke, event, data) {
  const result = await invoke('selftest_report', { event, data })
  if (result?.ok === false) throw new Error(result.error)
}

async function status(invoke, operation = 'update_status') {
  let result
  try {
    result = await invoke(operation)
  } catch (cause) {
    throw new Error(
      `${operation} failed: ${cause instanceof Error ? cause.message : String(cause)}`,
    )
  }
  const refused = failure(result, operation)
  if (refused !== null) throw new Error(refused)
  if (!Array.isArray(result.blockers)) throw new Error(`${operation} returned no blocker snapshot`)
  return result
}

function expectedCandidate(snapshot, expected) {
  return (
    record(snapshot.available) &&
    snapshot.available.version === expected &&
    snapshot.phase === 'available'
  )
}

function expectedReady(snapshot, expected, blockers) {
  return (
    snapshot.phase === 'ready' &&
    snapshot.available?.version === expected &&
    snapshot.blockers.length === blockers
  )
}

async function closeOwnProjects(core, refresh, projects) {
  for (const project of projects) {
    await core('project.close', { project })
    await refresh()
  }
}

async function waitForNoBlockers(invoke, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const snapshot = await status(invoke)
    if (snapshot.blockers.length === 0) return snapshot
    if (Date.now() >= deadline) {
      throw new Error(`panes did not close before install: ${snapshot.blockers.length} blockers`)
    }
    await new Promise((wake) => setTimeout(wake, 100))
  }
}

/**
 * Packaged updater selftest. It is intentionally a command-level driver:
 * update_status, update_check, update_download and update_install are the
 * same guarded bridge calls a human's updater dialog uses, and the windows
 * that block the install belong to two projects opened the way a human's New
 * project opens them and closed the way a human's Close closes them. The
 * Node smoke owns the external HTTPS/signature/build boundary.
 */
export async function runUpdateSelftest({ config, invoke, core, refresh }) {
  const expected = config?.updaterExpectedVersion
  const ownProjects = []
  let installStarted = false

  try {
    const boot = await status(invoke)
    await report(invoke, 'update-boot', { currentVersion: boot.currentVersion })

    if (boot.currentVersion === expected) {
      await refresh()
      const restarted = await status(invoke)
      if (restarted.blockers.length !== 0) {
        throw new Error(`restarted app has ${restarted.blockers.length} open panes`)
      }
      await report(invoke, 'update-restarted', {
        currentVersion: restarted.currentVersion,
        blockers: restarted.blockers.length,
        phase: restarted.phase,
        ready: true,
      })
      return
    }

    if (typeof config?.dir !== 'string' || config.dir.length === 0) {
      throw new Error('the updater selftest has no workspace directory')
    }
    const secondDir = `${config.dir}/${SECOND_WORKSPACE}`
    for (const directory of [config.dir, secondDir]) {
      const { project } = await core('project.open', { directory, harness: 'claude-code' })
      if (!Number.isInteger(project?.id)) {
        throw new Error(`project.open returned no project: ${JSON.stringify(project)}`)
      }
      ownProjects.push(project.id)
      await refresh()
    }

    const checked = await status(invoke, 'update_check')
    if (!expectedCandidate(checked, expected)) {
      throw new Error(`update_check did not offer ${expected}: ${JSON.stringify(checked)}`)
    }

    const downloaded = await status(invoke, 'update_download')
    if (!expectedReady(downloaded, expected, 2)) {
      throw new Error(
        `update_download did not reach ready with two blockers: ${JSON.stringify(downloaded)}`,
      )
    }

    // This event is emitted before the deliberately blocked install. The
    // external driver can observe both fake PTYs and the unchanged app PID.
    await report(invoke, 'update-before-install', {
      phase: downloaded.phase,
      currentVersion: downloaded.currentVersion,
      available: downloaded.available,
      blockers: downloaded.blockers,
      downloadedBytes: downloaded.downloadedBytes,
    })

    const blocked = await invoke('update_install')
    if (
      blocked?.ok !== false ||
      blocked.phase !== 'ready' ||
      blocked.currentVersion !== downloaded.currentVersion ||
      !Array.isArray(blocked.blockers) ||
      blocked.blockers.length !== 2
    ) {
      throw new Error(
        `update_install did not refuse with both blockers: ${JSON.stringify(blocked)}`,
      )
    }

    await report(invoke, 'update-blocked', blocked)
    await closeOwnProjects(core, refresh, ownProjects)
    ownProjects.length = 0
    const clear = await waitForNoBlockers(invoke)
    if (!expectedReady(clear, expected, 0)) {
      throw new Error(`blockers did not clear before install: ${JSON.stringify(clear)}`)
    }

    // Refresh is the authority immediately before installation. In
    // particular, do not reuse `clear`: a failed refresh must never fall
    // through to update_install with a stale ready snapshot.
    const fresh = await status(invoke)
    if (!expectedReady(fresh, expected, 0)) {
      throw new Error(`fresh install status is not ready and unblocked: ${JSON.stringify(fresh)}`)
    }
    await report(invoke, 'update-before-install', {
      phase: fresh.phase,
      currentVersion: fresh.currentVersion,
      available: fresh.available,
      blockers: fresh.blockers,
      downloadedBytes: fresh.downloadedBytes,
    })

    installStarted = true
    // A successful install restarts the app and can close the invoke pipe
    // before its promise settles. An explicit {ok:false} is still a real
    // backend refusal and is reported; transport teardown is expected here.
    void Promise.resolve(invoke('update_install'))
      .then((result) => {
        if (result?.ok === false) {
          return report(invoke, 'update-failure', {
            error: result.error ?? 'update_install refused',
          })
        }
        return undefined
      })
      .catch(() => {})
  } catch (cause) {
    if (installStarted) return
    try {
      await closeOwnProjects(core, refresh, ownProjects)
    } catch {
      // The original failure is the useful evidence; the app's normal
      // selftest shutdown still owns the recorded process group.
    }
    await report(invoke, 'update-failure', {
      error: cause instanceof Error ? cause.message : String(cause),
    })
  }
}
