<script>
  // The paper sessions, in the dock next to History and Strategies.
  //
  // One row per session: what it is doing now (status + next occurrence), what it holds
  // (open positions), what it has produced (the last stat block) and what it has to say
  // (undelivered events). The last column is the point of the offline path: a fill the
  // engine could not push anywhere is read here on the next login, and acknowledged.
  import Icon from '$lib/ui/Icon.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { paperApi } from './paper.js';
  import { fmtNum } from './api.js';
  import { t } from '$lib/i18n';

  let { sessions = [], unseen = [], onedit = () => {}, onchanged = () => {} } = $props();

  let busy = $state('');
  /** The session a delete is being confirmed for. Deleting drops its book and its log too. */
  let doomed = $state(null);

  const unseenBy = $derived(
    unseen.reduce((acc, e) => {
      acc[e.session_id] = (acc[e.session_id] ?? 0) + 1;
      return acc;
    }, {})
  );

  function scheduleLabel(s) {
    if (s.kind === 'interval') return $t('backtest.paper.everyMinutes', { n: s.every_minutes ?? 0 });
    return $t(`automator.sched.kind.${s.kind}`);
  }

  function when(ts) {
    if (!ts) return '-';
    return new Date(ts).toLocaleString();
  }

  async function act(id, fn) {
    busy = id;
    try {
      await fn();
      onchanged();
    } catch {
      /* the row's own error field carries what the server said on the next load */
    } finally {
      busy = '';
    }
  }

  const runNow = (s) => act(s.id, () => paperApi.runNow(s.id));
  const toggle = (s) =>
    act(s.id, () => paperApi.setStatus(s.id, s.status === 'active' ? 'paused' : 'active'));
  const remove = (s) => act(s.id, () => paperApi.remove(s.id));
  const ack = (s) => act(s.id, () => paperApi.markSeen(s.id));
</script>

{#if !sessions.length}
  <EmptyState
    icon="zap"
    title={$t('backtest.paper.emptyTitle')}
    description={$t('backtest.paper.emptyHint')}
  />
{:else}
  <div class="rows">
    {#each sessions as s (s.id)}
      <div class="row" class:busy={busy === s.id}>
        <span class="dot {s.status}" title={$t(`backtest.paper.status.${s.status}`)}></span>
        <button class="name" onclick={() => onedit(s)}>{s.name}</button>

        <span class="meta">{scheduleLabel(s)}</span>
        <span class="meta" title={$t('backtest.paper.nextRun')}>
          <Icon name="clock" size={11} />
          {when(s.next_run_at)}
        </span>

        {#if s.stats}
          <span class="stat" class:up={(s.stats.net_pnl ?? 0) >= 0}>
            {fmtNum(s.stats.net_pnl ?? 0, 2)}
          </span>
          <span class="meta">{$t('backtest.paper.tradeCount', { n: s.stats.trades ?? 0 })}</span>
        {/if}

        {#if unseenBy[s.id]}
          <button class="badge" onclick={() => ack(s)} title={$t('backtest.paper.ack')}>
            {unseenBy[s.id]} {$t('backtest.paper.new')}
          </button>
        {/if}

        {#if s.last_error}
          <span class="err" title={s.last_error}><Icon name="alert-triangle" size={11} /></span>
        {/if}

        <div class="acts">
          <button onclick={() => runNow(s)} title={$t('backtest.paper.runNow')}>
            <Icon name="refresh-cw" size={12} />
          </button>
          <button onclick={() => toggle(s)} title={$t(`backtest.paper.${s.status === 'active' ? 'pause' : 'resume'}`)}>
            <Icon name={s.status === 'active' ? 'pause' : 'play'} size={12} />
          </button>
          <button onclick={() => onedit(s)} title={$t('common.edit')}>
            <Icon name="settings" size={12} />
          </button>
          <button class="danger" onclick={() => (doomed = s)} title={$t('common.delete')}>
            <Icon name="trash" size={12} />
          </button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<ConfirmModal
  open={!!doomed}
  title={$t('backtest.paper.deleteTitle')}
  message={$t('backtest.paper.deleteConfirm', { name: doomed?.name ?? '' })}
  confirmLabel={$t('common.delete')}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={() => {
    const s = doomed;
    doomed = null;
    if (s) remove(s);
  }}
  oncancel={() => (doomed = null)}
/>

<style>
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
    min-height: 34px;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .row.busy {
    opacity: 0.6;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .dot.active {
    background: var(--green);
  }
  .dot.error {
    background: var(--red);
  }
  .name {
    background: none;
    border: 0;
    color: var(--text);
    font-weight: var(--fw-medium);
    cursor: pointer;
    padding: 0;
    text-align: left;
    /* The name is the only elastic column: it truncates so the numbers and the actions on
       the right keep their place whatever the session is called. */
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name:hover {
    color: var(--accent);
  }
  .meta {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
    flex: none;
  }
  .stat {
    color: var(--red);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    flex: none;
  }
  .stat.up {
    color: var(--green);
  }
  .badge {
    flex: none;
    background: var(--accent);
    color: var(--surface);
    border: 0;
    border-radius: var(--radius);
    padding: 1px var(--space-2);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .err {
    color: var(--amber);
    display: inline-flex;
    flex: none;
  }
  /* One control group, aligned on the right edge of every row: same square for each button,
     so the icons line up down the list instead of drifting with their own glyph widths. */
  .acts {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  .acts button {
    background: none;
    border: 0;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
    padding: 0;
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    transition: background-color 0.12s ease, color 0.12s ease;
  }
  .acts button:hover {
    background: var(--surface);
    color: var(--text);
  }
  .acts .danger:hover {
    background: var(--surface);
    color: var(--red);
  }
</style>
