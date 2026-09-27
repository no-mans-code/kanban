<script lang="ts">
  import { api } from '../lib/api'
  import { filters } from '../lib/filters.svelte'
  import { isOverdue, relativeTime, shortDate } from '../lib/format'
  import { router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'
  import { PRIORITIES, TYPES, type ProjectDetail, type TicketSummary } from '../lib/types'
  import Avatar from './Avatar.svelte'
  import FilterBar from './FilterBar.svelte'
  import Icon from './Icon.svelte'
  import PriorityIcon from './PriorityIcon.svelte'
  import TypeIcon from './TypeIcon.svelte'

  let { project }: { project: ProjectDetail } = $props()

  type SortKey = 'key' | 'type' | 'title' | 'status' | 'assignee' | 'priority' | 'due' | 'updated'
  let tickets = $state<TicketSummary[]>([])
  let sortKey = $state<SortKey>('key')
  let desc = $state(false)

  $effect(() => {
    void app.ticketsVersion
    api.allTickets({ project: project.key, sort: 'key' }).then(
      (t) => (tickets = t),
      (e) => toasts.error(e),
    )
  })

  const statusPos = $derived(new Map(project.statuses.map((s) => [s.id, s.position])))

  function value(t: TicketSummary, k: SortKey): string | number {
    switch (k) {
      case 'key':
        return t.number
      case 'type':
        return TYPES.indexOf(t.type)
      case 'title':
        return t.title.toLowerCase()
      case 'status':
        return statusPos.get(t.status_id) ?? 0
      case 'assignee':
        return t.assignee_id === null ? '~' : (app.usersById.get(t.assignee_id)?.display_name.toLowerCase() ?? '')
      case 'priority':
        return PRIORITIES.indexOf(t.priority)
      case 'due':
        return t.due_date ?? Infinity
      case 'updated':
        return -t.updated_at
    }
  }

  const rows = $derived.by(() => {
    const out = tickets.filter((t) => filters.matches(t))
    out.sort((a, b) => {
      const x = value(a, sortKey)
      const y = value(b, sortKey)
      const c = x < y ? -1 : x > y ? 1 : a.number - b.number
      return desc ? -c : c
    })
    return out
  })

  function sortBy(k: SortKey) {
    if (sortKey === k) desc = !desc
    else {
      sortKey = k
      desc = false
    }
  }

  async function setStatus(t: TicketSummary, statusId: number) {
    const previous = t.status_id
    t.status_id = statusId
    try {
      const saved = await api.updateTicket(t.key, { status_id: statusId })
      Object.assign(t, saved)
    } catch (e) {
      t.status_id = previous
      toasts.error(e)
    }
  }

  const columns: [SortKey, string][] = [
    ['type', ''],
    ['key', 'Key'],
    ['title', 'Title'],
    ['status', 'Status'],
    ['assignee', 'Assignee'],
    ['priority', 'Priority'],
    ['due', 'Due'],
    ['updated', 'Updated'],
  ]
</script>

<FilterBar project={project.key} count={rows.length} total={tickets.length} />

<div class="wrap">
  <table>
    <thead>
      <tr>
        {#each columns as [k, label] (k)}
          <th class={k}>
            <button onclick={() => sortBy(k)} class:active={sortKey === k}>
              {label}
              {#if sortKey === k}<Icon name={desc ? 'down' : 'up'} size={12} />{/if}
            </button>
          </th>
        {/each}
        <th class="labels">Labels</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as t (t.key)}
        <tr onclick={() => router.openTicket(t.key)} class:selected={router.ticket === t.key}>
          <td class="type"><TypeIcon type={t.type} /></td>
          <td class="key">{t.key}</td>
          <td class="title">
            <span class="title-text">{t.title}</span>
            {#if t.is_blocked}<span class="chip blocked"><Icon name="lock" size={11} />Blocked</span>{/if}
          </td>
          <td class="status" onclick={(e) => e.stopPropagation()}>
            <select
              class="status-select cat-{t.status_category}"
              value={t.status_id}
              onchange={(e) => setStatus(t, Number(e.currentTarget.value))}
              aria-label="Status of {t.key}"
            >
              {#each project.statuses as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
            </select>
          </td>
          <td class="assignee">
            <span class="who">
              <Avatar userId={t.assignee_id} size={20} />
              {t.assignee_id === null ? 'Unassigned' : app.usersById.get(t.assignee_id)?.display_name}
            </span>
          </td>
          <td class="priority"><span class="who"><PriorityIcon priority={t.priority} />{t.priority}</span></td>
          <td class="due" class:overdue={isOverdue(t.due_date, t.status_category)}>
            {t.due_date === null ? '' : shortDate(t.due_date)}
          </td>
          <td class="updated muted">{relativeTime(t.updated_at)}</td>
          <td>
            <div class="label-list">
              {#each t.label_ids as id (id)}
                {@const l = app.labelsById.get(id)}
                {#if l}<span class="chip"><span class="dot" style:background={l.color}></span>{l.name}</span>{/if}
              {/each}
            </div>
          </td>
        </tr>
      {:else}
        <tr class="none"><td colspan="8">No tickets match.</td></tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 20px 20px;
  }
  table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    font-size: 13px;
  }
  th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--bg);
    text-align: left;
    border-bottom: 1px solid var(--border);
    padding: 0;
  }
  th button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    background: none;
    color: var(--text-3);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    text-align: left;
  }
  th button:hover,
  th button.active {
    color: var(--text);
  }
  th.labels {
    padding: 8px 10px;
    color: var(--text-3);
    font-size: 12px;
  }
  td {
    padding: 7px 10px;
    border-bottom: 1px solid var(--border);
    vertical-align: middle;
    white-space: nowrap;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover td {
    background: var(--surface);
  }
  tr.selected td {
    background: var(--accent-soft);
  }
  .type {
    width: 32px;
  }
  td.type {
    line-height: 0;
  }
  td.key {
    font: 500 12px var(--mono);
    color: var(--text-3);
    width: 90px;
  }
  td.title {
    white-space: normal;
    min-width: 260px;
  }
  .title-text {
    margin-right: 6px;
    font-weight: 500;
  }
  .blocked {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .who {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    text-transform: none;
  }
  td.priority .who {
    text-transform: capitalize;
  }
  td.due {
    color: var(--text-2);
    white-space: nowrap;
  }
  td.due.overdue {
    color: var(--danger);
    font-weight: 500;
  }
  .status-select {
    height: 24px;
    padding: 0 8px;
    border-radius: 12px;
    border: 1px solid color-mix(in srgb, var(--cat) 45%, transparent);
    background: color-mix(in srgb, var(--cat) 14%, transparent);
    color: var(--text);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }
  .status-select option {
    background: var(--surface);
  }
  .label-list {
    display: flex;
    gap: 4px;
  }
  .none td {
    text-align: center;
    color: var(--text-3);
    padding: 40px;
    cursor: default;
  }
</style>
