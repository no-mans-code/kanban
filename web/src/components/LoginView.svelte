<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { app } from '../lib/store.svelte'

  let username = $state('')
  let password = $state('')
  let busy = $state(false)
  let error = $state<string | null>(null)

  async function submit(e: Event) {
    e.preventDefault()
    error = null
    busy = true
    try {
      await api.auth.login(username.trim(), password)
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
    <h1>Sign in</h1>
    <label>
      <span class="field-label">Username</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="input" bind:value={username} autofocus autocomplete="username" required />
    </label>
    <label>
      <span class="field-label">Password</span>
      <input class="input" type="password" bind:value={password} autocomplete="current-password" required />
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <button class="btn btn-primary" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</button>
    <p class="muted small">
      Agents don't sign in here — they use an API token. Create one from Settings &gt; Tokens once you're in.
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
    max-width: 340px;
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
  label {
    display: block;
    text-align: left;
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
</style>
