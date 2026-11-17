/** Whether a name adds a word that the model's id does not already say. */
export function hasDistinctModelName(id, name) {
  const words = (text) =>
    text
      .toLowerCase()
      .split(/[^\p{L}\p{N}]+/u)
      .filter(Boolean)
  const idWords = words(id)
  const joined = idWords.join('')
  return words(name).some(
    (word) => !idWords.includes(word) && !([...word].length > 1 && joined.includes(word)),
  )
}

/** A harness's model row: its id, a distinct name, provider and its own default mark. */
export function modelOptionText(model) {
  const name = model.label && hasDistinctModelName(model.id, model.label) ? ` (${model.label})` : ''
  const provider = model.provider ? ` [${model.provider}]` : ''
  const defaultMark = model.markedDefault ? ' · default' : ''
  return `${model.id}${name}${provider}${defaultMark}`
}
