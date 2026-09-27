<script lang="ts">
  import { fullTime, relativeTime } from '../../lib/format'
  import { app } from '../../lib/store.svelte'
  import type { Activity } from '../../lib/types'
  import Avatar from '../Avatar.svelte'

  let { items }: { items: Activity[] } = $props()

  function name(id: number | null) {
    return id === null ? 'System' : (app.usersById.get(id)?.display_name ?? 'Unknown')
  }

  function describe(a: Activity): { text: string; from?: string | null; to?: string | null } {
    switch (a.action) {
      case 'created':
        return { text: 'created this ticket' }
      case 'updated':
        if (a.field === 'description') return { text: 'updated the description' }
        return { text: `changed ${a.field}`, from: a.old_value || '—', to: a.new_value || '—' }
      case 'commented':
        return { text: 'commented', to: a.new_value }
      case 'comment_edited':
        return { text: 'edited a comment', to: a.new_value }
      case 'comment_deleted':
        return { text: 'deleted a comment', from: a.old_value }
      case 'linked':
        return { text: `added a link: ${a.field}`, to: a.new_value }
      case 'unlinked':
        return { text: `removed a link: ${a.field}`, from: a.old_value }
      case 'watcher_added':
        return { text: 'added a watcher', to: a.new_value }
      case 'watcher_removed':
        return { text: 'removed a watcher', from: a.old_value }
      default:
        return { text: a.action }
    }
  }
</script>

<ol>
  {#each [...items].reverse() as a (a.id)}
    {@const d = describe(a)}
    <li>
      <Avatar userId={a.actor_id} size={22} />
      <div>
        <div>
          <strong>{name(a.actor_id)}</strong>
          {d.text}
          <span class="muted" title={fullTime(a.created_at)}>{relativeTime(a.created_at)}</span>
        </div>
        {#if d.from || d.to}
          <div class="change">
            {#if d.from}<span class="from">{d.from}</span>{/if}
            {#if d.from && d.to}<span class="muted">→</span>{/if}
            {#if d.to}<span class="to">{d.to}</span>{/if}
          </div>
        {/if}
      </div>
    </li>
  {/each}
</ol>

<style>
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    font-size: 13px;
  }
  li {
    display: flex;
    gap: 10px;
  }
  .muted {
    margin-left: 4px;
    font-size: 12px;
  }
  .change {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }
  .from,
  .to {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--surface-2);
    font-size: 12px;
    max-width: 100%;
    overflow-wrap: anywhere;
  }
  .from {
    text-decoration: line-through;
    color: var(--text-3);
  }
</style>
