<script>
  // Chart-workspace widgets — the cards of the Histviz mockup, each a `variant`. Reads
  // the module's workspaces, saved lists and alerts. Nothing new server-side.
  //
  // Config: { variant, limit, hours }.
  import { histvizApi } from '$lib/modules/histviz/api.js';
  import { fmtNum, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'workspaces');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));
  // "Recently" for the triggered card: the mockup's 24h window.
  const hours = $derived(Math.max(1, Math.min(168, item.config?.hours ?? 24)));

  const source = $derived(
    variant === 'lists' ? 'lists' : ['alerts', 'triggered'].includes(variant) ? 'alerts' : 'workspaces'
  );

  let rows = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    const src = source;
    let alive = true;
    rows = null;
    const call =
      src === 'lists' ? histvizApi.lists()
      : src === 'alerts' ? histvizApi.alerts()
      : histvizApi.workspaces();
    call
      .then((r) => { if (alive) rows = Array.isArray(r) ? r : (r.alerts ?? r.lists ?? r.workspaces ?? []); })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // A workspace stores its panes as JSON; how many charts it holds is that array's length.
  const paneCount = (w) => (Array.isArray(w.panes) ? w.panes.length : 0);
  const itemCount = (l) => (Array.isArray(l.items) ? l.items.length : 0);

  const active = $derived((rows ?? []).filter((a) => a.enabled));
  const triggered = $derived(
    (rows ?? [])
      .filter((a) => a.last_fired_at && Date.now() - new Date(a.last_fired_at).getTime() <= hours * 3600000)
      .sort((a, b) => b.last_fired_at.localeCompare(a.last_fired_at))
  );
  const opLabel = (a) => `${a.op === 'below' || a.op === 'crosses_below' ? '<' : '>'} ${fmtNum(a.value)}`;
</script>

<WidgetState
  {editing}
  error={err}
  loading={rows === null}
  empty={(rows ?? []).length === 0}
  preview={$t('dashboard.widgets.histviz.preview')}
  emptyText={$t('dashboard.widgets.histviz.empty')}
  rows={4}
>
  {#if variant === 'alerts' || variant === 'triggered'}
    {@const shown = (variant === 'alerts' ? active : triggered).slice(0, limit)}
    {#if shown.length === 0}
      <p class="w-state">
        {variant === 'alerts'
          ? $t('dashboard.widgets.histviz.noAlerts')
          : $t('dashboard.widgets.histviz.noneTriggered')}
      </p>
    {:else}
      <div class="w-body">
        <span class="w-sub">
          {variant === 'alerts'
            ? $t('dashboard.widgets.histviz.activeAlerts', { count: active.length })
            : $t('dashboard.widgets.histviz.firedIn', { count: triggered.length, hours })}
        </span>
        <ul class="w-list">
          {#each shown as a (a.id)}
            <li class="w-row">
              {#if variant === 'triggered'}
                <span class="w-dot" class:up={a.op !== 'below' && a.op !== 'crosses_below'}
                  class:down={a.op === 'below' || a.op === 'crosses_below'}></span>
              {/if}
              <span class="stackcol grow">
                <span class="tick">{a.ticker}</span>
                <span class="w-sub">{opLabel(a)}</span>
              </span>
              {#if variant === 'triggered'}
                <span class="w-sub when">{$ago(a.last_fired_at)}</span>
              {:else}
                <span class="w-num">{a.last_value == null ? '—' : fmtNum(a.last_value)}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  {:else if variant === 'lists'}
    <div class="w-body">
      <span class="w-sub">{$t('dashboard.widgets.histviz.watchableSets', { count: (rows ?? []).length })}</span>
      <ul class="w-list">
        {#each (rows ?? []).slice(0, limit) as l (l.id)}
          <li>
            <a class="w-row" href="/histviz">
              <span class="w-name grow">{l.name}</span>
              <span class="w-sub">{$t('dashboard.widgets.histviz.symbols', { count: itemCount(l) })}</span>
              <Icon name="chevron-right" size={13} />
            </a>
          </li>
        {/each}
      </ul>
    </div>
  {:else}
    <div class="w-body">
      <span class="w-sub">{$t('dashboard.widgets.histviz.workspaceCount', { count: (rows ?? []).length })}</span>
      <ul class="w-list">
        {#each (rows ?? []).slice(0, limit) as w (w.id)}
          <li>
            <a class="w-row" href="/histviz">
              <Icon name="candlestick" size={15} />
              <span class="w-name grow">{w.name}</span>
              <span class="w-sub">{$t('dashboard.widgets.histviz.charts', { count: paneCount(w) })}</span>
              <Icon name="chevron-right" size={13} />
            </a>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .when {
    flex-shrink: 0;
  }
  .stackcol {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .tick {
    font-family: var(--mono);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .w-row :global(svg) {
    color: var(--faint);
    flex-shrink: 0;
  }
  .w-dot.up {
    background: var(--green);
  }
  .w-dot.down {
    background: var(--red);
  }
</style>
