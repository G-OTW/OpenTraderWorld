<script>
  // "Worth it?": running subscriptions ranked by monthly cost per rating point, so the
  // expensive, little-used ones come first. Low-value rows (rated 2 or less, or twice the
  // median cost per point) carry a "review" tag. Only rated subscriptions can be ranked;
  // the others are counted and sent to the rating prompt instead of being given a score.
  import { fmtDate } from '$lib/format.js';
  import { worthRanking, isEnded, reviewIds } from './calc.js';
  import { money, initial } from './fmt.svelte.js';
  import Badge from '$lib/ui/Badge.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { t } from '$lib/i18n';

  let { subs = [], cur = 'USD', toDisplay = (a) => a, onrate = () => {} } = $props();

  const rows = $derived(worthRanking(subs, toDisplay));
  const review = $derived(reviewIds(rows));
  const unrated = $derived(subs.filter((s) => s.active && !isEnded(s) && !s.last_score).length);
</script>

<div class="worth">
  {#if rows.length === 0}
    <EmptyState icon="star" compact title={$t('subscriptions.worth.emptyTitle')} description={$t('subscriptions.worth.emptyBody')}>
      {#snippet action()}
        <button class="primary" onclick={onrate}>{$t('subscriptions.rate.rateNow')}</button>
      {/snippet}
    </EmptyState>
  {:else}
    <div class="table-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th class="num rank-h">#</th>
            <th>{$t('subscriptions.form.name')}</th>
            <th class="num">{$t('subscriptions.page.monthlyEq')}</th>
            <th>{$t('subscriptions.worth.score')}</th>
            <th class="num">{$t('subscriptions.worth.costPerPoint')}</th>
            <th class="num">{$t('subscriptions.worth.ratedOn')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each rows as r, i (r.sub.id)}
            <tr>
              <td class="num rank">{i + 1}</td>
              <td>
                <span class="who">
                  <span class="mono-tile" aria-hidden="true">{initial(r.sub.name)}</span>
                  <span class="name" title={r.sub.name}>{r.sub.name}</span>
                </span>
              </td>
              <td class="num">{money(r.monthly, cur)}</td>
              <td>
                <span class="score">
                  <span class="dots" aria-hidden="true">{#each [1, 2, 3, 4, 5] as n}<i class:on={r.score >= n}></i>{/each}</span>
                  <span class="of">{r.score} / 5</span>
                </span>
              </td>
              <td class="num strong">{money(r.costPerPoint, cur)}</td>
              <td class="num dim">{fmtDate(r.sub.last_rated_at)}</td>
              <td class="flag">
                {#if review.has(r.sub.id)}
                  <span title={$t('subscriptions.worth.reviewHint')}><Badge tone="warn">{$t('subscriptions.worth.review')}</Badge></span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="note">
      {$t('subscriptions.worth.reviewHint')}
      {#if unrated > 0}
        {$t('subscriptions.worth.unrated', { count: unrated })}
        <button class="link" onclick={onrate}>{$t('subscriptions.rate.rateNow')}</button>
      {/if}
    </p>
  {/if}
</div>

<style>
  .worth {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .table-wrap {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    overflow-x: auto;
  }
  tbody tr {
    transition: background-color 160ms ease;
  }
  tbody tr:hover {
    background: var(--surface-hover);
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  .rank-h,
  .rank {
    width: 2.5rem;
  }
  .rank,
  .dim {
    color: var(--dim);
  }
  .strong {
    color: var(--text);
    font-weight: 600;
  }
  .who {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    max-width: 280px;
  }
  .mono-tile {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .score {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }
  .of {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    color: var(--dim);
  }
  .dots {
    display: inline-flex;
    gap: 3px;
  }
  .dots i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    box-sizing: border-box;
    background: var(--surface-3);
    border: 1px solid var(--border-control);
  }
  .dots i.on {
    background: var(--accent);
    border-color: var(--accent);
  }
  .flag {
    width: 1%;
    white-space: nowrap;
    text-align: right;
  }
  .note {
    color: var(--dim);
    font-size: 12px;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font: inherit;
    text-decoration: underline;
  }
</style>
