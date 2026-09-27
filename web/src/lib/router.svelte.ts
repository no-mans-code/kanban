export type View = 'board' | 'list' | 'graph'

export type Route =
  | { name: 'home' }
  | { name: 'project'; project: string; view: View }
  | { name: 'settings'; tab: string }

function parse(path: string): Route {
  const parts = path.split('/').filter(Boolean)
  if (parts[0] === 'p' && parts[1]) {
    const view = (['board', 'list', 'graph'] as View[]).find((v) => v === parts[2]) ?? 'board'
    return { name: 'project', project: decodeURIComponent(parts[1]).toUpperCase(), view }
  }
  if (parts[0] === 'settings') return { name: 'settings', tab: parts[1] ?? 'general' }
  return { name: 'home' }
}

class Router {
  path = $state(location.pathname)
  search = $state(location.search)
  route = $derived(parse(this.path))
  /** The ticket shown in the side panel, from `?ticket=KEY`, on any page. */
  ticket = $derived(new URLSearchParams(this.search).get('ticket'))

  constructor() {
    addEventListener('popstate', () => this.sync())
  }

  private sync() {
    this.path = location.pathname
    this.search = location.search
  }

  navigate(url: string, replace = false) {
    const target = new URL(url, location.href)
    if (target.pathname + target.search === location.pathname + location.search) return
    history[replace ? 'replaceState' : 'pushState'](null, '', target.pathname + target.search)
    this.sync()
  }

  openTicket(key: string) {
    const q = new URLSearchParams(location.search)
    q.set('ticket', key)
    this.navigate(`${location.pathname}?${q}`)
  }

  closeTicket() {
    const q = new URLSearchParams(location.search)
    q.delete('ticket')
    const s = q.toString()
    this.navigate(location.pathname + (s ? `?${s}` : ''))
  }
}

export const router = new Router()

export function projectUrl(key: string, view: View = 'board') {
  return `/p/${encodeURIComponent(key)}/${view}`
}
