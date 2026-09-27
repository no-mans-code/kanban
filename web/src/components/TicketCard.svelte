<script lang="ts">
  import { app } from '../lib/store.svelte'
  import type { TicketSummary } from '../lib/types'
  import Avatar from './Avatar.svelte'
  import Icon from './Icon.svelte'
  import PriorityIcon from './PriorityIcon.svelte'
  import TypeIcon from './TypeIcon.svelte'

  let {
    ticket,
    dragging = false,
    ondragstart,
    ondragend,
    onopen,
  }: {
    ticket: TicketSummary
    dragging?: boolean
    ondragstart?: (e: DragEvent) => void
    ondragend?: (e: DragEvent) => void
    onopen: () => void
  } = $props()

  const labels = $derived(ticket.label_ids.map((id) => app.labelsById.get(id)).filter((l) => l !== undefined))
</script>

<div
  class="card"
  class:dragging
  class:done={ticket.status_category === 'done'}
  data-key={ticket.key}
  draggable={ondragstart ? 'true' : 'false'}
  {ondragstart}
  {ondragend}
  onclick={onopen}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      onopen()
    }
  }}
  role="button"
  tabindex="0"
>
  <div class="title">{ticket.title}</div>

  {#if labels.length || ticket.parent_key || ticket.is_blocked}
    <div class="tags">
      {#if ticket.is_blocked}
        <span class="chip blocked" title="Waiting on a ticket that isn't done"><Icon name="lock" size={11} /> Blocked</span>
      {/if}
      {#if ticket.parent_key}
        <span class="chip" title="Parent"><Icon name="layers" size={11} />{ticket.parent_key}</span>
      {/if}
      {#each labels.slice(0, 3) as label (label.id)}
        <span class="chip"><span class="dot" style:background={label.color}></span>{label.name}</span>
      {/each}
      {#if labels.length > 3}<span class="chip">+{labels.length - 3}</span>{/if}
    </div>
  {/if}

  <div class="meta">
    <TypeIcon type={ticket.type} size={15} />
    <span class="key">{ticket.key}</span>
    <span class="spacer"></span>
    {#if ticket.child_count > 0}
      <span class="stat" title="{ticket.done_child_count} of {ticket.child_count} child tickets done">
        <Icon name="subtasks" size={13} />{ticket.done_child_count}/{ticket.child_count}
      </span>
    {/if}
    {#if ticket.comment_count > 0}
      <span class="stat" title="{ticket.comment_count} comments"><Icon name="comment" size={13} />{ticket.comment_count}</span>
    {/if}
    <PriorityIcon priority={ticket.priority} size={15} />
    <Avatar userId={ticket.assignee_id} size={22} />
  </div>
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    background: var(--surface);
    box-shadow: var(--shadow-sm);
    cursor: pointer;
    user-select: none;
    transition: border-color 0.1s, background 0.1s;
  }
  .card:hover {
    border-color: var(--border-strong);
    background: var(--surface-2);
  }
  .card.dragging {
    opacity: 0.35;
  }
  .card.done .title {
    color: var(--text-2);
  }
  .title {
    font-size: 13.5px;
    font-weight: 500;
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .blocked {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .spacer {
    flex: 1;
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11.5px;
    color: var(--text-3);
  }
</style>
