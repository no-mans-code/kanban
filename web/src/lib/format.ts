const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })
const full = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })
const short = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' })

/** "Sep 27", for a compact due-date chip. */
export function shortDate(ms: number): string {
  return short.format(ms)
}

export function relativeTime(ms: number): string {
  const diff = (ms - Date.now()) / 1000
  const abs = Math.abs(diff)
  if (abs < 45) return 'just now'
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour')
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), 'day')
  return full.format(ms)
}

export function fullTime(ms: number): string {
  return full.format(ms)
}

/** Epoch ms (UTC midnight, as due dates are stored) -> `<input type=date>` value. */
export function dateInputValue(ms: number | null): string {
  if (ms === null) return ''
  return new Date(ms).toISOString().slice(0, 10)
}

/** `<input type=date>` value -> epoch ms at UTC midnight, or null if empty. */
export function dateInputToMs(value: string): number | null {
  if (!value) return null
  return Date.parse(`${value}T00:00:00Z`)
}

/** Whether a due date has passed and the ticket isn't in a done column. */
export function isOverdue(dueDate: number | null, statusCategory: string): boolean {
  return dueDate !== null && dueDate < Date.now() && statusCategory !== 'done'
}

/** "Ada Lovelace" -> "AL", "God (Coder)" -> "GC", "me" -> "ME". */
export function initials(name: string): string {
  const parts = name.split(/[^\p{L}\p{N}]+/u).filter(Boolean)
  if (parts.length === 0) return '?'
  const letters = parts.length > 1 ? parts[0][0] + parts[parts.length - 1][0] : parts[0].slice(0, 2)
  return letters.toUpperCase()
}

export function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1).replace('_', ' ')
}

/** Debounce for search boxes. */
export function debounce<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let t: ReturnType<typeof setTimeout> | undefined
  return (...args: A) => {
    clearTimeout(t)
    t = setTimeout(() => fn(...args), ms)
  }
}
