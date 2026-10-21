/** The tags a text holds, each without its `#` and in lower case: "#travel #client:ACME". */
export function parseTags(text) {
  return String(text ?? '')
    .split(/\s+/)
    .filter((word) => word.startsWith('#') && word.length > 1)
    .map((word) => word.slice(1).toLowerCase())
}

/** Whether the entry has the tag `tag`, whatever its case. */
export function hasTag(entry, tag) {
  return entry.tags.includes(tag.toLowerCase())
}
