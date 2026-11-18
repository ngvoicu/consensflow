// Ends the stand-in that an installer test started.
export function endOwned(child) {
  child?.kill()
}
