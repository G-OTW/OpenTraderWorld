<script>
  // Grid view: one row per item. Last close, change on the bar, change from the open, volume
  // against its average, each signal lit or not on the last closed bar, the last hit, and a
  // sparkline. Sortable by any column; "hits only" keeps the items that have hit.
  // `columns` (keys of GRID_COLUMNS, `signal:<id>`, `trend`) picks what shows, in the grid's
  // own order; without it every measure, the judged signals and the trend show. `limit`
  // keeps the first rows after sorting.
  import Sparkline from '$lib/modules/watchlists/Sparkline.svelte';
  import { chartUrl } from './api.js';
  import { GRID_COLUMNS, cssColor, itemColor, rowMetrics, sortRows, sparkCloses } from './board.js';
  import { fmtNum, fmtSignedPct, relativeTime } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { items = [], hives = [], signals = [], live = {}, sort = 'ticker', hitsOnly = false, columns = null, limit = 0, onsort } = $props();

  const rows = $derived(
    sortRows(hitsOnly ? items.filter((i) => i.last_hit_at) : items, sort, live).slice(0, limit > 0 ? limit : undefined)
  );
  // Signals that have been judged on at least one item of the view, in library order. A
  // signal picked as a column shows even before its first judgement.
  const judged = $derived(
    columns
      ? signals.filter((s) => columns.includes(`signal:${s.id}`))
      : signals.filter((s) => items.some((i) => typeof i.signal_state?.[s.id] === 'boolean'))
  );
  const trend = $derived(!columns || columns.includes('trend'));

  const cols = $derived([
    ['ticker', 'scanner.col.item'],
    ...(columns ? GRID_COLUMNS.filter(([k]) => columns.includes(k)) : GRID_COLUMNS)
  ]);
  const ago = (v) => {
    const r = relativeTime(v);
    return $t(r.key, r.params);
  };
  const tone = (v) => (v == null ? '' : v > 0 ? 'pos' : v < 0 ? 'neg' : '');
</script>

{#if !rows.length}
  <p class="hint pad">{hitsOnly ? $t('scanner.board.noHits') : $t('scanner.board.noItems')}</p>
{:else}
  <div class="scroll">
    <table class="tbl">
      <thead>
        <tr>
          {#each cols as [k, label] (k)}
            <th class:num={k !== 'ticker'} aria-sort={sort === k ? 'descending' : 'none'}>
              <button class="sorter" class:on={sort === k} onclick={() => onsort?.(k === 'close' ? 'ticker' : k)}>{$t(label)}</button>
            </th>
          {/each}
          {#each judged as s (s.id)}
            <th class="sig" title={s.name}>
              <button class="sorter" class:on={sort === `signal:${s.id}`} onclick={() => onsort?.(`signal:${s.id}`)}>{s.name}</button>
            </th>
          {/each}
          {#if trend}<th>{$t('scanner.col.trend')}</th>{/if}
        </tr>
      </thead>
      <tbody>
        {#each rows as it (it.id)}
          {@const m = rowMetrics(it, live[it.id])}
          <tr>
            {#each cols as [k] (k)}
              {#if k === 'ticker'}
                <td>
                  <span class="dot" style="--c:{cssColor(itemColor(it, hives)) || 'var(--border-control)'}"></span>
                  <a class="sym" href={chartUrl(it)}>{it.ticker}</a>
                  <span class="tf">{it.timeframe}</span>
                  {#if live[it.id]}<span class="live" title={$t('scanner.board.liveTitle')}></span>{/if}
                </td>
              {:else if k === 'close'}
                <td class="num">{m.close != null ? fmtNum(m.close, 4) : ''}</td>
              {:else if k === 'volume_ratio'}
                <td class="num">{m.volume_ratio != null ? `${fmtNum(m.volume_ratio, 2)}x` : ''}</td>
              {:else if k === 'hit_recency'}
                <td class="num dim">{it.last_hit_at ? ago(it.last_hit_at) : ''}</td>
              {:else}
                <td class="num {tone(m[k])}">{m[k] != null ? fmtSignedPct(m[k]) : ''}</td>
              {/if}
            {/each}
            {#each judged as s (s.id)}
              {@const on = it.signal_state?.[s.id]}
              <td class="sig">
                <span
                  class="lit"
                  class:on={on === true}
                  style="--c:{cssColor(s.color) || 'var(--accent)'}"
                  aria-label={on === true ? $t('scanner.board.lit') : on === false ? $t('scanner.board.unlit') : $t('scanner.board.unjudged')}
                ></span>
              </td>
            {/each}
            {#if trend}<td><Sparkline values={sparkCloses(it.spark)} width={96} height={22} /></td>{/if}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<style>
  .scroll {
    overflow-x: auto;
  }
  .pad {
    padding: var(--space-4);
  }
  .sorter {
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .sorter.on {
    color: var(--text);
    text-decoration: underline;
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
    margin-right: var(--space-1);
  }
  .sym {
    font-family: var(--mono);
    color: var(--text);
    text-decoration: none;
  }
  .sym:hover {
    text-decoration: underline;
  }
  .tf,
  .dim {
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .tf {
    margin-left: var(--space-1);
  }
  .live {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--green);
    margin-left: var(--space-1);
  }
  .sig {
    text-align: center;
    max-width: 90px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lit {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 1px solid var(--border-control);
  }
  .lit.on {
    background: var(--c);
    border-color: var(--c);
  }
</style>
