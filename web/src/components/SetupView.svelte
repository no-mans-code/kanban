<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { app } from '../lib/store.svelte'

  let code = $state('')
  let username = $state('')
  let displayName = $state('')
  let password = $state('')
  let confirm = $state('')
  let busy = $state(false)
  let error = $state<string | null>(null)

  async function submit(e: Event) {
    e.preventDefault()
    error = null
    if (password.length < 8) {
      error = 'Passwords need at least 8 characters.'
      return
    }
    if (password !== confirm) {
      error = "Passwords don't match."
      return
    }
    busy = true
    try {
      await api.auth.setup({ code: code.trim(), username: username.trim(), display_name: displayName.trim() || undefined, password })
      await app.checkAuth()
    } catch (e) {
      error = e instanceof ApiError ? e.message : String(e)
    } finally {
      busy = false
    }
  }
</script>

<div class="wrap">
  <form class="card" onsubmit={submit}>
    <img src="/favicon.svg" alt="" width="40" height="40" />
    <h1>Set up this board</h1>
    <p class="lead">
      This board has no administrator yet. Find the one-time setup code in the terminal where you started
      <code>kanban-server</code> — it's printed once, on first start.
    </p>

    <label>
      <span class="field-label">Setup code</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="input mono" bind:value={code} placeholder="ABCD-1234" autofocus autocomplete="off" required />
    </label>
    <label>
      <span class="field-label">Your username</span>
      <input class="input" bind:value={username} placeholder="e.g. your first name" autocomplete="username" required />
    </label>
    <label>
      <span class="field-label">Display name (optional)</span>
      <input class="input" bind:value={displayName} placeholder={username || 'Shown on your avatar'} />
    </label>
    <label>
      <span class="field-label">Password</span>
      <input class="input" type="password" bind:value={password} minlength="8" autocomplete="new-password" required />
    </label>
    <label>
      <span class="field-label">Confirm password</span>
      <input class="input" type="password" bind:value={confirm} minlength="8" autocomplete="new-password" required />
    </label>

    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <button class="btn btn-primary" disabled={busy}>{busy ? 'Setting up…' : 'Create administrator account'}</button>
    <p class="muted small">
      This account can see and manage every project. You can create more restricted accounts for agents afterward,
      from Settings.
    </p>
  </form>
</div>

<style>
  .wrap {
    min-height: 100%;
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
    max-width: 380px;
    padding: 28px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: center;
  }
  .card img {
    margin: 0 auto 4px;
  }
  h1 {
    margin: 0 0 4px;
    font-size: 18px;
  }
  .lead {
    margin: 0 0 8px;
    color: var(--text-2);
    font-size: 13px;
    text-align: left;
  }
  label {
    display: block;
    text-align: left;
  }
  .mono {
    font-family: var(--mono);
    letter-spacing: 0.05em;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
    text-align: left;
  }
  .small {
    font-size: 12px;
    text-align: left;
  }
  code {
    font-family: var(--mono);
    background: var(--surface-3);
    padding: 1px 5px;
    border-radius: 4px;
  }
</style>
