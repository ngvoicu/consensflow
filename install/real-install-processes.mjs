// End the processes listed by the Mac snapshot for the app this run opened.
export function endListedProcesses(lines, kill = process.kill) {
  for (const line of lines) {
    try {
      kill(Number(line.trim().split(/\s+/)[0]), 'SIGTERM')
    } catch (error) {
      if (error?.code !== 'ESRCH') throw error
    }
  }
}
