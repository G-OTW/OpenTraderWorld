<script>
  // The loss carry-forward registry of one profile.
  //
  // A row is one tax year's own net result in one netting pool (negative = a loss). What
  // each pool still carries into the year on the form is derived by the server from those
  // rows: oldest loss first, consumed by later gains, dropped after the regime's years. So
  // the user records facts (a year's result) and never maintains a running balance.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { taxcalcApi, fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  let {
    profileId = '',
    taxYear = new Date().getFullYear(),
    currency = 'USD',
    result = null,
    hasRows = $bindable(false),
    onchanged = () => {}
  } = $props();

  let data = $state(null);
  let error = $state('');
  let busy = $state(false);
  let draftYear = $state(new Date().getFullYear() - 1);
  let draftPool = $state('');
  let draftNet = $state('');
  let draftNote = $state('');

  const poolLabel = (k) => $t(`taxcalc.losses.pool.${k}`);
  const yearsLabel = (y) =>
    y == null
      ? $t('taxcalc.losses.noLimit')
      : y === 0
        ? $t('taxcalc.losses.notCarried')
        : $t('taxcalc.losses.years', { years: y });
  const poolOpts = $derived(
    (data?.pools ?? []).map((p) => ({ value: p.key, label: poolLabel(p.key) }))
  );
  // A computed result for the year on the form can be written to the registry as is.
  const recordable = $derived(!!result?.pools?.length && !!profileId);

  $effect(() => {
    if (profileId) load(profileId, taxYear);
    else data = null;
  });

  async function load(id, year) {
    error = '';
    try {
      data = await taxcalcApi.losses(id, Number(year));
      hasRows = data.rows.length > 0;
      if (!data.pools.some((p) => p.key === draftPool)) draftPool = data.pools[0]?.key ?? '';
    } catch (e) {
      error = e.message;
    }
  }

  async function run(fn) {
    busy = true;
    error = '';
    try {
      await fn();
      await load(profileId, taxYear);
      onchanged();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  const add = () =>
    run(async () => {
      if (draftNet === '' || !draftPool) return;
      await taxcalcApi.putLoss(profileId, Number(draftYear), draftPool, Number(draftNet), draftNote);
      draftNet = '';
      draftNote = '';
    });

  const remove = (row) => run(() => taxcalcApi.deleteLoss(profileId, row.tax_year, row.pool));

  const record = () =>
    run(async () => {
      for (const p of result.pools) {
        await taxcalcApi.putLoss(profileId, Number(taxYear), p.pool, p.net, '');
      }
    });
</script>

{#if data}
  <div class="reg">
    <div class="head">
      <h2>{$t('taxcalc.losses.title')}</h2>
      {#if recordable}
        <button class="ghost sm" onclick={record} disabled={busy}>
          {$t('taxcalc.losses.record', { year: taxYear })}
        </button>
      {/if}
    </div>
    <p class="muted small">{$t('taxcalc.losses.hint')}</p>

    <div class="carry">
      {#each data.carry as c (c.pool)}
        <div class="pool">
          <div class="pool-head">
            <strong>{poolLabel(c.pool)}</strong>
            <span class="muted small">{yearsLabel(c.years)}</span>
          </div>
          <div class="amount">
            {$t('taxcalc.losses.available', { year: data.year })}
            <strong>{fmtMoney(c.available, currency)}</strong>
          </div>
          {#each c.losses as l (l.origin)}
            <div class="muted small">
              {l.origin}: {fmtMoney(l.left, currency)}
              {#if l.expires != null}· {$t('taxcalc.losses.until', { year: l.expires })}{/if}
            </div>
          {/each}
        </div>
      {/each}
    </div>

    {#if data.rows.length}
      <table class="tbl">
        <thead>
          <tr>
            <th>{$t('taxcalc.losses.year')}</th>
            <th>{$t('taxcalc.losses.poolCol')}</th>
            <th class="num">{$t('taxcalc.losses.net')}</th>
            <th>{$t('taxcalc.losses.note')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each data.rows as r (r.tax_year + r.pool)}
            <tr>
              <td>{r.tax_year}</td>
              <td>{poolLabel(r.pool)}</td>
              <td class="num" class:up={r.net > 0} class:down={r.net < 0}>{fmtMoney(r.net, currency)}</td>
              <td class="muted">{r.note}</td>
              <td class="act">
                <button
                  class="link red icon"
                  aria-label={$t('common.delete')}
                  title={$t('common.delete')}
                  onclick={() => remove(r)}
                  disabled={busy}
                >
                  <Icon name="trash" size={13} />
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    <div class="add">
      <label>
        {$t('taxcalc.losses.year')}
        <input type="number" step="1" bind:value={draftYear} />
      </label>
      <div class="fld">
        <span>{$t('taxcalc.losses.poolCol')}</span>
        <Dropdown
          value={draftPool}
          options={poolOpts}
          ariaLabel={$t('taxcalc.losses.poolCol')}
          onpick={(v) => (draftPool = v)}
        />
      </div>
      <label>
        {$t('taxcalc.losses.netInput', { currency })}
        <input type="number" step="0.01" bind:value={draftNet} placeholder="-1500" />
      </label>
      <label class="grow">
        {$t('taxcalc.losses.note')}
        <input bind:value={draftNote} />
      </label>
      <button class="primary sm" onclick={add} disabled={busy || draftNet === ''}>
        {$t('taxcalc.losses.add')}
      </button>
    </div>

    <ErrorText {error} copyable />
  </div>
{/if}

<style>
  .reg {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  h2 {
    margin: 0;
    font-size: var(--text-md, 1rem);
  }
  .carry {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
  }
  .pool {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
  }
  .pool-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--space-2);
  }
  .amount {
    font-size: var(--text-sm);
    color: var(--muted);
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .amount strong {
    color: var(--text);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .tbl {
    width: 100%;
    border-collapse: collapse;
  }
  .tbl td,
  .tbl th {
    padding: var(--space-1) var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    font-size: var(--text-sm);
    text-align: left;
  }
  .tbl th {
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: var(--fw-medium);
  }
  .num {
    text-align: right !important;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .act {
    width: 1%;
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: flex-end;
  }
  .add label,
  .add .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .add input[type='number'] {
    width: 120px;
  }
  .grow {
    flex: 1;
    min-width: 140px;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
    margin: 0;
  }
  .link {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .link.red:hover {
    color: var(--red);
  }
</style>
