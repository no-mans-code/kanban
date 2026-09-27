<script lang="ts">
  import { api } from '../../lib/api'
  import { router } from '../../lib/router.svelte'
  import { toasts } from '../../lib/toast.svelte'
  import { childTypeFor, type TicketDetail, type TicketSummary } from '../../lib/types'
  import Avatar from '../Avatar.svelte'
  import Icon from '../Icon.svelte'
  import TicketPicker from '../TicketPicker.svelte'
  import TypeIcon from '../TypeIcon.svelte'

  let { detail, onchange }: { detail: TicketDetail; onchange: () => void } = $props()

  let mode = $state<'none' | 'new' | 'existing'>('none')
  let title = $state('')
  const childType = $derived(childTypeFor(detail.type))
  const project = $derived(detail.key.slice(0, detail.key.lastIndexOf('-')))
  const done = $derived(detail.children.filter((c) => c.status_category === 'done').length)
  const heading = $derived(detail.type === 'epic' ? 'Tickets in this epic' : 'Subtasks')

  async function create() {
    const t = title.trim()
    if (!t || !childType) return
    title = ''
    try {
      await api.createTicket({ project, type: childType, title: t, parent: detail.key })
      onchange()
    } catch (e) {
      title = t
      toasts.error(e)
    }
  }

  async function attach(t: TicketSummary) {
    try {
      await api.updateTicket(t.key, { parent: detail.key })
      mode = 'none'
      onchange()
    } catch (e) {
      toasts.error(e)
    }
  }

  function canAttach(t: TicketSummary) {
    const sameProject = t.key.startsWith(`${project}-`)
    return sameProject && t.parent_key !== detail.key && (detail.type === 'epic' ? t.type !== 'epic' && t.type !== 'subtask' : t.type === 'subtask')
  }
</script>

{#if childType}
  <section>
    <header>
      <h3>{heading}</h3>
      {#if detail.children.length}
        <div class="progress" title="{done} of {detail.children.length} done">
          <div class="bar" style:width="{(done / detail.children.length) * 100}%"></div>
        </div>
        <span class="muted small">{done}/{detail.children.length}</span>
      {/if}
      <span class="spacer"></span>
      <button class="icon-btn" title="Create a {childType}" onclick={() => (mode = mode === 'new' ? 'none' : 'new')}>
        <Icon name="plus" size={15} />
      </button>
      <button class="icon-btn" title="Add an existing ticket" onclick={() => (mode = mode === 'existing' ? 'none' : 'existing')}>
        <Icon name="link" size={15} />
      </button>
    </header>

    {#each detail.children as c (c.key)}
      <div class="row cat-{c.status_category}">
        <TypeIcon type={c.type} size={14} />
        <a class="key" href="?ticket={c.key}">{c.key}</a>
        <button class="title" class:done={c.status_category === 'done'} onclick={() => router.openTicket(c.key)}>
          {c.title}
        </button>
        <span class="status"><span class="dot"></span>{c.status_name}</span>
        <Avatar userId={c.assignee_id} size={20} />
      </div>
    {/each}

    {#if mode === 'new'}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="input"
        placeholder="New {childType} title, Enter to add"
        bind:value={title}
        autofocus
        onkeydown={(e) => {
          if (e.key === 'Enter') create()
          if (e.key === 'Escape') {
            e.stopPropagation()
            mode = 'none'
          }
        }}
      />
    {:else if mode === 'existing'}
      <TicketPicker autofocus placeholder="Find a ticket to move under {detail.key}" filter={canAttach} onpick={attach} />
    {/if}
  </section>
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .progress {
    width: 80px;
    height: 5px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }
  .bar {
    height: 100%;
    background: var(--success);
  }
  .small {
    font-size: 12px;
  }
  .spacer {
    flex: 1;
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
  .title.done {
    color: var(--text-3);
    text-decoration: line-through;
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
</style>
