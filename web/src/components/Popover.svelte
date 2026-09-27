<script lang="ts">
  import type { Snippet } from 'svelte'

  let {
    open = $bindable(false),
    trigger,
    children,
    align = 'left',
    width = 240,
  }: {
    open?: boolean
    trigger: Snippet<[{ toggle: () => void }]>
    children: Snippet<[{ close: () => void }]>
    align?: 'left' | 'right'
    width?: number
  } = $props()

  let root: HTMLElement

  function close() {
    open = false
  }

  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) close()
  }
</script>

<svelte:window
  onpointerdown={onWindowPointer}
  onkeydown={(e) => {
    if (open && e.key === 'Escape') {
      e.stopPropagation()
      close()
    }
  }}
/>

<div class="pop" bind:this={root}>
  {@render trigger({ toggle: () => (open = !open) })}
  {#if open}
    <div class="panel" data-popover class:right={align === 'right'} style:width="{width}px">
      {@render children({ close })}
    </div>
  {/if}
</div>

<style>
  .pop {
    position: relative;
  }
  .panel {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 50;
    max-height: 340px;
    overflow-y: auto;
    padding: 4px;
    border-radius: var(--radius);
    border: 1px solid var(--border-strong);
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .panel.right {
    left: auto;
    right: 0;
  }
</style>
