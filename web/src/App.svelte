<script lang="ts">
  import { onMount } from 'svelte'
  import BoardView from './components/BoardView.svelte'
  import CreateTicket from './components/CreateTicket.svelte'
  import ListView from './components/ListView.svelte'
  import Overlays from './components/Overlays.svelte'
  import SettingsView from './components/SettingsView.svelte'
  import LoginView from './components/LoginView.svelte'
  import SetupView from './components/SetupView.svelte'
  import Sidebar from './components/Sidebar.svelte'
  import TicketPanel from './components/TicketPanel.svelte'
  import Topbar from './components/Topbar.svelte'
  import Welcome from './components/Welcome.svelte'
  import { filters } from './lib/filters.svelte'
  import { projectUrl, router, type View } from './lib/router.svelte'
  import { app } from './lib/store.svelte'
  import { toasts } from './lib/toast.svelte'

  // The graph view carries the ELK layout engine (~1.4MB); load it only when opened.
  let graphView: Promise<typeof import('./components/GraphView.svelte').default> | undefined
  function loadGraphView() {
    graphView ??= import('./components/GraphView.svelte').then((m) => m.default)
    return graphView
  }

  let showCreate = $state(false)
  let showShortcuts = $state(false)
  let failed = $state<string | null>(null)

  const route = $derived(router.route)

  onMount(() => {
    app.checkAuth().catch((e) => (failed = e instanceof Error ? e.message : String(e)))
  })

  // Open the project named in the URL; send "/" to the first project.
  $effect(() => {
    if (!app.ready) return
    if (route.name === 'project') {
      app.openProject(route.project).catch(() => {
        toasts.show(`There is no project ${route.project}`, 'error')
        router.navigate('/', true)
      })
    } else if (route.name === 'home' && app.projects.length > 0) {
      router.navigate(projectUrl(app.projects[0].key), true)
    }
  })

  // Filters belong to one project; don't carry them into another.
  $effect(() => {
    void (route.name === 'project' ? route.project : null)
    filters.clear()
  })

  const title = $derived.by(() => {
    if (route.name === 'settings') return 'Settings'
    if (route.name === 'project') {
      const name = app.projects.find((p) => p.key === route.project)?.name ?? route.project
      const view = { board: 'Board', list: 'List', graph: 'Dependencies' }[route.view]
      return `${name} · ${view}`
    }
    return 'Kanban'
  })

  $effect(() => {
    document.title = router.ticket ? `${router.ticket} · ${title}` : title
  })

  function currentProject(): string | null {
    if (route.name === 'project') return route.project
    return app.project?.key ?? app.projects[0]?.key ?? null
  }

  let chord = false
  let chordTimer: ReturnType<typeof setTimeout> | undefined

  function go(view: View) {
    const key = currentProject()
    if (key) router.navigate(projectUrl(key, view))
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return
    const el = e.target as HTMLElement | null
    if (el?.closest('input, textarea, select, [contenteditable="true"]')) return
    if (document.querySelector('[aria-modal="true"], [data-popover]')) return

    if (chord) {
      chord = false
      clearTimeout(chordTimer)
      const target = { b: 'board', l: 'list', d: 'graph' }[e.key] as View | undefined
      if (target) go(target)
      else if (e.key === 's') router.navigate('/settings/general')
      e.preventDefault()
      return
    }
    switch (e.key) {
      case 'c':
        if (app.projects.length) showCreate = true
        break
      case '/':
        document.getElementById('global-search')?.focus()
        break
      case 'f':
        document.getElementById('filter-search')?.focus()
        break
      case 'g':
        chord = true
        chordTimer = setTimeout(() => (chord = false), 1200)
        break
      case 't':
        app.toggleTheme()
        break
      case '?':
        showShortcuts = true
        break
      case 'Escape':
        if (router.ticket) router.closeTicket()
        break
      default:
        return
    }
    e.preventDefault()
  }

  // Same-origin links navigate in place instead of reloading the page.
  function onClick(e: MouseEvent) {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
    const a = (e.target as Element | null)?.closest('a')
    if (!a || a.target === '_blank' || a.hasAttribute('download')) return
    const url = new URL(a.href, location.href)
    if (url.origin !== location.origin) return
    e.preventDefault()
    const ticket = url.searchParams.get('ticket')
    if (a.getAttribute('href')?.startsWith('?') && ticket) router.openTicket(ticket)
    else router.navigate(url.pathname + url.search)
  }
</script>

<svelte:window onkeydown={onKeydown} />
<svelte:document onclick={onClick} />

{#if failed}
  <div class="notice">
    <h2>Can't reach the board server</h2>
    <p class="muted">{failed}</p>
    <button class="btn" onclick={() => location.reload()}>Retry</button>
  </div>
{:else if !app.authChecked}
  <div class="notice muted">Loading…</div>
{:else if app.setupRequired}
  <SetupView />
{:else if !app.me}
  <LoginView />
{:else}
  <div class="app">
    <Sidebar />
    <main>
      <Topbar {title} oncreate={() => (showCreate = true)} onhelp={() => (showShortcuts = true)} />
      {#if !app.ready}
        <div class="notice muted">Loading…</div>
      {:else if route.name === 'settings'}
        <SettingsView tab={route.tab} />
      {:else if route.name === 'project' && app.project?.key === route.project}
        {#if route.view === 'board'}
          <BoardView project={app.project} />
        {:else if route.view === 'list'}
          <ListView project={app.project} />
        {:else}
          {#await loadGraphView() then GraphView}
            <GraphView project={app.project} />
          {:catch e}
            <div class="notice muted">Couldn't load the graph view: {e}</div>
          {/await}
        {/if}
      {:else if app.projects.length === 0}
        <Welcome />
      {/if}
    </main>
  </div>

  {#if router.ticket}
    <TicketPanel ticketKey={router.ticket} />
  {/if}
  {#if showCreate}
    <CreateTicket onclose={() => (showCreate = false)} />
  {/if}
  <Overlays bind:showShortcuts />
{/if}

<style>
  .app {
    display: flex;
    height: 100%;
  }
  main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .notice {
    margin: 15vh auto 0;
    text-align: center;
  }
</style>
