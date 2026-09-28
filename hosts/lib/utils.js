/** A name as the roster spells it: lowercase, dashes, never empty. */
export function slugify(value) {
  const slug = String(value ?? '')
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .replace(/-{2,}/g, '-')
  return slug || 'agent'
}

/** `@zeus` and `zeus` name the same agent. */
export function stripMention(value) {
  return String(value ?? '').replace(/^@+/, '')
}
