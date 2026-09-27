<script lang="ts">
  import {
    Background,
    Controls,
    MarkerType,
    MiniMap,
    SvelteFlow,
    type Connection,
    type Edge,
    type Node,
  } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import ELK from 'elkjs/lib/elk.bundled.js'
  import { api } from '../lib/api'
  import { router } from '../lib/router.svelte'
  import { app } from '../lib/store.svelte'
  import { confirmer, toasts } from '../lib/toast.svelte'
  import type { GraphData, ProjectDetail } from '../lib/types'
  import FitView from './graph/FitView.svelte'
  import TicketNode from './graph/TicketNode.svelte'
  import Icon from './Icon.svelte'

  let { project }: { project: ProjectDetail } = $props()

  const NODE_W = 240
  const NODE_H = 84
  const elk = new ELK()
  const nodeTypes = { ticket: TicketNode }

  let nodes = $state.raw<Node[]>([])
  let edges = $state.raw<Edge[]>([])
  let data = $state<GraphData | null>(null)
  let showAll = $state(false)
  let showHierarchy = $state(false)
  let fitToken = $state(0)
  let shape = ''

  const keyOf = $derived(new Map((data?.nodes ?? []).map((n) => [n.id, n.key])))
  const chainLen = $derived(data?.longest_chain.length ?? 0)

  // Refreshes can overlap (live events arrive in bursts); only the newest one may render.
  let latest = 0
  $effect(() => {
    void app.ticketsVersion
    const all = showAll
    const hierarchy = showHierarchy
    const request = ++latest
    api.graph(project.key, all).then(
      (d) => layout(d, all, hierarchy, request),
      (e) => toasts.error(e),
    )
  })

  async function layout(d: GraphData, all: boolean, hierarchy: boolean, request: number) {
    const result = await elk.layout({
      id: 'root',
      layoutOptions: {
        'elk.algorithm': 'layered',
        'elk.direction': 'RIGHT',
        'elk.layered.spacing.nodeNodeBetweenLayers': '80',
        'elk.spacing.nodeNode': '28',
        'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
        'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      },
      children: d.nodes.map((n) => ({ id: String(n.id), width: NODE_W, height: NODE_H })),
      edges: d.edges.map((e) => ({ id: `b-${e.source}-${e.target}`, sources: [String(e.source)], targets: [String(e.target)] })),
    })
    if (request !== latest) return
    const pos = new Map((result.children ?? []).map((c) => [c.id, { x: c.x ?? 0, y: c.y ?? 0 }]))
    const chain = d.longest_chain
    const onChain = new Set(chain)
    const chainEdges = new Set(chain.slice(1).map((id, i) => `b-${chain[i]}-${id}`))

    data = d
    nodes = d.nodes.map((n) => ({
      id: String(n.id),
      type: 'ticket',
      position: pos.get(String(n.id)) ?? { x: 0, y: 0 },
      data: { ticket: n, onChain: chain.length > 1 && onChain.has(n.id) },
    }))
    const dependencyEdges: Edge[] = d.edges.map((e) => {
      const id = `b-${e.source}-${e.target}`
      const hot = chainEdges.has(id)
      return {
        id,
        source: String(e.source),
        target: String(e.target),
        type: 'smoothstep',
        class: hot ? 'chain' : 'dep',
        markerEnd: { type: MarkerType.ArrowClosed, width: 16, height: 16 },
        animated: hot,
      }
    })
    const hierarchyEdges: Edge[] = hierarchy
      ? d.hierarchy.map((h) => ({
          id: `h-${h.parent}-${h.child}`,
          source: String(h.parent),
          target: String(h.child),
          class: 'hier',
          selectable: false,
          focusable: false,
        }))
      : []
    edges = [...dependencyEdges, ...hierarchyEdges]

    const newShape = `${all}|${hierarchy}|${d.nodes.map((n) => n.id).join(',')}`
    if (newShape !== shape) {
      shape = newShape
      fitToken++
    }
  }

  async function connect(c: Connection) {
    const source = keyOf.get(Number(c.source))
    const target = keyOf.get(Number(c.target))
    if (!source || !target) return
    try {
      await api.createLink(source, target, 'blocks')
      toasts.show(`${source} now blocks ${target}`, 'success')
    } catch (e) {
      toasts.error(e)
    }
  }

  async function removeEdge(edge: Edge) {
    if (!edge.id.startsWith('b-')) return
    const source = keyOf.get(Number(edge.source))
    const target = keyOf.get(Number(edge.target))
    if (!source || !target) return
    const ok = await confirmer.ask('Remove dependency?', `${source} will no longer block ${target}.`, {
      confirm: 'Remove',
      danger: true,
    })
    if (!ok) return
    try {
      await api.deleteLink(source, target, 'blocks')
    } catch (e) {
      toasts.error(e)
    }
  }
</script>

<div class="toolbar">
  <label class="toggle"><input type="checkbox" bind:checked={showAll} /> Show tickets without dependencies</label>
  <label class="toggle"><input type="checkbox" bind:checked={showHierarchy} /> Show parent → child</label>
  <span class="spacer"></span>
  <span class="stat" title="Tickets on the longest dependency chain across the whole board">
    <Icon name="route" size={14} />
    Longest chain: <strong>{chainLen}</strong>
    · Limit: <strong>{data?.max_height ?? 'none'}</strong>
  </span>
  <span class="hint">Drag from a ticket's right edge to another ticket: the first blocks the second.</span>
</div>

<div class="canvas">
  {#if data && data.nodes.length === 0}
    <div class="empty">
      <p>No dependencies in {project.key} yet.</p>
      <button class="btn" onclick={() => (showAll = true)}>Show all tickets to start linking</button>
    </div>
  {/if}
  <SvelteFlow
    bind:nodes
    bind:edges
    {nodeTypes}
    colorMode={app.theme}
    minZoom={0.1}
    deleteKey={null}
    nodesConnectable
    proOptions={{ hideAttribution: true }}
    onnodeclick={({ node }) => {
      const key = keyOf.get(Number(node.id))
      if (key) router.openTicket(key)
    }}
    onedgeclick={({ edge }) => removeEdge(edge)}
    onbeforeconnect={(c) => {
      connect(c)
      return false
    }}
  >
    <FitView token={fitToken} />
    <Background gap={20} />
    <Controls showLock={false} />
    <MiniMap pannable zoomable nodeStrokeWidth={2} />
  </SvelteFlow>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    flex-wrap: wrap;
    padding: 12px 20px;
    font-size: 13px;
    color: var(--text-2);
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }
  .toggle input {
    accent-color: var(--accent);
  }
  .spacer {
    flex: 1;
  }
  .stat {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .stat strong {
    color: var(--text);
  }
  .hint {
    font-size: 12px;
    color: var(--text-3);
  }
  .canvas {
    position: relative;
    flex: 1;
    min-height: 0;
    margin: 0 20px 20px;
    border: 1px solid var(--border);
    border-radius: 10px;
    overflow: hidden;
  }
  .empty {
    position: absolute;
    inset: 0;
    z-index: 5;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 4px;
    color: var(--text-2);
    pointer-events: none;
  }
  .empty .btn {
    pointer-events: auto;
  }
  .canvas :global(.svelte-flow) {
    --xy-background-color: var(--bg);
    --xy-edge-stroke: var(--text-3);
    --xy-edge-stroke-selected: var(--accent);
    --xy-handle-background-color: var(--accent);
    --xy-handle-border-color: var(--surface);
    --xy-minimap-background-color: var(--surface);
    --xy-controls-button-background-color: var(--surface);
    --xy-controls-button-background-color-hover: var(--surface-2);
    --xy-controls-button-color: var(--text-2);
    --xy-controls-button-border-color: var(--border);
  }
  .canvas :global(.svelte-flow__node) {
    padding: 0;
    border: none;
    background: none;
    box-shadow: none;
    border-radius: var(--radius);
  }
  .canvas :global(.svelte-flow__handle) {
    width: 10px;
    height: 10px;
  }
  .canvas :global(.svelte-flow__edge.chain path) {
    stroke: var(--accent);
    stroke-width: 2.2;
  }
  .canvas :global(.svelte-flow__edge.dep path) {
    stroke-width: 1.6;
  }
  .canvas :global(.svelte-flow__edge.hier path) {
    stroke-dasharray: 4 4;
    stroke: var(--border-strong);
  }
  .canvas :global(.svelte-flow__edge.dep:hover path),
  .canvas :global(.svelte-flow__edge.chain:hover path) {
    stroke: var(--danger);
    cursor: pointer;
  }
</style>
