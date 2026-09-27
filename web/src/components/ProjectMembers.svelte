<script lang="ts">
  import { api, ApiError } from '../lib/api'
  import { app } from '../lib/store.svelte'
  import { confirmer, toasts } from '../lib/toast.svelte'
  import type { ProjectMember, Role } from '../lib/types'
  import Avatar from './Avatar.svelte'
  import Icon from './Icon.svelte'
  import UserPicker from './UserPicker.svelte'

  let { projectKey }: { projectKey: string } = $props()

  let members = $state<ProjectMember[]>([])
  let error = $state<string | null>(null)

  async function load() {
    try {
      members = await api.members(projectKey)
      error = null
    } catch (e) {
      error = e instanceof ApiError ? e.message : String(e)
    }
  }
  $effect(() => {
    void projectKey
    load()
  })

  async function setRole(userId: number, role: Role) {
    try {
      members = await api.setMember(projectKey, userId, role)
    } catch (e) {
      toasts.error(e)
    }
  }

  async function remove(m: ProjectMember) {
    const ok = await confirmer.ask(`Remove ${m.display_name} from ${projectKey}?`, 'They will no longer see this project (unless they are a site admin).', {
      confirm: 'Remove',
      danger: true,
    })
    if (!ok) return
    try {
      members = await api.removeMember(projectKey, m.user_id)
    } catch (e) {
      toasts.error(e)
    }
  }

  async function add(userId: number) {
    try {
      members = await api.setMember(projectKey, userId, 'member')
    } catch (e) {
      toasts.error(e)
    }
  }

  const memberIds = $derived(members.map((m) => m.user_id))
</script>

<div class="members">
  {#if error}
    <p class="muted small">{error}</p>
  {:else}
    {#each members as m (m.user_id)}
      <div class="row">
        <Avatar userId={m.user_id} size={22} />
        <span class="name">{m.display_name}</span>
        <span class="muted small">@{m.username} · {m.kind}</span>
        <span class="spacer"></span>
        <select class="select role" value={m.role} onchange={(e) => setRole(m.user_id, e.currentTarget.value as Role)}>
          <option value="viewer">Viewer</option>
          <option value="member">Member</option>
          <option value="admin">Admin</option>
        </select>
        <button class="icon-btn" title="Remove" onclick={() => remove(m)}><Icon name="x" size={14} /></button>
      </div>
    {/each}
    <div class="add">
      <UserPicker value={null} allowNone={false} label="Add someone" exclude={memberIds} onpick={(id) => id !== null && add(id)} />
    </div>
  {/if}
</div>

<style>
  .members {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 0 4px 14px;
    border-left: 2px solid var(--border);
    margin: 4px 0 10px 14px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
  .role {
    width: 100px;
  }
  .small {
    font-size: 12px;
  }
  .add {
    margin-top: 4px;
    max-width: 220px;
  }
</style>
