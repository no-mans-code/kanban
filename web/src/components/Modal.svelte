<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    title,
    onclose,
    width = 560,
    children,
    footer,
  }: {
    title: string
    onclose: () => void
    width?: number
    children: Snippet
    footer?: Snippet
  } = $props()
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      e.stopPropagation()
      onclose()
    }
  }}
/>

<div class="overlay" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-label={title} style:width="min({width}px, calc(100vw - 32px))">
    <h2>{title}</h2>
    <div class="body">{@render children()}</div>
    {#if footer}<footer>{@render footer()}</footer>{/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: start center;
    padding-top: 12vh;
    background: var(--overlay);
    animation: fade 0.1s ease-out;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .modal {
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    border-radius: 12px;
    border: 1px solid var(--border-strong);
    background: var(--surface);
    box-shadow: var(--shadow);
    animation: pop 0.12s ease-out;
  }
  @keyframes pop {
    from {
      transform: translateY(6px) scale(0.99);
    }
  }
  h2 {
    margin: 0;
    padding: 18px 20px 4px;
    font-size: 16px;
    font-weight: 600;
  }
  .body {
    padding: 12px 20px;
    overflow-y: auto;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px 16px;
  }
</style>
