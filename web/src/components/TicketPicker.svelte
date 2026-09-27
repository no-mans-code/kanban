<script lang="ts">
  import { api } from '../lib/api'
  import { debounce } from '../lib/format'
  import type { TicketSummary } from '../lib/types'
  import TypeIcon from './TypeIcon.svelte'

  let {
    placeholder = 'Search tickets by key or text',
    exclude = [],
    filter = () => true,
    onpick,
    autofocus = false,
    id,
  }: {
    placeholder?: string
    exclude?: string[]
    filter?: (t: TicketSummary) => boolean
    onpick: (t: TicketSummary) => void
    autofocus?: boolean
    id?: string
  } = $props()

  let q = $state('')
  let results = $state<TicketSummary[]>([])
  let active = $state(0)
  let open = $state(false)

  const run = debounce(async (text: string) => {
    const list = await api.tickets(text.trim() ? { q: text, sort: 'updated', limit: 30 } : { sort: 'updated', limit: 30 })
    if (text === q) {
      // Someone who types a whole key means that ticket, not the ones that mention it.
      const exact = text.trim().toUpperCase()
      results = list
        .filter((t) => !exclude.includes(t.key) && filter(t))
        .sort((a, b) => Number(b.key === exact) - Number(a.key === exact))
        .slice(0, 10)
      active = 0
    }
  }, 120)

  $effect(() => {
    if (open) run(q)
  })

  function choose(t: TicketSummary) {
    onpick(t)
    q = ''
    open = false
  }
</script>

<div class="picker">
  <!-- svelte-ignore a11y_autofocus -->
  <input
    class="input"
    {id}
    {placeholder}
    {autofocus}
    bind:value={q}
    onfocus={() => (open = true)}
    onblur={() => setTimeout(() => (open = false), 120)}
    onkeydown={(e) => {
      if (e.key === 'ArrowDown') active = Math.min(active + 1, results.length - 1)
      else if (e.key === 'ArrowUp') active = Math.max(active - 1, 0)
      else if (e.key === 'Enter' && results[active]) {
        e.preventDefault()
        choose(results[active])
      } else return
      e.preventDefault()
    }}
  />
  {#if open && results.length > 0}
    <div class="results" role="listbox">
      {#each results as t, i (t.key)}
        <button
          class="row"
          class:active={i === active}
          role="option"
          aria-selected={i === active}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => choose(t)}
        >
          <TypeIcon type={t.type} size={14} />
          <span class="key">{t.key}</span>
          <span class="title">{t.title}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .picker {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  .results {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    z-index: 50;
    padding: 4px;
    border-radius: var(--radius);
    border: 1px solid var(--border-strong);
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    font-size: 13px;
    cursor: pointer;
  }
  .row.active,
  .row:hover {
    background: var(--surface-2);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
