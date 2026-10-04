<script>
  // Data broker admin — the one place connectors are created, credentialed and granted.
  //
  // A connector is a named instance of a provider: several instances of the same provider
  // can coexist, each with its own key, its own request limit, and its own set of modules
  // allowed to use it. Credentials are write-only: we only ever know which names are set.
  // Master/detail — left pane lists connectors, right pane edits the selected one.
  //
  // Self-loading on purpose: the same component is the /connectors page and the modal every
  // data module opens, so hosts never have to fetch the list to show it. `onchanged` fires
  // after every mutation so a host can refresh its *own* picker.
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import VaultPicker from '$lib/vault/VaultPicker.svelte';
  import UsagePie from '$lib/modules/histdata/UsagePie.svelte';
  import { modules as moduleRegistry } from '$lib/modules/registry.js';
  import { connectorsApi, ALL_MODULES, isReady } from './api.js';
  import { t } from '$lib/i18n';

  // `module` = the module the admin was opened from: new connectors are granted to it by
  // default, and its row is marked in the grant editor.
  let { module = null, onchanged } = $props();

  let providers = $state([]);
  let connectors = $state([]);
  let moduleIds = $state([]);
  let loading = $state(true);
  let error = $state('');

  const moduleName = (id) => moduleRegistry.find((m) => m.id === id)?.name ?? id;

  async function load() {
    try {
      const [p, c, m] = await Promise.all([
        connectorsApi.providers(),
        connectorsApi.list(),
        connectorsApi.modules()
      ]);
      providers = p;
      connectors = c;
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
    connectors = await connectorsApi.list();
    onchanged?.();
  }

  // Remember which connector was open across refresh (per-browser).
  const SEL_KEY = 'otw.connectors.selected.v1';
  let selectedId = $state(
    (() => {
      try {
        return localStorage.getItem(SEL_KEY);
      } catch {
        return null;
      }
    })()
  );
  $effect(() => {
    try {
      if (selectedId) localStorage.setItem(SEL_KEY, selectedId);
    } catch {
      /* non-fatal */
    }
  });
  const selected = $derived(connectors.find((c) => c.id === selectedId) ?? connectors[0] ?? null);

  /** Short label for a connector's reach, shown under its name in the list. */
  function grantLabel(c) {
    const m = c.modules ?? [];
    if (m.includes(ALL_MODULES)) return $t('connectors.grants.all');
    if (!m.length) return $t('connectors.grants.none');
    return m.map(moduleName).join(' · ');
  }

  // ── Add connector ──
  let adding = $state(false);
  let addProvider = $state('');
  let addName = $state('');
  let addModules = $state([ALL_MODULES]);
  let addTrack = $state(false);
  let addUnlimited = $state(false);
  let addMax = $state('');
  let addPeriod = $state('day');
  // Vault items picked for the provider's required secrets, keyed by secret name —
  // the keys are entered in the same form and saved right after creation.
  let addVault = $state({});
  // Non-secret provider settings (an IB Gateway host and port), keyed by field name.
  // Unlike the keys these are part of the connector row itself, so they go out with the
  // create call rather than in a second request.
  let addConfig = $state({});
  const addCap = $derived(providers.find((x) => x.provider === addProvider) ?? null);
  const providerOpts = $derived(
    providers.map((p) => ({ value: p.provider, label: `${p.label}${p.stream ? ' · live' : ''}` }))
  );
  const PERIODS = ['minute', 'hour', 'day', 'week', 'month'];
  const periodOpts = $derived(PERIODS.map((p) => ({ value: p, label: $t(`common.period.${p}`) })));

  function openAdd() {
    adding = true;
    addProvider = '';
    addName = '';
    // Opened from a module: grant that module only. Opened from the page: grant everything.
    addModules = module ? [module] : [ALL_MODULES];
    addTrack = false;
    addUnlimited = false;
    addMax = '';
    addVault = {};
    addConfig = {};
  }

  /** Seed the settings with the provider's placeholders, so the common case is one click. */
  function seedConfig(cap) {
    const out = {};
    for (const f of cap?.config_fields ?? []) out[f.name] = f.placeholder ?? '';
    return out;
  }
  /** A field's label, translated when the language pack names it, else the server's. */
  const fieldLabel = (f) => {
    const key = `connectors.field.${f.name}`;
    const tr = $t(key);
    return tr === key ? f.label : tr;
  };
  const missingConfig = (cap, values) =>
    (cap?.config_fields ?? []).some(
      (f) => f.required && `${values?.[f.name] ?? ''}`.trim() === ''
    );

  // Prefill the name with the provider label (edit freely afterwards).
  function onAddProviderChange() {
    const p = providers.find((x) => x.provider === addProvider);
    if (p && !addName.trim()) addName = p.label;
    addConfig = seedConfig(p);
  }
  function limitBody(track, unlimited, max, period) {
    if (!track) return { enabled: false, max_requests: null, period: 'day' };
    return { enabled: true, max_requests: unlimited ? null : Number(max), period };
  }
  async function createConnector() {
    error = '';
    if (!addProvider || !addName.trim()) return;
    if (addTrack && !addUnlimited && !(Number(addMax) >= 1)) {
      error = $t('connectors.errMax');
      return;
    }
    try {
      const c = await connectorsApi.create({
        provider: addProvider,
        name: addName.trim(),
        modules: addModules,
        config: addConfig,
        limit: limitBody(addTrack, addUnlimited, addMax, addPeriod)
      });
      // Keys picked in the form land on the fresh connector in the same flow.
      for (const [secretName, item] of Object.entries(addVault)) {
        if (item) await connectorsApi.setSecret(c.id, secretName, '', item);
      }
      adding = false;
      selectedId = c.id;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Rename + settings + limit editing (re-seeded whenever the selection changes) ──
  let nameDraft = $state('');
  let cfgDraft = $state({});
  let limTrack = $state(false);
  let limUnlimited = $state(false);
  let limMax = $state('');
  let limPeriod = $state('day');
  $effect(() => {
    const c = selected;
    nameDraft = c?.name ?? '';
    cfgDraft = Object.fromEntries(
      (c?.config_fields ?? []).map((f) => [f.name, `${c?.config?.[f.name] ?? ''}`])
    );
    limTrack = !!c?.quota;
    limUnlimited = !!c?.quota && c.quota.max_requests == null;
    limMax = c?.quota?.max_requests ?? '';
    limPeriod = c?.quota?.period ?? 'day';
    testResult = null;
  });

  async function saveConfig() {
    error = '';
    if (!selected) return;
    try {
      await connectorsApi.update(selected.id, { config: cfgDraft });
      testResult = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Connection test ──
  // A failed test is an answer, not an error: the server returns { ok, detail } and the
  // detail names the setting to change, so it is shown in place rather than thrown.
  let testing = $state(false);
  let testResult = $state(null);
  async function runTest() {
    if (!selected) return;
    testing = true;
    testResult = null;
    try {
      testResult = await connectorsApi.test(selected.id);
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
      await connectorsApi.update(selected.id, { name: nm });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
  async function saveLimit() {
    error = '';
    if (!selected) return;
    if (limTrack && !limUnlimited && !(Number(limMax) >= 1)) {
      error = $t('connectors.errMax');
      return;
    }
    try {
      await connectorsApi.update(selected.id, {
        limit: limitBody(limTrack, limUnlimited, limMax, limPeriod)
      });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Module grants ──
  // "All modules" is exclusive: picking it drops the explicit list, picking a module
  // drops the wildcard. Saved immediately — a grant toggle is not a form.
  async function setGrants(c, next) {
    error = '';
    try {
      await connectorsApi.update(c.id, { modules: next });
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
  function toggleGrant(c, id) {
    const m = c.modules ?? [];
    if (id === ALL_MODULES) return setGrants(c, [ALL_MODULES]);
    const explicit = m.includes(ALL_MODULES) ? [] : m;
    const next = explicit.includes(id) ? explicit.filter((x) => x !== id) : [...explicit, id];
    return setGrants(c, next);
  }
  function toggleAddGrant(id) {
    if (id === ALL_MODULES) {
      addModules = [ALL_MODULES];
      return;
    }
    const explicit = addModules.includes(ALL_MODULES) ? [] : addModules;
    addModules = explicit.includes(id) ? explicit.filter((x) => x !== id) : [...explicit, id];
  }
  const granted = (list, id) => (list ?? []).includes(id);

  // ── Delete ──
  let confirmOpen = $state(false);
  async function removeConnector() {
    if (!selected) return;
    try {
      await connectorsApi.remove(selected.id);
      selectedId = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  // ── Secrets: the picked vault item per "connectorId:secretName" ──
  // Entries are created lazily on first pick, so a key can be missing at render time.
  // VaultPicker's bindable prop therefore declares no fallback (binding `undefined` to a
  // prop that has one throws props_invalid_value); we normalize with ?? when reading.
  let vaultDrafts = $state({});
  async function saveSecret(id, name) {
    const key = `${id}:${name}`;
    const vaultItem = vaultDrafts[key];
    if (!vaultItem) return;
    try {
      await connectorsApi.setSecret(id, name, '', vaultItem);
      vaultDrafts[key] = null;
      await reload();
    } catch (e) {
      error = e.message;
    }
  }
  // Vault reference already stored for a secret, if any (drives the "plugged" badge).
  const vaultRef = (c, name) => (c.secrets ?? []).find((s) => s.name === name)?.vault_item_id;
  async function clearSecret(id, name) {
    try {
      await connectorsApi.deleteSecret(id, name);
      await reload();
    } catch (e) {
      error = e.message;
    }
  }

  function quotaLine(c) {
    if (!c.quota) return '';
    const period = $t(`common.period.${c.quota.period}`);
    const max = c.quota.max_requests == null ? '∞' : c.quota.max_requests;
    return `${c.quota.used} / ${max} · ${period}`;
  }
  function resetsAt(c) {
    if (!c.quota?.resets_at) return '';
    try {
      return new Date(c.quota.resets_at).toLocaleString(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short'
      });
    } catch {
      return '';
    }
  }
</script>

{#if loading}
  <Skeleton height="220px" />
{:else}
  <div class="split">
    <div class="left">
      <button class="addbtn" class:active={adding} onclick={() => (adding ? (adding = false) : openAdd())}>
        <Icon name="plus" size={14} /> {$t('connectors.add')}
      </button>
      <ul class="list">
        {#each connectors as c (c.id)}
          <li>
            <button class="row" class:active={selected?.id === c.id} onclick={() => (selectedId = c.id)}>
              {#if c.quota && c.quota.max_requests != null}
                <UsagePie used={c.quota.used} max={c.quota.max_requests} title={quotaLine(c)} />
              {:else}
                <span class="dot" class:ok={isReady(c)}></span>
              {/if}
              <span class="names">
                <span class="nm">{c.name}</span>
                <span class="prov">{c.label} · {grantLabel(c)}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>

    <div class="detail">
      {#if adding}
        <div class="addform">
          <h3>{$t('connectors.add')}</h3>
          <div class="fld">
            {$t('connectors.provider')}
            <Dropdown
              bind:value={addProvider}
              options={providerOpts}
              placeholder={$t('histdata.download.selectPlaceholder')}
              ariaLabel={$t('connectors.provider')}
              onpick={onAddProviderChange}
            />
          </div>
          <label class="fld">
            {$t('connectors.name')}
            <input bind:value={addName} placeholder={$t('connectors.namePlaceholder')} />
          </label>
          <div class="fld">
            {$t('connectors.grants.label')}
            <div class="chips">
              <button
                class="chip"
                class:on={granted(addModules, ALL_MODULES)}
                onclick={() => toggleAddGrant(ALL_MODULES)}>{$t('connectors.grants.all')}</button
              >
              {#each moduleIds as id (id)}
                <button class="chip" class:on={granted(addModules, id)} onclick={() => toggleAddGrant(id)}>
                  {moduleName(id)}
                </button>
              {/each}
            </div>
          </div>
          {#if addCap}
            {#if addCap.config_fields?.length}
              <!-- Settings, not credentials: an address is not a secret and has to stay
                   readable, otherwise a failed connection cannot be diagnosed. -->
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
              <!-- Everything the provider needs, right in the form: pick each key from the
                   vault (or create one inline); saved onto the connector at creation. -->
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
            {:else}
              <p class="note">{$t('connectors.noCredentials')}</p>
            {/if}
            {#if addCap.rate_limit}<p class="rate">{addCap.rate_limit}</p>{/if}
            <!-- Live is sold separately from history by most vendors, so what it needs is
                 said where the account is created rather than discovered on a dead chart. -->
            {#if addCap.stream_note}
              <p class="rate live"><Icon name="radio" size={11} /> {addCap.stream_note}</p>
            {/if}
          {/if}
          <div class="limit">
            <label class="chk">
              <input type="checkbox" bind:checked={addTrack} />
              {$t('connectors.trackUsage')}
            </label>
            {#if addTrack}
              <label class="chk">
                <input type="checkbox" bind:checked={addUnlimited} />
                {$t('connectors.unlimited')}
              </label>
              {#if !addUnlimited}
                <input class="num" type="number" min="1" bind:value={addMax} placeholder="500" />
              {/if}
              <span class="per">{$t('connectors.per')}</span>
              <div class="pick">
                <Dropdown bind:value={addPeriod} options={periodOpts} ariaLabel={$t('connectors.per')} />
              </div>
            {/if}
          </div>
          <div class="addactions">
            <Button variant="ghost" onclick={() => (adding = false)}>{$t('common.cancel')}</Button>
            <Button
              variant="primary"
              onclick={createConnector}
              disabled={!addProvider || !addName.trim() || missingConfig(addCap, addConfig)}
            >
              {$t('connectors.create')}
            </Button>
          </div>
        </div>
      {:else if !selected}
        <p class="note">{$t('connectors.none')}</p>
      {:else}
        {@const c = selected}
        <div class="head">
          <a class="name" href={c.website} target="_blank" rel="noreferrer">{c.name}</a>
          <span class="provlbl">{c.label}</span>
          {#if !c.required_secrets.length}<span class="tag free">{$t('connectors.keyless')}</span>{/if}
          {#if c.stream}<span class="tag live">{$t('connectors.live')}</span>{/if}
          {#if c.docs_url}
            <a class="docs" href={c.docs_url} target="_blank" rel="noreferrer" title={$t('connectors.openDocs')}>
              {$t('connectors.apiDocs')} <Icon name="external-link" size={11} />
            </a>
          {/if}
        </div>
        {#if c.rate_limit}<p class="rate">{c.rate_limit}</p>{/if}
        {#if c.stream_note}
          <p class="rate live"><Icon name="radio" size={11} /> {c.stream_note}</p>
        {/if}

        <div class="fldrow">
          <span class="sname">{$t('connectors.name')}</span>
          <input bind:value={nameDraft} />
          <Button onclick={rename} disabled={!nameDraft.trim() || nameDraft.trim() === c.name}>
            {$t('connectors.rename')}
          </Button>
        </div>

        <div class="fldrow grants">
          <span class="sname">{$t('connectors.grants.label')}</span>
          <div class="chips">
            <button class="chip" class:on={granted(c.modules, ALL_MODULES)} onclick={() => toggleGrant(c, ALL_MODULES)}>
              {$t('connectors.grants.all')}
            </button>
            {#each moduleIds as id (id)}
              <button
                class="chip"
                class:on={granted(c.modules, id) && !granted(c.modules, ALL_MODULES)}
                class:implied={granted(c.modules, ALL_MODULES)}
                onclick={() => toggleGrant(c, id)}
              >
                {moduleName(id)}
              </button>
            {/each}
          </div>
        </div>

        {#if c.config_fields?.length}
          <div class="settings box">
            {#each c.config_fields as f (f.name)}
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
              <Button onclick={saveConfig} disabled={missingConfig(c, cfgDraft)}>
                {$t('common.save')}
              </Button>
              {#if c.testable}
                <Button variant="ghost" onclick={runTest} disabled={testing}>
                  {testing ? $t('connectors.testing') : $t('connectors.test')}
                </Button>
              {/if}
            </div>
            {#if testResult}
              <p class="testout" class:bad={!testResult.ok}>
                <Icon name={testResult.ok ? 'check' : 'alert-triangle'} size={12} />
                <span>{testResult.detail}</span>
              </p>
            {/if}
          </div>
        {/if}

        {#if c.required_secrets.length}
          <!-- Keys live in the central vault; "From vault" below plugs one in per secret. -->
          <p class="vaulthint">
            <Icon name="lock" size={12} />
            <span>
              {$t('connectors.vaultHint')}
              <a href="/settings#vault">{$t('connectors.vaultHintLink')}</a>
            </span>
          </p>
          <div class="secrets">
            {#each c.required_secrets as name (name)}
              {@const isSet = c.set_secrets.includes(name)}
              <div class="secret">
                <span class="sname">{name}</span>
                <span class="state" class:set={isSet}>
                  {#if isSet && vaultRef(c, name)}<Icon name="lock" size={11} /> {$t('vault.picker.plugged')}{:else}{isSet ? $t('connectors.set') : $t('connectors.notSet')}{/if}
                </span>
                <div class="spicker">
                  <VaultPicker bind:vaultItemId={vaultDrafts[`${c.id}:${name}`]} hasSecret={isSet} />
                </div>
                <Button onclick={() => saveSecret(c.id, name)}>{$t('common.save')}</Button>
                {#if isSet}
                  <Button variant="danger" onclick={() => clearSecret(c.id, name)}>{$t('common.clear')}</Button>
                {/if}
              </div>
            {/each}
          </div>
        {:else}
          <p class="note">{$t('connectors.noCredentials')}</p>
        {/if}

        <div class="limitbox">
          <div class="limithead">
            <span>{$t('connectors.limitTitle')}</span>
            {#if c.quota}
              <span class="usage">
                {#if c.quota.max_requests != null}
                  <UsagePie used={c.quota.used} max={c.quota.max_requests} title={quotaLine(c)} />
                {/if}
                {quotaLine(c)}
                <span class="resets">· {$t('connectors.resets', { date: resetsAt(c) })}</span>
              </span>
            {/if}
          </div>
          <div class="limit">
            <label class="chk">
              <input type="checkbox" bind:checked={limTrack} />
              {$t('connectors.trackUsage')}
            </label>
            {#if limTrack}
              <label class="chk">
                <input type="checkbox" bind:checked={limUnlimited} />
                {$t('connectors.unlimited')}
              </label>
              {#if !limUnlimited}
                <input class="num" type="number" min="1" bind:value={limMax} placeholder="500" />
              {/if}
              <span class="per">{$t('connectors.per')}</span>
              <div class="pick">
                <Dropdown bind:value={limPeriod} options={periodOpts} ariaLabel={$t('connectors.per')} />
              </div>
            {/if}
            <Button onclick={saveLimit}>{$t('common.save')}</Button>
          </div>
        </div>

        <div class="dangerzone">
          <Button variant="danger" icon="trash" onclick={() => (confirmOpen = true)}>
            {$t('connectors.delete')}
          </Button>
        </div>
      {/if}
      <ErrorText error={error} copyable />
    </div>
  </div>
{/if}

<ConfirmModal
  bind:open={confirmOpen}
  title={$t('connectors.delete')}
  message={$t('connectors.deleteMessage', { name: selected?.name ?? '' })}
  confirmLabel={$t('connectors.delete')}
  danger
  onconfirm={removeConnector}
/>

<style>
  .split {
    display: grid;
    grid-template-columns: 250px 1fr;
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
  .tag {
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: var(--radius);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  /* Text-only colored tags — no filled pill. */
  .free {
    color: var(--green);
  }
  .live {
    color: var(--accent);
  }
  .docs {
    font-size: var(--text-xs);
    color: var(--muted);
    text-decoration: none;
    margin-left: auto;
  }
  .rate {
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.4;
    max-width: 60ch;
  }
  /* What live costs, next to what history costs: the same kind of fact, told apart by
     the glyph rather than by colour. */
  .rate.live {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .rate.live :global(svg) {
    flex: none;
    transform: translateY(1px);
  }
  .note {
    color: var(--muted);
    font-size: var(--text-base);
  }
  /* Quiet inline hint above the credential rows — informative, not an alert. */
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
  /* `.fld` is a flex column, so the label text and the required marker were two stacked
     items; one inline box keeps the star beside the word it qualifies. */
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
  }
  /* The vault picker stacks a mode row above the input; top-align its row. */
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
  }
  /* Grant toggles: quiet by default, accent-outlined when the module is allowed.
     `implied` renders the modules a wildcard grant covers without claiming they were
     picked one by one. */
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
  .limitbox {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: 560px;
  }
  .limithead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .usage {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-weight: var(--fw-normal);
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .resets {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .limit {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
  }
  .chk input {
    margin: 0;
  }
  .num {
    width: 90px;
  }
  .per {
    color: var(--muted);
  }
  /* The Dropdown trigger fills its parent, so an inline one needs a width to size to. */
  .pick {
    width: 120px;
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
