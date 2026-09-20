/**
 * The name of a member's session: two plain words after the member's own
 * handle, `diana-amber-pine`, easy to say aloud and to tell apart on the
 * board. Thirty-two of each gives a thousand names per member.
 */
export const ADJECTIVES = [
  'amber',
  'brisk',
  'calm',
  'coral',
  'crisp',
  'dusky',
  'eager',
  'frosty',
  'gentle',
  'golden',
  'hazel',
  'ivory',
  'jolly',
  'keen',
  'lively',
  'lunar',
  'mellow',
  'misty',
  'noble',
  'olive',
  'pale',
  'quiet',
  'rosy',
  'rusty',
  'sandy',
  'silver',
  'sunny',
  'tidy',
  'velvet',
  'vivid',
  'windy',
  'zesty',
]
export const NOUNS = [
  'anchor',
  'birch',
  'brook',
  'canyon',
  'cedar',
  'cliff',
  'comet',
  'delta',
  'dune',
  'ember',
  'fjord',
  'glade',
  'harbor',
  'island',
  'juniper',
  'kestrel',
  'lagoon',
  'meadow',
  'oriole',
  'pebble',
  'pine',
  'quarry',
  'reef',
  'ridge',
  'saddle',
  'summit',
  'thistle',
  'tundra',
  'valley',
  'willow',
  'window',
  'yarrow',
]

/** A fresh `adjective-noun`; `random` is injected so tests can pick. */
export function sessionName(random = Math.random) {
  const pick = (words) => words[Math.min(words.length - 1, Math.floor(random() * words.length))]
  return `${pick(ADJECTIVES)}-${pick(NOUNS)}`
}
