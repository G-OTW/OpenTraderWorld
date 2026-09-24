<script>
  // Pull a period of executions from a broker account into this journal.
  //
  // Same two rails as the file import: nothing is written before the user has looked at
  // the trades a pull would file, and everything written carries a batch id, so a bad
  // pull is reverted whole. What differs is the identity: a position is recognised by the
  // execution that opened it, so pulling a wider window a second time refreshes the rows
  // already there instead of doubling them.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SymbolHelp from '$lib/ui/SymbolHelp.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import BrokerButton from '$lib/brokers/BrokerButton.svelte';
  import ImportTradeCard from './ImportTradeCard.svelte';
  import { brokersApi, isReady } from '$lib/brokers/api.js';
  import { journalApi, fmtSignedMoney } from './api.js';
  import { t, locale } from '$lib/i18n';

  let {
    open = $bindable(false),
    categoryId = '',
    categories = [],
    onchanged = () => {}
  } = $props();

  let accounts = $state([]);
  let accountId = $state('');
  let from = $state('');
  let to = $state('');
  let symbols = $state([]);
  let symbolDraft = $state('');
  let suggestions = $state([]);
  let allowShort = $state(false);
  let busy = $state(false);
  let loadingSymbols = $state(false);
  let error = $state('');
  let preview = $state(null);
  let report = $state(null);
  let selected = $state(0);

  const account = $derived(accounts.find((a) => a.id === accountId) ?? null);
  const cap = $derived(account?.capability ?? null);
  const accountOpts = $derived(
    accounts.map((a) => ({
      value: a.id,
      label: isReady(a) ? a.name : `${a.name} · ${$t('brokers.incomplete')}`
    }))
  );
  const targetCategory = $derived(
    categoryId || categories.find((c) => c.is_default)?.id || categories[0]?.id || ''
  );
  const categoryName = $derived(
    categories.find((c) => c.id === targetCategory)?.name ?? ''
  );
  const ready = $derived(
    !!accountId && !!from && !!to && (!cap?.needs_symbols || symbols.length > 0)
  );

  const iso = (d) => d.toISOString().slice(0, 10);

  function setRange(days) {
    const end = new Date();
    const start = new Date(end.getTime() - days * 86400000);
    from = iso(start);
    to = iso(end);
  }

  // Reopening lands on the account, the instruments and the window this journal was last
  // pulled with: a broker sync is a habit, not a one-off.
  $effect(() => {
    if (!open) return;
    error = '';
    preview = null;
    report = null;
    load();
  });

  async function load() {
    try {
      accounts = await brokersApi.list('journal');
      const last = targetCategory ? await journalApi.brokerSyncLast(targetCategory) : null;
      const remembered = accounts.find((a) => a.id === last?.account_id);
      accountId = remembered?.id ?? accounts[0]?.id ?? '';
      symbols = Array.isArray(last?.symbols) ? last.symbols : [];
      if (last?.period_from && last?.period_to) {
        from = last.period_from.slice(0, 10);
        to = last.period_to.slice(0, 10);
      } else {
        setRange(30);
      }
      allowShort = !(accounts.find((a) => a.id === accountId)?.capability?.spot_only ?? true);
    } catch (e) {
      error = e.message;
    }
  }

  // The picked id comes with the event: reading it back through the binding would depend
  // on when the derived refreshes.
  function onAccountChange(id) {
    preview = null;
    report = null;
    suggestions = [];
    const picked = accounts.find((a) => a.id === id);
    allowShort = !(picked?.capability?.spot_only ?? true);
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

  function body() {
    return {
      account_id: accountId,
      category_id: targetCategory || null,
      // A day is taken whole, in UTC: the broker stamps in its own zone and a half-open
      // local day would silently drop the fills on its edge.
      from: `${from}T00:00:00Z`,
      to: `${to}T23:59:59Z`,
      symbols,
      allow_short: allowShort
    };
  }

  async function runPreview() {
    busy = true;
    error = '';
    report = null;
    try {
      preview = await journalApi.brokerSyncPreview(body());
      selected = 0;
    } catch (e) {
      error = e.message;
      preview = null;
    } finally {
      busy = false;
    }
  }

  async function runImport() {
    busy = true;
    error = '';
    try {
      report = await journalApi.brokerSyncCommit(body());
      preview = null;
      onchanged();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  function fmtDay(isoStr) {
    if (!isoStr) return '—';
    return new Date(isoStr).toLocaleDateString($locale, {
      day: '2-digit',
      month: 'short',
      year: 'numeric'
    });
  }
</script>

<Modal bind:open title={$t('journal.brokerSync.title')} size="lg">
  <div class="wrap">
    <p class="hint">{$t('journal.brokerSync.hint')}</p>

    {#if accounts.length === 0}
      <EmptyState
        icon="briefcase"
        description={$t('journal.brokerSync.noAccounts')}
        compact
      />
      <div class="manage"><BrokerButton module="journal" onchanged={load} /></div>
    {:else}
      <div class="form">
        <div class="fld">
          <span class="lbl">{$t('journal.brokerSync.account')}</span>
          <Dropdown
            bind:value={accountId}
            options={accountOpts}
            ariaLabel={$t('journal.brokerSync.account')}
            onpick={onAccountChange}
          />
        </div>
        <div class="fld">
          <span class="lbl">{$t('journal.brokerSync.into')}</span>
          <span class="static">{categoryName || $t('journal.brokerSync.defaultBook')}</span>
        </div>
      </div>

      {#if cap}
        <p class="note">
          <Icon name="eye" size={12} />
          <span>{cap.key_note}</span>
        </p>
        {#if cap.window_from_broker}
          <p class="note warn">
            <Icon name="alert-triangle" size={12} />
            <span>{$t('journal.brokerSync.windowFromBroker')}</span>
          </p>
        {/if}
      {/if}

      <div class="form">
        <label class="fld">
          <span class="lbl">{$t('journal.brokerSync.from')}</span>
          <input type="date" bind:value={from} />
        </label>
        <label class="fld">
          <span class="lbl">{$t('journal.brokerSync.to')}</span>
          <input type="date" bind:value={to} />
        </label>
      </div>
      <div class="ranges">
        <button class="chip" onclick={() => setRange(7)}>{$t('journal.brokerSync.last7')}</button>
        <button class="chip" onclick={() => setRange(30)}>{$t('journal.brokerSync.last30')}</button>
        <button class="chip" onclick={() => setRange(90)}>{$t('journal.brokerSync.last90')}</button>
        <button class="chip" onclick={() => setRange(365)}>{$t('journal.brokerSync.last365')}</button>
      </div>

      {#if cap?.needs_symbols || symbols.length}
        <div class="fld">
          <span class="lbl">
            {$t('journal.brokerSync.symbols')}
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
              placeholder={$t('journal.brokerSync.symbolPlaceholder')}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ',') {
                  e.preventDefault();
                  addSymbol();
                }
              }}
              onblur={() => addSymbol()}
            />
          </div>
          <span class="help">
            {cap?.needs_symbols
              ? $t('journal.brokerSync.symbolsRequired')
              : $t('journal.brokerSync.symbolsFilter')}
          </span>
          <div class="suggest">
            <Button variant="ghost" onclick={loadSuggestions} disabled={loadingSymbols}>
              {loadingSymbols
                ? $t('journal.brokerSync.loadingSymbols')
                : $t('journal.brokerSync.suggest')}
            </Button>
            {#each suggestions.slice(0, 40) as s (s)}
              {#if !symbols.includes(s)}
                <button class="chip" onclick={() => addSymbol(s)}>{s}</button>
              {/if}
            {/each}
          </div>
        </div>
      {/if}

      {#if cap?.spot_only}
        <label class="chk">
          <input type="checkbox" bind:checked={allowShort} />
          {$t('journal.brokerSync.allowShort')}
        </label>
        <span class="help">{$t('journal.brokerSync.allowShortHelp')}</span>
      {/if}

      <div class="actions">
        <Button onclick={runPreview} disabled={!ready || busy}>
          {busy && !report ? $t('journal.brokerSync.pulling') : $t('journal.brokerSync.preview')}
        </Button>
        {#if preview && preview.stats.trades > 0}
          <Button variant="primary" onclick={runImport} disabled={busy}>
            {$t('journal.brokerSync.import')}
          </Button>
        {/if}
      </div>

      <ErrorText {error} copyable />

      {#if report}
        <div class="report">
          <p class="big">
            <Icon name="check-circle" size={16} />
            {$t('journal.brokerSync.done', {
              imported: report.imported,
              executions: report.executions
            })}
          </p>
          <p class="sub">
            {$t('journal.brokerSync.doneDetail', {
              updated: report.updated,
              duplicates: report.duplicates,
              failed: report.failed
            })}
          </p>
        </div>
      {/if}

      {#if preview}
        <div class="stats">
          <span><b>{preview.executions}</b> {$t('journal.brokerSync.stat.fills')}</span>
          <span><b>{preview.stats.trades}</b> {$t('journal.brokerSync.stat.trades')}</span>
          <span><b>{preview.stats.closed}</b> {$t('journal.brokerSync.stat.closed')}</span>
          <span><b>{preview.stats.open}</b> {$t('journal.brokerSync.stat.open')}</span>
          {#if preview.stats.duplicates}
            <span class="warned">
              <b>{preview.stats.duplicates}</b>
              {$t('journal.brokerSync.stat.duplicates')}
            </span>
          {/if}
          {#if preview.stats.errors}
            <span class="bad">
              <b>{preview.stats.errors}</b>
              {$t('journal.brokerSync.stat.errors')}
            </span>
          {/if}
        </div>

        {#if preview.errors.length}
          <ul class="errs">
            {#each preview.errors.slice(0, 20) as e (e.row + e.message)}
              <li><Icon name="alert-triangle" size={11} /> {e.message}</li>
            {/each}
          </ul>
        {/if}

        {#if preview.preview.length}
          <ImportTradeCard item={preview.preview[selected]} />
          <div class="strip">
            {#each preview.preview as p, i (p.index)}
              <button class="row" class:active={i === selected} onclick={() => (selected = i)}>
                <span class="r-ticker">{p.trade.ticker || '—'}</span>
                <span class="r-side {p.trade.side}">{$t(`journal.side.${p.trade.side}`)}</span>
                <span class="r-date">{fmtDay(p.trade.exit_at ?? p.trade.entry_at)}</span>
                <span
                  class="r-pnl mono {p.computed.net_pnl == null
                    ? ''
                    : p.computed.net_pnl >= 0
                      ? 'pos'
                      : 'neg'}"
                >
                  {p.computed.net_pnl == null
                    ? $t('journal.trades.open')
                    : fmtSignedMoney(p.computed.net_pnl, p.trade.currency)}
                </span>
                {#if p.duplicate}<span class="r-dup">{$t('journal.brokerSync.alreadyIn')}</span>{/if}
              </button>
            {/each}
          </div>
        {:else}
          <EmptyState icon="inbox" description={$t('journal.brokerSync.nothing')} compact />
        {/if}
      {/if}
    {/if}
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
  }
  .form {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
    min-width: 0;
  }
  .lbl {
    display: block;
  }
  .req {
    color: var(--red);
    margin-left: 2px;
  }
  .static {
    color: var(--text);
    padding: var(--space-2) 0;
  }
  .help {
    font-size: 11px;
    color: var(--muted);
    line-height: 1.4;
  }
  .note {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 78ch;
  }
  .note.warn {
    color: var(--amber);
  }
  .note :global(svg) {
    flex: none;
    transform: translateY(1px);
  }
  .ranges,
  .suggest {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
  }
  .chip {
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .chip:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  /* One control, one frame: the box owns the hairline and the input inside it has none. */
  .tagbox {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    background: var(--surface);
  }
  .tagbox:focus-within {
    border-color: var(--accent);
  }
  .tagbox input {
    flex: 1;
    min-width: 120px;
    border: none;
    background: transparent;
    color: var(--text);
    padding: var(--space-1);
  }
  .tagbox input:focus {
    outline: none;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    color: var(--text);
  }
  .drop {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0;
    display: inline-flex;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text);
    cursor: pointer;
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .manage {
    display: flex;
    justify-content: center;
  }
  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .stats b {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .warned b {
    color: var(--amber);
  }
  .bad b {
    color: var(--red);
  }
  .errs {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-height: 140px;
    overflow-y: auto;
    font-size: var(--text-xs);
    color: var(--amber);
  }
  .report {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .big {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--green);
    font-size: var(--text-md);
  }
  .sub {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .strip {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 220px;
    overflow-y: auto;
  }
  .strip .row {
    display: grid;
    grid-template-columns: 1fr 60px 110px 1fr auto;
    gap: var(--space-2);
    align-items: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    color: var(--text);
    font-size: var(--text-xs);
    cursor: pointer;
    text-align: left;
  }
  .strip .row:hover {
    background: var(--surface-2);
  }
  .strip .row.active {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .r-ticker {
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .r-side.long {
    color: var(--green);
  }
  .r-side.short {
    color: var(--red);
  }
  .r-date,
  .r-dup {
    color: var(--muted);
  }
  .r-pnl {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .pos {
    color: var(--green);
  }
  .neg {
    color: var(--red);
  }

  @media (max-width: 640px) {
    .form {
      grid-template-columns: 1fr;
    }
  }
</style>
