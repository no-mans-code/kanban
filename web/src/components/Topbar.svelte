<script lang="ts">
  import { router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import Icon from './Icon.svelte'
  import TicketPicker from './TicketPicker.svelte'
  import UserPicker from './UserPicker.svelte'

  let { title, oncreate, onhelp }: { title: string; oncreate: () => void; onhelp: () => void } = $props()
</script>

<header class="topbar">
  <h1>{title}</h1>
  <div class="search">
    <TicketPicker id="global-search" placeholder="Search everything   ( / )" onpick={(t) => router.openTicket(t.key)} />
  </div>
  <span class="spacer"></span>
  {#if !app.connected}
    <span class="offline" title="Live updates paused. Reconnecting…"><span class="dot"></span>Offline</span>
  {/if}
  <button class="btn btn-primary" onclick={oncreate} disabled={app.projects.length === 0}>
    <Icon name="plus" size={15} stroke={2.2} /> Create <span class="kbd on-accent">C</span>
  </button>
  <button class="icon-btn" title="Keyboard shortcuts (?)" onclick={onhelp}><Icon name="keyboard" size={17} /></button>
  <button class="icon-btn" title="Switch to {app.theme === 'dark' ? 'light' : 'dark'} mode (T)" onclick={() => app.toggleTheme()}>
    <Icon name={app.theme === 'dark' ? 'sun' : 'moon'} size={17} />
  </button>
  <div class="actor" title="Who your changes are attributed to">
    <span class="as muted">Acting as</span>
    <UserPicker value={app.actorId} allowNone={false} label="Pick someone" onpick={(id) => app.setActor(id)} />
  </div>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--topbar-h);
    padding: 0 16px 0 20px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    white-space: nowrap;
  }
  .search {
    display: flex;
    width: 320px;
    margin-left: 12px;
  }
  .spacer {
    flex: 1;
  }
  .offline {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--warn);
  }
  .offline .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  .on-accent {
    border-color: rgba(255, 255, 255, 0.35);
    color: rgba(255, 255, 255, 0.85);
    min-width: 16px;
    padding: 0 3px;
    font-size: 10px;
  }
  .actor {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-left: 10px;
    margin-left: 2px;
    border-left: 1px solid var(--border);
    font-size: 13px;
  }
  .as {
    font-size: 12px;
    white-space: nowrap;
  }
  .actor :global(.panel) {
    left: auto;
    right: 0;
  }
  @media (max-width: 900px) {
    .search,
    .as {
      display: none;
    }
  }
</style>
