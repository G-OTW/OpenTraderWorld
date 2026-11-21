<script>
  // Fill the tax form from a broker account: the realized gains of one tax year, per line.
  //
  // Read only, and stored nowhere. The server pulls a window of fills, matches every sale
  // to its purchases under the profile's cost method, each leg at its own day's rate, and
  // adds up what each disposal realized; this applies the three numbers to the form, which
  // the user saves as usual. Under a French profile, spot crypto follows the global
  // portfolio formula instead, and each sale to fiat needs the value of everything held
  // just before it: the user types it here and reads again.
  //
  // The window is the subtle part, and it is the user's to set. A sale made in March was
  // bought at some earlier date, and without that purchase there is no cost basis: the
  // pull therefore starts where the field says, defaulting to 1 January of the year, and a
  // sale whose purchase is outside it comes back as an error naming the fix rather than a
  // gain measured against nothing.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SymbolHelp from '$lib/ui/SymbolHelp.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import BrokerButton from '$lib/brokers/BrokerButton.svelte';
  import LongRunModal from '$lib/tasks/LongRunModal.svelte';
  import RunningBanner from '$lib/tasks/RunningBanner.svelte';
  import { LongRun } from '$lib/tasks/longrun.svelte.js';
  import { brokersApi, isReady } from '$lib/brokers/api.js';
  import { taxcalcApi, fmtMoney } from './api.js';
  import { t } from '$lib/i18n';

  const MODULE = 'taxcalc';

  let {
    open = $bindable(false),
    profileId = '',
    profileCurrency = '',
    onapply = () => {}
  } = $props();

  let accounts = $state([]);
  let accountId = $state('');
  let taxYear = $state(new Date().getFullYear());
  let from = $state('');
  let symbols = $state([]);
  let symbolDraft = $state('');
  let suggestions = $state([]);
  let loadingSymbols = $state(false);
  let busy = $state(false);
  let error = $state('');
  let preview = $state(null);
  let atPar = $state(false);
  // French crypto: global value before each sale, keyed by execution id.
  let cryptoValues = $state({});
  let cryptoOpening = $state('');

  const account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  const cap = $derived(account?.capability ?? null);
  const accountOpts = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: isReady(a) ? a.name : `${a.name} · ${$t('brokers.incomplete')}`
    }))
  );
  const ready = $derived(!!accountId && (!cap?.needs_symbols || symbols.length > 0));

  // A long read is offered the background; reopening takes its answer back.
  const guard = new LongRun({
    scope: () => 'taxcalc',
    onresult: (task) => {
      const r = task.result ?? {};
      if (accounts.some((a) => a.id === r.account_id)) accountId = r.account_id;
      if (r.preview) {
        taxYear = r.preview.tax_year;
        from = day(r.preview.from);
        preview = r.preview;
      }
    },
    onerror: (msg) => (error = msg),
    onsent: () => (open = false)
  });

  $effect(() => {
    if (!open) return;
    boot().then(() => guard.attach());
    return () => guard.detach();
  });

  async function boot() {
    error = '';
    try {
      accounts = await brokersApi.list(MODULE);
      if (!accounts.some((a) => a.id === accountId)) accountId = accounts[0]?.id ?? '';
      if (!from) from = `${taxYear}-01-01`;
    } catch (e) {
      error = e.message;
    }
  }

  function onAccountChange(id) {
    accountId = id;
    preview = null;
    suggestions = [];
  }

  function onYearChange() {
    preview = null;
    from = `${taxYear}-01-01`;
  }

  async function loadSuggestions() {
    if (!accountId) return;
    loadingSymbols = true;
    error = '';
    try {
      suggestions = await brokersApi.symbols(accountId);
    } catch (e) {
      error = e.message;
    } finally {
      loadingSymbols = false;
    }
  }

  function addSymbol(raw) {
    const s = (raw ?? symbolDraft).trim().toUpperCase();
    if (!s || symbols.includes(s)) {
      symbolDraft = '';
      return;
    }
    symbols = [...symbols, s];
    symbolDraft = '';
  }
  const dropSymbol = (s) => (symbols = symbols.filter((x) => x !== s));

  function request() {
    return {
        account_id: accountId,
        tax_year: Number(taxYear),
        from: from || null,
        symbols,
        currency: profileCurrency || null,
        profile_id: profileId || null,
        stablecoins_at_par: atPar,
        crypto_values: Object.fromEntries(
          Object.entries(cryptoValues)
            .filter(([, v]) => v !== '' && v != null && Number(v) > 0)
            .map(([k, v]) => [k, Number(v)])
        ),
        crypto_opening_cost: cryptoOpening === '' ? null : Number(cryptoOpening)
    };
  }

  // The window the server reads: from the field (1 January by default) to the year's end.
  function read() {
    const start = from || `${taxYear}-01-01`;
    return guard.guard(
      accountId,
      {
        kind: 'executions',
        from: `${start}T00:00:00Z`,
        to: `${taxYear}-12-31T23:59:59Z`,
        symbols
      },
      readNow,
      () => taxcalcApi.brokerPreviewBg(request())
    );
  }

  async function readNow() {
    busy = true;
    error = '';
    preview = null;
    try {
      preview = await taxcalcApi.brokerPreview(request());
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function apply() {
    if (!preview) return;
    onapply({
      realized_capital_gains: preview.capital_gains,
      derivative_gains: preview.derivative_gains,
      crypto_gains: preview.crypto_gains,
      long_term: preview.long_term_unknown ? null : preview.long_term,
      proceeds: preview.proceeds,
      currency: preview.currency
    });
    open = false;
  }

  const day = (iso) => (iso ? String(iso).slice(0, 10) : '');
  const cryptoIncomplete = $derived(
    !!preview?.crypto_fr && !preview.crypto_fr.exempt && preview.crypto_fr.year_gain == null
  );
  const ruleLabel = (l) =>
    l.holding_days != null
      ? $t('taxcalc.broker.heldDays', { days: l.holding_days })
      : $t(`taxcalc.broker.rule.${l.rule}`);
</script>

<Modal bind:open title={$t('taxcalc.broker.title')} size="lg">
  <div class="wrap">
    {#if !accounts.length}
      <EmptyState
        icon="briefcase"
        title={$t('taxcalc.broker.noAccount')}
        description={$t('taxcalc.broker.noAccountHelp')}
      >
        {#snippet action()}
          <BrokerButton module={MODULE} onchanged={boot} />
        {/snippet}
      </EmptyState>
    {:else}
      <div class="row">
        <div class="grow fld">
          <span>{$t('taxcalc.broker.account')}</span>
          <Dropdown
            value={accountId}
            options={accountOpts}
            ariaLabel={$t('taxcalc.broker.account')}
            onpick={onAccountChange}
          />
        </div>
        <label class="fld">
          <span>{$t('taxcalc.broker.taxYear')}</span>
          <input type="number" bind:value={taxYear} onchange={onYearChange} />
        </label>
        <label class="fld">
          <span>{$t('taxcalc.broker.readFrom')}</span>
          <input type="date" bind:value={from} onchange={() => (preview = null)} />
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={atPar} onchange={() => (preview = null)} />
          <span>{$t('taxcalc.broker.stablecoinsAtPar')}</span>
        </label>
        <BrokerButton module={MODULE} onchanged={boot} />
      </div>

      <p class="note">
        <Icon name="info" size={12} />
        <span>{$t('taxcalc.broker.windowHint')}</span>
      </p>

      {#if cap?.needs_symbols || symbols.length}
        <div class="fld">
          <span>
            {$t('taxcalc.broker.symbols')}
            {#if cap?.needs_symbols}<span class="req">*</span>{/if}
            <!-- The broker answers its own spelling, so the shapes it takes sit here. -->
            <SymbolHelp provider={account?.broker ?? ''} />
          </span>
          <div class="tagbox">
            {#each symbols as s (s)}
              <span class="tag">
                {s}
                <button class="drop" onclick={() => dropSymbol(s)} aria-label={$t('common.remove')}>
                  <Icon name="x" size={10} />
                </button>
              </span>
            {/each}
            <input
              bind:value={symbolDraft}
              placeholder={$t('taxcalc.broker.symbolPlaceholder')}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ',') {
                  e.preventDefault();
                  addSymbol();
                }
              }}
              onblur={() => addSymbol()}
            />
          </div>
          <div class="suggest">
            <Button variant="ghost" onclick={loadSuggestions} disabled={loadingSymbols}>
              {loadingSymbols ? $t('common.loading') : $t('taxcalc.broker.suggest')}
            </Button>
            {#each suggestions.slice(0, 40) as s (s)}
              {#if !symbols.includes(s)}
                <button class="chip" onclick={() => addSymbol(s)}>{s}</button>
              {/if}
            {/each}
          </div>
        </div>
      {/if}

      <RunningBanner {guard} />

      <div class="acts">
        <Button onclick={read} disabled={!ready || busy || !!guard.running}>
          {busy ? $t('taxcalc.broker.reading') : $t('taxcalc.broker.read')}
        </Button>
      </div>

      <ErrorText {error} copyable />

      {#if preview}
        <div class="result">
          <p class="muted small">
            {$t('taxcalc.broker.summary', {
              closed: preview.closed,
              year: preview.tax_year,
              executions: preview.executions
            })}
            · {$t('taxcalc.broker.methodUsed', { method: $t(`taxcalc.profile.method.${preview.method}`) })}
          </p>
          <table class="tbl">
            <tbody>
              <tr>
                <td>{$t('taxcalc.loadJournal.capitalGains')}</td>
                <td class="num">{fmtMoney(preview.capital_gains, preview.currency)}</td>
              </tr>
              <tr>
                <td>{$t('taxcalc.loadJournal.derivativeGains')}</td>
                <td class="num">{fmtMoney(preview.derivative_gains, preview.currency)}</td>
              </tr>
              <tr>
                <td>{$t('taxcalc.loadJournal.cryptoGains')}</td>
                <td class="num">{fmtMoney(preview.crypto_gains, preview.currency)}</td>
              </tr>
            </tbody>
          </table>

          {#if preview.still_open || preview.before_year}
            <p class="muted small">
              {$t('taxcalc.broker.leftOut', {
                open: preview.still_open,
                before: preview.before_year
              })}
            </p>
          {/if}

          {#if preview.long_term_unknown}
            <p class="warn small">
              <Icon name="alert-triangle" size={11} />
              {$t('taxcalc.broker.longUnknown')}
            </p>
          {/if}
          {#each preview.notes ?? [] as n}
            <p class="muted small">{n}</p>
          {/each}

          {#if preview.crypto_fr}
            <div class="crypto">
              <p class="small">
                <strong>{$t('taxcalc.broker.cryptoFrTitle')}</strong>
                <span class="muted">{$t('taxcalc.broker.cryptoFrHint')}</span>
              </p>
              {#if preview.crypto_swaps}
                <p class="muted small">{$t('taxcalc.broker.cryptoSwaps', { count: preview.crypto_swaps })}</p>
              {/if}
              <label class="fld">
                <span>{$t('taxcalc.broker.cryptoOpening', { currency: preview.currency })}</span>
                <input type="number" step="0.01" min="0" bind:value={cryptoOpening} placeholder="0" />
              </label>
              {#if preview.crypto_fr.cessions.length}
                <div class="table-wrap">
                  <table class="tbl lines">
                    <thead>
                      <tr>
                        <th>{$t('taxcalc.broker.instrument')}</th>
                        <th>{$t('taxcalc.broker.closedOn')}</th>
                        <th class="num">{$t('taxcalc.broker.cryptoProceeds')}</th>
                        <th class="num">{$t('taxcalc.broker.cryptoGlobalValue')}</th>
                        <th class="num">{$t('taxcalc.broker.realized')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each preview.crypto_fr.cessions as c (c.id)}
                        <tr class:faded={!c.in_year}>
                          <td>{c.symbol}</td>
                          <td class="muted">{day(c.at)}</td>
                          <td class="num">{fmtMoney(c.proceeds, preview.currency)}</td>
                          <td class="num">
                            <input
                              class="vin"
                              type="number"
                              step="0.01"
                              min="0"
                              value={cryptoValues[c.id] ?? c.global_value ?? ''}
                              oninput={(e) => (cryptoValues = { ...cryptoValues, [c.id]: e.target.value })}
                              aria-label={$t('taxcalc.broker.cryptoGlobalValue')}
                            />
                          </td>
                          <td class="num" class:up={c.gain > 0} class:down={c.gain < 0}>
                            {c.gain == null ? '·' : fmtMoney(c.gain, preview.currency)}
                          </td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
                <div class="acts">
                  <Button variant="ghost" onclick={read} disabled={busy || !!guard.running}>
                    {$t('taxcalc.broker.recompute')}
                  </Button>
                </div>
              {/if}
              {#if cryptoIncomplete}
                <p class="warn small">
                  <Icon name="alert-triangle" size={11} />
                  {$t('taxcalc.broker.cryptoIncomplete')}
                </p>
              {/if}
            </div>
          {/if}

          {#if preview.errors?.length}
            <div class="errs">
              <p class="warn small">
                <Icon name="alert-triangle" size={11} />
                {$t('taxcalc.broker.fillErrors', { count: preview.errors.length })}
              </p>
              {#each preview.errors.slice(0, 6) as e}
                <p class="errline">{e}</p>
              {/each}
            </div>
          {/if}

          {#if preview.lines?.length}
            <details>
              <summary>{$t('taxcalc.broker.showLines', { count: preview.lines.length })}</summary>
              <div class="table-wrap">
                <table class="tbl lines">
                  <thead>
                    <tr>
                      <th>{$t('taxcalc.broker.instrument')}</th>
                      <th>{$t('taxcalc.broker.closedOn')}</th>
                      <th>{$t('taxcalc.broker.matchedBy')}</th>
                      <th class="num">{$t('taxcalc.broker.cost')}</th>
                      <th class="num">{$t('taxcalc.broker.cryptoProceeds')}</th>
                      <th class="num">{$t('taxcalc.broker.realized')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each preview.lines as l, i (i)}
                      <tr>
                        <td>
                          {l.ticker}
                          <span class="muted">{l.asset_class}</span>
                        </td>
                        <td class="muted">{day(l.closed_at)}</td>
                        <td class="muted">{ruleLabel(l)}</td>
                        <td class="num">{fmtMoney(l.cost, preview.currency)}</td>
                        <td class="num">{fmtMoney(l.proceeds, preview.currency)}</td>
                        <td
                          class="num"
                          class:up={l.gain > 0}
                          class:down={l.gain < 0}
                          title={l.currency !== preview.currency
                            ? $t('taxcalc.broker.nativeGain', { amount: fmtMoney(l.native_gain, l.currency) })
                            : undefined}
                        >
                          {fmtMoney(l.gain, preview.currency)}
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            </details>
          {/if}

          <p class="muted small">{$t('taxcalc.broker.applyHint')}</p>
        </div>
      {/if}
    {/if}
  </div>

  {#snippet footer()}
    <button class="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</button>
    <button class="primary" onclick={apply} disabled={!preview}>
      {$t('taxcalc.broker.applyToForm')}
    </button>
  {/snippet}
</Modal>

<LongRunModal {guard} />

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
    min-width: 180px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .crypto {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: var(--hairline) solid var(--border);
  }
  .crypto .fld {
    max-width: 320px;
  }
  .vin {
    width: 130px;
    text-align: right;
  }
  tr.faded td {
    opacity: 0.6;
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .req {
    color: var(--red);
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
    line-height: 1.5;
  }
  /* One control, one frame: the field inside the tag box is borderless. */
  .tagbox {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1);
  }
  .tagbox input {
    flex: 1;
    min-width: 110px;
    border: none;
    background: transparent;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px var(--space-2);
    border-radius: var(--radius);
    background: var(--surface-2);
    font-family: var(--mono, ui-monospace, monospace);
    font-size: var(--text-xs);
  }
  .drop {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
  }
  .suggest {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: center;
    margin-top: var(--space-1);
  }
  .chip {
    padding: 1px var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
  }
  .acts {
    display: flex;
    gap: var(--space-2);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
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
  }
  .tbl th {
    text-align: left;
    font-size: var(--text-xs);
    color: var(--muted);
    font-weight: var(--fw-medium);
  }
  .num {
    text-align: right;
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .up {
    color: var(--green);
  }
  .down {
    color: var(--red);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: var(--text-xs);
  }
  .warn {
    display: flex;
    align-items: baseline;
    gap: var(--space-1);
    flex-wrap: wrap;
    color: var(--amber);
    margin: 0;
  }
  .errs {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .errline {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .table-wrap {
    max-height: 260px;
    overflow: auto;
  }
  summary {
    font-size: var(--text-xs);
    color: var(--muted);
    cursor: pointer;
  }
</style>
