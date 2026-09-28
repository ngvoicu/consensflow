/**
 * The question trip with the members' own question tools: each member asks
 * the chief with its harness's ask-the-user tool instead of `cf ask`. Whether
 * that reaches the chief, and the answer the member, per harness, is what it
 * measures.
 */
import { questionTrip } from './question-trip.mjs'

export default questionTrip({
  id: 'question-trip-native',
  title:
    "A worker, an advisor and a reviewer each ask the chief with their harness's own question tool",
  how: 'cu unealta proprie de întrebări a harness-ului lor (nu cu cf ask)',
})
