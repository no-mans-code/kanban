import { api, setActor } from './api'
import type { ChangeEvent, Label, Project, ProjectDetail, User } from './types'

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

  /** Bumped on every ticket/comment/link change anywhere; views refetch on it. */
  ticketsVersion = $state(0)
  settingsVersion = $state(0)
  lastChange = $state<ChangeEvent | null>(null)

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
      this.ticketsVersion++
    } else if (scope === 'project') {
      this.loadProjects()
      if (change.project_id === null || change.project_id === this.project?.id) {
        this.reloadProject()
        this.ticketsVersion++
      }
    } else if (scope === 'settings') this.settingsVersion++
    else {
      this.lastChange = change
      this.ticketsVersion++
    }
  }

  private resync() {
    this.loadUsers()
    this.loadLabels()
    this.loadProjects()
    this.reloadProject()
    this.ticketsVersion++
    this.settingsVersion++
  }
}

export const app = new AppStore()
