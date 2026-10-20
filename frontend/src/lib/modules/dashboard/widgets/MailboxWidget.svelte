<script>
  // Mailbox widget: the latest unread mail, newest first. A click opens the module —
  // the dashboard says what arrived, the module is where you read it.
  import { mailboxApi } from '$lib/modules/mailbox/api.js';
  import { dateKey, fmtNum, ago } from '$lib/format.js';
  import { t } from '$lib/i18n';
  import Icon from '$lib/ui/Icon.svelte';
  import WidgetState from './WidgetState.svelte';
  import Stat from './parts/Stat.svelte';
  import MiniBars from './parts/MiniBars.svelte';
  import { dayLabels } from './parts/axis.js';
  import Donut from './parts/Donut.svelte';
  import { livePulse, LIVE } from '../live.svelte.js';

  let { item, editing } = $props();
  const variant = $derived(item?.config?.variant ?? 'unread');

  // `config.category` — 'all' or one of the sender categories.
  const category = $derived(item?.config?.category ?? 'all');
  const limit = $derived(Math.max(1, Math.min(50, item?.config?.limit ?? 8)));

  const live = livePulse(LIVE.inbox);

  let messages = $state(null);
  let overview = $state.raw(null);
  let recent = $state(null);
  let err = $state('');

  $effect(() => {
    live.n;
    if (editing || variant !== 'unread') return;
    void category;
    load();
  });

  // Only the newest request in flight writes: refreshes are seconds apart.
  let seq = 0;
  async function load() {
    const mine = ++seq;
    err = '';
    try {
      const rows = await mailboxApi.listMessages({ category, unread: true, limit });
      if (mine === seq) messages = rows;
    } catch (e) {
      if (mine === seq) err = e.message;
    }
  }

  // The counter and the sync card share one accounts call: it already carries the counts.
  $effect(() => {
    live.n;
    if (editing || !['count', 'sync'].includes(variant)) return;
    let alive = true;
    mailboxApi.overview()
      .then((o) => { if (alive) overview = o; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  // The sender breakdown counts a window of recent messages, read or not.
  $effect(() => {
    live.n;
    if (editing || !['senders', 'count'].includes(variant)) return;
    let alive = true;
    mailboxApi.listMessages({ limit: item?.config?.window ?? 200 })
      .then((m) => { if (alive) recent = m; })
      .catch((e) => { if (alive) err = e.message; });
    return () => { alive = false; };
  });

  const accounts = $derived(overview?.accounts ?? []);
  const synced = $derived(accounts.filter((a) => a.enabled && !a.needs_reauth && !a.last_error));
  const lastSync = $derived(
    accounts.map((a) => a.last_success_at).filter(Boolean).sort().at(-1) ?? null
  );

  // One bar per day: how much mail landed that day over the last two weeks.
  const perDay = $derived.by(() => {
    const counts = new Map();
    for (const m of recent ?? []) counts.set(dateKey(m.received_at), (counts.get(dateKey(m.received_at)) ?? 0) + 1);
    const out = [];
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    for (let i = 13; i >= 0; i--) {
      const day = new Date(d);
      day.setDate(d.getDate() - i);
      out.push(counts.get(dateKey(day)) ?? 0);
    }
    return out;
  });

  // Top senders, with the tail folded into one "others" slice rather than a long legend.
  const bySender = $derived.by(() => {
    const counts = new Map();
    for (const m of recent ?? []) counts.set(m.sender_name, (counts.get(m.sender_name) ?? 0) + 1);
    const rows = [...counts].map(([label, value]) => ({ label, value })).sort((a, b) => b.value - a.value);
    const top = rows.slice(0, 5);
    const rest = rows.slice(5).reduce((sum, r) => sum + r.value, 0);
    if (rest > 0) top.push({ label: $t('dashboard.widgets.mailbox.others'), value: rest });
    return top;
  });
</script>

<WidgetState
  {editing}
  error={err}
  loading={(variant === 'unread' && messages === null) || (['count', 'sync'].includes(variant) && overview === null) || (variant === 'senders' && recent === null)}
  empty={variant === 'unread' && messages?.length === 0}
  preview={$t('dashboard.widgets.mailbox.preview')}
  emptyText={$t('dashboard.widgets.mailbox.empty')}
  rows={4}
>
  {#if variant === 'count'}
    <div class="w-body">
      <Stat value={String(overview.counts?.unread ?? 0)} note={$t('dashboard.widgets.mailbox.unreadMessages')} />
      {#if (recent ?? []).length > 0}
        <MiniBars values={perDay} labels={dayLabels(perDay.length)} tone="accent" height={50}
          valueFormat={(v) => fmtNum(v, 0)}
          label={$t('dashboard.widgets.mailbox.unreadMessages')} />
      {/if}
    </div>
  {:else if variant === 'sync'}
    <div class="w-body">
      <div class="syncline">
        <span class="w-dot" class:ok={synced.length === accounts.length && accounts.length > 0}></span>
        <span class="synctitle">
          {accounts.length === 0
            ? $t('dashboard.widgets.mailbox.noAccounts')
            : synced.length === accounts.length
              ? $t('dashboard.widgets.mailbox.allSynced')
              : $t('dashboard.widgets.mailbox.someFailing', { count: accounts.length - synced.length })}
        </span>
      </div>
      {#if lastSync}
        <span class="w-sub">{$t('dashboard.widgets.mailbox.lastSync', { when: $ago(lastSync) })}</span>
      {/if}
      <ul class="w-list">
        {#each accounts as a (a.id)}
          <li class="w-row">
            <span class="w-name grow">{a.name || a.email}</span>
            <span class="w-pill" class:pos={!a.last_error && !a.needs_reauth}
              class:neg={a.needs_reauth} class:warn={!!a.last_error && !a.needs_reauth}>
              {a.needs_reauth
                ? $t('dashboard.widgets.mailbox.needsReauth')
                : a.last_error
                  ? $t('dashboard.widgets.mailbox.failing')
                  : $t('dashboard.widgets.mailbox.synced')}
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {:else if variant === 'senders'}
    <Donut segments={bySender} centerLabel={$t('dashboard.widgets.mailbox.messages')} />
  {:else}
  <div class="w-body">
    <div class="w-list">
      {#each messages as m (m.id)}
        <a class="w-row stack" href="/mailbox">
          <span class="line">
            <span class="w-name from">{m.sender_name}</span>
            <span class="w-sub when">{$ago(m.received_at)}</span>
          </span>
          <span class="w-name subject">{m.subject || $t('mailbox.noSubject')}</span>
        </a>
      {/each}
    </div>
    <a class="w-foot" href="/mailbox">
      {$t('dashboard.widgets.mailbox.open')}
      <Icon name="chevron-right" size={12} />
    </a>
  </div>
  {/if}
</WidgetState>

<style>
  .grow {
    flex: 1;
  }
  .syncline {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .syncline .w-dot {
    background: var(--amber);
  }
  .syncline .w-dot.ok {
    background: var(--green);
  }
  .synctitle {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    color: var(--text);
  }
  .line {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }
  /* Sender over subject: unread mail is triaged by who sent it. */
  .from {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .when {
    flex-shrink: 0;
  }
  .subject {
    color: var(--text);
  }
  a:hover .subject {
    color: var(--accent);
  }
</style>
