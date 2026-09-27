<script lang="ts">
  import { renderMarkdown } from '../lib/markdown'

  let { source }: { source: string } = $props()
  // renderMarkdown sanitizes with DOMPurify, so {@html} here is safe.
  const html = $derived(renderMarkdown(source))
</script>

<div class="md">{@html html}</div>
