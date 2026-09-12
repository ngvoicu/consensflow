/** Only the native integration supplies selection, readiness and insertion. */
export function createReceiver({
  session,
  ready,
  insert,
  request,
  config,
  registration = () => ({}),
  onError = () => {},
}) {
  request ??= receiverRequest(config)
  let previous,
    binding,
    running,
    timer,
    stopped = false
  const release = (claim, admitted, bytesWritten, reason) =>
    request('release', {
      result: claim.result,
      claim: claim.id,
      lease: claim.receiver.lease,
      admitted,
      bytesWritten,
      reason,
    })
  const selected = (claim) =>
    !stopped &&
    session() === claim.receiver.session &&
    binding?.lease === claim.receiver.lease &&
    ready()
  async function step() {
    if (stopped) return
    const current = session()
    if (!current) {
      if (binding) {
        await request('retire', { lease: binding.lease })
        binding = null
      }
      return
    }
    if (previous === undefined) previous = (await request('state', {}))?.lease ?? null
    if (stopped || session() !== current) return
    if (binding?.session !== current) {
      binding = await request('register', { ...registration(), session: current, previous })
      previous = binding.lease
    }
    if (stopped || session() !== current || !ready()) return
    const claim = await request('claim', { lease: binding.lease })
    if (!claim) return
    if (!selected(claim)) {
      await release(claim, false, 0, 'native selection or readiness changed before insertion')
      return
    }
    await request('begin', { result: claim.result, claim: claim.id, lease: claim.receiver.lease })
    if (!selected(claim)) {
      await release(claim, false, 0, 'native selection or readiness changed before insertion')
      return
    }
    // No await between this final selection check and invoking the native adapter.
    let outcome
    try {
      outcome = await insert(claim)
    } catch (error) {
      await release(claim, null, undefined, String(error.message)).catch(onError)
      throw error
    }
    if (outcome?.admitted === false) {
      await release(claim, false, outcome.bytesWritten, outcome.reason)
      return
    }
    // The app checks the actual native history. API success never substitutes for receipt.
    await request('receipt', { result: claim.result, claim: claim.id, lease: claim.receiver.lease })
  }
  function poll() {
    if (stopped) return Promise.resolve()
    if (!running)
      running = step().finally(() => {
        running = null
      })
    return running
  }
  return {
    poll,
    start() {
      if (stopped || timer) return
      timer = setInterval(() => {
        poll().catch(onError)
      }, 750)
      timer.unref?.()
      poll().catch(onError)
    },
    async stop() {
      stopped = true
      clearInterval(timer)
      await running?.catch(onError)
      if (binding) await request('retire', { lease: binding.lease })
      binding = null
    },
  }
}

export function receiverRequest(config) {
  if (typeof config === 'string') config = JSON.parse(config)
  if (!config?.url || !config.token) throw new Error('receiver configuration is required')
  const origin = new URL(config.url)
  if (origin.protocol !== 'http:' || !['127.0.0.1', '[::1]', 'localhost'].includes(origin.hostname))
    throw new Error('receiver service must be local')
  return async (operation, body) => {
    const response = await fetch(new URL(`/api/receiver/${operation}`, origin), {
      method: 'POST',
      headers: { Authorization: `Bearer ${config.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(5000),
    })
    const data = await response.json()
    if (!response.ok)
      throw Object.assign(new Error(data.error ?? `receiver refused ${operation}`), {
        status: response.status,
      })
    return data
  }
}
