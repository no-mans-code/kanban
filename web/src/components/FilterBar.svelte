<script lang="ts">
  import { api } from '../lib/api'
  import { debounce } from '../lib/format'
  import { filters } from '../lib/filters.svelte'
  import { app } from '../lib/store.svelte'
  import { PRIORITIES, TYPES } from '../lib/types'
  import Avatar from './Avatar.svelte'
  import Icon from './Icon.svelte'

  let { project, count, total }: { project: string; count: number; total: number } = $props()

  const search = debounce(async (q: string, key: string) => {
    if (!q.trim()) {
      filters.serverHits = null
      return
    }
    try {
      const hits = await api.allTickets({ project: key, q })
      if (filters.text === q) filters.serverHits = new Set(hits.map((h) => h.key))
    } catch {
      filters.serverHits = null
    }
  }, 150)

  $effect(() => {
    search(filters.text, project)
  })
</script>

<div class="bar">
  <label class="search">
    <Icon name="search" size={15} />
    <input
      id="filter-search"
      placeholder="Filter tickets"
      bind:value={filters.text}
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          filters.text = ''
          e.currentTarget.blur()
        }
      }}
    />
  </label>

  <div class="people" role="group" aria-label="Filter by assignee">
    {#each app.activeUsers as user (user.id)}
      <button
        class="person"
        class:on={filters.assignees.includes(user.id)}
        onclick={() => filters.toggleAssignee(user.id)}
        title="{user.display_name}{filters.assignees.includes(user.id) ? ' (filtering)' : ''}"
      >
        <Avatar userId={user.id} size={26} />
      </button>
    {/each}
    <button
      class="person"
      class:on={filters.assignees.includes('none')}
      onclick={() => filters.toggleAssignee('none')}
      title="Unassigned"
    >
      <Avatar userId={null} size={26} />
    </button>
  </div>

  <select class="select compact" bind:value={filters.type} aria-label="Type">
    <option value="">All types</option>
    {#each TYPES as t (t)}<option value={t}>{t}</option>{/each}
  </select>
  <select class="select compact" bind:value={filters.priority} aria-label="Priority">
    <option value="">All priorities</option>
    {#each PRIORITIES as p (p)}<option value={p}>{p}</option>{/each}
  </select>
  <select class="select compact" bind:value={filters.label} aria-label="Label">
    <option value={null}>All labels</option>
    {#each app.labels as l (l.id)}<option value={l.id}>{l.name}</option>{/each}
  </select>

  {#if filters.active}
    <button class="btn btn-ghost btn-sm" onclick={() => filters.clear()}>Clear</button>
    <span class="count">{count} of {total}</span>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 12px 20px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 220px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text-3);
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    outline: none;
    color: var(--text);
    font-size: 13px;
  }
  .people {
    display: flex;
    padding-left: 4px;
  }
  .person {
    margin-left: -4px;
    padding: 0;
    border: 2px solid var(--bg);
    border-radius: 50%;
    background: none;
    cursor: pointer;
    transition: transform 0.1s;
  }
  .person:hover {
    transform: translateY(-2px);
    z-index: 1;
  }
  .person.on {
    border-color: var(--accent);
    z-index: 2;
  }
  .compact {
    width: auto;
    text-transform: capitalize;
  }
  .count {
    font-size: 12px;
    color: var(--text-3);
  }
</style>
