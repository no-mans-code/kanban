<script lang="ts">
  import { api } from '../lib/api'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'
  import Icon from './Icon.svelte'
  import Popover from './Popover.svelte'

  let { value, onchange }: { value: number[]; onchange: (ids: number[]) => void } = $props()

  let open = $state(false)
  let q = $state('')
  const selected = $derived(value.map((id) => app.labelsById.get(id)).filter((l) => l !== undefined))
  const options = $derived(app.labels.filter((l) => l.name.toLowerCase().includes(q.trim().toLowerCase())))
  const exact = $derived(app.labels.some((l) => l.name.toLowerCase() === q.trim().toLowerCase()))

  function toggle(id: number) {
    onchange(value.includes(id) ? value.filter((v) => v !== id) : [...value, id])
  }

  async function create() {
    const name = q.trim()
    if (!name) return
    try {
      const label = await api.createLabel({ name })
      await app.loadLabels()
      q = ''
      onchange([...value, label.id])
    } catch (e) {
      toasts.error(e)
    }
  }
</script>

<Popover bind:open width={240}>
  {#snippet trigger({ toggle: t })}
    <button class="trigger" onclick={t}>
      {#each selected as l (l.id)}
        <span class="chip"><span class="dot" style:background={l.color}></span>{l.name}</span>
      {:else}
        <span class="muted">None</span>
      {/each}
    </button>
  {/snippet}
  {#snippet children()}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="input filter"
      placeholder="Find or create a label"
      bind:value={q}
      autofocus
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          if (options.length === 1) toggle(options[0].id)
          else if (!exact) create()
        }
      }}
    />
    {#each options as l (l.id)}
      <button class="opt" onclick={() => toggle(l.id)}>
        <span class="check">{#if value.includes(l.id)}<Icon name="check" size={14} />{/if}</span>
        <span class="dot" style:background={l.color}></span>
        {l.name}
      </button>
    {/each}
    {#if q.trim() && !exact}
      <button class="opt create" onclick={create}><Icon name="plus" size={14} /> Create "{q.trim()}"</button>
    {/if}
  {/snippet}
</Popover>

<style>
  .trigger {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    width: 100%;
    min-height: 30px;
    align-items: center;
    padding: 4px 6px;
    margin-left: -6px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .trigger:hover {
    background: var(--surface-2);
  }
  .filter {
    margin-bottom: 4px;
    height: 30px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    font-size: 13px;
    cursor: pointer;
  }
  .opt:hover {
    background: var(--surface-2);
  }
  .check {
    width: 14px;
    line-height: 0;
    color: var(--accent);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
  .create {
    color: var(--accent);
  }
</style>
