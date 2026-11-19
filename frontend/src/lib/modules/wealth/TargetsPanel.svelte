<script>
  // Wealth-level targets: the share each category should hold, with a band; the drift table
  // reads the same math as Portfolio targets. "Set targets" edits the whole plan.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import { wealthApi } from './api.js';
  import { signedMoney, pct } from './view.js';
  import { fmtSignedPct } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let { reloadKey = 0, categories = [] } = $props();

  let data = $state(null);
  let error = $state('');
  let open = $state(false);
  let draft = $state([]);
  let saveError = $state('');
  let saving = $state(false);

  $effect(() => {
    void reloadKey;
    load();
  });

  async function load() {
    error = '';
    try {
      data = await wealthApi.targets();
    } catch (e) {
      error = e.message;
    }
  }

  function edit() {
    const known = new Map((data?.targets ?? []).map((x) => [x.category, x]));
    const names = [...new Set([...categories, ...known.keys()])];
    draft = names.map((c) => ({
      category: c,
      target_pct: known.get(c)?.target_pct ?? '',
      band_pct: known.get(c)?.band_pct ?? 5
    }));
    saveError = '';
    open = true;
  }

  const draftTotal = $derived(draft.reduce((s, d) => s + (Number(d.target_pct) || 0), 0));

  async function save() {
    saving = true;
    saveError = '';
    try {
      const targets = draft
        .filter((d) => d.target_pct !== '' && d.target_pct != null)
        .map((d) => ({ category: d.category, target_pct: Number(d.target_pct), band_pct: Number(d.band_pct) || 0 }));
      data = await wealthApi.saveTargets(targets);
      open = false;
    } catch (e) {
      saveError = e.message;
    }
    saving = false;
  }

  const cur = $derived(data?.display_currency ?? 'USD');
  const report = $derived(data?.drift?.ok ? data.drift.report : null);
  const reason = $derived(data?.drift?.ok === false ? data.drift.reason : null);
  const label = (c) => c || $t('wealth.page.uncategorized');
  // Drift is neither gain nor loss: signed, never toned.
  const signedPct = (v) => (v == null ? $t('wealth.page.na') : fmtSignedPct(v, 1));
  // One symmetric scale for every row: the widest deviation or band, with a little air.
  const span = $derived(
    Math.max(1, ...(report?.rows ?? []).map((r) => Math.max(Math.abs(r.deviation_pct ?? 0), Math.abs(r.band_pct ?? 0)))) * 1.15
  );
  const at = (v) => 50 + (Math.max(-span, Math.min(span, v)) / span) * 50;
</script>

<!-- Deviation from target on a diverging track: the band is a muted range around zero, the
     bar grows from zero toward under or over. Diverging hues, not gain and loss: being over
     a target is not good or bad in itself. -->
{#snippet drift(r)}
  <span class="drift" aria-hidden="true">
    <span class="band" style:left="{at(-r.band_pct)}%" style:width="{at(r.band_pct) - at(-r.band_pct)}%"></span>
    <span class="zero"></span>
    <span
      class="dev"
      class:neg={r.deviation_pct < 0}
      style:left="{Math.min(at(0), at(r.deviation_pct))}%"
      style:width="{Math.abs(at(r.deviation_pct) - at(0))}%"
    ></span>
  </span>
{/snippet}

<div class="panel">
  <div class="bar">
    {#if report}
      <span class="status" class:off={!report.in_band}>
        {#if !report.in_band}<span class="dot" aria-hidden="true"></span>{/if}
        {report.in_band ? $t('wealth.targets.inBand') : $t('wealth.targets.offBand', { pct: pct(report.worst_deviation_pct) })}
      </span>
    {:else}<span></span>{/if}
    <button class="btn sm" onclick={edit}><Icon name="target" size={13} /> {$t('wealth.targets.set')}</button>
  </div>
  <ErrorText {error} />
  {#if reason}
    <EmptyState icon="target" description={$t(`wealth.targets.reason.${reason}`)} compact />
  {:else if report}
    <div class="tbl-wrap">
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('wealth.page.colCategory')}</th>
            <th class="num">{$t('wealth.targets.current')}</th>
            <th class="num">{$t('wealth.targets.target')}</th>
            <th class="track-h">{$t('wealth.targets.deviation')}</th>
            <th class="num">{$t('wealth.targets.toTarget')}</th>
          </tr>
        </thead>
        <tbody>
          {#each report.rows as r (r.category)}
            <tr>
              <td class="name" title={label(r.category)}>
                {#if r.out_of_band}<span class="dot" aria-hidden="true"></span>{/if}{label(r.category)}
                {#if r.out_of_band}<span class="off-word">{$t('wealth.targets.offWord')}</span>{/if}
              </td>
              <td class="num">{pct(r.current_pct)}</td>
              <td class="num nowrap">{pct(r.target_pct)} <span class="band-txt">±{pct(r.band_pct)}</span></td>
              <td class="track">
                <span class="track-cell">
                  {@render drift(r)}
                  <span class="signed">{signedPct(r.deviation_pct)}</span>
                </span>
              </td>
              <td class="num signed">{signedMoney(r.delta_value, cur)}</td>
            </tr>
          {/each}
          {#each report.unallocated as u (u.category)}
            <tr class="unalloc">
              <td class="name" title={label(u.category)}>{label(u.category)}</td>
              <td class="num">{pct(u.current_pct)}</td>
              <td class="word" colspan="3">{$t('wealth.targets.unallocated')}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="legend"><span class="key band-key"></span>{$t('wealth.targets.bandLegend')}</p>
  {/if}
</div>

<Modal bind:open size="md" title={$t('wealth.targets.set')}>
  <p class="intro">{$t('wealth.targets.intro')}</p>
  <div class="tbl-wrap">
    <table class="tbl edit">
      <thead>
        <tr>
          <th>{$t('wealth.page.colCategory')}</th>
          <th class="num">{$t('wealth.targets.target')}</th>
          <th class="num">{$t('wealth.targets.band')}</th>
        </tr>
      </thead>
      <tbody>
        {#each draft as d (d.category)}
          <tr>
            <td>{label(d.category)}</td>
            <td class="num"><input type="number" min="0" max="100" step="any" bind:value={d.target_pct} aria-label="{$t('wealth.targets.target')}: {label(d.category)}" /></td>
            <td class="num"><input type="number" min="0" step="any" bind:value={d.band_pct} aria-label="{$t('wealth.targets.band')}: {label(d.category)}" /></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
  <p class="small" class:bad={draftTotal > 100}>{$t('wealth.targets.total', { pct: pct(draftTotal) })}</p>
  <ErrorText error={saveError} />
  {#snippet footer()}
    <button class="btn ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={save} disabled={saving || draftTotal > 100}>{saving ? $t('common.saving') : $t('common.save')}</button>
  {/snippet}
</Modal>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    flex-wrap: wrap;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--muted);
    font-size: 12px;
  }
  .status.off {
    color: var(--amber-ink);
  }
  .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--amber);
    margin-right: var(--space-2);
    flex: none;
  }
  .status .dot {
    margin-right: 0;
  }
  .tbl-wrap {
    overflow-x: auto;
  }
  table {
    width: 100%;
    min-width: 560px;
    table-layout: fixed;
  }
  th:first-child {
    width: 26%;
  }
  .track-h {
    width: 30%;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .off-word {
    margin-left: var(--space-2);
    color: var(--amber-ink);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .band-txt {
    color: var(--dim);
    font-size: 11px;
  }
  .track-cell {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 56px;
    align-items: center;
    gap: var(--space-2);
  }
  .signed {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    font-size: 12px;
    color: var(--muted);
    text-align: right;
  }
  td.signed {
    color: var(--text);
  }
  .nowrap {
    white-space: nowrap;
  }
  .drift {
    position: relative;
    height: 12px;
    display: block;
  }
  .band {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--border-control);
    border-radius: 2px;
  }
  .zero {
    position: absolute;
    left: 50%;
    top: -3px;
    bottom: -3px;
    width: 1px;
    background: var(--faint);
  }
  .dev {
    position: absolute;
    top: 3px;
    height: 6px;
    background: var(--diverge-pos);
    border-radius: 0 3px 3px 0;
  }
  .dev.neg {
    background: var(--diverge-neg);
    border-radius: 3px 0 0 3px;
  }
  tr.unalloc td {
    color: var(--muted);
  }
  .word {
    color: var(--dim);
    font-size: 12px;
  }
  .legend {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--dim);
    font-size: 11px;
  }
  .key {
    display: inline-block;
    width: 14px;
    height: 8px;
    border-radius: 2px;
  }
  .band-key {
    background: var(--border-control);
  }
  .intro {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: 12px;
  }
  .small {
    font-size: 12px;
    margin: var(--space-2) 0 0;
    color: var(--muted);
    font-family: var(--mono);
  }
  .bad {
    color: var(--red-ink);
  }
  .edit {
    min-width: 0;
  }
  .edit input {
    width: 90px;
    text-align: right;
    font-family: var(--mono);
  }
  @media (max-width: 767px) {
    .bar button {
      min-height: 44px;
    }
  }
</style>
