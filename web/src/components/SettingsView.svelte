<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { projectUrl, router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { confirmer, toasts } from '../lib/toast.svelte'
  import { CATEGORIES, CATEGORY_NAMES, type Label, type ProjectDetail, type Settings, type Status } from '../lib/types'
  import ProjectMembers from './ProjectMembers.svelte'
  import PeoplePanel from './settings/PeoplePanel.svelte'
  import TokensPanel from './settings/TokensPanel.svelte'
  import Icon from './Icon.svelte'

  let { tab }: { tab: string } = $props()

  const TABS = $derived(
    [
      ['general', 'General'],
      ['workflow', 'Workflow'],
      ['labels', 'Labels'],
      ['tokens', 'Tokens'],
      app.isSiteAdmin ? ['people', 'People'] : null,
      ['projects', 'Projects'],
    ].filter((t): t is [string, string] => t !== null),
  )

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
  const canEditWorkflow = $derived(wf?.role === 'admin')

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

  // ------------------------------------------------ projects
  let pKey = $state('')
  let pName = $state('')
  let expanded = $state<string | null>(null)

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
      {#if app.isSiteAdmin}
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
      {:else}
        <p class="muted">Only a site administrator can change this. Ask yours if it needs to be different.</p>
      {/if}
      {#if settings}
        <p class="now">
          {#if settings.dag_height === 0}
            {settings.is_site_admin ? 'There are no dependency chains yet.' : "There are no dependency chains in projects you can see yet."}
          {:else}
            The longest chain {settings.is_site_admin ? '' : 'you can see '}today has <strong>{settings.dag_height}</strong> tickets:
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
      {#if wf && !canEditWorkflow}
        <p class="muted">You need to be an admin of {wf.key} to change its workflow. You have {wf.role} access.</p>
      {/if}
      {#if wf}
        <div class="rows">
          {#each wf.statuses as s, i (s.id)}
            <div class="row cat-{s.category}">
              <span class="dot"></span>
              <input
                class="input grow"
                value={s.name}
                disabled={!canEditWorkflow}
                onchange={(e) => wfCall(() => api.updateStatus(s.id, { name: e.currentTarget.value }))}
                aria-label="Status name"
              />
              <select
                class="select cat"
                value={s.category}
                disabled={!canEditWorkflow}
                onchange={(e) => wfCall(() => api.updateStatus(s.id, { category: e.currentTarget.value }))}
                aria-label="Category"
              >
                {#each CATEGORIES as c (c)}<option value={c}>{CATEGORY_NAMES[c]}</option>{/each}
              </select>
              <input
                class="input wip"
                type="number"
                min="1"
                placeholder="No limit"
                value={s.wip_limit ?? ''}
                disabled={!canEditWorkflow}
                title="WIP limit for this column"
                aria-label="WIP limit"
                onchange={(e) => {
                  const n = Math.round(Number(e.currentTarget.value.trim()))
                  // A number input still accepts "1.5", "-3" or garbage by keyboard;
                  // send a clean value (a whole number >= 1) rather than a raw one the
                  // server would reject as an int, or fall back to "no limit".
                  const wip_limit = Number.isFinite(n) && n >= 1 ? n : null
                  wfCall(() => api.updateStatus(s.id, { wip_limit }))
                }}
              />
              {#if canEditWorkflow}
                <button class="icon-btn" title="Move left" disabled={i === 0} onclick={() => move(i, -1)}><Icon name="up" size={14} /></button>
                <button class="icon-btn" title="Move right" disabled={i === wf.statuses.length - 1} onclick={() => move(i, 1)}><Icon name="down" size={14} /></button>
                <button class="icon-btn" title="Delete" disabled={wf.statuses.length === 1} onclick={() => deleteStatus(s, i)}><Icon name="trash" size={14} /></button>
              {/if}
            </div>
          {/each}
          {#if canEditWorkflow}
            <div class="row add">
              <input class="input grow" placeholder="New status" bind:value={newStatus} onkeydown={(e) => e.key === 'Enter' && addStatus()} />
              <select class="select cat" bind:value={newCategory} aria-label="Category">
                {#each CATEGORIES as c (c)}<option value={c}>{CATEGORY_NAMES[c]}</option>{/each}
              </select>
              <button class="btn" disabled={!newStatus.trim()} onclick={addStatus}>Add</button>
            </div>
          {/if}
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

    {:else if tab === 'tokens'}
      <TokensPanel />

    {:else if tab === 'people' && app.isSiteAdmin}
      <PeoplePanel />

    {:else if tab === 'projects'}
      <h2>Projects</h2>
      <div class="rows">
        {#each app.projects as p (p.key)}
          <div class="proj">
            <div class="row">
              <button class="icon-btn" title={expanded === p.key ? 'Hide members' : 'Show members'} onclick={() => (expanded = expanded === p.key ? null : p.key)}>
                <Icon name={expanded === p.key ? 'chevron-down' : 'chevron'} size={13} />
              </button>
              <span class="key pkey">{p.key}</span>
              <input
                class="input grow"
                value={p.name}
                disabled={p.role !== 'admin'}
                onchange={(e) => renameProject(p.key, e.currentTarget.value)}
                aria-label="Project name"
              />
              <span class="chip role">{p.role}</span>
              <a class="btn btn-sm" href={projectUrl(p.key)}>Open</a>
            </div>
            {#if expanded === p.key}<ProjectMembers projectKey={p.key} />{/if}
          </div>
        {/each}
      </div>
      {#if app.isSiteAdmin}
        <h3>New project</h3>
        <div class="row add">
          <input class="input pk" placeholder="KEY" maxlength="10" bind:value={pKey} oninput={() => (pKey = pKey.toUpperCase())} />
          <input class="input grow" placeholder="Project name" bind:value={pName} onkeydown={(e) => e.key === 'Enter' && createProject()} />
          <button class="btn btn-primary" disabled={!pKey.trim() || !pName.trim()} onclick={createProject}>Create</button>
        </div>
        <p class="muted small">The key prefixes every ticket number, like {pKey || 'KEY'}-1. 2-10 letters or digits, starting with a letter.</p>
      {:else}
        <p class="muted small">Only a site administrator can create new projects.</p>
      {/if}
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
  .wip {
    width: 80px;
    flex: none;
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
  .role {
    text-transform: capitalize;
  }
  .icon-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }
  input:disabled,
  select:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
