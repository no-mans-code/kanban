import type {
  Activity,
  Comment,
  DeleteDecisions,
  DeletePlan,
  GraphData,
  Label,
  LinkKind,
  Me,
  ProjectDetail,
  ProjectItem,
  ProjectMember,
  Settings,
  Status,
  TicketDetail,
  TicketSummary,
  ApiToken,
  User,
} from './types'

export class ApiError extends Error {
  status: number
  code: string
  detail: unknown

  constructor(status: number, code: string, message: string, detail?: unknown) {
    super(message)
    this.status = status
    this.code = code
    this.detail = detail
  }
}

/** Called whenever any request comes back 401: the session/token is gone. */
let unauthorizedHandler: (() => void) | null = null
export function onUnauthorized(handler: () => void) {
  unauthorizedHandler = handler
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  return (await send(method, path, body)).data as T
}

async function send(method: string, path: string, body?: unknown): Promise<{ data: unknown; res: Response }> {
  const headers: Record<string, string> = {}
  if (body !== undefined) headers['content-type'] = 'application/json'
  let res: Response
  try {
    // 'same-origin' (the default) sends the session cookie automatically;
    // no per-request identity header is needed, unlike the old X-Actor.
    res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) })
  } catch {
    throw new ApiError(0, 'offline', 'Cannot reach the board server')
  }
  if (res.status === 401) unauthorizedHandler?.()
  if (res.status === 204) return { data: undefined, res }
  const data = await res.json().catch(() => null)
  if (!res.ok) {
    const err = data?.error
    throw new ApiError(res.status, err?.code ?? 'http', err?.message ?? res.statusText, err?.detail)
  }
  return { data, res }
}

const PAGE = 5000

/** Every ticket matching `q`, following pages until X-Total-Count is reached. */
async function allTickets(q: TicketQuery): Promise<TicketSummary[]> {
  const out: TicketSummary[] = []
  for (;;) {
    const { data, res } = await send('GET', `/api/tickets${query({ ...q, limit: PAGE, offset: out.length })}`)
    const page = data as TicketSummary[]
    out.push(...page)
    const total = Number(res.headers.get('x-total-count') ?? out.length)
    if (page.length === 0 || out.length >= total) return out
  }
}

const enc = encodeURIComponent

function query(params: Record<string, string | number | boolean | null | undefined>): string {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
  }
  const s = q.toString()
  return s ? `?${s}` : ''
}

export interface TicketQuery {
  project?: string
  status_id?: number
  assignee?: number | 'none'
  type?: string
  priority?: string
  label_id?: number
  parent?: string
  watcher?: number
  q?: string
  overdue?: boolean
  sort?: 'rank' | 'updated' | 'created' | 'priority' | 'due' | 'key'
  limit?: number
  offset?: number
}

export interface TicketInput {
  project: string
  type: string
  title: string
  description?: string
  priority?: string
  status_id?: number
  assignee_id?: number | null
  parent?: string | null
  label_ids?: number[]
  due_date?: number | null
}

export type TicketPatch = Partial<{
  title: string
  description: string
  type: string
  priority: string
  status_id: number
  assignee_id: number | null
  parent: string | null
  label_ids: number[]
  due_date: number | null
}>

export interface CreateTokenInput {
  name: string
  user_id?: number
  /** Project keys to narrow to; omit for every project the owner can reach. */
  projects?: string[]
  read_only?: boolean
  /** Days until expiry; omit for 90, null for never. */
  expires_in_days?: number | null
}

export const api = {
  auth: {
    status: () => request<{ setup_required: boolean; user: Me | null; via: 'token' | 'session' | null }>('GET', '/api/auth/status'),
    setup: (body: { code: string; username: string; display_name?: string; password: string }) =>
      request<unknown>('POST', '/api/auth/setup', body),
    login: (username: string, password: string) => request<unknown>('POST', '/api/auth/login', { username, password }),
    logout: () => request<void>('POST', '/api/auth/logout'),
    changePassword: (current: string, next: string) => request<void>('POST', '/api/auth/password', { current, new: next }),
  },

  users: () => request<User[]>('GET', '/api/users'),
  createUser: (body: { username: string; display_name?: string; color?: string; kind?: 'human' | 'agent'; password?: string; is_admin?: boolean }) =>
    request<User>('POST', '/api/users', body),
  updateUser: (id: number, body: Partial<{ display_name: string; color: string; active: boolean; is_admin: boolean; password: string }>) =>
    request<User>('PATCH', `/api/users/${id}`, body),

  tokens: (userId?: number) => request<ApiToken[]>('GET', `/api/tokens${query({ user_id: userId })}`),
  createToken: (body: CreateTokenInput) => request<ApiToken & { token: string }>('POST', '/api/tokens', body),
  revokeToken: (id: number) => request<void>('DELETE', `/api/tokens/${id}`),

  projects: () => request<ProjectItem[]>('GET', '/api/projects'),
  project: (key: string) => request<ProjectDetail>('GET', `/api/projects/${enc(key)}`),
  createProject: (body: { key: string; name: string; description?: string }) => request<ProjectDetail>('POST', '/api/projects', body),
  updateProject: (key: string, body: Partial<{ name: string; description: string }>) => request<ProjectDetail>('PATCH', `/api/projects/${enc(key)}`, body),

  members: (key: string) => request<ProjectMember[]>('GET', `/api/projects/${enc(key)}/members`),
  setMember: (key: string, userId: number, role: 'viewer' | 'member' | 'admin') =>
    request<ProjectMember[]>('PUT', `/api/projects/${enc(key)}/members/${userId}`, { role }),
  removeMember: (key: string, userId: number) => request<ProjectMember[]>('DELETE', `/api/projects/${enc(key)}/members/${userId}`),

  createStatus: (key: string, body: { name: string; category: string }) => request<Status[]>('POST', `/api/projects/${enc(key)}/statuses`, body),
  updateStatus: (id: number, body: Partial<{ name: string; category: string; wip_limit: number | null }>) =>
    request<Status[]>('PATCH', `/api/statuses/${id}`, body),
  deleteStatus: (id: number, moveTo?: number) => request<Status[]>('DELETE', `/api/statuses/${id}${query({ move_to: moveTo })}`),
  reorderStatuses: (key: string, ids: number[]) => request<Status[]>('PUT', `/api/projects/${enc(key)}/statuses/order`, { ids }),

  labels: () => request<Label[]>('GET', '/api/labels'),
  createLabel: (body: { name: string; color?: string }) => request<Label>('POST', '/api/labels', body),
  updateLabel: (id: number, body: Partial<{ name: string; color: string }>) => request<Label>('PATCH', `/api/labels/${id}`, body),
  deleteLabel: (id: number) => request<void>('DELETE', `/api/labels/${id}`),

  /** One page (default 2000). For views that must show everything, use allTickets. */
  tickets: (q: TicketQuery) => request<TicketSummary[]>('GET', `/api/tickets${query({ ...q })}`),
  allTickets,
  ticket: (key: string) => request<TicketDetail>('GET', `/api/tickets/${enc(key)}`),
  createTicket: (body: TicketInput) => request<TicketDetail>('POST', '/api/tickets', body),
  updateTicket: (key: string, body: TicketPatch) => request<TicketDetail>('PATCH', `/api/tickets/${enc(key)}`, body),
  moveTicket: (key: string, status_id: number, after: string | null) => request<TicketSummary>('POST', `/api/tickets/${enc(key)}/move`, { status_id, after }),
  deletePlan: (key: string) => request<DeletePlan>('GET', `/api/tickets/${enc(key)}/delete-plan`),
  deleteTicket: (key: string, decisions: DeleteDecisions = {}) => request<void>('DELETE', `/api/tickets/${enc(key)}${query({ ...decisions })}`),
  activity: (key: string) => request<Activity[]>('GET', `/api/tickets/${enc(key)}/activity`),
  addWatcher: (key: string, user_id: number) => request<number[]>('POST', `/api/tickets/${enc(key)}/watchers`, { user_id }),
  removeWatcher: (key: string, user_id: number) => request<number[]>('DELETE', `/api/tickets/${enc(key)}/watchers/${user_id}`),

  comments: (key: string) => request<Comment[]>('GET', `/api/tickets/${enc(key)}/comments`),
  createComment: (key: string, body: string) => request<Comment>('POST', `/api/tickets/${enc(key)}/comments`, { body }),
  updateComment: (id: number, body: string) => request<Comment>('PATCH', `/api/comments/${id}`, { body }),
  deleteComment: (id: number) => request<void>('DELETE', `/api/comments/${id}`),

  createLink: (source: string, target: string, kind: LinkKind) => request<void>('POST', '/api/links', { source, target, kind }),
  deleteLink: (source: string, target: string, kind: LinkKind) => request<void>('DELETE', `/api/links${query({ source, target, kind })}`),

  graph: (project: string, all: boolean) => request<GraphData>('GET', `/api/graph${query({ project, all })}`),
  settings: () => request<Settings>('GET', '/api/settings'),
  updateSettings: (body: { max_dag_height: number | null }) => request<Settings>('PATCH', '/api/settings', body),
}
