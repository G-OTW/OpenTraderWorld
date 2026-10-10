<script>
  // Webhook widgets — the cards of the Webhooks mockup, each a `variant`. The endpoint
  // list already carries the received counter and the last-received stamp; the event log
  // is per endpoint, so the feed and the error rate read the endpoints they need and
  // merge — capped, because an endpoint keeps only its last 50 events anyway.
  //
  // Config: { variant, limit }.
  import { webhooksApi } from '$lib/modules/webhooks/api.js';
  import { fmtNum, fmtPct, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import RankBars from './parts/RankBars.svelte';
  import { dayLabels } from './parts/axis.js';

  let { item, editing } = $props();
  const variant = $derived(item.config?.variant ?? 'total');
  const limit = $derived(Math.max(1, Math.min(30, item.config?.limit ?? 6)));
  // An endpoint silent for longer than this reads as a problem, not as a quiet hour.
  const quietHours = $derived(Math.max(1, Math.min(168, item.config?.quietHours ?? 2)));

  const needsEvents = $derived(['feed', 'errors', 'total'].includes(variant));

  let hooks = $state(null);
  let events = $state(null);
  let err = $state('');

  $effect(() => {
    if (editing) return;
    let alive = true;
    webhooksApi.list()
      .then((d) => { if (alive) hooks = d.webhooks ?? []; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  $effect(() => {
    if (editing || !needsEvents || !hooks) return;
    let alive = true;
    const pick = hooks.slice(0, 8);
    Promise.all(pick.map((h) => webhooksApi.events(h.id).then((rows) =>
      rows.map((e) => ({ ...e, hook: h })))))
      .then((lists) => { if (alive) events = lists.flat(); })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const total = $derived((hooks ?? []).reduce((s, h) => s + (h.received_count ?? 0), 0));

  // One bar per day over the last 30, from the stored event log.
  const perDay = $derived.by(() => {
    const counts = new Map();
    for (const e of events ?? []) {
      const key = e.received_at.slice(0, 10);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    const out = [];
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    for (let i = 29; i >= 0; i--) {
      const day = new Date(d);
      day.setDate(d.getDate() - i);
      out.push(counts.get(day.toISOString().slice(0, 10)) ?? 0);
    }
    return out;
  });

  const feed = $derived(
    [...(events ?? [])].sort((a, b) => b.received_at.localeCompare(a.received_at)).slice(0, limit)
  );

  // Error rate over what the log still holds, not over the lifetime counter: the two
  // count different things and mixing them would overstate a fixed endpoint.
  const errorRates = $derived.by(() => {
    const per = new Map();
    for (const e of events ?? []) {
      const acc = per.get(e.hook.id) ?? { name: e.hook.name, bad: 0, total: 0 };
      acc.total++;
      if (e.status !== 'ok') acc.bad++;
      per.set(e.hook.id, acc);
    }
    return [...per.values()]
      .map((a) => {
        const pct = a.total ? (a.bad / a.total) * 100 : 0;
        return {
          label: a.name,
          value: pct,
          display: fmtPct(pct, 1),
          color: pct >= 3 ? 'var(--red)' : pct > 0 ? 'var(--amber)' : 'var(--green)'
        };
      })
      .sort((a, b) => b.value - a.value)
      .slice(0, limit);
  });

  const lastSeen = $derived(
    [...(hooks ?? [])]
      .map((h) => ({
        h,
        at: h.last_received_at,
        silent: !h.last_received_at ||
          Date.now() - new Date(h.last_received_at).getTime() > quietHours * 3600000
      }))
      .sort((a, b) => String(b.at ?? '').localeCompare(String(a.at ?? '')))
      .slice(0, limit)
  );
</script>

<WidgetState
  {editing}
  error={err}
  loading={hooks === null || (needsEvents && events === null)}
  empty={(hooks ?? []).length === 0}
  preview={$t('dashboard.widgets.webhooks.preview')}
  emptyText={$t('dashboard.widgets.webhooks.empty')}
  rows={4}
>
  {#if variant === 'total'}
    <div class="w-body">
      <Stat value={fmtNum(total, 0)} note={$t('dashboard.widgets.webhooks.allEndpoints')} />
      <MiniBars values={perDay} labels={dayLabels(perDay.length)} tone="accent" height={64}
        valueFormat={(v) => fmtNum(v, 0)} label={$t('dashboard.widgets.webhooks.allEndpoints')} />
    </div>
  {:else if variant === 'errors'}
    <RankBars rows={errorRates} />
  {:else if variant === 'feed'}
    {#if feed.length === 0}
      <p class="w-state">{$t('dashboard.widgets.webhooks.noEvents')}</p>
    {:else}
      <ul class="w-list">
        {#each feed as e (e.id)}
          <li class="w-row stack">
            <span class="line">
              <span class="w-dot" class:ok={e.status === 'ok'} class:bad={e.status !== 'ok'}></span>
              <span class="w-name path">{e.hook.name}</span>
              <span class="w-sub when">{$ago(e.received_at)}</span>
            </span>
            <span class="w-name w-sub detail">{e.detail || e.status}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    <!-- lastSeen: silence is the failure mode this card names. -->
    <ul class="w-list">
      {#each lastSeen as x (x.h.id)}
        <li class="w-row">
          <span class="w-name path grow">{x.h.name}</span>
          <span class="w-sub when">
            {x.at ? $ago(x.at) : $t('dashboard.widgets.webhooks.never')}
          </span>
          {#if x.silent}
            <span class="w-pill neg">
              <Icon name="alert-triangle" size={11} />
              {$t('dashboard.widgets.webhooks.silent', { hours: quietHours })}
            </span>
          {:else}
            <span class="w-dot ok"></span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
    min-width: 0;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }
  .path {
    font-family: var(--mono);
    font-size: var(--text-sm);
    flex: 1;
    min-width: 0;
  }
  .when {
    flex-shrink: 0;
  }
  .detail {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .w-dot.ok {
    background: var(--green);
  }
  .w-dot.bad {
    background: var(--red);
  }
</style>
