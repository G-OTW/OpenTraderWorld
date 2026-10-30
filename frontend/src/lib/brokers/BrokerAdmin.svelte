<script>
  // Account broker admin: the one place broker accounts are created, credentialed and
  // granted. Master/detail, same shape as the data broker's, and deliberately a separate
  // list: a market-data key and an account key are not the same credential, and letting a
  // module read prices is not letting it read a book.
  //
  // Self-loading on purpose: the same component is the Settings section and the modal a
  // module opens, so hosts never fetch the list to show it. `onchanged` fires after every
  // mutation so a host can refresh its own picker.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import { modules as moduleRegistry } from '$lib/modules/registry.js';
  import PaperAccount from './PaperAccount.svelte';
  import RiskLimitsModal from './RiskLimitsModal.svelte';
  import LiveTrading from './LiveTrading.svelte';
  import BackgroundTrading from './BackgroundTrading.svelte';
  import { brokersApi, ALL_MODULES, TRADING, PAPER, trades, isReady } from './api.js';
  import { t } from '$lib/i18n';

  // `module` = the module the admin was opened from: new accounts are granted to it by
  // default.
  let { module = null, onchanged } = $props();

  let providers = $state([]);
  let accounts = $state([]);
  let moduleIds = $state([]);
  let loading = $state(true);
  let error = $state('');

  const moduleName = (id) => moduleRegistry.find((m) => m.id === id)?.name ?? id;

  async function load() {
    try {
      const [p, a, m] = await Promise.all([
        brokersApi.providers(),
        brokersApi.list(),
        brokersApi.modules()
      ]);
      providers = p;
      accounts = a;
      moduleIds = m;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    load();
  });

  async function reload() {
    accounts = await brokersApi.list();
    onchanged?.();
  }

  // ── Orders: metrics and kill switch ──
  // Metrics come from the order audit log (every call sent, refused or not), so they hold
  // across restarts. The kill switch cancels every working order of the account at the
  // broker, those placed elsewhere included.
  let stats = $state({});
  async function loadStats() {
    try {
      stats = Object.fromEntries((await brokersApi.orderStats()).map((r) => [r.account_id, r]));
    } catch {
      stats = {};
    }
  }
  $effect(() => {
    loadStats();
  });
  let killOpen = $state(false);
  let killing = $state(false);
  let killResult = $state(null);
  async function killSwitch() {
    if (!selected) return;
    killing = true;
    killResult = null;
    try {
      const r = await brokersApi.cancelAll(selected.id);
      killResult = { ok: true, detail: $t('brokers.orders.killed', { n: r.cancelled ?? 0 }) };
    } catch (e) {
      killResult = { ok: false, detail: e.message };
    } finally {
      killing = false;
      loadStats();
    }
  }
  const ms = (v) => (v == null ? '–' : `${Math.round(v)} ms`);

  // ── Risk limits: per trading account, checked before every order leaves ──
  let limitsOpen = $state(false);

  let selectedId = $state(null);
  const selected = $derived(accounts.find((a) => a.id === selectedId) ?? accounts[0] ?? null);

  /** Short label for an account's reach, shown under its name in the list. */
  function grantLabel(a) {
    const m = (a.modules ?? []).filter((x) => x !== TRADING);
    const reads = m.includes(ALL_MODULES)
      ? $t('brokers.grants.all')
      : m.length
        ? m.map(moduleName).join(' · ')
        : $t('brokers.grants.none');
    return trades(a) ? `${reads} · ${$t('brokers.trading.label')}` : reads;
  }

  /** What this broker can answer, as a line of facts rather than a table. */
  function reach(cap) {
    const out = [];
    if (cap.executions) out.push($t('brokers.reach.executions'));
    if (cap.positions) out.push($t('brokers.reach.positions'));
    if (cap.orders) out.push($t('brokers.reach.orders'));
    return out.join(' · ');
  }

  // ── Add account ──
  let adding = $state(false);
  let addBroker = $state('');
  let addName = $state('');
  let addModules = $state([ALL_MODULES]);
  let addVault = $state({});
  let addConfig = $state({});
  const addCap = $derived(providers.find((x) => x.broker === addBroker) ?? null);
  const brokerOpts = $derived(providers.map((p) => ({ value: p.broker, label: p.label })));

  function openAdd() {
    adding = true;
    addBroker = '';
    addName = '';
    addModules = module ? [module] : [ALL_MODULES];
    addVault = {};
    addConfig = {};
  }

  // A paper account opens filled in: its currency and balance are the placeholders until
  // changed, and nothing else is needed to start.
  function seedConfig(cap) {
    const out = {};
    for (const f of cap?.config_fields ?? []) out[f.name] = cap.broker === PAPER && f.required ? f.placeholder : '';
    return out;
  }
  /** A field's label, translated when the language pack names it, else the server's. */
  const fieldLabel = (f) => {
    const key = `brokers.field.${f.name}`;
    const tr = $t(key);
    return tr === key ? f.label : tr;
  };
  const missingConfig = (cap, values) =>
    (cap?.config_fields ?? []).some((f) => f.required && `${values?.[f.name] ?? ''}`.trim() === '');

  function onAddBrokerChange() {
    const p = providers.find((x) => x.broker === addBroker);
    if (p && !addName.trim()) addName = p.label;
    addConfig = seedConfig(p);
    // A paper account is made to be traded, and turning it on asks nothing: no money at risk.
    if (p?.broker === PAPER && !addModules.includes(TRADING)) addModules = [...addModules, TRADING];
  }

  async function createAccount() {
    error = '';
    if (!addBroker || !addName.trim()) return;
    try {
      const a = await brokersApi.create({
        broker: addBroker,
        name: addName.trim(),
        modules: addModules,
        config: addConfig
      });
      for (const [secretName, item] of Object.entries(addVault)) {
        if (item) await brokersApi.setSecret(a.id, secretName, '', item);
      }
      adding = false;
      selectedId = a.id;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Rename + settings (re-seeded whenever the selection changes) ──
  let nameDraft = $state('');
  let cfgDraft = $state({});
  $effect(() => {
    const a = selected;
    nameDraft = a?.name ?? '';
    cfgDraft = Object.fromEntries(
      (a?.capability?.config_fields ?? []).map((f) => [f.name, `${a?.config?.[f.name] ?? ''}`])
    );
    testResult = null;
  });

  async function saveConfig() {
    error = '';
    if (!selected) return;
    try {
      await brokersApi.update(selected.id, { config: cfgDraft });
      testResult = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // A failed test is an answer, not an error: the server returns { ok, detail } and the
  // detail names what to change, so it is shown in place rather than thrown.
  let testing = $state(false);
  let testResult = $state(null);
  async function runTest() {
    if (!selected) return;
    testing = true;
    testResult = null;
    try {
      testResult = await brokersApi.test(selected.id);
    } catch (e) {
      testResult = { ok: false, detail: e.message };
    } finally {
      testing = false;
    }
  }

  async function rename() {
    error = '';
    const nm = nameDraft.trim();
    if (!selected || !nm || nm === selected.name) return;
    try {
      await brokersApi.update(selected.id, { name: nm });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Module grants ──
  async function setGrants(a, next) {
    error = '';
    try {
      await brokersApi.update(a.id, { modules: next });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
  /** The next grant list after a click. Trading is its own switch: picking modules or the
   *  wildcard never turns it on or off. */
  function nextGrants(m, id) {
    const keep = m.includes(TRADING) ? [TRADING] : [];
    if (id === TRADING) return m.includes(TRADING) ? m.filter((x) => x !== TRADING) : [...m, TRADING];
    if (id === ALL_MODULES) return [ALL_MODULES, ...keep];
    const explicit = m.includes(ALL_MODULES) ? [] : m.filter((x) => x !== TRADING);
    const next = explicit.includes(id) ? explicit.filter((x) => x !== id) : [...explicit, id];
    return [...next, ...keep];
  }
  // Turning trading on is refused until the password is typed for that one change: the
  // API client raises the prompt and runs the request again (see $lib/reauth.svelte.js).
  function toggleGrant(a, id) {
    return setGrants(a, nextGrants(a.modules ?? [], id));
  }
  function toggleAddGrant(id) {
    addModules = nextGrants(addModules, id);
  }
  const granted = (list, id) => (list ?? []).includes(id);

  // ── Delete ──
  let confirmOpen = $state(false);
  async function removeAccount() {
    if (!selected) return;
    try {
      await brokersApi.remove(selected.id);
      selectedId = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Credentials: the picked vault item per "accountId:secretName" ──
  let vaultDrafts = $state({});
  async function saveSecret(id, name) {
    const key = `${id}:${name}`;
    const vaultItem = vaultDrafts[key];
    if (!vaultItem) return;
    try {
      await brokersApi.setSecret(id, name, '', vaultItem);
      vaultDrafts[key] = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
  const vaultRef = (a, name) => (a.secrets ?? []).find((s) => s.name === name)?.vault_item_id;
  async function clearSecret(id, name) {
    try {
      await brokersApi.deleteSecret(id, name);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
</script>

{#if loading}
  <Skeleton height="220px" />
{:else}
  <div class="split">
    <div class="left">
      <button
        class="addbtn"
        class:active={adding}
        onclick={() => (adding ? (adding = false) : openAdd())}
      >
        <Icon name="plus" size={14} /> {$t('brokers.add')}
      </button>
      <ul class="list">
        {#each accounts as a (a.id)}
          <li>
            <button
              class="row"
              class:active={selected?.id === a.id}
              onclick={() => (selectedId = a.id)}
            >
              <span class="dot" class:ok={isReady(a)}></span>
              <span class="names">
                <span class="nm">{a.name}</span>
                <span class="prov">{a.capability.label} · {grantLabel(a)}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>

    <div class="detail">
      {#if adding}
        <div class="addform">
          <h3>{$t('brokers.add')}</h3>
          <div class="fld">
            {$t('brokers.broker')}
            <Dropdown
              bind:value={addBroker}
              options={brokerOpts}
              placeholder={$t('brokers.selectPlaceholder')}
              ariaLabel={$t('brokers.broker')}
              onpick={onAddBrokerChange}
            />
          </div>
          <label class="fld">
            {$t('brokers.name')}
            <input bind:value={addName} placeholder={$t('brokers.namePlaceholder')} />
          </label>
          <div class="fld">
            {$t('brokers.grants.label')}
            <div class="chips">
              <button
                class="chip"
                class:on={granted(addModules, ALL_MODULES)}
                onclick={() => toggleAddGrant(ALL_MODULES)}>{$t('brokers.grants.all')}</button
              >
              {#each moduleIds as id (id)}
                <button
                  class="chip"
                  class:on={granted(addModules, id)}
                  onclick={() => toggleAddGrant(id)}
                >
                  {moduleName(id)}
                </button>
              {/each}
              <button
                class="chip trade"
                class:on={granted(addModules, TRADING)}
                title={$t('brokers.trading.hint')}
                onclick={() => toggleAddGrant(TRADING)}
              >
                <Icon name="lock" size={11} />
                {$t('brokers.trading.label')}
              </button>
            </div>
          </div>
          {#if granted(addModules, TRADING) && addBroker !== PAPER}
            <p class="experimental"><Icon name="alert-triangle" size={12} /> <span>{$t('brokers.trading.experimental')}</span></p>
          {/if}
          {#if addCap}
            <p class="readonly">
              <Icon name="eye" size={12} />
              <span>{addCap.key_note}</span>
            </p>
            {#if addCap.config_fields?.length}
              <div class="settings">
                {#each addCap.config_fields as f (f.name)}
                  <label class="fld">
                    <span class="lbl">
                      {fieldLabel(f)}{#if f.required}<span class="req">*</span>{/if}
                    </span>
                    <input
                      type={f.kind === 'number' ? 'number' : 'text'}
                      bind:value={addConfig[f.name]}
                      placeholder={f.placeholder}
                    />
                    {#if f.help}<span class="help">{f.help}</span>{/if}
                  </label>
                {/each}
              </div>
            {/if}
            {#if addCap.required_secrets?.length}
              <div class="secrets">
                {#each addCap.required_secrets as secretName (secretName)}
                  <div class="secret">
                    <span class="sname">{secretName}</span>
                    <div class="spicker">
                      <VaultPicker bind:vaultItemId={addVault[secretName]} hasSecret={false} />
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
            {#if addCap.rate_limit}<p class="rate">{addCap.rate_limit}</p>{/if}
          {/if}
          <div class="addactions">
            <Button variant="ghost" onclick={() => (adding = false)}>{$t('common.cancel')}</Button>
            <Button
              variant="primary"
              onclick={createAccount}
              disabled={!addBroker || !addName.trim() || missingConfig(addCap, addConfig)}
            >
              {$t('brokers.create')}
            </Button>
          </div>
        </div>
      {:else if !selected}
        <p class="note">{$t('brokers.none')}</p>
      {:else}
        {@const a = selected}
        {@const cap = a.capability}
        <div class="head">
          {#if cap.website}
            <a class="name" href={cap.website} target="_blank" rel="noreferrer">{a.name}</a>
          {:else}
            <span class="name">{a.name}</span>
          {/if}
          <span class="provlbl">{cap.label}</span>
          {#if cap.docs_url}
            <a
              class="docs"
              href={cap.docs_url}
              target="_blank"
              rel="noreferrer"
              title={$t('brokers.openDocs')}
            >
              {$t('brokers.apiDocs')} <Icon name="external-link" size={11} />
            </a>
          {/if}
        </div>
        <p class="reach">{reach(cap)}</p>
        <p class="readonly">
          <Icon name="eye" size={12} />
          <span>{cap.key_note}</span>
        </p>
        {#if cap.window_from_broker}
          <p class="rate">{$t('brokers.windowFromBroker')}</p>
        {/if}
        {#if cap.needs_symbols}
          <p class="rate">{$t('brokers.needsSymbols')}</p>
        {/if}
        {#if cap.history_days > 0 && !cap.window_from_broker}
          <p class="rate">{$t('brokers.historyDays', { days: cap.history_days })}</p>
        {/if}
        {#if cap.rate_limit}<p class="rate">{cap.rate_limit}</p>{/if}

        <div class="fldrow">
          <span class="sname">{$t('brokers.name')}</span>
          <input bind:value={nameDraft} />
          <Button onclick={rename} disabled={!nameDraft.trim() || nameDraft.trim() === a.name}>
            {$t('brokers.rename')}
          </Button>
        </div>

        <div class="fldrow grants">
          <span class="sname">{$t('brokers.grants.label')}</span>
          <div class="chips">
            <button
              class="chip"
              class:on={granted(a.modules, ALL_MODULES)}
              onclick={() => toggleGrant(a, ALL_MODULES)}
            >
              {$t('brokers.grants.all')}
            </button>
            {#each moduleIds as id (id)}
              <button
                class="chip"
                class:on={granted(a.modules, id) && !granted(a.modules, ALL_MODULES)}
                class:implied={granted(a.modules, ALL_MODULES)}
                onclick={() => toggleGrant(a, id)}
              >
                {moduleName(id)}
              </button>
            {/each}
            <button
              class="chip trade"
              class:on={trades(a)}
              aria-pressed={trades(a)}
              title={a.broker === PAPER ? $t('brokers.paper.tradingHint') : $t('brokers.trading.hint')}
              onclick={() => toggleGrant(a, TRADING)}
            >
              <Icon name="lock" size={11} />
              {$t('brokers.trading.label')}
            </button>
          </div>
        </div>

        {#if cap.config_fields?.length}
          <div class="settings box">
            {#each cap.config_fields as f (f.name)}
              <label class="fld">
                <span class="lbl">
                  {fieldLabel(f)}{#if f.required}<span class="req">*</span>{/if}
                </span>
                <input
                  type={f.kind === 'number' ? 'number' : 'text'}
                  bind:value={cfgDraft[f.name]}
                  placeholder={f.placeholder}
                />
                {#if f.help}<span class="help">{f.help}</span>{/if}
              </label>
            {/each}
            <div class="setactions">
              <Button onclick={saveConfig} disabled={missingConfig(cap, cfgDraft)}>
                {$t('common.save')}
              </Button>
            </div>
          </div>
        {/if}

        {#if cap.required_secrets.length}
          <p class="vaulthint">
            <Icon name="lock" size={12} />
            <span>
              {$t('brokers.vaultHint')}
              <a href="/settings#vault">{$t('brokers.vaultHintLink')}</a>
            </span>
          </p>
          <div class="secrets">
            {#each cap.required_secrets as name (name)}
              {@const isSet = a.set_secrets.includes(name)}
              <div class="secret">
                <span class="sname">{name}</span>
                <span class="state" class:set={isSet}>
                  {#if isSet && vaultRef(a, name)}<Icon name="lock" size={11} />
                    {$t('vault.picker.plugged')}{:else}{isSet
                      ? $t('brokers.set')
                      : $t('brokers.notSet')}{/if}
                </span>
                <div class="spicker">
                  <VaultPicker bind:vaultItemId={vaultDrafts[`${a.id}:${name}`]} hasSecret={isSet} />
                </div>
                <Button onclick={() => saveSecret(a.id, name)}>{$t('common.save')}</Button>
                {#if isSet}
                  <Button variant="danger" onclick={() => clearSecret(a.id, name)}>
                    {$t('common.clear')}
                  </Button>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if a.broker === PAPER}
          <PaperAccount account={a} onchanged={reload} />
        {/if}

        {#if cap.testable}
          <div class="setactions">
            <Button variant="ghost" onclick={runTest} disabled={testing}>
              {testing ? $t('brokers.testing') : $t('brokers.test')}
            </Button>
          </div>
          {#if testResult}
            <p class="testout" class:bad={!testResult.ok}>
              <Icon name={testResult.ok ? 'check' : 'alert-triangle'} size={12} />
              <span>{testResult.detail}</span>
            </p>
          {/if}
        {/if}

        {#if trades(a) && a.broker !== PAPER}
          <p class="experimental"><Icon name="alert-triangle" size={12} /> <span>{$t('brokers.trading.experimental')}</span></p>
        {/if}

        {#if trades(a) && a.broker !== PAPER && a.order_caps}
          <LiveTrading account={a} onchanged={reload} />
        {/if}

        {#if trades(a)}
          <BackgroundTrading account={a} onchanged={reload} />
        {/if}

        {#if trades(a) || stats[a.id]}
          {@const st = stats[a.id]}
          <div class="orders box">
            <span class="sname">{$t('brokers.orders.title')}</span>
            <dl class="metrics">
              <div><dt>{$t('brokers.orders.calls')}</dt><dd>{st?.calls ?? 0}</dd></div>
              <div><dt>{$t('brokers.orders.placed')}</dt><dd>{st?.placed ?? 0}</dd></div>
              <div><dt>{$t('brokers.orders.refused')}</dt><dd class:bad={(st?.refused ?? 0) > 0}>{st?.refused ?? 0}</dd></div>
              <div><dt>{$t('brokers.orders.latency')}</dt><dd>{ms(st?.latency_p50_ms)} / {ms(st?.latency_p99_ms)}</dd></div>
              <div><dt>{$t('brokers.orders.last')}</dt><dd>{st?.last_at ? new Date(st.last_at).toLocaleString() : '–'}</dd></div>
            </dl>
            <div class="setactions">
              {#if trades(a)}
                <Button icon="shield" onclick={() => (limitsOpen = true)}>{$t('brokers.risk.limits')}</Button>
              {/if}
              <Button variant="danger" icon="stop" onclick={() => (killOpen = true)} disabled={killing}>
                {killing ? $t('brokers.orders.killing') : $t('brokers.orders.kill')}
              </Button>
            </div>
            {#if killResult}
              <p class="testout" class:bad={!killResult.ok}>
                <Icon name={killResult.ok ? 'check' : 'alert-triangle'} size={12} />
                <span>{killResult.detail}</span>
              </p>
            {/if}
          </div>
        {/if}

        <div class="dangerzone">
          <Button variant="danger" icon="trash" onclick={() => (confirmOpen = true)}>
            {$t('brokers.delete')}
          </Button>
        </div>
      {/if}
      <ErrorText {error} copyable />
    </div>
  </div>
{/if}

<RiskLimitsModal bind:open={limitsOpen} account={selected} />

<ConfirmModal
  bind:open={killOpen}
  title={$t('brokers.orders.kill')}
  message={$t('brokers.orders.killMessage', { name: selected?.name ?? '' })}
  confirmLabel={$t('brokers.orders.kill')}
  danger
  onconfirm={killSwitch}
/>

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('brokers.delete')}
  message={$t('brokers.deleteMessage', { name: selected?.name ?? '' })}
  confirmLabel={$t('brokers.delete')}
  danger
  onconfirm={removeAccount}
/>

<style>
  .orders {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .metrics {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: 0;
  }
  .metrics dt {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .metrics dd {
    margin: 0;
    font-family: var(--mono);
  }
  .metrics dd.bad {
    color: var(--red);
  }
  /* minmax(0, 1fr): an auto-sized grid track refuses to shrink under its content, so
     the chip row and the vault pickers would push the detail column past the modal
     instead of wrapping inside it. */
  .split {
    display: grid;
    grid-template-columns: 250px minmax(0, 1fr);
    gap: var(--space-4);
    align-items: start;
  }
  .left {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-right: 1px solid var(--border);
    padding-right: var(--space-3);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    color: var(--text);
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.active {
    background: var(--surface-2);
    border-color: var(--accent);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--red);
    flex: none;
  }
  .dot.ok {
    background: var(--green);
  }
  .names {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .nm {
    font-weight: var(--fw-medium);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .prov {
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .addbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    width: 100%;
    background: transparent;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    color: var(--text);
    cursor: pointer;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    padding: var(--space-2) var(--space-3);
  }
  .addbtn:hover,
  .addbtn.active {
    background: var(--surface-2);
  }
  .detail {
    min-width: 0;
    min-height: 120px;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .name {
    font-weight: var(--fw-medium);
    font-size: var(--text-md);
    color: var(--text);
    text-decoration: none;
  }
  .provlbl {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .docs {
    font-size: var(--text-xs);
    color: var(--muted);
    text-decoration: none;
    margin-left: auto;
  }
  .reach {
    color: var(--text);
    font-size: var(--text-sm);
  }
  .rate {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.4;
    max-width: 60ch;
  }
  /* What the key is allowed to do: the one fact worth repeating everywhere an account is
     created, since the broker's own form defaults to a trading key. */
  .readonly {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
    max-width: 66ch;
  }
  .readonly :global(svg) {
    flex: none;
    transform: translateY(1px);
  }
  /* Order sending is new: said once, where trading is switched on. */
  .experimental {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--amber);
    border-radius: var(--radius);
    color: var(--amber);
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .experimental :global(svg) {
    flex: none;
    transform: translateY(2px);
  }
  .note {
    color: var(--muted);
    font-size: var(--text-base);
  }
  .vaulthint {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) 0 var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .vaulthint :global(svg) {
    flex: none;
    opacity: 0.7;
  }
  .vaulthint a {
    color: var(--accent);
    text-decoration: none;
  }
  .vaulthint a:hover {
    text-decoration: underline;
  }
  .settings {
    display: grid;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
  .settings.box {
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
  }
  .settings .lbl {
    display: block;
  }
  .settings .req {
    color: var(--red);
    margin-left: 2px;
  }
  .settings .help {
    font-size: 11px;
    color: var(--muted);
    line-height: 1.4;
  }
  .setactions {
    display: flex;
    gap: var(--space-2);
  }
  .testout {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--green);
  }
  .testout.bad {
    color: var(--amber);
  }
  .secrets {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .secret,
  .fldrow {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    min-width: 0;
  }
  .secret {
    align-items: flex-start;
  }
  .secret .state,
  .secret .sname,
  .secret :global(.btn) {
    margin-top: 4px;
  }
  .spicker {
    flex: 1;
    min-width: 0;
  }
  .sname {
    width: 110px;
    font-size: var(--text-base);
    color: var(--muted);
  }
  .state {
    width: 60px;
    font-size: var(--text-xs);
    color: var(--red);
  }
  .state.set {
    color: var(--green);
  }
  .fldrow input {
    flex: 1;
    max-width: 320px;
  }
  .grants {
    align-items: flex-start;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    min-width: 0;
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
  .chip.on {
    color: var(--text);
    border-color: var(--accent);
    background: var(--surface-2);
  }
  .chip.implied {
    color: var(--muted);
    border-style: dashed;
  }
  /* Trading is not one more reader: set apart from the modules, amber when on. */
  .chip.trade {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-left: var(--space-2);
  }
  .chip.trade.on {
    border-color: var(--amber);
    color: var(--amber);
  }
  .dangerzone {
    margin-top: var(--space-2);
  }
  .addform {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 480px;
  }
  .addform h3 {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .addactions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  @media (max-width: 720px) {
    .split {
      grid-template-columns: 1fr;
    }
    .left {
      border-right: none;
      padding-right: 0;
      border-bottom: 1px solid var(--border);
      padding-bottom: var(--space-3);
    }
  }
</style>
