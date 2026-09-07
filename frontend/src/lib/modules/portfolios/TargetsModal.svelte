<script>
  // The target allocation, edited as one statement.
  //
  // Wholesale, not row by row: an allocation has to add up to 100, and saving one bucket at
  // a time would let the set sit invalid between two calls. The running total is shown live
  // for the same reason the server refuses a set that misses it.
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { fmtPct } from '$lib/format.js';
  import { DRIFT_BUCKETS, portfoliosApi } from './api.js';

  let { portfolioId, onclose, onsaved } = $props();

  let rows = $state([]);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state(null);

  $effect(() => {
    portfoliosApi
      .targets(portfolioId)
      .then((t) => {
        rows = t.map((r) => ({ bucket: r.bucket, target_pct: r.target_pct, band_pct: r.band_pct }));
      })
      .catch((e) => (error = e.message))
      .finally(() => (loading = false));
  });

  const total = $derived(rows.reduce((a, r) => a + (Number(r.target_pct) || 0), 0));
  const balanced = $derived(rows.length === 0 || Math.abs(total - 100) < 0.01);
  const free = $derived(DRIFT_BUCKETS.filter((b) => !rows.some((r) => r.bucket === b)));

  function add(bucket) {
    rows = [...rows, { bucket, target_pct: 0, band_pct: 5 }];
  }
  function remove(i) {
    rows = rows.filter((_, j) => j !== i);
  }
  /** Give the remainder to this row, so the set can be balanced without arithmetic. */
  function fill(i) {
    const others = rows.reduce((a, r, j) => a + (j === i ? 0 : Number(r.target_pct) || 0), 0);
    rows = rows.map((r, j) => (j === i ? { ...r, target_pct: Math.max(0, 100 - others) } : r));
  }

  async function save() {
    if (!balanced || saving) return;
    saving = true;
    error = null;
    try {
      await portfoliosApi.saveTargets(
        portfolioId,
        rows.map((r) => ({
          bucket: r.bucket,
          target_pct: Number(r.target_pct) || 0,
          band_pct: Number(r.band_pct) || 0
        }))
      );
      onsaved?.();
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<Modal open title={$t('portfolios.targets.title')} size="md" {onclose}>
  <p class="intro">{$t('portfolios.targets.intro')}</p>

  {#if loading}
    <p class="muted">{$t('common.loading')}</p>
  {:else}
    <table class="tbl">
      <thead>
        <tr>
          <th>{$t('portfolios.targets.bucket')}</th>
          <th class="r">{$t('portfolios.targets.target')}</th>
          <th class="r">{$t('portfolios.targets.band')}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r, i (r.bucket)}
          <tr>
            <td>{$t(`portfolios.class.${r.bucket}`)}</td>
            <td class="r">
              <input class="pct" type="number" step="any" min="0" max="100" bind:value={r.target_pct} />
              <button class="mini" onclick={() => fill(i)} title={$t('portfolios.targets.fill')}>=</button>
            </td>
            <td class="r">
              <input class="pct" type="number" step="any" min="0" bind:value={r.band_pct} />
            </td>
            <td class="r">
              <button class="mini" onclick={() => remove(i)} aria-label={$t('common.remove')}>
                <Icon name="trash" size={13} />
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>

    {#if free.length}
      <div class="add">
        <span>{$t('portfolios.targets.add')}</span>
        {#each free as b (b)}
          <button class="chip" onclick={() => add(b)}>+ {$t(`portfolios.class.${b}`)}</button>
        {/each}
      </div>
    {/if}

    <p class="total" class:bad={!balanced}>
      {$t('portfolios.targets.total', { pct: fmtPct(total) })}
      {#if !balanced}<span> · {$t('portfolios.targets.mustBe100')}</span>{/if}
    </p>
    {#if rows.length === 0}
      <p class="muted">{$t('portfolios.targets.emptyClears')}</p>
    {/if}
  {/if}

  <ErrorText {error} />

  {#snippet footer()}
    <button class="btn ghost" onclick={onclose}>{$t('common.cancel')}</button>
    <button class="btn primary" onclick={save} disabled={!balanced || saving}>
      {saving ? $t('common.saving') : $t('common.save')}
    </button>
  {/snippet}
</Modal>

<style>
  .intro,
  .muted {
    color: var(--muted);
    font-size: 12px;
  }
  .r {
    text-align: right;
  }
  .pct {
    width: 70px;
    text-align: right;
  }
  .mini {
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    padding: 2px 4px;
  }
  .mini:hover {
    color: var(--text);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    margin-top: var(--space-3);
    font-size: 12px;
    color: var(--muted);
  }
  .chip {
    padding: 2px 9px;
    border: 1px dashed var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 12px;
  }
  .chip:hover {
    border-style: solid;
    color: var(--text);
  }
  .total {
    margin-top: var(--space-3);
    font-weight: 600;
  }
  .total.bad {
    color: var(--amber);
  }
</style>
