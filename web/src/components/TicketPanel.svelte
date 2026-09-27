<script lang="ts">
  import { untrack } from 'svelte'
  import { api, type TicketPatch } from '../lib/api'
  import { capitalize, fullTime, relativeTime } from '../lib/format'
  import { projectUrl, router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'
  import {
    PRIORITIES,
    TYPES,
    type Activity,
    type Comment,
    type ProjectDetail,
    type TicketDetail,
  } from '../lib/types'
  import Avatar from './Avatar.svelte'
  import DeleteDialog from './DeleteDialog.svelte'
  import Icon from './Icon.svelte'
  import LabelPicker from './LabelPicker.svelte'
  import Markdown from './Markdown.svelte'
  import Children from './panel/Children.svelte'
  import Comments from './panel/Comments.svelte'
  import History from './panel/History.svelte'
  import Links from './panel/Links.svelte'
  import PriorityIcon from './PriorityIcon.svelte'
  import TicketPicker from './TicketPicker.svelte'
  import TypeIcon from './TypeIcon.svelte'
  import UserPicker from './UserPicker.svelte'

  let { ticketKey }: { ticketKey: string } = $props()

  let detail = $state<TicketDetail | null>(null)
  let comments = $state<Comment[]>([])
  let activity = $state<Activity[]>([])
  let project = $state<ProjectDetail | null>(null)
  let missing = $state(false)
  let tab = $state<'comments' | 'history'>('comments')
  let editingDesc = $state(false)
  let descDraft = $state('')
  let titleDraft = $state('')
  let titleFocused = $state(false)
  let pickingParent = $state(false)
  let deleting = $state(false)

  const projectKey = $derived(ticketKey.slice(0, ticketKey.lastIndexOf('-')).toUpperCase())
  const blockers = $derived(
    (detail?.links ?? []).filter(
      (l) => l.kind === 'blocks' && l.direction === 'inward' && l.ticket.status_category !== 'done',
    ),
  )
  const watching = $derived(app.actorId !== null && (detail?.watcher_ids.includes(app.actorId) ?? false))

  async function load(key: string) {
    try {
      const [d, c, a] = await Promise.all([api.ticket(key), api.comments(key), api.activity(key)])
      if (key !== ticketKey) return
      detail = d
      comments = c
      activity = a
      missing = false
      if (!titleFocused) titleDraft = d.title
    } catch (e) {
      if (key === ticketKey) {
        missing = true
        detail = null
      }
    }
  }

  $effect(() => {
    load(ticketKey)
  })

  /** Tickets whose changes show up in this panel: itself, its parent, children and links. */
  function shown(key: string): Set<string> {
    const keys = new Set([key])
    if (detail?.key === key) {
      if (detail.parent_key) keys.add(detail.parent_key)
      for (const c of detail.children) keys.add(c.key)
      for (const l of detail.links) keys.add(l.ticket.key)
    }
    return keys
  }

  // Live updates: refetch only when a change batch touches something shown here.
  let seenVersion = app.ticketsVersion
  $effect(() => {
    const version = app.ticketsVersion
    if (version === seenVersion) return
    seenVersion = version
    untrack(() => {
      const changed = app.changedKeys
      const relevant = shown(ticketKey)
      if (changed === null || [...changed].some((k) => relevant.has(k))) load(ticketKey)
    })
  })

  $effect(() => {
    const key = projectKey
    if (app.project?.key === key) project = app.project
    else api.project(key).then((p) => (project = p), () => (project = null))
  })

  $effect(() => {
    // Reset per-ticket UI state when switching tickets.
    void ticketKey
    editingDesc = false
    pickingParent = false
    tab = 'comments'
  })

  async function patch(body: TicketPatch) {
    if (!detail) return
    const key = detail.key
    try {
      const saved = await api.updateTicket(key, body)
      if (key === ticketKey) {
        detail = saved
        activity = await api.activity(key)
      }
    } catch (e) {
      toasts.error(e)
      load(key)
    }
  }

  function saveTitle() {
    titleFocused = false
    const t = titleDraft.trim()
    if (detail && t && t !== detail.title) patch({ title: t })
    else if (detail) titleDraft = detail.title
  }

  function saveDescription() {
    editingDesc = false
    if (detail && descDraft !== detail.description) patch({ description: descDraft })
  }

  async function setWatching(userId: number, on: boolean) {
    if (!detail) return
    try {
      detail.watcher_ids = on
        ? await api.addWatcher(detail.key, userId)
        : await api.removeWatcher(detail.key, userId)
    } catch (e) {
      toasts.error(e)
    }
  }

  function toggleWatch() {
    if (app.actorId !== null) setWatching(app.actorId, !watching)
  }

  function copyLink() {
    const url = `${location.origin}${projectUrl(projectKey)}?ticket=${ticketKey}`
    navigator.clipboard.writeText(url).then(() => toasts.show('Link copied', 'success', 2000))
  }

  function autosize(el: HTMLTextAreaElement) {
    const fit = () => {
      el.style.height = 'auto'
      el.style.height = `${el.scrollHeight}px`
    }
    fit()
    el.addEventListener('input', fit)
    return { destroy: () => el.removeEventListener('input', fit) }
  }
</script>

<aside class="panel" aria-label="Ticket {ticketKey}">
  <header>
    <nav class="crumbs">
      <a href={projectUrl(projectKey)}>{projectKey}</a>
      {#if detail?.parent_key}
        <Icon name="chevron" size={12} />
        <a href="?ticket={detail.parent_key}">{detail.parent_key}</a>
      {/if}
      <Icon name="chevron" size={12} />
      {#if detail}<TypeIcon type={detail.type} size={14} />{/if}
      <span class="key">{ticketKey}</span>
    </nav>
    <span class="spacer"></span>
    {#if detail}
      <button class="btn btn-ghost btn-sm" class:on={watching} onclick={toggleWatch} title={watching ? 'Stop watching' : 'Watch'}>
        <Icon name="eye" size={14} />{detail.watcher_ids.length}
      </button>
      <button class="icon-btn" title="Copy link" onclick={copyLink}><Icon name="copy" size={15} /></button>
      <button class="icon-btn" title="Delete ticket" onclick={() => (deleting = true)}><Icon name="trash" size={15} /></button>
    {/if}
    <button class="icon-btn" title="Close (Esc)" onclick={() => router.closeTicket()}><Icon name="x" size={16} /></button>
  </header>

  {#if missing}
    <div class="missing">
      <p>{ticketKey} doesn't exist, or was deleted.</p>
      <button class="btn" onclick={() => router.closeTicket()}>Close</button>
    </div>
  {:else if detail}
    <div class="scroll">
      <div class="main">
        <textarea
          class="title"
          rows="1"
          bind:value={titleDraft}
          use:autosize
          onfocus={() => (titleFocused = true)}
          onblur={saveTitle}
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              e.currentTarget.blur()
            }
            if (e.key === 'Escape' && detail) {
              e.stopPropagation()
              titleDraft = detail.title
              e.currentTarget.blur()
            }
          }}
          aria-label="Title"
        ></textarea>

        {#if blockers.length}
          <div class="blocked">
            <Icon name="lock" size={14} />
            <span>Blocked by</span>
            {#each blockers as b (b.ticket.key)}
              <a href="?ticket={b.ticket.key}" class="key">{b.ticket.key}</a>
              <span class="muted">({b.ticket.status_name})</span>
            {/each}
          </div>
        {/if}

        <section>
          <h3>Description</h3>
          {#if editingDesc}
            <!-- svelte-ignore a11y_autofocus -->
            <textarea
              class="textarea desc"
              bind:value={descDraft}
              autofocus
              placeholder="Markdown supported. Ticket keys like {ticketKey} become links."
              onkeydown={(e) => {
                if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) saveDescription()
                if (e.key === 'Escape') {
                  e.stopPropagation()
                  editingDesc = false
                }
              }}
            ></textarea>
            <div class="row-end">
              <span class="muted small">Ctrl+Enter to save</span>
              <button class="btn btn-ghost btn-sm" onclick={() => (editingDesc = false)}>Cancel</button>
              <button class="btn btn-primary btn-sm" onclick={saveDescription}>Save</button>
            </div>
          {:else}
            <div
              class="desc-view"
              role="button"
              tabindex="0"
              onclick={(e) => {
                if ((e.target as HTMLElement).closest('a')) return
                descDraft = detail?.description ?? ''
                editingDesc = true
              }}
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  descDraft = detail?.description ?? ''
                  editingDesc = true
                }
              }}
            >
              {#if detail.description.trim()}
                <Markdown source={detail.description} />
              {:else}
                <span class="muted">Add a description…</span>
              {/if}
            </div>
          {/if}
        </section>

        <Children {detail} onchange={() => load(ticketKey)} />
        <Links {detail} onchange={() => load(ticketKey)} />

        <section>
          <div class="tabs" role="tablist">
            <button role="tab" aria-selected={tab === 'comments'} class:on={tab === 'comments'} onclick={() => (tab = 'comments')}>
              Comments <span class="muted">{comments.length}</span>
            </button>
            <button role="tab" aria-selected={tab === 'history'} class:on={tab === 'history'} onclick={() => (tab = 'history')}>
              History
            </button>
          </div>
          {#if tab === 'comments'}
            <Comments ticketKey={detail.key} {comments} onchange={() => load(ticketKey)} />
          {:else}
            <History items={activity} />
          {/if}
        </section>
      </div>

      <div class="side">
        <div class="field">
          <span class="field-label">Status</span>
          <select
            class="status cat-{detail.status_category}"
            value={detail.status_id}
            onchange={(e) => patch({ status_id: Number(e.currentTarget.value) })}
            aria-label="Status"
          >
            {#each project?.statuses ?? [] as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
          </select>
        </div>

        <div class="field">
          <span class="field-label">Assignee</span>
          <UserPicker value={detail.assignee_id} onpick={(id) => patch({ assignee_id: id })} />
        </div>

        <div class="field">
          <span class="field-label">Reporter</span>
          <div class="static">
            <Avatar userId={detail.reporter_id} size={22} />
            {detail.reporter_id === null ? 'System' : app.usersById.get(detail.reporter_id)?.display_name}
          </div>
        </div>

        <div class="field">
          <span class="field-label">Priority</span>
          <div class="with-icon">
            <PriorityIcon priority={detail.priority} />
            <select class="select bare" value={detail.priority} onchange={(e) => patch({ priority: e.currentTarget.value })} aria-label="Priority">
              {#each PRIORITIES as p (p)}<option value={p}>{capitalize(p)}</option>{/each}
            </select>
          </div>
        </div>

        <div class="field">
          <span class="field-label">Type</span>
          <div class="with-icon">
            <TypeIcon type={detail.type} />
            <select class="select bare" value={detail.type} onchange={(e) => patch({ type: e.currentTarget.value })} aria-label="Type">
              {#each TYPES as t (t)}<option value={t}>{capitalize(t)}</option>{/each}
            </select>
          </div>
        </div>

        {#if detail.type !== 'epic'}
          <div class="field">
            <span class="field-label">Parent</span>
            {#if pickingParent}
              <TicketPicker
                autofocus
                placeholder={detail.type === 'subtask' ? 'Find a story, task or bug' : 'Find an epic'}
                exclude={[detail.key]}
                filter={(t) =>
                  t.key.startsWith(`${projectKey}-`) &&
                  (detail?.type === 'subtask' ? ['story', 'task', 'bug'].includes(t.type) : t.type === 'epic')}
                onpick={(t) => {
                  pickingParent = false
                  patch({ parent: t.key })
                }}
              />
            {:else}
              <div class="static">
                {#if detail.parent_key}
                  <a href="?ticket={detail.parent_key}" class="key">{detail.parent_key}</a>
                  <button class="icon-btn tiny" title="Change parent" onclick={() => (pickingParent = true)}><Icon name="edit" size={13} /></button>
                  {#if detail.type !== 'subtask'}
                    <button class="icon-btn tiny" title="Remove parent" onclick={() => patch({ parent: null })}><Icon name="x" size={13} /></button>
                  {/if}
                {:else}
                  <button class="link" onclick={() => (pickingParent = true)}>Set parent</button>
                {/if}
              </div>
            {/if}
          </div>
        {/if}

        <div class="field">
          <span class="field-label">Labels</span>
          <LabelPicker value={detail.label_ids} onchange={(ids) => patch({ label_ids: ids })} />
        </div>

        <div class="field">
          <span class="field-label">Watchers</span>
          <div class="watchers">
            {#each detail.watcher_ids as id (id)}
              <span class="watcher">
                <Avatar userId={id} size={22} />
                <button
                  class="unwatch"
                  title="Remove {app.usersById.get(id)?.display_name}"
                  onclick={() => setWatching(id, false)}><Icon name="x" size={10} stroke={3} /></button
                >
              </span>
            {/each}
            <UserPicker
              value={null}
              allowNone={false}
              label="Add"
              exclude={detail.watcher_ids}
              onpick={(id) => id !== null && setWatching(id, true)}
            />
          </div>
        </div>

        <dl class="dates">
          <dt>Created</dt>
          <dd title={fullTime(detail.created_at)}>{relativeTime(detail.created_at)}</dd>
          <dt>Updated</dt>
          <dd title={fullTime(detail.updated_at)}>{relativeTime(detail.updated_at)}</dd>
          {#if detail.resolved_at}
            <dt>Resolved</dt>
            <dd title={fullTime(detail.resolved_at)}>{relativeTime(detail.resolved_at)}</dd>
          {/if}
        </dl>
      </div>
    </div>
  {:else}
    <div class="loading"><div class="shimmer"></div><div class="shimmer short"></div></div>
  {/if}
</aside>

{#if deleting}
  <DeleteDialog {ticketKey} onclose={() => (deleting = false)} />
{/if}

<style>
  .panel {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 30;
    width: min(920px, 100vw);
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border-left: 1px solid var(--border-strong);
    box-shadow: var(--shadow);
    animation: slide 0.14s ease-out;
  }
  @keyframes slide {
    from {
      transform: translateX(24px);
      opacity: 0;
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: 4px;
    height: var(--topbar-h);
    padding: 0 12px 0 20px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .crumbs {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-3);
  }
  .crumbs a {
    color: var(--text-2);
  }
  .spacer {
    flex: 1;
  }
  header .btn.on {
    color: var(--accent);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
  }
  .main {
    flex: 1 1 520px;
    min-width: 0;
    padding: 20px 24px 40px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  .side {
    flex: 0 0 280px;
    position: sticky;
    top: 0;
    padding: 20px 20px 40px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  @media (max-width: 860px) {
    .side {
      position: static;
      flex-basis: 100%;
      border-top: 1px solid var(--border);
    }
  }
  .title {
    width: 100%;
    margin: 0 0 -10px -8px;
    padding: 4px 8px;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    font-size: 21px;
    font-weight: 600;
    line-height: 1.3;
    resize: none;
    overflow: hidden;
  }
  .title:hover {
    background: var(--surface-2);
  }
  .title:focus {
    outline: none;
    border-color: var(--accent);
    background: var(--surface);
  }
  .blocked {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    background: var(--danger-soft);
    color: var(--danger);
    font-size: 13px;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 13px;
    font-weight: 600;
  }
  .desc-view {
    min-height: 40px;
    margin-left: -8px;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    cursor: text;
  }
  .desc-view:hover {
    background: var(--surface-2);
  }
  .desc {
    min-height: 160px;
  }
  .row-end {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 6px;
  }
  .small {
    font-size: 12px;
    margin-right: auto;
  }
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 14px;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    padding: 6px 10px;
    margin-bottom: -1px;
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--text-2);
    font-weight: 500;
    cursor: pointer;
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .field .static,
  .with-icon {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    font-size: 13px;
  }
  .status {
    height: 30px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    border: 1px solid color-mix(in srgb, var(--cat) 50%, transparent);
    background: color-mix(in srgb, var(--cat) 16%, transparent);
    color: var(--text);
    font-weight: 600;
    font-size: 13px;
    cursor: pointer;
  }
  .status option,
  .bare option {
    background: var(--surface);
    color: var(--text);
  }
  .bare {
    border-color: transparent;
    background-color: transparent;
    padding-left: 0;
    height: 30px;
  }
  .bare:hover {
    background-color: var(--surface-2);
  }
  .tiny {
    width: 22px;
    height: 22px;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font-size: 13px;
  }
  .watchers {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .watcher {
    position: relative;
    line-height: 0;
  }
  .unwatch {
    position: absolute;
    top: -4px;
    right: -4px;
    display: none;
    place-items: center;
    width: 14px;
    height: 14px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: var(--danger);
    color: #fff;
    cursor: pointer;
  }
  .watcher:hover .unwatch {
    display: grid;
  }
  .dates {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 12px;
    margin: 8px 0 0;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 12px;
  }
  dt {
    color: var(--text-3);
  }
  dd {
    margin: 0;
    color: var(--text-2);
  }
  .missing,
  .loading {
    padding: 40px 24px;
  }
  .shimmer {
    height: 24px;
    margin-bottom: 12px;
    border-radius: 6px;
    background: linear-gradient(90deg, var(--surface) 0%, var(--surface-2) 50%, var(--surface) 100%);
    background-size: 200% 100%;
    animation: shimmer 1s infinite linear;
  }
  .shimmer.short {
    width: 60%;
  }
  @keyframes shimmer {
    to {
      background-position: -200% 0;
    }
  }
</style>
