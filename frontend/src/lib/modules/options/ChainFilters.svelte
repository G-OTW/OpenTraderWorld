<script>
  // Chain filters: which expiries, which strikes, how wide a spread, and the view. Expiries
  // and the strike window decide what is fetched; the rest filters the loaded chain.
  import { t } from '$lib/i18n';
  import { MAX_EXPIRIES } from './chain.js';

  let {
    expiries = [],
    picked = [],
    expiry = $bindable(),
    strike = $bindable(),
    spread = $bindable(),
    view = $bindable('both')
  } = $props();

  // A chip click starts from what the current preset shows, then toggles that date.
  function toggleDate(d) {
    const base = expiry.mode === 'specific' ? expiry.dates : picked;
    expiry.mode = 'specific';
    expiry.dates = base.includes(d) ? base.filter((x) => x !== d) : [...base, d];
  }

  function setExpiryMode(m) {
    if (m === 'specific' && !expiry.dates.length) expiry.dates = picked.slice(0, MAX_EXPIRIES);
    expiry.mode = m;
  }
</script>

<div class="filters">
  <div class="qp-field wide">
    <span class="qp-label">{$t('options.filter.expiry')}</span>
    <div class="qp-seg">
      {#each ['week', 'month', 'quarter', 'range', 'specific'] as m (m)}
        <button class:active={expiry.mode === m} onclick={() => setExpiryMode(m)}>{$t(`options.filter.expiry.${m}`)}</button>
      {/each}
    </div>
    {#if expiry.mode === 'range'}
      <div class="pair">
        <input type="date" bind:value={expiry.from} aria-label={$t('options.filter.from')} />
        <input type="date" bind:value={expiry.to} aria-label={$t('options.filter.to')} />
      </div>
    {/if}
    {#if expiries.length}
      <div class="chips">
        {#each expiries as e (e.date)}
          <button class="chip" class:on={picked.includes(e.date)} onclick={() => toggleDate(e.date)} title={$t('options.filter.dte', { n: Math.round(e.dte) })}>
            {e.date}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="qp-field">
    <span class="qp-label">{$t('options.filter.strikes')}</span>
    <div class="qp-seg">
      {#each [5, 10, 20] as n (n)}
        <button class:active={strike.mode === 'near' && strike.n === n} onclick={() => ((strike.mode = 'near'), (strike.n = n))}>±{n}</button>
      {/each}
      <button class:active={strike.mode === 'pct5'} onclick={() => (strike.mode = 'pct5')}>±5%</button>
      <button class:active={strike.mode === 'range'} onclick={() => (strike.mode = 'range')}>{$t('options.filter.manual')}</button>
      <button class:active={strike.mode === 'specific'} onclick={() => (strike.mode = 'specific')}>{$t('options.filter.specific')}</button>
    </div>
    {#if strike.mode === 'range'}
      <div class="pair">
        <input type="number" step="any" bind:value={strike.lo} placeholder={$t('options.filter.lo')} aria-label={$t('options.filter.lo')} />
        <input type="number" step="any" bind:value={strike.hi} placeholder={$t('options.filter.hi')} aria-label={$t('options.filter.hi')} />
      </div>
    {:else if strike.mode === 'specific'}
      <input type="text" bind:value={strike.list} placeholder={$t('options.filter.specificHint')} aria-label={$t('options.filter.specific')} />
    {/if}
  </div>

  <div class="qp-field">
    <span class="qp-label">{$t('options.filter.spread')}</span>
    <div class="qp-seg">
      {#each ['any', 'tight', 'medium', 'wide', 'manual'] as m (m)}
        <button class:active={spread.mode === m} onclick={() => (spread.mode = m)}>{$t(`options.filter.spread.${m}`)}</button>
      {/each}
    </div>
    {#if spread.mode === 'manual'}
      <input type="number" min="0" step="1" bind:value={spread.value} placeholder="%" aria-label={$t('options.filter.spreadMax')} />
    {/if}
  </div>

  <div class="qp-field">
    <span class="qp-label">{$t('options.view.label')}</span>
    <div class="qp-seg">
      {#each ['calls', 'puts', 'both', 'straddle'] as v (v)}
        <button class:active={view === v} onclick={() => (view = v)}>{$t(`options.view.${v}`)}</button>
      {/each}
    </div>
  </div>
</div>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    min-width: 0;
  }
  .wide {
    flex-basis: 100%;
  }
  .pair {
    display: flex;
    gap: var(--space-2);
  }
  .pair input {
    flex: 1 1 0;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .chip {
    font-family: var(--mono);
    font-size: var(--fs-desc);
    background: var(--surface-2);
    border: var(--hairline) solid transparent;
    border-radius: var(--radius-sm);
    color: var(--muted);
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .chip.on {
    border-color: var(--accent);
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
  }
  /* Phone: one filter per line, touch-sized buttons and chips. */
  @media (max-width: 767px) {
    .filters .qp-field {
      flex: 1 1 100%;
    }
    .qp-seg button,
    .chip,
    .filters input {
      min-height: 44px;
    }
    .pair input {
      min-width: 0;
    }
  }
</style>
