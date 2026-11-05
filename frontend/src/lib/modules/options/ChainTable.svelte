<script>
  // The chain, grouped by expiry under a header row (date, days left, ATM IV). Views: calls,
  // puts, both (calls mirrored left of the strike, puts right) and straddle (call and put of
  // one strike side by side with the combined price and its breakevens).
  import { t } from '$lib/i18n';
  import { BY_KEY, price, pct, num } from './columns.js';

  let { groups = [], view = 'both', columns = [], spot = null, onrow = () => {} } = $props();

  let collapsed = $state({});

  const cols = $derived(columns.map((k) => BY_KEY[k]).filter(Boolean));
  const left = $derived([...cols].reverse());
  const span = $derived(view === 'both' ? cols.length * 2 + 1 : view === 'straddle' ? 9 : cols.length + 1);

  const itm = (r, strike) => spot != null && r && (r.call ? strike < spot : strike > spot);
  const mid = (r) => r?.mid ?? null;
  const sum = (s) => (mid(s.call) != null && mid(s.put) != null ? mid(s.call) + mid(s.put) : null);
  const label = (k) => $t(`options.col.${k}`);

  const key = (r) => `${r.expiry}|${r.strike}|${r.call ? 'c' : 'p'}`;
  const index = $derived.by(() => {
    const m = new Map();
    for (const g of groups) for (const s of g.strikes) for (const r of [s.call, s.put]) if (r) m.set(key(r), r);
    return m;
  });
  function pick(e) {
    const k = e.target.closest?.('[data-c]')?.dataset.c;
    const r = k && index.get(k);
    if (r) onrow(r);
  }
</script>

<div class="qp-scroll wrap">
  <table class="qp-table chain">
    <thead>
      {#if view === 'both'}
        <tr class="sides">
          <th colspan={cols.length} class="num">{$t('options.view.calls')}</th>
          <th></th>
          <th colspan={cols.length}>{$t('options.view.puts')}</th>
        </tr>
      {/if}
      <tr class="cols">
        {#if view === 'both'}
          {#each left as c (c.key)}<th class="num">{label(c.key)}</th>{/each}
          <th class="num strike">{$t('options.col.strike')}</th>
          {#each cols as c (c.key)}<th class="num">{label(c.key)}</th>{/each}
        {:else if view === 'straddle'}
          <th class="num strike">{$t('options.col.strike')}</th>
          <th class="num">{$t('options.straddle.call')}</th>
          <th class="num">{$t('options.straddle.put')}</th>
          <th class="num">{$t('options.straddle.price')}</th>
          <th class="num">{$t('options.straddle.ofSpot')}</th>
          <th class="num">{$t('options.straddle.lower')}</th>
          <th class="num">{$t('options.straddle.upper')}</th>
          <th class="num">{$t('options.straddle.callIv')}</th>
          <th class="num">{$t('options.straddle.putIv')}</th>
        {:else}
          <th class="num strike">{$t('options.col.strike')}</th>
          {#each cols as c (c.key)}<th class="num">{label(c.key)}</th>{/each}
        {/if}
      </tr>
    </thead>
    <!-- A contract is picked by delegation: one handler for the whole chain, not one per cell. -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <tbody onclick={pick}>
      {#each groups as g (g.expiry)}
        <tr class="ghead">
          <td colspan={span}>
            <button class="gtoggle" onclick={() => (collapsed[g.expiry] = !collapsed[g.expiry])} aria-expanded={!collapsed[g.expiry]}>
              <span class="caret">{collapsed[g.expiry] ? '▸' : '▾'}</span>
              <strong>{g.expiry}</strong>
              <span class="qp-muted">{$t('options.group.dte', { n: num(g.dte, 1) })}</span>
              <span class="qp-muted">{$t('options.group.atmIv', { v: pct(g.atmIv, 1) })}</span>
              <span class="qp-muted">{$t('options.group.count', { n: g.strikes.length })}</span>
            </button>
          </td>
        </tr>
        {#if !collapsed[g.expiry]}
          {#each g.strikes as s (s.strike)}
            <tr class:atm={s.strike === g.atm}>
              {#if view === 'both'}
                {#each left as c (c.key)}
                  <td class="num cell" class:itm={itm(s.call, s.strike)} class:empty={!s.call} data-c={s.call ? key(s.call) : undefined}>{s.call ? c.fmt(s.call[c.key]) : ''}</td>
                {/each}
                <td class="num strike">{price(s.strike)}</td>
                {#each cols as c (c.key)}
                  <td class="num cell" class:itm={itm(s.put, s.strike)} class:empty={!s.put} data-c={s.put ? key(s.put) : undefined}>{s.put ? c.fmt(s.put[c.key]) : ''}</td>
                {/each}
              {:else if view === 'straddle'}
                <td class="num strike">{price(s.strike)}</td>
                <td class="num cell" class:empty={!s.call} data-c={s.call ? key(s.call) : undefined}>{price(mid(s.call))}</td>
                <td class="num cell" class:empty={!s.put} data-c={s.put ? key(s.put) : undefined}>{price(mid(s.put))}</td>
                <td class="num">{price(sum(s))}</td>
                <td class="num">{pct(sum(s) != null && spot ? sum(s) / spot : null)}</td>
                <td class="num">{price(sum(s) != null ? s.strike - sum(s) : null)}</td>
                <td class="num">{price(sum(s) != null ? s.strike + sum(s) : null)}</td>
                <td class="num">{pct(s.call?.iv_mid)}</td>
                <td class="num">{pct(s.put?.iv_mid)}</td>
              {:else}
                {@const r = view === 'calls' ? s.call : s.put}
                {#if r}
                  <td class="num strike">{price(s.strike)}</td>
                  {#each cols as c (c.key)}
                    <td class="num cell" class:itm={itm(r, s.strike)} data-c={key(r)}>{c.fmt(r[c.key])}</td>
                  {/each}
                {/if}
              {/if}
            </tr>
          {/each}
        {/if}
      {/each}
    </tbody>
  </table>
</div>

<style>
  .wrap {
    max-height: 70vh;
    overflow-y: auto;
  }
  .chain thead tr.cols th {
    position: sticky;
    top: 0;
    z-index: 1;
  }
  .chain thead tr.sides th {
    font-size: var(--fs-desc);
    text-transform: none;
    border-bottom: none;
  }
  /* The strike stays in view while the chain scrolls sideways: pinned to the left edge once
     scrolled past, to the right edge before it is reached (calls mirrored on its left). */
  .strike {
    position: sticky;
    left: 0;
    right: 0;
    z-index: 0;
    text-align: center;
    font-weight: var(--fw-medium);
    background: var(--surface-2);
  }
  .chain thead tr.cols th.strike {
    z-index: 2;
  }
  td.cell {
    cursor: pointer;
  }
  td.cell.empty {
    cursor: default;
  }
  td.itm {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  tr.atm td.strike {
    color: var(--accent);
  }
  .ghead td {
    background: var(--bg);
    padding: 0;
  }
  .gtoggle {
    display: flex;
    gap: var(--space-3);
    align-items: baseline;
    width: 100%;
    background: none;
    border: none;
    color: var(--text);
    text-align: left;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-variant-numeric: tabular-nums;
  }
  .caret {
    width: var(--space-3);
    color: var(--muted);
  }
  /* Narrow screens scroll the chain sideways: the expiry header rides along at the left edge. */
  @media (max-width: 1099px) {
    .gtoggle {
      position: sticky;
      left: 0;
      width: max-content;
      min-height: 44px;
      align-items: center;
    }
  }
  /* Phone: tighter cells, every contract a touch-sized target. */
  @media (max-width: 767px) {
    .wrap {
      max-height: 70dvh;
    }
    .chain th,
    .chain tr:not(.ghead) td {
      padding: var(--space-2);
    }
    .chain td.cell,
    .chain td.strike {
      height: 44px;
    }
  }
</style>
