<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { projectUrl, router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { confirmer, toasts } from '../lib/toast.svelte'
  import { CATEGORIES, CATEGORY_NAMES, type Label, type ProjectDetail, type Settings, type Status, type User } from '../lib/types'
  import Avatar from './Avatar.svelte'
  import Icon from './Icon.svelte'

  let { tab }: { tab: string } = $props()

  const TABS: [string, string][] = [
    ['general', 'General'],
    ['workflow', 'Workflow'],
    ['labels', 'Labels'],
    ['people', 'People'],
    ['projects', 'Projects'],
  ]

  // ------------------------------------------------ general
  let settings = $state<Settings | null>(null)
  let unlimited = $state(true)
  let limit = $state(5)
  let limitError = $state<string | null>(null)

  // settingsVersion also moves when links change or tickets are deleted, the
  // only ticket changes that can alter the dependency height shown here.
  $effect(() => {
    void app.settingsVersion
    api.settings().then((s) => {
      settings = s
      unlimited = s.max_dag_height === null
      if (s.max_dag_height !== null) limit = s.max_dag_height
    })
  })

  async function saveLimit() {
    limitError = null
    try {
      settings = await api.updateSettings({ max_dag_height: unlimited ? null : Math.floor(limit) })
      toasts.show('Saved', 'success', 1500)
    } catch (e) {
      limitError = e instanceof ApiError ? e.message : String(e)
    }
  }

  // ------------------------------------------------ workflow
  let wfKey = $state(app.project?.key ?? app.projects[0]?.key ?? '')
  let wf = $state<ProjectDetail | null>(null)
  let newStatus = $state('')
  let newCategory = $state<Status['category']>('in_progress')

  $effect(() => {
    void app.ticketsVersion
    if (wfKey) api.project(wfKey).then((p) => (wf = p))
  })

  async function wfCall(fn: () => Promise<Status[]>) {
    try {
      const statuses = await fn()
      if (wf) wf.statuses = statuses
      if (app.project?.key === wfKey) app.reloadProject()
    } catch (e) {
      toasts.error(e)
    }
  }

  function move(i: number, delta: number) {
    if (!wf) return
    const ids = wf.statuses.map((s) => s.id)
    const j = i + delta
    if (j < 0 || j >= ids.length) return
    ;[ids[i], ids[j]] = [ids[j], ids[i]]
    wfCall(() => api.reorderStatuses(wfKey, ids))
  }

  function addStatus() {
    const name = newStatus.trim()
    if (!name) return
    wfCall(async () => {
      const statuses = await api.createStatus(wfKey, { name, category: newCategory })
      newStatus = ''
      return statuses
    })
  }

  /** Tickets in a deleted status move to the status on its left (or right, for the first). */
  async function deleteStatus(s: Status, index: number) {
    if (!wf) return
    const target = wf.statuses[index === 0 ? 1 : index - 1]
    try {
      await api.deleteStatus(s.id)
    } catch (e) {
      if (!(e instanceof ApiError) || e.code !== 'status_in_use') return toasts.error(e)
      const count = e.message.split(' ')[0]
      const ok = await confirmer.ask(
        `Delete "${s.name}"?`,
        `${count} tickets are in this status. They will move to "${target.name}".`,
        { confirm: 'Move and delete', danger: true },
      )
      if (!ok) return
      await wfCall(() => api.deleteStatus(s.id, target.id))
      return
    }
    await wfCall(() => api.project(wfKey).then((p) => p.statuses))
  }

  // ------------------------------------------------ labels
  let newLabel = $state('')

  function addLabel() {
    const name = newLabel.trim()
    if (!name) return
    labelCall(async () => {
      await api.createLabel({ name })
      newLabel = ''
    })
  }

  async function labelCall(fn: () => Promise<unknown>) {
    try {
      await fn()
      await app.loadLabels()
    } catch (e) {
      toasts.error(e)
    }
  }

  async function deleteLabel(l: Label) {
    if (await confirmer.ask(`Delete label "${l.name}"?`, 'It will be removed from every ticket.', { confirm: 'Delete', danger: true }))
      labelCall(() => api.deleteLabel(l.id))
  }

  // ------------------------------------------------ people
  let newUsername = $state('')
  let newDisplay = $state('')

  async function userCall(fn: () => Promise<unknown>) {
    try {
      await fn()
      await app.loadUsers()
    } catch (e) {
      toasts.error(e)
    }
  }

  function addUser() {
    const username = newUsername.trim()
    if (!username) return
    userCall(async () => {
      await api.createUser({ username, display_name: newDisplay.trim() || undefined })
      newUsername = ''
      newDisplay = ''
    })
  }

  // ------------------------------------------------ projects
  let pKey = $state('')
  let pName = $state('')

  async function createProject() {
    try {
      const p = await api.createProject({ key: pKey.trim(), name: pName.trim() })
      await app.loadProjects()
      pKey = ''
      pName = ''
      router.navigate(projectUrl(p.key))
    } catch (e) {
      toasts.error(e)
    }
  }

  async function renameProject(key: string, name: string) {
    try {
      await api.updateProject(key, { name })
      await app.loadProjects()
    } catch (e) {
      toasts.error(e)
    }
  }
</script>

<div class="settings">
  <nav class="tabs">
    {#each TABS as [id, label] (id)}
      <a href="/settings/{id}" class:on={tab === id}>{label}</a>
    {/each}
  </nav>

  <div class="content">
    {#if tab === 'general'}
      <h2>Dependency graph</h2>
      <p class="lead">
        Limit how deep chains of <em>blocks</em> links can get. Height counts the tickets on the longest chain, so
        <span class="key">A → B → C</span> has height 3. The limit applies to every project.
      </p>
      <div class="card">
        <label class="radio"><input type="radio" bind:group={unlimited} value={true} /> No limit (default)</label>
        <label class="radio">
          <input type="radio" bind:group={unlimited} value={false} /> At most
          <input class="input num" type="number" min="1" bind:value={limit} disabled={unlimited} onfocus={() => (unlimited = false)} />
          tickets per chain
        </label>
        <div class="actions">
          <button class="btn btn-primary" onclick={saveLimit}>Save</button>
        </div>
        {#if limitError}<p class="error">{limitError}</p>{/if}
      </div>
      {#if settings}
        <p class="now">
          {#if settings.dag_height === 0}
            There are no dependency chains yet.
          {:else}
            The longest chain today has <strong>{settings.dag_height}</strong> tickets:
            {#each settings.longest_chain as k, i (k)}
              <a class="key" href="?ticket={k}">{k}</a>{#if i < settings.longest_chain.length - 1}<span class="muted"> → </span>{/if}
            {/each}
          {/if}
        </p>
      {/if}

    {:else if tab === 'workflow'}
      <h2>Workflow</h2>
      <p class="lead">The columns of a project's board, left to right. The category decides what counts as done.</p>
      <label class="inline">
        <span class="field-label">Project</span>
        <select class="select" bind:value={wfKey}>
          {#each app.projects as p (p.key)}<option value={p.key}>{p.name} ({p.key})</option>{/each}
        </select>
      </label>
      {#if wf}
        <div class="rows">
          {#each wf.statuses as s, i (s.id)}
            <div class="row cat-{s.category}">
              <span class="dot"></span>
              <input
                class="input grow"
                value={s.name}
                onchange={(e) => wfCall(() => api.updateStatus(s.id, { name: e.currentTarget.value }))}
                aria-label="Status name"
              />
              <select
                class="select cat"
                value={s.category}
                onchange={(e) => wfCall(() => api.updateStatus(s.id, { category: e.currentTarget.value }))}
                aria-label="Category"
              >
                {#each CATEGORIES as c (c)}<option value={c}>{CATEGORY_NAMES[c]}</option>{/each}
              </select>
              <button class="icon-btn" title="Move left" disabled={i === 0} onclick={() => move(i, -1)}><Icon name="up" size={14} /></button>
              <button class="icon-btn" title="Move right" disabled={i === wf.statuses.length - 1} onclick={() => move(i, 1)}><Icon name="down" size={14} /></button>
              <button class="icon-btn" title="Delete" disabled={wf.statuses.length === 1} onclick={() => deleteStatus(s, i)}><Icon name="trash" size={14} /></button>
            </div>
          {/each}
          <div class="row add">
            <input class="input grow" placeholder="New status" bind:value={newStatus} onkeydown={(e) => e.key === 'Enter' && addStatus()} />
            <select class="select cat" bind:value={newCategory} aria-label="Category">
              {#each CATEGORIES as c (c)}<option value={c}>{CATEGORY_NAMES[c]}</option>{/each}
            </select>
            <button class="btn" disabled={!newStatus.trim()} onclick={addStatus}>Add</button>
          </div>
        </div>
      {/if}

    {:else if tab === 'labels'}
      <h2>Labels</h2>
      <p class="lead">Shared by every project.</p>
      <div class="rows">
        {#each app.labels as l (l.id)}
          <div class="row">
            <input type="color" class="swatch" value={l.color} onchange={(e) => labelCall(() => api.updateLabel(l.id, { color: e.currentTarget.value }))} aria-label="Color" />
            <input class="input grow" value={l.name} onchange={(e) => labelCall(() => api.updateLabel(l.id, { name: e.currentTarget.value }))} aria-label="Label name" />
            <button class="icon-btn" title="Delete" onclick={() => deleteLabel(l)}><Icon name="trash" size={14} /></button>
          </div>
        {/each}
        <div class="row add">
          <input class="input grow" placeholder="New label" bind:value={newLabel} onkeydown={(e) => e.key === 'Enter' && addLabel()} />
          <button class="btn" disabled={!newLabel.trim()} onclick={addLabel}>Add</button>
        </div>
      </div>

    {:else if tab === 'people'}
      <h2>People</h2>
      <p class="lead">
        Everyone who can be assigned or watch tickets, humans and agents alike. There are no passwords: the board
        only listens on this machine, and you pick who you're acting as in the top right.
      </p>
      <div class="rows">
        {#each app.users as u (u.id) }
          <div class="row" class:inactive={!u.active}>
            <Avatar userId={u.id} size={28} />
            <input class="input grow" value={u.display_name} onchange={(e) => userCall(() => api.updateUser(u.id, { display_name: e.currentTarget.value }))} aria-label="Display name" />
            <span class="muted user">@{u.username}</span>
            <input type="color" class="swatch" value={u.color} onchange={(e) => userCall(() => api.updateUser(u.id, { color: e.currentTarget.value }))} aria-label="Color" />
            <button class="btn btn-sm" onclick={() => userCall(() => api.updateUser(u.id, { active: !u.active }))}>
              {u.active ? 'Deactivate' : 'Reactivate'}
            </button>
          </div>
        {/each}
        <div class="row add">
          <input class="input" placeholder="username" bind:value={newUsername} />
          <input class="input grow" placeholder="Display name (optional)" bind:value={newDisplay} onkeydown={(e) => e.key === 'Enter' && addUser()} />
          <button class="btn" disabled={!newUsername.trim()} onclick={addUser}>Add person</button>
        </div>
      </div>

    {:else if tab === 'projects'}
      <h2>Projects</h2>
      <div class="rows">
        {#each app.projects as p (p.key)}
          <div class="row">
            <span class="key pkey">{p.key}</span>
            <input class="input grow" value={p.name} onchange={(e) => renameProject(p.key, e.currentTarget.value)} aria-label="Project name" />
            <a class="btn btn-sm" href={projectUrl(p.key)}>Open</a>
          </div>
        {/each}
      </div>
      <h3>New project</h3>
      <div class="row add">
        <input class="input pk" placeholder="KEY" maxlength="10" bind:value={pKey} oninput={() => (pKey = pKey.toUpperCase())} />
        <input class="input grow" placeholder="Project name" bind:value={pName} onkeydown={(e) => e.key === 'Enter' && createProject()} />
        <button class="btn btn-primary" disabled={!pKey.trim() || !pName.trim()} onclick={createProject}>Create</button>
      </div>
      <p class="muted small">The key prefixes every ticket number, like {pKey || 'KEY'}-1. 2-10 letters or digits, starting with a letter.</p>
    {/if}
  </div>
</div>

<style>
  .settings {
    flex: 1;
    min-height: 0;
    display: flex;
    overflow: hidden;
  }
  .tabs {
    flex: 0 0 180px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 20px 12px;
    border-right: 1px solid var(--border);
  }
  .tabs a {
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    font-weight: 500;
  }
  .tabs a:hover {
    background: var(--surface-2);
    text-decoration: none;
  }
  .tabs a.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .content {
    flex: 1;
    overflow-y: auto;
    padding: 24px 32px 60px;
    max-width: 820px;
  }
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
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
  }
  .radio {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .radio input[type='radio'] {
    accent-color: var(--accent);
  }
  .num {
    width: 80px;
  }
  .actions {
    display: flex;
  }
  .error {
    margin: 0;
    color: var(--danger);
  }
  .now {
    margin-top: 16px;
    color: var(--text-2);
  }
  .inline {
    display: block;
    max-width: 320px;
    margin-bottom: 16px;
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
  .row.add {
    margin-top: 8px;
  }
  .row.inactive {
    opacity: 0.55;
  }
  .grow {
    flex: 1;
  }
  .dot {
    width: 10px;
    height: 10px;
    flex: none;
    border-radius: 50%;
    background: var(--cat);
  }
  .cat {
    width: 150px;
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
  .user {
    font-size: 12px;
    width: 110px;
  }
  .pkey {
    width: 70px;
  }
  .pk {
    width: 110px;
    font-family: var(--mono);
  }
  .small {
    font-size: 12px;
  }
  .icon-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
</style>
