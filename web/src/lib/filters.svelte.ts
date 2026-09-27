import { isOverdue } from './format'
import type { TicketSummary } from './types'

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
}

export const filters = new Filters()
