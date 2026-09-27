<script lang="ts">
  import { api, ApiError, type CreateTokenInput } from '../../lib/api'
  import { fullTime, relativeTime } from '../../lib/format'
  import { app } from '../../lib/store.svelte'
  import { confirmer, toasts } from '../../lib/toast.svelte'
  import type { ApiToken } from '../../lib/types'
  import Avatar from '../Avatar.svelte'
  import Icon from '../Icon.svelte'

  const isAdmin = $derived(app.isSiteAdmin)
  let ownerId = $state<number | null>(app.me?.id ?? null)
  let tokens = $state<ApiToken[]>([])

  let name = $state('')
  let scope = $state<'all' | 'some'>('all')
  let projects = $state<Set<string>>(new Set())
  let readOnly = $state(false)
  let expiry = $state<'90' | '30' | '365' | 'never'>('90')
  let creating = $state(false)
  let error = $state<string | null>(null)
  let justCreated = $state<(ApiToken & { token: string }) | null>(null)

  async function load() {
    if (ownerId === null) return
    tokens = await api.tokens(ownerId === app.me?.id ? undefined : ownerId).catch((e) => (toasts.error(e), []))
  }
  $effect(() => {
    void ownerId
    load()
  })

  function resetForm() {
    name = ''
    scope = 'all'
    projects = new Set()
    readOnly = false
    expiry = '90'
    error = null
  }

  async function create() {
    error = null
    if (!name.trim()) return
    if (scope === 'some' && projects.size === 0) {
      error = 'Pick at least one project, or choose "Every project" instead.'
      return
    }
    creating = true
    try {
      const body: CreateTokenInput = {
        name: name.trim(),
        read_only: readOnly,
        expires_in_days: expiry === 'never' ? null : Number(expiry),
      }
      if (ownerId !== app.me?.id && ownerId !== null) body.user_id = ownerId
      if (scope === 'some') body.projects = [...projects]
      const created = await api.createToken(body)
      justCreated = created
      resetForm()
      await load()
    } catch (e) {
      error = e instanceof ApiError ? e.message : String(e)
    } finally {
      creating = false
    }
  }

  async function revoke(t: ApiToken) {
    const ok = await confirmer.ask(`Revoke "${t.name}"?`, 'Anything using this token stops working immediately.', { confirm: 'Revoke', danger: true })
    if (!ok) return
    try {
      await api.revokeToken(t.id)
      await load()
    } catch (e) {
      toasts.error(e)
    }
  }

  function copy(text: string) {
    navigator.clipboard.writeText(text).then(() => toasts.show('Copied', 'success', 1500))
  }

  const mcpConfig = $derived(
    justCreated
      ? JSON.stringify(
          { mcpServers: { kanban: { command: 'kanban-server', args: ['mcp', '--token', justCreated.token] } } },
          null,
          2,
        )
      : '',
  )

  function status(t: ApiToken): { text: string; danger: boolean } {
    if (t.revoked_at) return { text: 'Revoked', danger: true }
    if (t.expires_at && t.expires_at <= Date.now()) return { text: 'Expired', danger: true }
    return { text: 'Active', danger: false }
  }
</script>

<h2>API tokens</h2>
<p class="lead">
  How agents authenticate. A token belongs to one account and can be narrowed to specific projects, made read-only,
  and given an expiry. Create one for each agent from its own account, scoped to only the projects it should touch —
  a token from a site-admin ("master") account reaches every project by default.
</p>

{#if isAdmin}
  <label class="owner-picker">
    <span class="field-label">Show tokens for</span>
    <select class="select" bind:value={ownerId}>
      {#each app.users as u (u.id)}<option value={u.id}>{u.display_name} (@{u.username}){u.id === app.me?.id ? ' — you' : ''}</option>{/each}
    </select>
  </label>
{/if}

<div class="rows">
  {#each tokens as t (t.id)}
    {@const st = status(t)}
    <div class="row" class:dim={st.danger}>
      <Avatar userId={t.user_id} size={24} />
      <div class="info">
        <div class="line1">
          <strong>{t.name}</strong>
          <span class="hint">kbn_{t.hint}…</span>
          {#if t.read_only}<span class="chip">read-only</span>{/if}
          <span class="chip" class:danger={st.danger}>{st.text}</span>
        </div>
        <div class="line2 muted">
          {t.all_projects ? 'Every project this account can reach' : (t.projects ?? []).join(', ') || 'no projects'}
          · created {relativeTime(t.created_at)}
          {#if t.last_used_at}· last used {relativeTime(t.last_used_at)}{/if}
          {#if t.expires_at}· expires {relativeTime(t.expires_at)}{/if}
        </div>
      </div>
      {#if !t.revoked_at}
        <button class="btn btn-sm btn-danger" onclick={() => revoke(t)}>Revoke</button>
      {/if}
    </div>
  {:else}
    <p class="muted">No tokens yet.</p>
  {/each}
</div>

<h3>New token</h3>
<div class="add-form">
  <input class="input" placeholder={'Name, e.g. "coder agent on KAN"'} bind:value={name} />

  <div class="field">
    <span class="field-label">Projects</span>
    <label class="radio"><input type="radio" bind:group={scope} value="all" /> Every project this account can reach</label>
    <label class="radio"><input type="radio" bind:group={scope} value="some" /> Only these:</label>
    {#if scope === 'some'}
      <div class="project-list">
        {#each app.projects as p (p.key)}
          <label class="check">
            <input
              type="checkbox"
              checked={projects.has(p.key)}
              onchange={(e) => {
                const next = new Set(projects)
                if (e.currentTarget.checked) next.add(p.key)
                else next.delete(p.key)
                projects = next
              }}
            />
            {p.key} — {p.name}
          </label>
        {/each}
      </div>
    {/if}
  </div>

  <label class="check"><input type="checkbox" bind:checked={readOnly} /> Read-only (can view, never change anything)</label>

  <label class="field">
    <span class="field-label">Expires</span>
    <select class="select narrow" bind:value={expiry}>
      <option value="30">In 30 days</option>
      <option value="90">In 90 days</option>
      <option value="365">In 1 year</option>
      <option value="never">Never</option>
    </select>
  </label>

  {#if error}<p class="error">{error}</p>{/if}
  <div class="row">
    <button class="btn btn-primary" disabled={!name.trim() || creating} onclick={create}>
      <Icon name="plus" size={14} /> Create token
    </button>
  </div>
</div>

{#if justCreated}
  <div class="reveal">
    <h3><Icon name="check" size={15} /> Token created</h3>
    <p class="muted">This is shown once. Copy it now — the board only ever stores its hash.</p>
    <div class="secret">
      <code>{justCreated.token}</code>
      <button class="btn btn-sm" onclick={() => copy(justCreated!.token)}>Copy</button>
    </div>
    <p class="muted small">Paste it into an MCP client's config, e.g.:</p>
    <div class="secret">
      <pre>{mcpConfig}</pre>
      <button class="btn btn-sm" onclick={() => copy(mcpConfig)}>Copy</button>
    </div>
    <button class="btn btn-ghost btn-sm" onclick={() => (justCreated = null)}>Done</button>
  </div>
{/if}

<style>
  h2 {
    margin: 0 0 6px;
    font-size: 18px;
  }
  h3 {
    margin: 28px 0 10px;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 14px;
  }
  .lead {
    margin: 0 0 18px;
    color: var(--text-2);
  }
  .owner-picker {
    display: block;
    max-width: 320px;
    margin-bottom: 18px;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 4px;
    border-bottom: 1px solid var(--border);
  }
  .row.dim {
    opacity: 0.6;
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .line1 {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .hint {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text-3);
  }
  .chip.danger {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .line2 {
    font-size: 12px;
    margin-top: 2px;
  }
  .add-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 480px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .radio,
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .radio input,
  .check input {
    accent-color: var(--accent);
  }
  .project-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-left: 22px;
    padding: 6px 0;
  }
  .narrow {
    width: 160px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 13px;
  }
  .reveal {
    margin-top: 24px;
    padding: 16px;
    border: 1px solid var(--success);
    border-radius: 10px;
    background: color-mix(in srgb, var(--success) 10%, var(--surface));
    max-width: 560px;
  }
  .secret {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 8px 0;
    padding: 10px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .secret code {
    flex: 1;
    font-family: var(--mono);
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .secret pre {
    flex: 1;
    margin: 0;
    font-family: var(--mono);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .small {
    font-size: 12px;
  }
</style>
