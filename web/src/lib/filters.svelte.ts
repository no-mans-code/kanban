import { isOverdue } from './format'
import type { TicketSummary } from './types'

/** A named snapshot of a filter combination. Per-user, not shared - lives
 * only in this browser's localStorage, not the server. */
export interface SavedFilter {
  id: string
  name: string
  projectKey: string
  text: string
  assignees: (number | 'none')[]
  type: string
  priority: string
  label: number | null
  overdue: boolean
}

const SAVED_KEY = 'kanban.savedFilters'

function loadSaved(): SavedFilter[] {
  try {
    const raw = localStorage.getItem(SAVED_KEY)
    return raw ? JSON.parse(raw) : []
  } catch {
    return []
  }
}

function persistSaved(list: SavedFilter[]) {
  try {
    localStorage.setItem(SAVED_KEY, JSON.stringify(list))
  } catch {
    // Private mode or blocked storage: it just won't persist.
  }
}

class Filters {
  text = $state('')
  /** '' any, 'none' unassigned, otherwise user ids */
  assignees = $state<(number | 'none')[]>([])
  type = $state('')
  priority = $state('')
  label = $state<number | null>(null)
  overdue = $state(false)
  /** Keys the server's full-text search matched (descriptions, comments). */
  serverHits = $state<Set<string> | null>(null)
  saved = $state<SavedFilter[]>(loadSaved())

  active = $derived(
    this.text.trim() !== '' ||
      this.assignees.length > 0 ||
      this.type !== '' ||
      this.priority !== '' ||
      this.label !== null ||
      this.overdue,
  )

  matches(t: TicketSummary): boolean {
    if (this.type && t.type !== this.type) return false
    if (this.priority && t.priority !== this.priority) return false
    if (this.label !== null && !t.label_ids.includes(this.label)) return false
    if (this.overdue && !isOverdue(t.due_date, t.status_category)) return false
    if (this.assignees.length > 0) {
      const who = t.assignee_id ?? 'none'
      if (!this.assignees.includes(who)) return false
    }
    const q = this.text.trim().toLowerCase()
    if (q) {
      const local = t.title.toLowerCase().includes(q) || t.key.toLowerCase().includes(q)
      if (!local && !this.serverHits?.has(t.key)) return false
    }
    return true
  }

  toggleAssignee(id: number | 'none') {
    this.assignees = this.assignees.includes(id)
      ? this.assignees.filter((a) => a !== id)
      : [...this.assignees, id]
  }

  clear() {
    this.text = ''
    this.assignees = []
    this.type = ''
    this.priority = ''
    this.label = null
    this.overdue = false
    this.serverHits = null
  }

  save(name: string, projectKey: string) {
    const entry: SavedFilter = {
      id: crypto.randomUUID(),
      name,
      projectKey,
      text: this.text,
      assignees: [...this.assignees],
      type: this.type,
      priority: this.priority,
      label: this.label,
      overdue: this.overdue,
    }
    this.saved = [...this.saved, entry]
    persistSaved(this.saved)
  }

  apply(f: SavedFilter) {
    this.text = f.text
    this.assignees = [...f.assignees]
    this.type = f.type
    this.priority = f.priority
    this.label = f.label
    this.overdue = f.overdue
    this.serverHits = null
  }

  removeSaved(id: string) {
    this.saved = this.saved.filter((f) => f.id !== id)
    persistSaved(this.saved)
  }
}

export const filters = new Filters()
