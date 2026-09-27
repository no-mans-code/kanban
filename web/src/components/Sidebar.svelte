<script lang="ts">
  import { projectUrl, router, type View } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import Icon from './Icon.svelte'

  const route = $derived(router.route)
  const current = $derived(route.name === 'project' ? route.project : null)
  const view = $derived(route.name === 'project' ? route.view : null)

  const VIEWS: [View, string, string][] = [
    ['board', 'Board', 'board'],
    ['list', 'List', 'list'],
    ['graph', 'Dependencies', 'graph'],
  ]
</script>

<nav class="sidebar">
  <a class="brand" href="/">
    <img src="/favicon.svg" alt="" width="22" height="22" />
    <span>Kanban</span>
  </a>

  <div class="section">Projects</div>
  {#each app.projects as p (p.key)}
    <a class="project" class:on={current === p.key} href={projectUrl(p.key, view ?? 'board')}>
      <span class="badge">{p.key.slice(0, 2)}</span>
      <span class="name">{p.name}</span>
    </a>
    {#if current === p.key}
      <div class="views">
        {#each VIEWS as [v, label, icon] (v)}
          <a class="view" class:on={view === v} href={projectUrl(p.key, v)}>
            <Icon name={icon} size={15} />{label}
          </a>
        {/each}
      </div>
    {/if}
  {/each}
  <a class="project add" href="/settings/projects"><Icon name="plus" size={14} /> New project</a>

  <span class="spacer"></span>
  <a class="view" class:on={route.name === 'settings'} href="/settings/general"><Icon name="settings" size={15} />Settings</a>
</nav>

<style>
  .sidebar {
    width: var(--sidebar-w);
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 10px 14px;
    background: var(--bg-elev);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  a {
    color: var(--text-2);
    text-decoration: none;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--topbar-h);
    padding: 0 6px;
    color: var(--text);
    font-weight: 700;
    font-size: 15px;
    letter-spacing: -0.01em;
  }
  .section {
    margin: 12px 8px 6px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .project,
  .view {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 32px;
    padding: 0 8px;
    border-radius: var(--radius-sm);
    font-weight: 500;
  }
  .project:hover,
  .view:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .project.on {
    color: var(--text);
  }
  .view.on {
    background: var(--accent-soft);
    color: var(--text);
  }
  .badge {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    flex: none;
    border-radius: 6px;
    background: var(--surface-3);
    font-size: 10px;
    font-weight: 700;
    color: var(--text);
  }
  .project.on .badge {
    background: var(--accent-strong);
    color: var(--accent-text);
  }
  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .views {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 2px 0 6px 14px;
    padding-left: 10px;
    border-left: 1px solid var(--border);
  }
  .add {
    color: var(--text-3);
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
</style>
