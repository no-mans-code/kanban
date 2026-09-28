<script lang="ts">
  import { api } from '../lib/api'
  import { capitalize, dateInputToMs, dateInputValue } from '../lib/format'
  import { router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { toasts } from '../lib/toast.svelte'
  import { PRIORITIES, TYPES, type ProjectDetail, type TicketSummary, type TicketType } from '../lib/types'
  import LabelPicker from './LabelPicker.svelte'
  import Modal from './Modal.svelte'
  import TicketPicker from './TicketPicker.svelte'
  import UserPicker from './UserPicker.svelte'

  let { onclose }: { onclose: () => void } = $props()

  let projectKey = $state(app.project?.key ?? app.projects[0]?.key ?? '')
  let project = $state<ProjectDetail | null>(app.project)
  let type = $state<TicketType>('task')
  let title = $state('')
  let description = $state('')
  let priority = $state('medium')
  let statusId = $state<number | null>(null)
  let assignee = $state<number | null>(null)
  let parent = $state<TicketSummary | null>(null)
  let labels = $state<number[]>([])
  let dueDate = $state<number | null>(null)
  let componentId = $state<number | null>(null)
  let fixVersionId = $state<number | null>(null)
  let another = $state(false)
  let saving = $state(false)

  $effect(() => {
    const key = projectKey
    if (!key) return
    if (project?.key !== key) api.project(key).then((p) => (project = p), toasts.error.bind(toasts))
  })

  $effect(() => {
    // Components/versions are per-project: a picked one may no longer apply.
    void projectKey
    componentId = null
    fixVersionId = null
  })

  const needsParent = $derived(type === 'subtask')
  const parentAllowed = $derived(type !== 'epic')

  function parentFits(t: TicketSummary) {
    if (!t.key.startsWith(`${projectKey}-`)) return false
    return type === 'subtask' ? ['story', 'task', 'bug'].includes(t.type) : t.type === 'epic'
  }

  $effect(() => {
    // Changing type can invalidate the chosen parent.
    void type
    if (parent && !parentFits(parent)) parent = null
  })

  async function submit() {
    if (!title.trim() || saving) return
    if (needsParent && !parent) {
      toasts.show('A subtask needs a parent ticket', 'error')
      return
    }
    saving = true
    try {
      const t = await api.createTicket({
        project: projectKey,
        type,
        title: title.trim(),
        description,
        priority,
        status_id: statusId ?? undefined,
        assignee_id: assignee,
        parent: parentAllowed ? (parent?.key ?? null) : null,
        label_ids: labels,
        due_date: dueDate,
        component_id: componentId,
        fix_version_id: fixVersionId,
      })
      toasts.show(`Created ${t.key}`, 'success')
      if (another) {
        title = ''
        description = ''
        dueDate = null
        componentId = null
        fixVersionId = null
      } else {
        onclose()
        router.openTicket(t.key)
      }
    } catch (e) {
      toasts.error(e)
    } finally {
      saving = false
    }
  }
</script>

<Modal title="Create ticket" {onclose} width={640}>
  <div
    class="form"
    role="presentation"
    onkeydown={(e) => {
      if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) submit()
    }}
  >
    <div class="grid">
      <label>
        <span class="field-label">Project</span>
        <select class="select" bind:value={projectKey}>
          {#each app.projects as p (p.key)}<option value={p.key}>{p.name} ({p.key})</option>{/each}
        </select>
      </label>
      <label>
        <span class="field-label">Type</span>
        <select class="select" bind:value={type}>
          {#each TYPES as t (t)}<option value={t}>{capitalize(t)}</option>{/each}
        </select>
      </label>
    </div>

    <label>
      <span class="field-label">Title</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input class="input" bind:value={title} autofocus placeholder="Short summary" maxlength="300" />
    </label>

    <label>
      <span class="field-label">Description</span>
      <textarea class="textarea" bind:value={description} rows="5" placeholder="Markdown supported"></textarea>
    </label>

    <div class="grid">
      <label>
        <span class="field-label">Status</span>
        <select class="select" bind:value={statusId}>
          <option value={null}>Default ({project?.statuses[0]?.name ?? '…'})</option>
          {#each project?.statuses ?? [] as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
        </select>
      </label>
      <label>
        <span class="field-label">Priority</span>
        <select class="select" bind:value={priority}>
          {#each PRIORITIES as p (p)}<option value={p}>{capitalize(p)}</option>{/each}
        </select>
      </label>
      <div>
        <span class="field-label">Assignee</span>
        <UserPicker value={assignee} onpick={(id) => (assignee = id)} />
      </div>
      <div>
        <span class="field-label">Labels</span>
        <LabelPicker value={labels} onchange={(ids) => (labels = ids)} />
      </div>
      <label>
        <span class="field-label">Due date</span>
        <input
          type="date"
          class="input"
          value={dateInputValue(dueDate)}
          onchange={(e) => (dueDate = dateInputToMs(e.currentTarget.value))}
        />
      </label>
      <label>
        <span class="field-label">Component</span>
        <select class="select" bind:value={componentId}>
          <option value={null}>None</option>
          {#each project?.components ?? [] as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
      </label>
      <label>
        <span class="field-label">Fix version</span>
        <select class="select" bind:value={fixVersionId}>
          <option value={null}>None</option>
          {#each project?.versions ?? [] as v (v.id)}<option value={v.id}>{v.name}</option>{/each}
        </select>
      </label>
    </div>

    {#if parentAllowed}
      <div>
        <span class="field-label">
          Parent {needsParent ? '(required: a story, task or bug)' : '(optional: an epic)'}
        </span>
        {#if parent}
          <div class="parent">
            <span class="key">{parent.key}</span>
            {parent.title}
            <button class="btn btn-ghost btn-sm" onclick={() => (parent = null)}>Change</button>
          </div>
        {:else}
          <TicketPicker
            placeholder={needsParent ? 'Find the parent ticket' : 'Find an epic'}
            filter={parentFits}
            onpick={(t) => (parent = t)}
          />
        {/if}
      </div>
    {/if}
  </div>

  {#snippet footer()}
    <label class="another"><input type="checkbox" bind:checked={another} /> Create another</label>
    <span class="spacer"></span>
    <span class="muted hint">Ctrl+Enter</span>
    <button class="btn btn-ghost" onclick={onclose}>Cancel</button>
    <button class="btn btn-primary" onclick={submit} disabled={!title.trim() || saving || !projectKey}>Create</button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  label {
    display: block;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  .parent {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .another {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-2);
  }
  .another input {
    accent-color: var(--accent);
  }
  .spacer {
    flex: 1;
  }
  .hint {
    font-size: 12px;
  }
</style>
