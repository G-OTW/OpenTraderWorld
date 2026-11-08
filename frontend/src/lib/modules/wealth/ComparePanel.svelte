<script>
  // Then versus now: the category table at the start of the headline period next to today,
  // each read as the latest revision on or before that date, the change between them.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Delta from '$lib/ui/charts/Delta.svelte';
  import { wealthApi } from './api.js';
  import { compareSnapshots } from './engine.js';
  import { money } from './view.js';
  import { dateKey } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { currency = 'USD', reloadKey = 0, from = null } = $props();

  let cmp = $state(null);
  let thenAt = $state('');
  let error = $state('');

  $effect(() => {
    void reloadKey;
    const at = from;
    if (!at) return;
    const today = dateKey();
    error = '';
    Promise.all([wealthApi.snapshot(at), wealthApi.snapshot(today)])
      .then(([a, b]) => {
        thenAt = a.at;
        cmp = compareSnapshots(a, b);
      })
      .catch((e) => {
        cmp = null;
        error = e.message;
      });
  });

  const label = (c) => c || $t('wealth.page.uncategorized');
  const absent = $derived($t('wealth.compare.absent'));
</script>

{#snippet change(d, p)}
  <span class="chg">
    <Delta value={d} format={(v) => money(v, currency)} sensitive />
    {#if p != null}<Delta value={p} digits={1} arrow={false} />{/if}
  </span>
{/snippet}

<div class="panel">
  <ErrorText {error} />
  {#if cmp}
    <div class="tbl-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('wealth.page.colCategory')}</th>
            <th class="num">{thenAt}</th>
            <th class="num">{$t('wealth.movers.change')}</th>
            <th class="num">{$t('wealth.compare.now')}</th>
          </tr>
        </thead>
        <tbody>
          {#each cmp.rows as r (r.name)}
            <tr>
              <td class="name" title={label(r.name)}>{label(r.name)}</td>
              <td class="num" class:dim={r.then == null}>{money(r.then, currency, absent)}</td>
              <td class="num">{@render change(r.delta, r.pct)}</td>
              <td class="num" class:dim={r.now == null}>{money(r.now, currency, absent)}</td>
            </tr>
          {/each}
          <tr class="total">
            <td>{$t('wealth.page.netWorth')}</td>
            <td class="num">{money(cmp.total.then, currency)}</td>
            <td class="num">{@render change(cmp.total.delta, cmp.total.pct)}</td>
            <td class="num">{money(cmp.total.now, currency)}</td>
          </tr>
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .tbl-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    table-layout: fixed;
  }
  th:first-child {
    width: 30%;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chg {
    display: inline-flex;
    justify-content: flex-end;
    gap: var(--space-2);
    white-space: nowrap;
  }
  .dim {
    color: var(--dim);
    font-family: var(--font);
  }
  .total td {
    font-weight: 600;
    border-bottom: none;
    border-top: var(--hairline) solid var(--border-control);
  }
</style>
