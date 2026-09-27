<script lang="ts">
  import { initials } from '../lib/format'
  import { app } from '../lib/store.svelte'

  let { userId, size = 22 }: { userId: number | null; size?: number } = $props()
  const user = $derived(userId === null ? null : app.usersById.get(userId))
</script>

{#if user}
  <span
    class="avatar"
    style:width="{size}px"
    style:height="{size}px"
    style:font-size="{Math.round(size * 0.42)}px"
    style:background={user.color}
    title={user.display_name}
  >
    {initials(user.display_name)}
  </span>
{:else}
  <span class="avatar empty" style:width="{size}px" style:height="{size}px" title="Unassigned">
    <svg width={size * 0.55} height={size * 0.55} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M20 21a8 8 0 0 0-16 0M12 13a5 5 0 1 0 0-10 5 5 0 0 0 0 10z" />
    </svg>
  </span>
{/if}

<style>
  .avatar {
    display: inline-grid;
    place-items: center;
    flex: none;
    border-radius: 50%;
    color: #fff;
    font-weight: 600;
    letter-spacing: 0.02em;
    user-select: none;
  }
  .empty {
    background: var(--surface-3);
    color: var(--text-3);
    border: 1px dashed var(--border-strong);
  }
</style>
