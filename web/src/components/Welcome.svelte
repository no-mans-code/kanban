<script lang="ts">
  import { api } from '../lib/api'
  import { projectUrl, router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'

  let key = $state('')
  let name = $state('')

  async function create() {
    try {
      const p = await api.createProject({ key: key.trim(), name: name.trim() })
      await app.loadProjects()
      router.navigate(projectUrl(p.key))
    } catch (e) {
      toasts.error(e)
    }
  }
</script>

<div class="welcome">
  <img src="/favicon.svg" alt="" width="48" height="48" />
  <h2>Create your first project</h2>
  <p class="muted">A project holds a board of tickets. Its key prefixes every ticket number.</p>
  <form
    onsubmit={(e) => {
      e.preventDefault()
      create()
    }}
  >
    <input class="input key-in" placeholder="KEY" maxlength="10" bind:value={key} oninput={() => (key = key.toUpperCase())} />
    <input class="input" placeholder="Project name" bind:value={name} />
    <button class="btn btn-primary" disabled={!key.trim() || !name.trim()}>Create</button>
  </form>
  <p class="muted small">
    Want sample data instead? Restart the server with <code>--demo</code>.
  </p>
</div>

<style>
  .welcome {
    margin: 12vh auto 0;
    max-width: 520px;
    text-align: center;
    padding: 0 20px;
  }
  h2 {
    margin: 16px 0 6px;
  }
  form {
    display: flex;
    gap: 8px;
    margin: 20px 0 12px;
  }
  .key-in {
    width: 110px;
    font-family: var(--mono);
  }
  .small {
    font-size: 12px;
  }
  code {
    font-family: var(--mono);
  }
</style>
