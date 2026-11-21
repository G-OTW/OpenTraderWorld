<script>
  // One robot on a phone: its state, its money, when it next looks at the market.
  import Icon from '$lib/ui/Icon.svelte';
  import RobotBadges from './RobotBadges.svelte';
  import { t } from '$lib/i18n';
  import { age, countdown } from './fleet.js';

  let { robot, now = Date.now() } = $props();

  const money = (v) => (typeof v === 'number' && Number.isFinite(v) ? v.toLocaleString(undefined, { maximumFractionDigits: 2 }) : '-');
  const signed = (v) => (typeof v === 'number' && Number.isFinite(v) ? `${v > 0 ? '+' : ''}${money(v)}` : '-');
  const next = $derived(countdown(robot.next_run_at, now));
  const synced = $derived(age(robot.exec_synced_at, now));
</script>

<a class="rcard" href={`/robots/${robot.id}`} data-robot-card>
  <div class="top">
    <b class="name">{robot.name}</b>
    <Icon name="chevron-right" size={14} />
  </div>
  <RobotBadges {robot} />
  <div class="figures">
    <span><small>{$t('robots.fleet.equity')}</small><b>{money(robot.equity)}</b></span>
    <span><small>{$t('robots.fleet.today')}</small><b class:pos={robot.today_pnl > 0} class:neg={robot.today_pnl < 0}>{signed(robot.today_pnl)}</b></span>
    <span><small>{$t('robots.fleet.open')}</small><b>{robot.open}</b></span>
  </div>
  <div class="meta">
    <span>{(robot.tickers ?? []).join(', ')}</span>
    <span class="t">{next ? $t('robots.fleet.nextIn', { in: next }) : $t('robots.fleet.noNext')}</span>
    {#if synced}<span class="t">{$t('robots.fleet.syncedAgo', { ago: synced })}</span>{/if}
  </div>
  {#if robot.last_error}<p class="err">{robot.last_error}</p>{/if}
</a>

<style>
  /* Square hairline panel, same shape as the StatCards above it. `rcard`, not `card`:
     the global .card layer would add its radius and lift. */
  .rcard {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--pad-metric);
    border: var(--hairline) solid var(--border);
    background: var(--surface);
    color: var(--text);
    text-decoration: none;
    transition: background var(--dur-fast) var(--ease);
  }
  .rcard:hover {
    background: var(--surface-2);
  }
  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
    color: var(--dim);
  }
  .name {
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .figures {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--space-3);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .figures span {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  small {
    font-size: var(--fs-metric-label);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .figures b {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--fs-metric-value);
    font-weight: var(--fw-normal);
    overflow-wrap: anywhere;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    font-size: var(--fs-desc);
    color: var(--dim);
  }
  .meta .t {
    font-family: var(--mono);
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .err {
    margin: 0;
    font-size: var(--fs-desc);
    color: var(--red);
  }
</style>
