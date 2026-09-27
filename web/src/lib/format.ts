const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })
const full = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

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
