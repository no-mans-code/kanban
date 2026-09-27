<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { initials } from '../lib/format'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'
  import Icon from './Icon.svelte'
  import Popover from './Popover.svelte'

  let open = $state(false)
  let changing = $state(false)
  let current = $state('')
  let next = $state('')
  let confirm = $state('')
  let error = $state<string | null>(null)
  let busy = $state(false)

  const me = $derived(app.me)

  function resetForm() {
    changing = false
    current = next = confirm = ''
    error = null
  }

  async function logout() {
    open = false
    await app.logout()
  }

  async function savePassword() {
    error = null
    if (next.length < 8) {
      error = 'New password needs at least 8 characters.'
      return
    }
    if (next !== confirm) {
      error = "New passwords don't match."
      return
    }
    busy = true
    try {
      await api.auth.changePassword(current, next)
      toasts.show('Password changed', 'success')
      open = false
      resetForm()
    } catch (e) {
      error = e instanceof ApiError ? e.message : String(e)
    } finally {
      busy = false
    }
  }
</script>

{#if me}
  <Popover bind:open align="right" width={220}>
    {#snippet trigger({ toggle })}
      <button class="trigger" onclick={toggle} title={me.display_name}>
        <span class="avatar" style:background={me.color}>{initials(me.display_name)}</span>
      </button>
    {/snippet}
    {#snippet children()}
      <div class="head">
        <span class="avatar" style:background={me.color}>{initials(me.display_name)}</span>
        <div>
          <div class="name">{me.display_name}</div>
          <div class="muted small">@{me.username}{me.is_admin ? ' · admin' : ''}</div>
        </div>
      </div>
      {#if changing}
        <div class="pw">
          <input class="input" type="password" placeholder="Current password" bind:value={current} autocomplete="current-password" />
          <input class="input" type="password" placeholder="New password" bind:value={next} autocomplete="new-password" />
          <input class="input" type="password" placeholder="Confirm new password" bind:value={confirm} autocomplete="new-password" />
          {#if error}<p class="error">{error}</p>{/if}
          <div class="row">
            <button class="btn btn-ghost btn-sm" onclick={resetForm}>Cancel</button>
            <button class="btn btn-primary btn-sm" disabled={busy} onclick={savePassword}>Save</button>
          </div>
        </div>
      {:else}
        <button class="opt" onclick={() => (changing = true)}><Icon name="lock" size={14} /> Change password</button>
        <a class="opt" href="/settings/general"><Icon name="settings" size={14} /> Settings</a>
        <button class="opt danger" onclick={logout}><Icon name="x" size={14} /> Sign out</button>
      {/if}
    {/snippet}
  </Popover>
{/if}

<style>
  .trigger {
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
    line-height: 0;
  }
  .avatar {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    color: #fff;
    font-size: 12px;
    font-weight: 600;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 10px;
    margin-bottom: 4px;
    border-bottom: 1px solid var(--border);
  }
  .name {
    font-weight: 600;
    font-size: 13px;
  }
  .small {
    font-size: 11.5px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    text-align: left;
    font-size: 13px;
    cursor: pointer;
  }
  .opt:hover {
    background: var(--surface-2);
    text-decoration: none;
  }
  .opt.danger {
    color: var(--danger);
  }
  .pw {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 4px;
  }
  .pw .input {
    height: 30px;
  }
  .error {
    margin: 0;
    font-size: 12px;
    color: var(--danger);
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
