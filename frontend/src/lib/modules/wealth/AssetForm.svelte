<script>
  // Create/edit an asset: pick a template (or none), name, type, currency, category, plus
  // the two fields that make the headline honest: whether it is owed rather than owned, and
  // how often its valuation should be revisited. Per kind: a loan's terms (its balance then
  // follows the schedule), a wallet's chain and address (read on chain), and for anything
  // valued by hand an assumed yearly drift. Any owned asset can name the debt held against it.
  import { CURRENCIES, ASSET_TYPES, CREATABLE_TYPES, CHAINS, signOf, REVIEW_DEFAULTS, fmtMoney } from './api.js';
  import { annuity, walletAddressOk } from './engine.js';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let {
    initial = null,
    templates = [],
    categories = [],
    /** Every asset, to offer the debts this one can be paired with. */
    assets = [],
    /** Owners already in use, for the suggestion list. */
    owners = [],
    // New assets default to the module's display currency; still editable, and an existing
    // asset always keeps its own stored currency (it's the FX reference).
    defaultCurrency = 'USD',
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  let a = $state(blank());

  function blank() {
    const base = {
      template_id: null,
      name: '',
      asset_type: 'money',
      currency: defaultCurrency,
      category: '',
      sign: 1,
      portfolio_id: null,
      review_days: 0,
      paired_liability_id: null,
      drift_pct_year: '',
      broker_account_id: null,
      chain: 'btc',
      address: '',
      owner: ''
    };
    const loan = { principal: '', rate_pct: '', term_months: '', start: '', payment: '' };
    if (initial) {
      return {
        ...base,
        ...initial,
        category: initial.category ?? '',
        owner: initial.owner ?? '',
        chain: initial.chain ?? 'btc',
        address: initial.address ?? '',
        drift_pct_year: initial.drift_pct_year ?? '',
        loanOn: initial.loan != null,
        loan: { ...loan, ...(initial.loan ?? {}), payment: initial.loan?.payment ?? '' }
      };
    }
    return { ...base, loanOn: false, loan };
  }

  // Picking a template defaults the asset type to the template's type.
  function onTemplate(v) {
    const id = v || null;
    a.template_id = id;
    const tpl = templates.find((t) => t.id === id);
    if (tpl) a.asset_type = tpl.asset_type;
  }

  // A linked asset is valued from its portfolio, so its type, sign and review interval are
  // not the user's to change here: the link flow owns them.
  const linked = $derived(a.portfolio_id != null || a.broker_account_id != null);
  const isWallet = $derived(a.asset_type === 'wallet');
  const isLoan = $derived(a.asset_type === 'loan');
  // Drift is an assumption about something valued by hand: never on a read or computed value.
  const canDrift = $derived(!linked && !isWallet && !(isLoan && a.loanOn));
  const addressOk = $derived(!isWallet || walletAddressOk(a.chain, a.address));
  const liabilities = $derived(assets.filter((x) => x.sign === -1 && x.id !== initial?.id));
  const preview = $derived(annuity(a.loan.principal, a.loan.rate_pct, a.loan.term_months));
  const num = (v) => (v === '' || v === null || v === undefined ? null : Number(v));
  // An older row may carry a type that is no longer offered; keep it in the list so editing
  // it does not silently reclassify it.
  const typeOptions = $derived(
    CREATABLE_TYPES.some((t) => t.id === a.asset_type)
      ? CREATABLE_TYPES
      : ASSET_TYPES.filter((t) => CREATABLE_TYPES.includes(t) || t.id === a.asset_type)
  );

  function onType(v) {
    a.asset_type = v;
    // The sign follows the kind of thing: a loan is owed. Still overridable below, because
    // a negative balance can show up on any account.
    a.sign = signOf(v);
    if (!initial) a.review_days = REVIEW_DEFAULTS[v] ?? 0;
  }

  function submit() {
    if (!addressOk) return;
    const loanOn = isLoan && a.loanOn;
    onsubmit({
      template_id: a.template_id || null,
      name: a.name,
      asset_type: a.asset_type,
      currency: a.currency,
      category: a.category || null,
      sign: linked || isWallet ? 1 : loanOn ? -1 : Number(a.sign) || 1,
      portfolio_id: a.portfolio_id ?? null,
      review_days: linked || isWallet || loanOn ? 0 : Number(a.review_days) || 0,
      loan: loanOn
        ? {
            principal: num(a.loan.principal),
            rate_pct: num(a.loan.rate_pct) ?? 0,
            term_months: num(a.loan.term_months),
            start: a.loan.start,
            payment: num(a.loan.payment)
          }
        : null,
      paired_liability_id: Number(a.sign) === 1 && !isLoan ? a.paired_liability_id || null : null,
      drift_pct_year: canDrift ? num(a.drift_pct_year) : null,
      broker_account_id: a.broker_account_id ?? null,
      chain: isWallet ? a.chain : null,
      address: isWallet ? a.address.trim() : null,
      owner: a.owner.trim() || null
    });
  }
</script>

<form class="asset-form" onsubmit={(e) => { e.preventDefault(); submit(); }}>
  <datalist id="asset-categories">
    {#each categories as c}<option value={c}></option>{/each}
  </datalist>
  <datalist id="asset-owners">
    {#each owners as o}<option value={o}></option>{/each}
  </datalist>

  <div class="field">
    <span>{$t('wealth.assetForm.type')}</span>
    <!-- The kind as segmented chips: it decides which fold opens below. A linked asset's
         kind follows its link. -->
    <div class="kinds" role="radiogroup" aria-label={$t('wealth.assetForm.type')}>
      {#each typeOptions as at (at.id)}
        <button
          type="button"
          role="radio"
          class="chip"
          class:active={a.asset_type === at.id}
          aria-checked={a.asset_type === at.id}
          disabled={linked}
          onclick={() => onType(at.id)}
        >{at.label}</button>
      {/each}
    </div>
  </div>

  <div class="grid">
    <label class="field span2">
      <span>{$t('wealth.assetForm.name')}</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={a.name} autofocus placeholder={$t('wealth.assetForm.namePlaceholder')} />
    </label>
    <label class="field">
      <span>{$t('wealth.assetForm.category')}</span>
      <input bind:value={a.category} list="asset-categories" autocomplete="off" placeholder={$t('wealth.assetForm.optionalPlaceholder')} />
    </label>
    <label class="field">
      <span>{$t('wealth.assetForm.owner')}</span>
      <input bind:value={a.owner} list="asset-owners" autocomplete="off" placeholder={$t('wealth.assetForm.optionalPlaceholder')} />
    </label>
    <div class="field">
      <span>{$t('wealth.assetForm.currency')}</span>
      <Dropdown
        bind:value={a.currency}
        ariaLabel={$t('wealth.assetForm.currency')}
        options={CURRENCIES.map((c) => ({ value: c, label: c }))}
      />
    </div>
    {#if templates.length > 0}
      <div class="field">
        <span>{$t('wealth.assetForm.template')}</span>
        <Dropdown
          value={a.template_id ?? ''}
          onpick={onTemplate}
          ariaLabel={$t('wealth.assetForm.template')}
          options={[
            { value: '', label: $t('wealth.assetForm.noneOption') },
            ...templates.map((tpl) => ({ value: tpl.id, label: tpl.name }))
          ]}
        />
      </div>
    {/if}
    {#if !linked && !isWallet && !(isLoan && a.loanOn)}
      <div class="field">
        <span>{$t('wealth.assetForm.side')}</span>
        <!-- Owned or owed. A net worth that cannot subtract a mortgage is not a net worth,
             and this is the one field that makes the headline true. -->
        <Dropdown
          value={String(a.sign)}
          onpick={(v) => (a.sign = Number(v))}
          ariaLabel={$t('wealth.assetForm.side')}
          options={[
            { value: '1', label: $t('wealth.assetForm.owned') },
            { value: '-1', label: $t('wealth.assetForm.owed') }
          ]}
        />
      </div>
      <label class="field">
        <span>{$t('wealth.assetForm.review')}</span>
        <!-- A house valued three years ago is wrong in silence. This is how long before the
             page says so. 0 never nags. -->
        <input type="number" min="0" step="1" bind:value={a.review_days} />
      </label>
    {/if}
    {#if canDrift}
      <label class="field">
        <span>{$t('wealth.drift.label')}</span>
        <!-- An assumed yearly change (+3 for a flat, -15 for a car), drawn as an estimate
             after the last real valuation and never stored as one. -->
        <input type="number" step="any" bind:value={a.drift_pct_year} placeholder={$t('wealth.drift.placeholder')} />
      </label>
    {/if}
    {#if Number(a.sign) === 1 && !isLoan && !linked && liabilities.length}
      <div class="field">
        <span>{$t('wealth.pair.label')}</span>
        <Dropdown
          value={a.paired_liability_id ?? ''}
          onpick={(v) => (a.paired_liability_id = v || null)}
          ariaLabel={$t('wealth.pair.label')}
          options={[{ value: '', label: $t('wealth.assetForm.noneOption') }, ...liabilities.map((l) => ({ value: l.id, label: l.name }))]}
        />
      </div>
    {/if}
  </div>

  {#if isWallet}
    <!-- Read on chain: the address is checked for its format only, the balance comes from
         the chain itself. -->
    <fieldset class="fold">
      <legend>{$t('wealth.assetForm.walletFold')}</legend>
      <div class="grid">
        <div class="field">
          <span>{$t('wealth.wallet.chain')}</span>
          <Dropdown bind:value={a.chain} ariaLabel={$t('wealth.wallet.chain')} options={CHAINS.map((c) => ({ value: c.id, label: c.label }))} />
        </div>
        <label class="field span3">
          <span>{$t('wealth.wallet.address')}</span>
          <input class="mono" bind:value={a.address} autocomplete="off" spellcheck="false" aria-invalid={!addressOk} />
        </label>
      </div>
      {#if a.address && !addressOk}
        <p class="bad">{a.chain === 'eth' ? $t('wealth.wallet.badEth') : $t('wealth.wallet.badBtc')}</p>
      {:else}
        <p class="linked">{$t('wealth.wallet.note')}</p>
      {/if}
    </fieldset>
  {/if}

  {#if isLoan}
    <fieldset class="fold">
      <legend>{$t('wealth.assetForm.loanFold')}</legend>
      <label class="check">
        <input type="checkbox" bind:checked={a.loanOn} />
        <span>{$t('wealth.loan.useSchedule')}</span>
      </label>
      {#if a.loanOn}
        <div class="grid">
          <label class="field"><span>{$t('wealth.loan.principal')}</span><input type="number" min="0" step="any" bind:value={a.loan.principal} required /></label>
          <label class="field"><span>{$t('wealth.loan.rate')}</span><input type="number" min="0" step="any" bind:value={a.loan.rate_pct} required /></label>
          <label class="field"><span>{$t('wealth.loan.term')}</span><input type="number" min="1" step="1" bind:value={a.loan.term_months} required /></label>
          <label class="field"><span>{$t('wealth.loan.start')}</span><input type="date" bind:value={a.loan.start} required /></label>
          <label class="field span2">
            <span>{$t('wealth.loan.payment')}</span>
            <input type="number" min="0" step="any" bind:value={a.loan.payment} placeholder={preview != null ? fmtMoney(preview, a.currency) : ''} />
          </label>
        </div>
        <p class="linked">{$t('wealth.loan.note')}</p>
      {/if}
    </fieldset>
  {/if}

  {#if linked}
    <p class="linked">{$t('wealth.assetForm.linkedNote')}</p>
  {/if}

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary" disabled={!addressOk}>{initial ? $t('common.save') : $t('wealth.assetForm.addAsset')}</button>
  </div>
</form>

<style>
  .asset-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  /* Two columns of label-above fields; a wide field spans both. */
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
  }
  .span2 {
    grid-column: span 2;
  }
  .span3 {
    grid-column: 2 / -1;
  }
  .kinds {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }
  .kinds .chip:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  /* A fold: a hairline above, a small title, the fields of one kind. No box of its own. */
  .fold {
    margin: 0;
    padding: var(--space-3) 0 0;
    border: none;
    border-top: var(--hairline) solid var(--border);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }
  .fold legend {
    padding: 0 var(--space-2) 0 0;
    color: var(--dim);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .mono {
    font-family: var(--mono);
  }
  .linked {
    margin: 0;
    font-size: 12px;
    color: var(--muted);
    line-height: var(--lh-base);
  }
  .bad {
    margin: 0;
    font-size: 12px;
    color: var(--red-ink);
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
  }
  /* The actions stay in reach while the body scrolls: a long loan fold never hides Save. */
  .actions {
    position: sticky;
    bottom: calc(-1 * var(--space-4));
    z-index: 1;
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-bottom: calc(-1 * var(--space-4));
    padding: var(--space-3) 0 var(--space-4);
    border-top: var(--hairline) solid var(--border);
    background: var(--surface-raised);
  }
  /* Phone: one field per line, the actions take a finger. */
  @media (max-width: 767px) {
    .grid { grid-template-columns: minmax(0, 1fr); }
    .span2, .span3 { grid-column: auto; }
    .kinds .chip { min-height: 44px; }
    .actions { flex-wrap: wrap; }
    .actions button { flex: 1 1 auto; min-height: 44px; }
  }
</style>
