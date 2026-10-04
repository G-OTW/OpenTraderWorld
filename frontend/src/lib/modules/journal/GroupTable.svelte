<script>
  // One "outcome by X" table: strategy, symbol, asset class, side or tag. Every column
  // is an outcome measure, so a row answers "does this bucket actually make money?"
  // rather than just "how often do I trade it".
  //
  // The net column carries a proportional bar behind the number: the ranking is visible
  // before any of the figures are read.
  import { fmtMoney, fmtSignedMoney, fmtPct, fmtNum } from './api.js';
  import Badge from '$lib/ui/Badge.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let {
    rows = [],
    currency = 'USD',
    // Renames a row (asset classes and sides are enum keys, not display text).
    nameOf = (r) => r.name,
    // Rows shown before the "show all" toggle.
    limit = 8,
    emptyTitle = ''
  } = $props();

  let sort = $state('trades'); // trades | net | win_rate | expectancy | avg_r
  let desc = $state(true);
  let expanded = $state(false);

  function sortBy(key) {
    if (sort === key) {
      desc = !desc;
    } else {
      sort = key;
      desc = true;
    }
  }

  const sorted = $derived.by(() => {
    const v = (r) => {
      const x = r[sort];
      // Missing measures sort last in both directions instead of pretending to be zero.
      return x == null ? (desc ? -Infinity : Infinity) : x;
    };
    return [...rows].sort((a, b) => (desc ? v(b) - v(a) : v(a) - v(b)));
  });
  const visible = $derived(expanded ? sorted : sorted.slice(0, limit));
  const maxNet = $derived(Math.max(1e-9, ...rows.map((r) => Math.abs(r.net))));

  const COLS = [
    { key: 'trades', labelKey: 'journal.analytics.group.trades' },
    { key: 'win_rate', labelKey: 'journal.analytics.group.winRate' },
    { key: 'net', labelKey: 'journal.analytics.group.net' },
    { key: 'expectancy', labelKey: 'journal.analytics.group.expectancy' },
    { key: 'avg_r', labelKey: 'journal.analytics.group.avgR' },
    { key: 'profit_factor', labelKey: 'journal.analytics.group.profitFactor' }
  ];

  const TAG_TONE = { mistake: 'danger', rule: 'success', setup: 'neutral' };
</script>

{#if rows.length === 0}
  <EmptyState compact icon="bar-chart" title={emptyTitle || $t('journal.analytics.group.empty')} />
{:else}
  <table class="tbl">
    <thead>
      <tr>
        <th>{$t('journal.analytics.group.name')}</th>
        {#each COLS as c (c.key)}
          <th class="num">
            <button class="sorter" class:active={sort === c.key} onclick={() => sortBy(c.key)}>
              {$t(c.labelKey)}{#if sort === c.key}<span class="arrow">{desc ? '▾' : '▴'}</span>{/if}
            </button>
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each visible as r (r.key)}
        <tr>
          <td>
            <span class="name">
              {nameOf(r) || $t('journal.analytics.group.unassigned')}
              {#if r.kind}<Badge tone={TAG_TONE[r.kind] ?? 'neutral'}>{$t(`journal.tags.kind.${r.kind}`)}</Badge>{/if}
            </span>
          </td>
          <td class="num">{r.trades}</td>
          <td class="num">{r.win_rate == null ? '—' : fmtPct(r.win_rate)}</td>
          <td class="num net">
            <span
              class="netbar"
              class:neg={r.net < 0}
              style="width:{(Math.abs(r.net) / maxNet) * 100}%"
            ></span>
            <span class="netval" class:pos={r.net > 0} class:neg={r.net < 0}>
              {fmtSignedMoney(r.net, currency)}
            </span>
          </td>
          <td class="num" class:pos={r.expectancy > 0} class:neg={r.expectancy < 0}>
            {r.expectancy == null ? '—' : fmtSignedMoney(r.expectancy, currency)}
          </td>
          <td class="num" class:pos={r.avg_r > 0} class:neg={r.avg_r < 0}>
            {r.avg_r == null ? '—' : `${r.avg_r > 0 ? '+' : ''}${fmtNum(r.avg_r)}R`}
          </td>
          <td class="num">{r.profit_factor == null ? '—' : fmtNum(r.profit_factor)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if sorted.length > limit}
    <button class="more" onclick={() => (expanded = !expanded)}>
      {expanded
        ? $t('journal.analytics.group.showLess')
        : $t('journal.analytics.group.showAll', { count: sorted.length })}
    </button>
  {/if}
{/if}

<style>
  .sorter {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: inherit;
    text-transform: inherit;
    letter-spacing: inherit;
    cursor: pointer;
  }
  .sorter:hover,
  .sorter.active {
    color: var(--text);
  }
  .arrow {
    margin-left: 2px;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  /* The bar sits behind the number, anchored right like the column it lives in. */
  td.net {
    position: relative;
  }
  .netbar {
    position: absolute;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    height: 60%;
    background: color-mix(in srgb, var(--green) 16%, transparent);
    pointer-events: none;
  }
  .netbar.neg {
    background: color-mix(in srgb, var(--red) 16%, transparent);
  }
  .netval {
    position: relative;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }
  .more {
    margin-top: var(--space-2);
    background: none;
    border: none;
    padding: 0;
    font-size: var(--text-xs);
    color: var(--accent);
    cursor: pointer;
  }
  .more:hover {
    text-decoration: underline;
  }
</style>
