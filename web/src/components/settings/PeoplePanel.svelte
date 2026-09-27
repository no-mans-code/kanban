<script lang="ts">
  import { api, ApiError } from '../../lib/api'
  import { app } from '../../lib/store.svelte'
  import { confirmer, toasts } from '../../lib/toast.svelte'
  import type { User, UserKind } from '../../lib/types'
  import Avatar from '../Avatar.svelte'
  import Icon from '../Icon.svelte'

  let kind = $state<UserKind>('agent')
  let username = $state('')
  let displayName = $state('')
  let password = $state('')
  let error = $state<string | null>(null)
  let busy = $state(false)

  async function userCall(fn: () => Promise<unknown>) {
    try {
      await fn()
      await app.loadUsers()
    } catch (e) {
      toasts.error(e)
    }
  }

  async function addUser() {
    error = null
    const name = username.trim()
    if (!name) return
    if (kind === 'human' && password.length < 8) {
      error = 'A human account needs a password of at least 8 characters, so they can sign in.'
      return
    }
    busy = true
    try {
      await api.createUser({
        username: name,
        display_name: displayName.trim() || undefined,
        kind,
        password: kind === 'human' ? password : undefined,
      })
      await app.loadUsers()
      username = displayName = password = ''
    } catch (e) {
      error = e instanceof ApiError ? e.message : String(e)
    } finally {
      busy = false
    }
  }

  async function toggleMaster(u: User) {
    if (!u.is_admin) {
      const ok = await confirmer.ask(
        `Make ${u.display_name} a site administrator?`,
        'They will see and manage every project by default — the same full access you have. Use this for the ' +
          'human operator and, if you want it, one manager/master agent. Give ordinary agents project membership ' +
          'instead, so they only see the work meant for them.',
        { confirm: 'Make admin' },
      )
      if (!ok) return
    }
    userCall(() => api.updateUser(u.id, { is_admin: !u.is_admin }))
  }
</script>

<h2>People</h2>
<p class="lead">
  Every human and agent that can hold tickets or a token. Site administrators (marked <strong>master</strong>) see
  and manage every project by default. Everyone else only reaches the projects they're added to — that's what keeps
  an agent from picking up work by accident.
</p>

<div class="rows">
  {#each app.users as u (u.id)}
    <div class="row" class:inactive={!u.active}>
      <Avatar userId={u.id} size={28} />
      <input
        class="input grow"
        value={u.display_name}
        onchange={(e) => userCall(() => api.updateUser(u.id, { display_name: e.currentTarget.value }))}
        aria-label="Display name"
      />
      <span class="muted user">@{u.username}</span>
      <span class="chip kind">{u.kind}</span>
      {#if u.is_admin}<span class="chip master">master</span>{/if}
      <input type="color" class="swatch" value={u.color} onchange={(e) => userCall(() => api.updateUser(u.id, { color: e.currentTarget.value }))} aria-label="Color" />
      <button class="btn btn-sm" disabled={u.id === app.me?.id} onclick={() => toggleMaster(u)}>
        {u.is_admin ? 'Remove master' : 'Make master'}
      </button>
      <button class="btn btn-sm" disabled={u.id === app.me?.id} onclick={() => userCall(() => api.updateUser(u.id, { active: !u.active }))}>
        {u.active ? 'Deactivate' : 'Reactivate'}
      </button>
    </div>
  {/each}
</div>

<h3>Add a person</h3>
<div class="add-form">
  <div class="kind-toggle" role="radiogroup" aria-label="Account type">
    <label class:on={kind === 'agent'}><input type="radio" bind:group={kind} value="agent" /> Agent (API token only)</label>
    <label class:on={kind === 'human'}><input type="radio" bind:group={kind} value="human" /> Human (signs in)</label>
  </div>
  <div class="row">
    <input class="input" placeholder="username" bind:value={username} />
    <input class="input grow" placeholder="Display name (optional)" bind:value={displayName} />
  </div>
  {#if kind === 'human'}
    <input class="input" type="password" placeholder="Password (min 8 characters)" bind:value={password} />
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
  <div class="row">
    <button class="btn btn-primary" disabled={!username.trim() || busy} onclick={addUser}>
      <Icon name="plus" size={14} /> Add {kind}
    </button>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 6px;
    font-size: 18px;
  }
  h3 {
    margin: 28px 0 10px;
    font-size: 14px;
  }
  .lead {
    margin: 0 0 18px;
    color: var(--text-2);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row.inactive {
    opacity: 0.55;
  }
  .grow {
    flex: 1;
  }
  .user {
    font-size: 12px;
    width: 110px;
  }
  .chip.kind {
    text-transform: capitalize;
  }
  .chip.master {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .swatch {
    width: 32px;
    height: 32px;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--surface);
    cursor: pointer;
  }
  .add-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 480px;
  }
  .kind-toggle {
    display: flex;
    gap: 8px;
    margin-bottom: 4px;
  }
  .kind-toggle label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    font-size: 13px;
    cursor: pointer;
  }
  .kind-toggle label.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .kind-toggle input {
    accent-color: var(--accent);
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
</style>
