export type TicketType = 'epic' | 'story' | 'task' | 'bug' | 'subtask'
export type Priority = 'highest' | 'high' | 'medium' | 'low' | 'lowest'
export type Category = 'todo' | 'in_progress' | 'done'
export type LinkKind = 'blocks' | 'relates' | 'duplicates' | 'clones'

export const TYPES: TicketType[] = ['epic', 'story', 'task', 'bug', 'subtask']
export const PRIORITIES: Priority[] = ['highest', 'high', 'medium', 'low', 'lowest']
export const CATEGORIES: Category[] = ['todo', 'in_progress', 'done']

export const CATEGORY_NAMES: Record<Category, string> = {
  todo: 'To do',
  in_progress: 'In progress',
  done: 'Done',
}

/** Link choices as shown to people: a kind plus which side this ticket is on. */
export const LINK_CHOICES: { label: string; kind: LinkKind; outward: boolean }[] = [
  { label: 'blocks', kind: 'blocks', outward: true },
  { label: 'is blocked by', kind: 'blocks', outward: false },
  { label: 'relates to', kind: 'relates', outward: true },
  { label: 'duplicates', kind: 'duplicates', outward: true },
  { label: 'is duplicated by', kind: 'duplicates', outward: false },
  { label: 'clones', kind: 'clones', outward: true },
  { label: 'is cloned by', kind: 'clones', outward: false },
]

/** Which child type a parent of this type takes (Jira's hierarchy). */
export function childTypeFor(parent: TicketType): TicketType | null {
  if (parent === 'epic') return 'task'
  if (parent === 'subtask') return null
  return 'subtask'
}

export type UserKind = 'human' | 'agent'
export type Role = 'viewer' | 'member' | 'admin'

export interface User {
  id: number
  username: string
  display_name: string
  color: string
  active: boolean
  created_at: number
  kind: UserKind
  is_admin: boolean
  has_password: boolean
}

/** The signed-in caller, as returned by /api/auth/status. */
export interface Me {
  id: number
  username: string
  display_name: string
  color: string
  kind: UserKind
  is_admin: boolean
}

export interface ProjectMember {
  user_id: number
  username: string
  display_name: string
  color: string
  kind: UserKind
  role: Role
}

export interface ApiToken {
  id: number
  user_id: number
  username: string
  name: string
  hint: string
  read_only: boolean
  all_projects: boolean
  projects: string[] | null
  created_at: number
  expires_at: number | null
  last_used_at: number | null
  revoked_at: number | null
}

export interface Project {
  id: number
  key: string
  name: string
  description: string
  created_at: number
}

/** A project as listed for the signed-in caller: only ones they can see, each with their role in it. */
export interface ProjectItem extends Project {
  role: Role
}

export interface Status {
  id: number
  project_id: number
  name: string
  category: Category
  position: number
  wip_limit: number | null
}

export interface ProjectDetail extends Project {
  statuses: Status[]
  role: Role
}

export interface Label {
  id: number
  name: string
  color: string
}

export interface TicketSummary {
  id: number
  key: string
  project_id: number
  number: number
  type: TicketType
  title: string
  status_id: number
  status_name: string
  status_category: Category
  priority: Priority
  assignee_id: number | null
  reporter_id: number | null
  parent_id: number | null
  parent_key: string | null
  rank: number
  created_at: number
  updated_at: number
  resolved_at: number | null
  due_date: number | null
  label_ids: number[]
  child_count: number
  done_child_count: number
  is_blocked: boolean
  comment_count: number
}

export interface LinkView {
  kind: LinkKind
  direction: 'outward' | 'inward'
  label: string
  ticket: TicketSummary
}

export interface TicketDetail extends TicketSummary {
  description: string
  watcher_ids: number[]
  children: TicketSummary[]
  links: LinkView[]
}

export interface Comment {
  id: number
  ticket_id: number
  author_id: number | null
  body: string
  created_at: number
  updated_at: number
}

export interface Activity {
  id: number
  ticket_id: number
  actor_id: number | null
  action: string
  field: string | null
  old_value: string | null
  new_value: string | null
  created_at: number
}

export interface GraphData {
  nodes: TicketSummary[]
  edges: { source: number; target: number }[]
  hierarchy: { parent: number; child: number }[]
  longest_chain: number[]
  max_height: number | null
}

export interface Settings {
  max_dag_height: number | null
  dag_height: number
  longest_chain: string[]
  is_site_admin: boolean
}

export type ChildrenChoice = 'delete' | 'detach' | 'move' | 'promote'
export type DependentsChoice = 'drop' | 'bridge' | 'children' | 'transfer'

export interface DeletePlan {
  ticket: TicketSummary
  parent: TicketSummary | null
  children: TicketSummary[]
  grandchildren: TicketSummary[]
  blockers: TicketSummary[]
  dependents: (TicketSummary & { becomes_ready_if_dropped: boolean })[]
  children_dependents: TicketSummary[]
  other_links: { label: string; ticket: TicketSummary }[]
  children_options: ChildrenChoice[]
  dependents_options: DependentsChoice[]
}

export interface DeleteDecisions {
  children?: ChildrenChoice
  dependents?: DependentsChoice
  move_to?: string
  transfer_to?: string
}

export interface ChangeEvent {
  type: string
  project_id: number | null
  keys: string[]
}
