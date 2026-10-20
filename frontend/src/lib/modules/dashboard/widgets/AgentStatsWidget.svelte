<script>
  // Agent widgets that are not the composer: the recent-conversation list and the token
  // counter. Both read the module's own endpoints.
  //
  // Token totals are only served per conversation (`getConversation` carries the sums),
  // so the counter reads a bounded window of the newest ones rather than the whole
  // history — the cap is the config's `scan`.
  import { agentApi } from '$lib/modules/agent/api.js';
  import { fmtNum, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import WidgetState from './WidgetState.svelte';
  import MiniBars from './parts/MiniBars.svelte';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'recent');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));
  const scan = $derived(Math.max(1, Math.min(20, item.config?.scan ?? 10)));

  let convs = $state(null);
  let usage = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    let alive = true;
    agentApi.listConversations()
      .then((c) => { if (alive) convs = c; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  $effect(() => {
    if (editing || variant !== 'tokens' || !convs) return;
    let alive = true;
    const pick = convs.slice(0, scan);
    Promise.all(pick.map((c) => agentApi.getConversation(c.id).then((d) => ({
      id: c.id,
      title: c.title,
      tokens: (d.usage?.input_tokens ?? 0) + (d.usage?.output_tokens ?? 0)
    }))))
      .then((rows) => { if (alive) usage = rows; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // Oldest conversation first, so the bars read left to right like every other series.
  const usageBars = $derived((usage ?? []).slice().reverse());
  const total = $derived((usage ?? []).reduce((s, r) => s + r.tokens, 0));
  const avg = $derived(usage?.length ? Math.round(total / usage.length) : 0);
</script>

<WidgetState
  {editing}
  error={err}
  loading={convs === null || (variant === 'tokens' && usage === null)}
  empty={(convs ?? []).length === 0}
  preview={$t('dashboard.widgets.agentStats.preview')}
  emptyText={$t('dashboard.widgets.agentStats.empty')}
  rows={4}
>
  {#if variant === 'tokens'}
    <div class="w-body">
      <div class="totals">
        <div class="cell">
          <span class="fig">{fmtNum(total, 0)}</span>
          <span class="w-sub">{$t('dashboard.widgets.agentStats.totalTokens')}</span>
        </div>
        <div class="cell">
          <span class="fig">{fmtNum(avg, 0)}</span>
          <span class="w-sub">{$t('dashboard.widgets.agentStats.avgPerConversation')}</span>
        </div>
        <div class="barbox">
          <MiniBars values={usageBars.map((r) => r.tokens)} labels={usageBars.map((r) => r.title)}
            tone="accent" height={52} valueFormat={(v) => fmtNum(v, 0)}
            label={$t('dashboard.widgets.agentStats.totalTokens')} />
        </div>
      </div>
      <span class="w-sub">{$t('dashboard.widgets.agentStats.overLast', { count: usage?.length ?? 0 })}</span>
    </div>
  {:else}
    <ul class="w-list">
      {#each (convs ?? []).slice(0, limit) as c (c.id)}
        <li>
          <a class="w-row stack" href="/agent">
            <span class="line">
              <span class="w-name title">{c.title || $t('dashboard.widgets.agentStats.untitled')}</span>
              <span class="w-sub when">{$ago(c.updated_at)}</span>
            </span>
            {#if c.summary}<span class="w-name w-sub">{c.summary}</span>{/if}
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</WidgetState>

<style>
  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
    min-width: 0;
  }
  .title {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .when {
    flex-shrink: 0;
  }
  a:hover .title {
    color: var(--accent);
  }
  /* Two figures then the bars, split by hairlines: the mockup's usage strip. */
  .totals {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-width: 0;
  }
  .cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .cell + .cell {
    padding-left: var(--space-4);
    border-left: var(--hairline) solid var(--border);
  }
  .fig {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 22px;
    font-weight: var(--fw-medium);
    letter-spacing: -0.01em;
    color: var(--text);
  }
  .barbox {
    flex: 1;
    min-width: 0;
    padding-left: var(--space-4);
    border-left: var(--hairline) solid var(--border);
  }
</style>
