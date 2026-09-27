<script lang="ts">
  import { untrack } from 'svelte'
  import { api } from '../lib/api'
  import { filters } from '../lib/filters.svelte'
  import { router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'
  import type { ProjectDetail, Status, TicketSummary } from '../lib/types'
  import FilterBar from './FilterBar.svelte'
  import Icon from './Icon.svelte'
  import TicketCard from './TicketCard.svelte'

  let { project }: { project: ProjectDetail } = $props()

  let tickets = $state<TicketSummary[]>([])
  let loaded = $state(false)
  let dragKey = $state<string | null>(null)
  let drop = $state<{ statusId: number; index: number } | null>(null)
  let adding = $state<number | null>(null)
  let newTitle = $state('')

  const visible = $derived(tickets.filter((t) => filters.matches(t)))
  const columns = $derived(
    project.statuses.map((status) => ({
      status,
      items: visible.filter((t) => t.status_id === status.id).sort((a, b) => a.rank - b.rank),
      total: tickets.filter((t) => t.status_id === status.id).length,
    })),
  )

  async function load(key: string) {
    try {
      tickets = await api.allTickets({ project: key, sort: 'rank' })
      loaded = true
    } catch (e) {
      toasts.error(e)
    }
  }

  // Initial load and refetch on any change pushed by the server. A refetch is
  // held back while a card is mid-drag so the column doesn't shift under it.
  let pending = false
  $effect(() => {
    void app.ticketsVersion
    const key = project.key
    if (untrack(() => dragKey)) {
      pending = true
      return
    }
    load(key)
  })

  /** Position of a card among the column's cards, not counting the dragged one. */
  function slotOf(items: TicketSummary[], key: string): number {
    let n = 0
    for (const t of items) {
      if (t.key === key) return n
      if (t.key !== dragKey) n++
    }
    return n
  }

  function onDragStart(e: DragEvent, t: TicketSummary) {
    dragKey = t.key
    e.dataTransfer?.setData('text/plain', t.key)
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
  }

  function endDrag() {
    dragKey = null
    drop = null
    if (pending) {
      pending = false
      load(project.key)
    }
  }

  function onDragOver(e: DragEvent, status: Status, list: HTMLElement) {
    if (!dragKey) return
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
    const cards = [...list.querySelectorAll<HTMLElement>('[data-key]')].filter((c) => c.dataset.key !== dragKey)
    let index = cards.length
    for (let i = 0; i < cards.length; i++) {
      const r = cards[i].getBoundingClientRect()
      if (e.clientY < r.top + r.height / 2) {
        index = i
        break
      }
    }
    if (drop?.statusId !== status.id || drop.index !== index) drop = { statusId: status.id, index }
  }

  async function onDrop(e: DragEvent, status: Status) {
    e.preventDefault()
    const key = dragKey
    const target = drop
    if (!key || !target) return endDrag()
    const moving = tickets.find((t) => t.key === key)
    if (!moving) return endDrag()

    const column = columns.find((c) => c.status.id === status.id)!.items.filter((t) => t.key !== key)
    const before = column[target.index - 1] ?? null
    const after = column[target.index] ?? null
    const unchanged = moving.status_id === status.id && (before?.key ?? null) === prevKey(moving, column)
    if (unchanged) return endDrag()

    // Optimistic: place the card now, reconcile with the server's answer.
    const snapshot = { ...moving }
    const lo = before?.rank ?? (after ? after.rank - 1024 : 0)
    const hi = after?.rank ?? lo + 2048
    Object.assign(moving, {
      status_id: status.id,
      status_name: status.name,
      status_category: status.category,
      rank: (lo + hi) / 2,
    })
    endDrag()
    try {
      const saved = await api.moveTicket(key, status.id, before?.key ?? null)
      const i = tickets.findIndex((t) => t.key === key)
      if (i >= 0) tickets[i] = saved
    } catch (err) {
      Object.assign(moving, snapshot)
      toasts.error(err)
    }
  }

  /** Key of the ticket directly above `t` in its current column, if any. */
  function prevKey(t: TicketSummary, column: TicketSummary[]): string | null {
    const above = column.filter((c) => c.rank < t.rank)
    return above.length ? above[above.length - 1].key : null
  }

  async function quickAdd(status: Status) {
    const title = newTitle.trim()
    if (!title) return
    newTitle = ''
    try {
      const t = await api.createTicket({ project: project.key, type: 'task', title, status_id: status.id })
      if (!tickets.some((x) => x.key === t.key)) tickets.push(t)
    } catch (e) {
      newTitle = title
      toasts.error(e)
    }
  }
</script>

<FilterBar project={project.key} count={visible.length} total={tickets.length} />

<div class="board">
  {#each columns as { status, items, total } (status.id)}
    <section
      class="column cat-{status.category}"
      class:target={drop?.statusId === status.id}
      aria-label={status.name}
    >
      <header>
        <span class="dot"></span>
        <h2>{status.name}</h2>
        <span class="count">{items.length === total ? total : `${items.length}/${total}`}</span>
        <span class="spacer"></span>
        <button class="icon-btn" title="Add a ticket to {status.name}" onclick={() => (adding = status.id)}>
          <Icon name="plus" size={15} />
        </button>
      </header>

      <div
        class="cards"
        role="list"
        ondragover={(e) => onDragOver(e, status, e.currentTarget)}
        ondrop={(e) => onDrop(e, status)}
      >
        {#each items as t (t.key)}
          {#if drop?.statusId === status.id && dragKey !== t.key && slotOf(items, t.key) === drop.index}
            <div class="placeholder"></div>
          {/if}
          <TicketCard
            ticket={t}
            dragging={dragKey === t.key}
            ondragstart={(e) => onDragStart(e, t)}
            ondragend={endDrag}
            onopen={() => router.openTicket(t.key)}
          />
        {/each}
        {#if drop?.statusId === status.id && drop.index >= items.filter((t) => t.key !== dragKey).length}
          <div class="placeholder"></div>
        {/if}

        {#if adding === status.id}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="input quick"
            placeholder="What needs doing? Enter to add"
            bind:value={newTitle}
            autofocus
            onkeydown={(e) => {
              if (e.key === 'Enter') quickAdd(status)
              if (e.key === 'Escape') adding = null
            }}
            onblur={() => {
              if (!newTitle.trim()) adding = null
            }}
          />
        {:else if loaded && items.length === 0 && !drop}
          <button class="empty" onclick={() => (adding = status.id)}>
            <Icon name="plus" size={14} /> Add ticket
          </button>
        {/if}
      </div>
    </section>
  {/each}
</div>

<style>
  .board {
    flex: 1;
    display: flex;
    gap: 12px;
    min-height: 0;
    padding: 0 20px 20px;
    overflow-x: auto;
  }
  .column {
    display: flex;
    flex-direction: column;
    flex: 0 0 292px;
    min-height: 0;
    border-radius: 10px;
    background: var(--bg-elev);
    border: 1px solid transparent;
    transition: border-color 0.1s;
  }
  .column.target {
    border-color: var(--accent);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 8px 6px 14px;
  }
  header .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--cat);
  }
  h2 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--text-2);
  }
  .count {
    font-size: 12px;
    color: var(--text-3);
  }
  .spacer {
    flex: 1;
  }
  .cards {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 60px;
    padding: 4px 8px 12px;
    overflow-y: auto;
  }
  .placeholder {
    flex: none;
    height: 4px;
    margin: -2px 2px;
    border-radius: 2px;
    background: var(--accent);
  }
  .quick {
    flex: none;
    height: 36px;
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 40px;
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius);
    background: none;
    color: var(--text-3);
    font-size: 12.5px;
    cursor: pointer;
  }
  .empty:hover {
    color: var(--text);
    border-color: var(--text-3);
  }
</style>
