<script lang="ts">
  import { confirmer, toasts } from '../lib/toast.svelte'
  import Icon from './Icon.svelte'
  import Modal from './Modal.svelte'

  let { showShortcuts = $bindable(false) }: { showShortcuts?: boolean } = $props()

  const SHORTCUTS: [string[], string][] = [
    [['c'], 'Create a ticket'],
    [['/'], 'Search all tickets'],
    [['f'], 'Filter the current board or list'],
    [['g', 'b'], 'Go to the board'],
    [['g', 'l'], 'Go to the list'],
    [['g', 'd'], 'Go to the dependency graph'],
    [['g', 's'], 'Go to settings'],
    [['t'], 'Switch dark / light theme'],
    [['Esc'], 'Close the ticket panel or a dialog'],
    [['?'], 'Show this help'],
  ]
</script>

{#if confirmer.current}
  {@const c = confirmer.current}
  <Modal title={c.title} onclose={() => confirmer.answer(false)} width={420}>
    <p class="message">{c.message}</p>
    {#snippet footer()}
      <button class="btn btn-ghost" onclick={() => confirmer.answer(false)}>Cancel</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button class="btn {c.danger ? 'btn-danger' : 'btn-primary'}" autofocus onclick={() => confirmer.answer(true)}>
        {c.confirm}
      </button>
    {/snippet}
  </Modal>
{/if}

{#if showShortcuts}
  <Modal title="Keyboard shortcuts" onclose={() => (showShortcuts = false)} width={440}>
    <dl class="keys">
      {#each SHORTCUTS as [keys, what] (what)}
        <dt>{#each keys as k, i (i)}<span class="kbd">{k}</span>{#if i < keys.length - 1}<span class="then">then</span>{/if}{/each}</dt>
        <dd>{what}</dd>
      {/each}
    </dl>
  </Modal>
{/if}

<div class="toasts" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <div class="toast {t.kind}" role={t.kind === 'error' ? 'alert' : 'status'}>
      <span>{t.message}</span>
      <button class="icon-btn" onclick={() => toasts.dismiss(t.id)} aria-label="Dismiss"><Icon name="x" size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .message {
    margin: 0;
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .keys {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 10px 16px;
    margin: 0;
    align-items: center;
  }
  dt {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .then {
    font-size: 11px;
    color: var(--text-3);
  }
  dd {
    margin: 0;
    color: var(--text-2);
    font-size: 13px;
  }
  .toasts {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 80;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
    pointer-events: none;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(620px, calc(100vw - 32px));
    padding: 8px 8px 8px 14px;
    border-radius: var(--radius);
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    box-shadow: var(--shadow);
    font-size: 13px;
    pointer-events: auto;
    animation: rise 0.14s ease-out;
  }
  .toast.error {
    border-color: var(--danger);
    background: color-mix(in srgb, var(--danger) 12%, var(--surface-2));
  }
  .toast.success {
    border-color: color-mix(in srgb, var(--success) 60%, transparent);
  }
  @keyframes rise {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
</style>
