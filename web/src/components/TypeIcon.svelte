<script lang="ts" module>
  import type { TicketType } from '../lib/types'

  export const TYPE_COLORS: Record<TicketType, string> = {
    epic: '#a371f7',
    story: '#3fb950',
    task: '#4c8dff',
    bug: '#f0564f',
    subtask: '#58b6e8',
  }
</script>

<script lang="ts">
  let { type, size = 16 }: { type: TicketType; size?: number } = $props()
</script>

<svg width={size} height={size} viewBox="0 0 16 16" aria-label={type} role="img">
  <title>{type}</title>
  <rect width="16" height="16" rx="3.5" fill={TYPE_COLORS[type]} />
  {#if type === 'epic'}
    <path d="M9.2 3 5 9h3l-1.2 4L11 7H8z" fill="#fff" />
  {:else if type === 'story'}
    <path d="M5 3.5h6v9L8 10.3 5 12.5z" fill="#fff" />
  {:else if type === 'task'}
    <path d="m4.6 8.2 2.3 2.3 4.5-4.8" fill="none" stroke="#fff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" />
  {:else if type === 'bug'}
    <circle cx="8" cy="8" r="3.3" fill="#fff" />
  {:else}
    <rect x="4" y="4" width="4.5" height="4.5" rx="1" fill="none" stroke="#fff" stroke-width="1.5" />
    <rect x="7.5" y="7.5" width="4.5" height="4.5" rx="1" fill="#fff" />
  {/if}
</svg>
