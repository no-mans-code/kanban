<script lang="ts">
  import { app } from '../lib/store.svelte'
  import Avatar from './Avatar.svelte'
  import Popover from './Popover.svelte'

  let {
    value,
    onpick,
    allowNone = true,
    label = 'Unassigned',
    exclude = [],
  }: {
    value: number | null
    onpick: (id: number | null) => void
    allowNone?: boolean
    label?: string
    exclude?: number[]
  } = $props()

  let open = $state(false)
  let q = $state('')
  const myId = $derived(app.me?.id ?? null)
  const selected = $derived(value === null ? null : app.usersById.get(value))
  const options = $derived(
    app.activeUsers.filter(
      (u) => !exclude.includes(u.id) && u.display_name.toLowerCase().includes(q.trim().toLowerCase()),
    ),
  )

  function pick(id: number | null, close: () => void) {
    close()
    q = ''
    if (id !== value) onpick(id)
  }
</script>

<Popover bind:open width={240}>
  {#snippet trigger({ toggle })}
    <button class="trigger" onclick={toggle}>
      <Avatar userId={value} size={22} />
      <span class:muted={!selected}>{selected?.display_name ?? label}</span>
    </button>
  {/snippet}
  {#snippet children({ close })}
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="input filter"
      placeholder="Search people"
      bind:value={q}
      autofocus
      onkeydown={(e) => {
        if (e.key === 'Enter' && options[0]) pick(options[0].id, close)
      }}
    />
    {#if myId !== null && myId !== value && !exclude.includes(myId)}
      <button class="opt" onclick={() => pick(myId, close)}>
        <Avatar userId={myId} size={20} /> Assign to me
      </button>
    {/if}
    {#if allowNone}
      <button class="opt" class:on={value === null} onclick={() => pick(null, close)}>
        <Avatar userId={null} size={20} /> {label}
      </button>
    {/if}
    {#each options as u (u.id)}
      <button class="opt" class:on={u.id === value} onclick={() => pick(u.id, close)}>
        <Avatar userId={u.id} size={20} />
        {u.display_name}
        <span class="muted">@{u.username}</span>
      </button>
    {/each}
  {/snippet}
</Popover>

<style>
  .trigger {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
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
  .trigger span {
    white-space: nowrap;
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
  .opt:hover,
  .opt.on {
    background: var(--surface-2);
  }
  .opt .muted {
    margin-left: auto;
    font-size: 11px;
  }
</style>
