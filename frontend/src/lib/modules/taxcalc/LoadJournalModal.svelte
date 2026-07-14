<script>
  // Load realized PnL from the Trading Journal into the tax form. Pick a category (or all)
  // and a tax year; we run the journal breakdown once per asset-class bucket over that year
  // and route each bucket's realized (net-of-fees) PnL into the matching itemized tax input:
  //   stock / etf / other  → capital gains
  //   option / future / forex → derivative gains
  //   crypto               → crypto gains
  // Journal figures come back in the journal's display currency. The tax engine has no FX, so
  // when that differs from the profile currency we convert here (year-end rate) before filling
  // the form. Rates are USD-based (1 USD = rate·quote); cross-rate = amount·rate(to)/rate(from).
  import Modal from '$lib/ui/Modal.svelte';
  import { journalApi } from '$lib/modules/journal/api.js';
  import { fmtMoney } from '$lib/modules/taxcalc/api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

  let { open = $bindable(false), profileCurrency = '', onapply = () => {} } = $props();

  // Which journal asset classes feed which tax bucket.
  const BUCKETS = {
    capital: ['stock', 'etf', 'other'],
    derivative: ['option', 'future', 'forex'],
    crypto: ['crypto']
  };

  let categories = $state([]);
  let categoryId = $state(''); // '' = all categories
  let taxYear = $state(new Date().getFullYear());
  let loading = $state(false);
  let error = $state('');
  let preview = $state(null); // { capital, derivative, crypto, currency, trades, fees, unconverted }

  $effect(() => {
    if (!open) return;
    journalApi
      .listCategories()
      .then((cs) => (categories = cs))
      .catch((e) => (error = e.message));
  });

  // RFC3339 bounds for the tax year (trade effective date is filtered by since/until).
  function yearBounds(y) {
    return {
      since: `${y}-01-01T00:00:00Z`,
      until: `${y}-12-31T23:59:59Z`
    };
  }

  async function load() {
    error = '';
    preview = null;
    loading = true;
    try {
      const { since, until } = yearBounds(Number(taxYear));
      const base = { since, until };
      if (categoryId) base.category_id = categoryId;

      const buckets = { capital: 0, derivative: 0, crypto: 0 };
      let currency = 'USD';
      let trades = 0;
      let fees = 0;
      let unconverted = 0;

      // One breakdown per asset class keeps the routing exact (the breakdown itself has no
      // per-asset split). Sequential to keep it simple; the set is small (7 classes).
      for (const [bucket, classes] of Object.entries(BUCKETS)) {
        for (const asset_class of classes) {
          const b = await journalApi.breakdown({ ...base, asset_class });
          if (!b) continue;
          buckets[bucket] += b.realized_pnl || 0;
          currency = b.display_currency || currency;
          trades += b.closed_count || 0;
          fees += b.total_fees || 0;
          unconverted += b.unconverted_trades || 0;
        }
      }

      // Convert journal → profile currency when they differ (tax engine is FX-blind). Use the
      // tax year's end-of-year rate (carry-forward). If the rate is unavailable, leave the
      // figures in journal currency and flag it so the user converts manually.
      let outCurrency = currency;
      let fxNote = '';
      const target = (profileCurrency || '').toUpperCase();
      if (target && target !== currency) {
        const rate = await fxFactor(currency, target, Number(taxYear));
        if (rate == null) {
          fxNote = $t('taxcalc.loadJournal.noFxRate', { from: currency, to: target });
        } else {
          buckets.capital *= rate;
          buckets.derivative *= rate;
          buckets.crypto *= rate;
          fees *= rate;
          outCurrency = target;
          fxNote = $t('taxcalc.loadJournal.fxConverted', { from: currency, to: target, year: taxYear });
        }
      }

      preview = {
        capital: round2(buckets.capital),
        derivative: round2(buckets.derivative),
        crypto: round2(buckets.crypto),
        currency: outCurrency,
        trades,
        fees: round2(fees),
        unconverted,
        fxNote
      };
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // Cross-rate factor from → to on the given year's last day. Rates are USD-based
  // (quote missing ⇒ USD itself, factor 1). Returns null if a needed rate is missing.
  async function fxFactor(from, to, year) {
    if (from === to) return 1;
    const rates = await journalApi.fxRatesOn(`${year}-12-31`);
    const r = (ccy) => (ccy === 'USD' ? 1 : rates?.[ccy]);
    const rf = r(from);
    const rt = r(to);
    if (rf == null || rt == null || rf === 0) return null;
    return rt / rf; // amount_to = amount_from · rate(USD→to) / rate(USD→from)
  }

  function round2(n) {
    return Math.round((n + Number.EPSILON) * 100) / 100;
  }

  function apply() {
    if (!preview) return;
    onapply({
      realized_capital_gains: preview.capital,
      derivative_gains: preview.derivative,
      crypto_gains: preview.crypto,
      currency: preview.currency
    });
    open = false;
  }
</script>

<Modal bind:open title={$t('taxcalc.loadJournal.title')} size="md">
  <ErrorText error={error} />

  <div class="row">
    <label class="grow">
      {$t('taxcalc.loadJournal.category')}
      <select bind:value={categoryId}>
        <option value="">{$t('taxcalc.loadJournal.allCategories')}</option>
        {#each categories as c (c.id)}
          <option value={c.id}>{c.name}</option>
        {/each}
      </select>
    </label>
    <label>
      {$t('taxcalc.loadJournal.taxYear')}
      <input type="number" bind:value={taxYear} />
    </label>
  </div>

  <button class="primary" onclick={load} disabled={loading}>
    {loading ? $t('common.loading') : $t('taxcalc.loadJournal.loadPnl')}
  </button>

  {#if preview}
    <div class="preview">
      <p class="muted small">
        {preview.trades === 1
          ? $t('taxcalc.loadJournal.summarySingular', { year: taxYear, fees: fmtMoney(preview.fees, preview.currency), currency: preview.currency })
          : $t('taxcalc.loadJournal.summaryPlural', { count: preview.trades, year: taxYear, fees: fmtMoney(preview.fees, preview.currency), currency: preview.currency })}
      </p>
      <table class="tbl">
        <tbody>
          <tr>
            <td>{$t('taxcalc.loadJournal.capitalGains')} <span class="muted">{$t('taxcalc.loadJournal.capitalGainsHint')}</span></td>
            <td class="num">{fmtMoney(preview.capital, preview.currency)}</td>
          </tr>
          <tr>
            <td>{$t('taxcalc.loadJournal.derivativeGains')} <span class="muted">{$t('taxcalc.loadJournal.derivativeGainsHint')}</span></td>
            <td class="num">{fmtMoney(preview.derivative, preview.currency)}</td>
          </tr>
          <tr>
            <td>{$t('taxcalc.loadJournal.cryptoGains')}</td>
            <td class="num">{fmtMoney(preview.crypto, preview.currency)}</td>
          </tr>
        </tbody>
      </table>
      {#if preview.unconverted > 0}
        <p class="warn small">
          {preview.unconverted === 1
            ? $t('taxcalc.loadJournal.unconvertedSingular')
            : $t('taxcalc.loadJournal.unconvertedPlural', { count: preview.unconverted })}
        </p>
      {/if}
      {#if preview.fxNote}
        <p class="{preview.currency === profileCurrency?.toUpperCase() ? 'muted' : 'warn'} small">
          {preview.fxNote}
        </p>
      {/if}
      <p class="muted small">
        {$t('taxcalc.loadJournal.applyHint')}
      </p>
    </div>
  {/if}

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={apply} disabled={!preview}>{$t('taxcalc.loadJournal.applyToForm')}</button>
  {/snippet}
</Modal>

<style>
  .row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
    margin-bottom: var(--space-3);
  }
  .grow {
    flex: 1;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .preview {
    margin-top: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  td .muted {
    font-size: var(--text-xs);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
  }
  .warn {
    color: var(--amber);
  }
</style>
