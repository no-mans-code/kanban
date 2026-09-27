import { api, setActor } from './api'
import type { ChangeEvent, Label, Project, ProjectDetail, User } from './types'

const COALESCE_MS = 150
const THEME_KEY = 'kanban.theme'
const ACTOR_KEY = 'kanban.actor'

function load(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function save(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // Private mode or blocked storage: the preference just won't persist.
  }
}

class AppStore {
  users = $state<User[]>([])
  labels = $state<Label[]>([])
  projects = $state<Project[]>([])
  /** The project currently open, with its workflow statuses. */
  project = $state<ProjectDetail | null>(null)
  actorId = $state<number | null>(null)
  theme = $state<'dark' | 'light'>(document.documentElement.dataset.theme === 'light' ? 'light' : 'dark')
  connected = $state(false)
  ready = $state(false)

  /**
   * Bumped at most once per COALESCE_MS for ticket, comment and link changes;
   * views refetch on it. `changedKeys` lists the tickets touched in that
   * batch, or is null when anything may have changed (resync, label or
   * workflow edits).
   */
  ticketsVersion = $state(0)
  changedKeys = $state<ReadonlySet<string> | null>(null)
  /** Bumped when settings or the dependency graph may have changed. */
  settingsVersion = $state(0)

  private pendingKeys = new Set<string>()
  private pendingAll = false
  private pendingTickets = false
  private pendingSettings = false
  private flushTimer: ReturnType<typeof setTimeout> | undefined

  usersById = $derived(new Map(this.users.map((u) => [u.id, u])))
  labelsById = $derived(new Map(this.labels.map((l) => [l.id, l])))
  activeUsers = $derived(this.users.filter((u) => u.active))
  actor = $derived(this.actorId === null ? null : (this.usersById.get(this.actorId) ?? null))

  async init() {
    await Promise.all([this.loadUsers(), this.loadLabels(), this.loadProjects()])
    const saved = Number(load(ACTOR_KEY))
    const pick = this.users.find((u) => u.id === saved && u.active) ?? this.activeUsers[0] ?? null
    this.setActor(pick?.id ?? null)
    this.connect()
    this.ready = true
  }

  async loadUsers() {
    this.users = await api.users()
  }

  async loadLabels() {
    this.labels = await api.labels()
  }

  async loadProjects() {
    this.projects = await api.projects()
  }

  async openProject(key: string) {
    if (this.project?.key === key) return
    this.project = await api.project(key)
  }

  async reloadProject() {
    if (this.project) this.project = await api.project(this.project.key)
  }

  setActor(id: number | null) {
    this.actorId = id
    setActor(id)
    if (id !== null) save(ACTOR_KEY, String(id))
  }

  toggleTheme() {
    this.theme = this.theme === 'dark' ? 'light' : 'dark'
    document.documentElement.dataset.theme = this.theme
    save(THEME_KEY, this.theme)
  }

  private connect() {
    const source = new EventSource('/api/events')
    source.onopen = () => {
      // After a reconnect we may have missed changes.
      if (!this.connected && this.ready) this.resync()
      this.connected = true
    }
    source.onerror = () => {
      this.connected = false
    }
    source.addEventListener('resync', () => this.resync())
    source.addEventListener('change', (e) => {
      const change = JSON.parse((e as MessageEvent).data) as ChangeEvent
      this.apply(change)
    })
  }

  private apply(change: ChangeEvent) {
    const [scope] = change.type.split('.')
    if (scope === 'user') this.loadUsers()
    else if (scope === 'label') {
      this.loadLabels()
      this.queue({ all: true })
    } else if (scope === 'project') {
      this.loadProjects()
      if (change.project_id === null || change.project_id === this.project?.id) {
        this.reloadProject()
        this.queue({ all: true })
      }
    } else if (scope === 'settings') this.queue({ settings: true })
    else {
      // Links and deletions can change the dependency graph's height.
      const graph = scope === 'link' || change.type === 'ticket.deleted'
      this.queue({ keys: change.keys, settings: graph })
    }
  }

  /**
   * Agents can change many tickets in a burst. Collect everything that arrives
   * within a short window and let views refetch once for the whole batch.
   */
  private queue(what: { keys?: string[]; all?: boolean; settings?: boolean }) {
    if (what.keys) {
      this.pendingTickets = true
      for (const k of what.keys) this.pendingKeys.add(k)
    }
    if (what.all) {
      this.pendingTickets = true
      this.pendingAll = true
    }
    if (what.settings) this.pendingSettings = true
    this.flushTimer ??= setTimeout(() => this.flush(), COALESCE_MS)
  }

  private flush() {
    this.flushTimer = undefined
    if (this.pendingTickets) {
      this.changedKeys = this.pendingAll ? null : new Set(this.pendingKeys)
      this.ticketsVersion++
    }
    if (this.pendingSettings) this.settingsVersion++
    this.pendingKeys.clear()
    this.pendingAll = this.pendingTickets = this.pendingSettings = false
  }

  private resync() {
    this.loadUsers()
    this.loadLabels()
    this.loadProjects()
    this.reloadProject()
    this.queue({ all: true, settings: true })
  }
}

export const app = new AppStore()
