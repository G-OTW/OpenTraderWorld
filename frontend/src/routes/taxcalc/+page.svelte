<script>
  import { copyLog } from '$lib/ui/copyLog.js';
  // TaxCalculator — estimate trading/investing tax only (no personal tax).
  // Left: tax profiles (country + person type + rules), reusable across years.
  // Right: a scenario form. Summary mode = "I invested X, my stack is now Y"; itemized mode =
  // per-income-type. Toggle trading|investing context. "Calculate" runs the engine WITHOUT
  // persisting (ephemeral); "Save to history" explicitly stores the scenario for later reopen.
  import { taxcalcApi, fmtMoney } from '$lib/modules/taxcalc/api.js';
  import ProfileEditor from '$lib/modules/taxcalc/ProfileEditor.svelte';
  import ResultBreakdown from '$lib/modules/taxcalc/ResultBreakdown.svelte';
  import LoadJournalModal from '$lib/modules/taxcalc/LoadJournalModal.svelte';
  import { installedIds } from '$lib/modules/installed.js';
  import { t } from '$lib/i18n';

  // Loading figures from the Trading Journal only works when that module is installed.
  // The rest of the calculator is standalone, so we gate just this one action.
  const journalInstalled = $derived(!$installedIds || $installedIds.has('journal'));

  let templates = $state([]);
  let profiles = $state([]);
  let error = $state('');

  let editorOpen = $state(false);
  let editId = $state(null);
  let journalOpen = $state(false);
  let journalNote = $state('');

  // Scenario form state.
  let profileId = $state('');
  let name = $state('My 2026 estimate');
  let taxYear = $state(new Date().getFullYear());
  let mode = $state('summary');
  let context = $state('investing');
  let result = $state(null);
  let computing = $state(false);
  let saving = $state(false);
  let saveNote = $state('');

  // Summary inputs.
  let startValue = $state(0);
  let endValue = $state(0);
  let contributions = $state(0);
  let withdrawals = $state(0);
  let realizedPct = $state(100); // investing: % of the gain treated as realized/taxable now.

  // Itemized inputs.
  let realizedCapitalGains = $state(0);
  let dividends = $state(0);
  let interestIncome = $state(0);
  let derivativeGains = $state(0);
  let cryptoGains = $state(0);
  let priorLossesCarried = $state(0);

  // Shared advanced inputs (reach engine paths that were previously unreachable from the UI).
  let holdingDays = $state(''); // blank = unspecified (no holding relief tier applied).
  let wealthTaxValue = $state(0); // portfolio snapshot for wealth-tax brackets, if the profile has any.

  // Saved scenarios (history) — populated only when the user clicks Save.
  let scenarios = $state([]);

  let selectedProfile = $derived(profiles.find((p) => p.id === profileId));
  let profileHasWealthTax = $derived(
    Array.isArray(selectedProfile?.wealth_tax) && selectedProfile.wealth_tax.length > 0
  );

  function loadProfiles() {
    taxcalcApi
      .profiles()
      .then((ps) => {
        profiles = ps;
        if (!profileId && ps.length) profileId = ps[0].id;
      })
      .catch((e) => (error = e.message));
  }

  function loadScenarios() {
    taxcalcApi
      .scenarios()
      .then((ss) => (scenarios = ss))
      .catch((e) => (error = e.message));
  }

  $effect(() => {
    taxcalcApi.templates().then((ts) => (templates = ts)).catch((e) => (error = e.message));
    loadProfiles();
    loadScenarios();
  });

  function inputs() {
    const shared = {};
    const hd = String(holdingDays).trim();
    if (hd !== '') shared.holding_days = Number(hd);
    if (profileHasWealthTax) shared.portfolio_value_for_wealth_tax = Number(wealthTaxValue);

    if (mode === 'summary') {
      return {
        ...shared,
        start_value: Number(startValue),
        end_value: Number(endValue),
        contributions: Number(contributions),
        withdrawals: Number(withdrawals),
        realized_pct: Number(realizedPct)
      };
    }
    return {
      ...shared,
      realized_capital_gains: Number(realizedCapitalGains),
      dividends: Number(dividends),
      interest_income: Number(interestIncome),
      derivative_gains: Number(derivativeGains),
      crypto_gains: Number(cryptoGains),
      prior_losses_carried: Number(priorLossesCarried)
    };
  }

  /** Current form as a scenario payload (shared by compute + save). */
  function scenarioPayload() {
    return {
      profile_id: profileId,
      name,
      tax_year: Number(taxYear),
      mode,
      context,
      currency: selectedProfile?.currency || 'USD',
      inputs: inputs()
    };
  }

  // Ephemeral: runs the engine, persists nothing.
  async function compute() {
    if (!profileId) {
      error = $t('taxcalc.page.pickProfileFirst');
      return;
    }
    computing = true;
    error = '';
    saveNote = '';
    result = null;
    try {
      result = await taxcalcApi.computePreview(scenarioPayload());
    } catch (e) {
      error = e.message;
    } finally {
      computing = false;
    }
  }

  // Explicit persist: only what the user chooses to keep goes to history.
  async function saveToHistory() {
    if (!profileId) {
      error = $t('taxcalc.page.pickProfileFirst');
      return;
    }
    saving = true;
    error = '';
    try {
      await taxcalcApi.createScenario(scenarioPayload());
      saveNote = $t('taxcalc.page.savedToHistory', { name });
      loadScenarios();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  // Reopen a saved scenario back into the form (and show its cached result if any).
  async function openScenario(id) {
    error = '';
    saveNote = '';
    try {
      const s = await taxcalcApi.scenario(id);
      profileId = s.profile_id;
      name = s.name;
      taxYear = s.tax_year;
      mode = s.mode;
      context = s.context;
      const i = s.inputs || {};
      startValue = i.start_value ?? 0;
      endValue = i.end_value ?? 0;
      contributions = i.contributions ?? 0;
      withdrawals = i.withdrawals ?? 0;
      realizedPct = i.realized_pct ?? 100;
      realizedCapitalGains = i.realized_capital_gains ?? 0;
      dividends = i.dividends ?? 0;
      interestIncome = i.interest_income ?? 0;
      derivativeGains = i.derivative_gains ?? 0;
      cryptoGains = i.crypto_gains ?? 0;
      priorLossesCarried = i.prior_losses_carried ?? 0;
      holdingDays = i.holding_days ?? '';
      wealthTaxValue = i.portfolio_value_for_wealth_tax ?? 0;
      result = s.result ?? null;
    } catch (e) {
      error = e.message;
    }
  }

  async function removeScenario(id) {
    if (!confirm($t('taxcalc.page.confirmDeleteScenario'))) return;
    try {
      await taxcalcApi.deleteScenario(id);
      loadScenarios();
    } catch (e) {
      error = e.message;
    }
  }

  function newProfile() {
    editId = null;
    editorOpen = true;
  }
  function editProfile(id) {
    editId = id;
    editorOpen = true;
  }
  async function removeProfile(id) {
    if (!confirm($t('taxcalc.page.confirmDeleteProfile'))) return;
    await taxcalcApi.deleteProfile(id);
    if (profileId === id) profileId = '';
    loadProfiles();
  }

  // Fill the itemized inputs from a journal breakdown (realized PnL split by asset class).
  function applyJournal(v) {
    mode = 'itemized';
    context = 'trading';
    realizedCapitalGains = v.realized_capital_gains;
    derivativeGains = v.derivative_gains;
    cryptoGains = v.crypto_gains;
    const profCcy = selectedProfile?.currency;
    journalNote =
      profCcy && v.currency && profCcy !== v.currency
        ? $t('taxcalc.page.journalNoteMismatch', { currency: v.currency, profileCurrency: profCcy })
        : $t('taxcalc.page.journalNoteLoaded', { currency: v.currency });
  }
</script>

<div class="page">
  <header>
    <h1>{$t('taxcalc.page.title')}</h1>
    <span class="muted">{$t('taxcalc.page.subtitle')}</span>
  </header>

  {#if error}<p class="err" title={$t('taxcalc.profile.clickToCopy')} use:copyLog={error}>{error}</p>{/if}

  <div class="cols">
    <aside class="profiles">
      <div class="aside-head">
        <h2>{$t('taxcalc.page.profiles')}</h2>
        <button class="primary sm" onclick={newProfile}>{$t('taxcalc.page.newProfile')}</button>
      </div>
      <div class="profile-list">
        {#each profiles as p (p.id)}
          <div class="profile {p.id === profileId ? 'active' : ''}">
            <button class="pick" onclick={() => (profileId = p.id)}>
              <strong>{p.name}</strong>
              <span class="muted">{p.country} · {p.person_type} · {p.currency}</span>
            </button>
            <div class="actions">
              <button class="link" onclick={() => editProfile(p.id)}>{$t('taxcalc.page.edit')}</button>
              <button class="link red" onclick={() => removeProfile(p.id)}>{$t('taxcalc.page.del')}</button>
            </div>
          </div>
        {/each}
        {#if profiles.length === 0}
          <p class="muted small">{$t('taxcalc.page.noProfiles')}</p>
        {/if}
      </div>
    </aside>

    <section class="calc">
      <div class="row">
        <label class="grow">
          {$t('taxcalc.page.profile')}
          <select bind:value={profileId}>
            <option value="" disabled>{$t('taxcalc.page.selectProfile')}</option>
            {#each profiles as p (p.id)}
              <option value={p.id}>{p.name}</option>
            {/each}
          </select>
        </label>
        <label>
          {$t('taxcalc.loadJournal.taxYear')}
          <input type="number" bind:value={taxYear} />
        </label>
      </div>

      <div class="row">
        <label class="grow">
          {$t('taxcalc.page.scenarioName')}
          <input bind:value={name} />
        </label>
        <div class="toggles">
          <div class="toggle">
            <button class:on={context === 'investing'} onclick={() => (context = 'investing')}>{$t('taxcalc.page.investing')}</button>
            <button class:on={context === 'trading'} onclick={() => (context = 'trading')}>{$t('taxcalc.page.trading')}</button>
          </div>
          <div class="toggle">
            <button class:on={mode === 'summary'} onclick={() => (mode = 'summary')}>{$t('taxcalc.page.summary')}</button>
            <button class:on={mode === 'itemized'} onclick={() => (mode = 'itemized')}>{$t('taxcalc.page.itemized')}</button>
          </div>
          <button
            class="ghost"
            disabled={!journalInstalled}
            title={journalInstalled ? undefined : $t('taxcalc.page.journalRequired')}
            onclick={() => (journalOpen = true)}
          >📈 {$t('taxcalc.page.loadTradingJournal')}</button>
        </div>
      </div>

      {#if mode === 'summary'}
        <div class="grid">
          <label>
            {$t('taxcalc.page.investedAtStart')}
            <input type="number" step="0.01" bind:value={startValue} />
          </label>
          <label>
            {$t('taxcalc.page.stackNow')}
            <input type="number" step="0.01" bind:value={endValue} />
          </label>
          <label>
            {$t('taxcalc.page.contributions')}
            <input type="number" step="0.01" bind:value={contributions} />
          </label>
          <label>
            {$t('taxcalc.page.withdrawals')}
            <input type="number" step="0.01" bind:value={withdrawals} />
          </label>
          {#if context === 'investing'}
            <label>
              {$t('taxcalc.page.realizedShare')}
              <input type="number" step="1" min="0" max="100" bind:value={realizedPct} />
            </label>
          {/if}
        </div>
        <p class="muted small">{$t('taxcalc.page.gainFormula')}</p>
      {:else}
        {#if journalNote}<p class="note small" use:copyLog={journalNote}>{journalNote}</p>{/if}
        <div class="grid">
          <label>
            {$t('taxcalc.loadJournal.capitalGains')}
            <input type="number" step="0.01" bind:value={realizedCapitalGains} />
          </label>
          <label>
            {$t('taxcalc.page.dividends')}
            <input type="number" step="0.01" bind:value={dividends} />
          </label>
          <label>
            {$t('taxcalc.page.interestIncome')}
            <input type="number" step="0.01" bind:value={interestIncome} />
          </label>
          <label>
            {$t('taxcalc.page.derivativeGainsLabel')}
            <input type="number" step="0.01" bind:value={derivativeGains} />
          </label>
          <label>
            {$t('taxcalc.loadJournal.cryptoGains')}
            <input type="number" step="0.01" bind:value={cryptoGains} />
          </label>
          <label>
            {$t('taxcalc.page.priorLossesCarried')}
            <input type="number" step="0.01" bind:value={priorLossesCarried} />
          </label>
        </div>
      {/if}

      {#if context === 'investing' || profileHasWealthTax}
        <div class="grid">
          {#if context === 'investing'}
            <label>
              {$t('taxcalc.page.holdingPeriodDays')}
              <input type="number" step="1" min="0" placeholder={$t('taxcalc.page.unspecified')} bind:value={holdingDays} />
            </label>
          {/if}
          {#if profileHasWealthTax}
            <label>
              {$t('taxcalc.page.portfolioValueWealthTax')}
              <input type="number" step="0.01" bind:value={wealthTaxValue} />
            </label>
          {/if}
        </div>
        {#if context === 'investing'}
          <p class="muted small">{$t('taxcalc.page.holdingDaysHint')}</p>
        {/if}
      {/if}

      <div class="actions-row">
        <button class="primary" onclick={compute} disabled={computing}>
          {computing ? $t('taxcalc.page.computing') : $t('taxcalc.page.calculateTax')}
        </button>
        <button class="ghost" onclick={saveToHistory} disabled={saving}>
          {saving ? $t('common.saving') : `💾 ${$t('taxcalc.page.saveToHistory')}`}
        </button>
        {#if saveNote}<span class="note small">{saveNote}</span>{/if}
      </div>

      {#if result}
        <div class="result-head">
          <span>{$t('taxcalc.page.estimatedTax')}</span>
          <strong>{fmtMoney(result.total_tax, result.currency)}</strong>
          <span class="muted">{$t('taxcalc.page.effectiveRate', { rate: result.effective_rate_pct.toFixed(2) })}</span>
        </div>
        <ResultBreakdown {result} />
      {/if}

      {#if scenarios.length}
        <div class="history">
          <h2>{$t('taxcalc.page.history')}</h2>
          <div class="hist-list">
          {#each scenarios as s (s.id)}
            <div class="hist-row">
              <button class="hist-open" onclick={() => openScenario(s.id)}>
                <strong>{s.name}</strong>
                <span class="muted small">
                  {s.tax_year} · {s.context} · {s.mode}{s.result
                    ? ` · ${fmtMoney(s.result.total_tax, s.result.currency)}`
                    : ''}
                </span>
              </button>
              <button class="link red" onclick={() => removeScenario(s.id)}>{$t('taxcalc.page.del')}</button>
            </div>
          {/each}
          </div>
        </div>
      {/if}
    </section>
  </div>
</div>

<ProfileEditor bind:open={editorOpen} {editId} {templates} onsaved={loadProfiles} />
<LoadJournalModal
  bind:open={journalOpen}
  profileCurrency={selectedProfile?.currency || ''}
  onapply={applyJournal}
/>

<style>
  .page {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--space-6);
    gap: var(--space-4);
    overflow-y: auto;
  }
  header {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }
  h1 {
    font-size: 1.4rem;
    font-weight: 700;
  }
  h2 {
    font-size: 0.95rem;
    font-weight: 600;
  }
  .cols {
    display: grid;
    grid-template-columns: 280px 1fr;
    gap: var(--space-6);
    align-items: start;
  }
  /* Sticky sidebar: header stays pinned, the profile list scrolls on its own
     when it outgrows the viewport (no visible scrollbar). */
  .profiles {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    position: sticky;
    top: var(--space-4);
    max-height: calc(100vh - var(--space-8));
  }
  .aside-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex: none;
  }
  .profile-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow-y: auto;
    min-height: 0;
    scrollbar-width: none; /* Firefox */
    -ms-overflow-style: none; /* IE/Edge */
  }
  .profile-list::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  .profile {
    display: flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .profile.active {
    border-color: var(--accent);
  }
  .pick {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    background: transparent;
    border: none;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .actions {
    display: flex;
    gap: var(--space-1);
    padding-right: var(--space-2);
  }
  .calc {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
  }
  .grow {
    flex: 1;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: 0.8rem;
    color: var(--muted);
  }
  .toggles {
    display: flex;
    gap: var(--space-3);
  }
  .toggle {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .toggle button {
    background: var(--surface-2);
    border: none;
    color: var(--muted);
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    font-size: 0.85rem;
  }
  .toggle button.on {
    background: var(--accent);
    color: #fff;
  }
  .primary.sm {
    padding: var(--space-1) var(--space-3);
    font-size: 0.8rem;
  }
  .note {
    color: var(--accent);
  }
  .actions-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .actions-row .primary {
    align-self: auto;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .history {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-top: var(--space-4);
    padding-top: var(--space-4);
    border-top: 1px solid var(--border);
  }
  /* Saved scenarios scroll on their own once the list gets long (no visible bar). */
  .hist-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: 340px;
    overflow-y: auto;
    scrollbar-width: none; /* Firefox */
    -ms-overflow-style: none; /* IE/Edge */
  }
  .hist-list::-webkit-scrollbar {
    display: none; /* Chrome/Safari */
  }
  .hist-row {
    display: flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .hist-open {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: left;
    background: transparent;
    border: none;
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .hist-open:hover {
    background: var(--surface-2);
  }
  .hist-row .link {
    padding-right: var(--space-3);
  }
  .link {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.78rem;
  }
  .link:hover {
    color: var(--text);
  }
  .link.red:hover {
    color: var(--red);
  }
  .result-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    font-size: 1.1rem;
  }
  .result-head strong {
    font-size: 1.4rem;
    color: var(--text);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 0.78rem;
  }
  .err {
    color: var(--red);
  }
</style>
