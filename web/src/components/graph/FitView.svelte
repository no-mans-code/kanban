<script lang="ts">
  import { useSvelteFlow } from '@xyflow/svelte'

  // Lives inside <SvelteFlow> so it can reach the viewport. Refits whenever
  // `token` changes: on first layout and when the node set changes shape, but
  // not on live refreshes, so the view doesn't jump while someone is looking.
  let { token }: { token: number } = $props()
  const flow = useSvelteFlow()

  $effect(() => {
    if (token === 0) return
    requestAnimationFrame(() => flow.fitView({ padding: 0.15, duration: 250, maxZoom: 1.2 }))
  })
</script>
