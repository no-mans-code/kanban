<script lang="ts">
  import type { Priority } from '../lib/types'

  let { priority, size = 16 }: { priority: Priority; size?: number } = $props()

  const COLORS: Record<Priority, string> = {
    highest: '#f0564f',
    high: '#f5883b',
    medium: '#e6b33e',
    low: '#4c8dff',
    lowest: '#58b6e8',
  }
</script>

<svg width={size} height={size} viewBox="0 0 16 16" role="img" aria-label="{priority} priority">
  <title>{priority} priority</title>
  <g fill="none" stroke={COLORS[priority]} stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
    {#if priority === 'highest'}
      <path d="m4 8 4-3.5L12 8M4 12l4-3.5 4 3.5" />
    {:else if priority === 'high'}
      <path d="m4 10 4-4 4 4" />
    {:else if priority === 'medium'}
      <path d="M4 6.5h8M4 10h8" />
    {:else if priority === 'low'}
      <path d="m4 6 4 4 4-4" />
    {:else}
      <path d="m4 4 4 3.5L12 4M4 8l4 3.5L12 8" />
    {/if}
  </g>
</svg>
