<script>
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { onMount, onDestroy } from 'svelte';

  const POLL_MS = 20000;

  // services: map of name → 'up' | 'down'. Empty until first probe resolves.
  let services = $state({});
  let reachable = $state(true); // false when /health itself can't be reached
  let open = $state(false);

  const names = $derived(Object.keys(services));
  const downCount = $derived(names.filter((n) => services[n] !== 'up').length);

  // Aggregate light: red if unreachable or every service down; orange if some down; green if all up.
  const level = $derived.by(() => {
    if (!reachable) return 'red';
    if (names.length === 0) return 'checking';
    if (downCount === 0) return 'green';
    if (downCount >= names.length) return 'red';
    return 'orange';
  });

  const summaryKey = $derived(
    level === 'green' ? 'status.healthy' : level === 'red' ? 'status.down' : 'status.degraded'
  );

  async function probe() {
    try {
      const res = await api.health();
      services = res?.services ?? {};
      reachable = true;
    } catch {
      reachable = false;
    }
  }

  let timer;
  onMount(() => {
    probe();
    timer = setInterval(probe, POLL_MS);
  });
  onDestroy(() => clearInterval(timer));
</script>

<div
  class="status"
  role="status"
  onmouseenter={() => (open = true)}
  onmouseleave={() => (open = false)}
>
  <span class="dot" data-level={level} aria-label={$t(summaryKey)} title={$t(summaryKey)}></span>

  {#if open}
    <div class="popover" role="tooltip">
      <div class="head">{$t('status.title')}</div>
      {#if !reachable}
        <div class="row">
          <span class="rowdot" data-up="false"></span>
          <span class="name">{$t('status.core')}</span>
          <span class="state down">{$t('status.svcDown')}</span>
        </div>
      {:else if names.length === 0}
        <div class="row"><span class="name muted">{$t('status.checking')}</span></div>
      {:else}
        {#each names as name}
          {@const up = services[name] === 'up'}
          <div class="row">
            <span class="rowdot" data-up={up}></span>
            <span class="name">{$t('status.' + name)}</span>
            <span class="state" class:down={!up}>
              {up ? $t('status.up') : $t('status.svcDown')}
            </span>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .status {
    position: relative;
    display: inline-flex;
    align-items: center;
    padding: 0 var(--space-2);
    height: 32px;
    cursor: default;
  }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot[data-level='green'] {
    background: var(--green);
  }
  .dot[data-level='orange'] {
    background: var(--amber);
  }
  .dot[data-level='red'] {
    background: var(--red);
  }

  .popover {
    position: absolute;
    top: calc(100% + var(--space-2));
    right: 0;
    z-index: 50;
    min-width: 200px;
    background: var(--surface);
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-3);
    box-shadow: none;
  }
  .head {
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
    margin-bottom: var(--space-2);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
    font-size: var(--text-sm);
  }
  .rowdot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--green);
    flex: none;
  }
  .rowdot[data-up='false'] {
    background: var(--red);
  }
  .name {
    color: var(--text);
  }
  .name.muted {
    color: var(--muted);
  }
  .state {
    margin-left: auto;
    color: var(--green);
    font-size: var(--text-xs);
  }
  .state.down {
    color: var(--red);
  }
</style>
