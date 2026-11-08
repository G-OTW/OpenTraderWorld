<script>
  import { t } from '$lib/i18n';
  import { heatShade } from '../insights.js';
  let { rows = [], columns = [], values = [], counts = [], format = String, mode = 'return', label = '', max = 1, compact = [], compactLabel = '' } = $props();
  let detail = $state('');
  const describe = (i, j, v) => `${rows[i]} · ${columns[j]}: ${Number.isFinite(v) ? format(v) : $t('quant.seasonality.noData')}${counts[i]?.[j] != null ? ` · ${$t('quant.seasonality.samples', { n: counts[i][j] })}` : ''}`;
</script>

<div class="matrix-wrap">
  <div class="matrix-scroll">
    <table class="matrix" aria-label={label}>
      <thead><tr><td></td>{#each columns as col, i (i)}<th scope="col">{col}</th>{/each}</tr></thead>
      <tbody>
        {#each values as row, i (i)}
          <tr><th scope="row">{rows[i]}</th>
            {#each row as v, j (j)}
              <td>
                <button type="button" class="cell" class:missing={!Number.isFinite(v)}
                  style:background={heatShade(v, max, mode)} aria-label={describe(i, j, v)}
                  title={describe(i, j, v)} onfocus={() => detail = describe(i, j, v)}
                  onpointerenter={() => detail = describe(i, j, v)} onclick={() => detail = describe(i, j, v)}>
                  {Number.isFinite(v) ? format(v) : '—'}
                </button>
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  {#if compact.length}
    <div class="compact">
      <p class="compact-label">{compactLabel}</p>
      <ul>{#each compact as cell (cell.label)}<li>
        <span>{cell.label}</span><span>{Number.isFinite(cell.value) ? format(cell.value) : '—'}</span>
        {#if cell.count != null}<span class="samples">{$t('quant.seasonality.samples', { n: cell.count })}</span>{/if}
      </li>{/each}</ul>
    </div>
  {/if}
  <div class="legend">
    <span>{mode === 'correlation' ? '−1' : mode === 'return' ? format(-max) : format(0)}</span>
    <span class="ramp" class:unsigned={mode !== 'return' && mode !== 'correlation'}></span>
    <span>{mode === 'correlation' ? '+1' : format(max)}</span>
  </div>
  <p class="detail" aria-live="polite">{detail || $t('dashboard.widgets.insights.inspectCell')}</p>
</div>

<style>
  .matrix-wrap { min-width: 0; }
  .compact { display: none; }
  .compact-label { margin: 0 0 6px; color: var(--muted); font-size: 11px; }
  .compact ul { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; padding: 0; margin: 0; list-style: none; }
  .compact li { display: flex; flex-direction: column; gap: 3px; font: 11px var(--mono); color: var(--text); overflow-wrap: anywhere; }
  .samples { font: 10px var(--font); color: var(--muted); }
  .matrix-scroll { overflow-x: auto; scrollbar-width: thin; }
  .matrix { width: 100%; border-collapse: separate; border-spacing: 3px; table-layout: auto; }
  th { font-size: 11px; font-weight: 500; color: var(--muted); white-space: nowrap; text-align: center; }
  tbody th { text-align: left; padding-right: 6px; }
  td { padding: 0; }
  .cell { min-width: 44px; width: 100%; min-height: 26px; padding: 4px; border: 0; border-radius: 3px; color: var(--text); font: 11px var(--mono); font-variant-numeric: tabular-nums; cursor: pointer; }
  .cell:hover, .cell:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .missing { background-image: repeating-linear-gradient(135deg, transparent, transparent 4px, var(--border) 4px, var(--border) 5px); }
  .legend { display: flex; align-items: center; gap: 8px; margin-top: 8px; color: var(--muted); font: 11px var(--mono); }
  .ramp { flex: 1; height: 5px; border-radius: 3px; background: linear-gradient(90deg, var(--diverge-neg), var(--surface-2), var(--diverge-pos)); }
  .ramp.unsigned { background: linear-gradient(90deg, var(--surface-2), var(--amber)); }
  .detail { color: var(--muted); font-size: 11px; line-height: 1.4; min-height: 2.8em; margin: 6px 0 0; overflow-wrap: anywhere; }
  @container widget (max-width: 360px) {
    .matrix-wrap:has(.compact) .matrix-scroll, .matrix-wrap:has(.compact) .legend, .matrix-wrap:has(.compact) .detail { display: none; }
    .compact { display: block; }
  }
</style>
