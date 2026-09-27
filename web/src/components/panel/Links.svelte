<script lang="ts">
  import { api } from '../../lib/api'
  import { router } from '../../lib/router.svelte'
  import { toasts } from '../../lib/toast.svelte'
  import { LINK_CHOICES, type LinkView, type TicketDetail, type TicketSummary } from '../../lib/types'
  import Avatar from '../Avatar.svelte'
  import Icon from '../Icon.svelte'
  import TicketPicker from '../TicketPicker.svelte'
  import TypeIcon from '../TypeIcon.svelte'

  let { detail, onchange }: { detail: TicketDetail; onchange: () => void } = $props()

  let adding = $state(false)
  let choice = $state(1) // "is blocked by" is the most common thing to record

  const groups = $derived.by(() => {
    const map = new Map<string, LinkView[]>()
    for (const l of detail.links) map.set(l.label, [...(map.get(l.label) ?? []), l])
    return [...map.entries()]
  })

  async function add(other: TicketSummary) {
    const c = LINK_CHOICES[choice]
    const [source, target] = c.outward ? [detail.key, other.key] : [other.key, detail.key]
    try {
      await api.createLink(source, target, c.kind)
      adding = false
      onchange()
    } catch (e) {
      toasts.error(e)
    }
  }

  async function remove(l: LinkView) {
    const [source, target] = l.direction === 'outward' ? [detail.key, l.ticket.key] : [l.ticket.key, detail.key]
    try {
      await api.deleteLink(source, target, l.kind)
      onchange()
    } catch (e) {
      toasts.error(e)
    }
  }
</script>

<section>
  <header>
    <h3>Linked tickets</h3>
    <button class="icon-btn" title="Add a link" onclick={() => (adding = !adding)}><Icon name="plus" size={15} /></button>
  </header>

  {#if adding}
    <div class="add">
      <select class="select kind" bind:value={choice} aria-label="Link type">
        {#each LINK_CHOICES as c, i (c.label)}<option value={i}>{c.label}</option>{/each}
      </select>
      <TicketPicker autofocus exclude={[detail.key]} onpick={add} />
    </div>
  {/if}

  {#each groups as [label, links] (label)}
    <div class="group">
      <div class="label">{label}</div>
      {#each links as l (l.ticket.key + l.kind + l.direction)}
        <div class="row cat-{l.ticket.status_category}">
          <TypeIcon type={l.ticket.type} size={14} />
          <a class="key" href="?ticket={l.ticket.key}">{l.ticket.key}</a>
          <button class="title" onclick={() => router.openTicket(l.ticket.key)}>{l.ticket.title}</button>
          <span class="status"><span class="dot"></span>{l.ticket.status_name}</span>
          <Avatar userId={l.ticket.assignee_id} size={20} />
          <button class="icon-btn remove" title="Remove link" onclick={() => remove(l)}><Icon name="x" size={14} /></button>
        </div>
      {/each}
    </div>
  {:else}
    {#if !adding}<p class="muted none">No links. Dependencies you add here show up in the DAG view.</p>{/if}
  {/each}
</section>

<style>
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .add {
    display: flex;
    gap: 6px;
    margin-bottom: 10px;
  }
  .kind {
    width: 150px;
    flex: none;
  }
  .group {
    margin-bottom: 8px;
  }
  .label {
    font-size: 12px;
    color: var(--text-3);
    margin: 6px 0 4px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    margin-bottom: 4px;
    font-size: 13px;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    border: none;
    background: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--cat);
  }
  .remove {
    width: 24px;
    height: 24px;
    opacity: 0;
  }
  .row:hover .remove {
    opacity: 1;
  }
  .none {
    font-size: 13px;
    margin: 0;
  }
</style>
