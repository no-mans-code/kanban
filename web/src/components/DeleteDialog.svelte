<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { router } from '../lib/router.svelte'
  import { toasts } from '../lib/toast.svelte'
  import type { ChildrenChoice, DeletePlan, DependentsChoice, TicketSummary } from '../lib/types'
  import Modal from './Modal.svelte'
  import TicketPicker from './TicketPicker.svelte'
  import TypeIcon from './TypeIcon.svelte'

  let { ticketKey, onclose }: { ticketKey: string; onclose: () => void } = $props()

  let plan = $state<DeletePlan | null>(null)
  let loadError = $state<string | null>(null)
  let children = $state<ChildrenChoice | null>(null)
  let dependents = $state<DependentsChoice | null>(null)
  let moveTo = $state<TicketSummary | null>(null)
  let transferTo = $state<TicketSummary | null>(null)
  let typed = $state('')
  let busy = $state(false)
  let error = $state<string | null>(null)

  $effect(() => {
    api.deletePlan(ticketKey).then(
      (p) => (plan = p),
      (e) => (loadError = e instanceof Error ? e.message : String(e)),
    )
  })

  const keys = (list: { key: string }[]) => list.map((t) => t.key).join(', ')
  const project = $derived(ticketKey.slice(0, ticketKey.lastIndexOf('-')))

  const CHILD_LABELS: Record<ChildrenChoice, string> = {
    delete: 'Delete them too',
    detach: 'Keep them as standalone tickets',
    move: 'Move them under another ticket',
    promote: 'Promote them into the parent epic',
  }
  const DEPENDENT_LABELS: Record<DependentsChoice, string> = {
    drop: 'Drop the dependency',
    bridge: 'Keep the order (bridge)',
    children: 'Make them wait for its children instead',
    transfer: 'Make them wait for another ticket',
  }

  /** Can this ticket take every child of the one being deleted? */
  function fitsAllChildren(t: TicketSummary): boolean {
    if (!plan || t.key === plan.ticket.key || plan.children.some((c) => c.key === t.key)) return false
    if (!t.key.startsWith(`${project}-`)) return false
    return plan.children.every((c) =>
      t.type === 'epic' ? ['story', 'task', 'bug'].includes(c.type) : c.type === 'subtask' && t.type !== 'subtask',
    )
  }

  const affected = $derived(
    plan !== null &&
      (plan.children.length > 0 || plan.dependents.length > 0 || plan.blockers.length > 0 || plan.other_links.length > 0),
  )

  const complete = $derived(
    plan !== null &&
      (plan.children_options.length === 0 || (children !== null && (children !== 'move' || moveTo !== null))) &&
      (plan.dependents_options.length === 0 ||
        (dependents !== null && (dependents !== 'transfer' || transferTo !== null))) &&
      !(children === 'delete' && dependents === 'children'),
  )

  const confirmed = $derived(!affected || typed.trim().toUpperCase() === ticketKey.toUpperCase())

  /** Plain-language consequences of the current choices. */
  const outcome = $derived.by(() => {
    if (!plan) return []
    const out: string[] = []
    const kids = plan.children
    const grand = plan.grandchildren.length
    out.push(`${plan.ticket.key} is deleted permanently.`)
    if (kids.length) {
      if (children === 'delete')
        out.push(`${kids.length} child ticket${kids.length > 1 ? 's' : ''} (${keys(kids)})${grand ? ` and ${grand} of their subtasks` : ''} are deleted too.`)
      if (children === 'detach')
        out.push(
          kids.some((k) => k.type === 'subtask')
            ? `${keys(kids)} become standalone tasks.`
            : `${keys(kids)} stay, without a parent.`,
        )
      if (children === 'move' && moveTo) out.push(`${keys(kids)} move under ${moveTo.key}.`)
      if (children === 'promote' && plan.parent) out.push(`${keys(kids)} become tasks in ${plan.parent.key}.`)
    }
    const deps = plan.dependents
    if (deps.length) {
      if (dependents === 'drop') {
        const ready = deps.filter((d) => d.becomes_ready_if_dropped)
        out.push(`${keys(deps)} stop waiting on it.`)
        if (ready.length) out.push(`${keys(ready)} become${ready.length === 1 ? 's' : ''} ready to start.`)
      }
      if (dependents === 'bridge') out.push(`${keys(deps)} will wait for ${keys(plan.blockers)} instead.`)
      if (dependents === 'children') out.push(`${keys(deps)} will wait for ${keys(kids)} instead.`)
      if (dependents === 'transfer' && transferTo) out.push(`${keys(deps)} will wait for ${transferTo.key} instead.`)
    }
    if (plan.blockers.length && dependents !== 'bridge')
      out.push(`${keys(plan.blockers)} no longer block${plan.blockers.length === 1 ? 's' : ''} anything through it.`)
    if (children === 'delete' && plan.children_dependents.length)
      out.push(`${keys(plan.children_dependents)} stop waiting on the deleted children.`)
    if (plan.other_links.length) out.push(`Its other links (${keys(plan.other_links.map((l) => l.ticket))}) are removed.`)
    if (plan.parent) out.push(`${plan.parent.key}'s history records the deletion.`)
    return out
  })

  async function confirm() {
    if (!plan || !complete || !confirmed || busy) return
    busy = true
    error = null
    try {
      await api.deleteTicket(ticketKey, {
        children: children ?? undefined,
        dependents: dependents ?? undefined,
        move_to: children === 'move' ? moveTo?.key : undefined,
        transfer_to: dependents === 'transfer' ? transferTo?.key : undefined,
      })
      toasts.show(`${ticketKey} deleted`, 'success')
      onclose()
      router.closeTicket()
    } catch (e) {
      error = e instanceof ApiError ? e.message : String(e)
    } finally {
      busy = false
    }
  }
</script>

{#snippet row(t: TicketSummary, note = '')}
  <li class="cat-{t.status_category}">
    <TypeIcon type={t.type} size={14} />
    <span class="key">{t.key}</span>
    <span class="title">{t.title}</span>
    {#if note}<span class="note">{note}</span>{/if}
    <span class="status"><span class="dot"></span>{t.status_name}</span>
  </li>
{/snippet}

<Modal title="Delete {ticketKey}?" {onclose} width={680}>
  {#if loadError}
    <p class="error">{loadError}</p>
  {:else if !plan}
    <p class="muted">Checking what this affects…</p>
  {:else}
    <div class="ticket">
      <TypeIcon type={plan.ticket.type} />
      <strong>{plan.ticket.title}</strong>
    </div>

    {#if plan.children.length}
      <section>
        <h3>
          {plan.children.length} child ticket{plan.children.length > 1 ? 's' : ''}
          {#if plan.grandchildren.length}<span class="muted"> with {plan.grandchildren.length} subtasks of their own</span>{/if}
        </h3>
        <ul>{#each plan.children as c (c.key)}{@render row(c)}{/each}</ul>
        <div class="choices" role="radiogroup" aria-label="What happens to the children">
          {#each plan.children_options as option (option)}
            <label class="choice" class:on={children === option}>
              <input type="radio" name="children" value={option} bind:group={children} />
              {CHILD_LABELS[option]}{#if option === 'promote' && plan.parent}&nbsp;<span class="key">{plan.parent.key}</span>{/if}
            </label>
          {/each}
        </div>
        {#if children === 'move'}
          <div class="picker">
            {#if moveTo}
              <span>Move under <span class="key">{moveTo.key}</span> {moveTo.title}</span>
              <button class="btn btn-ghost btn-sm" onclick={() => (moveTo = null)}>Change</button>
            {:else}
              <TicketPicker autofocus placeholder="Find the new parent" filter={fitsAllChildren} onpick={(t) => (moveTo = t)} />
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    {#if plan.dependents.length}
      <section>
        <h3>{plan.dependents.length} ticket{plan.dependents.length > 1 ? 's' : ''} waiting on it</h3>
        <ul>
          {#each plan.dependents as d (d.key)}
            {@render row(d, dependents === 'drop' && d.becomes_ready_if_dropped ? 'becomes ready' : '')}
          {/each}
        </ul>
        <div class="choices" role="radiogroup" aria-label="What happens to tickets waiting on it">
          {#each plan.dependents_options as option (option)}
            {@const disabled = option === 'children' && children === 'delete'}
            <label class="choice" class:on={dependents === option} class:disabled title={disabled ? 'Its children are being deleted' : ''}>
              <input type="radio" name="dependents" value={option} bind:group={dependents} {disabled} />
              {DEPENDENT_LABELS[option]}{#if option === 'bridge'}<span class="muted">: wait for {keys(plan.blockers)}</span>{/if}
            </label>
          {/each}
        </div>
        {#if dependents === 'transfer'}
          <div class="picker">
            {#if transferTo}
              <span>Wait for <span class="key">{transferTo.key}</span> {transferTo.title}</span>
              <button class="btn btn-ghost btn-sm" onclick={() => (transferTo = null)}>Change</button>
            {:else}
              <TicketPicker autofocus placeholder="Find the ticket they should wait for" exclude={[plan.ticket.key]} onpick={(t) => (transferTo = t)} />
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    {#if plan.blockers.length || plan.other_links.length}
      <section>
        <h3>Links that go away</h3>
        <ul>
          {#each plan.blockers as b (b.key)}{@render row(b, 'blocks it')}{/each}
          {#each plan.other_links as l (l.ticket.key + l.label)}{@render row(l.ticket, l.label)}{/each}
        </ul>
      </section>
    {/if}

    <section class="outcome" aria-live="polite">
      <h3>What will happen</h3>
      <ul class="bullets">{#each outcome as line (line)}<li>{line}</li>{/each}</ul>
      {#if !complete}<p class="muted">Choose an option in each section above.</p>{/if}
    </section>

    {#if affected}
      <label class="type-to-confirm">
        <span>Type <strong class="key">{ticketKey}</strong> to confirm</span>
        <input class="input" bind:value={typed} autocomplete="off" spellcheck="false" onkeydown={(e) => e.key === 'Enter' && confirm()} />
      </label>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  {/if}

  {#snippet footer()}
    <button class="btn btn-ghost" onclick={onclose}>Cancel</button>
    <button class="btn btn-danger" disabled={!complete || !confirmed || busy} onclick={confirm}>
      {busy ? 'Deleting…' : `Delete ${ticketKey}`}
    </button>
  {/snippet}
</Modal>

<style>
  .ticket {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }
  section {
    margin-top: 16px;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 160px;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .note {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 8px;
    background: var(--accent-soft);
    color: var(--text);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--text-2);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--cat);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 13px;
    cursor: pointer;
  }
  .choice.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .choice.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .choice input {
    accent-color: var(--accent);
  }
  .picker {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    font-size: 13px;
  }
  .outcome {
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--danger-soft);
  }
  .bullets {
    list-style: disc;
    padding-left: 18px;
    max-height: none;
    gap: 2px;
  }
  .bullets li {
    display: list-item;
    border: none;
    padding: 0;
  }
  .type-to-confirm {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 16px;
    font-size: 13px;
  }
  .type-to-confirm .input {
    max-width: 220px;
    font-family: var(--mono);
  }
  .error {
    margin: 12px 0 0;
    color: var(--danger);
  }
</style>
