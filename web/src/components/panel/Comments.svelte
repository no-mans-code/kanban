<script lang="ts">
  import { api } from '../../lib/api'
  import { fullTime, relativeTime } from '../../lib/format'
  import { app } from '../../lib/store.svelte'
  import { confirmer, toasts } from '../../lib/toast.svelte'
  import type { Comment } from '../../lib/types'
  import Avatar from '../Avatar.svelte'
  import Markdown from '../Markdown.svelte'

  let { ticketKey, comments, onchange }: { ticketKey: string; comments: Comment[]; onchange: () => void } = $props()

  let draft = $state('')
  let posting = $state(false)
  let editing = $state<number | null>(null)
  let editText = $state('')

  async function post() {
    const body = draft.trim()
    if (!body || posting) return
    posting = true
    try {
      await api.createComment(ticketKey, body)
      draft = ''
      onchange()
    } catch (e) {
      toasts.error(e)
    } finally {
      posting = false
    }
  }

  async function saveEdit(c: Comment) {
    try {
      await api.updateComment(c.id, editText)
      editing = null
      onchange()
    } catch (e) {
      toasts.error(e)
    }
  }

  async function remove(c: Comment) {
    if (!(await confirmer.ask('Delete comment?', 'This cannot be undone.', { confirm: 'Delete', danger: true }))) return
    try {
      await api.deleteComment(c.id)
      onchange()
    } catch (e) {
      toasts.error(e)
    }
  }

  function name(id: number | null) {
    return id === null ? 'System' : (app.usersById.get(id)?.display_name ?? 'Unknown')
  }
</script>

<div class="composer">
  <Avatar userId={app.me?.id ?? null} size={28} />
  <div class="box">
    <textarea
      class="textarea"
      placeholder="Add a comment. Markdown works; Ctrl+Enter to send."
      bind:value={draft}
      rows="3"
      onkeydown={(e) => {
        if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) post()
      }}
    ></textarea>
    {#if draft.trim()}
      <div class="actions">
        <button class="btn btn-ghost btn-sm" onclick={() => (draft = '')}>Cancel</button>
        <button class="btn btn-primary btn-sm" onclick={post} disabled={posting}>Comment</button>
      </div>
    {/if}
  </div>
</div>

<ol class="list">
  {#each [...comments].reverse() as c (c.id)}
    <li>
      <Avatar userId={c.author_id} size={28} />
      <div class="body">
        <div class="head">
          <strong>{name(c.author_id)}</strong>
          <span class="muted" title={fullTime(c.created_at)}>{relativeTime(c.created_at)}</span>
          {#if c.updated_at !== c.created_at}<span class="muted" title={fullTime(c.updated_at)}>(edited)</span>{/if}
          <span class="spacer"></span>
          {#if editing !== c.id}
            <button class="link" onclick={() => ((editing = c.id), (editText = c.body))}>Edit</button>
            <button class="link" onclick={() => remove(c)}>Delete</button>
          {/if}
        </div>
        {#if editing === c.id}
          <!-- svelte-ignore a11y_autofocus -->
          <textarea
            class="textarea"
            bind:value={editText}
            autofocus
            onkeydown={(e) => {
              if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) saveEdit(c)
              if (e.key === 'Escape') {
                e.stopPropagation()
                editing = null
              }
            }}
          ></textarea>
          <div class="actions">
            <button class="btn btn-ghost btn-sm" onclick={() => (editing = null)}>Cancel</button>
            <button class="btn btn-primary btn-sm" onclick={() => saveEdit(c)}>Save</button>
          </div>
        {:else}
          <Markdown source={c.body} />
        {/if}
      </div>
    </li>
  {:else}
    <li class="none muted">No comments yet.</li>
  {/each}
</ol>

<style>
  .composer,
  li {
    display: flex;
    gap: 10px;
  }
  .box,
  .body {
    flex: 1;
    min-width: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 6px;
  }
  .list {
    list-style: none;
    margin: 18px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin-bottom: 4px;
    font-size: 13px;
  }
  .head .muted {
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    font-size: 12px;
    color: var(--text-3);
    cursor: pointer;
  }
  .link:hover {
    color: var(--text);
  }
  .none {
    padding-left: 38px;
    font-size: 13px;
  }
</style>
