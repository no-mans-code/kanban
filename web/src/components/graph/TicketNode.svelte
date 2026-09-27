<script lang="ts">
  import { Handle, Position, type NodeProps } from '@xyflow/svelte'
  import type { TicketSummary } from '../../lib/types'
  import Avatar from '../Avatar.svelte'
  import Icon from '../Icon.svelte'
  import PriorityIcon from '../PriorityIcon.svelte'
  import TypeIcon from '../TypeIcon.svelte'

  let { data }: NodeProps = $props()
  const ticket = $derived(data.ticket as TicketSummary)
  const onChain = $derived(Boolean(data.onChain))
</script>

<Handle type="target" position={Position.Left} />
<div class="node cat-{ticket.status_category}" class:chain={onChain} class:done={ticket.status_category === 'done'}>
  <div class="top">
    <TypeIcon type={ticket.type} size={14} />
    <span class="key">{ticket.key}</span>
    {#if ticket.is_blocked}<span class="lock" title="Blocked"><Icon name="lock" size={12} /></span>{/if}
    <span class="spacer"></span>
    <PriorityIcon priority={ticket.priority} size={14} />
    <Avatar userId={ticket.assignee_id} size={18} />
  </div>
  <div class="title">{ticket.title}</div>
  <div class="status"><span class="dot"></span>{ticket.status_name}</div>
</div>
<Handle type="source" position={Position.Right} />

<style>
  .node {
    width: 240px;
    height: 84px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 10px;
    border-radius: var(--radius);
    border: 1px solid var(--border-strong);
    border-left: 3px solid var(--cat);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-sm);
    font-size: 12px;
  }
  .node.chain {
    box-shadow: 0 0 0 2px var(--accent-soft), var(--shadow-sm);
    border-color: var(--accent);
    border-left-color: var(--cat);
  }
  .node.done .title {
    color: var(--text-2);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .spacer {
    flex: 1;
  }
  .lock {
    color: var(--danger);
    line-height: 0;
  }
  .title {
    font-size: 12.5px;
    font-weight: 500;
    line-height: 1.3;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--text-3);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--cat);
  }
</style>
