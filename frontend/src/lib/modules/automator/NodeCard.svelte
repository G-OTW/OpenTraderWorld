<script>
  // One task in the grid. It owns nothing: every gesture is reported upward, so the grid
  // stays the single place where order, selection and removal are decided.
  import Icon from '$lib/ui/Icon.svelte';
  import { KIND_META, label } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let {
    node,
    selected = false,
    dragging = false,
    status = '',
    ondragstart,
    onselect,
    onopen,
    onremove
  } = $props();

  const icon = $derived(KIND_META[node.kind]?.icon ?? 'zap');
  const summary = $derived(describe(node));

  // One line under the title: what this task will actually do, not what type it is.
  function describe(n) {
    const c = n.config ?? {};
    switch (n.kind) {
      case 'api':
        return `${c.method ?? 'GET'} ${c.path ?? ''}`.trim();
      case 'http':
        return `${c.method ?? 'GET'} ${c.url ?? ''}`.trim();
      case 'agent':
        return c.output === 'json' ? $t('automator.node.agentJson') : $t('automator.node.agentText');
      case 'notify':
        return c.title ?? '';
      case 'if':
        return $t('automator.node.conditions', { n: (c.conditions ?? []).length });
      case 'transform':
        return c.op ?? '';
      case 'delay':
        return $t('automator.node.delaySeconds', { n: c.seconds ?? 0 });
      default:
        return '';
    }
  }

  function open(ev) {
    ev.stopPropagation();
    onselect?.();
    onopen?.();
  }
</script>

<div
  class="task"
  class:selected
  class:dragging
  class:failed={status === 'failed'}
  class:ok={status === 'ok'}
  class:skipped={status === 'skipped'}
  draggable="true"
  ondragstart={ondragstart}
  onclick={open}
  onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && open(e)}
  role="button"
  tabindex="0"
  title={$t('automator.grid.openTask')}
>
  <div class="head">
    <Icon name={icon} size={14} />
    <!-- Its own tooltip, so a name too long for the card can still be read: a nested
         title wins over the card's "open this task". -->
    <span class="title" title={label(node)}>{label(node)}</span>
    {#if status}
      <span class="dot {status}" title={status}></span>
    {/if}
    <button
      class="kill"
      onclick={(e) => {
        e.stopPropagation();
        onremove?.();
      }}
      title={$t('common.delete')}
      aria-label={$t('common.delete')}
    >
      <Icon name="x" size={14} />
    </button>
  </div>

  {#if summary}
    <p class="sub" title={summary}>{summary}</p>
  {/if}

  {#if node.on_error === 'continue' || node.retry?.count}
    <div class="flags">
      {#if node.on_error === 'continue'}
        <span class="flag"><Icon name="arrow-right" size={10} />{$t('automator.node.flagContinue')}</span>
      {/if}
      {#if node.retry?.count}
        <span class="flag"><Icon name="refresh-cw" size={10} />{node.retry.count}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  /* Every card is the same size, whatever it has to say. A summary line or a flag is
     optional, and letting it change the height made each step a different height and the
     whole board ragged. A step only ever grows by a whole card, when its tasks wrap. */
  .task {
    width: 208px;
    height: 72px;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 0.5px solid var(--border);
    padding: var(--space-2) var(--space-3);
    cursor: grab;
    user-select: none;
    overflow: hidden;
  }
  .task:hover {
    border-color: var(--accent);
  }
  .task.selected {
    border-color: var(--accent);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .task.dragging {
    opacity: 0.4;
  }
  .task.failed {
    box-shadow: inset 2px 0 0 var(--red);
  }
  .task.ok {
    box-shadow: inset 2px 0 0 var(--green);
  }
  .task.skipped {
    opacity: 0.55;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .title {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    margin-left: auto;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.ok {
    background: var(--green);
  }
  .dot.failed {
    background: var(--red);
  }
  .dot.simulated {
    background: var(--amber);
  }
  /* A 24 px target, pulled into the card's own padding so the bigger button does not push
     the title around. It stays hidden until the card is hovered, but never for the
     keyboard: focus brings it back. */
  .kill {
    margin: -6px -8px -6px auto;
    width: 24px;
    height: 24px;
    flex: none;
    background: none;
    border: 0;
    padding: 0;
    color: var(--muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
  }
  .dot + .kill {
    margin-left: var(--space-1);
  }
  .task:hover .kill,
  .kill:focus-visible {
    opacity: 1;
  }
  .kill:hover {
    color: var(--red);
    background: var(--surface-2);
  }
  .sub {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--mono);
  }
  .flags {
    display: flex;
    gap: var(--space-2);
    margin-top: auto;
  }
  .flag {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 10px;
    color: var(--muted);
  }
</style>
